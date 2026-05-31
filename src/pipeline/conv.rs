pub fn to_lower(buffer: &mut [u8]) {
    for byte in buffer.iter_mut() {
        *byte = byte.to_ascii_lowercase();
    }
}

pub fn to_upper(buffer: &mut [u8]) {
    for byte in buffer.iter_mut() {
        *byte = byte.to_ascii_uppercase();
    }
}

pub fn swap(buffer: &mut [u8]) -> usize {
    for i in (0..buffer.len()-1).step_by(2) {
        buffer.swap(i, i + 1);
    }

    if buffer.len() % 2 == 0 { buffer.len() } else { buffer.len() - 1 }
}
