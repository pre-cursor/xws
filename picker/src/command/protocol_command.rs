use std::error::Error;

use clap::{Args, Subcommand};

use super::helpers::encode;
use crate::kernel::{picker, workspace::WorkspaceStore};

#[derive(Args)]
pub struct ProtocolArgs {
    #[command(subcommand)]
    pub operation: ProtocolSubcommand,
}

#[derive(Subcommand)]
pub enum ProtocolSubcommand {
    Env {
        /// workspace name
        name: String,
    },

    Path {
        /// workspace name
        name: String,
    },

    Pick {
        /// active workspace
        active: Option<String>,
    },
}

pub fn run(args: ProtocolArgs) -> Result<u8, Box<dyn Error>> {
    let store = WorkspaceStore::from_environment()?;

    match args.operation {
        ProtocolSubcommand::Env { name } => {
            for (key, value) in store.environment(&name)? {
                println!("XWS/1\tvar\t{key}\t{}", encode(&value));
            }
            Ok(0)
        }
        ProtocolSubcommand::Path { name } => {
            println!(
                "XWS/1\tpath\t{}",
                encode(&store.workspace_path(&name)?.to_string_lossy())
            );
            Ok(0)
        }
        ProtocolSubcommand::Pick { active } => {
            let active = active.filter(|name| !name.is_empty());
            let workspaces = store.list()?;
            if workspaces.is_empty() {
                return Err(format!(
                    "no workspaces found in {}",
                    store.workspaces_dir().display()
                )
                .into());
            }
            match picker::pick(&workspaces, active.as_deref())? {
                Some(name) => println!("XWS/1\tselected\t{}", encode(&name)),
                None => println!("XWS/1\tcancel"),
            }
            Ok(0)
        }
    }
}
