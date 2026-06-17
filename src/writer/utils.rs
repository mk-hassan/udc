//! Utilities for configuring output file options and platform-specific I/O flags.
//!
//! This module translates `oflag` values into `OpenOptions` custom flags on the
//! current platform and applies macOS-specific direct I/O configuration when
//! requested.

use std::fs::OpenOptions;

use crate::enums::OutputFlags;

/// Builds [`OpenOptions`] with platform-specific output flags applied.
///
/// On Unix-like platforms, this maps supported `oflag` bits to the underlying
/// `OpenOptionsExt::custom_flags` values. Unsupported flags are ignored by this
/// helper and are expected to be handled by higher-level validation.
#[cfg(target_family = "unix")]
pub fn get_options_with_flags(flags: u8) -> OpenOptions {
    use std::os::unix::fs::OpenOptionsExt;

    let mut options = OpenOptions::new();

    let mut unix_flags = 0;
    #[cfg(target_os = "linux")]
    if flags & OutputFlags::Direct as u8 != 0 {
        unix_flags |= libc::O_DIRECT;
    }

    if flags & OutputFlags::Nonblock as u8 != 0 {
        unix_flags |= libc::O_NONBLOCK;
    }

    if flags & OutputFlags::Sync as u8 != 0 {
        unix_flags |= libc::O_SYNC;
    }

    if flags & OutputFlags::Dsync as u8 != 0 {
        unix_flags |= libc::O_DSYNC;
    }

    if unix_flags != 0 {
        options.custom_flags(unix_flags);
    }

    options
}

/// Builds [`OpenOptions`] with platform-specific output flags applied.
///
/// On Windows, this maps supported `oflag` bits to the corresponding
/// `OpenOptionsExt::custom_flags` values. Flags that are not meaningful on
/// Windows are ignored or downgraded to a warning.
#[cfg(target_family = "windows")]
pub fn get_options_with_flags(flags: u8) -> OpenOptions {
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem;

    let mut options = OpenOptions::new();

    let mut windows_flags = 0;
    if flags & OutputFlags::Direct as u8 != 0 {
        windows_flags |= FileSystem::FILE_FLAG_NO_BUFFERING.0;
    }

    if flags & OutputFlags::Nonblock as u8 != 0 {
        eprintln!("Warning: 'iflag=nonblock' flag is not supported on Windows and will be ignored");
    }

    if flags & OutputFlags::Sync as u8 != 0 || flags & OutputFlags::Dsync as u8 != 0 {
        windows_flags |= FileSystem::FILE_FLAG_WRITE_THROUGH.0;
    }

    if windows_flags != 0 {
        options.custom_flags(windows_flags);
    }

    options
}

/// Configures a file descriptor for direct I/O on macOS.
///
/// This applies `F_NOCACHE` to the file descriptor so the kernel avoids
/// caching reads and writes for the target file.
#[cfg(target_os = "macos")]
pub fn configure_file_for_direct_io(
    file: &std::fs::File,
) -> Result<(), Box<dyn std::error::Error>> {
    use libc::{F_NOCACHE, fcntl};
    use std::os::unix::io::AsRawFd;

    let fd = file.as_raw_fd();
    let result = unsafe { fcntl(fd, F_NOCACHE, 1) };
    if result == -1 {
        return Err(Box::new(std::io::Error::last_os_error()));
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn temp_path() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("udc_wutils_test_{}_{}", std::process::id(), id))
    }

    // ── get_options_with_flags ────────────────────────────────────────────────

    /// With no flags, the returned `OpenOptions` opens a file for writing.
    #[test]
    #[cfg(target_family = "unix")]
    fn no_flags_opens_file_for_writing() {
        let path = temp_path();
        let mut opts = get_options_with_flags(0);
        let result = opts.write(true).create(true).truncate(true).open(&path);
        assert!(result.is_ok(), "Expected open to succeed with no flags");
        let _ = std::fs::remove_file(path);
    }

    /// The `Nonblock` flag produces options that still open a regular file
    /// (non-blocking has no observable effect on regular files).
    #[test]
    #[cfg(target_family = "unix")]
    fn nonblock_flag_opens_regular_file() {
        let path = temp_path();
        let mut opts = get_options_with_flags(OutputFlags::Nonblock as u8);
        let result = opts.write(true).create(true).truncate(true).open(&path);
        assert!(
            result.is_ok(),
            "Expected open to succeed with Nonblock flag"
        );
        let _ = std::fs::remove_file(path);
    }

    /// The `Sync` flag produces options that open a regular file successfully.
    #[test]
    #[cfg(target_family = "unix")]
    fn sync_flag_opens_regular_file() {
        let path = temp_path();
        let mut opts = get_options_with_flags(OutputFlags::Sync as u8);
        let result = opts.write(true).create(true).truncate(true).open(&path);
        assert!(result.is_ok(), "Expected open to succeed with Sync flag");
        let _ = std::fs::remove_file(path);
    }

    /// The `Dsync` flag produces options that open a regular file successfully.
    #[test]
    #[cfg(target_family = "unix")]
    fn dsync_flag_opens_regular_file() {
        let path = temp_path();
        let mut opts = get_options_with_flags(OutputFlags::Dsync as u8);
        let result = opts.write(true).create(true).truncate(true).open(&path);
        assert!(result.is_ok(), "Expected open to succeed with Dsync flag");
        let _ = std::fs::remove_file(path);
    }

    /// Combining `Sync` and `Nonblock` flags does not panic.
    #[test]
    #[cfg(target_family = "unix")]
    fn combined_sync_nonblock_flags_open_file() {
        let path = temp_path();
        let flags = OutputFlags::Sync as u8 | OutputFlags::Nonblock as u8;
        let mut opts = get_options_with_flags(flags);
        let result = opts.write(true).create(true).truncate(true).open(&path);
        assert!(
            result.is_ok(),
            "Expected open to succeed with Sync|Nonblock flags"
        );
        let _ = std::fs::remove_file(path);
    }

    /// On Linux, the `Direct` flag is forwarded as `O_DIRECT`; the call should
    /// not panic (filesystem support varies, so we only assert no panic).
    #[test]
    #[cfg(target_os = "linux")]
    fn direct_flag_does_not_panic_on_linux() {
        let path = temp_path();
        let mut opts = get_options_with_flags(OutputFlags::Direct as u8);
        let _ = opts.write(true).create(true).truncate(true).open(&path);
        let _ = std::fs::remove_file(path);
    }

    // ── Windows: get_options_with_flags ──────────────────────────────────────

    /// With no flags, the returned `OpenOptions` opens a file for writing.
    #[test]
    #[cfg(target_family = "windows")]
    fn no_flags_opens_file_for_writing() {
        let path = temp_path();
        let mut opts = get_options_with_flags(0);
        let result = opts.write(true).create(true).truncate(true).open(&path);
        assert!(result.is_ok(), "Expected open to succeed with no flags");
        let _ = std::fs::remove_file(path);
    }

    /// The `Nonblock` flag is unsupported on Windows but must not cause a panic
    /// or prevent the file from being opened.
    #[test]
    #[cfg(target_family = "windows")]
    fn nonblock_flag_opens_regular_file() {
        let path = temp_path();
        let mut opts = get_options_with_flags(OutputFlags::Nonblock as u8);
        let result = opts.write(true).create(true).truncate(true).open(&path);
        assert!(
            result.is_ok(),
            "Expected open to succeed with Nonblock flag on Windows"
        );
        let _ = std::fs::remove_file(path);
    }

    /// The `Direct` flag sets `FILE_FLAG_NO_BUFFERING`; the call must not panic
    /// (sector-aligned access may be required, so open failure is acceptable).
    #[test]
    #[cfg(target_family = "windows")]
    fn direct_flag_does_not_panic_on_windows() {
        let path = temp_path();
        let mut opts = get_options_with_flags(OutputFlags::Direct as u8);
        let _ = opts.write(true).create(true).truncate(true).open(&path);
        let _ = std::fs::remove_file(path);
    }

    // ── configure_file_for_direct_io ──────────────────────────────────────────

    /// On macOS, `configure_file_for_direct_io` succeeds for a valid open file.
    #[test]
    #[cfg(target_os = "macos")]
    fn configure_direct_io_succeeds_on_valid_file() {
        let path = temp_path();
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .unwrap();

        let result = configure_file_for_direct_io(&file);
        assert!(
            result.is_ok(),
            "Expected F_NOCACHE fcntl to succeed on a valid file descriptor"
        );
        let _ = std::fs::remove_file(path);
    }
}
