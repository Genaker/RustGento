use base64::Engine;

/// Encodes an entity ID as the base64 opaque "uid" Magento/Venia storefronts
/// expect for category/product identifiers -- just `base64(to_string(id))`,
/// matching Go's `MagentoCategories`/`MagentoProducts` resolvers.
pub fn encode(id: u64) -> String {
    base64::engine::general_purpose::STANDARD.encode(id.to_string())
}

/// Decodes a uid back to an entity ID. Returns `None` for anything that
/// isn't valid base64 encoding a plain integer -- callers should treat that
/// as "no matching entity" rather than an error, since a malformed uid from
/// a client is just an unmatched filter, not a fault.
pub fn decode(uid: &str) -> Option<u64> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(uid).ok()?;
    let s = String::from_utf8(bytes).ok()?;
    s.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_encode_and_decode() {
        for id in [0u64, 1, 42, 999_999_999] {
            assert_eq!(decode(&encode(id)), Some(id));
        }
    }

    #[test]
    fn decode_rejects_invalid_base64() {
        assert_eq!(decode("not valid base64!!!"), None);
    }

    #[test]
    fn decode_rejects_base64_of_non_numeric_content() {
        let encoded = base64::engine::general_purpose::STANDARD.encode("not-a-number");
        assert_eq!(decode(&encoded), None);
    }
}
