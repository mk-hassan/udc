//! Input flag utilities and OS-specific file descriptor tuning.
//!
//! This module provides cross-platform translation layers mapping bitwise internal
//! representation constraints (`iflag`) directly to platform-native raw system call settings.

use crate::enums::InputFlags;
use std::fs::OpenOptions;

/// Configures an `OpenOptions` configuration sequence matching platform-specific options.
///
/// Maps generic flags like `O_DIRECT` or `O_NONBLOCK` into their respective native
/// equivalents for Linux or general POSIX-compliant kernels.
///
/// ## Examples
///
/// ```ignore
/// use udc::enums::InputFlags;
/// let flags = InputFlags::Nonblock as u8;
/// let options = get_options_with_flags(flags);
/// ```
#[cfg(target_family = "unix")]
pub fn get_options_with_flags(flags: u8) -> OpenOptions {
    use std::os::unix::fs::OpenOptionsExt;

    let mut options = OpenOptions::new();
    let mut unix_flags = 0;

    #[cfg(target_os = "linux")]
    if flags & InputFlags::Direct as u8 != 0 {
        unix_flags |= libc::O_DIRECT;
    }

    if flags & InputFlags::Nonblock as u8 != 0 {
        unix_flags |= libc::O_NONBLOCK;
    }

    if unix_flags != 0 {
        options.custom_flags(unix_flags);
    }

    options
}

/// Configures an `OpenOptions` configuration sequence matching Windows subsystem options.
///
/// Replicates file flag bindings, mapping direct I/O constraints into unbuffered
/// hardware sector adjustments.
///
/// ## Warnings
///
/// Nonblocking I/O operations are unsupported via standard Windows named pipes or
/// console handles through this method, emitting a descriptive warning message if specified.
#[allow(unused_imports)]
#[cfg(target_family = "windows")]
pub fn get_options_with_flags(flags: u8) -> OpenOptions {
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem;

    let mut options = OpenOptions::new();
    let mut windows_flags = 0;

    if flags & InputFlags::Direct as u8 != 0 {
        windows_flags |= FileSystem::FILE_FLAG_NO_BUFFERING.0;
    }

    if flags & InputFlags::Nonblock as u8 != 0 {
        eprintln!("Warning: 'iflag=nonblock' flag is not supported on Windows and will be ignored");
    }

    if windows_flags != 0 {
        options.custom_flags(windows_flags);
    }

    options
}

/// Disables standard virtual memory kernel page caching for a specified file descriptor on macOS systems.
///
/// Binds directly to the Darwin low-level `fcntl` subsystem using the `F_NOCACHE` command bitmask.
/// This fulfills the direct hardware sector communication required by `iflag=direct`.
///
/// ## Errors
///
/// Returns an I/O error variant wrapping the underlying kernel code if `fcntl` rejects cache adjustments.
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
    use std::fs::File;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn temp_file_with(data: &[u8]) -> (File, PathBuf) {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("udc_utils_test_{}_{}", std::process::id(), id));
        std::fs::write(&path, data).unwrap();
        (File::open(&path).unwrap(), path)
    }

    // ── get_options_with_flags ────────────────────────────────────────────────

    /// With no flags set, the returned `OpenOptions` opens a real file for reading.
    #[test]
    #[cfg(target_family = "unix")]
    fn no_flags_opens_file_for_reading() {
        let (_, path) = temp_file_with(b"hello");
        let mut opts = get_options_with_flags(0);
        let file = opts.read(true).open(&path);
        assert!(file.is_ok(), "Expected file open to succeed with no flags");
        let _ = std::fs::remove_file(path);
    }

    /// The `Nonblock` flag produces options that still open a regular file
    /// successfully (non-blocking has no observable effect on normal files).
    #[test]
    #[cfg(target_family = "unix")]
    fn nonblock_flag_opens_regular_file() {
        let (_, path) = temp_file_with(b"data");
        let mut opts = get_options_with_flags(InputFlags::Nonblock as u8);
        let file = opts.read(true).open(&path);
        assert!(
            file.is_ok(),
            "Expected file open to succeed with Nonblock flag"
        );
        let _ = std::fs::remove_file(path);
    }

    /// On Linux, the `Direct` flag is forwarded as `O_DIRECT`; the file must
    /// still open successfully on a path that supports direct I/O.
    #[test]
    #[cfg(target_family = "unix")]
    fn direct_flag_opens_file_on_linux() {
        let (_, path) = temp_file_with(b"direct");
        let mut opts = get_options_with_flags(InputFlags::Direct as u8);
        // O_DIRECT on a regular file may or may not succeed depending on the
        // filesystem, so only verify the call does not panic.
        let _ = opts.read(true).open(&path);
        let _ = std::fs::remove_file(path);
    }

    /// Combining `Direct` and `Nonblock` flags does not panic or produce an
    /// error unrelated to the underlying kernel (open must succeed for a normal
    /// file on macOS where `Direct` is handled via F_NOCACHE separately).
    #[test]
    #[cfg(target_os = "macos")]
    fn combined_flags_open_file_on_macos() {
        let (_, path) = temp_file_with(b"combined");
        let flags = InputFlags::Direct as u8 | InputFlags::Nonblock as u8;
        let mut opts = get_options_with_flags(flags);
        let file = opts.read(true).open(&path);
        assert!(
            file.is_ok(),
            "Expected combined flags open to succeed on macOS"
        );
        let _ = std::fs::remove_file(path);
    }

    // ── Windows: get_options_with_flags ──────────────────────────────────────

    /// With no flags set, the returned `OpenOptions` opens a real file for reading.
    #[test]
    #[cfg(target_family = "windows")]
    fn no_flags_opens_file_for_reading() {
        let (_, path) = temp_file_with(b"hello");
        let mut opts = get_options_with_flags(0);
        let file = opts.read(true).open(&path);
        assert!(file.is_ok(), "Expected file open to succeed with no flags");
        let _ = std::fs::remove_file(path);
    }

    /// The `Nonblock` flag is unsupported on Windows but must not cause a panic
    /// or prevent the file from being opened.
    #[test]
    #[cfg(target_family = "windows")]
    fn nonblock_flag_opens_regular_file() {
        let (_, path) = temp_file_with(b"data");
        let mut opts = get_options_with_flags(InputFlags::Nonblock as u8);
        let file = opts.read(true).open(&path);
        assert!(
            file.is_ok(),
            "Expected file open to succeed with Nonblock flag on Windows"
        );
        let _ = std::fs::remove_file(path);
    }

    /// The `Direct` flag sets `FILE_FLAG_NO_BUFFERING`; on a regular file this
    /// may fail depending on the filesystem, so only verify the call does not panic.
    #[test]
    #[cfg(target_family = "windows")]
    fn direct_flag_does_not_panic_on_windows() {
        let (_, path) = temp_file_with(b"direct");
        let mut opts = get_options_with_flags(InputFlags::Direct as u8);
        // FILE_FLAG_NO_BUFFERING requires sector-aligned access; open may fail
        // on a regular temp file — that is acceptable, panic is not.
        let _ = opts.read(true).open(&path);
        let _ = std::fs::remove_file(path);
    }

    // ── configure_file_for_direct_io ──────────────────────────────────────────

    /// On macOS, `configure_file_for_direct_io` succeeds for a valid, open file.
    #[test]
    #[cfg(target_os = "macos")]
    fn configure_direct_io_succeeds_on_valid_file() {
        let (file, path) = temp_file_with(b"nocache");
        let result = configure_file_for_direct_io(&file);
        assert!(
            result.is_ok(),
            "Expected F_NOCACHE to succeed on a valid file descriptor"
        );
        let _ = std::fs::remove_file(path);
    }
}
