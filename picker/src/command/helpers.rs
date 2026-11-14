use crate::kernel::session::SessionStore;

use std::error::Error;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

pub fn encode(value: &str) -> String {
    if value.is_empty() {
        "-".to_owned()
    } else {
        STANDARD.encode(value.as_bytes())
    }
}

pub fn decode(value: &str) -> Result<String, Box<dyn Error>> {
    if value == "-" {
        return Ok(String::new());
    }
    Ok(String::from_utf8(STANDARD.decode(value)?)?)
}

pub fn ensure_inactive_in_other_shells(
    sessions: Option<SessionStore>,
) -> impl Fn(&str) -> Result<(), Box<dyn std::error::Error>> {
    move |name| match &sessions {
        Some(sessions) => sessions.ensure_inactive_in_other_shells(name),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_empty_string_returns_dash() {
        assert_eq!(encode(""), "-");
    }

    #[test]
    fn decode_dash_returns_empty_string() {
        assert_eq!(decode("-").unwrap(), "");
    }

    #[test]
    fn encode_decode_round_trip() {
        let original = "hello/world with spaces & special characters = +";
        let encoded = encode(original);
        assert_ne!(encoded, original);
        assert_eq!(decode(&encoded).unwrap(), original);
    }
}
