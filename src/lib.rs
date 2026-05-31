mod constants;
pub mod config;
pub mod pipeline;

pub use pipeline::metrics::Metrics;

#[derive(Debug, PartialEq, Eq)]
pub enum SourceType {
  File(String),
  Standard
}