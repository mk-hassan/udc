use std::fs::{ File, OpenOptions };
use std::io::{ BufWriter, Seek, SeekFrom, Stdout, Write, Error, ErrorKind };

use crate::constants;
use crate::config::{ Config, SourceType };

use crate::pipeline::conv;

pub struct Writer {
	obs: usize,
	is_sparse: bool,
	logical_pos: u64,
	target: TargetWriter,
    to_lower: bool,
    to_upper: bool,
    swap: bool,
}

enum TargetWriter {
    File(BufWriter<File>),
    Stdout(BufWriter<Stdout>),
}

impl Seek for TargetWriter {
	fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
		match self {
			TargetWriter::File(f) => f.seek(pos),
			_ => Ok(0)
		}
	}
}

impl Writer {
	pub fn build(config: &Config) -> Result<Writer, Box<dyn std::error::Error>> {
		let mut target = Writer::build_target(config)?;
		let poisition = match &mut target {
			TargetWriter::File(f) => f.get_mut().stream_position()?,
			_ => 0u64
		};

		let writer = Writer {
			obs: config.get_obs(),
			is_sparse: config.is_sparse(),
			logical_pos: poisition,
			target: target,
            to_lower: config.is_to_lower(),
            to_upper: config.is_to_upper(),
            swap: config.is_swap()
		};

		Ok(writer)
	}

	fn build_target(config: &Config) -> Result<TargetWriter, Box<dyn std::error::Error>> {
		match config.get_destination() {
			SourceType::File(path) => {
				let mut file = OpenOptions::new()
					.create(true)
					.write(true)
					.truncate(config.is_truncate())
					.open(path)?;
				
				let seek_blocks = config.get_seek().unwrap_or(0) as u64;
				match u64::checked_mul(seek_blocks, config.get_obs() as u64) {
					Some(bytes) => file.seek(SeekFrom::Start(bytes))?,
					None => return Err(Box::new(Error::other(constants::SEEK_SIZE_OUT_OF_BOUNDS)))
				};

				Ok(TargetWriter::File(BufWriter::with_capacity(config.get_obs(), file)))
			}
			SourceType::Standard => Ok(
				TargetWriter::Stdout(BufWriter::with_capacity(config.get_obs(), std::io::stdout()))
			)
		}
	}

	pub fn write_all(&mut self, buffer: &mut [u8]) -> Result<usize, Box<dyn std::error::Error>> {
		let mut bytes = self.obs.min(buffer.len());
		
		if self.is_sparse && Self::is_all_zeros(&buffer[..bytes]) {
			let Ok(seeked_bytes) = i64::try_from(bytes) else {
				return Err(Box::new(Error::new(ErrorKind::Other, constants::SEEK_SIZE_OUT_OF_BOUNDS)));
			};
			
			self.target.seek(SeekFrom::Current(seeked_bytes))?;
			self.logical_pos += bytes as u64;
			return Ok(bytes);
		}
		
        bytes = if self.swap && bytes % 2 == 1 { bytes  - 1 } else { bytes };
        self.apply_convs(&mut buffer[..bytes]);
		match &mut self.target {
			TargetWriter::File(f) => f.write_all(&buffer[..bytes])?,
			TargetWriter::Stdout(s) => s.write_all(&buffer[..bytes])?
		}

		self.logical_pos += bytes as u64;
		Ok(bytes)
	}

    fn apply_convs(&self, buffer: &mut [u8]) {
        if self.swap {
            conv::swap(buffer);
        }

        if self.to_lower {
            conv::to_lower(buffer);
        } else if self.to_upper {
            conv::to_upper(buffer);
        }
    }

	pub fn finalize(&mut self) -> Result<(), Error> {
		if self.is_sparse {
			if let TargetWriter::File(f) = &mut self.target {
				f.flush()?;
				f.get_mut().set_len(self.logical_pos)?;
			}
		}
    	Ok(())
	}

	#[inline]
	fn is_all_zeros(buffer: &[u8]) -> bool {
		buffer.iter().all(|&b| b == 0)
	}
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ WriteOps, DataOps };
    use std::fs;

    fn temp_path(name: &str) -> String {
        std::env::temp_dir()
            .join(format!("ccdd_writer_{}.bin", name))
            .to_str()
            .unwrap()
            .to_owned()
    }

    fn make_config(path: &str, obs: usize) -> Config {
        Config::new()
            .destination(SourceType::File(path.to_string()))
            .output_block_size(obs)
    }

    fn make_config_flags(path: &str, obs: usize, flags: u8) -> Config {
        make_config(path, obs).write_convs(flags)
    }

    fn make_config_data(path: &str, obs: usize, data_flags: u8) -> Config {
        make_config(path, obs).data_convs(data_flags)
    }

    fn flush_writer(writer: &mut Writer) {
        if let TargetWriter::File(f) = &mut writer.target {
            f.flush().unwrap();
        }
    }

    // ─── basic write / truncate ───────────────────────────────────────────────

    #[test]
    fn test_write_creates_file_with_correct_content() {
        let path = temp_path("basic");
        let _ = fs::remove_file(&path);

        let mut writer = Writer::build(&make_config(&path, 512)).unwrap();
        writer.write_all( &mut b"Hello, dd!".to_vec()).unwrap();
        flush_writer(&mut writer);

        assert_eq!(fs::read(&path).unwrap(), b"Hello, dd!");
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_obs_limits_bytes_per_write_all_call() {
        let path = temp_path("obs_limit");
        let _ = fs::remove_file(&path);

        // obs=4: write_all on a larger buffer writes at most 4 bytes per call
        let mut writer = Writer::build(&make_config(&path, 4)).unwrap();
        let n = writer.write_all(&mut b"0123456789".to_vec()).unwrap();
        assert_eq!(n, 4);
        assert_eq!(writer.logical_pos, 4);

        let n = writer.write_all(&mut b"0123456789"[4..8].to_vec()).unwrap();
        assert_eq!(n, 4);
        assert_eq!(writer.logical_pos, 8);
        flush_writer(&mut writer);

        assert_eq!(fs::read(&path).unwrap(), b"01234567");
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_truncate_clears_existing_file() {
        let path = temp_path("truncate");
        fs::write(&path, b"EXISTING_LONG_CONTENT").unwrap();

        // truncate=true (default): file is wiped on open, only new data remains
        let mut writer = Writer::build(&make_config(&path, 512)).unwrap();
        writer.write_all(&mut b"NEW".to_vec()).unwrap();
        flush_writer(&mut writer);

        assert_eq!(fs::read(&path).unwrap(), b"NEW");
        fs::remove_file(&path).unwrap();
    }

    // ─── notrunc ─────────────────────────────────────────────────────────────

    #[test]
    fn test_notrunc_preserves_bytes_beyond_written_region() {
        let path = temp_path("notrunc");
        fs::write(&path, b"ABCDEFGH").unwrap();

        // notrunc, obs=4: overwrite first 4 bytes, last 4 must survive
        let mut writer = Writer::build(&make_config_flags(
            &path, 4, WriteOps::NoTrunc as u8,
        )).unwrap();
        writer.write_all(&mut b"1234".to_vec()).unwrap();
        flush_writer(&mut writer);

        assert_eq!(fs::read(&path).unwrap(), b"1234EFGH");
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_notrunc_multiple_blocks_partial_overwrite() {
        let path = temp_path("notrunc_multi");
        fs::write(&path, b"ABCDEFGHIJKLMNOP").unwrap(); // 16 bytes

        // notrunc, obs=4: two 4-byte writes overwrite first 8 bytes, last 8 untouched
        let mut writer = Writer::build(&make_config_flags(
            &path, 4, WriteOps::NoTrunc as u8,
        )).unwrap();
        writer.write_all(&mut b"12341234".to_vec()).unwrap(); // obs clamps to first 4: "1234"
        writer.write_all(&mut b"56785678".to_vec()).unwrap(); // obs clamps to first 4: "5678"
        flush_writer(&mut writer);

        assert_eq!(fs::read(&path).unwrap(), b"12345678IJKLMNOP");
        fs::remove_file(&path).unwrap();
    }

    // ─── seek ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_seek_writes_at_correct_byte_offset() {
        let path = temp_path("seek");
        let _ = fs::remove_file(&path);

        // seek=2, obs=4 → skip 8 bytes; write lands at byte offset 8
        let config = make_config(&path, 4).seek(2);
        let mut writer = Writer::build(&config).unwrap();
        writer.write_all(&mut b"DATA".to_vec()).unwrap();
        flush_writer(&mut writer);

        let content = fs::read(&path).unwrap();
        assert_eq!(content.len(), 12);
        assert!(content[..8].iter().all(|&b| b == 0)); // null-padded prefix
        assert_eq!(&content[8..12], b"DATA");
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_seek_logical_pos_starts_at_byte_offset() {
        let path = temp_path("seek_logpos");
        let _ = fs::remove_file(&path);

        // seek=3, obs=4 → logical_pos starts at 12
        let config = make_config(&path, 4).seek(3);
        let writer = Writer::build(&config).unwrap();
        assert_eq!(writer.logical_pos, 12);

        drop(writer);
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_seek_notrunc_preserves_bytes_before_offset() {
        let path = temp_path("seek_notrunc");
        fs::write(&path, b"ABCDEFGH").unwrap();

        // notrunc + seek=1 + obs=4 → cursor at byte 4, first 4 bytes untouched
        let config = make_config_flags(&path, 4, WriteOps::NoTrunc as u8).seek(1);
        let mut writer = Writer::build(&config).unwrap();
        assert_eq!(writer.logical_pos, 4);
        writer.write_all(&mut b"1234".to_vec()).unwrap();
        flush_writer(&mut writer);

        assert_eq!(fs::read(&path).unwrap(), b"ABCD1234");
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_seek_zero_is_noop() {
        let path = temp_path("seek_zero");
        let _ = fs::remove_file(&path);

        let config = make_config(&path, 4).seek(0);
        let mut writer = Writer::build(&config).unwrap();
        assert_eq!(writer.logical_pos, 0);
        writer.write_all(&mut b"TEST".to_vec()).unwrap();
        flush_writer(&mut writer);

        assert_eq!(fs::read(&path).unwrap(), b"TEST");
        fs::remove_file(&path).unwrap();
    }

    // ─── sparse ───────────────────────────────────────────────────────────────

    #[test]
    fn test_sparse_zero_block_advances_logical_pos_without_writing() {
        let path = temp_path("sparse_zeros");
        let _ = fs::remove_file(&path);

        let mut writer = Writer::build(&make_config_flags(
            &path, 4, WriteOps::Sparse as u8,
        )).unwrap();

        let n = writer.write_all(&mut [0u8; 4]).unwrap();
        assert_eq!(n, 4);
        assert_eq!(writer.logical_pos, 4);

        // no bytes have been written through the BufWriter — file remains empty
        flush_writer(&mut writer);
        assert_eq!(fs::metadata(&path).unwrap().len(), 0);

        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_sparse_nonzero_block_writes_normally() {
        let path = temp_path("sparse_nonzero");
        let _ = fs::remove_file(&path);

        let mut writer = Writer::build(&make_config_flags(
            &path, 4, WriteOps::Sparse as u8,
        )).unwrap();
        writer.write_all(&mut b"DATA".to_vec()).unwrap();
        assert_eq!(writer.logical_pos, 4);
        flush_writer(&mut writer);

        assert_eq!(fs::read(&path).unwrap(), b"DATA");
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_sparse_finalize_extends_file_to_logical_pos() {
        let path = temp_path("sparse_finalize");
        let _ = fs::remove_file(&path);

        // data block then zero block → logical_pos=8, only 4 bytes written before finalize
        let mut writer = Writer::build(&make_config_flags(
            &path, 4, WriteOps::Sparse as u8,
        )).unwrap();
        writer.write_all(&mut b"DATA".to_vec()).unwrap();    // written: 0..4
        writer.write_all(&mut [0u8; 4]).unwrap();  // sparse skip: 4..8
        assert_eq!(writer.logical_pos, 8);

        writer.finalize().unwrap();
        drop(writer);

        let content = fs::read(&path).unwrap();
        assert_eq!(content.len(), 8);
        assert_eq!(&content[..4], b"DATA");
        assert_eq!(&content[4..8], &[0u8; 4]);
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_sparse_mixed_data_zeros_data_produces_correct_file() {
        let path = temp_path("sparse_mixed");
        let _ = fs::remove_file(&path);

        // HEAD, zero hole, TAIL — classic sparse pattern
        let mut writer = Writer::build(&make_config_flags(
            &path, 4, WriteOps::Sparse as u8,
        )).unwrap();
        writer.write_all(&mut b"HEAD".to_vec()).unwrap();   // 0..4  written
        writer.write_all(&mut [0u8; 4]).unwrap(); // 4..8  sparse (seek, not written)
        writer.write_all(&mut b"TAIL".to_vec()).unwrap();   // 8..12 written
        assert_eq!(writer.logical_pos, 12);

        writer.finalize().unwrap();
        drop(writer);

        let content = fs::read(&path).unwrap();
        assert_eq!(content.len(), 12);
        assert_eq!(&content[0..4], b"HEAD");
        assert_eq!(&content[4..8], &[0u8; 4]);
        assert_eq!(&content[8..12], b"TAIL");
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_sparse_trailing_zeros_finalize_extends_to_full_size() {
        let path = temp_path("sparse_trail");
        let _ = fs::remove_file(&path);

        // data followed by multiple sparse zero blocks
        let mut writer = Writer::build(&make_config_flags(
            &path, 4, WriteOps::Sparse as u8,
        )).unwrap();
        writer.write_all(&mut b"KEEP".to_vec()).unwrap();    // 0..4
        writer.write_all(&mut [0u8; 4]).unwrap();  // 4..8  sparse
        writer.write_all(&mut [0u8; 4]).unwrap();  // 8..12 sparse
        assert_eq!(writer.logical_pos, 12);

        writer.finalize().unwrap();
        drop(writer);

        assert_eq!(fs::metadata(&path).unwrap().len(), 12);
        let content = fs::read(&path).unwrap();
        assert_eq!(&content[..4], b"KEEP");
        assert!(content[4..].iter().all(|&b| b == 0));
        fs::remove_file(&path).unwrap();
    }

    // ─── logical_pos tracking ─────────────────────────────────────────────────

    #[test]
    fn test_logical_pos_starts_at_zero_by_default() {
        let path = temp_path("logpos_zero");
        let _ = fs::remove_file(&path);

        let writer = Writer::build(&make_config(&path, 512)).unwrap();
        assert_eq!(writer.logical_pos, 0);

        drop(writer);
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_logical_pos_accumulates_across_multiple_writes() {
        let path = temp_path("logpos_accum");
        let _ = fs::remove_file(&path);

        let mut writer = Writer::build(&make_config(&path, 4)).unwrap();
        writer.write_all(&mut b"AAAA".to_vec()).unwrap();
        assert_eq!(writer.logical_pos, 4);
        writer.write_all(&mut b"BBBB".to_vec()).unwrap();
        assert_eq!(writer.logical_pos, 8);
        writer.write_all(&mut b"CCCC".to_vec()).unwrap();
        assert_eq!(writer.logical_pos, 12);

        flush_writer(&mut writer);
        assert_eq!(fs::metadata(&path).unwrap().len(), 12);
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_logical_pos_clamped_to_obs_per_call() {
        let path = temp_path("logpos_clamp");
        let _ = fs::remove_file(&path);

        // obs=4: even if the buffer is larger, logical_pos only advances by obs
        let mut writer = Writer::build(&make_config(&path, 4)).unwrap();
        writer.write_all(&mut b"0123456789".to_vec()).unwrap();
        assert_eq!(writer.logical_pos, 4);

        drop(writer);
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_logical_pos_advances_on_sparse_zero_block() {
        let path = temp_path("logpos_sparse");
        let _ = fs::remove_file(&path);

        let mut writer = Writer::build(&make_config_flags(
            &path, 4, WriteOps::Sparse as u8,
        )).unwrap();
        writer.write_all(&mut [0u8; 4]).unwrap();
        assert_eq!(writer.logical_pos, 4);
        writer.write_all(&mut [0u8; 2]).unwrap();
        assert_eq!(writer.logical_pos, 6);

        drop(writer);
        fs::remove_file(&path).unwrap();
    }

	#[test]
    fn test_logical_pos_advances_on_sparse_zero_block_with_zero_length() {
        let path = temp_path("logpos_sparse_zero");
        let _ = fs::remove_file(&path);

        let mut writer = Writer::build(&make_config_flags(
            &path, 4, WriteOps::Sparse as u8,
        )).unwrap();
        writer.write_all(&mut [0u8; 0]).unwrap();
        assert_eq!(writer.logical_pos, 0);
        writer.write_all(&mut [0u8; 2]).unwrap();
        assert_eq!(writer.logical_pos, 2);

        drop(writer);
        fs::remove_file(&path).unwrap();
    }

    // ─── conv: lcase / ucase / swap ───────────────────────────────────────────

    #[test]
    fn test_lcase_converts_to_lowercase() {
        let path = temp_path("lcase");
        let _ = fs::remove_file(&path);

        let mut writer = Writer::build(&make_config_data(&path, 512, DataOps::ToLower as u8)).unwrap();
        writer.write_all(&mut b"Hello, World!".to_vec()).unwrap();
        flush_writer(&mut writer);

        assert_eq!(fs::read(&path).unwrap(), b"hello, world!");
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_ucase_converts_to_uppercase() {
        let path = temp_path("ucase");
        let _ = fs::remove_file(&path);

        let mut writer = Writer::build(&make_config_data(&path, 512, DataOps::ToUpper as u8)).unwrap();
        writer.write_all(&mut b"Hello, World!".to_vec()).unwrap();
        flush_writer(&mut writer);

        assert_eq!(fs::read(&path).unwrap(), b"HELLO, WORLD!");
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_swap_even_length_swaps_adjacent_pairs() {
        let path = temp_path("swap_even");
        let _ = fs::remove_file(&path);

        let mut writer = Writer::build(&make_config_data(&path, 512, DataOps::Swap as u8)).unwrap();
        writer.write_all(&mut b"ABCDEFGH".to_vec()).unwrap();
        flush_writer(&mut writer);

        assert_eq!(fs::read(&path).unwrap(), b"BADCFEHG");
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_swap_odd_length_truncates_last_byte() {
        let path = temp_path("swap_odd");
        let _ = fs::remove_file(&path);

        // obs=5, input=5 bytes → odd → truncated to 4 before swap → only 4 bytes written
        let mut writer = Writer::build(&make_config_data(&path, 5, DataOps::Swap as u8)).unwrap();
        writer.write_all(&mut b"ABCDE".to_vec()).unwrap();
        flush_writer(&mut writer);

        assert_eq!(fs::read(&path).unwrap(), b"BADC");
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_swap_then_lcase_applied_in_order() {
        let path = temp_path("swap_lcase");
        let _ = fs::remove_file(&path);

        // swap first: "ABCD" → "BADC", then lcase: "badc"
        let flags = DataOps::Swap as u8 | DataOps::ToLower as u8;
        let mut writer = Writer::build(&make_config_data(&path, 512, flags)).unwrap();
        writer.write_all(&mut b"ABCD".to_vec()).unwrap();
        flush_writer(&mut writer);

        assert_eq!(fs::read(&path).unwrap(), b"badc");
        fs::remove_file(&path).unwrap();
    }
}