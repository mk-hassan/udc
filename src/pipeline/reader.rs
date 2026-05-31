use std::fs::{ File };
use std::io::{ Read, BufReader, Seek, SeekFrom, Stdin, Error, ErrorKind };

use crate::config::Config;
use crate::{SourceType, constants};

pub struct Reader {
	ibs: usize,
	is_sync: bool,
	target: TargetReader,
}

enum TargetReader {
    File(BufReader<File>),
    Stdin(BufReader<Stdin>),
}

impl Read for TargetReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::File(f) => f.read(buf),
            Self::Stdin(s) => s.read(buf),
        }
    }
}

impl Reader {
    pub fn build(config: &Config) -> Result<Reader, Box<dyn std::error::Error>> {
		let reader = Reader {
			ibs: config.get_ibs(),
			is_sync: config.is_sync(),
			target: Self::build_target(config)?
		};

		Ok(reader)
    }

	fn build_target(config: &Config) -> Result<TargetReader, Box<dyn std::error::Error>> {
		let skipped_bytes = Self::checked_skip(&config)?;

		match config.get_source() {
            SourceType::File(path) => {
				let mut file = BufReader::with_capacity(
                	config.get_ibs(),
                	File::open(path)?,
            	);

				file.seek(SeekFrom::Start(skipped_bytes))?;
				Ok(TargetReader::File(file))
			},
            SourceType::Standard => {
				let mut stdin = BufReader::with_capacity(
					config.get_ibs(),
					std::io::stdin(),
            	);

				Reader::skip(&mut stdin, skipped_bytes as usize, config.get_ibs())?;
				Ok(TargetReader::Stdin(stdin))
			}
        }
	}

	fn checked_skip(config: &Config) -> Result<u64, Box<dyn std::error::Error>> {
		let ibs = u64::try_from(config.get_ibs())?;
		let skip_blocks = u64::try_from(config.get_skip().unwrap_or(0))?;
		match u64::checked_mul(skip_blocks, ibs) {
			Some(bytes) => Ok(bytes),
			None => return Err(Box::new(Error::new(ErrorKind::Other, constants::SKIP_AMOUNT_OUT_OF_BOUND)))
		}
	}

	fn skip(dest: &mut impl Read, mut bytes: usize, ibs: usize) -> Result<(), Box<dyn std::error::Error>> {
        let mut discard = vec![0u8; ibs];
		while bytes > 0 {
			let n = dest.read(&mut discard[..bytes.min(ibs)])?;
			if n == 0 { break; }
			bytes -= n;
		}
		Ok(())
    }

	pub fn read(&mut self, buffer: &mut [u8]) -> Result<usize, Box<dyn std::error::Error>> {
		let ibs = self.ibs;

		match <TargetReader as Read>::read(&mut self.target, buffer) {
			Ok(0) => Ok(0),
			Ok(n) => {
				if self.is_sync && n < ibs {
					buffer[n..ibs].fill(0);
				}
				Ok(if self.is_sync { ibs } else { n })
			},
			Err(error) => {
				if self.is_sync {
					buffer.fill(0);
				}
				Err(Box::new(error))
			}
		}
	}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WriteOps;
    use std::fs;

    fn temp_path(name: &str) -> String {
        std::env::temp_dir()
            .join(format!("ccdd_reader_{}.bin", name))
            .to_str()
            .unwrap()
            .to_owned()
    }

    fn make_config(path: &str, ibs: usize) -> Config {
        Config::new()
            .source(SourceType::File(path.to_string()))
            .input_block_size(ibs)
    }

    fn make_config_sync(path: &str, ibs: usize) -> Config {
        make_config(path, ibs).write_convs(WriteOps::Sync as u8)
    }

    // ─── basic reads ─────────────────────────────────────────────────────────

    #[test]
    fn test_read_exact_full_block() {
        let path = temp_path("full_block");
        fs::write(&path, b"ABCD").unwrap();

        let mut reader = Reader::build(&make_config(&path, 4)).unwrap();
        let mut buf = [0u8; 4];
        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 4);
        assert_eq!(&buf, b"ABCD");
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_read_partial_last_block_no_sync() {
        // file = 5 bytes, ibs = 4 → first read full, second partial (1 byte)
        let path = temp_path("partial_no_sync");
        fs::write(&path, b"ABCDE").unwrap();

        let mut reader = Reader::build(&make_config(&path, 4)).unwrap();
        let mut buf = [0u8; 4];

        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 4);
        assert_eq!(&buf, b"ABCD");

        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 1); // partial: only 1 byte, no padding without sync
        assert_eq!(buf[0], b'E');
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_read_eof_returns_zero() {
        let path = temp_path("eof");
        fs::write(&path, b"ABCD").unwrap();

        let mut reader = Reader::build(&make_config(&path, 4)).unwrap();
        let mut buf = [0u8; 4];
        reader.read(&mut buf).unwrap();           // consume all data
        assert_eq!(reader.read(&mut buf).unwrap(), 0); // EOF
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_read_empty_file_returns_zero_immediately() {
        let path = temp_path("empty");
        fs::write(&path, b"").unwrap();

        let mut reader = Reader::build(&make_config(&path, 4)).unwrap();
        let mut buf = [0u8; 4];
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_read_multiple_full_blocks() {
        let path = temp_path("multi_blocks");
        fs::write(&path, b"AAAABBBBCCCC").unwrap(); // 12 bytes, ibs=4

        let mut reader = Reader::build(&make_config(&path, 4)).unwrap();
        let mut buf = [0u8; 4];

        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 4);
        assert_eq!(&buf, b"AAAA");

        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 4);
        assert_eq!(&buf, b"BBBB");

        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 4);
        assert_eq!(&buf, b"CCCC");

        assert_eq!(reader.read(&mut buf).unwrap(), 0); // EOF
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_read_ibs_of_one_reads_one_byte_at_a_time() {
        let path = temp_path("ibs_one");
        fs::write(&path, b"XYZ").unwrap();

        let mut reader = Reader::build(&make_config(&path, 1)).unwrap();
        let mut buf = [0u8; 1];

        assert_eq!(reader.read(&mut buf).unwrap(), 1);
        assert_eq!(buf[0], b'X');
        assert_eq!(reader.read(&mut buf).unwrap(), 1);
        assert_eq!(buf[0], b'Y');
        assert_eq!(reader.read(&mut buf).unwrap(), 1);
        assert_eq!(buf[0], b'Z');
        assert_eq!(reader.read(&mut buf).unwrap(), 0); // EOF
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_read_ibs_larger_than_file() {
        // ibs=16, file=5 bytes: single partial read returns 5
        let path = temp_path("ibs_large");
        fs::write(&path, b"HELLO").unwrap();

        let mut reader = Reader::build(&make_config(&path, 16)).unwrap();
        let mut buf = [0u8; 16];
        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 5);
        assert_eq!(&buf[..5], b"HELLO");
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
        fs::remove_file(&path).unwrap();
    }

    // ─── sync ────────────────────────────────────────────────────────────────

    #[test]
    fn test_sync_full_block_is_returned_unchanged() {
        // full block with sync: no padding needed, returns ibs
        let path = temp_path("sync_full");
        fs::write(&path, b"ABCD").unwrap();

        let mut reader = Reader::build(&make_config_sync(&path, 4)).unwrap();
        let mut buf = [0u8; 4];
        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 4);
        assert_eq!(&buf, b"ABCD");
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_sync_partial_block_padded_with_zeros_and_returns_ibs() {
        // file = 2 bytes, ibs = 4; sync pads to ibs and returns ibs
        let path = temp_path("sync_partial");
        fs::write(&path, b"AB").unwrap();

        let mut reader = Reader::build(&make_config_sync(&path, 4)).unwrap();
        let mut buf = [0xFFu8; 4]; // pre-fill with non-zero to confirm padding
        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 4);              // always returns ibs with sync
        assert_eq!(&buf[..2], b"AB");
        assert_eq!(&buf[2..], &[0u8; 2]); // zero-padded tail
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_sync_file_smaller_than_ibs_pads_entire_tail() {
        let path = temp_path("sync_small");
        fs::write(&path, b"HI").unwrap(); // 2 bytes, ibs = 8

        let mut reader = Reader::build(&make_config_sync(&path, 8)).unwrap();
        let mut buf = [0xFFu8; 8];
        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 8);
        assert_eq!(&buf[..2], b"HI");
        assert_eq!(&buf[2..], &[0u8; 6]);
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_sync_eof_returns_zero_not_padded() {
        // EOF sentinel is 0 regardless of sync; must not be converted to ibs
        let path = temp_path("sync_eof");
        fs::write(&path, b"ABCD").unwrap();

        let mut reader = Reader::build(&make_config_sync(&path, 4)).unwrap();
        let mut buf = [0u8; 4];
        reader.read(&mut buf).unwrap();             // drain the file
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_sync_empty_file_returns_zero() {
        let path = temp_path("sync_empty");
        fs::write(&path, b"").unwrap();

        let mut reader = Reader::build(&make_config_sync(&path, 4)).unwrap();
        let mut buf = [0u8; 4];
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_sync_multiple_blocks_last_partial_padded() {
        // 7 bytes, ibs=4: first block full, second block padded
        let path = temp_path("sync_multi");
        fs::write(&path, b"AAAABBB").unwrap();

        let mut reader = Reader::build(&make_config_sync(&path, 4)).unwrap();
        let mut buf = [0xFFu8; 4];

        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 4);
        assert_eq!(&buf, b"AAAA"); // full block, no change

        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 4); // partial padded to ibs
        assert_eq!(&buf[..3], b"BBB");
        assert_eq!(buf[3], 0u8);

        assert_eq!(reader.read(&mut buf).unwrap(), 0); // EOF
        fs::remove_file(&path).unwrap();
    }

    // ─── skip ────────────────────────────────────────────────────────────────

    #[test]
    fn test_skip_zero_reads_from_start() {
        let path = temp_path("skip_zero");
        fs::write(&path, b"ABCDEFGH").unwrap();

        let mut reader = Reader::build(&make_config(&path, 4).skip(0)).unwrap();
        let mut buf = [0u8; 4];
        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 4);
        assert_eq!(&buf, b"ABCD");
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_skip_one_block_skips_first_ibs_bytes() {
        let path = temp_path("skip_one");
        fs::write(&path, b"XXXXDATA").unwrap(); // ibs=4, skip first block

        let mut reader = Reader::build(&make_config(&path, 4).skip(1)).unwrap();
        let mut buf = [0u8; 4];
        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 4);
        assert_eq!(&buf, b"DATA");
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_skip_multiple_blocks_correct_offset() {
        // 16 bytes, ibs=4, skip=2 → read starts at byte 8
        let path = temp_path("skip_multi");
        fs::write(&path, b"AAAABBBBCCCCDDDD").unwrap();

        let mut reader = Reader::build(&make_config(&path, 4).skip(2)).unwrap();
        let mut buf = [0u8; 4];

        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 4);
        assert_eq!(&buf, b"CCCC");

        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 4);
        assert_eq!(&buf, b"DDDD");

        assert_eq!(reader.read(&mut buf).unwrap(), 0); // EOF
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_skip_to_exact_eof_boundary_gives_eof() {
        // skip=2, ibs=4 → skip exactly all 8 bytes → first read is EOF
        let path = temp_path("skip_exact_eof");
        fs::write(&path, b"ABCDEFGH").unwrap();

        let mut reader = Reader::build(&make_config(&path, 4).skip(2)).unwrap();
        let mut buf = [0u8; 4];
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_skip_past_eof_gives_eof() {
        // skip=5 * ibs=4 = 20 bytes, but file is only 4 bytes
        let path = temp_path("skip_past_eof");
        fs::write(&path, b"ABCD").unwrap();

        let mut reader = Reader::build(&make_config(&path, 4).skip(5)).unwrap();
        let mut buf = [0u8; 4];
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_skip_with_sync_pads_first_read_after_skip() {
        // 10 bytes, ibs=4, skip=1, sync=true → first read after skip is 4 bytes,
        // second read is 2 bytes padded to 4
        let path = temp_path("skip_sync");
        fs::write(&path, b"XXXXABCDEF").unwrap();

        let mut reader = Reader::build(
            &make_config(&path, 4).skip(1).write_convs(WriteOps::Sync as u8)
        ).unwrap();
        let mut buf = [0xFFu8; 4];

        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 4);
        assert_eq!(&buf, b"ABCD");

        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 4); // "EF" padded to 4
        assert_eq!(&buf[..2], b"EF");
        assert_eq!(&buf[2..], &[0u8; 2]);

        assert_eq!(reader.read(&mut buf).unwrap(), 0);
        fs::remove_file(&path).unwrap();
    }

    // ─── error cases ─────────────────────────────────────────────────────────

    #[test]
    fn test_nonexistent_file_build_fails() {
        let config = make_config("/nonexistent/ccdd_no_such_file.bin", 512);
        assert!(Reader::build(&config).is_err());
    }

    #[test]
    fn test_skip_overflow_build_fails() {
        // skip * ibs overflows u64; build must fail before trying to open any file
        let overflow_skip = usize::MAX / 2 + 1;
        let config = Config::new()
            .source(SourceType::Standard)
            .input_block_size(2)
            .skip(overflow_skip);
        assert!(Reader::build(&config).is_err());
    }
}
