use crate::kernel::workspace::WorkspaceStore;
use clap::Args;

#[derive(Args)]
pub struct ProfilesArgs {}

pub fn run(_: ProfilesArgs) -> Result<u8, Box<dyn std::error::Error>> {
    let store = WorkspaceStore::from_environment()?;

    for profile in store.profiles()? {
        println!("{profile}");
    }

    Ok(0)
}
