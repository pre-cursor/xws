use crate::kernel::workspace::WorkspaceStore;
use clap::Args;

#[derive(Args)]
pub struct WhichArgs {
    /// Name of the workspace whose bin directory should be printed
    pub name: String,
}

pub fn run(args: WhichArgs) -> Result<u8, Box<dyn std::error::Error>> {
    let store = WorkspaceStore::from_environment()?;

    let path = store.workspace_path(&args.name)?;
    println!("{}", path.join("bin").display());

    Ok(0)
}
