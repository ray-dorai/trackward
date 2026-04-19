use sha2::{Digest, Sha256};

/// Compute the SHA-256 digest of a byte slice, returned as a hex string.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Serialize `Vec<u8>` as a hex string in JSON. Used for `row_hash` and
/// `prev_hash` so dossier payloads stay readable instead of turning into
/// arrays of 32 integers.
pub mod hex_bytes {
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        use serde::Deserialize;
        let s = String::deserialize(d)?;
        hex::decode(&s).map_err(serde::de::Error::custom)
    }
}

/// Same as [`hex_bytes`] but for `Option<Vec<u8>>` (nullable columns).
pub mod hex_bytes_opt {
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        bytes: &Option<Vec<u8>>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        match bytes {
            Some(b) => s.serialize_str(&hex::encode(b)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
        use serde::Deserialize;
        let s = Option::<String>::deserialize(d)?;
        match s {
            Some(s) => Ok(Some(
                hex::decode(&s).map_err(serde::de::Error::custom)?,
            )),
            None => Ok(None),
        }
    }
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
