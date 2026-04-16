use sha2::{Digest, Sha256};

/// Compute the SHA-256 digest of a byte slice, returned as a hex string.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_digest() {
        // SHA-256 of empty string
        let digest = sha256_hex(b"");
        assert_eq!(
            digest,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
