pub const INPUT_FILE_ARG: &str = "if";
pub const OUTPUT_FILE_ARG: &str = "of";
pub const BLOCK_SIZE_ARG: &str = "bs";
pub const INPUT_BLOCK_SIZE_ARG: &str = "ibs";
pub const OUTPUT_BLOCK_SIZE_ARG: &str = "obs";
pub const COUNT_ARG: &str = "count";
pub const SKIP_ARG: &str = "skip";
pub const SEEK_ARG: &str = "seek";
pub const CONVERSION_ARG: &str = "conv";

// data conversion options
pub const CONVERSION_OPTION_LOWER_CASE: &str = "lcase";
pub const CONVERSION_OPTION_UPPER_CASE: &str = "ucase";
pub const CONVERSION_OPTION_SWAP: &str = "swap";

// output operations options
pub const OUTPUT_OPTION_NO_TRUNC: &str = "notrunc";
pub const OUTPUT_OPTION_SYNC: &str = "sync";
pub const OUTPUT_OPTION_SPARSE: &str = "sparse";
pub const OUTPUT_OPTION_NO_ERROR: &str = "noerror";

pub const DEFAULT_BLOCK_SIZE: usize = 512;
pub const MAX_BLOCK_SIZE: usize = 1024 * 1024 * 1024; // 1GB


// error messages
pub const INVALID_ARGUMENT_FORMAT: &str = "ccdd: config: expected key=value";
pub const INVALID_ARGUMENT_KEY_VALUE: &str = "ccdd: config: key and value cannot be empty";
pub const INVALID_ARGUMENT_VALUE: &str = "ccdd: config: invalid argument value";

pub const INVALID_BLOCK_SIZE: &str = "ccdd: invalid block size";
pub const BLOCK_SIZE_EXCEEDS_LIMIT: &str = "ccdd: block size exceeds the limit";
pub const INVALID_BLOCK_SIZE_MULTIPLIER: &str = "ccdd: invalid block size multiplier";
pub const INVALID_CONVERSION_OPTION: &str = "ccdd: invalid conversion option";
pub const LOWER_CASE_ILLEGAL_CONVERSION_COMBINATION: &str = "ccdd: lcase: illegal conversion combination";
pub const UPPER_CASE_ILLEGAL_CONVERSION_COMBINATION: &str = "ccdd: ucase: illegal conversion combination";
pub const INVALID_INPUT_OUTPUT_COMBINATION: &str = "ccdd: input and output file cannot be the same";


pub const SEEK_SIZE_OUT_OF_BOUNDS: &str = "ccdd: seek: amount exceeds the limit";
pub const SKIP_AMOUNT_OUT_OF_BOUND: &str = "ccdd: skip: amount exceeds the limit";