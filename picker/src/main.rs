use clap::Parser;
use std::error::Error;
use std::process::ExitCode;

use xws_picker::command;

#[derive(Parser)]
#[command(name = "xws", bin_name = "xws", version)]
struct Cli {
    #[clap(subcommand)]
    command: command::PickerSubcommand,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            eprintln!("xws: {err}");
            let mut source = err.source();
            while let Some(cause) = source {
                eprintln!("  Caused by: {cause}");
                source = cause.source();
            }
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<u8, Box<dyn Error>> {
    let cli = Cli::parse();
    command::handle(cli.command)
}
