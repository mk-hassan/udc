//! Block-oriented data transfer pipeline.
//!
//! This module orchestrates the full lifecycle of a `dd`-style copy operation:
//! reading aligned input blocks, applying in-place data conversions, and writing
//! the result to the output target.
//!
//! ## Architecture
//!
//! ```text
//!  ┌────────┐  ibs bytes   ┌──────────────┐  obs bytes   ┌────────┐
//!  │ Reader │ ──────────►  │  Conversion  │ ──────────►  │ Writer │
//!  └────────┘              │   Pipeline   │              └────────┘
//!                          │ (lcase/ucase/│
//!                          │   swab/…)    │
//!                          └──────────────┘
//! ```
//!
//! The [`Pipeline`] struct holds ownership of both I/O handles and the two
//! staging buffers (read-buffer sized to `ibs`, write-buffer sized to `obs`).
//! Execution is driven by [`Pipeline::run`], which loops until EOF or the
//! optional `count` limit is reached.
//!
//! ## Error handling
//!
//! All recoverable I/O errors are represented by [`PipelineError`].  When
//! `conv=noerror` is active, read errors are swallowed and the erroneous block
//! is either skipped (default) or zero-padded (`conv=sync`).

use core::fmt;
use std::{
    io::{Read, Write},
    sync::atomic::{AtomicBool, Ordering},
};

mod conv;
pub mod metrics;
mod utils;

use super::{
    aligned_buffer::AlignedBuffer,
    config::Config,
    enums::{DataOps, PrintStatus, SourceType},
    reader::Reader,
    writer::Writer,
};

use metrics::Metrics;

#[derive(Debug)]
pub enum PipelineError {
    Mismatch(String, String),
    Hardware(String),
    IoError(std::io::Error),
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PipelineError::Mismatch(expected, found) => write!(
                f,
                "multiplicity mismatch: expected {} to be multiple of {}",
                expected, found
            ),
            PipelineError::Hardware(msg) => {
                write!(f, "udc: hardware: error reading sector size: {}", msg)
            }
            PipelineError::IoError(msg) => write!(f, "udc: io error: {}", msg),
        }
    }
}

impl From<std::io::Error> for PipelineError {
    fn from(value: std::io::Error) -> Self {
        PipelineError::IoError(value)
    }
}

impl std::error::Error for PipelineError {}

type Result<T> = std::result::Result<T, PipelineError>;

pub struct Pipeline {
    reader: Reader,
    read_buffer: AlignedBuffer,
    writer: Writer,
    write_buffer: AlignedBuffer,
    metrics: Metrics,
    config: Config,
    remaining_count: Option<usize>,
    eof_reached: bool,
    error_found: bool,
}

pub static PRINT_REQUEST: AtomicBool = AtomicBool::new(false);

impl Pipeline {
    /// Constructs and initialises a fully configured [`Pipeline`] from a [`Config`].
    ///
    /// Resolves Direct I/O alignment requirements, allocates the two staging
    /// buffers, and opens the reader/writer targets.  When `iflag=direct` or
    /// `oflag=direct` is set, sector-size alignment is validated here; any
    /// mismatch surfaces as [`PipelineError::Mismatch`].
    ///
    /// ## Errors
    ///
    /// Returns a boxed error for:
    /// - unsupported or inaccessible file paths
    /// - block-size / sector-size alignment mismatches (direct I/O)
    /// - any OS-level resource acquisition failure
    pub fn build(config: Config) -> std::result::Result<Pipeline, Box<dyn std::error::Error>> {
        let pipeline = Self {
            reader: Reader::build(&config)?,
            writer: Writer::build(&config)?,
            read_buffer: Self::get_buffer_reader(&config)?,
            write_buffer: Self::get_buffer_writer(&config)?,
            metrics: Metrics::default(),
            remaining_count: *config.get_count(),
            eof_reached: false,
            error_found: false,
            config,
        };

        Ok(pipeline)
    }

    fn get_buffer_reader(config: &Config) -> Result<AlignedBuffer> {
        let mut input_alignment = 1;
        if config.is_direct_input() {
            match config.get_source() {
                SourceType::Standard => eprintln!(
                    "udc: warning: direct access for the standard input is not supported for any platform"
                ),
                SourceType::File(path) => {
                    input_alignment =
                        utils::check_direct_access(path, config.get_ibs(), config.get_skip())?
                }
            }
        }

        Ok(AlignedBuffer::new(config.get_ibs(), input_alignment))
    }

    fn get_buffer_writer(config: &Config) -> Result<AlignedBuffer> {
        let mut output_alignment = 1;
        if config.is_direct_output() {
            match config.get_destination() {
                SourceType::Standard => eprintln!(
                    "udc: warning: direct access for the standard output is not supported for any platform"
                ),
                SourceType::File(path) => {
                    output_alignment =
                        utils::check_direct_access(path, config.get_obs(), config.get_seek())?
                }
            }
        }

        Ok(AlignedBuffer::new(config.get_obs(), output_alignment))
    }

    /// Executes the data-transfer loop until EOF or the `count` limit is reached.
    ///
    /// Each iteration:
    /// 1. Reads up to `ibs` bytes into the read buffer via [`Pipeline::handle_read`]
    /// 2. Updates read metrics: a full-capacity read increments `read_blocks`;
    ///    a shorter non-zero read increments `read_partials`
    /// 3. Optionally pads the block to `ibs` with NUL bytes (`conv=sync`)
    ///    Note: metrics are recorded *before* padding, so a padded block still
    ///    counts as a partial in `read_partials`
    /// 4. Copies the data into the write buffer in `obs`-sized chunks, applying
    ///    the conversion pipeline (`lcase` / `ucase` / `swab`) in place
    /// 5. Writes each write-buffer chunk; updates write metrics: an
    ///    `obs`-byte write increments `write_blocks`, anything else increments
    ///    `write_partials` (including the zero-byte EOF flush)
    /// 6. On EOF, flushes any remaining partial write-buffer bytes and returns
    ///
    /// ## Returns
    ///
    /// Returns `Ok(())` on success
    ///
    /// ## Errors
    ///
    /// Returns [`PipelineError`] if an unforgivable read error occurs or any
    /// write / seek operation fails.
    pub fn run(&mut self) -> Result<()> {
        loop {
            // read into the buffer
            self.handle_read()?;

            // sync handling
            if self.error_found && !self.config.is_sync() {
                continue;
            }
            if self.config.is_sync() && !self.eof_reached {
                self.read_buffer.fill_rest(0);
                self.read_buffer.set_length(self.read_buffer.get_capacity());
            }

            // write the buffer to the output
            self.handle_write()?;

            // handle print when requested
            if self.config.get_print_option() == &PrintStatus::Progress
                || PRINT_REQUEST.swap(false, Ordering::Relaxed)
            {
                println!("{}", self.metrics);
            }

            // EOF check to end the process
            if self.eof_reached {
                break;
            }
        }

        Ok(())
    }

    /// Reads from the configured source into the aligned read buffer.
    ///
    /// Reading continues until one of the following conditions is met:
    /// - the read buffer is full,
    /// - EOF is reached,
    /// - a read error occurs and is propagated,
    /// - the optional `count` limit is exhausted.
    ///
    /// This method updates pipeline state flags and metrics:
    /// - `self.eof_reached` is set on EOF,
    /// - `self.error_found` is set when `conv=noerror` tolerates an I/O error,
    /// - `self.metrics.read_blocks` or `self.metrics.read_partials` is incremented
    ///   based on whether the final read filled the buffer.
    ///
    /// ## Errors
    ///
    /// Returns `PipelineError::IoError` when a read fails and `conv=noerror`
    /// is disabled.
    fn handle_read(&mut self) -> Result<()> {
        let mut accumulated_read_count = 0usize;

        loop {
            // Slide the active window forward based on bytes already accumulated
            let current_window = &mut self.read_buffer.as_mut_slice()[accumulated_read_count..];

            let read_promise = self.reader.read(current_window);

            // Error Handling (conv=noerror logic)
            if let Err(err) = read_promise {
                if !self.config.is_noerror() {
                    return Err(PipelineError::IoError(err));
                }
                self.error_found = true;
                return Ok(());
            }

            let mut read_count = read_promise.unwrap();

            // Enforce limits from the count parameter if one was provided
            if let Some(remaining_count) = self.remaining_count {
                read_count = read_count.min(remaining_count);
                self.remaining_count = Some(remaining_count - read_count);
            }

            // Natural End-Of-File (EOF) detection
            if read_count == 0 {
                self.eof_reached = true;
                return Ok(());
            }

            accumulated_read_count += read_count;
            self.read_buffer.set_length(accumulated_read_count);
            if !self.config.is_fullblock() || self.read_buffer.is_full() {
                break;
            }
        }

        if self.read_buffer.is_full() {
            self.metrics.read_blocks += 1;
        } else if !self.read_buffer.is_empty() {
            self.metrics.read_partials += 1;
        }

        Ok(())
    }

    /// Copies bytes from the read buffer into the write buffer
    ///
    /// This method moves the current contents of `read_buffer` into `write_buffer`
    /// in repeated chunks until no readable bytes remain or the write buffer is full.
    /// It does not perform any data conversion; conversion is applied later during `write()`
    fn handle_write(&mut self) -> Result<()> {
        let mut read_start = 0usize;
        let mut read_count = self.read_buffer.get_length();
        while read_count > 0 {
            let copied_count =
                self.write_buffer
                    .copy_from(self.read_buffer.as_slice(), read_start, read_count);

            if self.write_buffer.is_full() {
                self.perform_write()?;
            }

            read_start += copied_count;
            read_count -= copied_count;
        }

        if self.eof_reached && !self.write_buffer.is_empty() {
            self.perform_write()?;
            self.writer.flush()?;
        }

        self.read_buffer.clear();
        Ok(())
    }

    /// Writes buffered output data to the destination writer
    ///
    /// The method applies the configured transformation pipeline to each output
    /// slice before writing it. When `swap` mode is enabled and EOF has not yet
    /// been reached, an odd-length buffer is truncated by one byte so the
    /// subsequent swap operation remains aligned
    ///
    /// Metrics are updated for every write attempt:
    /// - `write_blocks` is incremented for full-capacity writes
    /// - `write_partials` is incremented for shorter writes
    ///
    /// After writing, the consumed portion of `write_buffer` is drained.
    ///
    /// ## Errors
    ///
    /// Returns `PipelineError::IoError` if the underlying writer fails.
    fn perform_write(&mut self) -> Result<()> {
        let mut write_start = 0usize;
        let mut write_count = self.write_buffer.get_length();

        if !self.eof_reached && self.config.is_swap() && write_count % 2 == 1 {
            write_count -= 1;
        }

        conv::convert(
            &mut self.write_buffer.as_mut_slice()[..write_count],
            self.config.get_data_convs(),
        );
        while write_count > 0 {
            let written_bytes = self
                .writer
                .write(&self.write_buffer.as_mut_slice()[write_start..write_start + write_count])?;

            if written_bytes == self.write_buffer.get_capacity() {
                self.metrics.write_blocks += 1;
            } else {
                self.metrics.write_partials += 1;
            }

            write_start += written_bytes;
            write_count -= written_bytes;
        }

        self.metrics.total_bytes += write_start;
        self.write_buffer.drain(write_start);
        Ok(())
    }

    /// Prints the current metrics to stdout based on the provided `PrintStatus`
    pub fn print_metrics(&self) {
        match self.config.get_print_option() {
            PrintStatus::None => (),
            PrintStatus::Noxfer => println!("{}", self.metrics.input_output_stats()),
            _ => println!("{}", self.metrics),
        }
    }

    // Returns a reference to the current metrics for external inspection or testing
    pub fn get_metrics(&self) -> &Metrics {
        &self.metrics
    }
}

#[cfg(test)]
mod tests;
