use std::error::Error;
use std::fmt::{ Display };
use std::time::{Duration, Instant};

mod conv;
mod reader;
mod writer;

use crate::config::{ Config };
use reader::Reader;
use writer::Writer;

pub struct Metrics {
    total_bytes: usize,
    read_blocks: usize,
    read_partials: usize,
    write_blocks: usize,
    write_partials: usize,
    time_duration: Duration,
}	

impl Metrics {
	pub fn new() -> Self {
		Metrics {
			total_bytes: 0,
			read_blocks: 0,
			read_partials: 0,
			write_blocks: 0,
			write_partials: 0,
			time_duration: Duration::new(0, 0),
		}
	}
}

impl Display for Metrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let secs = self.time_duration.as_secs_f64();
        let mb_per_sec = (self.total_bytes as f64) / (1024.0 * 1024.0) / secs;
        writeln!(f, "{}+{} records in", self.read_blocks, self.read_partials)?;
        writeln!(
            f,
            "{}+{} records out",
            self.write_blocks, self.write_partials
        )?;
        write!(
            f,
            "{} bytes copied, {:.6} s, {:.2} MB/s",
            self.total_bytes, secs, mb_per_sec
        )
    }
}

pub fn run(config: &Config) -> Result<Metrics, Box<dyn Error>> {
    let start = Instant::now();
	
    let mut reader = Reader::build(config)?;
    let mut writer = Writer::build(config)?;
    
	let ibs = config.get_ibs();
    let obs = config.get_obs();
    
	let count = config.get_count();
	let mut blocks_counter = 0usize;
    
    let mut buffer = vec![0u8; ibs];
	let mut accum: Vec<u8> = Vec::new();
    
    let mut metrics = Metrics::new();
    loop {
        let reads = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => n,
            Err(error) => {
                if !config.is_noerror() {
                    return Err(error);
                }
                
                eprintln!("ccdd: read: error reading block {}", blocks_counter + 1);
                if config.is_sync() { ibs } else { 0 }
            }
        };
        
        // skipped block, noerror=true + sync=false
        if reads == 0 { continue }

		blocks_counter += 1;
		
        if reads == ibs { metrics.read_blocks += 1; }
		else { metrics.read_partials += 1; }
        metrics.total_bytes += reads;
		
		accum.extend_from_slice(&buffer[..reads]);
        while accum.len() >= obs {
			let bytes = writer.write_all(&accum)?;
            accum.drain(..bytes);
            metrics.write_blocks += 1;
        }
		
		if count.is_some_and(|c| blocks_counter >= c) { break; }
    }

    if !accum.is_empty() {
        writer.write_all(&accum)?;
        metrics.write_partials += 1;
    }

    writer.finalize()?;

    metrics.time_duration = start.elapsed();
    Ok(metrics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SourceType;
    use crate::config::WriteOps;
    use std::fs;

    /// Runs the pipeline with the given write_convs flags; returns (Metrics, output bytes).
    fn run_with_flags(
        test_name: &str,
        input: &[u8],
        ibs: usize,
        obs: usize,
        skip: Option<usize>,
        seek: Option<usize>,
        count: Option<usize>,
        flags: u8,
    ) -> (Metrics, Vec<u8>) {
        let dir = std::env::temp_dir();
        let in_path = dir.join(format!("ccdd_{}_in.bin", test_name));
        let out_path = dir.join(format!("ccdd_{}_out.bin", test_name));

        fs::write(&in_path, input).unwrap();

        let mut config = Config::new();
        config = config
            .source(SourceType::File(in_path.to_str().unwrap().to_string()))
            .destination(SourceType::File(out_path.to_str().unwrap().to_string()))
            .input_block_size(ibs)
            .output_block_size(obs)
            .write_convs(flags);
        if let Some(s) = skip  { config = config.skip(s); }
        if let Some(s) = seek  { config = config.seek(s); }
        if let Some(c) = count { config = config.count(c); }

        let metrics = run(&config).unwrap();
        let output = fs::read(&out_path).unwrap();

        let _ = fs::remove_file(&in_path);
        let _ = fs::remove_file(&out_path);
        (metrics, output)
    }

    /// Convenience wrapper: no write_convs flags.
    fn run_transform(
        test_name: &str,
        input: &[u8],
        ibs: usize,
        obs: usize,
        skip: Option<usize>,
        seek: Option<usize>,
        count: Option<usize>,
    ) -> (Metrics, Vec<u8>) {
        run_with_flags(test_name, input, ibs, obs, skip, seek, count, 0)
    }

    // ── content correctness ──────────────────────────────────────────────────

    #[test]
    fn test_empty_file() {
        // read() returns Ok(0) immediately → no blocks, no writes
        let (m, out) = run_transform("empty", &[], 512, 512, None, None, None);
        assert_eq!(&[] as &[u8], out.as_slice(), "[empty] content mismatch");
        assert_eq!(m.total_bytes,   0);
        assert_eq!(m.read_blocks,   0);
        assert_eq!(m.read_partials, 0);
        assert_eq!(m.write_blocks,  0);
        assert_eq!(m.write_partials,0);
    }

    #[test]
    fn test_single_byte() {
        // 1 byte < ibs=512 → partial read + partial write
        let (m, out) = run_transform("single_byte", &[0xAB], 512, 512, None, None, None);
        assert_eq!(&[0xAB], out.as_slice(), "[single_byte] content mismatch");
        assert_eq!(m.total_bytes,    1);
        assert_eq!(m.read_blocks,    0);
        assert_eq!(m.read_partials,  1);
        assert_eq!(m.write_blocks,   0);
        assert_eq!(m.write_partials, 1);
    }

    #[test]
    fn test_all_256_byte_values_preserved() {
        // Every possible byte value survives the copy unchanged
        let input: Vec<u8> = (0u8..=255).collect();
        let (_, out) = run_transform("all_bytes", &input, 64, 64, None, None, None);
        assert_eq!(input.as_slice(), out.as_slice());
    }

    // ── ibs == obs ───────────────────────────────────────────────────────────

    #[test]
    fn test_exact_multiple_of_block_size() {
        // 1024 bytes, ibs=obs=512 → 2 full reads, 2 full writes, no partials
        let input: Vec<u8> = (0..1024).map(|i: u16| (i % 256) as u8).collect();
        let (m, out) = run_transform("exact_mult", &input, 512, 512, None, None, None);
        assert_eq!(input.as_slice(), out.as_slice(), "[exact_mult] content mismatch");
        assert_eq!(m.total_bytes,    1024);
        assert_eq!(m.read_blocks,    2);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   2);
        assert_eq!(m.write_partials, 0);
    }

    #[test]
    fn test_not_multiple_of_block_size() {
        // 1000 bytes, ibs=obs=512 → 1 full + 1 partial on both sides
        let input: Vec<u8> = (0..1000).map(|i: u16| (i % 256) as u8).collect();
        let (m, out) = run_transform("not_mult", &input, 512, 512, None, None, None);
        assert_eq!(input.as_slice(), out.as_slice(), "[not_mult] content mismatch");
        assert_eq!(m.total_bytes,    1000);
        assert_eq!(m.read_blocks,    1);
        assert_eq!(m.read_partials,  1);
        assert_eq!(m.write_blocks,   1);
        assert_eq!(m.write_partials, 1);
    }

    #[test]
    fn test_file_size_exactly_ibs() {
        // File == ibs → 1 full read, 1 full write, 0 partials
        let input = vec![0xAA; 512];
        let (m, out) = run_transform("eq_ibs", &input, 512, 512, None, None, None);
        assert_eq!(input.as_slice(), out.as_slice(), "[eq_ibs] content mismatch");
        assert_eq!(m.total_bytes,    512);
        assert_eq!(m.read_blocks,    1);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   1);
        assert_eq!(m.write_partials, 0);
    }

    #[test]
    fn test_file_smaller_than_ibs() {
        // 100 bytes, ibs=512 → single partial read + single partial write
        let input = vec![0xBB; 100];
        let (m, out) = run_transform("lt_ibs", &input, 512, 512, None, None, None);
        assert_eq!(input.as_slice(), out.as_slice(), "[lt_ibs] content mismatch");
        assert_eq!(m.total_bytes,    100);
        assert_eq!(m.read_blocks,    0);
        assert_eq!(m.read_partials,  1);
        assert_eq!(m.write_blocks,   0);
        assert_eq!(m.write_partials, 1);
    }

    // ── ibs < obs (accumulation) ─────────────────────────────────────────────

    #[test]
    fn test_ibs_less_than_obs() {
        // 1000 bytes, ibs=200, obs=512
        // 5 full reads of 200; after read3 accum=600 → write 512; end accum=488 → partial
        let input: Vec<u8> = (0..1000).map(|i: u16| (i % 256) as u8).collect();
        let (m, out) = run_transform("ibs_lt_obs", &input, 200, 512, None, None, None);
        assert_eq!(input.as_slice(), out.as_slice(), "[ibs_lt_obs] content mismatch");
        assert_eq!(m.total_bytes,    1000);
        assert_eq!(m.read_blocks,    5);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   1);
        assert_eq!(m.write_partials, 1);
    }

    #[test]
    fn test_obs_larger_than_whole_file() {
        // obs > file size → accum never reaches threshold → single partial write
        let input = vec![0xCC; 100];
        let (m, out) = run_transform("obs_gt_file", &input, 512, 1024, None, None, None);
        assert_eq!(input.as_slice(), out.as_slice(), "[obs_gt_file] content mismatch");
        assert_eq!(m.total_bytes,    100);
        assert_eq!(m.write_blocks,   0);
        assert_eq!(m.write_partials, 1);
    }

    // ── ibs > obs (multiple writes per read) ────────────────────────────────

    #[test]
    fn test_ibs_greater_than_obs() {
        // 1000 bytes, ibs=512, obs=200
        // read1=512 → write 200+200, accum=112
        // read2=488 → accum=600 → write 200+200+200, accum=0
        // write_blocks=5, write_partials=0
        let input: Vec<u8> = (0..1000).map(|i: u16| (i % 256) as u8).collect();
        let (m, out) = run_transform("ibs_gt_obs", &input, 512, 200, None, None, None);
        assert_eq!(input.as_slice(), out.as_slice(), "[ibs_gt_obs] content mismatch");
        assert_eq!(m.total_bytes,    1000);
        assert_eq!(m.read_blocks,    1);
        assert_eq!(m.read_partials,  1);
        assert_eq!(m.write_blocks,   5);
        assert_eq!(m.write_partials, 0);
    }

    // ── extreme block sizes ──────────────────────────────────────────────────

    #[test]
    fn test_byte_by_byte() {
        // ibs=1, obs=1 → every byte is a full read block and full write block
        let (m, out) = run_transform("byte_by_byte", b"hello", 1, 1, None, None, None);
        assert_eq!(b"hello" as &[u8], out.as_slice(), "[byte_by_byte] content mismatch");
        assert_eq!(m.total_bytes,    5);
        assert_eq!(m.read_blocks,    5);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   5);
        assert_eq!(m.write_partials, 0);
    }

    #[test]
    fn test_ibs_1_obs_larger_than_file() {
        // ibs=1, obs=100, file=50 bytes
        // 50 full read blocks (ibs=1); accum never reaches 100 → single partial write
        let input = vec![0x42; 50];
        let (m, out) = run_transform("ibs1_obs100", &input, 1, 100, None, None, None);
        assert_eq!(input.as_slice(), out.as_slice(), "[ibs1_obs100] content mismatch");
        assert_eq!(m.total_bytes,    50);
        assert_eq!(m.read_blocks,    50);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   0);
        assert_eq!(m.write_partials, 1);
    }

    #[test]
    fn test_large_file_content_integrity() {
        // 1 MB, ibs=4096, obs=4096 — no byte lost or corrupted
        let input: Vec<u8> = (0..1024 * 1024).map(|i: u32| (i % 256) as u8).collect();
        let (m, out) = run_transform("large_1mb", &input, 4096, 4096, None, None, None);
        assert_eq!(input.as_slice(), out.as_slice(), "[large_1mb] content mismatch");
        assert_eq!(m.total_bytes, 1024 * 1024);
    }

    // ── skip ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_skip_one_block() {
        // skip=1, ibs=512 → discard first 512 bytes; output = bytes 512..1024
        let input: Vec<u8> = (0..1024u16).map(|i| (i % 256) as u8).collect();
        let (m, out) = run_transform("skip_one_block", &input, 512, 512, Some(1), None, None);
        assert_eq!(out, &input[512..]);
        assert_eq!(m.total_bytes,    512);
        assert_eq!(m.read_blocks,    1);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   1);
        assert_eq!(m.write_partials, 0);
    }

    #[test]
    fn test_skip_past_end_of_file() {
        // skip=3 on a 2-block file → all input discarded, output is empty
        let input = vec![0xAA; 1024];
        let (m, out) = run_transform("skip_past_eof", &input, 512, 512, Some(3), None, None);
        assert_eq!(out, &[] as &[u8]);
        assert_eq!(m.total_bytes,    0);
        assert_eq!(m.read_blocks,    0);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   0);
        assert_eq!(m.write_partials, 0);
    }

    #[test]
    fn test_skip_with_ibs_less_than_obs() {
        // ibs=100, obs=500, skip=3 → discard 300 bytes, copy bytes 300..1000 (700 bytes)
        // 7 full reads; accum: after read5 (500 bytes total) → write 500; end accum=200 → partial
        let input: Vec<u8> = (0..1000u16).map(|i| (i % 256) as u8).collect();
        let (m, out) = run_transform("skip_ibs_lt_obs", &input, 100, 500, Some(3), None, None);
        assert_eq!(out, &input[300..]);
        assert_eq!(m.total_bytes,    700);
        assert_eq!(m.read_blocks,    7);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   1);
        assert_eq!(m.write_partials, 1);
    }

    #[test]
    fn test_skip_partial_block_boundary() {
        // ibs=300, obs=300, skip=3 → discard 900 bytes, 100 bytes remain (partial block)
        let input: Vec<u8> = (0..1000u16).map(|i| (i % 256) as u8).collect();
        let (m, out) = run_transform("skip_partial_boundary", &input, 300, 300, Some(3), None, None);
        assert_eq!(out, &input[900..]);
        assert_eq!(m.total_bytes,    100);
        assert_eq!(m.read_blocks,    0);
        assert_eq!(m.read_partials,  1);
        assert_eq!(m.write_blocks,   0);
        assert_eq!(m.write_partials, 1);
    }

    // ── count ────────────────────────────────────────────────────────────────

    #[test]
    fn test_count_one_block() {
        // count=1, ibs=512, obs=512 → copy only the first 512 bytes
        let input: Vec<u8> = (0..1024u16).map(|i| (i % 256) as u8).collect();
        let (m, out) = run_transform("count_one_block", &input, 512, 512, None, None, Some(1));
        assert_eq!(out, &input[..512]);
        assert_eq!(m.total_bytes,    512);
        assert_eq!(m.read_blocks,    1);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   1);
        assert_eq!(m.write_partials, 0);
    }

    #[test]
    fn test_count_exceeds_file() {
        // count=10 but file has only 2 full blocks → limit never triggers, full copy
        let input: Vec<u8> = (0..1024u16).map(|i| (i % 256) as u8).collect();
        let (m, out) = run_transform("count_exceeds_file", &input, 512, 512, None, None, Some(10));
        assert_eq!(out, input.as_slice());
        assert_eq!(m.total_bytes,    1024);
        assert_eq!(m.read_blocks,    2);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   2);
        assert_eq!(m.write_partials, 0);
    }

    #[test]
    fn test_count_with_ibs_less_than_obs() {
        // ibs=100, obs=300, count=2 → copy 200 bytes; accum never reaches obs → 1 partial write
        let input: Vec<u8> = (0..1000u16).map(|i| (i % 256) as u8).collect();
        let (m, out) = run_transform("count_ibs_lt_obs", &input, 100, 300, None, None, Some(2));
        assert_eq!(out, &input[..200]);
        assert_eq!(m.total_bytes,    200);
        assert_eq!(m.read_blocks,    2);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   0);
        assert_eq!(m.write_partials, 1);
    }

    #[test]
    fn test_count_with_ibs_greater_than_obs() {
        // ibs=400, obs=200, count=1 → copy 400 bytes → exactly 2 full writes, no partial
        let input: Vec<u8> = (0..1000u16).map(|i| (i % 256) as u8).collect();
        let (m, out) = run_transform("count_ibs_gt_obs", &input, 400, 200, None, None, Some(1));
        assert_eq!(out, &input[..400]);
        assert_eq!(m.total_bytes,    400);
        assert_eq!(m.read_blocks,    1);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   2);
        assert_eq!(m.write_partials, 0);
    }

    // ── seek ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_seek_one_block() {
        // seek=1, obs=512 → 512 zero bytes prepended, then 512 bytes of input
        let input = vec![0xAA; 512];
        let (m, out) = run_transform("seek_one_block", &input, 512, 512, None, Some(1), None);
        assert_eq!(out.len(), 1024);
        assert!(out[..512].iter().all(|&b| b == 0));
        assert!(out[512..].iter().all(|&b| b == 0xAA));
        assert_eq!(m.total_bytes,    512);
        assert_eq!(m.read_blocks,    1);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   1);
        assert_eq!(m.write_partials, 0);
    }

    #[test]
    fn test_seek_with_empty_input() {
        // seek=2, obs=256, no input → only the 512 zero bytes written by seek
        let (m, out) = run_transform("seek_empty_input", &[], 512, 256, None, Some(2), None);
        assert_eq!(out.len(), 0);
        assert_eq!(m.total_bytes,    0);
        assert_eq!(m.read_blocks,    0);
        assert_eq!(m.write_blocks,   0);
        assert_eq!(m.write_partials, 0);
    }

    #[test]
    fn test_seek_with_ibs_less_than_obs() {
        // seek=1, ibs=100, obs=300 → 300 zeros then all 1000 input bytes
        // 10 full reads; accum: write at 300, 600, 900 bytes; end accum=100 → partial
        let input: Vec<u8> = (0..1000u16).map(|i| (i % 256) as u8).collect();
        let (m, out) = run_transform("seek_ibs_lt_obs", &input, 100, 300, None, Some(1), None);
        assert_eq!(out.len(), 1300);
        assert!(out[..300].iter().all(|&b| b == 0));
        assert_eq!(&out[300..], input.as_slice());
        assert_eq!(m.total_bytes,    1000);
        assert_eq!(m.read_blocks,    10);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   3);
        assert_eq!(m.write_partials, 1);
    }

    #[test]
    fn test_seek_multiple_blocks() {
        // seek=3, obs=100 → 300 zero bytes prepended, then 50 bytes of 0xFF
        let input = vec![0xFF; 50];
        let (m, out) = run_transform("seek_multiple_blocks", &input, 512, 100, None, Some(3), None);
        assert_eq!(out.len(), 350);
        assert!(out[..300].iter().all(|&b| b == 0));
        assert!(out[300..].iter().all(|&b| b == 0xFF));
        assert_eq!(m.total_bytes,    50);
        assert_eq!(m.read_blocks,    0);
        assert_eq!(m.read_partials,  1);
        assert_eq!(m.write_blocks,   0);
        assert_eq!(m.write_partials, 1);
    }

    // ── combinations ─────────────────────────────────────────────────────────

    #[test]
    fn test_skip_and_count() {
        // skip=1, count=1, ibs=512, obs=512 → copy only block 2 (bytes 512..1024)
        let input: Vec<u8> = (0..1536u16).map(|i| (i % 256) as u8).collect();
        let (m, out) = run_transform("skip_and_count", &input, 512, 512, Some(1), None, Some(1));
        assert_eq!(out, &input[512..1024]);
        assert_eq!(m.total_bytes,    512);
        assert_eq!(m.read_blocks,    1);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   1);
        assert_eq!(m.write_partials, 0);
    }

    #[test]
    fn test_skip_and_seek() {
        // skip=1, seek=1, ibs=512, obs=512
        // output = 512 zeros (seek) followed by bytes 512..1024 (after skip)
        let input: Vec<u8> = (0..1024u16).map(|i| (i % 256) as u8).collect();
        let (m, out) = run_transform("skip_and_seek", &input, 512, 512, Some(1), Some(1), None);
        assert_eq!(out.len(), 1024);
        assert!(out[..512].iter().all(|&b| b == 0));
        assert_eq!(&out[512..], &input[512..]);
        assert_eq!(m.total_bytes,    512);
        assert_eq!(m.read_blocks,    1);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   1);
        assert_eq!(m.write_partials, 0);
    }

    #[test]
    fn test_seek_and_count() {
        // seek=2, count=1, ibs=512, obs=512
        // output = 1024 zeros (seek) followed by only the first block of input
        let input: Vec<u8> = (0..1024u16).map(|i| (i % 256) as u8).collect();
        let (m, out) = run_transform("seek_and_count", &input, 512, 512, None, Some(2), Some(1));
        assert_eq!(out.len(), 1536);
        assert!(out[..1024].iter().all(|&b| b == 0));
        assert_eq!(&out[1024..], &input[..512]);
        assert_eq!(m.total_bytes,    512);
        assert_eq!(m.read_blocks,    1);
        assert_eq!(m.write_blocks,   1);
    }

    #[test]
    fn test_skip_seek_count() {
        // skip=1, seek=2, count=1, ibs=512, obs=512
        // output = 1024 zeros (seek) followed by block 2 of input (bytes 512..1024)
        let input: Vec<u8> = (0..1536u16).map(|i| (i % 256) as u8).collect();
        let (m, out) = run_transform("skip_seek_count", &input, 512, 512, Some(1), Some(2), Some(1));
        assert_eq!(out.len(), 1536);
        assert!(out[..1024].iter().all(|&b| b == 0));
        assert_eq!(&out[1024..], &input[512..1024]);
        assert_eq!(m.total_bytes,    512);
        assert_eq!(m.read_blocks,    1);
        assert_eq!(m.write_blocks,   1);
    }

    #[test]
    fn test_skip_count_ibs_lt_obs() {
        // ibs=100, obs=600, skip=5, count=10
        // skip 500 bytes; copy 10 blocks × 100 = 1000 bytes (bytes 500..1500)
        // accum: after read6 (600 bytes) → write 600; end accum=400 → partial
        let input: Vec<u8> = (0..2000u16).map(|i| (i % 256) as u8).collect();
        let (m, out) = run_transform("skip_count_ibs_lt_obs", &input, 100, 600, Some(5), None, Some(10));
        assert_eq!(out, &input[500..1500]);
        assert_eq!(m.total_bytes,    1000);
        assert_eq!(m.read_blocks,    10);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   1);
        assert_eq!(m.write_partials, 1);
    }

    #[test]
    fn test_seek_skip_count_ibs_gt_obs() {
        // ibs=600, obs=200, skip=2, count=2, seek=1
        // seek: 200 zeros; skip 1200 bytes; copy 2 blocks × 600 = 1200 bytes (bytes 1200..2400)
        // each 600-byte read fills accum → 3 full obs writes; 2 reads → write_blocks=6, no partial
        let input: Vec<u8> = (0..3000u16).map(|i| (i % 256) as u8).collect();
        let (m, out) = run_transform("seek_skip_count_ibs_gt_obs", &input, 600, 200, Some(2), Some(1), Some(2));
        assert_eq!(out.len(), 1400);
        assert!(out[..200].iter().all(|&b| b == 0));
        assert_eq!(&out[200..], &input[1200..2400]);
        assert_eq!(m.total_bytes,    1200);
        assert_eq!(m.read_blocks,    2);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   6);
        assert_eq!(m.write_partials, 0);
    }

    // ── count edge cases ─────────────────────────────────────────────────────

    #[test]
    fn test_count_zero_copies_first_block() {
        // count=0: the guard `blocks_counter >= 0` is always true after the first
        // block (usize ≥ 0 always), so the pipeline breaks after exactly one block.
        let input: Vec<u8> = (0..1024u16).map(|i| (i % 256) as u8).collect();
        let (m, out) = run_transform("count_zero", &input, 512, 512, None, None, Some(0));
        assert_eq!(out, &input[..512]);
        assert_eq!(m.total_bytes,    512);
        assert_eq!(m.read_blocks,    1);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   1);
        assert_eq!(m.write_partials, 0);
    }

    #[test]
    fn test_count_zero_on_empty_input_produces_empty_output() {
        // count=0 on an empty file: EOF is reached before any block → no output
        let (m, out) = run_transform("count_zero_empty", &[], 512, 512, None, None, Some(0));
        assert_eq!(out, &[] as &[u8]);
        assert_eq!(m.total_bytes,    0);
        assert_eq!(m.read_blocks,    0);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   0);
        assert_eq!(m.write_partials, 0);
    }

    #[test]
    fn test_count_one_ibs_lt_obs_partial_write() {
        // count=1 with ibs < obs: one block read (ibs bytes) never fills accum → partial write
        let input: Vec<u8> = (0..1000u16).map(|i| (i % 256) as u8).collect();
        let (m, out) = run_transform("count_one_ibs_lt_obs", &input, 128, 512, None, None, Some(1));
        assert_eq!(out, &input[..128]);
        assert_eq!(m.total_bytes,    128);
        assert_eq!(m.read_blocks,    1);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   0);
        assert_eq!(m.write_partials, 1);
    }

    #[test]
    fn test_count_partial_input_block_counted() {
        // The last read returning fewer than ibs bytes is still counted as one block
        // (a partial), which triggers the count limit just as a full block would.
        // ibs=512, obs=512, file=300 bytes, count=1 → 1 partial read, 1 partial write
        let input = vec![0xCC; 300];
        let (m, out) = run_transform("count_partial_input", &input, 512, 512, None, None, Some(1));
        assert_eq!(out, input.as_slice());
        assert_eq!(m.total_bytes,    300);
        assert_eq!(m.read_blocks,    0);
        assert_eq!(m.read_partials,  1);
        assert_eq!(m.write_blocks,   0);
        assert_eq!(m.write_partials, 1);
    }

    // ── sync ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_sync_partial_block_padded_before_accumulation() {
        // ibs=4, obs=4, input=6 bytes; the second read returns 2 bytes which sync
        // pads to 4 before accumulation → both writes are full, output is 8 bytes.
        let (m, out) = run_with_flags(
            "sync_pad", b"ABCDEF", 4, 4, None, None, None, WriteOps::Sync as u8,
        );
        assert_eq!(out.len(), 8);
        assert_eq!(&out[..6], b"ABCDEF");
        assert_eq!(&out[6..], &[0u8; 2]);
        assert_eq!(m.total_bytes,    8); // padded size
        assert_eq!(m.read_blocks,    2); // sync always returns ibs
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   2);
        assert_eq!(m.write_partials, 0);
    }

    #[test]
    fn test_sync_full_blocks_unaffected() {
        // When every read fills the buffer exactly, sync has no visible effect
        let input = vec![0xAA; 512];
        let (m, out) = run_with_flags(
            "sync_full", &input, 512, 512, None, None, None, WriteOps::Sync as u8,
        );
        assert_eq!(out, input.as_slice());
        assert_eq!(m.total_bytes,    512);
        assert_eq!(m.read_blocks,    1);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   1);
        assert_eq!(m.write_partials, 0);
    }

    #[test]
    fn test_sync_with_ibs_lt_obs_padding_affects_accumulation() {
        // ibs=3, obs=6, input=7 bytes ABCDEFG, sync=true
        // read1=3 (ABC), read2=3 (DEF): accum=6 → write 6 (ABCDEF), accum=``
        // read3=1 byte G padded to [G,0,0], returns 3: accum=[G,0,0] (3<6, no full write)
        // EOF → partial write [G,0,0]
        // Without sync: output=ABCDEFG (7 bytes); with sync: ABCDEFG\0\0 (9 bytes)
        let (m, out) = run_with_flags(
            "sync_ibs_lt_obs", b"ABCDEFG", 3, 6, None, None, None, WriteOps::Sync as u8,
        );
        assert_eq!(out.len(), 9);
        assert_eq!(&out[..7], b"ABCDEFG");
        assert_eq!(&out[7..], &[0u8; 2]);
        assert_eq!(m.total_bytes,    9);
        assert_eq!(m.read_blocks,    3);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   1);
        assert_eq!(m.write_partials, 1);
    }

    #[test]
    fn test_sync_with_ibs_gt_obs_padding_creates_extra_write_blocks() {
        // ibs=6, obs=4, input=9 bytes ABCDEFGHI, sync=true
        // read1=6 (ABCDEF, full): accum=6 → write 4 (ABCD), accum=EF
        // read2=3 bytes GHI padded to [GHI\0\0\0], returns 6:
        //   accum=[EF,GHI\0\0\0] (8) → write 4 (EFGH), accum=[I\0\0\0] (4) → write 4, accum=``
        // total_bytes=12, read_blocks=2, write_blocks=3, write_partials=0
        let (m, out) = run_with_flags(
            "sync_ibs_gt_obs", b"ABCDEFGHI", 6, 4, None, None, None, WriteOps::Sync as u8,
        );
        assert_eq!(out.len(), 12);
        assert_eq!(&out[..9], b"ABCDEFGHI");
        assert_eq!(&out[9..], &[0u8; 3]);
        assert_eq!(m.total_bytes,    12);
        assert_eq!(m.read_blocks,    2);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   3);
        assert_eq!(m.write_partials, 0);
    }

    #[test]
    fn test_sync_with_count_pads_last_counted_partial_block() {
        // ibs=4, obs=8, input=5 bytes ABCDE, count=2, sync=true
        // read1=4 (ABCD, full, count=1): accum=4<8
        // read2=1 byte E padded to [E\0\0\0], returns 4 (count=2 → break):
        //   accum=[ABCDE\0\0\0] (8) → write 8
        // Without sync: accum=5 → write_partials=1, output=ABCDE
        // With sync:    accum=8 → write_blocks=1, output=ABCDE\0\0\0
        let (m, out) = run_with_flags(
            "sync_count_pad", b"ABCDE", 4, 8, None, None, Some(2), WriteOps::Sync as u8,
        );
        assert_eq!(out.len(), 8);
        assert_eq!(&out[..5], b"ABCDE");
        assert_eq!(&out[5..], &[0u8; 3]);
        assert_eq!(m.total_bytes,    8);
        assert_eq!(m.read_blocks,    2);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   1);
        assert_eq!(m.write_partials, 0);
    }

    #[test]
    fn test_sync_with_skip_pads_partial_tail() {
        // ibs=4, obs=4, input=9 bytes AAAABBBBX, skip=1 → discard first 4, read BBBBX
        // read1=4 (BBBB, full) → write 4; read2=1 byte X padded to [X\0\0\0] → write 4
        // output=BBBBX\0\0\0 (8 bytes), all writes are full blocks
        let input = b"AAAABBBBX";
        let (m, out) = run_with_flags(
            "sync_skip_pad", input, 4, 4, Some(1), None, None, WriteOps::Sync as u8,
        );
        assert_eq!(out.len(), 8);
        assert_eq!(&out[..5], b"BBBBX");
        assert_eq!(&out[5..], &[0u8; 3]);
        assert_eq!(m.total_bytes,    8);
        assert_eq!(m.read_blocks,    2);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   2);
        assert_eq!(m.write_partials, 0);
    }

    #[test]
    fn test_sync_with_seek_and_partial_input() {
        // ibs=4, obs=4, input=6 bytes ABCDEF, seek=1, sync=true
        // Writer starts at byte offset 4 (zero-padded prefix from seek).
        // read1=4 (ABCD, full) → write 4; read2=2 bytes EF padded to [EF\0\0] → write 4
        // output: [0,0,0,0, ABCD, EF\0\0] (12 bytes total)
        let (m, out) = run_with_flags(
            "sync_seek_partial", b"ABCDEF", 4, 4, None, Some(1), None, WriteOps::Sync as u8,
        );
        assert_eq!(out.len(), 12);
        assert!(out[..4].iter().all(|&b| b == 0));
        assert_eq!(&out[4..8],  b"ABCD");
        assert_eq!(&out[8..10], b"EF");
        assert_eq!(&out[10..],  &[0u8; 2]);
        assert_eq!(m.total_bytes,    8);
        assert_eq!(m.read_blocks,    2);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   2);
        assert_eq!(m.write_partials, 0);
    }

    // ── noerror ──────────────────────────────────────────────────────────────

    #[test]
    fn test_noerror_on_valid_input_unchanged() {
        // noerror has no observable effect when no read errors occur
        let input: Vec<u8> = (0..512u16).map(|i| (i % 256) as u8).collect();
        let (m, out) = run_with_flags(
            "noerror_normal", &input, 512, 512, None, None, None, WriteOps::NoError as u8,
        );
        assert_eq!(out, input.as_slice());
        assert_eq!(m.total_bytes,    512);
        assert_eq!(m.read_blocks,    1);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   1);
        assert_eq!(m.write_partials, 0);
    }

    #[test]
    fn test_noerror_with_partial_read_on_valid_input() {
        // noerror does not suppress the partial-read metric on a short file
        let input = vec![0xBB; 100];
        let (m, out) = run_with_flags(
            "noerror_partial", &input, 512, 512, None, None, None, WriteOps::NoError as u8,
        );
        assert_eq!(out, input.as_slice());
        assert_eq!(m.total_bytes,    100);
        assert_eq!(m.read_blocks,    0);
        assert_eq!(m.read_partials,  1);
        assert_eq!(m.write_blocks,   0);
        assert_eq!(m.write_partials, 1);
    }

    #[test]
    fn test_noerror_preserves_ibs_lt_obs_accumulation() {
        // noerror does not change how bytes accumulate across reads when ibs < obs
        let input: Vec<u8> = (0..1000u16).map(|i| (i % 256) as u8).collect();
        let (m, out) = run_with_flags(
            "noerror_ibs_lt_obs", &input, 200, 512, None, None, None, WriteOps::NoError as u8,
        );
        assert_eq!(out, input.as_slice());
        assert_eq!(m.total_bytes,    1000);
        assert_eq!(m.read_blocks,    5);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   1);
        assert_eq!(m.write_partials, 1);
    }

    // ── sync + noerror ────────────────────────────────────────────────────────

    #[test]
    fn test_sync_and_noerror_partial_block_padded() {
        // sync + noerror on a valid file with a partial last block: result equals sync alone.
        // The noerror flag does not affect padding behaviour when no errors occur.
        let flags = WriteOps::Sync as u8 | WriteOps::NoError as u8;
        let (m, out) = run_with_flags(
            "sync_noerror_pad", b"ABCDE", 4, 4, None, None, None, flags,
        );
        assert_eq!(out.len(), 8);
        assert_eq!(&out[..5], b"ABCDE");
        assert_eq!(&out[5..], &[0u8; 3]);
        assert_eq!(m.total_bytes,    8);
        assert_eq!(m.read_blocks,    2);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   2);
        assert_eq!(m.write_partials, 0);
    }

    #[test]
    fn test_sync_and_noerror_ibs_lt_obs_multi_block() {
        // ibs=4, obs=8, input=10 bytes, sync + noerror
        // read1=4 (full), read2=4 (full): accum=8 → write 8, accum=``
        // read3=2 bytes padded to 4: accum=4 < 8, no full write
        // EOF → partial write 4 bytes
        // output: bytes 0..9 followed by 2 zero-pad bytes (12 bytes total)
        let input: Vec<u8> = (0..10u8).collect();
        let flags = WriteOps::Sync as u8 | WriteOps::NoError as u8;
        let (m, out) = run_with_flags(
            "sync_noerror_multi", &input, 4, 8, None, None, None, flags,
        );
        assert_eq!(out.len(), 12);
        assert_eq!(&out[..10], input.as_slice());
        assert_eq!(&out[10..], &[0u8; 2]);
        assert_eq!(m.total_bytes,    12);
        assert_eq!(m.read_blocks,    3);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   1);
        assert_eq!(m.write_partials, 1);
    }

    #[test]
    fn test_sync_and_noerror_with_count_and_skip() {
        // ibs=4, obs=4, input=16 bytes, skip=1, count=2, sync + noerror
        // skip 4 bytes → read from byte 4: BBBBCCCCDDDD (12 bytes)
        // read1=4 (BBBB, full, count=1) → write 4
        // read2=4 (CCCC, full, count=2 → break) → write 4
        // output=BBBBCCCC (8 bytes), all full blocks
        let input: Vec<u8> = b"AAAABBBBCCCCDDDD".to_vec();
        let flags = WriteOps::Sync as u8 | WriteOps::NoError as u8;
        let (m, out) = run_with_flags(
            "sync_noerror_skip_count", &input, 4, 4, Some(1), None, Some(2), flags,
        );
        assert_eq!(out, b"BBBBCCCC");
        assert_eq!(m.total_bytes,    8);
        assert_eq!(m.read_blocks,    2);
        assert_eq!(m.read_partials,  0);
        assert_eq!(m.write_blocks,   2);
        assert_eq!(m.write_partials, 0);
    }
}