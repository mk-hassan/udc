use std::fmt::Display;
use std::time::{ Duration, Instant };

#[derive(Debug)]
pub struct Metrics {
	pub total_bytes: usize,
    pub read_blocks: usize,
    pub read_partials: usize,
    pub write_blocks: usize,
    pub write_partials: usize,
	start_timestamp: Instant,
}	

impl Metrics {
	pub fn new() -> Self {
		Metrics {
			total_bytes: 0,
			read_blocks: 0,
			read_partials: 0,
			write_blocks: 0,
			write_partials: 0,
			start_timestamp: Instant::now()
		}
	}

	// return the time duration of the pipeline execution
	fn time_duration(&self) -> Duration {
		self.start_timestamp.elapsed()
	}

	pub fn input_output_stats(&self) -> String {
		format!(
			"{}+{} records in,\n{}+{} records out",
			self.read_blocks, self.read_partials, self.write_blocks, self.write_partials
		)
	}

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