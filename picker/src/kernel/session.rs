use crate::kernel::workspace;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Serialize, Deserialize)]
pub struct ActivationFrame {
    pub workspace: String,
    pub workspace_path: String,
    pub return_dir: String,
    pub previous_vars: Vec<(String, String, Option<String>)>,
}

pub type PopResult = Option<(ActivationFrame, Option<ActivationFrame>)>;

#[derive(Default, Serialize, Deserialize)]
struct Session {
    stack: Vec<ActivationFrame>,
    /// Shell that owns the session; absent in files written by older versions.
    #[serde(default)]
    shell_pid: Option<u32>,
}

/// Another live shell that has a workspace on its activation stack.
pub struct SessionHolder {
    pub path: PathBuf,
    pub shell_pid: Option<u32>,
    pub age_seconds: u64,
}

struct Entry {
    path: PathBuf,
    created_nanos: u128,
    session: Option<Session>,
}

pub struct SessionStore {
    root: PathBuf,
}

impl SessionStore {
    pub fn from_environment() -> Result<Self, Box<dyn Error>> {
        let root = match std::env::var_os("XWS_STATE_HOME") {
            Some(path) => PathBuf::from(path),
            None => PathBuf::from(
                std::env::var_os("HOME").ok_or("HOME is not set; set XWS_STATE_HOME")?,
            )
            .join(".xws"),
        };
        Ok(Self {
            root: root.join("sessions"),
        })
    }

    pub fn create(&self, shell_pid: Option<u32>) -> Result<String, Box<dyn Error>> {
        fs::create_dir_all(&self.root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700))?;
        }
        self.remove_stale_with(boot_time_nanos(), &pid_is_alive);

        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let base = format!("{}-{stamp}", std::process::id());
        for attempt in 0..100 {
            let id = if attempt == 0 {
                base.clone()
            } else {
                format!("{base}-{attempt}")
            };
            let path = self.session_path(&id)?;
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        file.set_permissions(fs::Permissions::from_mode(0o600))?;
                    }
                    let session = Session {
                        stack: Vec::new(),
                        shell_pid,
                    };
                    serde_json::to_writer(&mut file, &session)?;
                    file.write_all(b"\n")?;
                    return Ok(id);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }

        Err("could not allocate a unique xws session id".into())
    }

    pub fn push(&self, id: &str, frame: ActivationFrame) -> Result<(), Box<dyn Error>> {
        validate_frame(&frame)?;
        let path = self.session_path(id)?;
        let mut session = self.read(&path)?;
        session.stack.push(frame);
        self.write(&path, &session)
    }

    pub fn pop(&self, id: &str) -> Result<PopResult, Box<dyn Error>> {
        let path = self.session_path(id)?;
        let mut session = self.read(&path)?;
        let Some(frame) = session.stack.pop() else {
            return Ok(None);
        };
        let parent = session.stack.last().cloned();
        if session.stack.is_empty() {
            fs::remove_file(path)?;
        } else {
            self.write(&path, &session)?;
        }
        Ok(Some((frame, parent)))
    }

    pub fn top(&self, id: &str) -> Result<Option<ActivationFrame>, Box<dyn Error>> {
        let path = self.session_path(id)?;
        let session = self.read(&path)?;
        Ok(session.stack.last().cloned())
    }

    pub fn contains(&self, id: &str, workspace: &str) -> Result<bool, Box<dyn Error>> {
        let path = self.session_path(id)?;
        let session = self.read(&path)?;
        Ok(session
            .stack
            .iter()
            .any(|frame| frame.workspace == workspace))
    }

    /// Refuse when another live shell has `workspace` active, with a message
    /// that says how to resolve it.
    pub fn ensure_inactive_in_other_shells(&self, workspace: &str) -> Result<(), Box<dyn Error>> {
        let holders = self.holders(workspace);
        let Some(first) = holders.first() else {
            return Ok(());
        };
        let shell = match first.shell_pid {
            Some(pid) => format!("shell pid {pid}"),
            None => "another shell".to_owned(),
        };
        let more = match holders.len() {
            1 => String::new(),
            count => format!(" (and {} more)", count - 1),
        };
        Err(format!(
            "workspace '{workspace}' is active in {shell}, started {} ago{more}\n  \
             Run 'xws deactivate {workspace}' in that shell or close it, then try again.\n  \
             If that shell no longer exists, delete its stale session file:\n    rm '{}'",
            format_age(first.age_seconds),
            first.path.display()
        )
        .into())
    }

    /// Live sessions that have `workspace` on their stack. Stale sessions are
    /// ignored but never deleted here; only `create` cleans up.
    pub fn holders(&self, workspace: &str) -> Vec<SessionHolder> {
        self.holders_with(workspace, boot_time_nanos(), &pid_is_alive)
    }

    fn holders_with(
        &self,
        workspace: &str,
        boot: Option<u128>,
        alive: &dyn Fn(u32) -> bool,
    ) -> Vec<SessionHolder> {
        let now = now_nanos();
        let mut holders = Vec::new();
        for entry in self.entries() {
            if is_stale(&entry, boot, alive) {
                continue;
            }
            let Some(session) = entry.session else {
                continue;
            };
            if session
                .stack
                .iter()
                .any(|frame| frame.workspace == workspace)
            {
                holders.push(SessionHolder {
                    path: entry.path,
                    shell_pid: session.shell_pid,
                    age_seconds: u64::try_from(
                        now.saturating_sub(entry.created_nanos) / 1_000_000_000,
                    )
                    .unwrap_or(u64::MAX),
                });
            }
        }
        holders
    }

    /// Best-effort removal of session files whose shell cannot exist any more.
    fn remove_stale_with(&self, boot: Option<u128>, alive: &dyn Fn(u32) -> bool) {
        for entry in self.entries() {
            if is_stale(&entry, boot, alive) {
                let _ = fs::remove_file(&entry.path);
            }
        }
    }

    /// Session files named `<pid>-<nanos>[-<n>].json`; anything else is ignored.
    fn entries(&self) -> Vec<Entry> {
        let Ok(dir) = fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut entries = Vec::new();
        for item in dir.flatten() {
            let path = item.path();
            let Some(stem) = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_suffix(".json"))
            else {
                continue;
            };
            let mut parts = stem.splitn(3, '-');
            let created_nanos = parts
                .next()
                .and_then(|pid| pid.parse::<u32>().ok())
                .and_then(|_| parts.next())
                .and_then(|nanos| nanos.parse::<u128>().ok());
            let Some(created_nanos) = created_nanos else {
                continue;
            };
            if !item.file_type().is_ok_and(|kind| kind.is_file()) {
                continue;
            }
            let session = fs::read(&path)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok());
            entries.push(Entry {
                path,
                created_nanos,
                session,
            });
        }
        entries
    }

    fn session_path(&self, id: &str) -> Result<PathBuf, Box<dyn Error>> {
        if id.is_empty()
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err("invalid xws session id".into());
        }
        Ok(self.root.join(format!("{id}.json")))
    }

    fn read(&self, path: &Path) -> Result<Session, Box<dyn Error>> {
        let metadata = fs::symlink_metadata(path)
            .map_err(|_| "xws session is missing; start a new shell session")?;
        if !metadata.file_type().is_file() {
            return Err("xws session state is not a regular file".into());
        }
        let session: Session = serde_json::from_slice(&fs::read(path)?)?;
        for frame in &session.stack {
            validate_frame(frame)?;
        }
        Ok(session)
    }

    fn write(&self, path: &Path, session: &Session) -> Result<(), Box<dyn Error>> {
        let temp = path.with_extension(format!("{}.tmp", std::process::id()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        serde_json::to_writer(&mut file, session)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(temp, path)?;
        Ok(())
    }
}

fn format_age(seconds: u64) -> String {
    match seconds {
        0..=59 => format!("{seconds}s"),
        60..=3599 => format!("{}m", seconds / 60),
        3600..=86399 => format!("{}h", seconds / 3600),
        _ => format!("{}d", seconds / 86400),
    }
}

/// A session is stale when it predates the last boot or its shell has exited.
fn is_stale(entry: &Entry, boot: Option<u128>, alive: &dyn Fn(u32) -> bool) -> bool {
    if boot.is_some_and(|boot| entry.created_nanos < boot) {
        return true;
    }
    entry
        .session
        .as_ref()
        .and_then(|session| session.shell_pid)
        .is_some_and(|pid| !alive(pid))
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos())
}

#[cfg(unix)]
fn pid_is_alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return true;
    };
    // Signal 0 only checks existence; pid 0 would address the whole process group.
    if pid == 0 {
        return true;
    }
    // SAFETY: kill with signal 0 performs no action beyond the permission check.
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

#[cfg(not(unix))]
fn pid_is_alive(_pid: u32) -> bool {
    true
}

#[cfg(target_os = "macos")]
fn boot_time_nanos() -> Option<u128> {
    let mut boot = libc::timeval {
        tv_sec: 0,
        tv_usec: 0,
    };
    let mut size = std::mem::size_of::<libc::timeval>();
    // SAFETY: the name is NUL-terminated and `boot`/`size` describe a valid
    // writable buffer for the kern.boottime timeval.
    let result = unsafe {
        libc::sysctlbyname(
            c"kern.boottime".as_ptr(),
            std::ptr::from_mut(&mut boot).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 || size != std::mem::size_of::<libc::timeval>() {
        return None;
    }
    let seconds = u128::try_from(boot.tv_sec).ok()?;
    let micros = u128::try_from(boot.tv_usec).ok()?;
    Some(seconds * 1_000_000_000 + micros * 1_000)
}

#[cfg(target_os = "linux")]
fn boot_time_nanos() -> Option<u128> {
    let stat = fs::read_to_string("/proc/stat").ok()?;
    let seconds = stat
        .lines()
        .find_map(|line| line.strip_prefix("btime "))?
        .trim()
        .parse::<u128>()
        .ok()?;
    Some(seconds * 1_000_000_000)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn boot_time_nanos() -> Option<u128> {
    None
}

fn validate_frame(frame: &ActivationFrame) -> Result<(), Box<dyn Error>> {
    workspace::validate_name(&frame.workspace)?;
    if frame.workspace_path.is_empty() || frame.return_dir.is_empty() {
        return Err("invalid xws session frame path".into());
    }
    for (name, state, value) in &frame.previous_vars {
        workspace::validate_environment_name(name)?;
        match (state.as_str(), value) {
            ("set-exported" | "set-local", Some(_)) | ("unset", None) => {}
            _ => {
                return Err(
                    format!("invalid saved state for environment variable '{name}'").into(),
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_pop_restores_ordered_context_and_removes_empty_session() {
        let temp = tempfile::tempdir().unwrap();
        let store = SessionStore {
            root: temp.path().join("sessions"),
        };
        let id = store.create(None).unwrap();
        let first = ActivationFrame {
            workspace: "foo".to_owned(),
            workspace_path: "/workspaces/foo".to_owned(),
            return_dir: "/starting".to_owned(),
            previous_vars: vec![(
                "MODE".to_owned(),
                "set-exported".to_owned(),
                Some("original".to_owned()),
            )],
        };
        let second = ActivationFrame {
            workspace: "bar".to_owned(),
            workspace_path: "/workspaces/bar".to_owned(),
            return_dir: "/workspaces/foo/src".to_owned(),
            previous_vars: vec![(
                "MODE".to_owned(),
                "set-exported".to_owned(),
                Some("foo".to_owned()),
            )],
        };
        store.push(&id, first.clone()).unwrap();
        store.push(&id, second.clone()).unwrap();
        assert!(store.contains(&id, "foo").unwrap());
        assert!(store.contains(&id, "bar").unwrap());
        assert!(!store.contains(&id, "missing").unwrap());

        let (popped, parent) = store.pop(&id).unwrap().unwrap();
        assert_eq!(popped.workspace, second.workspace);
        assert_eq!(parent.unwrap().workspace, first.workspace);
        let (popped, parent) = store.pop(&id).unwrap().unwrap();
        assert_eq!(popped.workspace, first.workspace);
        assert!(parent.is_none());
        assert!(!store.session_path(&id).unwrap().exists());
    }

    #[test]
    fn rejects_malformed_saved_environment_records() {
        let temp = tempfile::tempdir().unwrap();
        let store = SessionStore {
            root: temp.path().join("sessions"),
        };
        fs::create_dir_all(&store.root).unwrap();
        fs::write(
            store.session_path("test").unwrap(),
            r#"{"stack":[{"workspace":"foo","workspace_path":"/foo","return_dir":"/","previous_vars":[["PATH","set-exported","old"]]}]}"#,
        )
        .unwrap();

        assert!(store.top("test").is_err());
    }

    #[test]
    fn age_is_formatted_in_the_largest_whole_unit() {
        assert_eq!(format_age(5), "5s");
        assert_eq!(format_age(120), "2m");
        assert_eq!(format_age(7200), "2h");
        assert_eq!(format_age(3 * 86400 + 5), "3d");
    }

    fn write_session(store: &SessionStore, id: &str, shell_pid: Option<u32>, workspace: &str) {
        fs::create_dir_all(&store.root).unwrap();
        let session = Session {
            stack: vec![ActivationFrame {
                workspace: workspace.to_owned(),
                workspace_path: format!("/workspaces/{workspace}"),
                return_dir: "/".to_owned(),
                previous_vars: Vec::new(),
            }],
            shell_pid,
        };
        fs::write(
            store.session_path(id).unwrap(),
            serde_json::to_vec(&session).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn stale_sessions_are_removed_and_live_ones_kept() {
        let temp = tempfile::tempdir().unwrap();
        let store = SessionStore {
            root: temp.path().join("sessions"),
        };
        let boot = 1_000_000_000_000u128;
        write_session(&store, "10-2000000000000", Some(111), "foo"); // live shell
        write_session(&store, "10-2000000000001", Some(222), "foo"); // dead shell
        write_session(&store, "10-500000000000", Some(111), "foo"); // before boot
        write_session(&store, "10-2000000000002", None, "foo"); // legacy, after boot
        fs::write(store.root.join("notes.txt"), "keep").unwrap();
        fs::write(store.root.join("junk.json"), "keep").unwrap();

        store.remove_stale_with(Some(boot), &|pid| pid == 111);

        let exists = |id: &str| store.session_path(id).unwrap().exists();
        assert!(exists("10-2000000000000"));
        assert!(!exists("10-2000000000001"));
        assert!(!exists("10-500000000000"));
        assert!(exists("10-2000000000002"));
        assert!(store.root.join("notes.txt").exists());
        assert!(store.root.join("junk.json").exists());
    }

    #[test]
    fn unknown_boot_time_skips_only_the_boot_rule() {
        let temp = tempfile::tempdir().unwrap();
        let store = SessionStore {
            root: temp.path().join("sessions"),
        };
        write_session(&store, "10-1", Some(111), "foo");
        write_session(&store, "10-2", Some(222), "foo");
        store.remove_stale_with(None, &|pid| pid == 111);
        assert!(store.session_path("10-1").unwrap().exists());
        assert!(!store.session_path("10-2").unwrap().exists());
    }

    #[test]
    fn holders_ignore_stale_sessions_without_deleting_them() {
        let temp = tempfile::tempdir().unwrap();
        let store = SessionStore {
            root: temp.path().join("sessions"),
        };
        write_session(&store, "10-2000000000000", Some(111), "foo");
        write_session(&store, "10-2000000000001", Some(222), "foo");
        write_session(&store, "10-2000000000002", Some(111), "bar");

        let holders = store.holders_with("foo", None, &|pid| pid == 111);
        assert_eq!(holders.len(), 1);
        assert_eq!(holders[0].shell_pid, Some(111));
        assert!(store.session_path("10-2000000000001").unwrap().exists());
        assert!(store.holders_with("missing", None, &|_| true).is_empty());
    }

    #[test]
    fn create_records_the_shell_pid_and_cleans_dead_sessions() {
        let temp = tempfile::tempdir().unwrap();
        let store = SessionStore {
            root: temp.path().join("sessions"),
        };
        // Larger than any pid_max in use, so no such process exists.
        write_session(&store, "10-9999999999999999999", Some(2_147_483_646), "foo");
        let id = store.create(Some(std::process::id())).unwrap();
        assert!(
            !store
                .session_path("10-9999999999999999999")
                .unwrap()
                .exists()
        );
        let session = store.read(&store.session_path(&id).unwrap()).unwrap();
        assert_eq!(session.shell_pid, Some(std::process::id()));
    }
}
