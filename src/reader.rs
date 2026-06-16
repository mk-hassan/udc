//! Input stream ingestion abstractions for the processing pipeline.
//!
//! Handles polymorphic standard input tracking and standard file interactions,
//! implementing unified `Read` and `Seek` traits for sequential block processing.

use std::fs::File;
use std::io::{Error, ErrorKind, Read, Seek, SeekFrom, Stdin};

use super::{config::Config, enums};

mod utils;

/// A polymorphic wrapper unifying uniform sequential resource input operations.
pub enum Reader {
    /// File-backed persistent storage stream block reader.
    File(File),
    /// Unseekable line-buffered or block-buffered interactive Standard Input device descriptor.
    Stdin(Stdin),
}

impl Read for Reader {
    /// Progresses the target stream forward, populating a mutable byte container slice.
    ///
    /// ## Errors
    ///
    /// Propagates platform-specific system call errors encountered by underlying device operations.
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Reader::File(file) => file.read(buf),
            Reader::Stdin(stdin) => stdin.read(buf),
        }
    }
}

impl Seek for Reader {
    /// Positions the processing stream to an absolute offset boundary.
    ///
    /// ## Errors
    ///
    /// * Returns an `InvalidInput` error variant if anything besides `SeekFrom::Start` is queried.
    /// * Returns an `UnexpectedEof` error variant if a non-seekable source terminates early.
    ///
    /// ## Warnings
    ///
    /// For unseekable streams like `Stdin`, this method emulates absolute position jumps by
    /// performing destructive byte consumption. Consequently, subsequent absolute seeks will
    /// perform calculations relative to current state instead of the historical file start.
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let SeekFrom::Start(offset) = pos else {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "udc: validation: only seeking from the start of the file is supported for reader skipping operations",
            ));
        };

        match self {
            Reader::File(file) => file.seek(pos),
            Reader::Stdin(stdin) => Self::skip(stdin, offset),
        }
    }
}

impl Reader {
    /// Instantiates and configures a `Reader` type matching configuration rules.
    ///
    /// Evaluates initialization requirements, handles low-level file open permissions,
    /// cross-platform flags, and skips initial data bounds.
    ///
    /// ## Errors
    ///
    /// Returns a box-allocated error wrapper if file access is rejected, platform configuration
    /// fails, or an early EOF happens during data skipping.
    pub fn build(config: &Config) -> Result<Reader, Box<dyn std::error::Error>> {
        let mut target = match config.get_source() {
            enums::SourceType::File(path) => {
                let file = Self::get_file_reader(path, config.get_iflag())?;
                Reader::File(file)
            }
            enums::SourceType::Standard => Reader::Stdin(std::io::stdin()),
        };

        if let &Some(skip_bytes) = config.get_skip() {
            target.seek(SeekFrom::Start(skip_bytes as u64))?;
        }

        Ok(target)
    }

    /// Emulates data positioning operations for unseekable resources by consuming bytes in chunks.
    ///
    /// Rather than single massive byte array generation, this uses a fixed allocation
    /// cache block (4096 bytes) to protect the environment against heap fragmentation or OOM failures.
    ///
    /// ## Errors
    ///
    /// Returns `UnexpectedEof` if the underlying stream runs dry before the requested skip offset
    /// constraint is satisfied.
    fn skip(source: &mut dyn Read, offset: u64) -> std::io::Result<u64> {
        let mut remaining = offset;
        let mut buffer = [0u8; 4096];

        while remaining > 0 {
            let to_read = remaining.min(buffer.len() as u64) as usize;
            match source.read(&mut buffer[..to_read]) {
                Ok(0) => {
                    return Err(Error::new(
                        ErrorKind::UnexpectedEof,
                        "udc: stdin: reached EOF before completing the skip operation",
                    ));
                }
                Ok(n) => {
                    remaining -= n as u64;
                }
                Err(ref e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(offset)
    }

    /// Resolves system paths, establishing operational files configured to matching cache specifications.
    fn get_file_reader(path: &str, flags: u8) -> Result<File, Box<dyn std::error::Error>> {
        let mut options = utils::get_options_with_flags(flags);

        let file = options.read(true).open(path)?;

        #[cfg(target_os = "macos")]
        {
            if flags & enums::InputFlags::Direct as u8 != 0 {
                utils::configure_file_for_direct_io(&file)?;
            }
        }

        Ok(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read, SeekFrom};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    // ── Helpers ───────────────────────────────────────────────────────────────

    /// Creates a temporary file populated with `data` and returns an open read
    /// handle together with the path for later cleanup.
    fn temp_file_with(data: &[u8]) -> (File, PathBuf) {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("udc_reader_test_{}_{}", std::process::id(), id));
        std::fs::write(&path, data).unwrap();
        (File::open(&path).unwrap(), path)
    }

    // ── Reader::skip ──────────────────────────────────────────────────────────

    /// Skipping zero bytes is a no-op; the full source remains readable.
    #[test]
    fn skip_zero_bytes_is_noop() {
        let mut src = Cursor::new(b"hello");
        assert_eq!(Reader::skip(&mut src, 0).unwrap(), 0);
        let mut buf = [0u8; 5];
        let _ = src.read(&mut buf).unwrap();
        assert_eq!(&buf, b"hello");
    }

    /// Skipping N bytes advances the read position by exactly N.
    #[test]
    fn skip_partial_advances_position() {
        let mut src = Cursor::new(b"hello world");
        assert_eq!(Reader::skip(&mut src, 6).unwrap(), 6);
        let mut buf = [0u8; 5];
        let _ = src.read(&mut buf).unwrap();
        assert_eq!(&buf, b"world");
    }

    /// Skipping exactly to EOF succeeds and the source is exhausted.
    #[test]
    fn skip_exactly_to_eof_succeeds() {
        let mut src = Cursor::new(b"abcdefgh");
        assert_eq!(Reader::skip(&mut src, 8).unwrap(), 8);
        let mut buf = [0u8; 1];
        assert_eq!(src.read(&mut buf).unwrap(), 0);
    }

    /// Skipping more bytes than the source contains returns `UnexpectedEof`.
    #[test]
    fn skip_beyond_eof_returns_unexpected_eof() {
        let mut src = Cursor::new(b"abc");
        let err = Reader::skip(&mut src, 10).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::UnexpectedEof);
    }

    /// Skipping any amount from an empty source returns `UnexpectedEof`.
    #[test]
    fn skip_empty_source_returns_unexpected_eof() {
        let mut src = Cursor::new(b"");
        let err = Reader::skip(&mut src, 1).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::UnexpectedEof);
    }

    /// Skipping more than one internal chunk (4096 bytes) works correctly.
    #[test]
    fn skip_larger_than_internal_chunk_works() {
        let data = vec![0x55u8; 5000];
        let mut src = Cursor::new(data);
        assert_eq!(Reader::skip(&mut src, 5000).unwrap(), 5000);
        let mut buf = [0u8; 1];
        assert_eq!(src.read(&mut buf).unwrap(), 0);
    }

    /// Skipping exactly one chunk boundary (4096 bytes) leaves the rest readable.
    #[test]
    fn skip_exactly_one_chunk_boundary() {
        let mut data = vec![0xAAu8; 4096];
        data.extend_from_slice(b"tail");
        let mut src = Cursor::new(data);
        assert_eq!(Reader::skip(&mut src, 4096).unwrap(), 4096);
        let mut buf = [0u8; 4];
        src.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"tail");
    }

    // ── Reader::read ──────────────────────────────────────────────────────────

    /// `Reader::File` reads the full content into the buffer.
    #[test]
    fn read_file_variant_reads_content() {
        let (file, path) = temp_file_with(b"test data");
        let mut reader = Reader::File(file);
        let mut buf = [0u8; 9];
        assert_eq!(reader.read(&mut buf).unwrap(), 9);
        assert_eq!(&buf, b"test data");
        let _ = std::fs::remove_file(path);
    }

    /// `Reader::File` returns 0 bytes when the source is empty (EOF).
    #[test]
    fn read_file_variant_returns_zero_at_eof() {
        let (file, path) = temp_file_with(b"");
        let mut reader = Reader::File(file);
        let mut buf = [0u8; 8];
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
        let _ = std::fs::remove_file(path);
    }

    /// Reads are bounded by the buffer length, not the source length.
    #[test]
    fn read_file_variant_respects_buffer_length() {
        let (file, path) = temp_file_with(b"0123456789");
        let mut reader = Reader::File(file);
        let mut buf = [0u8; 4];
        let n = reader.read(&mut buf).unwrap();
        assert!(n <= 4);
        assert_eq!(&buf[..n], b"0123"[..n].as_ref());
        let _ = std::fs::remove_file(path);
    }

    // ── Reader::seek ──────────────────────────────────────────────────────────

    /// `SeekFrom::Start` on a `Reader::File` repositions the file cursor.
    #[test]
    fn seek_file_start_advances_position() {
        let (file, path) = temp_file_with(b"abcde");
        let mut reader = Reader::File(file);
        assert_eq!(reader.seek(SeekFrom::Start(2)).unwrap(), 2);
        let mut buf = [0u8; 3];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"cde");
        let _ = std::fs::remove_file(path);
    }

    /// `SeekFrom::Start(0)` on a file rewinds to the beginning.
    #[test]
    fn seek_file_start_zero_rewinds() {
        let (file, path) = temp_file_with(b"rewind");
        let mut reader = Reader::File(file);
        reader.seek(SeekFrom::Start(3)).unwrap();
        reader.seek(SeekFrom::Start(0)).unwrap();
        let mut buf = [0u8; 6];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"rewind");
        let _ = std::fs::remove_file(path);
    }

    /// `SeekFrom::Current` is rejected with `InvalidInput` on a `Reader::File`.
    #[test]
    fn seek_rejects_seek_from_current_on_file() {
        let (file, path) = temp_file_with(b"abc");
        let mut reader = Reader::File(file);
        let err = reader.seek(SeekFrom::Current(1)).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
        let _ = std::fs::remove_file(path);
    }

    /// `SeekFrom::End` is rejected with `InvalidInput` on a `Reader::File`.
    #[test]
    fn seek_rejects_seek_from_end_on_file() {
        let (file, path) = temp_file_with(b"abc");
        let mut reader = Reader::File(file);
        let err = reader.seek(SeekFrom::End(0)).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
        let _ = std::fs::remove_file(path);
    }

    // ── Reader::build ─────────────────────────────────────────────────────────

    /// `Reader::build` opens an existing file source successfully.
    #[test]
    fn build_opens_file_source() {
        let (_, path) = temp_file_with(b"content");
        let args = vec![
            format!("if={}", path.to_str().unwrap()),
            "of=/dev/null".to_string(),
        ];
        let config = crate::config::Config::build(&args).unwrap();
        assert!(Reader::build(&config).is_ok());
        let _ = std::fs::remove_file(path);
    }

    /// `Reader::build` returns an error when the file path does not exist.
    #[test]
    fn build_returns_error_for_missing_file() {
        let args = vec![
            "if=/nonexistent/__udc_test_file__.bin".to_string(),
            "of=/dev/null".to_string(),
        ];
        let config = crate::config::Config::build(&args).unwrap();
        assert!(Reader::build(&config).is_err());
    }

    /// `Reader::build` with `skip_bytes=N` skips exactly N bytes so subsequent
    /// reads start at the correct offset.
    #[test]
    fn build_with_skip_bytes_advances_read_position() {
        // "skip_this_" = 10 bytes; "content_rest" = 12 bytes
        let (_, path) = temp_file_with(b"skip_this_content_rest");
        let args = vec![
            format!("if={}", path.to_str().unwrap()),
            "of=/dev/null".to_string(),
            "skip_bytes=10".to_string(),
        ];
        let config = crate::config::Config::build(&args).unwrap();
        let mut reader = Reader::build(&config).unwrap();
        let mut buf = vec![0u8; 12];
        let n = reader.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"content_rest");
        let _ = std::fs::remove_file(path);
    }

    /// `Reader::build` with no skip specified starts reading from the beginning.
    #[test]
    fn build_without_skip_starts_at_beginning() {
        let (_, path) = temp_file_with(b"from_start");
        let args = vec![
            format!("if={}", path.to_str().unwrap()),
            "of=/dev/null".to_string(),
        ];
        let config = crate::config::Config::build(&args).unwrap();
        let mut reader = Reader::build(&config).unwrap();
        let mut buf = [0u8; 10];
        let n = reader.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"from_start");
        let _ = std::fs::remove_file(path);
    }
}
