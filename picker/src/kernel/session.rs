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

    pub fn create(&self) -> Result<String, Box<dyn Error>> {
        fs::create_dir_all(&self.root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700))?;
        }

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
                    serde_json::to_writer(&mut file, &Session::default())?;
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
        let id = store.create().unwrap();
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
}
