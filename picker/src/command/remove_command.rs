use crate::{
    command::helpers::ensure_inactive_in_other_shells,
    kernel::{
        session::SessionStore,
        workspace::{ActiveContext, WorkspaceStore},
    },
};
use clap::Args;

#[derive(Args)]
pub struct RemoveArgs {
    #[arg(num_args = 1..)]
    pub names: Vec<String>,
}

pub fn run(args: RemoveArgs) -> Result<u8, Box<dyn std::error::Error>> {
    let store = WorkspaceStore::from_environment()?;
    let active = ActiveContext::from_env();

    let sessions = SessionStore::from_environment().ok();
    let other_shells = ensure_inactive_in_other_shells(sessions);

    let mut failed = false;
    for (name, result) in
        args.names
            .iter()
            .zip(store.remove_many(&active, &other_shells, &args.names))
    {
        if let Err(error) = result {
            eprintln!("xws rm: {name}: {error}");
            failed = true;
        }
    }
    if failed { Ok(1) } else { Ok(0) }
}
