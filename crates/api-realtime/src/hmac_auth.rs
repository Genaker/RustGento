use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

/// Verifies the realtime API's HMAC signature:
/// `hex(HMAC-SHA256(key=MAGENTO_CRYPT_KEY, msg=customer_id))`, compared to
/// `signature_hex` in constant time. Matches Go's scheme exactly, INCLUDING
/// its lack of a nonce/timestamp -- a valid signature is replayable
/// indefinitely. That's a real limitation of the original scheme, not
/// something this port "fixes": the plan calls for behavioral parity on
/// this specific point, since it's about matching an existing client
/// integration's expectations, not a security review of Go's design.
pub fn verify(key: &[u8], customer_id: &str, signature_hex: &str) -> bool {
    let Ok(mut mac) = HmacSha256::new_from_slice(key) else { return false };
    mac.update(customer_id.as_bytes());
    let expected = mac.finalize().into_bytes();

    let Ok(provided) = hex::decode(signature_hex) else { return false };
    expected.as_slice().ct_eq(&provided).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sign(key: &[u8], customer_id: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(key).unwrap();
        mac.update(customer_id.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    #[test]
    fn accepts_a_correctly_signed_request() {
        let key = b"test-crypt-key";
        let sig = sign(key, "42");
        assert!(verify(key, "42", &sig));
    }

    #[test]
    fn rejects_a_signature_for_a_different_customer_id() {
        let key = b"test-crypt-key";
        let sig = sign(key, "42");
        assert!(!verify(key, "99", &sig));
    }

    #[test]
    fn rejects_a_signature_made_with_the_wrong_key() {
        let sig = sign(b"wrong-key", "42");
        assert!(!verify(b"test-crypt-key", "42", &sig));
    }

    #[test]
    fn rejects_malformed_hex() {
        assert!(!verify(b"test-crypt-key", "42", "not-valid-hex!!!"));
    }

    #[test]
    fn rejects_valid_hex_that_is_not_a_valid_signature() {
        assert!(!verify(b"test-crypt-key", "42", "deadbeef"));
    }

    #[test]
    fn a_signature_is_replayable_for_the_same_customer_id() {
        // Pinning the documented limitation: no nonce/timestamp means the
        // exact same signature verifies every time for the same input.
        let key = b"test-crypt-key";
        let sig = sign(key, "42");
        assert!(verify(key, "42", &sig));
        assert!(verify(key, "42", &sig));
    }
}
