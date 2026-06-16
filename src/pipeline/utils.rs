use std::io;

use super::{ 
    PipelineError,
    Result
};


pub fn check_direct_access(path: &str, block_size: usize, seek: &Option<usize>) -> Result<usize> {
    let sector_size = get_sector_size(path)
        .map_err(|e| PipelineError::Hardware(e.to_string()))? as usize;

    if !block_size.is_multiple_of(sector_size) {
        return Err(PipelineError::Mismatch(format!("block size={block_size}"), format!("sector size={sector_size}")));
    }

    if let Some(skip_bytes) = seek && skip_bytes % sector_size != 0 {
        return Err(PipelineError::Mismatch(format!("skip={skip_bytes}"), format!("sector size={sector_size}")));
    }

    Ok(sector_size)
}

/// Returns the logical sector size (or optimal block size) for a given path.
/// Supports Linux, macOS, and Windows.
fn get_sector_size(path: &str) -> io::Result<u32> {
    #[cfg(target_os = "linux")]
    {
        linux_sector_size(path)
    }

    #[cfg(target_os = "macos")]
    {
        macos_sector_size(path)
    }

    #[cfg(target_os = "windows")]
    {
        windows_sector_size(path)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Sector size detection is only implemented for Linux, macOS, and Windows.",
        ))
    }
}

#[cfg(target_os = "linux")]
fn linux_sector_size(path: &str) -> io::Result<u32> {
    use std::fs::File;
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    use std::os::unix::io::AsRawFd;

    let meta = std::fs::metadata(path)?;

    if meta.file_type().is_block_device() {
        let file = File::open(path)?;
        let mut size: libc::c_int = 0;
        
        // BLKSSZGET
        const BLKSSZGET: libc::c_ulong = 0x1268;

        let res = unsafe { libc::ioctl(file.as_raw_fd(), BLKSSZGET, &mut size) };
        if res < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(size as u32)
    } else {
        Ok(meta.blksize() as u32)
    }
}

#[cfg(target_os = "macos")]
fn macos_sector_size(path: &str) -> io::Result<u32> {
    use std::fs::File;
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    use std::os::unix::io::AsRawFd;

    let meta = std::fs::metadata(path)?;

    // macOS has both block devices (/dev/disk) and raw character devices (/dev/rdisk).
    // For dd-like cloning, you usually use the raw char device.
    if meta.file_type().is_block_device() || meta.file_type().is_char_device() {
        let file = File::open(path)?;
        let mut size: u32 = 0;
        
        // macOS ioctl magic number for DKIOCGETBLOCKSIZE
        // Equivalent to _IOR('d', 24, uint32_t)
        const DKIOCGETBLOCKSIZE: libc::c_ulong = 0x40046418;

        let res = unsafe { libc::ioctl(file.as_raw_fd(), DKIOCGETBLOCKSIZE, &mut size) };
        if res < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(size)
    } else {
        Ok(meta.blksize() as u32)
    }
}

#[cfg(target_os = "windows")]
fn windows_sector_size(path: &str) -> io::Result<u32> {
    use std::ffi::OsStr;
    use std::os::raw::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;

    #[repr(C)]
    struct DISK_GEOMETRY {
        cylinders: i64,
        media_type: u32,
        tracks_per_cylinder: u32,
        sectors_per_track: u32,
        bytes_per_sector: u32,
    }

    extern "system" {
        fn DeviceIoControl(
            hDevice: *mut c_void,
            dwIoControlCode: u32,
            lpInBuffer: *mut c_void,
            nInBufferSize: u32,
            lpOutBuffer: *mut c_void,
            nOutBufferSize: u32,
            lpBytesReturned: *mut u32,
            lpOverlapped: *mut c_void,
        ) -> i32;

        fn GetVolumePathNameW(
            lpszFileName: *const u16,
            lpszVolumePathName: *mut u16,
            cchBufferLength: u32,
        ) -> i32;

        fn GetDiskFreeSpaceW(
            lpRootPathName: *const u16,
            lpSectorsPerCluster: *mut u32,
            lpBytesPerSector: *mut u32,
            lpNumberOfFreeClusters: *mut u32,
            lpTotalNumberOfClusters: *mut u32,
        ) -> i32;
    }

    let path_str = path.to_string_lossy();

    // Raw Physical Drive
    if path_str.starts_with("\\\\.\\") {
        let file = std::fs::File::open(path)?;
        let handle = file.as_raw_handle() as *mut c_void;
        
        let mut geometry = DISK_GEOMETRY {
            cylinders: 0,
            media_type: 0,
            tracks_per_cylinder: 0,
            sectors_per_track: 0,
            bytes_per_sector: 0,
        };
        let mut bytes_returned = 0;
        
        const IOCTL_DISK_GET_DRIVE_GEOMETRY: u32 = 0x70000;

        let res = unsafe {
            DeviceIoControl(
                handle,
                IOCTL_DISK_GET_DRIVE_GEOMETRY,
                std::ptr::null_mut(),
                0,
                &mut geometry as *mut _ as *mut c_void,
                std::mem::size_of::<DISK_GEOMETRY>() as u32,
                &mut bytes_returned,
                std::ptr::null_mut(),
            )
        };

        if res == 0 {
            return Err(io::Error::last_os_error());
        }
        return Ok(geometry.bytes_per_sector);
    }

    // Standard File
    let mut path_wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    path_wide.push(0); 

    let mut root_path = vec![0u16; 260]; 
    let res = unsafe {
        GetVolumePathNameW(
            path_wide.as_ptr(),
            root_path.as_mut_ptr(),
            root_path.len() as u32,
        )
    };
    
    if res == 0 {
        return Err(io::Error::last_os_error());
    }

    let mut sectors_per_cluster = 0;
    let mut bytes_per_sector = 0;
    let mut free_clusters = 0;
    let mut total_clusters = 0;

    let res = unsafe {
        GetDiskFreeSpaceW(
            root_path.as_ptr(),
            &mut sectors_per_cluster,
            &mut bytes_per_sector,
            &mut free_clusters,
            &mut total_clusters,
        )
    };

    if res == 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(bytes_per_sector)
}