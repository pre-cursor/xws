use std::error::Error;

use super::helpers::decode;
use crate::kernel::validation::validate_environment_name;

pub(super) const PROTOCOL_VERSION: &str = "XWS/1";

pub(super) type VariableRecord = (String, String, Option<String>);

pub(super) fn parse_session_variable_record(line: &str) -> Result<VariableRecord, Box<dyn Error>> {
    let fields: Vec<_> = line.split('\t').collect();
    if fields.len() != 5 || fields[0] != PROTOCOL_VERSION || fields[1] != "var" {
        return Err(format!("invalid {PROTOCOL_VERSION} session input record").into());
    }

    validate_environment_name(fields[2])?;
    let value = match fields[3] {
        "set-exported" | "set-local" => Some(decode(fields[4])?),
        "unset" if fields[4] == "-" => None,
        _ => return Err(format!("invalid {PROTOCOL_VERSION} variable state").into()),
    };

    Ok((fields[2].to_owned(), fields[3].to_owned(), value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_session_variable_record_decodes_set_value() {
        let record =
            parse_session_variable_record("XWS/1\tvar\tEDITOR\tset-local\tL3Vzci9iaW4=").unwrap();

        assert_eq!(
            record,
            (
                "EDITOR".to_owned(),
                "set-local".to_owned(),
                Some("/usr/bin".to_owned())
            )
        );
    }

    #[test]
    fn parse_session_variable_record_accepts_empty_set_value() {
        let record = parse_session_variable_record("XWS/1\tvar\tEDITOR\tset-exported\t-").unwrap();

        assert_eq!(
            record,
            (
                "EDITOR".to_owned(),
                "set-exported".to_owned(),
                Some(String::new())
            )
        );
    }

    #[test]
    fn parse_session_variable_record_accepts_unset_value() {
        let record = parse_session_variable_record("XWS/1\tvar\tEDITOR\tunset\t-").unwrap();

        assert_eq!(record, ("EDITOR".to_owned(), "unset".to_owned(), None));
    }

    #[test]
    fn parse_session_variable_record_rejects_invalid_record_header() {
        assert!(parse_session_variable_record("XWS/2\tvar\tEDITOR\tunset\t-").is_err());
        assert!(parse_session_variable_record("XWS/1\tpath\tEDITOR\tunset\t-").is_err());
        assert!(parse_session_variable_record("XWS/1\tvar\tEDITOR\tunset").is_err());
    }

    #[test]
    fn parse_session_variable_record_rejects_invalid_state_or_value() {
        assert!(parse_session_variable_record("XWS/1\tvar\tEDITOR\tunknown\t-").is_err());
        assert!(parse_session_variable_record("XWS/1\tvar\tEDITOR\tunset\tvalue").is_err());
    }
}
