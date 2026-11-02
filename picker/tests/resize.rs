use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use tempfile::tempdir;

fn nz(value: u16) -> std::num::NonZeroU16 {
    std::num::NonZeroU16::new(value).unwrap()
}

const HEADER: &str = "xws — select a workspace";
const FOOTER: &str = "Enter select";

struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    output: Receiver<Vec<u8>>,
    parser: vt100::Parser,
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

impl Session {
    fn start(prefix: &std::path::Path, state_home: &std::path::Path, rows: u16, cols: u16) -> Self {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer = pair.master.take_writer().unwrap();
        let (tx, output) = mpsc::channel();
        thread::spawn(move || {
            let mut buffer = [0; 1024];
            while let Ok(count) = reader.read(&mut buffer) {
                if count == 0 || tx.send(buffer[..count].to_vec()).is_err() {
                    break;
                }
            }
        });

        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_xws-picker"));
        command.arg("pick");
        command.env("XWS_PREFIX", prefix);
        command.env("XWS_STATE_HOME", state_home);
        command.env("XWS_PICKER_DISPLAY", "inline");
        let child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);

        // A shell prompt normally sits on the bottom row of the window.
        let mut parser = vt100::Parser::new(nz(rows), nz(cols), 0);
        parser.process("\r\n".repeat(usize::from(rows) - 1).as_bytes());
        Self {
            master: pair.master,
            writer,
            output,
            parser,
            child,
        }
    }

    /// Feed terminal output to the emulator until the stream is quiet.
    fn settle(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            match self.output.recv_timeout(Duration::from_millis(120)) {
                Ok(bytes) => self.parser.process(&bytes),
                Err(_) => return,
            }
        }
        panic!("terminal output never settled");
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        self.parser.screen_mut().set_size(nz(rows), nz(cols));
        self.settle();
    }

    fn send(&mut self, keys: &[u8]) {
        self.writer.write_all(keys).unwrap();
        self.writer.flush().unwrap();
        self.settle();
    }

    fn screen(&self) -> Vec<String> {
        self.parser
            .screen()
            .contents()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn describe(&self) -> String {
        format!("{:#?}", self.screen())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn workspaces(count: usize) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let temp = tempdir().unwrap();
    let prefix = temp.path().join("prefix");
    let state_home = temp.path().join("state");
    for index in 0..count {
        let status = std::process::Command::new(env!("CARGO_BIN_EXE_xws-picker"))
            .args(["create", &format!("ws-{index:02}")])
            .env("XWS_PREFIX", &prefix)
            .env("XWS_STATE_HOME", &state_home)
            .status()
            .unwrap();
        assert!(status.success());
    }
    (temp, prefix, state_home)
}

/// The picker block as drawn on screen: everything from the header down.
fn block(session: &Session) -> Vec<String> {
    let screen = session.screen();
    let start = screen
        .iter()
        .position(|line| line.starts_with("xws —"))
        .unwrap_or_else(|| panic!("no header\n{}", session.describe()));
    screen[start..].to_vec()
}

/// Header, then consecutive list rows, then the footer as the very last row.
fn assert_clean(session: &Session, rows: u16, list_rows: usize, context: &str) {
    let block = block(session);
    let dump = session.describe();
    assert_eq!(block.len(), list_rows + 2, "{context}: block size\n{dump}");
    assert!(block[0].contains(HEADER), "{context}\n{dump}");
    for (index, line) in block[1..=list_rows].iter().enumerate() {
        assert!(
            line.trim_start_matches([' ', '>']).trim() == format!("ws-{index:02}"),
            "{context}: row {index}\n{dump}"
        );
    }
    assert!(block[list_rows + 1].contains(FOOTER), "{context}\n{dump}");
    assert!(
        session.screen().len() <= usize::from(rows),
        "{context}: output taller than the window\n{dump}"
    );
}

#[test]
fn inline_picker_redraws_cleanly_when_the_window_shrinks_and_grows() {
    let (_temp, prefix, state_home) = workspaces(30);
    let mut session = Session::start(&prefix, &state_home, 24, 80);
    session.settle();
    assert_clean(&session, 24, 20, "start");

    session.resize(12, 80);
    assert_clean(&session, 12, 8, "after shrink");

    session.resize(8, 80);
    assert_clean(&session, 8, 4, "after second shrink");

    session.resize(30, 80);
    assert_clean(&session, 30, 20, "after grow");

    session.send(b"\x1b[B");
    let selected: Vec<_> = block(&session)
        .into_iter()
        .filter(|line| line.contains('>'))
        .collect();
    assert_eq!(selected.len(), 1, "{}", session.describe());
}

#[test]
fn inline_picker_rows_never_wrap_in_a_narrow_window() {
    let (_temp, prefix, state_home) = workspaces(6);
    let mut session = Session::start(&prefix, &state_home, 12, 80);
    session.settle();
    session.resize(12, 20);
    let block = block(&session);
    assert_eq!(block.len(), 8, "{}", session.describe());
    assert!(
        block.last().unwrap().contains("↑/↓"),
        "{}",
        session.describe()
    );
}

fn highlighted(session: &Session) -> Vec<String> {
    block(session)
        .into_iter()
        .filter(|line| line.trim_start().starts_with('>'))
        .map(|line| line.trim_start_matches([' ', '>']).trim().to_owned())
        .collect()
}

#[test]
fn selection_stays_visible_and_follows_the_keys_after_a_resize() {
    let (_temp, prefix, state_home) = workspaces(30);
    let mut session = Session::start(&prefix, &state_home, 24, 80);
    session.settle();

    for (rows, label) in [(12u16, "shrunk"), (6, "shrunk again"), (30, "grown")] {
        session.resize(rows, 80);
        session.send(b"\x1b[B");
        let mut previous = highlighted(&session);
        assert_eq!(previous.len(), 1, "{label}, down\n{}", session.describe());
        for step in 1..=8 {
            session.send(b"\x1b[A");
            let shown = highlighted(&session);
            assert_eq!(
                shown.len(),
                1,
                "{label}, up step {step}\n{}",
                session.describe()
            );
            assert_ne!(
                shown,
                previous,
                "{label}: highlight froze at up step {step}\n{}",
                session.describe()
            );
            previous = shown;
        }
        for step in 1..=8 {
            session.send(b"\x1b[B");
            let shown = highlighted(&session);
            assert_eq!(
                shown.len(),
                1,
                "{label}, down step {step}\n{}",
                session.describe()
            );
            assert_ne!(
                shown,
                previous,
                "{label}: highlight froze at down step {step}\n{}",
                session.describe()
            );
            previous = shown;
        }
    }
}
