use crate::kernel::workspace::{ActiveContext, WorkspaceStore};
use clap::Args;

#[derive(Args)]
pub struct MoveArgs {
    pub source: String,
    pub target: String,
}

pub fn run(args: MoveArgs) -> Result<u8, Box<dyn std::error::Error>> {
    let store = WorkspaceStore::from_environment()?;
    let active = ActiveContext::from_env();

    store.move_workspace(&active, &args.source, &args.target)?;
    println!("Moved workspace {} to {}", args.source, args.target);

    Ok(0)
}
