//! Session password hashing. Empty password means the room is open.

use sha2::{Digest, Sha256};

/// 32-byte SHA-256 of the password. All zeros when the password is empty.
#[must_use]
pub fn password_token(password: &str) -> [u8; 32] {
    let trimmed = password.trim();
    if trimmed.is_empty() {
        return [0; 32];
    }
    let digest = Sha256::digest(trimmed.as_bytes());
    let mut token = [0_u8; 32];
    token.copy_from_slice(&digest);
    token
}

/// Hex form used in public claims. Empty when the room is open.
#[must_use]
pub fn password_hex(password: &str) -> String {
    let token = password_token(password);
    if token.iter().all(|byte| *byte == 0) {
        String::new()
    } else {
        token.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

/// True when `got` is allowed into a room protected by `expected`.
#[must_use]
pub fn password_allows(expected: &[u8; 32], got: &[u8; 32]) -> bool {
    if expected.iter().all(|byte| *byte == 0) {
        return true;
    }
    expected == got
}

/// Parses a hex SHA-256, or zeros if the string is empty/invalid.
#[must_use]
pub fn token_from_hex(hex: &str) -> [u8; 32] {
    let raw = hex.trim();
    if raw.len() != 64 {
        return [0; 32];
    }
    let mut token = [0_u8; 32];
    for (index, chunk) in raw.as_bytes().chunks_exact(2).enumerate() {
        let Ok(text) = core::str::from_utf8(chunk) else {
            return [0; 32];
        };
        let Ok(value) = u8::from_str_radix(text, 16) else {
            return [0; 32];
        };
        token[index] = value;
    }
    token
}

#[cfg(test)]
mod tests {
    use super::{password_allows, password_hex, password_token, token_from_hex};

    #[test]
    fn empty_password_is_open() {
        assert_eq!(password_token(""), [0; 32]);
        assert_eq!(password_hex("   "), "");
        assert!(password_allows(&[0; 32], &[1; 32]));
    }

    #[test]
    fn matching_password_is_required_when_set() {
        let expected = password_token("mix-secret");
        assert!(!expected.iter().all(|byte| *byte == 0));
        assert!(password_allows(&expected, &password_token("mix-secret")));
        assert!(!password_allows(&expected, &password_token("wrong")));
        assert_eq!(token_from_hex(&password_hex("mix-secret")), expected);
    }
}
