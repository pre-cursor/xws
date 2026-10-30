mod picker;
mod session;
mod workspace;

use std::env;
use std::error::Error;
use std::process::ExitCode;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use session::{ActivationFrame, SessionStore};
use workspace::WorkspaceStore;

type VariableRecord = (String, String, Option<String>);

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("xws: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<u8, Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        print_help();
        return Ok(0);
    };

    match command.as_str() {
        "help" | "--help" | "-h" => {
            if args.next().is_some() {
                return Err("usage: xws-picker help|--help|-h".into());
            }
            print_help();
            Ok(0)
        }
        "version" | "--version" => {
            if args.next().is_some() {
                return Err("usage: xws-picker version|--version".into());
            }
            print_version();
            Ok(0)
        }
        _ => run_workspace_command(&command, args),
    }
}

fn run_workspace_command(
    command: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<u8, Box<dyn Error>> {
    let store = WorkspaceStore::from_environment()?;

    match command {
        "profiles" => {
            if args.next().is_some() {
                return Err("usage: xws-picker profiles".into());
            }
            for profile in store.profiles()? {
                println!("{profile}");
            }
            Ok(0)
        }
        "features" => {
            if args.next().is_some() {
                return Err("usage: xws-picker features".into());
            }
            for feature in store.features()? {
                println!("{feature}");
            }
            Ok(0)
        }
        "list" | "ls" => {
            if args.next().is_some() {
                return Err("usage: xws-picker list".into());
            }
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
        "pick" => {
            let active = args.next().filter(|name| !name.is_empty());
            if args.next().is_some() {
                return Err("usage: xws-picker pick [active-workspace]".into());
            }
            let workspaces = store.list()?;
            if workspaces.is_empty() {
                return Err(format!(
                    "no workspaces found in {}",
                    store.workspaces_dir().display()
                )
                .into());
            }
            match picker::pick(&workspaces, active.as_deref())? {
                Some(name) => {
                    println!("{name}");
                    Ok(0)
                }
                None => Ok(130),
            }
        }
        "path" | "which" => {
            let name = required_arg(args.next(), "workspace name")?;
            if args.next().is_some() {
                return Err("usage: xws-picker path|which <workspace>".into());
            }
            let path = store.workspace_path(&name)?;
            if command == "which" {
                println!("{}", path.join("bin").display());
            } else {
                println!("{}", path.display());
            }
            Ok(0)
        }
        "protocol" => {
            let operation = required_arg(args.next(), "protocol operation")?;
            match operation.as_str() {
                "env" => {
                    let name = required_arg(args.next(), "workspace name")?;
                    if args.next().is_some() {
                        return Err("usage: xws-picker protocol env <workspace>".into());
                    }
                    for (key, value) in store.environment(&name)? {
                        println!("XWS/1\tvar\t{key}\t{}", encode(&value));
                    }
                    Ok(0)
                }
                "path" => {
                    let name = required_arg(args.next(), "workspace name")?;
                    if args.next().is_some() {
                        return Err("usage: xws-picker protocol path <workspace>".into());
                    }
                    println!(
                        "XWS/1\tpath\t{}",
                        encode(&store.workspace_path(&name)?.to_string_lossy())
                    );
                    Ok(0)
                }
                "pick" => {
                    let active = args.next().filter(|name| !name.is_empty());
                    if args.next().is_some() {
                        return Err("usage: xws-picker protocol pick [active-workspace]".into());
                    }
                    let workspaces = store.list()?;
                    if workspaces.is_empty() {
                        return Err(format!(
                            "no workspaces found in {}",
                            store.workspaces_dir().display()
                        )
                        .into());
                    }
                    match picker::pick(&workspaces, active.as_deref())? {
                        Some(name) => println!("XWS/1\tselected\t{}", encode(&name)),
                        None => println!("XWS/1\tcancel"),
                    }
                    Ok(0)
                }
                _ => Err("usage: xws-picker protocol env|path|pick ...".into()),
            }
        }
        "session" => {
            let operation = required_arg(args.next(), "session operation")?;
            let sessions = SessionStore::from_environment()?;
            match operation.as_str() {
                "new" => {
                    if args.next().is_some() {
                        return Err("usage: xws-picker session new".into());
                    }
                    println!("XWS/1\tsession\t{}", sessions.create()?);
                    Ok(0)
                }
                "push" => {
                    let id = required_arg(args.next(), "session id")?;
                    let workspace = required_arg(args.next(), "workspace name")?;
                    workspace::validate_name(&workspace)?;
                    let workspace_path = required_arg(args.next(), "workspace path")?;
                    let return_dir = required_arg(args.next(), "return directory")?;
                    if args.next().is_some() {
                        return Err(
                            "usage: xws-picker session push <id> <workspace> <path> <return-dir>"
                                .into(),
                        );
                    }
                    let vars = read_var_records()?;
                    sessions.push(
                        &id,
                        ActivationFrame {
                            workspace,
                            workspace_path,
                            return_dir,
                            previous_vars: vars,
                        },
                    )?;
                    Ok(0)
                }
                "pop" => {
                    let id = required_arg(args.next(), "session id")?;
                    if args.next().is_some() {
                        return Err("usage: xws-picker session pop <id>".into());
                    }
                    match sessions.pop(&id)? {
                        Some((frame, parent)) => {
                            println!(
                                "XWS/1\tpopped\t{}\t{}\t{}",
                                encode(&frame.workspace),
                                encode(&frame.workspace_path),
                                encode(&frame.return_dir)
                            );
                            for (name, state, value) in frame.previous_vars {
                                match value {
                                    Some(value) => {
                                        println!("XWS/1\tvar\t{name}\t{state}\t{}", encode(&value))
                                    }
                                    None => println!("XWS/1\tvar\t{name}\tunset\t-"),
                                }
                            }
                            if let Some(parent) = parent {
                                println!(
                                    "XWS/1\tparent\t{}\t{}",
                                    encode(&parent.workspace),
                                    encode(&parent.workspace_path)
                                );
                            } else {
                                println!("XWS/1\tempty");
                            }
                            Ok(0)
                        }
                        None => Err("xws: session stack is empty".into()),
                    }
                }
                "peek" => {
                    let id = required_arg(args.next(), "session id")?;
                    if args.next().is_some() {
                        return Err("usage: xws-picker session peek <id>".into());
                    }
                    let frame = sessions.top(&id)?.ok_or("xws: session stack is empty")?;
                    println!(
                        "XWS/1\ttop\t{}\t{}\t{}",
                        encode(&frame.workspace),
                        encode(&frame.workspace_path),
                        encode(&frame.return_dir)
                    );
                    Ok(0)
                }
                "contains" => {
                    let id = required_arg(args.next(), "session id")?;
                    let workspace = required_arg(args.next(), "workspace name")?;
                    if args.next().is_some() {
                        return Err("usage: xws-picker session contains <id> <workspace>".into());
                    }
                    let kind = if sessions.contains(&id, &workspace)? {
                        "contains"
                    } else {
                        "absent"
                    };
                    println!("XWS/1\t{kind}");
                    Ok(0)
                }
                _ => Err("usage: xws-picker session new|push|pop|peek|contains ...".into()),
            }
        }
        "create" => {
            let mut profiles = Vec::new();
            let mut features = Vec::new();
            let mut name = None;
            while let Some(arg) = args.next() {
                if arg == "--profile" {
                    profiles.push(required_arg(args.next(), "profile name")?);
                } else if arg == "--with-feature" {
                    features.push(required_arg(args.next(), "feature name")?);
                } else if arg.starts_with('-') {
                    return Err(format!("unknown create option: {arg}").into());
                } else if name.replace(arg).is_some() {
                    return Err("usage: xws-picker create [--profile <profile>]... [--with-feature <feature>]... <workspace>".into());
                }
            }
            let name = required_arg(name, "workspace name")?;
            store.create(&name, &profiles, &features)?;
            println!("Created workspace: {name}");
            Ok(0)
        }
        "remove" | "rm" => {
            let names: Vec<String> = args.collect();
            if names.is_empty() {
                return Err("usage: xws-picker remove <workspace ...>".into());
            }
            let failed = store.remove_many(&names);
            if failed { Ok(1) } else { Ok(0) }
        }
        "move" | "mv" => {
            let source = required_arg(args.next(), "source workspace")?;
            let target = required_arg(args.next(), "target workspace")?;
            if args.next().is_some() {
                return Err("usage: xws-picker move <source> <target>".into());
            }
            store.move_workspace(&source, &target)?;
            println!("Moved workspace {source} to {target}");
            Ok(0)
        }
        _ => Err(format!("unknown command: {command}; run 'xws-picker help'").into()),
    }
}

fn encode(value: &str) -> String {
    if value.is_empty() {
        "-".to_owned()
    } else {
        STANDARD.encode(value.as_bytes())
    }
}

fn decode(value: &str) -> Result<String, Box<dyn Error>> {
    if value == "-" {
        return Ok(String::new());
    }
    Ok(String::from_utf8(STANDARD.decode(value)?)?)
}

fn read_var_records() -> Result<Vec<VariableRecord>, Box<dyn Error>> {
    use std::io::{self, BufRead};
    let mut vars = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in io::stdin().lock().lines() {
        let line = line?;
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() != 5 || fields[0] != "XWS/1" || fields[1] != "var" {
            return Err("invalid XWS/1 session input record".into());
        }
        workspace::validate_environment_name(fields[2])?;
        if !seen.insert(fields[2].to_owned()) {
            return Err(format!("duplicate XWS/1 variable record '{}'", fields[2]).into());
        }
        let value = match fields[3] {
            "set-exported" | "set-local" => Some(decode(fields[4])?),
            "unset" if fields[4] == "-" => None,
            _ => return Err("invalid XWS/1 variable state".into()),
        };
        vars.push((fields[2].to_owned(), fields[3].to_owned(), value));
    }
    Ok(vars)
}

fn required_arg(value: Option<String>, description: &str) -> Result<String, Box<dyn Error>> {
    value.ok_or_else(|| format!("missing {description}").into())
}

fn print_help() {
    println!(
        "Usage: xws <command> [arguments]\n\
\n\
The shell function handles activation in the current shell. Available commands:\n\
  xws                          Pick and activate a workspace\n\
  xws push|activate|use <workspace>    Activate and push a workspace\n\
  xws create [--profile NAME]... [--with-feature NAME]... <workspace>\n\
  xws profiles                 List available profiles\n\
  xws features                 List available features\n\
  xws ls | list                List workspaces\n\
  xws rm <workspace ...>       Remove workspaces\n\
  xws mv <source> <target>     Rename a workspace\n\
  xws which <workspace>        Print a workspace's bin directory\n\
  xws pop|deactivate           Pop one activation frame\n\
  xws version | --version      Print the xws version\n\
See README.md and docs/GETTING_STARTED.md for shell setup and usage."
    );
}

fn print_version() {
    println!("xws {}", env!("CARGO_PKG_VERSION"));
}
