#[cfg(not(unix))]
compile_error!("xws-picker supports Unix platforms only (Linux, macOS, etc.)");

pub mod command;
pub mod kernel;
