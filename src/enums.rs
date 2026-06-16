//! udc enums used for configuration and pipeline operations

#[derive(Debug, PartialEq, Eq, Default)]
pub enum SourceType {
  File(String),
  #[default]
  Standard
}

#[derive(Debug, PartialEq, Eq, Default)]
pub enum PrintStatus {
    None,
    Noxfer,
    Progress,
    #[default]
    Default
}

pub enum DataOps {
    ToLower = 1,
    ToUpper = 2,
    Swap = 4,
}

pub enum FileOps {
    NoTrunc = 1,
    Sync = 2,
    Sparse = 4,
    NoError = 8
}

pub enum InputFlags {
    Direct = 1,
    Nonblock = 2,
    Nocache = 4,
    FullBlock = 8,
    CountBytes = 16,
    SkipBytes = 32
}

pub enum OutputFlags {
    Direct = 1,
    Sync = 2,
    Dsync = 4,
    Excl = 8,
    Nonblock = 16,
    Nocache = 32,
    SeekBytes = 64
}
