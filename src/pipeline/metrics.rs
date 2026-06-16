//! Metrics tracking for the data pipeline
//!
//! This module collects runtime statistics for bytes copied and record throughput.
//! It is designed to support reporting, diagnostics, and performance summaries for
//! pipeline execution

use std::fmt::Display;
use std::time::{ Duration, Instant };

/// Aggregated metrics for pipeline execution
///
/// The `Metrics` struct stores counters for bytes processed, input/output blocks,
/// and partial block counts. It also records a start timestamp so that elapsed
/// runtime and throughput can be reported
#[derive(Debug)]
pub struct Metrics {
	/// Total number of bytes copied by the pipeline
	pub total_bytes: usize,
	/// Full blocks read from the input
	pub read_blocks: usize,
	/// Partial blocks read from the input
	pub read_partials: usize,
	/// Full blocks written to the output
	pub write_blocks: usize,
	/// Partial blocks written to the output
	pub write_partials: usize,
	start_timestamp: Instant,
}

impl Default for Metrics {
	fn default() -> Self {
		Self {
			total_bytes: 0,
			read_blocks: 0,
			read_partials: 0,
			write_blocks: 0,
			write_partials: 0,
			start_timestamp: Instant::now()
		}
	}
}

impl Metrics {
	/// Returns the elapsed duration since metrics collection began.
	fn time_duration(&self) -> Duration {
		self.start_timestamp.elapsed()
	}

	/// Formats a summary of input and output record counts.
	///
	/// This includes both full and partial record blocks.
	pub fn input_output_stats(&self) -> String {
		format!(
			"{}+{} records in,\n{}+{} records out",
			self.read_blocks, self.read_partials, self.write_blocks, self.write_partials
		)
	}

	/// Formats a throughput summary for the pipeline.
	///
	/// The returned string includes total bytes copied, elapsed runtime, and
	/// average megabytes per second.
	pub fn transer_stats(&self) -> String {
		let secs = self.time_duration().as_secs_f64();
		format!(
			"{} bytes copied, {:.6} s, {:.2} MB/s",
			self.total_bytes, secs, (self.total_bytes as f64) / (1024.0 * 1024.0) / secs
		)
	}
}

impl Display for Metrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}\n{}", self.input_output_stats(), self.transer_stats())
    }
}