use std::collections::{BTreeMap, HashSet};
use std::env;
use std::error::Error;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const LARGE_MIGRATION_THRESHOLD: u128 = 10 * 1024 * 1024 * 1024;

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

struct WorkspacePlan {
    name: String,
    source_root: PathBuf,
    source_src: PathBuf,
    destination: PathBuf,
    entries: Vec<PathBuf>,
    estimated_bytes: u128,
    detections: BTreeMap<&'static str, Vec<String>>,
    profiles: Vec<&'static str>,
    features: Vec<&'static str>,
    resume: bool,
    already_migrated: bool,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("workspace-migrate: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let mut interactive = false;
    let mut excludes = HashSet::new();
    let mut source_prefix = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--interactive" => interactive = true,
            "--exclude" => {
                let name = args.next().ok_or("--exclude requires a workspace name")?;
                validate_name(&name)?;
                excludes.insert(name);
            }
            "--prefix" => {
                let prefix = args.next().ok_or("--prefix requires a source prefix")?;
                if source_prefix.replace(PathBuf::from(prefix)).is_some() {
                    return Err("--prefix may only be specified once".into());
                }
            }
            "-h" | "--help" => {
                println!(
                    "Usage: workspace-migrate --prefix SOURCE_PREFIX [--interactive] [--exclude NAME]...\n\
                     Migrates immediate child directories containing src/.\n\
                     Batch mode confirms the complete migration once; --interactive prompts per workspace."
                );
                return Ok(());
            }
            option => return Err(format!("unknown option: {option}").into()),
        }
    }

    let source_prefix = source_prefix.ok_or("--prefix is required")?;
    let source_prefix = fs::canonicalize(&source_prefix)?;
    if !source_prefix.is_dir() {
        return Err(format!(
            "source prefix is not a directory: {}",
            source_prefix.display()
        )
        .into());
    }

    let mut plans = Vec::new();
    let destination_prefix = xws_prefix()?;
    let mut source_workspaces = fs::read_dir(&source_prefix)?.collect::<Result<Vec<_>, _>>()?;
    source_workspaces.sort_by_key(|entry| entry.file_name());
    for entry in source_workspaces {
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|name| format!("workspace name is not valid UTF-8: {name:?}"))?;
        validate_name(&name)?;
        if excludes.contains(&name) || !entry.path().join("src").is_dir() {
            continue;
        }
        plans.push(plan_workspace(name, entry.path(), &destination_prefix)?);
    }

    if plans.is_empty() {
        println!(
            "No non-excluded source workspaces with src/ found in {}.",
            source_prefix.display()
        );
        return Ok(());
    }

    ensure_rsync_available()?;
    print_preview(&plans)?;
    let pending_count = plans.iter().filter(|plan| !plan.already_migrated).count();
    if pending_count == 0 {
        for plan in &plans {
            println!(
                "Already migrated, source matches destination: {}",
                plan.name
            );
        }
        println!("All source workspaces are already migrated; no changes were made.");
        return Ok(());
    }

    if !interactive && !confirm(&format!("Migrate all {pending_count} workspaces?"))? {
        println!("Migration cancelled; no workspaces were changed.");
        return Ok(());
    }

    for plan in &plans {
        if plan.already_migrated {
            println!(
                "Already migrated, source matches destination: {}",
                plan.name
            );
            continue;
        }
        if interactive && !confirm(&format!("Migrate workspace '{}'?", plan.name))? {
            println!("Skipped workspace: {}", plan.name);
            continue;
        }
        migrate_workspace(plan)?;
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<(), Box<dyn Error>> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.chars().any(char::is_whitespace)
        || name.chars().any(char::is_control)
    {
        return Err(format!("unsupported workspace name: {name:?}").into());
    }
    Ok(())
}

fn xws_prefix() -> Result<PathBuf, Box<dyn Error>> {
    if let Some(prefix) = env::var_os("XWS_PREFIX") {
        return Ok(PathBuf::from(prefix));
    }
    Ok(PathBuf::from(env::var_os("HOME").ok_or("HOME is not set")?).join("xws"))
}

fn plan_workspace(
    name: String,
    source_root: PathBuf,
    prefix: &Path,
) -> Result<WorkspacePlan, Box<dyn Error>> {
    let source_root = fs::canonicalize(source_root)?;
    let source_src = source_root.join("src");
    if !source_src.is_dir() {
        return Err(format!(
            "source workspace '{name}' has no src/ directory: {}",
            source_src.display()
        )
        .into());
    }

    let destination = prefix.join("workspaces").join(&name);
    let marker = destination.join(".xws-migration-ws");
    let mut already_migrated = false;
    let resume = match fs::symlink_metadata(&destination) {
        Ok(_) => {
            if fs::symlink_metadata(&destination)?.file_type().is_symlink() || marker.is_symlink() {
                return Err(format!("xws workspace already exists: {name}").into());
            }
            if marker.is_file() {
                let marker_source = fs::read_to_string(&marker)?;
                if marker_source.trim_end_matches(['\n', '\r']) != source_src.to_string_lossy() {
                    return Err(format!("xws workspace already exists: {name}").into());
                }
                if !valid_xws_destination(&destination) {
                    return Err(format!("incomplete xws migration destination: {name}").into());
                }
                true
            } else if valid_xws_destination(&destination)
                && destination_src_matches_source(&source_src, &destination.join("src"))?
            {
                already_migrated = true;
                false
            } else {
                return Err(format!("xws workspace already exists: {name}").into());
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };

    let mut source_entries = fs::read_dir(&source_src)?.collect::<Result<Vec<_>, _>>()?;
    source_entries.sort_by_key(|entry| entry.file_name());
    let mut entries = Vec::new();
    let mut estimated_bytes = 0_u128;
    let mut detections = BTreeMap::new();
    let mut has_direct_files = false;
    for entry in source_entries {
        let path = entry.path();
        let entry_name = entry.file_name();
        estimated_bytes = estimated_bytes
            .checked_add(copyable_entry_size(&path)?)
            .ok_or("source size exceeds supported range")?;
        entries.push(path.clone());
        if entry.file_type()?.is_dir() {
            detect_at(&path, &entry_name.to_string_lossy(), &mut detections)?;
        } else {
            has_direct_files = true;
        }
    }
    if has_direct_files {
        detect_at(&source_src, "src", &mut detections)?;
    }

    let profiles = ["java", "python", "rust"]
        .into_iter()
        .filter(|profile| {
            detections.contains_key(match *profile {
                "java" => "Java",
                "python" => "Python",
                _ => "Rust",
            })
        })
        .collect::<Vec<_>>();
    let features = ["golang"]
        .into_iter()
        .filter(|_| detections.contains_key("Golang"))
        .collect::<Vec<_>>();

    if profiles.contains(&"python") && !command_exists("python3") {
        return Err(
            format!("Python profile requires python3 before migrating workspace '{name}'").into(),
        );
    }
    if !already_migrated {
        for entry in &entries {
            let destination_entry = destination.join("src").join(
                entry
                    .file_name()
                    .ok_or("source entry does not have a filename")?,
            );
            match fs::symlink_metadata(&destination_entry) {
                Ok(_) if resume && entries_match(entry, &destination_entry)? => {}
                Ok(_) => {
                    return Err(format!(
                        "destination collision for {}; source remains at {}",
                        destination_entry.display(),
                        entry.display()
                    )
                    .into());
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
    }

    Ok(WorkspacePlan {
        name,
        source_root,
        source_src,
        destination,
        entries,
        estimated_bytes,
        detections,
        profiles,
        features,
        resume,
        already_migrated,
    })
}

fn valid_xws_destination(destination: &Path) -> bool {
    is_real_directory(&destination.join("src"))
        && destination.join("bin/activate").is_file()
        && !fs::symlink_metadata(destination.join("bin/activate"))
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
}

fn destination_src_matches_source(
    source_src: &Path,
    destination_src: &Path,
) -> Result<bool, Box<dyn Error>> {
    let mut source_entries = fs::read_dir(source_src)?.collect::<Result<Vec<_>, _>>()?;
    let mut destination_entries = fs::read_dir(destination_src)?.collect::<Result<Vec<_>, _>>()?;
    source_entries.sort_by_key(|entry| entry.file_name());
    destination_entries.sort_by_key(|entry| entry.file_name());
    if source_entries.is_empty() || source_entries.len() != destination_entries.len() {
        return Ok(false);
    }
    for (source, destination) in source_entries.iter().zip(&destination_entries) {
        if source.file_name() != destination.file_name()
            || !entries_match_after_completed_migration(&source.path(), &destination.path())?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn detect_at(
    root: &Path,
    label: &str,
    detections: &mut BTreeMap<&'static str, Vec<String>>,
) -> Result<(), Box<dyn Error>> {
    for (language, marker) in [
        ("Java", "pom.xml"),
        ("Golang", "go.mod"),
        ("Python", "pyproject.toml"),
        ("Python", "requirements.txt"),
        ("Python", "setup.cfg"),
        ("Rust", "Cargo.toml"),
    ] {
        if root.join(marker).is_file() {
            detections
                .entry(language)
                .or_default()
                .push(format!("{label}/{marker}"));
        }
    }
    let makefile = root.join("Makefile");
    if makefile.is_file() {
        let contents = fs::read_to_string(&makefile)?;
        for (language, command) in [("Java", "javac"), ("Golang", "go")] {
            if makefile_invokes(&contents, command) {
                detections
                    .entry(language)
                    .or_default()
                    .push(format!("{label}/Makefile (invokes {command})"));
            }
        }
    }
    Ok(())
}

fn makefile_invokes(contents: &str, executable: &str) -> bool {
    contents.lines().any(|line| {
        let Some(recipe) = line.strip_prefix('\t') else {
            return false;
        };
        let recipe = recipe.split('#').next().unwrap_or("");
        recipe.split([';', '|', '&']).any(|segment| {
            let segment = segment.trim().trim_start_matches(['@', '-']).trim();
            segment
                .split_whitespace()
                .next()
                .is_some_and(|command| command == executable)
        })
    })
}

fn command_exists(command: &str) -> bool {
    env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| env::split_paths(&paths).collect::<Vec<_>>())
        .any(|directory| {
            let path = directory.join(command);
            #[cfg(unix)]
            {
                fs::metadata(path)
                    .is_ok_and(|metadata| metadata.is_file() && metadata.mode() & 0o111 != 0)
            }
            #[cfg(not(unix))]
            {
                path.is_file()
            }
        })
}

fn print_preview(plans: &[WorkspacePlan]) -> Result<(), Box<dyn Error>> {
    let total_bytes = plans.iter().try_fold(0_u128, |total, plan| {
        total
            .checked_add(plan.estimated_bytes)
            .ok_or("total source size exceeds supported range")
    })?;
    println!("ws migration preview:");
    for plan in plans {
        println!(
            "\n{}:\n  source: {}\n  destination: {}\n  estimated src size: {}",
            plan.name,
            plan.source_root.display(),
            plan.destination.display(),
            format_size(plan.estimated_bytes)
        );
        if plan.entries.is_empty() {
            println!("  src entries to rsync: (none)");
        } else {
            println!("  src entries to rsync:");
            for entry in &plan.entries {
                println!(
                    "    {}",
                    entry.file_name().unwrap_or_default().to_string_lossy()
                );
            }
        }
        if plan.detections.is_empty() {
            println!("  detected project types: (none)");
        } else {
            println!("  detected project types:");
            for (language, markers) in &plan.detections {
                println!("    {language}: {}", markers.join(", "));
            }
        }
        println!(
            "  profiles: {}",
            if plan.profiles.is_empty() {
                "(default only)".to_owned()
            } else {
                plan.profiles.join(", ")
            }
        );
        println!(
            "  features: {}",
            if plan.features.is_empty() {
                "(none)".to_owned()
            } else {
                plan.features.join(", ")
            }
        );
        if plan.resume {
            println!("  status: resumable incomplete migration");
        }
    }
    println!("\nEstimated total src size: {}", format_size(total_bytes));
    let large_plans = plans
        .iter()
        .filter(|plan| plan.estimated_bytes >= LARGE_MIGRATION_THRESHOLD)
        .collect::<Vec<_>>();
    if !large_plans.is_empty() {
        println!(
            "Large migration(s) detected (at least {}):",
            format_size(LARGE_MIGRATION_THRESHOLD)
        );
        for plan in large_plans {
            println!("  {}: {}", plan.name, format_size(plan.estimated_bytes));
        }
    }
    Ok(())
}

fn format_size(bytes: u128) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn confirm(prompt: &str) -> Result<bool, Box<dyn Error>> {
    print!("{prompt} [y/N] ");
    io::stdout().flush()?;
    let mut response = String::new();
    if io::stdin().read_line(&mut response)? == 0 {
        return Ok(false);
    }
    Ok(matches!(
        response.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn migrate_workspace(plan: &WorkspacePlan) -> Result<(), Box<dyn Error>> {
    if !plan.resume {
        create_xws_workspace(plan)?;
        fs::write(
            plan.destination.join(".xws-migration-ws"),
            plan.source_src.as_os_str().as_encoded_bytes(),
        )?;
    }

    let destination_src = plan.destination.join("src");
    if !is_real_directory(&destination_src) || !plan.destination.join("bin/activate").is_file() {
        return Err(format!(
            "destination workspace is incomplete: {}",
            plan.destination.display()
        )
        .into());
    }

    let staging = plan.destination.join(".xws-migration-ws-staging");
    if !plan.entries.is_empty() {
        ensure_migration_directory(&staging)?;
        let payload = staging.join("payload");
        ensure_migration_directory(&payload)?;
        ensure_directory_writable(&payload)?;
        for entry in &plan.entries {
            let destination = destination_src.join(
                entry
                    .file_name()
                    .ok_or("source entry does not have a filename")?,
            );
            match fs::symlink_metadata(&destination) {
                Ok(_) if plan.resume && entries_match(entry, &destination)? => {}
                Ok(_) => {
                    return Err(format!(
                        "destination collision for {}; source remains in {}",
                        destination.display(),
                        plan.source_src.display()
                    )
                    .into());
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }

        let source_contents = plan.source_src.join(".");
        let status = Command::new("rsync")
            .args(["-rlpt", "--partial", "--inplace", "--delete", "--"])
            .arg(source_contents)
            .arg(&payload)
            .status()?;
        if !status.success() {
            return Err(format!(
                "rsync failed; legacy source remains intact and staged data is retained at {}",
                staging.display()
            )
            .into());
        }

        for entry in &plan.entries {
            let staged_entry = payload.join(
                entry
                    .file_name()
                    .ok_or("source entry does not have a filename")?,
            );
            if fs::symlink_metadata(&staged_entry).is_err() {
                return Err(format!(
                    "rsync completed without producing staged entry {}; legacy source remains intact",
                    staged_entry.display()
                )
                .into());
            }
        }

        ensure_directory_writable(&payload)?;
        for entry in &plan.entries {
            let name = entry
                .file_name()
                .ok_or("source entry does not have a filename")?;
            let staged_entry = payload.join(name);
            let destination = destination_src.join(name);
            match fs::symlink_metadata(&destination) {
                Ok(_) if plan.resume && entries_match(entry, &destination)? => {
                    remove_entry_if_exists(&staged_entry)?;
                    println!(
                        "Already copied {} -> {}",
                        entry.display(),
                        destination.display()
                    );
                }
                Ok(_) => {
                    return Err(format!(
                        "destination collision for {}; source remains at {}",
                        destination.display(),
                        plan.source_src.display()
                    )
                    .into());
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    fs::rename(&staged_entry, &destination).map_err(|error| {
                        format!(
                            "could not publish staged entry {} at {}: {error}; legacy source remains intact and staged data is retained",
                            entry.display(),
                            destination.display()
                        )
                    })?;
                    println!("Copied {} -> {}", entry.display(), destination.display());
                }
                Err(error) => return Err(error.into()),
            }
        }
        fs::remove_dir_all(&staging)?;
    }

    fs::remove_file(plan.destination.join(".xws-migration-ws"))?;
    println!("Migrated workspace: {}", plan.name);
    Ok(())
}

fn create_xws_workspace(plan: &WorkspacePlan) -> Result<(), Box<dyn Error>> {
    let init = env::var_os("XWS_MIGRATE_INIT")
        .ok_or("migration shell adapter did not set XWS_MIGRATE_INIT")?;
    if !Path::new(&init).is_file() {
        return Err(format!(
            "xws shell adapter not found: {}",
            Path::new(&init).display()
        )
        .into());
    }

    let script = "source \"$XWS_MIGRATE_INIT\" || exit; xws create \"$@\"";
    let mut command = Command::new("bash");
    command
        .arg("-c")
        .arg(script)
        .arg("workspace-migrate-create")
        .env("XWS_MIGRATE_INIT", init);
    for profile in &plan.profiles {
        command.arg("--profile").arg(profile);
    }
    for feature in &plan.features {
        command.arg("--with-feature").arg(feature);
    }
    let status = command.arg(&plan.name).status()?;
    if !status.success() {
        return Err(format!(
            "xws create failed for '{}'; legacy source remains at {}",
            plan.name,
            plan.source_src.display()
        )
        .into());
    }
    Ok(())
}

fn ensure_rsync_available() -> Result<(), Box<dyn Error>> {
    let output = Command::new("rsync")
        .arg("--version")
        .output()
        .map_err(|error| {
            format!("rsync is required for ws migration but could not be started: {error}")
        })?;
    if !output.status.success() {
        return Err(format!(
            "rsync is required for ws migration; `rsync --version` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(())
}

fn is_real_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_dir())
}

fn ensure_migration_directory(path: &Path) -> Result<(), Box<dyn Error>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(()),
        Ok(_) => Err(format!(
            "migration staging path is not a directory: {}",
            path.display()
        )
        .into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(path)?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

fn ensure_directory_writable(path: &Path) -> Result<(), Box<dyn Error>> {
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

fn copyable_entry_size(path: &Path) -> Result<u128, Box<dyn Error>> {
    let metadata = fs::symlink_metadata(path)?;
    let file_type = metadata.file_type();
    if file_type.is_file() || file_type.is_symlink() {
        return Ok(metadata.len() as u128);
    }
    if !file_type.is_dir() {
        return Err(format!("unsupported source entry type: {}", path.display()).into());
    }
    let mut total = 0_u128;
    for child in fs::read_dir(path)? {
        total = total
            .checked_add(copyable_entry_size(&child?.path())?)
            .ok_or("source size exceeds supported range")?;
    }
    Ok(total)
}

fn entries_match(left: &Path, right: &Path) -> Result<bool, Box<dyn Error>> {
    entries_match_with_migration_metadata_policy(left, right, false)
}

fn entries_match_after_completed_migration(
    left: &Path,
    right: &Path,
) -> Result<bool, Box<dyn Error>> {
    entries_match_with_migration_metadata_policy(left, right, true)
}

fn entries_match_with_migration_metadata_policy(
    left: &Path,
    right: &Path,
    ignore_vscode_merge_base: bool,
) -> Result<bool, Box<dyn Error>> {
    let left_metadata = fs::symlink_metadata(left)?;
    let right_metadata = fs::symlink_metadata(right)?;
    let left_type = left_metadata.file_type();
    let right_type = right_metadata.file_type();
    if left_type.is_symlink() || right_type.is_symlink() {
        return Ok(left_type.is_symlink()
            && right_type.is_symlink()
            && fs::read_link(left)? == fs::read_link(right)?);
    }
    if left_type.is_file() || right_type.is_file() {
        if !left_type.is_file() || !right_type.is_file() {
            return Ok(false);
        }
        let matches =
            if ignore_vscode_merge_base && is_git_config_file(left) && is_git_config_file(right) {
                files_match_ignoring_vscode_merge_base(left, right)?
            } else {
                files_match(left, right)?
            };
        return Ok(matches);
    }
    if !left_type.is_dir() || !right_type.is_dir() {
        return Ok(false);
    }
    #[cfg(unix)]
    if left_metadata.mode() & 0o7777 != right_metadata.mode() & 0o7777 {
        return Ok(false);
    }
    let mut left_children = fs::read_dir(left)?.collect::<Result<Vec<_>, _>>()?;
    let mut right_children = fs::read_dir(right)?.collect::<Result<Vec<_>, _>>()?;
    left_children.sort_by_key(|child| child.file_name());
    right_children.sort_by_key(|child| child.file_name());
    if left_children.len() != right_children.len() {
        return Ok(false);
    }
    for (left_child, right_child) in left_children.iter().zip(&right_children) {
        if left_child.file_name() != right_child.file_name()
            || !entries_match_with_migration_metadata_policy(
                &left_child.path(),
                &right_child.path(),
                ignore_vscode_merge_base,
            )?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn is_git_config_file(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "config")
        && path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == ".git")
}

fn files_match_ignoring_vscode_merge_base(
    left: &Path,
    right: &Path,
) -> Result<bool, Box<dyn Error>> {
    let left_contents = fs::read(left)?;
    let right_contents = fs::read(right)?;
    Ok(without_vscode_merge_base(&left_contents) == without_vscode_merge_base(&right_contents))
}

fn without_vscode_merge_base(contents: &[u8]) -> Vec<u8> {
    let mut filtered = Vec::with_capacity(contents.len());
    let mut in_branch_section = false;
    for line in contents.split_inclusive(|byte| *byte == b'\n') {
        let trimmed = trim_ascii_whitespace(line);
        if trimmed.starts_with(b"[") {
            in_branch_section = is_branch_section_header(trimmed);
        } else if in_branch_section && is_vscode_merge_base_assignment(trimmed) {
            continue;
        }
        filtered.extend_from_slice(line);
    }
    filtered
}

fn is_branch_section_header(line: &[u8]) -> bool {
    let Some(close) = line.iter().rposition(|byte| *byte == b']') else {
        return false;
    };
    let trailing = trim_ascii_whitespace(&line[close + 1..]);
    if !trailing.is_empty() && !trailing.starts_with(b"#") && !trailing.starts_with(b";") {
        return false;
    }
    let section = trim_ascii_whitespace(&line[1..close]);
    let name_end = section
        .iter()
        .position(|byte| byte.is_ascii_whitespace() || *byte == b'.')
        .unwrap_or(section.len());
    section[..name_end].eq_ignore_ascii_case(b"branch")
}

fn is_vscode_merge_base_assignment(line: &[u8]) -> bool {
    let Some(equals) = line.iter().position(|byte| *byte == b'=') else {
        return false;
    };
    trim_ascii_whitespace(&line[..equals]).eq_ignore_ascii_case(b"vscode-merge-base")
}

fn trim_ascii_whitespace(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn files_match(left: &Path, right: &Path) -> Result<bool, Box<dyn Error>> {
    if fs::metadata(left)?.len() != fs::metadata(right)?.len() {
        return Ok(false);
    }
    let mut left = fs::File::open(left)?;
    let mut right = fs::File::open(right)?;
    let mut left_buffer = [0; 64 * 1024];
    let mut right_buffer = [0; 64 * 1024];
    loop {
        let left_count = left.read(&mut left_buffer)?;
        let right_count = right.read(&mut right_buffer)?;
        if left_count != right_count || left_buffer[..left_count] != right_buffer[..right_count] {
            return Ok(false);
        }
        if left_count == 0 {
            return Ok(true);
        }
    }
}

fn remove_entry_if_exists(path: &Path) -> Result<(), Box<dyn Error>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => fs::remove_dir_all(path)?,
        Ok(_) => fs::remove_file(path)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makefile_detection_only_matches_recipe_command_positions() {
        assert!(makefile_invokes("\tjavac Main.java\n", "javac"));
        assert!(makefile_invokes("\t@go build ./...\n", "go"));
        assert!(makefile_invokes("\techo ok && go build\n", "go"));
        assert!(!makefile_invokes("JAVAC = javac\n", "javac"));
        assert!(!makefile_invokes("#\tgo build\n", "go"));
        assert!(!makefile_invokes("\techo go\n", "go"));
    }

    #[test]
    fn detects_multiple_project_markers_at_only_the_given_root() {
        let temp = std::env::temp_dir().join(format!(
            "workspace-migrate-detect-{}-{}",
            std::process::id(),
            unique_suffix()
        ));
        fs::create_dir_all(temp.join("nested")).unwrap();
        fs::write(temp.join("Cargo.toml"), "").unwrap();
        fs::write(temp.join("requirements.txt"), "").unwrap();
        fs::write(temp.join("nested/pom.xml"), "").unwrap();
        let mut detections = BTreeMap::new();
        detect_at(&temp, "checkout", &mut detections).unwrap();
        assert_eq!(detections["Rust"], ["checkout/Cargo.toml"]);
        assert_eq!(detections["Python"], ["checkout/requirements.txt"]);
        assert!(!detections.contains_key("Java"));
        fs::remove_dir_all(temp).unwrap();
    }

    fn unique_suffix() -> String {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_string()
    }
}
