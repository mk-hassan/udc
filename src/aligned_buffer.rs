//! # Memory-Aligned Staging Buffers for Direct Hardware I/O
//!
//! This module provides [`AlignedBuffer`], a low-level memory layout container designed 
//! for zero-copy operations, block-device streaming, and direct hardware interaction.
//!
//! ## The Engineering Necessity of Alignment
//!
//! High-performance operating systems and filesystem storage nodes allow programmers to request direct 
//! storage streaming (such as Linux's `O_DIRECT` flag or Windows' `FILE_FLAG_NO_BUFFERING` attribute). 
//! These configurations entirely bypass the operating system's kernel page cache layers. This eliminates 
//! intermediate memory copying cycles and lowers overhead during bulk transfers.
//!
//! However, bypassing kernel buffer layers shifts architectural constraints directly onto the application space:
//! 1. **Address Realignment Bounds**: The target memory segment allocated on the heap must begin at a virtual 
//!    address addressable by clean multiples of the physical hardware sector size (typically 512 or 4096 bytes).
//! 2. **Block Transfer Uniformity**: Data transaction sizes requested during core kernel system calls 
//!    must perfectly align with these underlying storage track and sector bounds.
//!
//! Standard allocation types (like `Vec<u8>`) do not guarantee specific memory alignment thresholds upon allocation. 
//! [`AlignedBuffer`] fulfills these system conditions by interacting directly with the core system memory 
//! layout allocator ([`std::alloc`]) to guarantee strict alignment parameters throughout the buffer's lifecycle.
//!
//! ## Architectural Layout and Behavior
//!
//! ```text
//!    Requested Capacity (e.g., 1000 bytes) + Alignment Target (512 bytes)
//!                       │
//!                       ▼ (Automated div_ceil alignment rounding)
//!    ┌────────────────────────────────────────────────────────┐
//!    │ Total Aligned Capacity Allocation (1024 bytes)         │
//!    ├───────────────────────────┬────────────────────────────┤
//!    │ Active Occupied Slice     │ Unused / Uninitialized     │
//!    │ (0 .. length tracking)    │ (length .. capacity)       │
//!    └───────────────────────────┴────────────────────────────┘
//!    ▲                                                        
//!    └─ Buffer Pointer Address is uniquely aligned (e.g., address % 512 == 0)
//! ```
//!
//! * **Capacity Ceiling Optimization**: When initializing an execution instance, the buffer scales 
//!   the capacity upwards to match the nearest clean multiple of your hardware sector alignment requirements.
//! * **Dual-Sided Vector Isolation**: The structure separates the underlying raw memory allocation capacity 
//!   ([`AlignedBuffer::as_mut_slice`]), which exposes raw memory zones to file systems, from processed 
//!   active boundaries ([`AlignedBuffer::get_occupied_slice`]), which tracks the exact subset of bytes loaded.
//! * **Left-Shift Contiguous Draining**: The module exposes optimized un-overlapped shifting methods 
//!   ([`AlignedBuffer::drain`]) to move lingering byte segments forward when aligning split-block multi-stage pipeline writes.
//!
//! ## System Safety & Core Invariants
//!
//! As a low-level container interacting directly with system allocation layout APIs, `AlignedBuffer` guarantees 
//! strict memory safety guidelines:
//! * **Explicit Unallocation Coordination**: The implementation implements the [`Drop`] trait. This handles 
//!   reclaiming raw layout contexts and prevents kernel-level leakage by tracking the original allocation layout structure.
//! * **Encapsulated Raw Pointer Interfaces**: All unsafe pointer manipulation and byte-shifting functions are isolated 
//!   within the struct's internal engine. All public method boundaries receive and return standard, type-safe Rust 
//!   slice references (`&[u8]` and `&mut [u8]`).

use std::alloc::{alloc, dealloc, Layout};
use std::ptr::NonNull;

/// A fixed-capacity, heap-allocated byte buffer aligned to a specific memory boundary.
///
/// `AlignedBuffer` is designed for low-level systems I/O operations (like Direct I/O)
/// where data must be read into or written from memory addresses aligned to hardware
/// sector boundaries (e.g., 512 or 4096 bytes).
///
/// It acts similarly to a `Vec<u8>`, but guarantees that its underlying pointer matches
/// the requested alignment constraints throughout its lifecycle.
///
/// ## Examples
///
/// ```
/// use udc::aligned_buffer::AlignedBuffer;
///
/// let buffer = AlignedBuffer::new(1000, 512);
/// assert_eq!(buffer.get_length(), 0);
/// ```
pub struct AlignedBuffer {
    ptr: NonNull<u8>,
    layout: Layout,
    capacity: usize,
    length: usize,
}

impl AlignedBuffer {
    /// Creates a new `AlignedBuffer` with a minimum capacity and strict alignment constraint.
    ///
    /// The actual allocated capacity will be automatically rounded up to the nearest
    /// multiple of the requested alignment to guarantee safety during direct block I/O.
    ///
    /// ## Panics
    ///
    /// Panics if the alignment is not a power of two, if the capacity calculation overflows,
    /// or if the system runs out of memory during allocation.
    ///
    /// ## Examples
    ///
    /// ```
    /// use udc::aligned_buffer::AlignedBuffer;
    ///
    /// let buffer = AlignedBuffer::new(1000, 512);
    /// assert_eq!(buffer.get_length(), 0);
    /// ```
    pub fn new(capacity: usize, alignment: usize) -> Self {
        // Ensure capacity is a multiple of alignment for safety in direct I/O
        let aligned_capacity = capacity.div_ceil(alignment) * alignment;
        let layout = Layout::from_size_align(aligned_capacity, alignment)
            .expect("Invalid memory layout requested");

        let ptr = unsafe { alloc(layout) };
        let ptr = NonNull::new(ptr).expect("Failed to allocate aligned memory. System out of RAM?");

        Self {
            ptr,
            layout,
            capacity: aligned_capacity,
            length: 0,
        }
    }

    /// Returns a mutable slice covering the entire allocated capacity of the buffer.
    ///
    /// This is typically used to pass the buffer to system calls or file streams
    /// where data will be loaded into the buffer directly.
    ///
    /// ## Safety
    ///
    /// The caller must ensure that any uninitialized bytes in the returned slice are 
    /// handled safely, as this returns the raw uninitialized heap capacity.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.capacity) }
    }

    /// Returns a shared slice covering the entire allocated capacity of the buffer.
    pub fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.capacity) }
    }

    /// Returns a mutable slice covering only the portion of the buffer 
    /// that has been marked as "occupied" by the internal length tracking.
    /// 
    /// This is useful for accessing only the valid data that has been read into the buffer,
    /// while ignoring any uninitialized or unused capacity.
    /// 
    /// ## Safety
    /// 
    /// The caller must ensure that the internal length is correctly updated to reflect the valid data,
    /// as this method will return a slice that may include uninitialized bytes if the length is not set properly.
    ///
    /// ## Examples
    ///
    /// ```
    /// use udc::aligned_buffer::AlignedBuffer;
    ///
    /// let mut buffer = AlignedBuffer::new(10, 1);
    /// buffer.copy_from(&[1, 2, 3], 0, 3);
    /// 
    /// let occupied = buffer.get_occupied_slice();
    /// assert_eq!(occupied, &[1, 2, 3]);
    /// ```
    pub fn get_occupied_slice(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.length) }
    }

    /// Updates the internal length tracking of the buffer.
    ///
    /// This should be called manually after filling the buffer via external methods 
    /// like `as_mut_slice()`.
    ///
    /// ## Panics
    ///
    /// Panics if the provided `len` is greater than the buffer's allocated capacity.
    ///
    /// ## Examples
    ///
    /// ```
    /// use udc::aligned_buffer::AlignedBuffer;
    ///
    /// let mut buffer = AlignedBuffer::new(512, 512);
    /// buffer.set_length(100);
    /// assert_eq!(buffer.get_length(), 100);
    /// ```
    pub fn set_length(&mut self, len: usize) {
        assert!(len <= self.capacity, "Length cannot exceed total capacity");
        self.length = len;
    }

    /// Copies bytes from a source slice into this buffer, starting at the current internal length.
    ///
    /// If the buffer does not have enough remaining space to fit the entire requested `count`,
    /// it will copy as many bytes as possible until it reaches capacity.
    ///
    /// Returns the number of bytes successfully copied.
    ///
    /// ## Panics
    ///
    /// Panics if `start + count` exceeds the boundaries of the `src` slice.
    ///
    /// ## Examples
    ///
    /// ```
    /// use udc::aligned_buffer::AlignedBuffer;
    ///
    /// let mut buffer = AlignedBuffer::new(10, 1);
    /// let data = vec![1, 2, 3, 4, 5];
    /// 
    /// let copied = buffer.copy_from(&data, 0, 3);
    /// assert_eq!(copied, 3);
    /// assert_eq!(&buffer.as_slice()[..buffer.get_length()], &[1, 2, 3]);
    /// ```
    #[must_use]
    pub fn copy_from(&mut self, src: &[u8], start: usize, count: usize) -> usize {
        // Validation check to prevent Out-of-Bounds memory reads from `src`
        assert!(
            start + count <= src.len(),
            "Source slice bounds exceeded: start {} + count {} must be <= len {}",
            start, count, src.len()
        );

        if self.length == self.capacity {
            return 0;
        }
        
        let count = count.min(self.capacity - self.length);
        unsafe {
            std::ptr::copy_nonoverlapping(
                src.as_ptr().add(start),
                self.ptr.as_ptr().add(self.length),
                count,
            );
        }
        self.length += count;
        count
    }

    /// Resets the internal length of the buffer back to zero.
    ///
    /// Note: This does not clear or zero-out the underlying memory; it simply
    /// resets the tracking pointer, making the capacity available for reuse.
    ///
    /// ## Examples
    ///
    /// ```
    /// use udc::aligned_buffer::AlignedBuffer;
    ///
    /// let mut buffer = AlignedBuffer::new(10, 1);
    /// buffer.copy_from(&[1, 2, 3], 0, 3);
    /// assert_eq!(buffer.get_length(), 3);
    /// 
    /// buffer.clear();
    /// assert_eq!(buffer.get_length(), 0);
    /// ```
    pub fn clear(&mut self) {
        self.length = 0;
    }

    /// Drains a specified number of bytes from the start of the occupied portion of the buffer.
    ///
    /// Shifts all subsequent occupied bytes to the left to fill the gap, preserving the remaining data order,
    /// and decreases the tracking length accordingly.
    ///
    /// ## Panics
    ///
    /// Panics if `count` is greater than the current occupied length of the buffer.
    ///
    /// ## Examples
    ///
    /// ```
    /// use udc::aligned_buffer::AlignedBuffer;
    ///
    /// let mut buffer = AlignedBuffer::new(10, 1);
    /// buffer.copy_from(&[1, 2, 3, 4, 5], 0, 5);
    /// 
    /// buffer.drain(2);
    /// assert_eq!(buffer.get_occupied_slice(), &[3, 4, 5]);
    /// assert_eq!(buffer.get_length(), 3);
    /// ```
    pub fn drain(&mut self, count: usize) {
        assert!(
            count <= self.length,
            "Cannot drain more bytes than currently occupied"
        );
        
        unsafe {
            std::ptr::copy(
                self.ptr.as_ptr().add(count),
                self.ptr.as_ptr(),
                self.length - count,
            );
        }
        self.length -= count;
    }

    /// Fills the remaining unallocated capacity of the buffer with the specified byte value.
    /// 
    /// This is particularly useful in sync mode (`conv=sync`) to ensure that the entire buffer 
    /// is padded with trailing characters (like NUL or spaces) up to the target block size boundary.
    /// 
    /// ## Behavior
    ///
    /// Sets the internal length to the full capacity after filling, as the buffer is now considered fully occupied.
    ///
    /// ## Examples
    ///
    /// ```
    /// use udc::aligned_buffer::AlignedBuffer;
    ///
    /// let mut buffer = AlignedBuffer::new(5, 1);
    /// buffer.copy_from(&[1, 2], 0, 2);
    /// 
    /// buffer.fill_rest(0);
    /// assert_eq!(buffer.get_occupied_slice(), &[1, 2, 0, 0, 0]);
    /// assert_eq!(buffer.is_full(), true);
    /// ```
    pub fn fill_rest(&mut self, value: u8) {
        let current_length = self.length;
        self.as_mut_slice()[current_length..].fill(value);
        self.set_length(self.capacity);
    }

    /// Checks if the buffer is currently full (i.e., length equals capacity).
    /// 
    /// This can be used to determine if the buffer is ready for a write operation or if it needs to be cleared first.
    ///
    /// ## Examples
    ///
    /// ```
    /// use udc::aligned_buffer::AlignedBuffer;
    ///
    /// let mut buffer = AlignedBuffer::new(2, 1);
    /// assert_eq!(buffer.is_full(), false);
    /// 
    /// buffer.copy_from(&[1, 2], 0, 2);
    /// assert_eq!(buffer.is_full(), true);
    /// ```
    pub fn is_full(&self) -> bool {
        self.length == self.capacity
    }

    /// Returns the total length of valid data currently stored in the buffer.
    ///
    /// ## Examples
    ///
    /// ```
    /// use udc::aligned_buffer::AlignedBuffer;
    ///
    /// let mut buffer = AlignedBuffer::new(10, 1);
    /// assert_eq!(buffer.get_length(), 0);
    /// 
    /// buffer.copy_from(&[1, 2, 3], 0, 3);
    /// assert_eq!(buffer.get_length(), 3);
    /// ```
    pub fn get_length(&self) -> usize {
        self.length
    }

    /// Returns the total capacity of the buffer, which is the maximum number of bytes it can hold.
    ///
    /// ## Examples
    ///
    /// ```
    /// use udc::aligned_buffer::AlignedBuffer;
    ///
    /// let buffer = AlignedBuffer::new(1000, 512);
    /// assert_eq!(buffer.get_capacity(), 1024); // Rounded up to nearest multiple of 512
    /// ```
    pub fn get_capacity(&self) -> usize {
        self.capacity
    }

    /// Checks if the buffer is currently empty (i.e., length is zero).
    ///
    /// ## Examples
    ///
    /// ```
    /// use udc::aligned_buffer::AlignedBuffer;
    ///
    /// let mut buffer = AlignedBuffer::new(10, 1);
    /// assert_eq!(buffer.is_empty(), true);
    /// 
    /// buffer.copy_from(&[1], 0, 1);
    /// assert_eq!(buffer.is_empty(), false);
    /// ```
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }
}

impl Drop for AlignedBuffer {
    /// Safely deallocates the aligned memory back to the system allocator.
    fn drop(&mut self) {
        unsafe {
            dealloc(self.ptr.as_ptr(), self.layout);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_aligned_buffer() {
        // Arrange
        let alignment = 512;
        let capacity = 1024;

        // Act
        let buffer = AlignedBuffer::new(capacity, alignment);
        
        // Assert
        assert_eq!(buffer.get_length(), 0);
        assert_eq!(buffer.capacity, capacity);
        assert_eq!(buffer.layout.align(), alignment);
    }

    #[test]
    fn test_copy_to_io_buffer() {
        // Arrange
        let source = vec![1, 2, 3, 4, 5];
        let alignment = 16;
        let capacity = 8;
        let mut direct_buffer = AlignedBuffer::new(capacity, alignment);
        
        // Act
        let consumed_bytes = direct_buffer.copy_from(&source, 0, source.len());

        // Assert
        assert_eq!(&direct_buffer.as_slice()[..consumed_bytes], &[1, 2, 3, 4, 5]);
        assert_eq!(consumed_bytes, source.len());
    }

    #[test]
    fn test_copy_to_aligned_buffer_with_insufficient_capacity() {
        // Arrange
        let source = (1..255).collect::<Vec<u8>>();
        let alignment = 1;
        let capacity = 16;
        let mut buffered_buffer = AlignedBuffer::new(capacity, alignment);
        
        // Act
        let consumed_bytes = buffered_buffer.copy_from(&source, 0, source.len());

        // Assert
        assert_eq!(consumed_bytes, capacity);
        assert_eq!(buffered_buffer.get_length(), capacity);
    }

    #[test]
    fn test_copy_to_aligned_buffer_with_sufficient_capacity() {
        // Arrange
        let source = (1..128).collect::<Vec<u8>>();
        let alignment = 16;
        let capacity = 256;
        let mut direct_buffer = AlignedBuffer::new(capacity, alignment);

        // Act
        let consumed_bytes = direct_buffer.copy_from(&source, 0, source.len());

        // Assert
        assert_eq!(&direct_buffer.as_slice()[..consumed_bytes], &source[..consumed_bytes]);
        assert_eq!(consumed_bytes, source.len());
        assert_eq!(direct_buffer.get_length(), source.len());
    }

    #[test]
    fn test_copy_to_direct_io_buffer_with_insufficient_capacity_till_end() {
        // Arrange
        let source = (1..131).collect::<Vec<u8>>();
        let alignment = 16;
        let capacity = 64;
        let mut direct_buffer = AlignedBuffer::new(capacity, alignment);

        // Act
        let mut start = 0;
        for _ in 0..2 {
            let consumed_bytes = direct_buffer.copy_from(&source, start, source.len() - start);
            direct_buffer.clear(); 
            start += consumed_bytes;
        }
        let consumed_bytes = direct_buffer.copy_from(&source, start, source.len() - start);

        // Assert
        assert_eq!(&direct_buffer.as_slice()[..consumed_bytes], &[129, 130]);
        assert_eq!(consumed_bytes, 2);
        assert_eq!(direct_buffer.get_length(), 2);
    }

    #[test]
    #[should_panic(expected = "Source slice bounds exceeded")]
    fn test_copy_from_out_of_bounds_panics() {
        // Arrange
        let source = vec![1, 2, 3];
        let mut buffer = AlignedBuffer::new(10, 1);
        
        // Act & Assert
        let _ = buffer.copy_from(&source, 1, 5); 
    }

    #[test]
    fn test_get_occupied_slice_and_set_length() {
        // Arrange
        let mut buffer = AlignedBuffer::new(10, 1);
        buffer.as_mut_slice()[0..4].copy_from_slice(&[10, 20, 30, 40]);

        // Act
        buffer.set_length(4);
        let slice = buffer.get_occupied_slice();

        // Assert
        assert_eq!(slice, &[10, 20, 30, 40]);
        assert_eq!(buffer.get_length(), 4);
    }

    #[test]
    #[should_panic(expected = "Length cannot exceed total capacity")]
    fn test_set_length_out_of_bounds_panics() {
        // Arrange
        let mut buffer = AlignedBuffer::new(4, 4);

        // Act & Assert
        buffer.set_length(5);
    }

    #[test]
    fn test_drain_occupied_buffer() {
        // Arrange
        let mut buffer = AlignedBuffer::new(8, 1);
        let _ = buffer.copy_from(&[1, 2, 3, 4, 5], 0, 5);

        // Act
        buffer.drain(2);

        // Assert
        assert_eq!(buffer.get_occupied_slice(), &[3, 4, 5]);
        assert_eq!(buffer.get_length(), 3);
    }

    #[test]
    #[should_panic(expected = "Cannot drain more bytes than currently occupied")]
    fn test_drain_over_occupancy_panics() {
        // Arrange
        let mut buffer = AlignedBuffer::new(8, 1);
        let _ = buffer.copy_from(&[1, 2], 0, 2);

        // Act & Assert
        buffer.drain(3);
    }

    #[test]
    fn test_fill_rest_behavior() {
        // Arrange
        let mut buffer = AlignedBuffer::new(6, 1);
        let _ = buffer.copy_from(&[5, 6, 7], 0, 3);

        // Act
        buffer.fill_rest(9);

        // Assert
        assert_eq!(buffer.get_occupied_slice(), &[5, 6, 7, 9, 9, 9]);
        assert!(buffer.is_full());
    }

    #[test]
    fn test_emptiness_and_fullness_states() {
        // Arrange
        let mut buffer = AlignedBuffer::new(3, 1);

        // Assert Initial State
        assert!(buffer.is_empty());
        assert!(!buffer.is_full());

        // Act Part 1
        let _ = buffer.copy_from(&[1, 2], 0, 2);

        // Assert Partial Filled State
        assert!(!buffer.is_empty());
        assert!(!buffer.is_full());

        // Act Part 2
        let _ = buffer.copy_from(&[3], 0, 1);

        // Assert Fully Filled State
        assert!(!buffer.is_empty());
        assert!(buffer.is_full());
    }
}