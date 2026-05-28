pub const INPUT_FILE_ARG: &str = "if";
pub const OUTPUT_FILE_ARG: &str = "of";
pub const BLOCK_SIZE_ARG: &str = "bs";
pub const INPUT_BLOCK_SIZE_ARG: &str = "ibs";
pub const OUTPUT_BLOCK_SIZE_ARG: &str = "obs";
pub const COUNT_ARG: &str = "count";
pub const SKIP_ARG: &str = "skip";
pub const SEEK_ARG: &str = "seek";

pub const DEFAULT_BLOCK_SIZE: usize = 512;
pub const MAX_BLOCK_SIZE: usize = 1024 * 1024 * 1024; // 1GB