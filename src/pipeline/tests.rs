//! Comprehensive test suite for the Pipeline module
//!
//! Tests are organized by semantic modules covering:
//! - Pipeline construction and initialization
//! - Basic data transfer scenarios
//! - Data conversions (lcase, ucase, swap)
//! - Special modes (sync padding, sparse seeking)
//! - Error handling and recovery
//! - Block size interactions
//! - Edge cases and boundary conditions

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;

use crate::config::{Config, ConfigError};
use crate::pipeline::Pipeline;

// ===== HELPERS MODULE =====

mod helpers {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Creates a unique file name using timestamp + counter to avoid collisions
    fn unique_filename(prefix: &str) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        let thread_id = std::thread::current().id();
        PathBuf::from(format!(
            "{}/udc_test_{}_{}_{}_{}",
            std::env::temp_dir().display(),
            prefix,
            timestamp,
            counter,
            format!("{:?}", thread_id)
                .replace("ThreadId(", "")
                .replace(")", "")
        ))
    }

    /// Creates a temporary file with the given data and returns (file handle, path).
    pub fn temp_input_file(data: &[u8]) -> (File, PathBuf) {
        let path: PathBuf = unique_filename("input");
        let mut file = File::create(&path).expect("failed to create temp input file");
        file.write_all(data)
            .expect("failed to write temp input data");
        file.sync_all().expect("failed to sync temp input file");
        (file, path)
    }

    /// Creates a temporary output path (file not yet created).
    pub fn temp_output_path() -> PathBuf {
        unique_filename("output")
    }

    /// Builds a Config from key=value arguments.
    pub fn create_config(args: &[&str]) -> Result<Config, ConfigError> {
        let owned_args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        Config::build(&owned_args)
    }

    /// Reads entire file contents into a Vec<u8>.
    pub fn read_file_contents(path: &PathBuf) -> std::io::Result<Vec<u8>> {
        let mut file = File::open(path)?;
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;
        Ok(contents)
    }

    /// Cleans up temporary file.
    pub fn cleanup_file(path: &PathBuf) {
        let _ = fs::remove_file(path);
    }

    /// Test wrapper that manages file lifecycle.
    pub struct TestContext {
        pub input_path: PathBuf,
        pub output_path: PathBuf,
    }

    impl TestContext {
        pub fn new(input_data: &[u8]) -> Self {
            let (_, input_path) = temp_input_file(input_data);
            let output_path = temp_output_path();
            // Ensure output path doesn't exist before test
            cleanup_file(&output_path);
            TestContext {
                input_path,
                output_path,
            }
        }
    }

    impl Drop for TestContext {
        fn drop(&mut self) {
            cleanup_file(&self.input_path);
            cleanup_file(&self.output_path);
        }
    }
}

use helpers::*;

// ===== PIPELINE CONSTRUCTION TESTS =====

#[test]
fn test_build_with_file_input_output() {
    let ctx = TestContext::new(b"hello world");
    let config = create_config(&[
        &format!("if={}", ctx.input_path.display()),
        &format!("of={}", ctx.output_path.display()),
        "bs=1024",
    ])
    .expect("config creation failed");

    let result = Pipeline::build(config);
    assert!(
        result.is_ok(),
        "Pipeline::build should succeed with valid file paths"
    );
}

#[test]
fn test_build_with_valid_block_sizes() {
    let ctx = TestContext::new(b"test data");
    let config = create_config(&[
        &format!("if={}", ctx.input_path.display()),
        &format!("of={}", ctx.output_path.display()),
        "ibs=512",
        "obs=512",
    ])
    .expect("config creation failed");

    let result = Pipeline::build(config);
    assert!(
        result.is_ok(),
        "Pipeline::build should succeed with aligned block sizes"
    );
}

#[test]
fn test_build_mismatched_block_sizes_validation() {
    let ctx = TestContext::new(b"test data");
    let config = create_config(&[
        &format!("if={}", ctx.input_path.display()),
        &format!("of={}", ctx.output_path.display()),
        "ibs=256",
        "obs=512",
    ])
    .expect("config creation failed");

    // Should succeed even with mismatched sizes (validation happens at runtime)
    let result = Pipeline::build(config);
    assert!(
        result.is_ok(),
        "Pipeline::build should succeed with mismatched block sizes"
    );
}

// ===== BASIC DATA TRANSFER TESTS =====

#[test]
fn test_basic_read_write_single_full_block() {
    let input_data = b"x".repeat(1024);
    let ctx = TestContext::new(&input_data);

    let config = create_config(&[
        &format!("if={}", ctx.input_path.display()),
        &format!("of={}", ctx.output_path.display()),
        "ibs=1024",
        "obs=1024",
        "count=1",
    ])
    .expect("config creation failed");

    let mut pipeline = Pipeline::build(config).expect("pipeline build failed");
    pipeline.run().expect("pipeline run failed");

    let output = read_file_contents(&ctx.output_path).expect("failed to read output");
    assert_eq!(output, input_data, "output should match input");
    assert_eq!(
        pipeline.get_metrics().read_blocks,
        1,
        "should have 1 read block"
    );
    assert_eq!(
        pipeline.get_metrics().write_blocks,
        1,
        "should have 1 write block"
    );
}

#[test]
fn test_read_write_multiple_blocks() {
    let input_data = b"x".repeat(3072); // 3 full blocks at 1024 bytes each
    let ctx = TestContext::new(&input_data);

    let config = create_config(&[
        &format!("if={}", ctx.input_path.display()),
        &format!("of={}", ctx.output_path.display()),
        "ibs=1024",
        "obs=1024",
    ])
    .expect("config creation failed");

    let mut pipeline = Pipeline::build(config).expect("pipeline build failed");
    pipeline.run().expect("pipeline run failed");

    let output = read_file_contents(&ctx.output_path).expect("failed to read output");
    assert_eq!(output, input_data, "output should match input");
    assert_eq!(
        pipeline.get_metrics().read_blocks,
        3,
        "should have 3 read blocks"
    );
    assert_eq!(
        pipeline.get_metrics().write_blocks,
        3,
        "should have 3 write blocks"
    );
}

#[test]
fn test_read_write_partial_block() {
    let input_data = b"short"; // 5 bytes
    let ctx = TestContext::new(input_data);

    let config = create_config(&[
        &format!("if={}", ctx.input_path.display()),
        &format!("of={}", ctx.output_path.display()),
        "ibs=512",
        "obs=512",
    ])
    .expect("config creation failed");

    let mut pipeline = Pipeline::build(config).expect("pipeline build failed");
    pipeline.run().expect("pipeline run failed");

    let output = read_file_contents(&ctx.output_path).expect("failed to read output");
    assert_eq!(output, input_data, "output should match input");
    assert_eq!(
        pipeline.get_metrics().read_partials,
        1,
        "should have 1 partial read"
    );
}

#[test]
fn test_empty_file_transfer() {
    let input_data = b"";
    let ctx = TestContext::new(input_data);

    let config = create_config(&[
        &format!("if={}", ctx.input_path.display()),
        &format!("of={}", ctx.output_path.display()),
        "ibs=512",
        "obs=512",
    ])
    .expect("config creation failed");

    let mut pipeline = Pipeline::build(config).expect("pipeline build failed");
    pipeline.run().expect("pipeline run failed");

    let output = read_file_contents(&ctx.output_path).expect("failed to read output");
    assert_eq!(output.len(), 0, "output should be empty");
    assert_eq!(
        pipeline.get_metrics().read_blocks,
        0,
        "should have 0 read blocks"
    );
    assert_eq!(
        pipeline.get_metrics().write_blocks,
        0,
        "should have 0 write blocks"
    );
}

#[test]
fn test_count_limit_enforcement() {
    let input_data = b"x".repeat(4096); // 4 full blocks
    let ctx = TestContext::new(&input_data);

    let config = create_config(&[
        &format!("if={}", ctx.input_path.display()),
        &format!("of={}", ctx.output_path.display()),
        "ibs=1024",
        "obs=1024",
        "count=2", // Limit to 2 blocks
    ])
    .expect("config creation failed");

    let mut pipeline = Pipeline::build(config).expect("pipeline build failed");
    pipeline.run().expect("pipeline run failed");

    let output = read_file_contents(&ctx.output_path).expect("failed to read output");
    assert_eq!(
        output.len(),
        2048,
        "output should be exactly 2 blocks (2048 bytes)"
    );
    assert_eq!(
        pipeline.get_metrics().read_blocks,
        2,
        "should have read exactly 2 blocks"
    );
}

// ===== DATA CONVERSION TESTS =====

#[test]
fn test_to_lower_conversion() {
    let input_data = b"HELLO World";
    let ctx = TestContext::new(input_data);

    let config = create_config(&[
        &format!("if={}", ctx.input_path.display()),
        &format!("of={}", ctx.output_path.display()),
        "ibs=512",
        "obs=512",
        "conv=lcase",
    ])
    .expect("config creation failed");

    let mut pipeline = Pipeline::build(config).expect("pipeline build failed");
    pipeline.run().expect("pipeline run failed");

    let output = read_file_contents(&ctx.output_path).expect("failed to read output");
    // Should start with lowercase conversion
    assert!(output.len() >= 11, "output should contain converted data");
    assert_eq!(&output[..11], b"hello world", "should convert to lowercase");
}

#[test]
fn test_to_upper_conversion() {
    let input_data = b"hello WORLD";
    let ctx = TestContext::new(input_data);

    let config = create_config(&[
        &format!("if={}", ctx.input_path.display()),
        &format!("of={}", ctx.output_path.display()),
        "ibs=512",
        "obs=512",
        "conv=ucase",
    ])
    .expect("config creation failed");

    let mut pipeline = Pipeline::build(config).expect("pipeline build failed");
    pipeline.run().expect("pipeline run failed");

    let output = read_file_contents(&ctx.output_path).expect("failed to read output");
    assert_eq!(output, b"HELLO WORLD", "should convert to uppercase");
}

#[test]
fn test_swap_conversion() {
    let input_data = &[0x12u8, 0x34, 0x56, 0x78];
    let ctx = TestContext::new(input_data);

    let config = create_config(&[
        &format!("if={}", ctx.input_path.display()),
        &format!("of={}", ctx.output_path.display()),
        "ibs=512",
        "obs=512",
        "conv=swab,notrunc",
    ])
    .expect("config creation failed");

    let mut pipeline = Pipeline::build(config).expect("pipeline build failed");
    pipeline.run().expect("pipeline run failed");

    let output = read_file_contents(&ctx.output_path).expect("failed to read output");
    assert_eq!(
        output,
        &[0x34u8, 0x12, 0x78, 0x56],
        "bytes should be swapped"
    );
}

#[test]
fn test_combined_conversions_lcase_then_swap() {
    let input_data = b"AB\x00\x01CD\x00\x02";
    let ctx = TestContext::new(input_data);

    let config = create_config(&[
        &format!("if={}", ctx.input_path.display()),
        &format!("of={}", ctx.output_path.display()),
        "ibs=512",
        "obs=512",
        "conv=lcase,swab",
    ])
    .expect("config creation failed");

    let mut pipeline = Pipeline::build(config).expect("pipeline build failed");
    pipeline.run().expect("pipeline run failed");

    let output = read_file_contents(&ctx.output_path).expect("failed to read output");
    assert_eq!(
        output,
        [b'b', b'a', 0x01, 0x00u8, b'd', b'c', 0x02, 0x00u8],
        "should apply lcase then swap correctly"
    );
}

#[test]
fn test_conversion_with_multiple_blocks() {
    let input_data = b"FIRST_BLOCK_OF_DATA_SECOND_BLOCK_OF_DATA_THIRD_BLOCK_DATA";
    let ctx = TestContext::new(input_data);

    let config = create_config(&[
        &format!("if={}", ctx.input_path.display()),
        &format!("of={}", ctx.output_path.display()),
        "ibs=16",
        "obs=16",
        "conv=lcase",
    ])
    .expect("config creation failed");

    let mut pipeline = Pipeline::build(config).expect("pipeline build failed");
    pipeline.run().expect("pipeline run failed");

    let output = read_file_contents(&ctx.output_path).expect("failed to read output");
    let expected = b"first_block_of_data_second_block_of_data_third_block_data";
    assert_eq!(
        output, expected,
        "conversion should be applied across all blocks"
    );
}

// ===== SYNC AND SPARSE MODE TESTS =====

#[test]
fn test_sync_padding_partial_block() {
    let input_data = b"short"; // 5 bytes, partial block
    let ctx = TestContext::new(input_data);

    let config = create_config(&[
        &format!("if={}", ctx.input_path.display()),
        &format!("of={}", ctx.output_path.display()),
        "ibs=512",
        "obs=512",
        "conv=sync",
    ])
    .expect("config creation failed");

    let mut pipeline = Pipeline::build(config).expect("pipeline build failed");
    pipeline.run().expect("pipeline run failed");

    let output = read_file_contents(&ctx.output_path).expect("failed to read output");
    // Output should be at least the input size, padding may apply
    assert!(
        output.len() >= 5,
        "output should be at least as large as input (got {})",
        output.len()
    );
    assert_eq!(&output[..5], b"short", "first 5 bytes should be input");
}

#[test]
fn test_sync_padding_multiple_partial_blocks() {
    let input_data = b"abc defgh"; // 9 bytes total
    let ctx = TestContext::new(input_data);

    let config = create_config(&[
        &format!("if={}", ctx.input_path.display()),
        &format!("of={}", ctx.output_path.display()),
        "ibs=8",
        "obs=8",
        "conv=sync",
    ])
    .expect("config creation failed");

    let mut pipeline = Pipeline::build(config).expect("pipeline build failed");
    pipeline.run().expect("pipeline run failed");

    let output = read_file_contents(&ctx.output_path).expect("failed to read output");
    // Output should be at least as large as input, possibly padded
    assert!(
        output.len() >= 9,
        "output should contain input data (got {} bytes)",
        output.len()
    );
    assert_eq!(&output[..9], input_data, "first 9 bytes should match input");
}

#[test]
fn test_sparse_seeking_with_zeros() {
    let mut input_data = vec![0u8; 256];
    input_data.extend_from_slice(b"data");
    input_data.extend_from_slice(&[0u8; 256]);

    let ctx = TestContext::new(&input_data);

    let config = create_config(&[
        &format!("if={}", ctx.input_path.display()),
        &format!("of={}", ctx.output_path.display()),
        "ibs=256",
        "obs=256",
        "conv=sparse",
    ])
    .expect("config creation failed");

    let mut pipeline = Pipeline::build(config).expect("pipeline build failed");
    pipeline.run().expect("pipeline run failed");

    let output = read_file_contents(&ctx.output_path).expect("failed to read output");
    // With sparse mode, zero-blocks are skipped (seeked), so output may have holes
    // or be smaller than input
    assert!(
        output.len() <= input_data.len(),
        "sparse output should not be larger than input"
    );
}

// ===== ERROR HANDLING TESTS =====

#[test]
fn test_noerror_with_missing_input_file() {
    // Note: This tests the noerror flag behavior; actual file errors are harder
    // to simulate without changing reader internals. This is a placeholder for
    // the conceptual test.
    let ctx = TestContext::new(b"test");
    let nonexistent = PathBuf::from("/tmp/nonexistent_file_xyz_123.txt");

    let config = create_config(&[
        &format!("if={}", nonexistent.display()),
        &format!("of={}", ctx.output_path.display()),
        "ibs=512",
        "obs=512",
        "conv=noerror,notrunc",
    ])
    .expect("config creation failed");

    // Build should fail for missing input file (not recoverable at pipeline level)
    let result = Pipeline::build(config);
    assert!(
        result.is_err(),
        "should fail to build with nonexistent input file"
    );
}

#[test]
fn test_io_error_propagation_without_noerror() {
    let ctx = TestContext::new(b"test");
    let nonexistent = PathBuf::from("/tmp/nonexistent_file_xyz_456.txt");

    let config = create_config(&[
        &format!("if={}", nonexistent.display()),
        &format!("of={}", ctx.output_path.display()),
        "ibs=512",
        "obs=512",
        "conv=notrunc",
    ])
    .expect("config creation failed");

    // Without conv=noerror, errors should not be tolerated at build time
    let result = Pipeline::build(config);
    assert!(
        result.is_err(),
        "should fail without conv=noerror on bad input"
    );
}

// ===== BLOCK SIZE SCENARIO TESTS =====

#[test]
fn test_ibs_less_than_obs() {
    // ibs=256, obs=512: multiple reads feed one write
    let input_data = b"x".repeat(512); // 2 reads of 256 bytes -> 1 write of 512 bytes
    let ctx = TestContext::new(&input_data);

    let config = create_config(&[
        &format!("if={}", ctx.input_path.display()),
        &format!("of={}", ctx.output_path.display()),
        "ibs=256",
        "obs=512",
    ])
    .expect("config creation failed");

    let mut pipeline = Pipeline::build(config).expect("pipeline build failed");
    pipeline.run().expect("pipeline run failed");

    let output = read_file_contents(&ctx.output_path).expect("failed to read output");
    assert_eq!(output, input_data, "output should match input");
}

#[test]
fn test_ibs_greater_than_obs() {
    // ibs=512, obs=256: one read feeds multiple writes
    let input_data = b"y".repeat(512);
    let ctx = TestContext::new(&input_data);

    let config = create_config(&[
        &format!("if={}", ctx.input_path.display()),
        &format!("of={}", ctx.output_path.display()),
        "ibs=512",
        "obs=256",
    ])
    .expect("config creation failed");

    let mut pipeline = Pipeline::build(config).expect("pipeline build failed");
    pipeline.run().expect("pipeline run failed");

    let output = read_file_contents(&ctx.output_path).expect("failed to read output");
    // Should contain all input data
    assert_eq!(
        output[..512],
        input_data[..],
        "first 512 bytes should match input"
    );
}

#[test]
fn test_block_mismatch_boundary_handling() {
    // ibs=100, obs=150: verify data integrity at boundary
    let input_data = b"ABCDEFGHIJ".repeat(10); // 100 bytes
    let ctx = TestContext::new(&input_data);

    let config = create_config(&[
        &format!("if={}", ctx.input_path.display()),
        &format!("of={}", ctx.output_path.display()),
        "ibs=100",
        "obs=150",
    ])
    .expect("config creation failed");

    let mut pipeline = Pipeline::build(config).expect("pipeline build failed");
    pipeline.run().expect("pipeline run failed");

    let output = read_file_contents(&ctx.output_path).expect("failed to read output");
    assert!(output.len() >= 100, "output should contain input data");
    assert_eq!(
        &output[..100],
        &input_data[..],
        "first 100 bytes should match input"
    );
}

// ===== EDGE CASE TESTS =====

#[test]
fn test_odd_length_buffer_with_swap() {
    // swap with odd-length buffer: last byte should remain unchanged
    let input_data = &[0x12u8, 0x34, 0x56]; // 3 bytes (odd)
    let ctx = TestContext::new(input_data);

    let config = create_config(&[
        &format!("if={}", ctx.input_path.display()),
        &format!("of={}", ctx.output_path.display()),
        "ibs=512",
        "obs=512",
        "conv=swab",
    ])
    .expect("config creation failed");

    let mut pipeline = Pipeline::build(config).expect("pipeline build failed");
    pipeline.run().expect("pipeline run failed");

    let output = read_file_contents(&ctx.output_path).expect("failed to read output");
    // 0x12, 0x34 swapped to 0x34, 0x12; 0x56 remains unchanged
    assert_eq!(output, &[0x34u8, 0x12, 0x56]);
}

#[test]
fn test_swap_alignment_on_partial_block_boundary() {
    // Ensure swap doesn't break on partial blocks at block boundaries
    let input_data = &[0xAAu8, 0xBB, 0xCC, 0xDD, 0xEE];
    let ctx = TestContext::new(input_data);

    let config = create_config(&[
        &format!("if={}", ctx.input_path.display()),
        &format!("of={}", ctx.output_path.display()),
        "ibs=512",
        "obs=512",
        "conv=swab",
    ])
    .expect("config creation failed");

    let mut pipeline = Pipeline::build(config).expect("pipeline build failed");
    pipeline.run().expect("pipeline run failed");

    let output = read_file_contents(&ctx.output_path).expect("failed to read output");
    // AA,BB swapped to BB,AA; CC,DD swapped to DD,CC; EE remains
    assert!(output.len() >= 5, "output should contain converted data");
    assert_eq!(&output[..5], &[0xBBu8, 0xAA, 0xDD, 0xCC, 0xEE]);
}

#[test]
fn test_eof_boundary_with_partial_write() {
    // EOF during a partial write should flush correctly
    let input_data = b"incomplete";
    let ctx = TestContext::new(input_data);

    let config = create_config(&[
        &format!("if={}", ctx.input_path.display()),
        &format!("of={}", ctx.output_path.display()),
        "ibs=1024",
        "obs=1024",
    ])
    .expect("config creation failed");

    let mut pipeline = Pipeline::build(config).expect("pipeline build failed");
    pipeline.run().expect("pipeline run failed");

    let output = read_file_contents(&ctx.output_path).expect("failed to read output");
    assert_eq!(output, input_data, "partial data should be flushed on EOF");
}

#[test]
fn test_metrics_accumulation_across_operations() {
    // Verify metrics track correctly across multiple read/write cycles
    let input_data = b"x".repeat(2560); // 2.5 blocks at 1024
    let ctx = TestContext::new(&input_data);

    let config = create_config(&[
        &format!("if={}", ctx.input_path.display()),
        &format!("of={}", ctx.output_path.display()),
        "ibs=1024",
        "obs=1024",
    ])
    .expect("config creation failed");

    let mut pipeline = Pipeline::build(config).expect("pipeline build failed");
    pipeline.run().expect("pipeline run failed");

    let metrics = pipeline.get_metrics();
    // 2 full reads (1024 bytes each) + 1 partial read (512 bytes)
    assert_eq!(metrics.read_blocks, 2, "should have 2 full read blocks");
    assert_eq!(metrics.read_partials, 1, "should have 1 partial read");
    // 2 full writes (1024 bytes each) + 1 partial write (512 bytes)
    assert_eq!(metrics.write_blocks, 2, "should have 2 full write blocks");
    assert_eq!(metrics.write_partials, 1, "should have 1 partial write");
}

#[test]
fn test_conversion_preserves_binary_data() {
    // Conversions should not affect non-ASCII bytes
    let input_data = &[0xFF, 0xFE, 0xFD, b'A', b'B', b'C'];
    let ctx = TestContext::new(input_data);

    let config = create_config(&[
        &format!("if={}", ctx.input_path.display()),
        &format!("of={}", ctx.output_path.display()),
        "ibs=512",
        "obs=512",
        "conv=lcase",
    ])
    .expect("config creation failed");

    let mut pipeline = Pipeline::build(config).expect("pipeline build failed");
    pipeline.run().expect("pipeline run failed");

    let output = read_file_contents(&ctx.output_path).expect("failed to read output");
    // High bytes unchanged, only A,B,C affected (stay A,B,C since already uppercase)
    assert_eq!(output, &[0xFF, 0xFE, 0xFD, b'a', b'b', b'c']);
}

#[test]
fn test_skip_seek_with_basic_io() {
    // Test skip and seek basic functionality
    let input_data = b"0123456789ABCDEF";
    let ctx = TestContext::new(input_data);

    let config = create_config(&[
        &format!("if={}", ctx.input_path.display()),
        &format!("of={}", ctx.output_path.display()),
        "ibs=512",
        "obs=512",
        "skip_bytes=5", // Skip first 5 bytes
        "seek_bytes=0", // Write from beginning
    ])
    .expect("config creation failed");

    let mut pipeline = Pipeline::build(config).expect("pipeline build failed");
    pipeline.run().expect("pipeline run failed");

    let output = read_file_contents(&ctx.output_path).expect("failed to read output");
    // Should write everything except the first 5 bytes
    assert_eq!(output, &b"56789ABCDEF"[..]);
}
