use clap::Args;

use crate::kernel::picker;

const CANCELLED: u8 = 130;

#[derive(Args)]
pub struct PickArgs {
    /// Name of the currently active workspace to pre-select in the picker.
    ///
    /// This argument is optional (0..1):
    /// - If omitted (0 args), the picker starts with the first workspace selected.
    /// - If provided (1 arg), the picker centers and highlights that workspace.
    /// - Passing 2 or more arguments is rejected by clap as unexpected arguments.
    #[arg(value_name = "ACTIVE_WORKSPACE")]
    pub active: Option<String>,
}

pub fn run(args: PickArgs) -> Result<u8, Box<dyn std::error::Error>> {
    let store = crate::kernel::workspace::WorkspaceStore::from_environment()?;

    // `active` is optional (0..1 positional args). When omitted, it is `None`
    // and the picker defaults to selecting the first entry. Filter out empty strings
    // in case the caller passed an unset shell variable (e.g. `"$XWS_WORKSPACE"`).

    let active = args.active.as_deref().filter(|name| !name.is_empty());

    let workspaces = store.list()?;
    if workspaces.is_empty() {
        return Err(format!(
            "no workspaces found in {}",
            store.workspaces_dir().display()
        )
        .into());
    }

    match picker::pick(&workspaces, active)? {
        Some(name) => {
            println!("{name}");
            Ok(0)
        }
        None => Ok(CANCELLED),
    }
}
