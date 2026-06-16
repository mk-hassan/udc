//! # Unified Data Clone (UDC)
//!
//! `udc` is a cross-platform, high-performance, low-level block I/O data streaming utility designed
//! to emulate the core behavior of the standard POSIX `dd` command. It supports custom
//! input/output block sizing, target byte stream skipping/seeking, and in-flight
//! data block transformations (such as uppercase, lowercase, and byte swapping).
//!
//! ## Cross-Platform Portability vs. Standard POSIX `dd`
//!
//! While the traditional POSIX `dd` utility is inherently tied to Unix-like ecosystems—relying
//! strictly on POSIX-compliant system calls and file descriptor structures—`udc` is engineered
//! from the ground up for native, cross-platform execution across **Windows, Linux, and macOS**.
//!
//! Rather than forcing Windows systems to rely on translation layers or heavy emulation environments
//! (such as Cygwin, MSYS2, or WSL), `udc` leverages Rust's conditional compilation abstractions
//! to interface directly with platform-specific kernel subsystems:
//!
//! * **Windows Integration**: Employs native Win32 storage options, transforming `iflag=direct`
//!   and `oflag=direct` into `FILE_FLAG_NO_BUFFERING` flags via low-level Win32 file creation bindings,
//!   and implements Windows volume geometry queries (`GetVolumePathNameW` / `DeviceIoControl`)
//!   to resolve physical sector bounds.
//! * **Linux & Unix Integration**: Interacts cleanly with standard `libc` flags, utilizing `O_DIRECT`
//!   and `O_NONBLOCK` system optimizations to bypass the Linux virtual filesystem page cache.
//! * **macOS Integration**: Provides specialized tuning to manage Darwin's unique caching boundaries,
//!   safely optimizing raw disk descriptor access paths (e.g., `/dev/rdisk*`) while ensuring strict
//!   cache consistency.
//!
//! This cross-platform architecture allows engineers to use identical automation scripts and data
//! transformation syntax seamlessly across disparate development, staging, and deployment operating systems.
//!
//! ## Architecture Overview
//!
//! The execution lifecycle is driven by a synchronous pipeline model dividing concerns
//! into distinct, isolated operational layers:
//!
//! ```text
//!                             ┌────────────────┐
//!                             │   Config/CLI   │
//!                             └───────┬────────┘
//!                                     │ (Provides rules)
//!                                     ▼                                  
//! ┌────────┐              ┌───────────────────────┐                ┌────────┐
//! │ Reader │ ── ibs ────► │       Pipeline        │ ──── obs ────► │ Writer │
//! └────────┘              │ - Aligned Staging     │                └────────┘
//!                         │ - In-place Transform  │
//!                         │ - Managing I/O Flow   │
//!                         └───────────────────────┘
//!                                     │ (Collects metrics)
//!                                     ▼
//!                            ┌──────────────────┐
//!                            │ Metrics Tracking │
//!                            └──────────────────┘
//! ```
//!
//! ## Module Organization
//!
//! * [`config`]: Command-line parameter parser, syntax validator, and operational context generator.
//! * [`reader`]: Stream abstraction unifying sequential access over persistent files and `stdin`.
//! * [`writer`]: Stream abstraction handling output persistence, including advanced block structures
//!   like sparse block files (`conv=sparse`).
//! * [`aligned_buffer`]: Heap-allocated memory allocations mapped to specific alignment bounds
//!   (e.g., 512 or 4096 bytes) to bypass kernel caching boundaries for Direct I/O (`O_DIRECT`).
//! * [`pipeline`]: Synchronous engine coordinating reads, mutations, and structural stream flushing.

mod constants;
mod enums;

pub mod aligned_buffer;
pub mod config;
pub mod pipeline;
pub mod reader;
pub mod writer;
