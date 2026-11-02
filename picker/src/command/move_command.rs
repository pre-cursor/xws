use crate::kernel::{
    session::SessionStore,
    workspace::{ActiveContext, WorkspaceStore},
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
    let other_shells = |name: &str| match &sessions {
        Some(sessions) => sessions.ensure_inactive_in_other_shells(name),
        None => Ok(()),
    };
    store.move_workspace(&active, &other_shells, &args.source, &args.target)?;
    println!("Moved workspace {} to {}", args.source, args.target);

    Ok(0)
}
