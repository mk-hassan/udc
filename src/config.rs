use crate::constants;

#[derive(Debug)]
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
    print_option: PrintOption
}

#[derive(Debug, PartialEq, Eq)]
pub enum SourceType {
  File(String),
  Standard
}

#[derive(Debug, PartialEq, Eq)]
pub enum PrintOption {
    None,
    Noxfer,
    Progress,
    Default
}

pub enum DataOps {
    ToLower = 1,
    ToUpper = 2,
    Swap = 4,
}

pub enum WriteOps {
    NoTrunc = 1,
    Sync = 2,
    Sparse = 4,
    NoError = 8
}

impl Config {
    pub fn new() -> Self {
        Config {
            source: SourceType::Standard,
            destination: SourceType::Standard,
            block_size: None,
            input_block_size: None,
            output_block_size: None,
            count: None,
            skip: None,
            seek: None,
            data_convs: 0,
            write_convs: 0,
            print_option: PrintOption::Default
        }
    }

    pub fn build(args: &[String]) -> Result<Self, String> {
        let mut config = Config::new();

        for arg in args[1..].iter() {
            let (key, value) = Self::parse_and_validate_argument(arg)?;
            match key {
                constants::INPUT_FILE_ARG => {
                    if let SourceType::File(path) = &config.destination {
                        Self::check_paths(value, path)?
                    }
                    config = config.source(SourceType::File(value.to_string()));
                }
                constants::OUTPUT_FILE_ARG => {
                    if let SourceType::File(path) = &config.source {
                        Self::check_paths(path, value)?
                    }
                    config = config.destination(SourceType::File(value.to_string()));
                }
                constants::BLOCK_SIZE_ARG => config = config.block_size(Self::parse_and_validate_block_size(value)?),
                constants::INPUT_BLOCK_SIZE_ARG => config = config.input_block_size(Self::parse_and_validate_block_size(value)?),
                constants::OUTPUT_BLOCK_SIZE_ARG => config = config.output_block_size(Self::parse_and_validate_block_size(value)?),
                constants::COUNT_ARG => config = config.count(Self::parse_number(value)?),
                constants::SKIP_ARG => config = config.skip(Self::parse_number(value)?),
                constants::SEEK_ARG => config = config.seek(Self::parse_number(value)?),
                constants::CONVERSION_ARG => {
                    let (data_ops, output_ops) = Self::parse_conversions(value)?;
                    config = config.data_convs(data_ops);
                    config = config.write_convs(output_ops);
                }
                constants::PRINT_STATUS_ARG => {
                    let status_option = Self::parse_and_validate_status_option(value)?;
                    config = config.print_option(status_option);
                }
                _ => return Err(format!("ccdd: Unknown argument {}", key))
            }
        }

        Ok(config)
    }

    // helper functions for parsing and validating arguments
    fn parse_and_validate_argument(arg: &str) -> Result<(&str, &str), &'static str> {
        let splits: Vec<&str> = arg.splitn(2, |chr| chr == '=').collect();
        if splits.len() == 1 {
            return Err(constants::INVALID_ARGUMENT_FORMAT);
        }

        if splits.len() == 2 && (splits[0].is_empty() || splits[1].is_empty()) {
            return Err(constants::INVALID_ARGUMENT_KEY_VALUE);
        }

        Ok((splits[0], splits[1]))
    }

    fn parse_and_validate_block_size(size: &str) -> Result<usize, &'static str> {        
        let multiplier = &size[size.len() - 1..];
        if !(multiplier >= "a" && multiplier <= "z" || multiplier >= "A" && multiplier <= "Z") {
            return Ok(Config::parse_number(size)?);
        }

        let base_value = Config::parse_number(&size[..size.len() - 1])?;
        let multiplier_value = match multiplier {
            "c" | "b" => 1,
            "w" => 8,
            "k" | "K" => 1024,
            "M" => 1024 * 1024,
            "G" => 1024 * 1024 * 1024,
            _ => return Err(constants::INVALID_BLOCK_SIZE_MULTIPLIER)
        };
        
        let final_value = base_value
            .checked_mul(multiplier_value)
            .ok_or_else(|| constants::BLOCK_SIZE_EXCEEDS_LIMIT)?;

        if final_value >= constants::MAX_BLOCK_SIZE || final_value == 0 {
            return Err(constants::INVALID_BLOCK_SIZE);
        }

        Ok(final_value)
    }

    fn parse_number(value: &str) -> Result<usize, &'static str> {
        value.parse::<usize>().map_err(|_| constants::INVALID_ARGUMENT_VALUE)
    }

    fn parse_conversions(value: &str) -> Result<(u8, u8), &'static str> {
        let (mut data_conversions, mut output_conversions) = (0, 0);

        for option in value.split(',') {
            match option {
                constants::CONVERSION_OPTION_LOWER_CASE => {
                    if data_conversions & DataOps::ToUpper as u8 != 0 {
                        return Err(constants::LOWER_CASE_ILLEGAL_CONVERSION_COMBINATION);
                    }
                    data_conversions |= DataOps::ToLower as u8;
                }
                constants::CONVERSION_OPTION_UPPER_CASE => {
                    if data_conversions & DataOps::ToLower as u8 != 0 {
                        return Err(constants::UPPER_CASE_ILLEGAL_CONVERSION_COMBINATION);
                    }
                    data_conversions |= DataOps::ToUpper as u8;
                }
                constants::CONVERSION_OPTION_SWAP => data_conversions |= DataOps::Swap as u8,
                constants::OUTPUT_OPTION_NO_TRUNC => output_conversions |= WriteOps::NoTrunc as u8,
                constants::OUTPUT_OPTION_SYNC => output_conversions |= WriteOps::Sync as u8,
                constants::OUTPUT_OPTION_SPARSE => output_conversions |= WriteOps::Sparse as u8,
                constants::OUTPUT_OPTION_NO_ERROR => output_conversions |= WriteOps::NoError as u8,
                _ => return Err(constants::INVALID_CONVERSION_OPTION)
            }
        }

        Ok((data_conversions, output_conversions))
    }

    fn check_paths(source: &str, destination: &str) -> Result<(), &'static str> {
        if source == destination {
            return Err(constants::INVALID_INPUT_OUTPUT_COMBINATION);
        }
        Ok(())
    }

    fn parse_and_validate_status_option(value: &str) -> Result<PrintOption, &'static str> {
        match value {
            constants::NO_PRINT => Ok(PrintOption::None),
            constants::NOXFER_PRINT => Ok(PrintOption::Noxfer),
            constants::PROGRESS_PRINT => Ok(PrintOption::Progress),
            _ => Err(constants::INVALID_ARGUMENT_VALUE)
        }
    }

    // setters
    pub fn source(mut self, source: SourceType) -> Self {
        self.source = source;
        self
    }

    pub fn destination(mut self, destination: SourceType) -> Self {
        self.destination = destination;
        self
    }

    pub fn block_size(mut self, block_size: usize) -> Self {
        self.block_size = Some(block_size);
        self
    }

    pub fn input_block_size(mut self, input_block_size: usize) -> Self {
        self.input_block_size = Some(input_block_size);
        self
    }

    pub fn output_block_size(mut self, output_block_size: usize) -> Self {
        self.output_block_size = Some(output_block_size);
        self
    }

    pub fn count(mut self, count: usize) -> Self {
        self.count = Some(count);
        self
    }

    pub fn skip(mut self, skip: usize) -> Self {
        self.skip = Some(skip);
        self
    }

    pub fn seek(mut self, seek: usize) -> Self {
        self.seek = Some(seek);
        self
    }

    pub fn write_convs(mut self, convs: u8) -> Self {
        self.write_convs = convs;
        self
    }

    pub fn data_convs(mut self, convs: u8) -> Self {
        self.data_convs = convs;
        self
    }

    pub fn print_option(mut self, status_option: PrintOption) -> Self {
        self.print_option = status_option;
        self
    }

    // getters
    pub fn get_source(&self) -> &SourceType {
        &self.source  
    }

    pub fn get_destination(&self) -> &SourceType {
        &self.destination
    }

    pub fn get_ibs(&self) -> usize {
        match self.input_block_size {
            Some(size) => size,
            None => match self.block_size {
                Some(size) => size,
                None => constants::DEFAULT_BLOCK_SIZE
            }
        }
    }

    pub fn get_obs(&self) -> usize {
        match self.output_block_size {
            Some(size) => size,
            None => match self.block_size {
                Some(size) => size,
                None => constants::DEFAULT_BLOCK_SIZE
            }
        }
    }

    pub fn get_seek(&self) -> Option<usize> {
        self.seek
    }

    pub fn get_skip(&self) -> Option<usize> {
        self.skip
    }

    pub fn get_count(&self) -> Option<usize> {
        self.count
    }

    pub fn get_print_option(&self) -> &PrintOption {
        &self.print_option
    }

    // read & write options
    pub fn is_truncate(&self) -> bool {
        self.write_convs & WriteOps::NoTrunc as u8 == 0
    }

    pub fn is_sync(&self) -> bool {
        self.write_convs & WriteOps::Sync as u8 != 0
    }

    pub fn is_sparse(&self) -> bool {
        self.write_convs & WriteOps::Sparse as u8 != 0
    }

    pub fn is_noerror(&self) -> bool {
        self.write_convs & WriteOps::NoError as u8 != 0
    }

    pub fn is_to_lower(&self) -> bool {
        self.data_convs & DataOps::ToLower as u8 != 0
    }

    pub fn is_to_upper(&self) -> bool {
        self.data_convs & DataOps::ToUpper as u8 != 0
    }

    pub fn is_swap(&self) -> bool {
        self.data_convs & DataOps::Swap as u8 != 0
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    // ─── parse_and_validate_argument ─────────────────────────────────────────

    #[test]
    fn test_parse_arg_valid() {
        let result = Config::parse_and_validate_argument("if=input.txt").unwrap();
        assert_eq!(result, ("if", "input.txt"));
    }

    #[test]
    fn test_parse_arg_value_contains_equals() {
        // splitn(2) means only the first '=' is the separator
        let result = Config::parse_and_validate_argument("of=path=with=equals").unwrap();
        assert_eq!(result, ("of", "path=with=equals"));
    }

    #[test]
    fn test_parse_arg_no_separator() {
        assert!(Config::parse_and_validate_argument("ifinput.txt").is_err());
    }

    #[test]
    fn test_parse_arg_empty_value() {
        assert!(Config::parse_and_validate_argument("if=").is_err());
    }

    #[test]
    fn test_parse_arg_empty_key() {
        assert!(Config::parse_and_validate_argument("=value").is_err());
    }

    #[test]
    fn test_parse_arg_empty_key_and_value() {
        assert!(Config::parse_and_validate_argument("=").is_err());
    }

    // ─── parse_and_validate_block_size ───────────────────────────────────────

    #[test]
    fn test_block_size_numeric_only() {
        assert_eq!(Config::parse_and_validate_block_size("1").unwrap(), 1);
        assert_eq!(Config::parse_and_validate_block_size("512").unwrap(), 512);
        assert_eq!(Config::parse_and_validate_block_size("4096").unwrap(), 4096);
    }

    #[test]
    fn test_block_size_all_multipliers() {
        assert_eq!(Config::parse_and_validate_block_size("1c").unwrap(), 1);
        assert_eq!(Config::parse_and_validate_block_size("1b").unwrap(), 1);
        assert_eq!(Config::parse_and_validate_block_size("1w").unwrap(), 8);
        assert_eq!(Config::parse_and_validate_block_size("1k").unwrap(), 1024);
        assert_eq!(Config::parse_and_validate_block_size("1K").unwrap(), 1024);
        assert_eq!(Config::parse_and_validate_block_size("1M").unwrap(), 1024 * 1024);
        // G multiplier: 1G == MAX_BLOCK_SIZE and is rejected — see test_block_size_at_max_boundary
    }

    #[test]
    fn test_block_size_multi_digit_with_multiplier() {
        assert_eq!(Config::parse_and_validate_block_size("4k").unwrap(), 4 * 1024);
        assert_eq!(Config::parse_and_validate_block_size("2M").unwrap(), 2 * 1024 * 1024);
        assert_eq!(Config::parse_and_validate_block_size("8w").unwrap(), 64);
    }

    #[test]
    fn test_block_size_at_max_boundary() {
        // 1023M is just under MAX_BLOCK_SIZE (1G) → valid
        assert!(Config::parse_and_validate_block_size("1023M").is_ok());
        // 1G == MAX_BLOCK_SIZE → invalid (>= check)
        assert!(Config::parse_and_validate_block_size("1G").is_err());
        // 2G > MAX_BLOCK_SIZE → invalid
        assert!(Config::parse_and_validate_block_size("2G").is_err());
    }

    #[test]
    fn test_block_size_zero_with_multiplier() {
        assert!(Config::parse_and_validate_block_size("0k").is_err());
        assert!(Config::parse_and_validate_block_size("0M").is_err());
    }

    #[test]
    fn test_block_size_invalid_multiplier() {
        assert!(Config::parse_and_validate_block_size("4x").is_err());
        assert!(Config::parse_and_validate_block_size("1z").is_err());
        assert!(Config::parse_and_validate_block_size("1d").is_err());
    }

    #[test]
    fn test_block_size_non_numeric_base() {
        assert!(Config::parse_and_validate_block_size("xk").is_err());
        assert!(Config::parse_and_validate_block_size("k").is_err());
        assert!(Config::parse_and_validate_block_size("abcM").is_err());
    }

    #[test]
    fn test_block_size_exceeds_max_with_multiplier() {
        let size = format!("{}k", constants::MAX_BLOCK_SIZE + 1);
        assert!(Config::parse_and_validate_block_size(&size).is_err());
    }

    #[test]
    fn test_block_size_overflow() {
        // base value that causes usize overflow when multiplied by 1024
        let overflow_base = usize::MAX / 1024 + 1;
        let size = format!("{}k", overflow_base);
        assert!(Config::parse_and_validate_block_size(&size).is_err());
    }

    // ─── parse_conversions ───────────────────────────────────────────────────

    #[test]
    fn test_conversions_lcase() {
        let (data, write) = Config::parse_conversions("lcase").unwrap();
        assert_eq!(data, DataOps::ToLower as u8);
        assert_eq!(write, 0);
    }

    #[test]
    fn test_conversions_ucase() {
        let (data, write) = Config::parse_conversions("ucase").unwrap();
        assert_eq!(data, DataOps::ToUpper as u8);
        assert_eq!(write, 0);
    }

    #[test]
    fn test_conversions_swap() {
        let (data, write) = Config::parse_conversions("swap").unwrap();
        assert_eq!(data, DataOps::Swap as u8);
        assert_eq!(write, 0);
    }

    #[test]
    fn test_conversions_notrunc() {
        let (data, write) = Config::parse_conversions("notrunc").unwrap();
        assert_eq!(data, 0);
        assert_eq!(write, WriteOps::NoTrunc as u8);
    }

    #[test]
    fn test_conversions_sync() {
        let (data, write) = Config::parse_conversions("sync").unwrap();
        assert_eq!(data, 0);
        assert_eq!(write, WriteOps::Sync as u8);
    }

    #[test]
    fn test_conversions_sparse() {
        let (data, write) = Config::parse_conversions("sparse").unwrap();
        assert_eq!(data, 0);
        assert_eq!(write, WriteOps::Sparse as u8);
    }

    #[test]
    fn test_conversions_noerror() {
        let (data, write) = Config::parse_conversions("noerror").unwrap();
        assert_eq!(data, 0);
        assert_eq!(write, WriteOps::NoError as u8);
    }

    #[test]
    fn test_conversions_all_write_ops() {
        let (data, write) = Config::parse_conversions("notrunc,sync,sparse,noerror").unwrap();
        assert_eq!(data, 0);
        assert_eq!(
            write,
            WriteOps::NoTrunc as u8 | WriteOps::Sync as u8
                | WriteOps::Sparse as u8 | WriteOps::NoError as u8
        );
    }

    #[test]
    fn test_conversions_data_and_write_combined() {
        let (data, write) = Config::parse_conversions("ucase,notrunc,sparse").unwrap();
        assert_eq!(data, DataOps::ToUpper as u8);
        assert_eq!(write, WriteOps::NoTrunc as u8 | WriteOps::Sparse as u8);
    }

    #[test]
    fn test_conversions_swap_with_write_ops() {
        let (data, write) = Config::parse_conversions("swap,sync,noerror").unwrap();
        assert_eq!(data, DataOps::Swap as u8);
        assert_eq!(write, WriteOps::Sync as u8 | WriteOps::NoError as u8);
    }

    #[test]
    fn test_conversions_lcase_then_ucase_error() {
        assert!(Config::parse_conversions("lcase,ucase").is_err());
    }

    #[test]
    fn test_conversions_ucase_then_lcase_error() {
        assert!(Config::parse_conversions("ucase,lcase").is_err());
    }

    #[test]
    fn test_conversions_unknown_option() {
        assert!(Config::parse_conversions("unknown").is_err());
    }

    #[test]
    fn test_conversions_unknown_mixed_with_valid() {
        assert!(Config::parse_conversions("lcase,bogus").is_err());
    }

    // ─── check_paths ─────────────────────────────────────────────────────────

    #[test]
    fn test_check_paths_same_is_error() {
        assert!(Config::check_paths("file.txt", "file.txt").is_err());
    }

    #[test]
    fn test_check_paths_different_is_ok() {
        assert!(Config::check_paths("input.txt", "output.txt").is_ok());
    }

    // ─── build ───────────────────────────────────────────────────────────────

    #[test]
    fn test_build_no_args_uses_defaults() {
        let config = Config::build(&["program".to_string()]).unwrap();
        assert_eq!(config.source, SourceType::Standard);
        assert_eq!(config.destination, SourceType::Standard);
        assert_eq!(config.get_ibs(), constants::DEFAULT_BLOCK_SIZE);
        assert_eq!(config.get_obs(), constants::DEFAULT_BLOCK_SIZE);
        assert!(config.get_count().is_none());
        assert!(config.get_skip().is_none());
        assert!(config.get_seek().is_none());
        assert_eq!(config.data_convs, 0);
        assert_eq!(config.write_convs, 0);
    }

    #[test]
    fn test_build_if_and_of() {
        let args = ["program", "if=input.txt", "of=output.txt"]
            .map(str::to_string);
        let config = Config::build(&args).unwrap();
        assert_eq!(config.source, SourceType::File("input.txt".to_string()));
        assert_eq!(config.destination, SourceType::File("output.txt".to_string()));
    }

    #[test]
    fn test_build_of_before_if() {
        // argument order should not matter for valid configurations
        let args = ["program", "of=output.txt", "if=input.txt"]
            .map(str::to_string);
        let config = Config::build(&args).unwrap();
        assert_eq!(config.source, SourceType::File("input.txt".to_string()));
        assert_eq!(config.destination, SourceType::File("output.txt".to_string()));
    }

    #[test]
    fn test_build_bs_applies_to_both() {
        let args = ["program", "bs=4k"].map(str::to_string);
        let config = Config::build(&args).unwrap();
        assert_eq!(config.get_ibs(), 4 * 1024);
        assert_eq!(config.get_obs(), 4 * 1024);
    }

    #[test]
    fn test_build_ibs_overrides_bs() {
        let args = ["program", "bs=4k", "ibs=1k"].map(str::to_string);
        let config = Config::build(&args).unwrap();
        assert_eq!(config.get_ibs(), 1024);
        assert_eq!(config.get_obs(), 4 * 1024);
    }

    #[test]
    fn test_build_obs_overrides_bs() {
        let args = ["program", "bs=4k", "obs=2k"].map(str::to_string);
        let config = Config::build(&args).unwrap();
        assert_eq!(config.get_ibs(), 4 * 1024);
        assert_eq!(config.get_obs(), 2 * 1024);
    }

    #[test]
    fn test_build_ibs_and_obs_independent() {
        let args = ["program", "ibs=4k", "obs=2M"].map(str::to_string);
        let config = Config::build(&args).unwrap();
        assert_eq!(config.get_ibs(), 4 * 1024);
        assert_eq!(config.get_obs(), 2 * 1024 * 1024);
    }

    #[test]
    fn test_build_count_skip_seek() {
        let args = ["program", "count=10", "skip=5", "seek=3"].map(str::to_string);
        let config = Config::build(&args).unwrap();
        assert_eq!(config.get_count(), Some(10));
        assert_eq!(config.get_skip(), Some(5));
        assert_eq!(config.get_seek(), Some(3));
    }

    #[test]
    fn test_build_conv_lcase() {
        let args = ["program", "conv=lcase"].map(str::to_string);
        let config = Config::build(&args).unwrap();
        assert_eq!(config.data_convs, DataOps::ToLower as u8);
        assert_eq!(config.write_convs, 0);
    }

    #[test]
    fn test_build_conv_ucase() {
        let args = ["program", "conv=ucase"].map(str::to_string);
        let config = Config::build(&args).unwrap();
        assert_eq!(config.data_convs, DataOps::ToUpper as u8);
    }

    #[test]
    fn test_build_conv_notrunc_disables_truncate() {
        let args = ["program", "conv=notrunc"].map(str::to_string);
        let config = Config::build(&args).unwrap();
        assert!(!config.is_truncate());
    }

    #[test]
    fn test_build_conv_sync_sparse_noerror() {
        let args = ["program", "conv=sync,sparse,noerror"].map(str::to_string);
        let config = Config::build(&args).unwrap();
        assert!(config.is_sync());
        assert!(config.is_sparse());
        assert!(config.is_noerror());
    }

    #[test]
    fn test_build_same_if_then_of_error() {
        let args = ["program", "if=same.txt", "of=same.txt"].map(str::to_string);
        assert!(Config::build(&args).is_err());
    }

    #[test]
    fn test_build_same_of_then_if_error() {
        let args = ["program", "of=same.txt", "if=same.txt"].map(str::to_string);
        assert!(Config::build(&args).is_err());
    }

    #[test]
    fn test_build_unknown_argument_error() {
        let args = ["program", "unknown=value"].map(str::to_string);
        assert!(Config::build(&args).is_err());
    }

    #[test]
    fn test_build_invalid_block_size_error() {
        let args = ["program", "bs=0k"].map(str::to_string);
        assert!(Config::build(&args).is_err());
    }

    #[test]
    fn test_build_invalid_conv_combination_error() {
        let args = ["program", "conv=lcase,ucase"].map(str::to_string);
        assert!(Config::build(&args).is_err());
    }

    #[test]
    fn test_build_invalid_count_value_error() {
        let args = ["program", "count=abc"].map(str::to_string);
        assert!(Config::build(&args).is_err());
    }

    #[test]
    fn test_build_invalid_skip_value_error() {
        // negative numbers are not valid for usize
        let args = ["program", "skip=-1"].map(str::to_string);
        assert!(Config::build(&args).is_err());
    }

    #[test]
    fn test_build_invalid_seek_value_error() {
        let args = ["program", "seek=xyz"].map(str::to_string);
        assert!(Config::build(&args).is_err());
    }

    // ─── write flags defaults ─────────────────────────────────────────────────

    #[test]
    fn test_is_truncate_default_true() {
        assert!(Config::new().is_truncate());
    }

    #[test]
    fn test_is_sync_default_false() {
        assert!(!Config::new().is_sync());
    }

    #[test]
    fn test_is_sparse_default_false() {
        assert!(!Config::new().is_sparse());
    }

    #[test]
    fn test_is_noerror_default_false() {
        assert!(!Config::new().is_noerror());
    }
}