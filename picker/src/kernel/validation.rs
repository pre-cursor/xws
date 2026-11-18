use std::error::Error;

pub fn validate_name(name: &str) -> Result<(), Box<dyn Error>> {
    let mut chars = name.chars();
    let valid_first = chars.next().is_some_and(|ch| ch.is_ascii_alphanumeric());
    if !valid_first
        || !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.')
    {
        return Err(format!(
            "invalid name '{}': must start with an ASCII letter or digit and contain only ASCII letters, digits, '_', '-' or '.'",
            name.escape_debug()
        )
        .into());
    }
    Ok(())
}

/// Names that change how a shell or the dynamic loader behaves. Blocked as a
/// floor for `bin/env` and for restored session frames.
const RESERVED_ENVIRONMENT_NAMES: &[&str] = &[
    // Shell state and startup
    "PATH",
    "HOME",
    "PWD",
    "OLDPWD",
    "IFS",
    "SHLVL",
    "SHELL",
    "CDPATH",
    "ENV",
    "BASH_ENV",
    "BASHOPTS",
    "SHELLOPTS",
    "ZDOTDIR",
    // Code that runs at prompt time or on interpreter startup
    "PROMPT_COMMAND",
    "PS0",
    "PS1",
    "PS2",
    "PS4",
    "PYTHONSTARTUP",
    // History
    "HISTFILE",
];

/// Whole families, so new loader variables need no list update.
const RESERVED_ENVIRONMENT_PREFIXES: &[&str] = &[
    "XWS_",
    "_XWS_",
    "_xws_",      // xws internals
    "LD_",        // ELF loader (LD_PRELOAD, LD_LIBRARY_PATH, ...)
    "DYLD_",      // macOS loader (DYLD_INSERT_LIBRARIES, ...)
    "BASH_FUNC_", // exported bash functions
];

pub fn validate_environment_name(name: &str) -> Result<(), Box<dyn Error>> {
    let mut chars = name.chars();
    let valid_first = chars
        .next()
        .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic());
    if !valid_first
        || !chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        || RESERVED_ENVIRONMENT_NAMES.contains(&name)
        || RESERVED_ENVIRONMENT_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
    {
        return Err(format!("invalid or reserved environment variable name: '{name}'").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_simple_workspace_profile_and_feature_names() {
        for name in [
            "project-1",
            "default",
            "rust",
            "conda",
            "python",
            "go",
            "venv",
            "java_21",
            "example.com",
            "www.example.co.uk",
        ] {
            assert!(validate_name(name).is_ok(), "{name:?} should be accepted");
        }
    }

    #[test]
    fn rejects_names_outside_the_simple_identifier_rule() {
        for name in ["", ".", "..", "-x", ".foo", "two words", "two\nlines", "é"] {
            assert!(validate_name(name).is_err(), "{name:?} should be rejected");
        }
        for forbidden in [
            ':', ';', '*', '%', '$', '#', '@', '!', '(', ')', '&', '^', '<', '>', ',', '/', '?',
            '\'', '"', '{', '}', '[', ']', '\\', '+', '=', '~',
        ] {
            let name = format!("a{forbidden}b");
            assert!(validate_name(&name).is_err(), "{name:?} should be rejected");
        }
    }

    #[test]
    fn rejects_reserved_environment_names() {
        for name in ["XWS_WORKSPACE", "_xws_local_name", "PATH"] {
            assert!(
                validate_environment_name(name).is_err(),
                "{name} should be rejected"
            );
        }
    }

    #[test]
    fn loader_and_prompt_hook_variables_are_reserved() {
        for name in [
            "PATH",
            "LD_PRELOAD",
            "LD_LIBRARY_PATH",
            "DYLD_INSERT_LIBRARIES",
            "DYLD_FRAMEWORK_PATH",
            "BASH_FUNC_foo",
            "PROMPT_COMMAND",
            "PS1",
            "PYTHONSTARTUP",
            "HISTFILE",
        ] {
            assert!(
                validate_environment_name(name).is_err(),
                "{name} should be reserved"
            );
        }
        for name in [
            "GOPATH",
            "RUST_LOG",
            "CC",
            "PYTHONPATH",
            "TMPDIR",
            "LDFLAGS",
        ] {
            assert!(
                validate_environment_name(name).is_ok(),
                "{name} should be allowed"
            );
        }
    }
}
