use clap::Subcommand;

mod create_command;
mod features_command;
mod helpers;
mod list_command;
mod move_command;
mod path_command;
mod pick_command;
mod profiles_command;
mod protocol_command;
mod remove_command;
mod session_command;
mod version_command;
mod which_command;

#[derive(Subcommand)]
pub enum PickerSubcommand {
    // --- Workspace Management ---
    #[command(about = "Create a workspace")]
    Create(create_command::CreateArgs),

    #[command(about = "List workspaces", visible_alias = "ls")]
    List(list_command::ListArgs),

    #[command(about = "Rename a workspace", visible_alias = "mv")]
    Move(move_command::MoveArgs),

    #[command(about = "Print a workspace's path")]
    Path(path_command::PathArgs),

    #[command(about = "Pick a workspace", hide = true)]
    Pick(pick_command::PickArgs),

    #[command(about = "Remove a workspace", visible_alias = "rm")]
    Remove(remove_command::RemoveArgs),

    #[command(about = "Print a workspace's bin directory")]
    Which(which_command::WhichArgs),

    // --- Templates & Discovery ---
    #[command(about = "List available features")]
    Features(features_command::FeaturesArgs),

    #[command(about = "List available profiles")]
    Profiles(profiles_command::ProfilesArgs),

    // --- Shell Integration & Protocol ---
    #[command(about = "Manage protocols", hide = true)]
    Protocol(protocol_command::ProtocolArgs),

    #[command(about = "Manage sessions", hide = true)]
    Session(session_command::SessionArgs),

    // --- General ---
    #[command(about = "Show version information")]
    Version(version_command::VersionArgs),
}

pub fn handle(command: PickerSubcommand) -> Result<u8, Box<dyn std::error::Error>> {
    match command {
        // Workspace Management
        PickerSubcommand::Create(args) => create_command::run(args),
        PickerSubcommand::List(args) => list_command::run(args),
        PickerSubcommand::Move(args) => move_command::run(args),
        PickerSubcommand::Path(args) => path_command::run(args),
        PickerSubcommand::Pick(args) => pick_command::run(args),
        PickerSubcommand::Remove(args) => remove_command::run(args),
        PickerSubcommand::Which(args) => which_command::run(args),

        // Templates & Discovery
        PickerSubcommand::Features(args) => features_command::run(args),
        PickerSubcommand::Profiles(args) => profiles_command::run(args),

        // Shell Integration & Protocol
        PickerSubcommand::Protocol(args) => protocol_command::run(args),
        PickerSubcommand::Session(args) => session_command::run(args),

        // General
        PickerSubcommand::Version(args) => version_command::run(args),
    }
}
