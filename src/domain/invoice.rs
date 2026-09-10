use std::{error::Error, fmt};

const MAX_USER_LEN: usize = 64;

pub fn validate_user(user: &str) -> Result<(), LightningAddressUserError> {
    // URL path joining normalizes these whole segments instead of preserving
    // the Lightning Address identity used for discovery and persistence.
    if user.is_empty() || user.len() > MAX_USER_LEN || matches!(user, "." | "..") {
        return Err(LightningAddressUserError);
    }

    if user.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
    }) {
        Ok(())
    } else {
        Err(LightningAddressUserError)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightningAddressUserError;

impl fmt::Display for LightningAddressUserError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid Lightning Address user")
    }
}

impl Error for LightningAddressUserError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_phase1_style_users() {
        for user in [
            "herd",
            "dexter",
            "rowan",
            "cosmo",
            "newton",
            "nova",
            "goat-1",
            "goat.name",
        ] {
            validate_user(user).unwrap();
        }
    }

    #[test]
    fn rejects_uppercase_colons_and_invalid_lengths() {
        for user in ["Herd", "herd:other", "", "with space", ".", ".."] {
            assert!(validate_user(user).is_err());
        }
        assert!(validate_user(&"a".repeat(MAX_USER_LEN + 1)).is_err());
    }
}
