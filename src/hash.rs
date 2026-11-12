use sha2::{Digest, Sha256};

/// Lowercase hexadecimal representation of a byte slice.
pub fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // Writing to a String cannot fail.
        let _ = write!(output, "{byte:02x}");
    }
    output
}

/// SHA-256 of `bytes` as lowercase hexadecimal.
pub fn sha256_hex(bytes: &[u8]) -> String {
    to_hex(&Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_hex() {
        assert_eq!(to_hex(&[0x00, 0x0f, 0xff]), "000fff");
    }

    #[test]
    fn hashes_with_sha256() {
        assert_eq!(
            sha256_hex(b"radium"),
            "023dfb3b81fc4ae0fc11b4bbf25309a62bc01adfaee0ed164cf3ae75ecf3d687"
        );
    }
}
