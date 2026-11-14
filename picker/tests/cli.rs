use std::io::Write;
use std::process::{Command, Output, Stdio};

use tempfile::tempdir;

fn picker(prefix: &std::path::Path, state_home: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_xws-picker"));
    command
        .env("XWS_PREFIX", prefix)
        .env("XWS_STATE_HOME", state_home);
    command
}

fn stage_install_tree(root: &std::path::Path) -> std::path::PathBuf {
    let prefix = root.join("install");
    let bin = prefix.join("bin");
    let share = prefix.join("share/xws");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(share.join("script")).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_xws-picker"), bin.join("xws-picker")).unwrap();
    prefix
}

fn successful(output: Output) -> String {
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn failed(output: Output, expected_message: &str) {
    assert!(!output.status.success(), "command unexpectedly succeeded");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(expected_message),
        "expected stderr to contain {expected_message:?}, got {stderr:?}"
    );
}

#[test]
fn templates_dir_override_is_used_as_is_and_invalid_values_are_errors() {
    let temp = tempdir().unwrap();
    let prefix = temp.path().join("prefix");
    let state_home = temp.path().join("state");
    let repo_templates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../templates");

    let profiles = successful(
        picker(&prefix, &state_home)
            .arg("profiles")
            .env("XWS_TEMPLATES_DIR", &repo_templates)
            .output()
            .unwrap(),
    );
    assert!(profiles.lines().any(|line| line == "default"));

    let empty = temp.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let none = successful(
        picker(&prefix, &state_home)
            .arg("profiles")
            .env("XWS_TEMPLATES_DIR", &empty)
            .output()
            .unwrap(),
    );
    assert!(none.trim().is_empty());

    failed(
        picker(&prefix, &state_home)
            .arg("profiles")
            .env("XWS_TEMPLATES_DIR", temp.path().join("missing"))
            .output()
            .unwrap(),
        "XWS_TEMPLATES_DIR is not a directory",
    );
}

#[test]
fn create_list_path_and_environment_protocol_work_end_to_end() {
    let temp = tempdir().unwrap();
    let prefix = temp.path().join("prefix");
    let state_home = temp.path().join("state");

    successful(
        picker(&prefix, &state_home)
            .args(["create", "demo"])
            .output()
            .unwrap(),
    );
    let listed = successful(picker(&prefix, &state_home).arg("list").output().unwrap());
    assert_eq!(listed, "demo\n");
    let active_list = successful(
        picker(&prefix, &state_home)
            .arg("list")
            .env("XWS_WORKSPACE", "demo")
            .output()
            .unwrap(),
    );
    assert_eq!(active_list, "demo *\n");
    failed(
        picker(&prefix, &state_home)
            .args(["list", "demo"])
            .output()
            .unwrap(),
        "unexpected argument 'demo",
    );

    let workspace_path = prefix.join("workspaces/demo");
    let path = successful(
        picker(&prefix, &state_home)
            .args(["path", "demo"])
            .output()
            .unwrap(),
    );
    assert_eq!(path.trim(), workspace_path.to_str().unwrap());

    std::fs::write(
        workspace_path.join("bin/env"),
        "APP_MODE=development\nEMPTY=\n",
    )
    .unwrap();
    let env = successful(
        picker(&prefix, &state_home)
            .args(["protocol", "env", "demo"])
            .output()
            .unwrap(),
    );
    assert_eq!(
        env,
        "XWS/1\tvar\tAPP_MODE\tZGV2ZWxvcG1lbnQ=\nXWS/1\tvar\tEMPTY\t-\n"
    );
}

#[test]
fn create_composes_repeatable_profiles_with_default_hooks() {
    let temp = tempdir().unwrap();
    let prefix = temp.path().join("prefix");
    let state_home = temp.path().join("state");
    let default_templates = prefix.join("templates/profiles/default");
    std::fs::create_dir_all(&default_templates).unwrap();
    std::fs::write(
        default_templates.join("activate"),
        "# custom default activation\n",
    )
    .unwrap();
    std::fs::write(
        default_templates.join("once"),
        "printf 'default\\n' >> \"$XWS_WORKSPACE_PATH/profile-order\"\n",
    )
    .unwrap();

    for profile in ["alpha", "beta"] {
        let templates = prefix.join(format!("templates/profiles/{profile}"));
        std::fs::create_dir_all(&templates).unwrap();
        std::fs::write(
            templates.join("activate"),
            format!("# {profile} activation\n"),
        )
        .unwrap();
        std::fs::write(
            templates.join("deactivate"),
            format!("# {profile} deactivation\n"),
        )
        .unwrap();
        std::fs::write(
            templates.join("once"),
            format!("printf '{profile}\\n' >> \"$XWS_WORKSPACE_PATH/profile-order\"\n"),
        )
        .unwrap();
    }

    successful(
        picker(&prefix, &state_home)
            .args(["create", "--profile", "beta", "--profile", "alpha", "demo"])
            .output()
            .unwrap(),
    );

    let bin = prefix.join("workspaces/demo/bin");
    assert_eq!(
        std::fs::read_to_string(bin.join("activate")).unwrap(),
        "# custom default activation\n"
    );
    assert_eq!(
        std::fs::read_to_string(prefix.join("workspaces/demo/profile-order")).unwrap(),
        "default\nalpha\nbeta\n"
    );
    assert_eq!(
        std::fs::read_to_string(bin.join("activate.d/10-profile-00000000000000000000")).unwrap(),
        "# alpha activation\n"
    );
    assert_eq!(
        std::fs::read_to_string(bin.join("activate.d/10-profile-00000000000000000001")).unwrap(),
        "# beta activation\n"
    );
    assert_eq!(
        std::fs::read_to_string(bin.join("deactivate.d/20-profile-00000000000000000000")).unwrap(),
        "# beta deactivation\n"
    );
    assert_eq!(
        std::fs::read_to_string(bin.join("deactivate.d/20-profile-00000000000000000001")).unwrap(),
        "# alpha deactivation\n"
    );
}

#[test]
fn rust_profile_only_adds_the_backtrace_environment_default() {
    let temp = tempdir().unwrap();
    let prefix = temp.path().join("prefix");
    let state_home = temp.path().join("state");

    successful(
        picker(&prefix, &state_home)
            .args(["create", "--profile", "rust", "rust-demo"])
            .output()
            .unwrap(),
    );

    let bin = prefix.join("workspaces/rust-demo/bin");
    assert_eq!(
        std::fs::read_to_string(bin.join("env")).unwrap(),
        "# NAME=value; literal exported variables\nRUST_BACKTRACE=1\n"
    );
    assert_eq!(
        std::fs::read_dir(bin.join("activate.d")).unwrap().count(),
        0
    );
    assert_eq!(
        std::fs::read_dir(bin.join("deactivate.d")).unwrap().count(),
        0
    );

    let env = successful(
        picker(&prefix, &state_home)
            .args(["protocol", "env", "rust-demo"])
            .output()
            .unwrap(),
    );
    assert_eq!(env, "XWS/1\tvar\tRUST_BACKTRACE\tMQ==\n");
}

#[test]
fn conda_profile_records_an_existing_environment_and_venv_uses_its_python() {
    let temp = tempdir().unwrap();
    let prefix = temp.path().join("prefix");
    let state_home = temp.path().join("state");
    let templates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../templates");

    failed(
        picker(&prefix, &state_home)
            .args(["create", "--profile", "conda", "missing-env"])
            .env("XWS_TEMPLATES_DIR", &templates)
            .output()
            .unwrap(),
        "set CONDA_ENV",
    );

    let fake_bin = temp.path().join("fake-bin");
    std::fs::create_dir(&fake_bin).unwrap();
    let conda = fake_bin.join("conda");
    std::fs::write(
        &conda,
        "#!/usr/bin/env bash\n\
         [[ \"$1\" == run && \"$2\" == --name && \"$3\" == existing-env ]] || exit 2\n\
         [[ \"$4\" == python && \"$5\" == -m && \"$6\" == venv && \"$7\" == .venv ]] || exit 3\n\
         mkdir -p .venv/bin\n\
         printf 'VIRTUAL_ENV=\"$XWS_WORKSPACE_PATH/.venv\"\\ndeactivate() { unset VIRTUAL_ENV; }\\n' > .venv/bin/activate\n",
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&conda).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o755);
    std::fs::set_permissions(&conda, permissions).unwrap();
    let mut path_entries = vec![fake_bin];
    path_entries.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let path = std::env::join_paths(path_entries).unwrap();

    successful(
        picker(&prefix, &state_home)
            .args([
                "create",
                "--profile",
                "conda",
                "--with-feature",
                "venv",
                "isolated",
            ])
            .env("XWS_TEMPLATES_DIR", &templates)
            .env("CONDA_ENV", "existing-env")
            .env("PATH", path)
            .output()
            .unwrap(),
    );

    let workspace = prefix.join("workspaces/isolated");
    assert_eq!(
        std::fs::read_to_string(workspace.join(".conda-env")).unwrap(),
        "existing-env\n"
    );
    assert!(workspace.join(".venv/bin/activate").is_file());
    assert!(
        workspace
            .join("bin/activate.d/10-profile-00000000000000000000")
            .is_file()
    );
    assert!(workspace.join("bin/activate.d/20-feature-venv").is_file());
    assert!(workspace.join("bin/deactivate.d/10-feature-venv").is_file());
    assert!(
        workspace
            .join("bin/deactivate.d/20-profile-00000000000000000000")
            .is_file()
    );

    let hooks = Command::new("bash")
        .args([
            "-c",
            r#"
conda() {
  case "$1" in
    activate) CONDA_DEFAULT_ENV="$2"; export CONDA_DEFAULT_ENV ;;
    deactivate) unset CONDA_DEFAULT_ENV ;;
  esac
}
export XWS_WORKSPACE_PATH="$1"
source "$1/bin/activate.d/10-profile-00000000000000000000" || exit 1
source "$1/bin/activate.d/20-feature-venv" || exit 2
[[ "$CONDA_DEFAULT_ENV" == existing-env && "$VIRTUAL_ENV" == "$1/.venv" ]] || exit 3
source "$1/bin/deactivate.d/10-feature-venv" || exit 4
[[ -z "${VIRTUAL_ENV-}" && "$CONDA_DEFAULT_ENV" == existing-env ]] || exit 5
source "$1/bin/deactivate.d/20-profile-00000000000000000000" || exit 6
[[ -z "${CONDA_DEFAULT_ENV-}" ]]
"#,
            "test",
        ])
        .arg(&workspace)
        .status()
        .unwrap();
    assert!(hooks.success(), "generated activation hooks failed");
}

#[test]
fn venv_feature_requires_conda_profile() {
    let temp = tempdir().unwrap();
    let prefix = temp.path().join("prefix");
    let state_home = temp.path().join("state");
    let templates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../templates");

    failed(
        picker(&prefix, &state_home)
            .args(["create", "--with-feature", "venv", "standalone-venv"])
            .env("XWS_TEMPLATES_DIR", templates)
            .output()
            .unwrap(),
        "requires one of these profiles: conda",
    );
    assert!(!prefix.join("workspaces/standalone-venv").exists());
}

#[test]
fn profile_and_feature_bundles_are_discovered_from_their_directories() {
    let temp = tempdir().unwrap();
    let prefix = temp.path().join("prefix");
    let state_home = temp.path().join("state");
    let profile = prefix.join("templates/profiles/haskell");
    std::fs::create_dir_all(&profile).unwrap();
    let feature = prefix.join("templates/features/cabal");
    std::fs::create_dir_all(&feature).unwrap();
    let profile = prefix.join("templates/profiles/haskell");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(
        profile.join("env"),
        "CABAL_HOME=/opt/cabal\nHASKELL_PROFILE=1\n",
    )
    .unwrap();
    std::fs::write(
        feature.join("manifest.toml"),
        "with-profiles = [\"haskell\"]\n",
    )
    .unwrap();
    std::fs::write(feature.join("activate"), ": # cabal\n").unwrap();
    std::fs::write(feature.join("deactivate"), ": # cabal cleanup\n").unwrap();
    std::fs::write(feature.join("once"), "printf 'cabal\\n' >> setup.log\n").unwrap();
    std::fs::write(feature.join("env"), "CABAL_CONFIG=/etc/cabal\n").unwrap();

    assert_eq!(
        successful(
            picker(&prefix, &state_home)
                .arg("profiles")
                .output()
                .unwrap()
        ),
        "conda\ndefault\nhaskell\njava\npython\nrust\n"
    );
    assert_eq!(
        successful(
            picker(&prefix, &state_home)
                .arg("features")
                .output()
                .unwrap()
        ),
        "cabal\ngolang\nvenv\n"
    );

    successful(
        picker(&prefix, &state_home)
            .args([
                "create",
                "--profile",
                "haskell",
                "--with-feature",
                "cabal",
                "cabal-project",
            ])
            .output()
            .unwrap(),
    );
    let bin = prefix.join("workspaces/cabal-project/bin");
    assert!(
        std::fs::read_to_string(bin.join("env"))
            .unwrap()
            .contains("CABAL_HOME=/opt/cabal")
    );
    assert!(
        std::fs::read_to_string(bin.join("env"))
            .unwrap()
            .contains("CABAL_CONFIG=/etc/cabal")
    );
    assert_eq!(
        std::fs::read_to_string(prefix.join("workspaces/cabal-project/setup.log")).unwrap(),
        "cabal\n"
    );
    assert!(bin.join("activate.d/20-feature-cabal").is_file());
}

#[test]
fn feature_profile_restrictions_are_enforced() {
    let temp = tempdir().unwrap();
    let prefix = temp.path().join("prefix");
    let state_home = temp.path().join("state");

    let profile = prefix.join("templates/profiles/haskell");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("env"), "HASKELL_PROFILE=1\n").unwrap();
    let feature = prefix.join("templates/features/cabal");
    std::fs::create_dir_all(&feature).unwrap();
    std::fs::write(
        feature.join("manifest.toml"),
        "with-profiles = [\"haskell\"]\n",
    )
    .unwrap();
    std::fs::write(feature.join("activate"), ": # cabal\n").unwrap();
    std::fs::write(feature.join("deactivate"), ": # cabal cleanup\n").unwrap();

    failed(
        picker(&prefix, &state_home)
            .args(["create", "--with-feature", "cabal", "wrong-profile"])
            .output()
            .unwrap(),
        "requires one of these profiles: haskell",
    );
    assert!(!prefix.join("workspaces/wrong-profile").exists());

    successful(
        picker(&prefix, &state_home)
            .args([
                "create",
                "--profile",
                "haskell",
                "--with-feature",
                "cabal",
                "haskell-cabal",
            ])
            .output()
            .unwrap(),
    );
    assert!(
        prefix
            .join("workspaces/haskell-cabal/bin/activate.d/20-feature-cabal")
            .is_file()
    );
}

#[test]
fn java_profile_warns_and_continues_when_no_jdk_is_available() {
    let temp = tempdir().unwrap();
    let no_java_path = temp.path().join("empty-path");
    std::fs::create_dir(&no_java_path).unwrap();
    let hook = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../templates/profiles/java/activate");
    let output = Command::new("bash")
        .args([
            "-c",
            r#"
uname() { printf 'Linux\n'; }
PATH="$1"
JAVA_HOME=/existing/jdk
export PATH JAVA_HOME
source "$2"
[[ "$JAVA_HOME" == /existing/jdk && "$PATH" == "$1" ]]
"#,
            "test",
            no_java_path.to_str().unwrap(),
            hook.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "activation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("no JDK found; Java environment setup skipped")
    );
}

#[test]
fn session_protocol_round_trips_frames_and_removes_empty_session() {
    let temp = tempdir().unwrap();
    let prefix = temp.path().join("prefix");
    let state_home = temp.path().join("state");
    let response = successful(
        picker(&prefix, &state_home)
            .args(["session", "new"])
            .output()
            .unwrap(),
    );
    let session_id = response.trim().split('\t').nth(2).unwrap();

    let mut push = picker(&prefix, &state_home)
        .args([
            "session",
            "push",
            session_id,
            "demo",
            "/workspaces/demo",
            "/starting",
        ])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    push.stdin
        .take()
        .unwrap()
        .write_all(b"XWS/1\tvar\tMODE\tset-local\tbG9jYWw=\n")
        .unwrap();
    let push_output = push.wait_with_output().unwrap();
    assert!(
        push_output.status.success(),
        "{}",
        String::from_utf8_lossy(&push_output.stderr)
    );

    let peek = successful(
        picker(&prefix, &state_home)
            .args(["session", "peek", session_id])
            .output()
            .unwrap(),
    );
    assert_eq!(
        peek,
        "XWS/1\ttop\tZGVtbw==\tL3dvcmtzcGFjZXMvZGVtbw==\tL3N0YXJ0aW5n\n"
    );

    let popped = successful(
        picker(&prefix, &state_home)
            .args(["session", "pop", session_id])
            .output()
            .unwrap(),
    );
    assert!(popped.contains("XWS/1\tvar\tMODE\tset-local\tbG9jYWw=\n"));
    assert!(popped.ends_with("XWS/1\tempty\n"));
    assert!(
        !state_home
            .join("sessions")
            .join(format!("{session_id}.json"))
            .exists()
    );
}

#[test]
fn malformed_session_input_is_rejected_without_pushing_a_frame() {
    let temp = tempdir().unwrap();
    let prefix = temp.path().join("prefix");
    let state_home = temp.path().join("state");
    let response = successful(
        picker(&prefix, &state_home)
            .args(["session", "new"])
            .output()
            .unwrap(),
    );
    let session_id = response.trim().split('\t').nth(2).unwrap();

    let mut push = picker(&prefix, &state_home)
        .args([
            "session",
            "push",
            session_id,
            "demo",
            "/workspaces/demo",
            "/starting",
        ])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    push.stdin
        .take()
        .unwrap()
        .write_all(b"XWS/1\tvar\tPATH\tset-exported\tL2Jpbg==\n")
        .unwrap();
    let output = push.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("PATH"));

    let top = picker(&prefix, &state_home)
        .args(["session", "peek", session_id])
        .output()
        .unwrap();
    assert!(!top.status.success());
}

#[test]
fn move_and_remove_commands_enforce_workspace_rules() {
    let temp = tempdir().unwrap();
    let prefix = temp.path().join("prefix");
    let state_home = temp.path().join("state");
    for name in ["alpha", "beta"] {
        successful(
            picker(&prefix, &state_home)
                .args(["create", name])
                .output()
                .unwrap(),
        );
    }

    failed(
        picker(&prefix, &state_home)
            .args(["move", "alpha", "beta"])
            .output()
            .unwrap(),
        "target workspace already exists",
    );

    let active_path = prefix.join("workspaces/alpha");
    failed(
        picker(&prefix, &state_home)
            .args(["move", "alpha", "renamed"])
            .env("XWS_WORKSPACE_PATH", &active_path)
            .output()
            .unwrap(),
        "deactivate the source workspace",
    );
    failed(
        picker(&prefix, &state_home)
            .args(["remove", "alpha"])
            .env("XWS_WORKSPACE_PATH", &active_path)
            .output()
            .unwrap(),
        "deactivate the workspace",
    );
    assert!(active_path.is_dir());

    successful(
        picker(&prefix, &state_home)
            .args(["move", "alpha", "renamed"])
            .output()
            .unwrap(),
    );
    assert!(prefix.join("workspaces/renamed").is_dir());
    failed(
        picker(&prefix, &state_home)
            .args(["path", "alpha"])
            .output()
            .unwrap(),
        "does not exist",
    );

    let removed = picker(&prefix, &state_home)
        .args(["remove", "renamed", "missing"])
        .output()
        .unwrap();
    failed(removed, "workspace 'missing' does not exist");
    assert!(!prefix.join("workspaces/renamed").exists());
    assert!(prefix.join("workspaces/beta").is_dir());
}

#[test]
fn remove_and_move_refuse_a_workspace_active_in_another_live_shell() {
    let temp = tempdir().unwrap();
    let prefix = temp.path().join("prefix");
    let state_home = temp.path().join("state");
    successful(
        picker(&prefix, &state_home)
            .args(["create", "alpha"])
            .output()
            .unwrap(),
    );

    // The test process stands in for the other shell, so it is alive.
    let shell_pid = std::process::id().to_string();
    let created = successful(
        picker(&prefix, &state_home)
            .args(["session", "new", "--shell-pid", &shell_pid])
            .output()
            .unwrap(),
    );
    let id = created.trim().rsplit('\t').next().unwrap().to_owned();
    let alpha = prefix.join("workspaces/alpha");
    let mut push = picker(&prefix, &state_home)
        .args(["session", "push", &id, "alpha"])
        .arg(&alpha)
        .arg(temp.path())
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    drop(push.stdin.take());
    assert!(push.wait().unwrap().success());

    for args in [["remove", "alpha"], ["move", "alpha"]] {
        let mut command = picker(&prefix, &state_home);
        command.args(args);
        if args[0] == "move" {
            command.arg("renamed");
        }
        failed(command.output().unwrap(), "is active in shell pid");
    }
    let message = picker(&prefix, &state_home)
        .args(["remove", "alpha"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&message.stderr);
    assert!(stderr.contains("xws deactivate alpha"), "{stderr}");
    assert!(stderr.contains("rm '"), "{stderr}");
    assert!(alpha.is_dir());

    successful(
        picker(&prefix, &state_home)
            .args(["session", "pop", &id])
            .output()
            .unwrap(),
    );
    successful(
        picker(&prefix, &state_home)
            .args(["remove", "alpha"])
            .output()
            .unwrap(),
    );
    assert!(!alpha.exists());
}

#[test]
fn session_new_removes_sessions_of_exited_shells() {
    let temp = tempdir().unwrap();
    let prefix = temp.path().join("prefix");
    let state_home = temp.path().join("state");
    let sessions = state_home.join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    // pid 2147483646 does not exist; the timestamp is after any boot.
    let stale = sessions.join("1-99999999999999999999.json");
    std::fs::write(&stale, r#"{"stack":[],"shell_pid":2147483646}"#).unwrap();
    let kept = sessions.join("1-99999999999999999998.json");
    std::fs::write(&kept, r#"{"stack":[]}"#).unwrap();

    successful(
        picker(&prefix, &state_home)
            .args([
                "session",
                "new",
                "--shell-pid",
                &std::process::id().to_string(),
            ])
            .output()
            .unwrap(),
    );
    assert!(!stale.exists());
    assert!(kept.exists());
}

#[test]
fn command_usage_and_invalid_configuration_return_errors() {
    let temp = tempdir().unwrap();
    let prefix = temp.path().join("prefix");
    let state_home = temp.path().join("state");

    failed(
        picker(&prefix, &state_home)
            .args(["path"])
            .output()
            .unwrap(),
        "the following required arguments were not provided",
    );
    failed(
        picker(&prefix, &state_home)
            .args(["protocol", "unknown"])
            .output()
            .unwrap(),
        "unrecognized subcommand 'unknown'",
    );
    failed(
        picker(&prefix, &state_home)
            .args(["create", "--profile", "unknown", "demo"])
            .output()
            .unwrap(),
        "unknown profile",
    );
    failed(
        picker(&prefix, &state_home)
            .args([
                "create",
                "--profile",
                "python",
                "--profile",
                "python",
                "demo",
            ])
            .output()
            .unwrap(),
        "selected more than once",
    );
    failed(
        picker(&prefix, &state_home)
            .args(["create", "bad/name"])
            .output()
            .unwrap(),
        "invalid workspace name",
    );
    for name in ["-x", ".hidden", "a:b"] {
        failed(
            picker(&prefix, &state_home)
                .args(["create", "--", name])
                .output()
                .unwrap(),
            "invalid workspace name",
        );
    }
}

#[test]
fn list_skips_invalid_directories_with_a_warning() {
    let temp = tempdir().unwrap();
    let prefix = temp.path().join("prefix");
    let state_home = temp.path().join("state");

    successful(
        picker(&prefix, &state_home)
            .args(["create", "demo"])
            .output()
            .unwrap(),
    );
    for bad in ["-x", ".hidden", "a:b"] {
        std::fs::create_dir(prefix.join("workspaces").join(bad)).unwrap();
    }

    let output = picker(&prefix, &state_home).arg("list").output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "demo\n");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stderr.matches("skipping workspace directory").count(), 3);
}

#[test]
fn version_is_available_from_picker_and_shell_function() {
    let temp = tempdir().unwrap();
    let expected = format!("xws {}\n", env!("CARGO_PKG_VERSION"));

    for command in ["version", "--version"] {
        let output = Command::new(env!("CARGO_BIN_EXE_xws-picker"))
            .arg(command)
            .env_remove("HOME")
            .env_remove("XWS_PREFIX")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
    }

    for command in ["help", "--help", "-h"] {
        let output = Command::new(env!("CARGO_BIN_EXE_xws-picker"))
            .arg(command)
            .env_remove("HOME")
            .env_remove("XWS_PREFIX")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(
            String::from_utf8(output.stdout)
                .unwrap()
                .starts_with("Usage: xws <COMMAND>")
        );
    }

    let output = Command::new(env!("CARGO_BIN_EXE_xws-picker"))
        .args(["help", "list"])
        .env_remove("HOME")
        .env_remove("XWS_PREFIX")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("List workspaces")
    );

    let bash_init = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../script/xws.bash");
    for command in ["version", "--version"] {
        let output = Command::new("bash")
            .args(["-c", "source \"$1\" && xws \"$2\"", "bash"])
            .arg(bash_init.as_os_str())
            .arg(command)
            .env("HOME", temp.path())
            .env("XWS_PICKER", env!("CARGO_BIN_EXE_xws-picker"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
    }
}

#[test]
fn installed_binary_loads_templates_relative_to_its_install_prefix() {
    let temp = tempdir().unwrap();
    let prefix = stage_install_tree(temp.path());
    let state_home = temp.path().join("state");
    let templates = prefix.join("share/xws/templates");
    let default = templates.join("profiles/default");
    std::fs::create_dir_all(&default).unwrap();
    std::fs::write(default.join("activate"), "# installed activate\n").unwrap();
    std::fs::write(default.join("deactivate"), "# installed deactivate\n").unwrap();
    std::fs::write(default.join("once"), ": # installed once\n").unwrap();
    let feature = templates.join("features/custom");
    std::fs::create_dir_all(&feature).unwrap();
    std::fs::write(feature.join("manifest.toml"), "").unwrap();
    std::fs::write(feature.join("activate"), "# installed feature\n").unwrap();
    std::fs::write(
        feature.join("deactivate"),
        ": # installed feature cleanup\n",
    )
    .unwrap();

    successful(
        Command::new(prefix.join("bin/xws-picker"))
            .env("XWS_PREFIX", temp.path().join("workspaces"))
            .env("XWS_STATE_HOME", state_home)
            .args(["create", "--with-feature", "custom", "installed"])
            .output()
            .unwrap(),
    );
    let workspace_bin = temp.path().join("workspaces/workspaces/installed/bin");
    assert_eq!(
        std::fs::read_to_string(workspace_bin.join("activate")).unwrap(),
        "# installed activate\n"
    );
    assert_eq!(
        std::fs::read_to_string(workspace_bin.join("activate.d/20-feature-custom")).unwrap(),
        "# installed feature\n"
    );
}

#[cfg(unix)]
#[test]
fn installed_bash_adapter_finds_cli_relative_to_the_share_tree() {
    let temp = tempdir().unwrap();
    let prefix = stage_install_tree(temp.path());
    let share = prefix.join("share/xws");
    std::fs::create_dir_all(share.join("templates/profiles/default")).unwrap();
    for (name, contents) in [
        ("activate", ": # activate\n"),
        ("deactivate", ": # deactivate\n"),
        ("once", ": # once\n"),
    ] {
        std::fs::write(
            share.join("templates/profiles/default").join(name),
            contents,
        )
        .unwrap();
    }
    for name in ["xws.bash", "xws-common.sh"] {
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../script")
                .join(name),
            share.join("script").join(name),
        )
        .unwrap();
    }

    let output = Command::new("bash")
        .args([
            "-c",
            "source \"$XWS_BASH_INIT\" && xws create installed && xws profiles && xws features && xws push installed && xws list && printf '%s\\n' \"$PWD\" && xws pop",
        ])
        .env("XWS_BASH_INIT", share.join("script/xws.bash"))
        .env("XWS_PREFIX", temp.path().join("workspaces"))
        .env("XWS_STATE_HOME", temp.path().join("state"))
        .env_remove("XWS_PICKER")
        .output()
        .unwrap();
    let stdout = successful(output);
    assert!(
        stdout.contains(
            temp.path()
                .join("workspaces/workspaces/installed/src")
                .to_str()
                .unwrap()
        )
    );
    assert!(stdout.contains("installed *\n"));
    assert!(stdout.contains("default\n"));
}

#[test]
fn interactive_picker_reads_keys_and_returns_the_selected_workspace() {
    let temp = tempdir().unwrap();
    let prefix = temp.path().join("prefix");
    let state_home = temp.path().join("state");
    for name in ["alpha", "beta"] {
        successful(
            picker(&prefix, &state_home)
                .args(["create", name])
                .output()
                .unwrap(),
        );
    }

    let alternate = run_interactive_picker(&prefix, &state_home, None);
    assert!(
        alternate
            .windows(b"\x1b[?1049h".len())
            .any(|window| window == b"\x1b[?1049h")
    );
    assert!(
        alternate
            .windows(b"\x1b[?1049l".len())
            .any(|window| window == b"\x1b[?1049l")
    );
    assert_picker_selected_beta(&alternate);

    std::fs::create_dir_all(&state_home).unwrap();
    std::fs::write(
        state_home.join("config.toml"),
        "screen_buffer = \"inline\"\n",
    )
    .unwrap();
    let inline = run_interactive_picker(&prefix, &state_home, None);
    assert!(
        !inline
            .windows(b"\x1b[?1049h".len())
            .any(|window| window == b"\x1b[?1049h")
    );
    assert!(
        !inline
            .windows(b"\x1b[2J".len())
            .any(|window| window == b"\x1b[2J")
    );
    assert_picker_selected_beta(&inline);

    let env_override = run_interactive_picker(&prefix, &state_home, Some("alternate"));
    assert!(
        env_override
            .windows(b"\x1b[?1049h".len())
            .any(|window| window == b"\x1b[?1049h")
    );
}

fn run_interactive_picker(
    prefix: &std::path::Path,
    state_home: &std::path::Path,
    display: Option<&str>,
) -> Vec<u8> {
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    use std::io::Read;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 8,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let (output_tx, output_rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buffer = [0; 256];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    if output_tx.send(buffer[..count].to_vec()).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = output_tx.send(format!("PTY read failed: {error}").into_bytes());
                    break;
                }
            }
        }
    });

    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_xws-picker"));
    command.args(["pick"]);
    command.env("XWS_PREFIX", prefix);
    command.env("XWS_STATE_HOME", state_home);
    if let Some(display) = display {
        command.env("XWS_PICKER_DISPLAY", display);
    } else {
        command.env_remove("XWS_PICKER_DISPLAY");
    }
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let mut terminal_output = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !terminal_output
        .windows(b"Enter select".len())
        .any(|window| window == b"Enter select")
    {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match output_rx.recv_timeout(remaining) {
            Ok(output) => terminal_output.extend_from_slice(&output),
            Err(error) => panic!("picker did not render its prompt: {error}"),
        }
    }

    writer.write_all(b"\x1b[B\r").unwrap();
    let status = loop {
        if child.try_wait().unwrap().is_some() {
            break child.wait().unwrap();
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("picker timed out after receiving selection keys");
        }
        thread::sleep(Duration::from_millis(10));
    };
    assert!(status.success(), "picker exited with {status:?}");

    loop {
        match output_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(output) => terminal_output.extend_from_slice(&output),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                panic!("timed out while collecting picker terminal output")
            }
        }
    }
    terminal_output
}

fn assert_picker_selected_beta(terminal_output: &[u8]) {
    let returned_selection = terminal_output
        .split(|byte| *byte == b'\n')
        .rfind(|line| !line.is_empty())
        .unwrap_or_default();
    assert!(
        returned_selection.ends_with(b"beta\r"),
        "expected picker to return beta as the selection; got {:?}",
        String::from_utf8_lossy(terminal_output)
    );
    assert!(
        terminal_output
            .windows("↑/↓ or j/k move".len())
            .any(|window| window == "↑/↓ or j/k move".as_bytes()),
        "expected picker help text in terminal output"
    );
}
