use clap::Args;

#[derive(Args)]
pub struct VersionArgs {}

pub fn run(_args: VersionArgs) -> Result<u8, Box<dyn std::error::Error>> {
    println!("xws {}", env!("CARGO_PKG_VERSION"));
    Ok(0)
}
