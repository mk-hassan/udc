//! Configuration parsing and command-line validation engine for `udc`.
//!
//! This module handles ingestion, lexical verification, and syntactic parsing of
//! command-line options. It closely follows the standard POSIX `dd` style key-value
//! configuration format (`key=value`) and builds a type-safe runtime operational context.
//!
//! ## Core Responsibilities
//! - **Key-Value Validation**: Tokenizes command-line input slices, separating operational flags from parameters.
//! - **Block Size Hierarchy**: Configures asymmetric transmission boundaries (`ibs` and `obs`), falling back to `bs` when unified.
//! - **Dynamic Byte Multipliers**: Parses standard storage suffix multipliers (e.g., `k`, `b`, `M`, `G`) into exact byte capacities.
//! - **Mutually Exclusive Invariants**: Flags conflicting operations at compile/initialization time (e.g., combining `lcase` and `ucase`).
//!
//! For an exhaustive list of all supported command-line arguments, sub-options, and platform-specific flag
//! configurations, please consult the crate's root `README.md`.
//!
//! ## Example
//!
//! ```no_run
//! # use udc::config::Config;
//! let args = vec![
//!     "if=input.raw".to_string(),
//!     "of=output.raw".to_string(),
//!     "bs=4k".to_string(),
//!     "conv=notrunc,sync".to_string(),
//! ];
//!
//! let config = Config::build(&args)?;
//! # Ok::<(), udc::config::ConfigError>(())
//! ```

use crate::{constants, enums::*};
use std::collections::HashMap;
use std::fmt;

/// Configuration structure for udc operations.
///
/// Encapsulates all configurable parameters for the data copying operation, including
/// source/destination, block sizes, I/O flags, and data conversion options.
///
/// ## Fields
///
/// - `source`: Input source (file or standard input)
/// - `destination`: Output destination (file or standard output)
/// - `block_size`: Default block size (fallback for ibs/obs)
/// - `input_block_size`: Input block size (ibs)
/// - `output_block_size`: Output block size (obs)
/// - `count`: Number of blocks/bytes to process
/// - `skip`: Number of input blocks/bytes to skip
/// - `seek`: Number of output blocks/bytes to seek
/// - `data_convs`: Bitmask of data conversion options
/// - `write_convs`: Bitmask of write operation options
/// - `iflag`: Input operation flags
/// - `oflag`: Output operation flags
/// - `print_status`: Output status reporting level
#[derive(Debug, Default)]
pub struct Config {
    source: SourceType,
    destination: SourceType,
    block_size: Option<usize>,
    input_block_size: Option<usize>,
    output_block_size: Option<usize>,
    count: Option<usize>,
    skip: Option<usize>,
    seek: Option<usize>,
    data_convs: u8,
    write_convs: u8,
    iflag: u8,
    oflag: u8,
    print_status: PrintStatus,
}

/// Error types for configuration parsing and validation.
///
/// This enum represents all possible errors that can occur during configuration
/// construction and validation. Each variant provides specific context about what
/// went wrong.
///
/// ## Variants
///
/// - `UnknownArgument(String)`: An unrecognized argument key was provided
/// - `InvalidFormat(String)`: Argument doesn't follow the `key=value` format
/// - `InvalidArgumentValue(String, String)`: Argument has an invalid value
/// - `Duplicate(String)`: An argument was specified multiple times
/// - `IllegalComb(String, String)`: Two mutually exclusive arguments were used together
/// - `IllegalCombWithValue(String, String, String)`: Invalid combination of specific values
/// - `OutOfBounds(String, usize)`: An argument value is out of valid range
/// - `Other(String)`: A miscellaneous configuration error
#[derive(Debug)]
pub enum ConfigError {
    /// An unknown argument key was encountered
    UnknownArgument(String),
    /// Argument format is invalid (missing '=' or empty parts)
    InvalidFormat(String),
    /// Argument value is invalid for the given key
    InvalidArgumentValue(String, String),
    /// An argument was specified multiple times
    Duplicate(String),
    /// Two mutually exclusive arguments were used together
    IllegalComb(String, String),
    /// Invalid combination of specific values for an argument
    IllegalCombWithValue(String, String, String),
    /// An argument value is outside acceptable bounds
    OutOfBounds(String, usize),
    /// A miscellaneous configuration error
    Other(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::UnknownArgument(arg) => {
                write!(f, "udc: config: Unknown argument: {}", arg)
            }
            ConfigError::InvalidFormat(arg) => {
                write!(f, "udc: config: Invalid argument format: {}", arg)
            }
            ConfigError::InvalidArgumentValue(arg, value) => {
                write!(f, "udc: config: {}: Invalid argument value {}", arg, value)
            }
            ConfigError::Duplicate(arg) => {
                write!(f, "udc: config: Argument specified multiple times: {}", arg)
            }
            ConfigError::IllegalComb(arg1, arg2) => write!(
                f,
                "udc: config: Illegal argument combination: {} and {}",
                arg1, arg2
            ),
            ConfigError::IllegalCombWithValue(arg1, value1, value2) => write!(
                f,
                "udc: config: {}: Illegal values combination: {} {}",
                arg1, value1, value2
            ),
            ConfigError::OutOfBounds(arg, value) => write!(
                f,
                "udc: config: Argument out of bounds: {} = {}",
                arg, value
            ),
            ConfigError::Other(msg) => write!(f, "udc: config: {}", msg),
        }
    }
}

/// A specialized `Result` type for configuration operations.
///
/// ## Examples
///
/// ```no_run
/// # use udc::config::{Config, Result};
/// fn parse_config(args: &[String]) -> Result<Config> {
///     Config::build(args)
/// }
/// ```
pub type Result<T> = std::result::Result<T, ConfigError>;

impl Config {
    /// Builds a `Config` from command-line arguments.
    ///
    /// Parses an array of `key=value` formatted arguments and validates them to create
    /// a complete configuration. This is the primary entry point for configuration creation.
    ///
    /// ## Arguments
    ///
    /// * `args` - A slice of strings in `key=value` format
    ///
    /// ## Returns
    ///
    /// A `Result` containing the constructed `Config` or a `ConfigError` if parsing fails.
    ///
    /// ## Errors
    ///
    /// Returns an error if:
    /// - An unknown argument key is encountered
    /// - An argument format is invalid
    /// - Duplicate arguments are specified
    /// - Values are invalid or mutually exclusive
    /// - Input and output files are the same
    ///
    /// ## Examples
    ///
    /// ```no_run
    /// # use udc::config::Config;
    /// let args = vec![
    ///     "if=test.txt".to_string(),
    ///     "of=output.txt".to_string(),
    ///     "bs=1024".to_string(),
    /// ];
    /// let config = Config::build(&args)?;
    /// # Ok::<(), udc::config::ConfigError>(())
    /// ```
    pub fn build(args: &[String]) -> Result<Self> {
        let mut config = Config::default();

        let validated_args = Self::validate_arguments(args)?;

        let (mut count, mut count_bytes): (Option<&str>, Option<&str>) = (None, None);
        let (mut skip, mut skip_bytes): (Option<&str>, Option<&str>) = (None, None);
        let (mut seek, mut seek_bytes): (Option<&str>, Option<&str>) = (None, None);

        for (key, value) in validated_args {
            match key {
                constants::INPUT_FILE_ARG => config.source(value)?,
                constants::OUTPUT_FILE_ARG => config.destination(value)?,
                constants::BLOCK_SIZE_ARG => config.block_size(value)?,
                constants::INPUT_BLOCK_SIZE_ARG => config.input_block_size(value)?,
                constants::OUTPUT_BLOCK_SIZE_ARG => config.output_block_size(value)?,

                constants::COUNT_ARG => count = Some(value),
                constants::COUNT_BYTES_ARG => count_bytes = Some(value),

                constants::SKIP_ARG => skip = Some(value),
                constants::SKIP_BYTES_ARG => skip_bytes = Some(value),

                constants::SEEK_ARG => seek = Some(value),
                constants::SEEK_BYTES_ARG => seek_bytes = Some(value),

                constants::CONVERSION_ARG => config.conversions(value)?,
                constants::PRINT_STATUS_ARG => config.print_option(value)?,
                constants::IFLAG_ARG => config.iflag(value)?,
                constants::OFLAG_ARG => config.oflag(value)?,
                _ => return Err(ConfigError::UnknownArgument(key.to_string())),
            }
        }

        config.count(count, count_bytes)?;
        config.skip(skip, skip_bytes)?;
        config.seek(seek, seek_bytes)?;

        Ok(config)
    }

    /// Validates that all arguments are known and not duplicated.
    ///
    /// Internal method that checks argument keys against allowed list and detects duplicates.
    fn validate_arguments(args: &[String]) -> Result<Vec<(&str, &str)>> {
        let mut exist: HashMap<&str, bool> = HashMap::new();
        for argument in Self::get_allowed_arguments().iter() {
            exist.insert(argument, false);
        }

        let mut result: Vec<(&str, &str)> = Vec::new();
        for arg in args {
            let (key, value) = Self::parse_argument(arg)?;
            if !exist.contains_key(key) {
                return Err(ConfigError::UnknownArgument(key.to_string()));
            }

            if exist[key] {
                return Err(ConfigError::Duplicate(key.to_string()));
            }

            exist.insert(key, true);
            result.push((key, value));
        }

        Ok(result)
    }

    /// Parses a single argument string in `key=value` format.
    ///
    /// ## Returns
    ///
    /// A tuple of `(key, value)` if parsing succeeds, or `InvalidFormat` error otherwise.
    fn parse_argument(arg: &str) -> Result<(&str, &str)> {
        let splits: Vec<&str> = arg.splitn(2, '=').collect();
        if splits.len() == 1 {
            return Err(ConfigError::InvalidFormat(arg.to_string()));
        }

        if splits.len() == 2 && (splits[0].is_empty() || splits[1].is_empty()) {
            return Err(ConfigError::InvalidFormat(arg.to_string()));
        }

        Ok((splits[0], splits[1]))
    }

    /// Sets the input source (if=filename).
    ///
    /// ## Errors
    ///
    /// Returns an error if source and destination are the same file.
    ///
    /// ## Examples
    ///
    /// ```no_run
    /// # use udc::config::Config;
    /// let mut config = Config::default();
    /// config.source("input.txt")?;
    /// # Ok::<(), udc::config::ConfigError>(())
    /// ```
    pub fn source(&mut self, source: &str) -> Result<()> {
        if let SourceType::File(destination) = &self.destination
            && destination == source
        {
            return Err(ConfigError::Other(
                "input and output file cannot be the same".to_string(),
            ));
        }

        self.source = SourceType::File(source.to_string());
        Ok(())
    }

    /// Sets the output destination (of=filename).
    ///
    /// ## Errors
    ///
    /// Returns an error if source and destination are the same file.
    ///
    /// ## Examples
    ///
    /// ```no_run
    /// # use udc::config::{Config, Result};
    /// # fn main() -> Result<()> {
    /// let mut config = Config::default();
    /// config.destination("output.txt")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn destination(&mut self, destination: &str) -> Result<()> {
        if let SourceType::File(source) = &self.source
            && source == destination
        {
            return Err(ConfigError::Other(
                "input and output file cannot be the same".to_string(),
            ));
        }

        self.destination = SourceType::File(destination.to_string());
        Ok(())
    }

    /// Sets the default block size (both input and output).
    ///
    /// Parses a size string with optional multipliers (c=1, w=2, b=512, k=1024, K=1024, M=1048576, G=1073741824).
    ///
    /// ## Errors
    ///
    /// Returns an error if the size string is invalid or causes integer overflow.
    ///
    /// ## Examples
    ///
    /// ```no_run
    /// # use udc::config::{ Config, Result };
    /// # fn main() -> Result<()> {
    /// let mut config = Config::default();
    /// config.block_size("4096")?;
    /// config.block_size("1M")?;  // 1 megabyte
    /// # Ok(())
    /// # }
    /// ```
    pub fn block_size(&mut self, block_size: &str) -> Result<()> {
        let Some(parsed_size) = Self::validate_and_parse_size(block_size) else {
            return Err(ConfigError::InvalidArgumentValue(
                constants::BLOCK_SIZE_ARG.to_string(),
                block_size.to_string(),
            ));
        };

        self.block_size = Some(parsed_size);
        Ok(())
    }

    /// Sets the input block size (ibs).
    ///
    /// Takes precedence over the default block size for input operations.
    ///
    /// ## Errors
    ///
    /// Returns an error if the size string is invalid or causes integer overflow.
    ///
    /// ## Examples
    ///
    /// ```no_run
    /// # use udc::config::{ Config, Result };
    /// # fn main() -> Result<()> {
    /// let mut config = Config::default();
    /// config.input_block_size("2048")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn input_block_size(&mut self, input_block_size: &str) -> Result<()> {
        let Some(parsed_size) = Self::validate_and_parse_size(input_block_size) else {
            return Err(ConfigError::InvalidArgumentValue(
                constants::INPUT_BLOCK_SIZE_ARG.to_string(),
                input_block_size.to_string(),
            ));
        };

        self.input_block_size = Some(parsed_size);
        Ok(())
    }

    /// Sets the output block size (obs).
    ///
    /// Takes precedence over the default block size for output operations.
    ///
    /// ## Errors
    ///
    /// Returns an error if the size string is invalid or causes integer overflow.
    ///
    /// ## Examples
    ///
    /// ```no_run
    /// # use udc::config::{ Config, Result };
    /// # fn main() -> Result<()> {
    /// let mut config = Config::default();
    /// config.output_block_size("8192")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn output_block_size(&mut self, output_block_size: &str) -> Result<()> {
        let Some(parsed_size) = Self::validate_and_parse_size(output_block_size) else {
            return Err(ConfigError::InvalidArgumentValue(
                constants::OUTPUT_BLOCK_SIZE_ARG.to_string(),
                output_block_size.to_string(),
            ));
        };
        self.output_block_size = Some(parsed_size);
        Ok(())
    }

    /// Sets input flags (comma-separated options like: direct,sync,dsync,fullblock,count_bytes,skip_bytes).
    ///
    /// ## Errors
    ///
    /// Returns an error if an unknown flag is specified.
    ///
    /// ## Examples
    ///
    /// ```no_run
    /// # use udc::config::{ Config, Result };
    /// # fn main() -> Result<()> {
    /// let mut config = Config::default();
    /// config.iflag("direct,sync")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn iflag(&mut self, iflag: &str) -> Result<()> {
        self.iflag = Self::validate_iflag_option(iflag)?;
        Ok(())
    }

    /// Validates and parses input flags into a bitmask.
    ///
    /// Supported flags:
    /// - `direct` - Direct I/O
    /// - `sync` - Synchronized I/O
    /// - `dsync` - Data synchronized I/O
    /// - `fullblock` - Full block reads
    /// - `count_bytes` - Count in bytes instead of blocks
    /// - `skip_bytes` - Skip in bytes instead of blocks
    fn validate_iflag_option(value: &str) -> Result<u8> {
        let mut flags = 0;
        for option in value.split(',') {
            match option {
                constants::DIRECT => flags |= InputFlags::Direct as u8,
                constants::NONBLOCK => flags |= InputFlags::Nonblock as u8,
                constants::NOCACHE => flags |= InputFlags::Nocache as u8,
                constants::FULLBLOCK => flags |= InputFlags::FullBlock as u8,
                constants::COUNTBYTES => flags |= InputFlags::CountBytes as u8,
                constants::SKIPBYTES => flags |= InputFlags::SkipBytes as u8,
                _ => {
                    return Err(ConfigError::InvalidArgumentValue(
                        constants::IFLAG_ARG.to_string(),
                        option.to_string(),
                    ));
                }
            }
        }

        Ok(flags)
    }

    /// Sets output flags (comma-separated options like: direct,sync,dsync,seek_bytes).
    ///
    /// ## Errors
    ///
    /// Returns an error if an unknown flag is specified.
    ///
    /// ## Examples
    ///
    /// ```no_run
    /// # use udc::config::{ Config, Result };
    /// # fn main() -> Result<()> {
    /// let mut config = Config::default();
    /// config.oflag("direct,sync")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn oflag(&mut self, oflag: &str) -> Result<()> {
        self.oflag = Self::validate_oflag_option(oflag)?;
        Ok(())
    }

    /// Validates and parses output flags into a bitmask.
    ///
    /// Supported flags:
    /// - `direct` - Direct I/O
    /// - `sync` - Synchronized I/O
    /// - `dsync` - Data synchronized I/O
    /// - `seek_bytes` - Seek in bytes instead of blocks
    fn validate_oflag_option(value: &str) -> Result<u8> {
        let mut flags = 0;
        for option in value.split(',') {
            match option {
                constants::DIRECT => flags |= OutputFlags::Direct as u8,
                constants::SYNC => flags |= OutputFlags::Sync as u8,
                constants::DSYNC => flags |= OutputFlags::Dsync as u8,
                constants::EXCL => flags |= OutputFlags::Excl as u8,
                constants::NONBLOCK => flags |= OutputFlags::Nonblock as u8,
                constants::NOCACHE => flags |= OutputFlags::Nocache as u8,
                constants::SEEKBYTES => flags |= OutputFlags::SeekBytes as u8,
                _ => {
                    return Err(ConfigError::InvalidArgumentValue(
                        constants::OFLAG_ARG.to_string(),
                        option.to_string(),
                    ));
                }
            }
        }
        Ok(flags)
    }

    /// Sets the number of blocks/bytes to process from input.
    ///
    /// Can be specified as either blocks (count) or bytes (count_bytes), but not both.
    ///
    /// ## Errors
    ///
    /// Returns an error if both count and count_bytes are specified, or if values are invalid.
    ///
    /// ## Examples
    ///
    /// ```no_run
    /// # use udc::config::{ Config, Result };
    /// # fn main() -> Result<()> {
    /// let mut config = Config::default();
    /// config.count(Some("100"), None)?;  // 100 blocks
    /// # Ok(())
    /// # }
    /// ```
    pub fn count(&mut self, count: Option<&str>, count_bytes: Option<&str>) -> Result<()> {
        self.count = self.select_blocks_or_bytes(count, count_bytes, self.get_ibs())?;
        Ok(())
    }

    /// Sets the number of blocks/bytes to skip from input.
    ///
    /// Can be specified as either blocks (skip) or bytes (skip_bytes), but not both.
    ///
    /// ## Errors
    ///
    /// Returns an error if both skip and skip_bytes are specified, or if values are invalid.
    pub fn skip(&mut self, skip: Option<&str>, skip_bytes: Option<&str>) -> Result<()> {
        self.skip = self.select_blocks_or_bytes(skip, skip_bytes, self.get_ibs())?;
        Ok(())
    }

    /// Sets the number of blocks/bytes to seek in output.
    ///
    /// Can be specified as either blocks (seek) or bytes (seek_bytes), but not both.
    ///
    /// ## Errors
    ///
    /// Returns an error if both seek and seek_bytes are specified, or if values are invalid.
    pub fn seek(&mut self, seek: Option<&str>, seek_bytes: Option<&str>) -> Result<()> {
        self.seek = self.select_blocks_or_bytes(seek, seek_bytes, self.get_obs())?;
        Ok(())
    }

    /// Selects between block-based or byte-based values.
    ///
    /// Returns the value in the appropriate unit based on which parameter is specified.
    fn select_blocks_or_bytes(
        &self,
        number_of_blocks: Option<&str>,
        number_of_bytes: Option<&str>,
        multiplier: usize,
    ) -> Result<Option<usize>> {
        // invalid case
        if number_of_blocks.is_some() && number_of_bytes.is_some() {
            return Err(ConfigError::IllegalComb(
                constants::COUNT_ARG.to_string(),
                constants::COUNT_BYTES_ARG.to_string(),
            ));
        }

        // base case: neither blocks nor bytes specified → None
        if number_of_blocks.is_none() && number_of_bytes.is_none() {
            return Ok(None);
        }

        if let Some(bytes) = number_of_bytes {
            let Some(parsed_size) = Self::validate_and_parse_size(bytes) else {
                return Err(ConfigError::InvalidArgumentValue(
                    constants::COUNT_BYTES_ARG.to_string(),
                    bytes.to_string(),
                ));
            };
            return Ok(Some(parsed_size));
        }

        let value = number_of_blocks.unwrap();
        if self.iflag & InputFlags::CountBytes as u8 != 0 {
            let Some(parsed_size) = Self::validate_and_parse_size(value) else {
                return Err(ConfigError::InvalidArgumentValue(
                    constants::COUNT_ARG.to_string(),
                    value.to_string(),
                ));
            };
            return Ok(Some(parsed_size));
        }

        let Some(amount) =
            Self::parse_number(value).and_then(|parsed_value| parsed_value.checked_mul(multiplier))
        else {
            return Err(ConfigError::InvalidArgumentValue(
                constants::COUNT_ARG.to_string(),
                value.to_string(),
            ));
        };

        Ok(Some(amount))
    }

    /// Sets data conversion options (comma-separated).
    ///
    /// Supported conversions:
    /// - `lower` - Convert ASCII uppercase to lowercase
    /// - `upper` - Convert ASCII lowercase to uppercase
    /// - `swap` - Swap every pair of bytes
    /// - `notrunc` - Do not truncate output file
    /// - `sync` - Synchronize output
    /// - `sparse` - Seek rather than write zeros
    /// - `noerror` - Continue on read errors
    ///
    /// ## Errors
    ///
    /// Returns an error if mutually exclusive options are specified (e.g., lower and upper).
    ///
    /// ## Examples
    ///
    /// ```no_run
    /// # use udc::config::{ Config, Result };
    /// # fn main() -> Result<()> {
    /// let mut config = Config::default();
    /// config.conversions("notrunc,sync")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn conversions(&mut self, value: &str) -> Result<()> {
        let (mut data_conversions, mut output_conversions) = (0, 0);

        for option in value.split(',') {
            match option {
                constants::CONVERSION_OPTION_LOWER_CASE => {
                    data_conversions |= DataOps::ToLower as u8
                }
                constants::CONVERSION_OPTION_UPPER_CASE => {
                    data_conversions |= DataOps::ToUpper as u8
                }
                constants::CONVERSION_OPTION_SWAP => data_conversions |= DataOps::Swap as u8,
                constants::OUTPUT_OPTION_NO_TRUNC => output_conversions |= FileOps::NoTrunc as u8,
                constants::OUTPUT_OPTION_SYNC => output_conversions |= FileOps::Sync as u8,
                constants::OUTPUT_OPTION_SPARSE => output_conversions |= FileOps::Sparse as u8,
                constants::OUTPUT_OPTION_NO_ERROR => output_conversions |= FileOps::NoError as u8,
                _ => {
                    return Err(ConfigError::InvalidArgumentValue(
                        constants::CONVERSION_ARG.to_string(),
                        option.to_string(),
                    ));
                }
            }
        }

        if data_conversions & DataOps::ToLower as u8 != 0
            && data_conversions & DataOps::ToUpper as u8 != 0
        {
            return Err(ConfigError::IllegalCombWithValue(
                constants::CONVERSION_ARG.to_string(),
                constants::CONVERSION_OPTION_LOWER_CASE.to_string(),
                constants::CONVERSION_OPTION_UPPER_CASE.to_string(),
            ));
        }

        self.data_convs = data_conversions;
        self.write_convs = output_conversions;
        Ok(())
    }

    /// Sets the status reporting level.
    ///
    /// Supported values:
    /// - `none` - No status output
    /// - `noxfer` - No transfer speed
    /// - `progress` - Show progress
    ///
    /// ## Errors
    ///
    /// Returns an error if an invalid status option is specified.
    pub fn print_option(&mut self, value: &str) -> Result<()> {
        self.print_status = match value {
            constants::NO_PRINT => PrintStatus::None,
            constants::NOXFER_PRINT => PrintStatus::Noxfer,
            constants::PROGRESS_PRINT => PrintStatus::Progress,
            _ => {
                return Err(ConfigError::InvalidArgumentValue(
                    constants::PRINT_STATUS_ARG.to_string(),
                    value.to_string(),
                ));
            }
        };
        Ok(())
    }

    // ========== GETTERS ==========

    /// Returns a reference to the input source.
    ///
    /// ## Examples
    ///
    /// ```no_run
    /// # use udc::config::Config;
    /// let config = Config::default();
    /// let source = config.get_source();
    /// ```
    pub fn get_source(&self) -> &SourceType {
        &self.source
    }

    /// Returns a reference to the output destination.
    ///
    /// ## Examples
    ///
    /// ```no_run
    /// # use udc::config::Config;
    /// let config = Config::default();
    /// let destination = config.get_destination();
    /// ```
    pub fn get_destination(&self) -> &SourceType {
        &self.destination
    }

    /// Returns the input block size.
    ///
    /// Hierarchy: ibs > bs > default (8192 bytes)
    ///
    /// ## Examples
    ///
    /// ```no_run
    /// # use udc::config::Config;
    /// let config = Config::default();
    /// let ibs = config.get_ibs();
    /// ```
    pub fn get_ibs(&self) -> usize {
        match self.input_block_size {
            Some(size) => size,
            None => match self.block_size {
                Some(size) => size,
                None => constants::DEFAULT_BLOCK_SIZE,
            },
        }
    }

    /// Returns the output block size.
    ///
    /// Hierarchy: obs > bs > default (8192 bytes)
    ///
    /// ## Examples
    ///
    /// ```no_run
    /// # use udc::config::Config;
    /// let config = Config::default();
    /// let obs = config.get_obs();
    /// ```
    pub fn get_obs(&self) -> usize {
        match self.output_block_size {
            Some(size) => size,
            None => match self.block_size {
                Some(size) => size,
                None => constants::DEFAULT_BLOCK_SIZE,
            },
        }
    }

    /// Returns a reference to the seek position (in bytes).
    ///
    /// ## Examples
    ///
    /// ```no_run
    /// # use udc::config::Config;
    /// let config = Config::default();
    /// if let Some(seek) = config.get_seek() {
    ///     println!("Seek to: {}", seek);
    /// }
    /// ```
    pub fn get_seek(&self) -> &Option<usize> {
        &self.seek
    }

    /// Returns a reference to the skip amount (in bytes).
    ///
    /// ## Examples
    ///
    /// ```no_run
    /// # use udc::config::Config;
    /// let config = Config::default();
    /// if let Some(skip) = config.get_skip() {
    ///     println!("Skip: {}", skip);
    /// }
    /// ```
    pub fn get_skip(&self) -> &Option<usize> {
        &self.skip
    }

    /// Returns a reference to the count (in bytes).
    ///
    /// ## Examples
    ///
    /// ```no_run
    /// # use udc::config::Config;
    /// let config = Config::default();
    /// if let Some(count) = config.get_count() {
    ///     println!("Process {} bytes", count);
    /// }
    /// ```
    pub fn get_count(&self) -> &Option<usize> {
        &self.count
    }

    /// Returns a reference to the print/status option.
    ///
    /// ## Examples
    ///
    /// ```no_run
    /// # use udc::config::Config;
    /// let config = Config::default();
    /// let status = config.get_print_option();
    /// ```
    pub fn get_print_option(&self) -> &PrintStatus {
        &self.print_status
    }

    // ========== CONVERSION CHECKERS ==========

    /// Returns true if output should be truncated (default behavior)
    ///
    /// Returns false if `notrunc` conversion is specified
    ///
    /// ## Examples
    ///
    /// ```no_run
    /// # use udc::config::Config;
    /// let config = Config::default();
    /// if config.is_truncate() {
    ///     println!("Will truncate output file");
    /// }
    /// ```
    pub fn is_truncate(&self) -> bool {
        self.write_convs & FileOps::NoTrunc as u8 == 0
    }

    /// Returns true if output should be synchronized (sync conversion)
    pub fn is_sync(&self) -> bool {
        self.write_convs & FileOps::Sync as u8 != 0
    }

    /// Returns true if sparse file output is enabled
    pub fn is_sparse(&self) -> bool {
        self.write_convs & FileOps::Sparse as u8 != 0
    }

    /// Returns true if the operation should continue on read errors
    pub fn is_noerror(&self) -> bool {
        self.write_convs & FileOps::NoError as u8 != 0
    }

    /// Returns the data conversion bitmask
    pub fn get_data_convs(&self) -> u8 {
        self.data_convs
    }

    /// Returns true if lowercase conversion is enabled
    pub fn is_to_lower(&self) -> bool {
        self.data_convs & DataOps::ToLower as u8 != 0
    }

    /// Returns true if uppercase conversion is enabled
    pub fn is_to_upper(&self) -> bool {
        self.data_convs & DataOps::ToUpper as u8 != 0
    }

    /// Returns true if byte swapping is enabled
    pub fn is_swap(&self) -> bool {
        self.data_convs & DataOps::Swap as u8 != 0
    }

    // ========== FLAG CHECKERS ==========

    /// Return input flags bitmask.
    pub fn get_iflag(&self) -> u8 {
        self.iflag
    }

    pub fn is_direct_input(&self) -> bool {
        self.iflag & InputFlags::Direct as u8 != 0
    }

    pub fn is_fullblock(&self) -> bool {
        self.iflag & InputFlags::FullBlock as u8 != 0
    }

    /// Return output flags bitmask.
    pub fn get_oflag(&self) -> u8 {
        self.oflag
    }

    pub fn is_direct_output(&self) -> bool {
        self.oflag & OutputFlags::Direct as u8 != 0
    }

    pub fn is_exec(&self) -> bool {
        self.oflag & OutputFlags::Excl as u8 != 0
    }

    // ========== PRIVATE HELPERS ==========

    /// Parses and validates a size string with optional multiplier suffix.
    ///
    /// Supported multipliers:
    /// - `c` = 1 byte
    /// - `w` = 2 bytes (word)
    /// - `b` = 512 bytes (block)
    /// - `k` or `K` = 1024 bytes
    /// - `M` = 1048576 bytes
    /// - `G` = 1073741824 bytes
    ///
    /// Returns `None` if the size is invalid or causes integer overflow.
    ///
    /// ## Examples
    ///
    /// - `"1024"` → 1024
    /// - `"1M"` → 1048576
    /// - `"2G"` → 2147483648
    fn validate_and_parse_size(size: &str) -> Option<usize> {
        let base_value: usize;
        let mut multiplier_value = 1usize;

        let multiplier_char: char = size.chars().last().unwrap();
        if multiplier_char.is_alphabetic() {
            multiplier_value = Self::convert_multiplier(multiplier_char)?;
            base_value = Self::parse_number(&size[..size.len() - 1])?;
        } else {
            base_value = Self::parse_number(&size[..size.len()])?;
        }

        base_value.checked_mul(multiplier_value)
    }

    /// Converts a size multiplier character to its numeric value.
    ///
    /// Returns `None` if the character is not a recognized multiplier.
    #[inline]
    fn convert_multiplier(size_multiplier: char) -> Option<usize> {
        if size_multiplier.is_numeric() {
            return Some(1);
        }

        let value = match size_multiplier {
            'c' => 1,
            'w' => 2,
            'b' => 512,
            'k' | 'K' => 1024,
            'M' => 1024 * 1024,
            'G' => 1024 * 1024 * 1024,
            _ => return None,
        };

        Some(value)
    }

    /// Parses a numeric string into a usize.
    ///
    /// Returns `None` if parsing fails.
    #[inline]
    fn parse_number(value: &str) -> Option<usize> {
        value.parse::<usize>().ok()
    }

    /// Returns the list of allowed argument keys.
    #[inline]
    fn get_allowed_arguments() -> Vec<&'static str> {
        vec![
            constants::INPUT_FILE_ARG,
            constants::OUTPUT_FILE_ARG,
            constants::BLOCK_SIZE_ARG,
            constants::INPUT_BLOCK_SIZE_ARG,
            constants::OUTPUT_BLOCK_SIZE_ARG,
            constants::COUNT_ARG,
            constants::SKIP_ARG,
            constants::SEEK_ARG,
            constants::COUNT_BYTES_ARG,
            constants::SKIP_BYTES_ARG,
            constants::SEEK_BYTES_ARG,
            constants::CONVERSION_ARG,
            constants::PRINT_STATUS_ARG,
            constants::IFLAG_ARG,
            constants::OFLAG_ARG,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_error_display() {
        assert_eq!(
            ConfigError::UnknownArgument("bad_arg".to_string()).to_string(),
            "udc: config: Unknown argument: bad_arg"
        );
        assert_eq!(
            ConfigError::InvalidFormat("if".to_string()).to_string(),
            "udc: config: Invalid argument format: if"
        );
        assert_eq!(
            ConfigError::InvalidArgumentValue("bs".to_string(), "-1".to_string()).to_string(),
            "udc: config: bs: Invalid argument value -1"
        );
        assert_eq!(
            ConfigError::Duplicate("if".to_string()).to_string(),
            "udc: config: Argument specified multiple times: if"
        );
        assert_eq!(
            ConfigError::IllegalComb("count".to_string(), "count_bytes".to_string()).to_string(),
            "udc: config: Illegal argument combination: count and count_bytes"
        );
        assert_eq!(
            ConfigError::IllegalCombWithValue(
                "conv".to_string(),
                "lcase".to_string(),
                "ucase".to_string()
            )
            .to_string(),
            "udc: config: conv: Illegal values combination: lcase ucase"
        );
        assert_eq!(
            ConfigError::OutOfBounds("bs".to_string(), 0).to_string(),
            "udc: config: Argument out of bounds: bs = 0"
        );
        assert_eq!(
            ConfigError::Other("custom error".to_string()).to_string(),
            "udc: config: custom error"
        );
    }

    // --- ARGUMENT PARSING TESTS ---

    #[test]
    fn test_parse_argument_valid() {
        assert_eq!(
            Config::parse_argument("if=test.txt").unwrap(),
            ("if", "test.txt")
        );
        assert_eq!(Config::parse_argument("bs=1M").unwrap(), ("bs", "1M"));
    }

    #[test]
    fn test_parse_argument_invalid() {
        assert!(matches!(
            Config::parse_argument("invalid_format"),
            Err(ConfigError::InvalidFormat(_))
        ));
        assert!(matches!(
            Config::parse_argument("if="),
            Err(ConfigError::InvalidFormat(_))
        ));
        assert!(matches!(
            Config::parse_argument("=test.txt"),
            Err(ConfigError::InvalidFormat(_))
        ));
    }

    // --- SIZE & MULTIPLIER TESTS ---

    #[test]
    fn test_convert_multiplier() {
        assert_eq!(Config::convert_multiplier('c'), Some(1));
        assert_eq!(Config::convert_multiplier('w'), Some(2));
        assert_eq!(Config::convert_multiplier('b'), Some(512));
        assert_eq!(Config::convert_multiplier('k'), Some(1024));
        assert_eq!(Config::convert_multiplier('K'), Some(1024));
        assert_eq!(Config::convert_multiplier('M'), Some(1048576));
        assert_eq!(Config::convert_multiplier('G'), Some(1073741824));
        assert_eq!(Config::convert_multiplier('5'), Some(1)); // Numeric edge case
        assert_eq!(Config::convert_multiplier('z'), None);
    }

    #[test]
    fn test_validate_and_parse_size() {
        assert_eq!(Config::validate_and_parse_size("512"), Some(512));
        assert_eq!(Config::validate_and_parse_size("1k"), Some(1024));
        assert_eq!(Config::validate_and_parse_size("2M"), Some(2097152));
        assert_eq!(Config::validate_and_parse_size("invalid"), None);
        assert_eq!(Config::validate_and_parse_size("1z"), None);
    }

    // --- FILE PATH VALIDATION TESTS ---

    #[test]
    fn test_source_and_destination_conflict() {
        let mut config = Config::default();
        config.source("same_file.txt").unwrap();
        let err = config.destination("same_file.txt").unwrap_err();
        assert!(matches!(err, ConfigError::Other(_)));
    }

    #[test]
    fn test_source_and_destination_valid() {
        let mut config = Config::default();
        config.source("input.txt").unwrap();
        config.destination("output.txt").unwrap();

        assert_eq!(
            config.get_source(),
            &SourceType::File("input.txt".to_string())
        );
        assert_eq!(
            config.get_destination(),
            &SourceType::File("output.txt".to_string())
        );
    }

    // --- BLOCK SIZE FALLBACK HIERARCHY TESTS ---

    #[test]
    fn test_block_sizes_hierarchy() {
        let mut config = Config::default();

        // Default fallback
        assert_eq!(config.get_ibs(), constants::DEFAULT_BLOCK_SIZE);
        assert_eq!(config.get_obs(), constants::DEFAULT_BLOCK_SIZE);

        // Global bs overriding defaults
        config.block_size("1K").unwrap();
        assert_eq!(config.get_ibs(), 1024);
        assert_eq!(config.get_obs(), 1024);

        // Specific ibs/obs overriding bs
        config.input_block_size("2M").unwrap();
        config.output_block_size("2K").unwrap();
        assert_eq!(config.get_ibs(), 2097152);
        assert_eq!(config.get_obs(), 2048);
    }

    // --- BLOCK OR BYTES SELECTION TESTS ---

    #[test]
    fn test_select_blocks_or_bytes() {
        let mut config = Config::default();
        config.block_size("512").unwrap(); // multiplier = 512

        // Neither provided
        assert_eq!(
            config.select_blocks_or_bytes(None, None, 512).unwrap(),
            None
        );

        // Both provided (Illegal)
        assert!(matches!(
            config.select_blocks_or_bytes(Some("10"), Some("10"), 512),
            Err(ConfigError::IllegalComb(_, _))
        ));

        // Only bytes
        assert_eq!(
            config
                .select_blocks_or_bytes(None, Some("1k"), 512)
                .unwrap(),
            Some(1024)
        );

        // Only blocks (translates to blocks * multiplier)
        assert_eq!(
            config.select_blocks_or_bytes(Some("2"), None, 512).unwrap(),
            Some(1024)
        );

        // Only blocks, but count_bytes flag is active
        config.iflag = InputFlags::CountBytes as u8;
        assert_eq!(
            config
                .select_blocks_or_bytes(Some("1k"), None, 512)
                .unwrap(),
            Some(1024)
        );
    }

    // --- BITMASK PARSING & GETTERS TESTS ---

    #[test]
    fn test_iflag_parsing() {
        let mut config = Config::default();
        config.iflag("direct,nonblock,fullblock").unwrap();

        assert_eq!(
            config.iflag & InputFlags::Direct as u8,
            InputFlags::Direct as u8
        );
        assert_eq!(
            config.iflag & InputFlags::Nonblock as u8,
            InputFlags::Nonblock as u8
        );
        assert_eq!(
            config.iflag & InputFlags::FullBlock as u8,
            InputFlags::FullBlock as u8
        );
        assert_eq!(config.iflag & InputFlags::Nocache as u8, 0); // Not set

        assert!(matches!(
            config.iflag("invalid_flag"),
            Err(ConfigError::InvalidArgumentValue(_, _))
        ));
    }

    #[test]
    fn test_oflag_parsing() {
        let mut config = Config::default();
        config.oflag("dsync,seek_bytes").unwrap();

        assert_eq!(
            config.oflag & OutputFlags::Dsync as u8,
            OutputFlags::Dsync as u8
        );
        assert_eq!(
            config.oflag & OutputFlags::SeekBytes as u8,
            OutputFlags::SeekBytes as u8
        );
    }

    #[test]
    fn test_conversions_parsing_and_getters() {
        let mut config = Config::default();
        config
            .conversions("lcase,swab,notrunc,sparse,noerror")
            .unwrap();

        assert!(config.is_to_lower());
        assert!(config.is_swap());
        assert!(!config.is_to_upper());

        assert!(!config.is_truncate()); // notrunc flag flips this logic
        assert!(config.is_sparse());
        assert!(config.is_noerror());
        assert!(!config.is_sync());

        // Test conflicting options
        let err = config.conversions("lcase,ucase").unwrap_err();
        assert!(matches!(err, ConfigError::IllegalCombWithValue(_, _, _)));
    }

    // --- PRINT STATUS TESTS ---

    #[test]
    fn test_print_option() {
        let mut config = Config::default();

        config.print_option(constants::NO_PRINT).unwrap();
        assert_eq!(config.get_print_option(), &PrintStatus::None);

        config.print_option(constants::PROGRESS_PRINT).unwrap();
        assert_eq!(config.get_print_option(), &PrintStatus::Progress);

        assert!(matches!(
            config.print_option("invalid_status"),
            Err(ConfigError::InvalidArgumentValue(_, _))
        ));
    }
}
