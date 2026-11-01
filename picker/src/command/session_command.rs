use std::error::Error;

use clap::{Args, Subcommand};

use super::helpers::{decode, encode};
use crate::kernel::{
    session::{ActivationFrame, SessionStore},
    workspace::{validate_environment_name, validate_name},
};

type VariableRecord = (String, String, Option<String>);

#[derive(Args)]
pub struct SessionArgs {
    #[command(subcommand)]
    pub operation: SessionSubcommand,
}

#[derive(Subcommand)]
pub enum SessionSubcommand {
    /// Create a new session
    New,

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
        SessionSubcommand::New => {
            println!("XWS/1\tsession\t{}", sessions.create()?);
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
                    "XWS/1\tpopped\t{}\t{}\t{}",
                    encode(&frame.workspace),
                    encode(&frame.workspace_path),
                    encode(&frame.return_dir)
                );
                for (name, state, value) in frame.previous_vars {
                    match value {
                        Some(value) => {
                            println!("XWS/1\tvar\t{name}\t{state}\t{}", encode(&value))
                        }
                        None => println!("XWS/1\tvar\t{name}\tunset\t-"),
                    }
                }
                if let Some(parent) = parent {
                    println!(
                        "XWS/1\tparent\t{}\t{}",
                        encode(&parent.workspace),
                        encode(&parent.workspace_path)
                    );
                } else {
                    println!("XWS/1\tempty");
                }
                Ok(0)
            }
            None => Err("xws: session stack is empty".into()),
        },

        SessionSubcommand::Peek { id } => {
            let frame = sessions.top(&id)?.ok_or("xws: session stack is empty")?;
            println!(
                "XWS/1\ttop\t{}\t{}\t{}",
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
            println!("XWS/1\t{kind}");
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
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() != 5 || fields[0] != "XWS/1" || fields[1] != "var" {
            return Err("invalid XWS/1 session input record".into());
        }
        validate_environment_name(fields[2])?;
        if !seen.insert(fields[2].to_owned()) {
            return Err(format!("duplicate XWS/1 variable record '{}'", fields[2]).into());
        }
        let value = match fields[3] {
            "set-exported" | "set-local" => Some(decode(fields[4])?),
            "unset" if fields[4] == "-" => None,
            _ => return Err("invalid XWS/1 variable state".into()),
        };
        vars.push((fields[2].to_owned(), fields[3].to_owned(), value));
    }
    Ok(vars)
}
