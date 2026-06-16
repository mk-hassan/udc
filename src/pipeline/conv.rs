//! Data conversion utilities for the block-stream transformation pipeline.
//!
//! This module provides synchronous, in-place modifier functions acting over mutable
//! byte segments to enforce text casing constraints and word/byte alignment configurations.

use super::DataOps;

/// Converts all ASCII alphabetic characters in the provided buffer to lowercase.
///
/// Non-alphabetic ASCII characters, numbers, control symbols, and raw multi-byte
/// non-ASCII binary patterns are left completely unchanged.
///
/// ## Examples
///
/// ```ignore
/// use udc::conv::to_lower;
///
/// let mut buf = *b"DATA-Pipeline-2026";
/// to_lower(&mut buf);
/// assert_eq!(&buf, b"data-pipeline-2026");
/// ```
pub fn to_lower(buffer: &mut [u8]) {
    for byte in buffer.iter_mut() {
        *byte = byte.to_ascii_lowercase();
    }
}

/// Converts all ASCII alphabetic characters in the provided buffer to uppercase.
///
/// Non-alphabetic ASCII characters, numbers, control symbols, and raw multi-byte
/// non-ASCII binary patterns are left completely unchanged.
///
/// ## Examples
///l
/// ```ignore
/// use udc::conv::to_upper;
///
/// let mut buf = *b"data-pipeline-2026";
/// to_upper(&mut buf);
/// assert_eq!(&buf, b"DATA-PIPELINE-2026");
/// ```
pub fn to_upper(buffer: &mut [u8]) {
    for byte in buffer.iter_mut() {
        *byte = byte.to_ascii_uppercase();
    }
}

/// Swaps every adjacent pair of bytes sequentially across the slice.
///
/// This provides standard word endian/byte-flipping capabilities (`conv=swab`).
///
/// ## Corner Case Handling
///
/// If an odd-length byte slice is processed, the final trailing byte is intentionally left
/// in place and skipped to guarantee safe operation and prevent out-of-bounds panics.
///
/// ## Examples
///
/// ```ignore
/// use udc::conv::swap;
///
/// let mut buf = *b"12345";
/// swap(&mut buf);
/// assert_eq!(&buf, b"21435");
/// ```
pub fn swap(buffer: &mut [u8]) {
    // Drop down to the nearest even boundary to mask off safe 2-byte window groups
    let safe_len = buffer.len() / 2 * 2;
    for i in (0..safe_len).step_by(2) {
        buffer.swap(i, i + 1);
    }
}

/// Batch applies an ordered sequence of transformation operations over a buffer in place.
///
/// The conversions execute sequentially from left to right exactly in the order they
/// are structured within the array reference.
///
/// ## Examples
///
/// ```ignore
/// use udc::conv::convert;
/// use udc::DataOps;
///
/// let mut buf = *b"aBcDe";
/// let ops = [DataOps::ToUpper, DataOps::Swap];
/// convert(&mut buf, &ops);
/// assert_eq!(&buf, b"BADCe");
/// ```
pub fn convert(buffer: &mut [u8], convs: u8) {
    if convs & DataOps::ToLower as u8 != 0 {
        to_lower(buffer);
    }
    if convs & DataOps::ToUpper as u8 != 0 {
        to_upper(buffer);
    }
    if convs & DataOps::Swap as u8 != 0 {
        swap(buffer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_lower_standard_mixed_case() {
        // Arrange
        let mut buffer = *b"AbCdEfG123!@#";

        // Act
        to_lower(&mut buffer);

        // Assert
        assert_eq!(&buffer, b"abcdefg123!@#");
    }

    #[test]
    fn test_to_lower_empty_buffer() {
        // Arrange
        let mut buffer: [u8; 0] = [];

        // Act
        to_lower(&mut buffer);

        // Assert
        assert_eq!(&buffer, b"");
    }

    #[test]
    fn test_to_lower_handles_non_ascii_safely() {
        // Arrange
        let mut buffer = [0x41, 0xFF, 0x42, 0x80];

        // Act
        to_lower(&mut buffer);

        // Assert
        assert_eq!(&buffer, &[0x61, 0xFF, 0x62, 0x80]);
    }

    #[test]
    fn test_to_upper_standard_mixed_case() {
        // Arrange
        let mut buffer = *b"AbCdEfG123!@#";

        // Act
        to_upper(&mut buffer);

        // Assert
        assert_eq!(&buffer, b"ABCDEFG123!@#");
    }

    #[test]
    fn test_to_upper_empty_buffer() {
        // Arrange
        let mut buffer: [u8; 0] = [];

        // Act
        to_upper(&mut buffer);

        // Assert
        assert_eq!(&buffer, b"");
    }

    #[test]
    fn test_to_upper_handles_non_ascii_safely() {
        // Arrange
        let mut buffer = [0x61, 0xFF, 0x62, 0x80];

        // Act
        to_upper(&mut buffer);

        // Assert
        assert_eq!(&buffer, &[0x41, 0xFF, 0x42, 0x80]);
    }

    #[test]
    fn test_swap_even_length() {
        // Arrange
        let mut buffer = *b"abcdefgh";

        // Act
        swap(&mut buffer);

        // Assert
        assert_eq!(&buffer, b"badcfehg");
    }

    #[test]
    fn test_swap_odd_length_leaves_last_byte_untouched() {
        // Arrange
        let mut buffer = *b"abcdefg";

        // Act
        swap(&mut buffer);

        // Assert
        assert_eq!(&buffer, b"badcfeg");
    }

    #[test]
    fn test_swap_single_byte() {
        // Arrange
        let mut buffer = *b"z";

        // Act
        swap(&mut buffer);

        // Assert
        assert_eq!(&buffer, b"z");
    }

    #[test]
    fn test_swap_empty_buffer() {
        // Arrange
        let mut buffer: [u8; 0] = [];

        // Act
        swap(&mut buffer);

        // Assert
        assert_eq!(&buffer, b"");
    }

    #[test]
    fn test_convert_empty_operations_list() {
        // Arrange
        let mut buffer = *b"UnchangedData";
        let operations = 0u8;

        // Act
        convert(&mut buffer, operations);

        // Assert
        assert_eq!(&buffer, b"UnchangedData");
    }

    #[test]
    fn test_convert_sequential_chained_execution() {
        // Arrange
        let mut buffer = *b"aBcDeF";
        let operations = DataOps::ToUpper as u8 | DataOps::Swap as u8;

        // Act
        convert(&mut buffer, operations);

        // Assert
        assert_eq!(&buffer, b"BADCFE");
    }
}
