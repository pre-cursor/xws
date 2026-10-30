use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_TEST_DIR_ID: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ws migration test {} {} {}",
            std::process::id(),
            suffix,
            NEXT_TEST_DIR_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn xws_init(root: &Path) -> PathBuf {
    let path = root.join("xws-init.bash");
    fs::write(
        &path,
        r#"
xws() {
  [[ "$1" == create ]] || return 2
  shift
  printf '%s\n' "$@" >> "$XWS_CREATE_LOG"
  local name=
  while (($#)); do
    case "$1" in
      --profile|--with-feature) shift 2 ;;
      *) name="$1"; shift ;;
    esac
  done
  mkdir -p "$XWS_PREFIX/workspaces/$name/src" "$XWS_PREFIX/workspaces/$name/bin"
  : > "$XWS_PREFIX/workspaces/$name/bin/activate"
}
"#,
    )
    .unwrap();
    path
}

fn run_migration(root: &Path, args: &[&str], input: &str, source_prefix: &Path) -> Output {
    let xws_prefix = root.join("xws");
    let init = xws_init(root);
    let mut command = Command::new(env!("CARGO_BIN_EXE_workspace-migrate"));
    command
        .env("XWS_PREFIX", &xws_prefix)
        .env("XWS_MIGRATE_INIT", init)
        .env("XWS_CREATE_LOG", root.join("xws-create.log"))
        .args(args)
        .arg("--prefix")
        .arg(source_prefix);
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn successful(output: Output) -> String {
    assert!(
        output.status.success(),
        "command failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn failed(output: Output, message: &str) -> String {
    assert!(!output.status.success(), "command unexpectedly succeeded");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains(message),
        "expected stderr to contain {message:?}, got {stderr:?}"
    );
    stderr
}

fn workspace(root: &Path, name: &str) -> PathBuf {
    let path = root.join("legacy").join(name);
    fs::create_dir_all(path.join("bin")).unwrap();
    fs::create_dir_all(path.join("src")).unwrap();
    path
}

#[test]
fn migrates_source_tree_when_legacy_workspace_has_no_bin_directory() {
    let temp = TestDir::new();
    let legacy = temp.path().join("legacy");
    let app = legacy.join("app");
    fs::create_dir_all(app.join("src/project")).unwrap();
    fs::write(app.join("src/project/README"), "source only\n").unwrap();

    successful(run_migration(temp.path(), &[], "yes\n", &legacy));

    assert_eq!(
        fs::read_to_string(temp.path().join("xws/workspaces/app/src/project/README")).unwrap(),
        "source only\n"
    );
    assert!(app.join("src/project/README").is_file());
}

#[test]
fn generic_prefix_discovers_sources_and_only_explicitly_excludes_workspaces() {
    let temp = TestDir::new();
    let app = workspace(temp.path(), "app");
    fs::create_dir_all(app.join("src/java")).unwrap();
    fs::create_dir_all(app.join("src/go")).unwrap();
    fs::create_dir_all(app.join("src/ws")).unwrap();
    fs::write(app.join("src/ws/checkout"), "ordinary project directory\n").unwrap();
    fs::write(app.join("src/java/pom.xml"), "<project/>\n").unwrap();
    fs::write(app.join("src/java/Cargo.toml"), "[package]\n").unwrap();
    fs::write(app.join("src/go/go.mod"), "module demo\n").unwrap();
    fs::write(app.join("src/requirements.txt"), "example\n").unwrap();
    fs::write(app.join("src/README"), "direct source\n").unwrap();
    fs::write(app.join("src/.hidden"), "hidden source\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("README", app.join("src/alias")).unwrap();
    let excluded = workspace(temp.path(), "excluded");
    fs::write(excluded.join("src/keep"), "excluded source\n").unwrap();
    let legacy_ws = workspace(temp.path(), "ws");
    fs::create_dir_all(temp.path().join("legacy/no-src")).unwrap();

    let output = successful(run_migration(
        temp.path(),
        &["--exclude", "excluded"],
        "yes\n",
        &temp.path().join("legacy"),
    ));

    assert!(output.contains("Java: java/pom.xml"));
    assert!(output.contains("Golang: go/go.mod"));
    assert!(output.contains("Python: src/requirements.txt"));
    assert!(output.contains("Rust: java/Cargo.toml"));
    assert!(output.contains("profiles: java, python, rust"));
    assert!(output.contains("features: golang"));
    let create_args = fs::read_to_string(temp.path().join("xws-create.log")).unwrap();
    for expected in [
        "--profile\njava",
        "--profile\npython",
        "--profile\nrust",
        "--with-feature\ngolang",
        "app",
    ] {
        assert!(
            create_args.contains(expected),
            "missing {expected:?} in {create_args:?}"
        );
    }
    let migrated = temp.path().join("xws/workspaces/app/src");
    assert!(migrated.join("java/pom.xml").is_file());
    assert!(migrated.join("go/go.mod").is_file());
    assert!(migrated.join("requirements.txt").is_file());
    assert!(migrated.join("README").is_file());
    assert!(migrated.join(".hidden").is_file());
    assert_eq!(
        fs::read_to_string(migrated.join("ws/checkout")).unwrap(),
        "ordinary project directory\n"
    );
    #[cfg(unix)]
    assert_eq!(
        fs::read_link(migrated.join("alias")).unwrap(),
        Path::new("README")
    );
    assert!(app.join("src/ws/checkout").is_file());
    assert!(app.join("src/java/pom.xml").is_file());
    assert!(app.join("src/go/go.mod").is_file());
    assert!(app.join("src/requirements.txt").is_file());
    assert!(app.join("src/README").is_file());
    assert!(app.join("src/.hidden").is_file());
    #[cfg(unix)]
    assert_eq!(
        fs::read_link(app.join("src/alias")).unwrap(),
        Path::new("README")
    );
    assert!(!temp.path().join("xws/workspaces/excluded").exists());
    assert!(!temp.path().join("xws/workspaces/no-src").exists());
    assert!(excluded.join("src/keep").is_file());
    assert!(temp.path().join("xws/workspaces/ws").is_dir());
    assert!(legacy_ws.join("src").is_dir());
}

#[test]
fn batch_decline_makes_no_changes() {
    let temp = TestDir::new();
    let app = workspace(temp.path(), "app");
    fs::write(app.join("src/README"), "keep\n").unwrap();

    let output = successful(run_migration(
        temp.path(),
        &[],
        "no\n",
        &temp.path().join("legacy"),
    ));
    assert!(output.contains("Migration cancelled; no workspaces were changed."));
    assert!(app.join("src/README").is_file());
    assert!(!temp.path().join("xws/workspaces/app").exists());
    assert!(!temp.path().join("xws-create.log").exists());
}

#[test]
fn preview_reports_per_workspace_sizes_and_flags_large_migrations() {
    let temp = TestDir::new();
    let source_prefix = temp.path().join("source");
    let large_src = source_prefix.join("large/src");
    let small_src = source_prefix.join("small/src");
    fs::create_dir_all(&large_src).unwrap();
    fs::create_dir_all(&small_src).unwrap();
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(large_src.join("scan.tif"))
        .unwrap()
        .set_len(10 * 1024 * 1024 * 1024)
        .unwrap();
    fs::write(small_src.join("notes.txt"), "small\n").unwrap();

    let output = successful(run_migration(temp.path(), &[], "no\n", &source_prefix));

    assert!(output.contains("large:\n"));
    assert!(output.contains("estimated src size: 10.0 GiB"));
    assert!(output.contains("small:\n"));
    assert!(output.contains("estimated src size: 6 B"));
    assert!(output.contains("Estimated total src size: 10.0 GiB"));
    assert!(output.contains("Large migration(s) detected (at least 10.0 GiB):"));
    assert!(output.contains("  large: 10.0 GiB"));
    assert!(!temp.path().join("xws/workspaces/large").exists());
    assert!(!temp.path().join("xws/workspaces/small").exists());
}

#[test]
fn interactive_mode_prompts_per_workspace() {
    let temp = TestDir::new();
    let first = workspace(temp.path(), "first");
    let second = workspace(temp.path(), "second");
    fs::write(first.join("src/file"), "first\n").unwrap();
    fs::write(second.join("src/file"), "second\n").unwrap();

    let output = successful(run_migration(
        temp.path(),
        &["--interactive"],
        "yes\nno\n",
        &temp.path().join("legacy"),
    ));
    assert!(output.contains("Migrated workspace: first"));
    assert!(output.contains("Skipped workspace: second"));
    assert!(temp.path().join("xws/workspaces/first/src/file").is_file());
    assert!(second.join("src/file").is_file());
    assert!(!temp.path().join("xws/workspaces/second").exists());
}

#[test]
fn rerun_skips_completed_workspace_when_source_and_destination_match() {
    let temp = TestDir::new();
    let app = workspace(temp.path(), "app");
    fs::create_dir_all(app.join("src/project")).unwrap();
    fs::write(app.join("src/project/file"), "same content\n").unwrap();

    let first = successful(run_migration(
        temp.path(),
        &[],
        "yes\n",
        &temp.path().join("legacy"),
    ));
    assert!(first.contains("Migrated workspace: app"));
    let create_log = fs::read_to_string(temp.path().join("xws-create.log")).unwrap();

    let second = successful(run_migration(
        temp.path(),
        &[],
        "",
        &temp.path().join("legacy"),
    ));

    assert!(second.contains("Already migrated, source matches destination: app"));
    assert!(second.contains("All source workspaces are already migrated; no changes were made."));
    assert_eq!(
        fs::read_to_string(temp.path().join("xws-create.log")).unwrap(),
        create_log
    );
    assert!(app.join("src/project/file").is_file());
    assert!(
        !temp
            .path()
            .join("xws/workspaces/app/.xws-migration-ws")
            .exists()
    );
}

#[test]
fn rerun_ignores_only_vscode_merge_base_in_git_config() {
    let temp = TestDir::new();
    let app = workspace(temp.path(), "app");
    let source_config = app.join("src/project/.git/config");
    fs::create_dir_all(source_config.parent().unwrap()).unwrap();
    fs::write(
        &source_config,
        "[core]\n\trepositoryformatversion = 0\n[branch \"master\"]\n\tremote = origin\n",
    )
    .unwrap();
    successful(run_migration(
        temp.path(),
        &[],
        "yes\n",
        &temp.path().join("legacy"),
    ));

    let destination_config = temp
        .path()
        .join("xws/workspaces/app/src/project/.git/config");
    fs::OpenOptions::new()
        .append(true)
        .open(&destination_config)
        .unwrap()
        .write_all(b"\tvscode-merge-base = origin/master\n")
        .unwrap();
    let output = successful(run_migration(
        temp.path(),
        &[],
        "",
        &temp.path().join("legacy"),
    ));
    assert!(output.contains("Already migrated, source matches destination: app"));

    fs::OpenOptions::new()
        .append(true)
        .open(&destination_config)
        .unwrap()
        .write_all(b"\tpushRemote = unrelated\n")
        .unwrap();
    failed(
        run_migration(temp.path(), &[], "", &temp.path().join("legacy")),
        "xws workspace already exists",
    );
}

#[test]
fn an_interrupted_copy_can_be_retried_without_changing_sources() {
    let temp = TestDir::new();
    let app = workspace(temp.path(), "app");
    for name in ["a", "b", "c"] {
        fs::write(app.join("src").join(name), name).unwrap();
    }
    let xws_prefix = temp.path().join("xws");
    let staging = xws_prefix.join("workspaces/app/.xws-migration-ws-staging/payload");
    let bin = temp.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let count_file = temp.path().join("rsync-count");
    let real_rsync = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|directory| directory.join("rsync"))
        .find(|path| path.is_file())
        .expect("rsync must be available to run this test");
    let rsync_wrapper = bin.join("rsync");
    fs::write(
        &rsync_wrapper,
        r#"#!/bin/sh
[ "${1-}" = --version ] && exec "$REAL_RSYNC" "$@"
        case " $* " in
          *" --server "*) exec "$REAL_RSYNC" "$@" ;;
        esac
        count=0
[ ! -f "$RSYNC_COUNT_FILE" ] || count=$(cat "$RSYNC_COUNT_FILE")
count=$((count + 1))
printf '%s\n' "$count" > "$RSYNC_COUNT_FILE"
        "$REAL_RSYNC" "$@"
        status=$?
        [ "$status" -eq 0 ] || exit "$status"
        [ "$count" -ne 1 ] || exit 23
        exit "$status"
        "#,
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&rsync_wrapper, fs::Permissions::from_mode(0o755)).unwrap();

    let init = xws_init(temp.path());
    let mut command = Command::new(env!("CARGO_BIN_EXE_workspace-migrate"));
    command
        .env("XWS_PREFIX", &xws_prefix)
        .env("XWS_MIGRATE_INIT", init)
        .env("XWS_CREATE_LOG", temp.path().join("create.log"))
        .env("RSYNC_COUNT_FILE", &count_file)
        .env("REAL_RSYNC", real_rsync)
        .env(
            "PATH",
            format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
        )
        .arg("--prefix")
        .arg(app.parent().unwrap());
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"yes\n").unwrap();
    let failure = failed(child.wait_with_output().unwrap(), "rsync failed");

    assert!(staging.join("a").is_file(), "{failure}");
    for name in ["a", "b", "c"] {
        assert!(app.join("src").join(name).is_file());
    }

    let output = successful(run_migration(
        temp.path(),
        &[],
        "yes\n",
        &temp.path().join("legacy"),
    ));
    let destination = xws_prefix.join("workspaces/app");
    assert!(output.contains("status: resumable incomplete migration"));
    for name in ["a", "b", "c"] {
        assert!(destination.join("src").join(name).is_file());
        assert_eq!(
            fs::read_to_string(app.join("src").join(name)).unwrap(),
            name
        );
    }
    assert!(!destination.join(".xws-migration-ws").exists());
    assert!(!staging.exists());
}

#[test]
fn existing_workspace_and_source_collisions_are_not_overwritten() {
    let temp = TestDir::new();
    let app = workspace(temp.path(), "app");
    fs::write(app.join("src/file"), "legacy\n").unwrap();
    let existing = temp.path().join("xws/workspaces/app");
    fs::create_dir_all(&existing).unwrap();
    fs::write(existing.join("keep"), "existing\n").unwrap();

    failed(
        run_migration(temp.path(), &[], "yes\n", &temp.path().join("legacy")),
        "xws workspace already exists",
    );
    assert_eq!(
        fs::read_to_string(existing.join("keep")).unwrap(),
        "existing\n"
    );

    fs::remove_dir_all(&existing).unwrap();
    fs::create_dir_all(existing.join("src")).unwrap();
    fs::create_dir_all(existing.join("bin")).unwrap();
    fs::write(existing.join("bin/activate"), "").unwrap();
    fs::write(
        existing.join(".xws-migration-ws"),
        fs::canonicalize(app.join("src"))
            .unwrap()
            .as_os_str()
            .as_encoded_bytes(),
    )
    .unwrap();
    fs::write(existing.join("src/file"), "destination\n").unwrap();
    failed(
        run_migration(temp.path(), &[], "yes\n", &temp.path().join("legacy")),
        "destination collision",
    );
    assert_eq!(
        fs::read_to_string(app.join("src/file")).unwrap(),
        "legacy\n"
    );
    assert_eq!(
        fs::read_to_string(existing.join("src/file")).unwrap(),
        "destination\n"
    );
}
