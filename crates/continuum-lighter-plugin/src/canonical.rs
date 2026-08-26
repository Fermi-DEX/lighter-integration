//! Consensus-preimage helpers. All integers are big-endian and every
//! variable-length byte string is length-prefixed.

pub fn put_u8(out: &mut Vec<u8>, value: u8) {
    out.push(value);
}

pub fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

pub fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

pub fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

pub fn put_fixed<const N: usize>(out: &mut Vec<u8>, value: &[u8; N]) {
    out.extend_from_slice(value);
}

pub fn put_lighter_hash(out: &mut Vec<u8>, value: &[u64; 4]) {
    for limb in value {
        put_u64(out, *limb);
    }
}

pub fn put_bytes(out: &mut Vec<u8>, value: &[u8]) {
    put_u32(
        out,
        value
            .len()
            .try_into()
            .expect("canonical byte string exceeds u32"),
    );
    out.extend_from_slice(value);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_are_big_endian() {
        let mut out = Vec::new();
        put_u16(&mut out, 0x0102);
        put_u32(&mut out, 0x03040506);
        put_u64(&mut out, 0x0708090a0b0c0d0e);
        assert_eq!(out, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14]);
    }
}
