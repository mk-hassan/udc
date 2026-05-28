use super::SourceType;
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
    seek: Option<usize>
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
            seek: None
        }
    }

    pub fn build(args: &[String]) -> Result<Self, String> {
        let mut config = Config::new();

        for arg in args[1..].iter() {
            let (key, value) = Self::parse_and_validate_argument(arg)?;
            match key {
                constants::INPUT_FILE_ARG => config.source(SourceType::File(value.to_string())),
                constants::OUTPUT_FILE_ARG => config.destination(SourceType::File(value.to_string())),
                constants::BLOCK_SIZE_ARG => config.block_size(Self::parse_and_validate_block_size(value)?),
                constants::INPUT_BLOCK_SIZE_ARG => config.input_block_size(Self::parse_and_validate_block_size(value)?),
                constants::OUTPUT_BLOCK_SIZE_ARG => config.output_block_size(Self::parse_and_validate_block_size(value)?),
                constants::COUNT_ARG => config.count(Self::parse_count(value)?),
                constants::SKIP_ARG => config.skip(Self::parse_count(value)?),
                constants::SEEK_ARG => config.seek(Self::parse_count(value)?),
                _ => return Err(format!("ccdd: Unknown argument {}", key))
            }
        }

        Ok(config)
    }

    fn parse_and_validate_argument(arg: &str) -> Result<(&str, &str), String> {
        let splits: Vec<&str> = arg.splitn(2, |chr| chr == '=').collect();
        if splits.len() == 1 {
            return Err("ccdd: invalid argument format, expected key=value".to_string());
        }

        if splits.len() == 2 && (splits[0].is_empty() || splits[1].is_empty()) {
            return Err("ccdd: invalid argument format, key and value cannot be empty".to_string());
        }

        Ok((splits[0], splits[1]))
    }

    fn parse_and_validate_block_size(size: &str) -> Result<usize, String> {        
        let multiplier = &size[size.len() - 1..];
        if !(multiplier >= "a" && multiplier <= "z" || multiplier >= "A" && multiplier <= "Z") {
            return Ok(Config::parse_count(size)?);
        }

        let base_value = Config::parse_count(&size[..size.len() - 1])?;
        let multiplier_value = match multiplier {
            "c" | "b" => 1,
            "w" => 8,
            "k" | "K" => 1024,
            "M" => 1024 * 1024,
            "G" => 1024 * 1024 * 1024,
            _ => return Err(format!("ccdd: invalid block size multiplier {}", multiplier))
        };
        
        let final_value = base_value
            .checked_mul(multiplier_value)
            .ok_or_else(|| format!("ccdd: block size too large: {}", size))?;

        if final_value >= constants::MAX_BLOCK_SIZE || final_value == 0 {
            return Err(format!("ccdd: invalid block size: {}", size));
        }

        Ok(final_value)
    }

    fn parse_count(value: &str) -> Result<usize, String> {
        value.parse::<usize>().map_err(|_| format!("ccdd: invalid argument value: {}", value))
    }

    pub fn source(&mut self, source: SourceType) {
        self.source = source;
    }

    pub fn destination(&mut self, destination: SourceType) {
        self.destination = destination;
    }

    pub fn block_size(&mut self, block_size: usize) {
        self.block_size = Some(block_size);
    }

    pub fn input_block_size(&mut self, input_block_size: usize) {
        self.input_block_size = Some(input_block_size);
    }

    pub fn output_block_size(&mut self, output_block_size: usize) {
        self.output_block_size = Some(output_block_size);
    }

    pub fn count(&mut self, count: usize) {
        self.count = Some(count);
    }

    pub fn skip(&mut self, skip: usize) {
        self.skip = Some(skip);
    }

    pub fn seek(&mut self, seek: usize) {
        self.seek = Some(seek);
    }

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
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_arg_valid() {
        let arg = "if=input.txt";
        let result = Config::parse_and_validate_argument(arg).unwrap();
        assert_eq!(result, ("if", "input.txt"));
    }

    #[test]
    fn test_parse_arg_invalid_format() {
        // just key, no value
        let arg = "ifinput.txt";
        let result = Config::parse_and_validate_argument(arg);
        assert!(result.is_err());    

        // just value, no key
        let arg = "if=";
        let result = Config::parse_and_validate_argument(arg);
        assert!(result.is_err());

        // no key, no value
        let arg = "=";
        let result = Config::parse_and_validate_argument(arg);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_and_validate_block_size_valid() {
        let size = "4k";
        let result = Config::parse_and_validate_block_size(size).unwrap();
        assert_eq!(result, 4 * 1024);

        let size = "2M";
        let result = Config::parse_and_validate_block_size(size).unwrap();
        assert_eq!(result, 2 * 1024 * 1024);

        let size = "512";
        let result = Config::parse_and_validate_block_size(size).unwrap();
        assert_eq!(result, 512);

        let size = "1c";
        let result = Config::parse_and_validate_block_size(size).unwrap();
        assert_eq!(result, 1);
    }

    #[test]
    fn test_parse_and_validate_block_size_invalid_multiplier() {
        let size = "4x";
        let result = Config::parse_and_validate_block_size(size);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_and_validate_block_size_invalid_number() {
        let size = "xk";
        let result = Config::parse_and_validate_block_size(size);
        assert!(result.is_err());

        let size = "k";
        let result = Config::parse_and_validate_block_size(size);
        assert!(result.is_err());

        let size = "0k";
        let result = Config::parse_and_validate_block_size(size);
        assert!(result.is_err());

        let size = &format!("{}k", constants::MAX_BLOCK_SIZE + 1);
        let result = Config::parse_and_validate_block_size(size);
        assert!(result.is_err());
    }

    #[test]
    fn test_build_config_valid() {
        let args = vec![
            "program".to_string(),
            "if=input.txt".to_string(),
            "of=output.txt".to_string(),
            "ibs=4k".to_string(),
            "obs=2M".to_string()
        ];

        let config = Config::build(&args).unwrap();
        assert_eq!(config.source, SourceType::File("input.txt".to_string()));
        assert_eq!(config.destination, SourceType::File("output.txt".to_string()));
        assert_eq!(config.get_ibs(), 4 * 1024);
        assert_eq!(config.get_obs(), 2 * 1024 * 1024);
    }
}