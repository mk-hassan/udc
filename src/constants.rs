//! Constants used in the udc crate
//! 
//! This module is meant to provide a centralized location for constants that are used throughout the udc crate. 
//! These constants include expected arguments, data conversion options, output operation options, status print options, 
//! flags, and numerical constants.

// expected arguments
pub const INPUT_FILE_ARG: &str = "if";
pub const OUTPUT_FILE_ARG: &str = "of";
pub const BLOCK_SIZE_ARG: &str = "bs";
pub const INPUT_BLOCK_SIZE_ARG: &str = "ibs";
pub const OUTPUT_BLOCK_SIZE_ARG: &str = "obs";

pub const COUNT_ARG: &str = "count";
pub const COUNT_BYTES_ARG: &str = "count_bytes";

pub const SKIP_ARG: &str = "skip";
pub const SKIP_BYTES_ARG: &str = "skip_bytes";

pub const SEEK_ARG: &str = "seek";
pub const SEEK_BYTES_ARG: &str = "seek_bytes";

pub const CONVERSION_ARG: &str = "conv";
pub const PRINT_STATUS_ARG: &str = "status";
pub const IFLAG_ARG: &str = "iflag";
pub const OFLAG_ARG: &str = "oflag";

// data conversion options
pub const CONVERSION_OPTION_LOWER_CASE: &str = "lcase";
pub const CONVERSION_OPTION_UPPER_CASE: &str = "ucase";
pub const CONVERSION_OPTION_SWAP: &str = "swab";

// output operations options
pub const OUTPUT_OPTION_NO_TRUNC: &str = "notrunc";
pub const OUTPUT_OPTION_SYNC: &str = "sync";
pub const OUTPUT_OPTION_SPARSE: &str = "sparse";
pub const OUTPUT_OPTION_NO_ERROR: &str = "noerror";

// status print options
pub const NO_PRINT: &str = "none";
pub const NOXFER_PRINT: &str = "noxfer";
pub const PROGRESS_PRINT: &str = "progress";

// flags
pub const DIRECT: &str = "direct";
pub const NONBLOCK: &str = "nonblock";
pub const NOCACHE: &str = "nocache";
pub const SYNC: &str = "sync";
pub const DSYNC: &str = "dsync";
pub const EXCL: &str = "excl";
pub const FULLBLOCK: &str = "fullblock";
pub const COUNTBYTES: &str = "count_bytes";
pub const SKIPBYTES: &str = "skip_bytes";
pub const SEEKBYTES: &str = "seek_bytes";

// numerical constants
pub const DEFAULT_BLOCK_SIZE: usize = 512;