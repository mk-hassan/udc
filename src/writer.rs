//! Output stream abstractions and physical storage serialization hooks.
//!
//! This module provides the [`Writer`] enum, which acts as a uniform interface over
//! block-based destinations. It encapsulates file-system level persistence and console
//! standard streams, layer-abstracting low-level block logic such as:
//!
//! 1. **Sparse File Allocation (`conv=sparse`)**: Bypassing explicit byte-block layout allocations
//!    on underlying storage media by replacing pure zero blocks with seek jumps (`SeekFrom::Current`).
//! 2. **Direct I/O Configuration (`O_DIRECT`)**: Allowing low-level bypasses of the OS page cache
//!    for critical high-throughput sector cloning.
//!

use std::fs::File;
use std::io::{Seek, SeekFrom, Stdout, Write};

use super::{config::Config, enums::SourceType};

mod utils;

/// Unified destination interface for the data transmission lifecycle
///
/// Encapsulates resource-specific variants for the data stream target. It handles
/// specialized configuration contexts like block device tracking flags and terminal
/// pipeline handling
pub enum Writer {
    /// File-backed system target
    ///
    /// Encloses the underlying standard filesystem [`File`] handle along with a `bool`
    /// flag indicating whether sparse block optimization (`conv=sparse`) is active
    File(File, bool),
    /// Standard console output descriptor stream
    Stdout(Stdout),
}

impl Write for Writer {
    /// Writes a slice of bytes into the designated destination target.
    ///
    /// ### Sparse Optimization Behavior
    /// If this `Writer` is a [`Writer::File`] variant with sparse allocation enabled,
    /// and the input slice consists **entirely** of `0x00` zero bytes, this method will
    /// optimize space allocation. Instead of writing raw blocks to disk, it executes a
    /// relative forward seek via [`Seek::seek`]. The virtual file pointer shifts forward,
    /// leaving a filesystem "hole" that consumes no physical storage blocks.
    ///
    /// ### Errors
    /// Returns an [`std::io::Error`] if an underlying hardware block write fails,
    /// if a stream connection drops prematurely, or if a sparse seek boundary is violated.
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if Self::is_sparse(self) && Self::is_all_zeros(buf) {
            let _ = self.seek(SeekFrom::Current(buf.len() as i64))?;
            return Ok(buf.len());
        }

        let written_bytes = match self {
            Writer::File(f, _) => f.write(buf),
            Writer::Stdout(s) => s.write(buf),
        }?;

        Ok(written_bytes)
    }

    // Flushes all intermediate kernel buffers to disk, solidifying persistence.
    ///
    /// ### Structural Invariants for Sparse Output
    /// For a sparse [`Writer::File`], standard system-level zero-seeks do not advance
    /// the physical data metadata length if the file terminates on a sparse boundary.
    /// To ensure the final file size accurately matches the bytes processed, this function
    /// samples the active stream cursor position and updates the filesystem's structural length
    /// boundaries using an explicit file length allocation call before completing.
    ///
    /// ### Errors
    /// Returns an error if the synchronization interface encounters file-lock problems,
    /// system call interruption, or driver failures.
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Writer::File(f, is_sparse) => {
                f.flush()?;
                if *is_sparse {
                    let position = f.stream_position()?;
                    f.set_len(position)?;
                }
                Ok(())
            }
            Writer::Stdout(s) => s.flush(),
        }
    }
}

impl Seek for Writer {
    /// Repositions the write cursor within the underlying target descriptor.
    ///
    /// ### Stream Discrepancy Policy
    /// * **[`Writer::File`]**: Seeks are fully forwarded to the hardware block interface.
    /// * **[`Writer::Stdout`]**: Standard terminal pipelines are inherently unseekable.
    ///   To prevent breaking multi-stage shell execution chains, seeking a standard output
    ///   stream returns `Ok(0)` silently without raising hard errors.
    ///
    /// ### Errors
    /// Returns an error if an on-disk boundary underflow occurs, or if hardware sectors
    /// cannot be verified.
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        match self {
            Writer::File(file, _) => {
                let new_pos = file.seek(pos)?;
                Ok(new_pos)
            }
            _ => Ok(0),
        }
    }
}

impl Writer {
    /// Factory initializer that constructs a concrete `Writer` matching a [`Config`] profile.
    ///
    /// Evaluates target variables to determine whether it needs to allocate a persistent file
    /// or track a standard output stream handle. It configures properties such as truncation levels,
    /// exclusive block flags, and handles immediate initialization jumps if `seek_bytes` parameter bounds
    /// are active.
    ///
    /// # Errors
    /// Returns a generic trait object error wrapper if permission checks fail, if a file
    /// path directory tree is missing, or if an initial offset seek exceeds partition constraints.
    ///
    /// # Examples
    /// ```no_run
    /// use udc::config::Config;
    /// use udc::writer::Writer;
    ///
    /// let args = vec!["if=/dev/null".to_string(), "of=output.bin".to_string(), "seek_bytes=1024".to_string()];
    /// let config = Config::build(&args).unwrap();
    /// let writer = Writer::build(&config).unwrap();
    /// ```
    pub fn build(config: &Config) -> Result<Writer, Box<dyn std::error::Error>> {
        let mut target = match config.get_destination() {
            SourceType::File(path) => {
                let file = Self::get_file_writer(
                    path,
                    config.get_oflag(),
                    config.is_exec(),
                    config.is_truncate(),
                )?;
                Writer::File(file, config.is_sparse())
            }
            SourceType::Standard => Writer::Stdout(std::io::stdout()),
        };

        if let &Some(skip_bytes) = config.get_seek() {
            target.seek(SeekFrom::Start(skip_bytes as u64))?;
        }

        Ok(target)
    }

    /// High-resolution on-disk file allocator and multi-platform flags constructor.
    ///
    /// Aggregates platform-specific system flags from configuration parameters (such as `O_DIRECT`
    /// on Unix-like layouts or `FILE_FLAG_NO_BUFFERING` on Windows systems) before triggering
    /// the kernel resource allocation loop.
    fn get_file_writer(
        path: &str,
        flags: u8,
        create_new: bool,
        truncate: bool,
    ) -> Result<File, Box<dyn std::error::Error>> {
        let mut options = utils::get_options_with_flags(flags);

        let file = options
            .create(!create_new)
            .create_new(create_new)
            .truncate(truncate)
            .write(true)
            .open(path)?;

        #[cfg(target_os = "macos")]
        {
            if flags & crate::enums::OutputFlags::Direct as u8 != 0 {
                utils::configure_file_for_direct_io(&file)?;
            }
        }

        Ok(file)
    }

    /// Evaluates whether a given `Writer` reference is eligible for sparse file allocation.
    #[inline]
    fn is_sparse(writer: &Writer) -> bool {
        match writer {
            Writer::File(_, is_sparse) => *is_sparse,
            _ => false,
        }
    }

    /// Analyzes a memory block to determine if it consists exclusively of zero bytes (`0x00`).
    ///
    /// Empty buffers return `false` because they lack physical space to optimize.
    #[inline]
    fn is_all_zeros(buffer: &[u8]) -> bool {
        !buffer.is_empty() && buffer.iter().all(|&b| b == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{SeekFrom, Write};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn temp_path() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("udc_writer_test_{}_{}", std::process::id(), id))
    }

    /// Returns an open read-write `File` at `path` (created/truncated).
    fn open_rw(path: &PathBuf) -> File {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .unwrap()
    }

    /// Reads the full contents of the file at `path` as bytes.
    fn read_file(path: &PathBuf) -> Vec<u8> {
        std::fs::read(path).unwrap()
    }

    // ── Writer::is_all_zeros ──────────────────────────────────────────────────

    /// An empty buffer does not satisfy the all-zeros check.
    #[test]
    fn is_all_zeros_empty_buffer_returns_false() {
        assert!(!Writer::is_all_zeros(b""));
    }

    /// A buffer filled entirely with zeros returns true.
    #[test]
    fn is_all_zeros_all_zero_bytes() {
        assert!(Writer::is_all_zeros(&[0u8; 8]));
    }

    /// A buffer with at least one non-zero byte returns false.
    #[test]
    fn is_all_zeros_mixed_bytes_returns_false() {
        assert!(!Writer::is_all_zeros(b"\x00\x01\x00"));
    }

    /// A buffer with no zero bytes returns false.
    #[test]
    fn is_all_zeros_no_zeros_returns_false() {
        assert!(!Writer::is_all_zeros(b"hello"));
    }

    // ── Writer::is_sparse ─────────────────────────────────────────────────────

    /// `Writer::File` with sparse flag `true` is identified as sparse.
    #[test]
    fn is_sparse_file_with_flag_true() {
        let path = temp_path();
        let file = open_rw(&path);
        assert!(Writer::is_sparse(&Writer::File(file, true)));
        let _ = std::fs::remove_file(path);
    }

    /// `Writer::File` with sparse flag `false` is not identified as sparse.
    #[test]
    fn is_sparse_file_with_flag_false() {
        let path = temp_path();
        let file = open_rw(&path);
        assert!(!Writer::is_sparse(&Writer::File(file, false)));
        let _ = std::fs::remove_file(path);
    }

    /// `Writer::Stdout` is never considered sparse.
    #[test]
    fn is_sparse_stdout_always_false() {
        assert!(!Writer::is_sparse(&Writer::Stdout(std::io::stdout())));
    }

    // ── Writer::write ─────────────────────────────────────────────────────────

    /// Non-sparse mode writes data directly to the file.
    #[test]
    fn write_non_sparse_writes_data_to_file() {
        let path = temp_path();
        let file = open_rw(&path);
        let mut writer = Writer::File(file, false);
        let n = writer.write(b"hello world").unwrap();
        assert_eq!(n, 11);
        drop(writer);
        assert_eq!(read_file(&path), b"hello world");
        let _ = std::fs::remove_file(path);
    }

    /// In sparse mode, a buffer of all zeros advances the file position without
    /// writing any bytes to disk (the zeros are created lazily on flush).
    #[test]
    fn write_sparse_zeros_seeks_forward_without_writing() {
        let path = temp_path();
        let file = open_rw(&path);
        let mut writer = Writer::File(file, true);

        // Write real data first to establish file content.
        let _ = writer.write(b"AB").unwrap();

        // Write three zero bytes in sparse mode — should seek, not write.
        let n = writer.write(&[0u8; 3]).unwrap();
        assert_eq!(n, 3);

        // Flush causes set_len to extend the file to include the sparse region.
        writer.flush().unwrap();
        let contents = read_file(&path);
        assert_eq!(contents, b"AB\x00\x00\x00");
        let _ = std::fs::remove_file(path);
    }

    /// In sparse mode, non-zero data is written normally even if sparse is set.
    #[test]
    fn write_sparse_nonzero_data_writes_normally() {
        let path = temp_path();
        let file = open_rw(&path);
        let mut writer = Writer::File(file, true);
        let n = writer.write(b"data").unwrap();
        assert_eq!(n, 4);
        drop(writer);
        assert_eq!(read_file(&path), b"data");
        let _ = std::fs::remove_file(path);
    }

    /// In sparse mode, a mix of non-zero then zero writes produces the expected
    /// final file content after flush.
    #[test]
    fn write_sparse_mixed_content_produces_correct_file() {
        let path = temp_path();
        let file = open_rw(&path);
        let mut writer = Writer::File(file, true);

        let _ = writer.write(b"head").unwrap(); // 4 bytes written
        let _ = writer.write(&[0u8; 4]).unwrap(); // 4 sparse zeros (seeks)
        let _ = writer.write(b"tail").unwrap(); // 4 bytes written
        writer.flush().unwrap();

        let contents = read_file(&path);
        assert_eq!(contents, b"head\x00\x00\x00\x00tail");
        let _ = std::fs::remove_file(path);
    }

    // ── Writer::flush ─────────────────────────────────────────────────────────

    /// Sparse flush truncates the file to the current stream position, extending
    /// it to include any trailing sparse zero region.
    #[test]
    fn flush_sparse_extends_file_to_current_position() {
        let path = temp_path();
        let file = open_rw(&path);
        let mut writer = Writer::File(file, true);

        let _ = writer.write(b"XY").unwrap();
        let _ = writer.write(&[0u8; 6]).unwrap(); // sparse seek to position 8
        writer.flush().unwrap();

        let contents = read_file(&path);
        assert_eq!(contents.len(), 8);
        assert_eq!(&contents[..2], b"XY");
        assert_eq!(&contents[2..], &[0u8; 6]);
        let _ = std::fs::remove_file(path);
    }

    /// Non-sparse flush does not alter the file length beyond what was written.
    #[test]
    fn flush_non_sparse_does_not_alter_file_length() {
        let path = temp_path();
        let file = open_rw(&path);
        let mut writer = Writer::File(file, false);

        let _ = writer.write(b"fixed").unwrap();
        writer.flush().unwrap();

        let contents = read_file(&path);
        assert_eq!(contents, b"fixed");
        let _ = std::fs::remove_file(path);
    }

    // ── Writer::seek ──────────────────────────────────────────────────────────

    /// Seeking a `Writer::File` positions the write cursor at the requested offset.
    #[test]
    fn seek_file_positions_write_cursor() {
        let path = temp_path();
        // Pre-fill with known data to make the seek visible.
        std::fs::write(&path, b"AAAAAA").unwrap();
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        let mut writer = Writer::File(file, false);
        writer.seek(SeekFrom::Start(2)).unwrap();
        let _ = writer.write(b"BB").unwrap();
        drop(writer);
        assert_eq!(read_file(&path), b"AABBAA");
        let _ = std::fs::remove_file(path);
    }

    /// Seeking a `Writer::Stdout` silently returns `Ok(0)` without error.
    #[test]
    fn seek_stdout_returns_ok_zero_silently() {
        let mut writer = Writer::Stdout(std::io::stdout());
        let result = writer.seek(SeekFrom::Start(100));
        assert_eq!(result.unwrap(), 0);
    }

    // ── Writer::build ─────────────────────────────────────────────────────────

    /// `Writer::build` creates and opens an output file successfully.
    #[test]
    fn build_creates_output_file() {
        let path = temp_path();
        let args = vec![
            "if=/dev/null".to_string(),
            format!("of={}", path.to_str().unwrap()),
        ];
        let config = crate::config::Config::build(&args).unwrap();
        let result = Writer::build(&config);
        assert!(result.is_ok(), "Expected Ok, got {:?}", result.err());
        let _ = std::fs::remove_file(path);
    }

    /// `Writer::build` returns an error when the output directory does not exist.
    #[test]
    fn build_returns_error_for_invalid_path() {
        let args = vec![
            "if=/dev/null".to_string(),
            "of=/nonexistent/__dir__/out.bin".to_string(),
        ];
        let config = crate::config::Config::build(&args).unwrap();
        assert!(Writer::build(&config).is_err());
    }

    /// `Writer::build` with `seek_bytes=N` positions the write cursor at byte N
    /// so subsequent writes begin at the correct offset.
    #[test]
    fn build_with_seek_bytes_advances_write_position() {
        let path = temp_path();
        // Pre-fill with known bytes so we can confirm the seek offset.
        std::fs::write(&path, b"000000000000").unwrap();

        let args = vec![
            "if=/dev/null".to_string(),
            format!("of={}", path.to_str().unwrap()),
            "seek_bytes=4".to_string(),
            "conv=notrunc".to_string(),
        ];
        let config = crate::config::Config::build(&args).unwrap();
        let mut writer = Writer::build(&config).unwrap();
        let _ = writer.write(b"SEEK").unwrap();
        drop(writer);

        let contents = read_file(&path);
        assert_eq!(&contents[4..8], b"SEEK");
        let _ = std::fs::remove_file(path);
    }
}
