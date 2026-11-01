use crate::kernel::workspace::WorkspaceStore;
use clap::Args;
use std::env;

#[derive(Args)]
pub struct ListArgs {}

pub fn run(_: ListArgs) -> Result<u8, Box<dyn std::error::Error>> {
    let store = WorkspaceStore::from_environment()?;

    let active = env::var("XWS_WORKSPACE")
        .ok()
        .filter(|name| !name.is_empty());
    for name in store.list()? {
        let marker = if active.as_deref() == Some(name.as_str()) {
            " *"
        } else {
            ""
        };
        println!("{name}{marker}");
    }

    Ok(0)
}
