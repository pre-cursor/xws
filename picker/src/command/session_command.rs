use std::error::Error;

use clap::{Args, Subcommand};

use super::helpers::encode;
use super::wire::{PROTOCOL_VERSION, VariableRecord, parse_session_variable_record};
use crate::kernel::{
    session::{ActivationFrame, SessionStore},
    validation::validate_name,
};

#[derive(Args)]
pub struct SessionArgs {
    #[command(subcommand)]
    pub operation: SessionSubcommand,
}

#[derive(Subcommand)]
pub enum SessionSubcommand {
    /// Create a new session
    New {
        /// Process id of the owning shell; lets stale sessions be detected
        #[arg(long)]
        shell_pid: Option<u32>,
    },

    /// Push an activation frame onto the session stack
    Push {
        id: String,
        workspace: String,
        workspace_path: String,
        return_dir: String,
    },

    /// Pop an activation frame from the session stack
    Pop { id: String },

    /// Peek at the top activation frame on the session stack
    Peek { id: String },

    /// Check if a workspace is present in the session stack
    Contains { id: String, workspace: String },
}

pub fn run(args: SessionArgs) -> Result<u8, Box<dyn std::error::Error>> {
    let sessions = SessionStore::from_environment()?;

    match args.operation {
        SessionSubcommand::New { shell_pid } => {
            println!(
                "{PROTOCOL_VERSION}\tsession\t{}",
                sessions.create(shell_pid)?
            );
            Ok(0)
        }

        SessionSubcommand::Push {
            id,
            workspace,
            workspace_path,
            return_dir,
        } => {
            validate_name(&workspace)?;
            let vars = read_var_records()?; // reads from stdin as before
            sessions.push(
                &id,
                ActivationFrame {
                    workspace,
                    workspace_path,
                    return_dir,
                    previous_vars: vars,
                },
            )?;
            Ok(0)
        }

        SessionSubcommand::Pop { id } => match sessions.pop(&id)? {
            Some((frame, parent)) => {
                println!(
                    "{PROTOCOL_VERSION}\tpopped\t{}\t{}\t{}",
                    encode(&frame.workspace),
                    encode(&frame.workspace_path),
                    encode(&frame.return_dir)
                );
                for (name, state, value) in frame.previous_vars {
                    match value {
                        Some(value) => {
                            println!(
                                "{PROTOCOL_VERSION}\tvar\t{name}\t{state}\t{}",
                                encode(&value)
                            )
                        }
                        None => println!("{PROTOCOL_VERSION}\tvar\t{name}\tunset\t-"),
                    }
                }
                if let Some(parent) = parent {
                    println!(
                        "{PROTOCOL_VERSION}\tparent\t{}\t{}",
                        encode(&parent.workspace),
                        encode(&parent.workspace_path)
                    );
                } else {
                    println!("{PROTOCOL_VERSION}\tempty");
                }
                Ok(0)
            }
            None => Err("xws: session stack is empty".into()),
        },

        SessionSubcommand::Peek { id } => {
            let frame = sessions.top(&id)?.ok_or("xws: session stack is empty")?;
            println!(
                "{PROTOCOL_VERSION}\ttop\t{}\t{}\t{}",
                encode(&frame.workspace),
                encode(&frame.workspace_path),
                encode(&frame.return_dir)
            );
            Ok(0)
        }

        SessionSubcommand::Contains { id, workspace } => {
            let kind = if sessions.contains(&id, &workspace)? {
                "contains"
            } else {
                "absent"
            };
            println!("{PROTOCOL_VERSION}\t{kind}");
            Ok(0)
        }
    }
}

fn read_var_records() -> Result<Vec<VariableRecord>, Box<dyn Error>> {
    use std::io::{self, BufRead};
    let mut vars = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in io::stdin().lock().lines() {
        let line = line?;
        let record = parse_session_variable_record(&line)?;
        if !seen.insert(record.0.clone()) {
            return Err(format!(
                "duplicate {PROTOCOL_VERSION} variable record '{}'",
                record.0
            )
            .into());
        }
        vars.push(record);
    }
    Ok(vars)
}
