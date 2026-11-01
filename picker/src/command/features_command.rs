use crate::kernel::workspace::WorkspaceStore;
use clap::Args;

#[derive(Args)]
pub struct FeaturesArgs {}

pub fn run(_: FeaturesArgs) -> Result<u8, Box<dyn std::error::Error>> {
    let store = WorkspaceStore::from_environment()?;

    for feature in store.features()? {
        println!("{feature}");
    }

    Ok(0)
}
