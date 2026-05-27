mod constants;
pub mod config;
pub mod pipeline;

#[derive(Debug, PartialEq, Eq)]
pub enum SourceType {
  File(String),
  Standard
}