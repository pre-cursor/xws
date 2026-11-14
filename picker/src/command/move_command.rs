use crate::{
    command::helpers::ensure_inactive_in_other_shells,
    kernel::{
        session::SessionStore,
        workspace::{ActiveContext, WorkspaceStore},
    },
};
use clap::Args;

#[derive(Args)]
pub struct MoveArgs {
    pub source: String,
    pub target: String,
}

pub fn run(args: MoveArgs) -> Result<u8, Box<dyn std::error::Error>> {
    let store = WorkspaceStore::from_environment()?;
    let active = ActiveContext::from_env();

    let sessions = SessionStore::from_environment().ok();
    let other_shells = ensure_inactive_in_other_shells(sessions);

    store.move_workspace(&active, &other_shells, &args.source, &args.target)?;
    println!("Moved workspace {} to {}", args.source, args.target);

    Ok(0)
}
