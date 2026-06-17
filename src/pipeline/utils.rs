//! Pipeline utilities for direct I/O validation and hardware sector size detection.
//!
//! This module provides helpers that enforce the alignment requirements imposed
//! by direct (unbuffered) I/O on all supported platforms. When `iflag=direct` or
//! `oflag=direct` is active, both the block size and any seek offset must be
//! exact multiples of the underlying storage sector size; these helpers detect
//! and report mismatches before any I/O begins.

use std::io;

use super::{PipelineError, Result};

/// Validates that `block_size` and (optionally) a seek offset are aligned to the
/// sector size of the device or filesystem at `path`.
///
/// This must be called before opening a file with `O_DIRECT` / `FILE_FLAG_NO_BUFFERING`
/// because the kernel requires all I/O buffers and offsets to be sector-aligned.
///
/// # Returns
///
/// The sector size in bytes on success, so callers can use it for buffer alignment.
///
/// # Errors
///
/// - [`PipelineError::Hardware`] — if the sector size cannot be determined.
/// - [`PipelineError::Mismatch`] — if `block_size` or the seek offset is not a
///   multiple of the sector size.
pub fn check_direct_access(path: &str, block_size: usize, seek: &Option<usize>) -> Result<usize> {
    let sector_size =
        get_sector_size(path).map_err(|e| PipelineError::Hardware(e.to_string()))? as usize;

    if !block_size.is_multiple_of(sector_size) {
        return Err(PipelineError::Mismatch(
            format!("block size={block_size}"),
            format!("sector size={sector_size}"),
        ));
    }

    if let Some(skip_bytes) = seek
        && skip_bytes % sector_size != 0
    {
        return Err(PipelineError::Mismatch(
            format!("skip={skip_bytes}"),
            format!("sector size={sector_size}"),
        ));
    }

    Ok(sector_size)
}

/// Returns the logical sector size (in bytes) for the device or filesystem at `path`.
///
/// Dispatches to a platform-specific implementation. On Linux and macOS the result
/// comes from an `ioctl` on block/character devices, or from filesystem metadata
/// for regular files. On Windows it uses `IOCTL_DISK_GET_DRIVE_GEOMETRY` for raw
/// device paths and `GetDiskFreeSpaceW` as a fallback for ordinary file paths.
///
/// # Errors
///
/// Returns an [`io::Error`] if the platform implementation cannot determine the
/// sector size, or if the current platform is unsupported.
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

/// Returns the logical sector size for `path` on Linux.
///
/// For block devices (e.g. `/dev/sda`) the kernel's `BLKSSZGET` ioctl is used.
/// For regular files and other paths, the filesystem block size from
/// [`std::fs::Metadata::blksize`] is returned instead.
///
/// # Errors
///
/// Returns an [`io::Error`] if `stat` or the ioctl fails.
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

/// Returns the logical sector size for `path` on Windows.
///
/// First attempts to open `path` as a raw device handle (suitable for paths like
/// `\\.\PhysicalDrive0` or `\\.\C:`) and queries geometry via
/// `IOCTL_DISK_GET_DRIVE_GEOMETRY`. If the handle cannot be opened or the ioctl
/// fails, falls back to `GetDiskFreeSpaceW` using the drive root derived from
/// `path` (e.g. `C:\` for any path starting with a drive letter).
///
/// # Errors
///
/// Returns an [`io::Error`] if both the ioctl path and the `GetDiskFreeSpaceW`
/// fallback fail.
#[cfg(target_os = "windows")]
fn windows_sector_size(path: &str) -> io::Result<u32> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    type Handle = *mut core::ffi::c_void;
    type Dword = u32;
    type Bool = i32;

    const INVALID_HANDLE_VALUE: Handle = (!0usize) as Handle;
    const GENERIC_READ: Dword = 0x80000000;
    const FILE_SHARE_READ: Dword = 0x00000001;
    const FILE_SHARE_WRITE: Dword = 0x00000002;
    const OPEN_EXISTING: Dword = 3;
    // CTL_CODE(IOCTL_DISK_BASE=7, 0, METHOD_BUFFERED=0, FILE_ANY_ACCESS=0)
    const IOCTL_DISK_GET_DRIVE_GEOMETRY: Dword = 0x00070000;

    #[repr(C)]
    struct DiskGeometry {
        cylinders: i64,
        media_type: u32,
        tracks_per_cylinder: u32,
        sectors_per_track: u32,
        bytes_per_sector: u32,
    }

    unsafe extern "system" {
        fn CreateFileW(
            lp_file_name: *const u16,
            dw_desired_access: Dword,
            dw_share_mode: Dword,
            lp_security_attributes: *mut core::ffi::c_void,
            dw_creation_disposition: Dword,
            dw_flags_and_attributes: Dword,
            h_template_file: Handle,
        ) -> Handle;

        fn CloseHandle(h_object: Handle) -> Bool;

        fn DeviceIoControl(
            h_device: Handle,
            dw_io_control_code: Dword,
            lp_in_buffer: *mut core::ffi::c_void,
            n_in_buffer_size: Dword,
            lp_out_buffer: *mut core::ffi::c_void,
            n_out_buffer_size: Dword,
            lp_bytes_returned: *mut Dword,
            lp_overlapped: *mut core::ffi::c_void,
        ) -> Bool;

        fn GetDiskFreeSpaceW(
            lp_root_path_name: *const u16,
            lp_sectors_per_cluster: *mut Dword,
            lp_bytes_per_sector: *mut Dword,
            lp_number_of_free_clusters: *mut Dword,
            lp_total_number_of_clusters: *mut Dword,
        ) -> Bool;
    }

    let wide: Vec<u16> = OsStr::new(path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // Try opening as a device (e.g. \\.\PhysicalDrive0, \\.\C:)
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null_mut(),
            OPEN_EXISTING,
            0,
            ptr::null_mut(),
        )
    };

    if handle != INVALID_HANDLE_VALUE {
        let mut geom = DiskGeometry {
            cylinders: 0,
            media_type: 0,
            tracks_per_cylinder: 0,
            sectors_per_track: 0,
            bytes_per_sector: 0,
        };
        let mut bytes_returned: Dword = 0;

        let ok = unsafe {
            DeviceIoControl(
                handle,
                IOCTL_DISK_GET_DRIVE_GEOMETRY,
                ptr::null_mut(),
                0,
                &mut geom as *mut _ as *mut core::ffi::c_void,
                std::mem::size_of::<DiskGeometry>() as Dword,
                &mut bytes_returned,
                ptr::null_mut(),
            )
        };
        unsafe { CloseHandle(handle) };

        if ok != 0 {
            return Ok(geom.bytes_per_sector);
        }
    }

    // Fall back to GetDiskFreeSpaceW for regular file paths (e.g. "C:\path\to\file")
    let root = if path.len() >= 2 && path.chars().nth(1) == Some(':') {
        format!("{}:\\", &path[..1])
    } else {
        path.to_string()
    };

    let root_wide: Vec<u16> = OsStr::new(&root)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut sectors_per_cluster: Dword = 0;
    let mut bytes_per_sector: Dword = 0;
    let mut free_clusters: Dword = 0;
    let mut total_clusters: Dword = 0;

    let ok = unsafe {
        GetDiskFreeSpaceW(
            root_wide.as_ptr(),
            &mut sectors_per_cluster,
            &mut bytes_per_sector,
            &mut free_clusters,
            &mut total_clusters,
        )
    };

    if ok != 0 {
        Ok(bytes_per_sector)
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Returns the logical sector size for `path` on macOS.
///
/// For block devices (`/dev/diskN`) and raw character devices (`/dev/rdiskN`)
/// the `DKIOCGETBLOCKSIZE` ioctl is issued. For regular files the filesystem
/// block size from [`std::fs::Metadata::blksize`] is returned instead.
///
/// # Errors
///
/// Returns an [`io::Error`] if `stat` or the ioctl fails.
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
