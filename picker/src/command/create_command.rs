use crate::kernel::workspace::WorkspaceStore;
use clap::Args;

#[derive(Args)]
pub struct CreateArgs {
    /// Name of the workspace to create
    pub name: String,

    /// Profiles to include (can be specified multiple times)
    #[arg(long = "profile", value_name = "PROFILE")]
    pub profiles: Vec<String>,

    /// Features to include (can be specified multiple times)
    #[arg(long = "with-feature", value_name = "FEATURE")]
    pub features: Vec<String>,
}

pub fn run(args: CreateArgs) -> Result<u8, Box<dyn std::error::Error>> {
    let store = WorkspaceStore::from_environment()?;

    store.create(&args.name, &args.profiles, &args.features)?;
    println!("Created workspace: {}", args.name);
    Ok(0)
}
