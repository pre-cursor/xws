use crate::kernel::workspace::WorkspaceStore;
use clap::Args;

#[derive(Args)]
pub struct PathArgs {
    /// Name of the workspace whose path should be printed
    pub name: String,
}

pub fn run(args: PathArgs) -> Result<u8, Box<dyn std::error::Error>> {
    let store = WorkspaceStore::from_environment()?;

    let path = store.workspace_path(&args.name)?;
    println!("{}", path.display());

    Ok(0)
}
