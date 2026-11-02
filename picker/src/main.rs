use clap::Parser;
use std::error::Error;
use std::process::ExitCode;

use xws_picker::command;

use clap::builder::styling::{Style, Styles};

const STYLES: Styles = Styles::styled()
    .header(Style::new().bold().underline())
    .usage(Style::new().bold().underline())
    .literal(Style::new().bold());

const HELP_TEMPLATE: &str = "\
{usage-heading} {usage}

\x1b[1;4mWorkspace Management:\x1b[0m
{subcommands}

\x1b[1;4mEnvironment Switching:\x1b[0m
  \x1b[1mxws\x1b[0m               Pick and activate a workspace
  \x1b[1mpush\x1b[0m <workspace>  Activate and push a workspace onto the stack (aliases: activate, use)
  \x1b[1mpop\x1b[0m               Deactivate top workspace and restore environment (alias: deactivate)

\x1b[1;4mOptions:\x1b[0m
{options}

Run 'xws help <command>' for more information about a command";

#[derive(Parser)]
#[command(
    name = "xws",
    bin_name = "xws",
    version,
    styles = STYLES,
    help_template = HELP_TEMPLATE
)]
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
