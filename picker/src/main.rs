use clap::Parser;
use std::error::Error;
use std::io::Write;
use std::process::ExitCode;

use log::error;
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
    init_logger();
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            let mut message = err.to_string();
            let mut source = err.source();
            while let Some(cause) = source {
                message.push_str(&format!("\n  Caused by: {cause}"));
                source = cause.source();
            }
            error!("xws: {message}");
            ExitCode::FAILURE
        }
    }
}

fn init_logger() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .format(|buf, record| writeln!(buf, "{}", record.args()))
        .init();
}

fn run() -> Result<u8, Box<dyn Error>> {
    let cli = Cli::parse();
    command::handle(cli.command)
}
