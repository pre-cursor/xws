use std::error::Error;
use std::io::{self, Write};

use crossterm::cursor::{
    Hide, MoveDown, MoveTo, MoveToColumn, MoveToNextLine, MoveUp, RestorePosition, SavePosition,
    Show,
};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::style::{Attribute, Print, ResetColor, SetAttribute};
use crossterm::terminal::{
    self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
    enable_raw_mode,
};
use crossterm::{execute, queue};
use serde::Deserialize;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DisplayMode {
    Alternate,
    Inline,
}

impl DisplayMode {
    fn from_environment() -> Result<Self, Box<dyn Error>> {
        if let Some(value) = std::env::var_os("XWS_PICKER_DISPLAY") {
            return Self::parse(&value.to_string_lossy(), "XWS_PICKER_DISPLAY");
        }
        let config_path = config_path()?;
        let config = match std::fs::read_to_string(&config_path) {
            Ok(contents) => toml::from_str::<PickerConfig>(&contents)
                .map_err(|error| format!("cannot parse {}: {error}", config_path.display()))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => PickerConfig::default(),
            Err(error) => {
                return Err(format!("cannot read {}: {error}", config_path.display()).into());
            }
        };
        match config.screen_buffer {
            Some(value) => Self::parse(&value, &config_path.display().to_string()),
            None => Ok(Self::Alternate),
        }
    }

    fn parse(value: &str, source: &str) -> Result<Self, Box<dyn Error>> {
        match value {
            "alternate" => Ok(Self::Alternate),
            "inline" => Ok(Self::Inline),
            _ => Err(
                format!("{source}: screen_buffer must be either 'alternate' or 'inline'").into(),
            ),
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PickerConfig {
    screen_buffer: Option<String>,
}

fn config_path() -> Result<std::path::PathBuf, Box<dyn Error>> {
    let root = match std::env::var_os("XWS_STATE_HOME") {
        Some(path) => std::path::PathBuf::from(path),
        None => std::path::PathBuf::from(
            std::env::var_os("HOME").ok_or("HOME is not set; set XWS_STATE_HOME")?,
        )
        .join(".xws"),
    };
    Ok(root.join("config.toml"))
}

struct TerminalGuard {
    display: DisplayMode,
    inline_rows: u16,
}

impl TerminalGuard {
    fn enter(display: DisplayMode, inline_rows: u16) -> Result<Self, Box<dyn Error>> {
        enable_raw_mode()?;
        let setup = match display {
            DisplayMode::Alternate => execute!(
                io::stderr(),
                EnterAlternateScreen,
                Hide,
                Clear(ClearType::All)
            ),
            DisplayMode::Inline => execute!(
                io::stderr(),
                MoveToColumn(0),
                Print("\r\n".repeat(usize::from(inline_rows))),
                MoveUp(inline_rows),
                Hide
            ),
        };
        if let Err(error) = setup {
            match display {
                DisplayMode::Alternate => {
                    let _ = execute!(
                        io::stderr(),
                        Show,
                        LeaveAlternateScreen,
                        ResetColor,
                        SetAttribute(Attribute::Reset)
                    );
                }
                DisplayMode::Inline => {
                    let _ = execute!(
                        io::stderr(),
                        Show,
                        ResetColor,
                        SetAttribute(Attribute::Reset)
                    );
                }
            }
            return match disable_raw_mode() {
                Ok(()) => Err(error.into()),
                Err(restore_error) => Err(format!(
                    "terminal setup failed: {error}; also failed to restore terminal mode: {restore_error}"
                )
                .into()),
            };
        }
        Ok(Self {
            display,
            inline_rows,
        })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        match self.display {
            DisplayMode::Alternate => {
                let _ = execute!(
                    io::stderr(),
                    ResetColor,
                    SetAttribute(Attribute::Reset),
                    Show,
                    LeaveAlternateScreen
                );
            }
            DisplayMode::Inline => {
                let _ = execute!(
                    io::stderr(),
                    ResetColor,
                    SetAttribute(Attribute::Reset),
                    Show,
                    MoveDown(self.inline_rows)
                );
            }
        }
        let _ = disable_raw_mode();
    }
}

pub fn pick(workspaces: &[String], active: Option<&str>) -> Result<Option<String>, Box<dyn Error>> {
    if workspaces.is_empty() {
        return Ok(None);
    }

    let display = DisplayMode::from_environment()?;
    let (_, rows) = terminal::size()?;
    let initial_height = viewport_height(rows);
    let inline_height = initial_height.min(workspaces.len());
    let inline_rows = inline_height.saturating_add(2).min(usize::from(rows)) as u16;
    let mut selected = active.and_then(|name| workspaces.iter().position(|item| item == name));
    let mut first_visible = selected.map_or(0, |index| index.saturating_sub(3));
    let _terminal = TerminalGuard::enter(display, inline_rows)?;

    loop {
        let (_, rows) = terminal::size()?;
        let viewport_height = match display {
            DisplayMode::Alternate => viewport_height(rows),
            DisplayMode::Inline => inline_height.min(viewport_height(rows)),
        };
        if let Some(index) = selected {
            first_visible = center_active(index, first_visible, workspaces.len(), viewport_height);
        }
        render(
            workspaces,
            selected,
            first_visible,
            viewport_height,
            rows,
            display,
        )?;

        match event::read()? {
            Event::Resize(_, _) => continue,
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    selected = Some(move_selection(selected, workspaces.len(), Direction::Up));
                    first_visible = keep_visible(
                        selected.unwrap(),
                        first_visible,
                        workspaces.len(),
                        viewport_height,
                        Direction::Up,
                    );
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    selected = Some(move_selection(selected, workspaces.len(), Direction::Down));
                    first_visible = keep_visible(
                        selected.unwrap(),
                        first_visible,
                        workspaces.len(),
                        viewport_height,
                        Direction::Down,
                    );
                }
                KeyCode::Enter => return Ok(selected.map(|index| workspaces[index].clone())),
                KeyCode::Esc | KeyCode::Char('q') => return Ok(None),
                KeyCode::Char('c') if is_ctrl_c(&key) => return Ok(None),
                _ => {}
            },
            _ => {}
        }
    }
}

fn viewport_height(rows: u16) -> usize {
    usize::from(rows.saturating_sub(4)).max(1)
}

fn is_ctrl_c(key: &KeyEvent) -> bool {
    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}

#[derive(Clone, Copy)]
enum Direction {
    Up,
    Down,
}

fn move_selection(selected: Option<usize>, len: usize, direction: Direction) -> usize {
    match direction {
        Direction::Up => selected.map_or(0, |index| index.saturating_sub(1)),
        Direction::Down => selected.map_or(len - 1, |index| (index + 1).min(len - 1)),
    }
}

fn center_active(index: usize, first: usize, len: usize, height: usize) -> usize {
    let target_row = height / 2;
    let proposed = index.saturating_sub(target_row);
    let max_first = len.saturating_sub(height);
    proposed.clamp(0, max_first).max(first.min(max_first))
}

fn keep_visible(
    selected: usize,
    first: usize,
    len: usize,
    height: usize,
    direction: Direction,
) -> usize {
    let max_first = len.saturating_sub(height);
    let first = first.min(max_first);
    let upper_margin = height / 3;
    let lower_margin = height.saturating_mul(2) / 3;
    let proposed = match direction {
        Direction::Up if selected < first.saturating_add(upper_margin) => {
            selected.saturating_sub(upper_margin)
        }
        Direction::Down if selected > first.saturating_add(lower_margin) => {
            selected.saturating_sub(lower_margin)
        }
        _ => first,
    };
    proposed.min(max_first)
}

fn render(
    workspaces: &[String],
    selected: Option<usize>,
    first: usize,
    height: usize,
    terminal_rows: u16,
    display: DisplayMode,
) -> Result<(), Box<dyn Error>> {
    let mut output = io::stderr().lock();
    match display {
        DisplayMode::Alternate => {
            queue!(
                output,
                MoveTo(0, 0),
                Clear(ClearType::All),
                Print("xws — select a workspace")
            )?;
        }
        DisplayMode::Inline => {
            queue!(
                output,
                SavePosition,
                MoveToColumn(0),
                Clear(ClearType::CurrentLine),
                Print("xws — select a workspace"),
                MoveToNextLine(1)
            )?;
        }
    }
    for row in 0..height {
        match display {
            DisplayMode::Alternate => queue!(output, MoveTo(0, (row + 1) as u16))?,
            DisplayMode::Inline => queue!(output, Clear(ClearType::CurrentLine))?,
        }
        if let Some(name) = workspaces.get(first + row) {
            if selected == Some(first + row) {
                queue!(
                    output,
                    SetAttribute(Attribute::Reverse),
                    Print(format!(" > {name} ")),
                    SetAttribute(Attribute::Reset)
                )?;
            } else {
                queue!(output, Print(format!("   {name}")))?;
            }
        }
        if display == DisplayMode::Inline {
            queue!(output, MoveToNextLine(1))?;
        }
    }
    if terminal_rows > 1 {
        match display {
            DisplayMode::Alternate => {
                queue!(output, MoveTo(0, terminal_rows - 1))?;
            }
            DisplayMode::Inline => {
                queue!(output, Clear(ClearType::CurrentLine))?;
            }
        }
        queue!(output, Print("↑/↓ or j/k move  Enter select  q quit"))?;
    }
    if display == DisplayMode::Inline {
        queue!(output, RestorePosition)?;
    }
    output.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_key_selects_opposite_endpoints_without_wrapping() {
        let len = 10;
        assert_eq!(move_selection(None, len, Direction::Up), 0);
        assert_eq!(move_selection(None, len, Direction::Down), len - 1);
        assert_eq!(move_selection(Some(0), len, Direction::Up), 0);
        assert_eq!(move_selection(Some(len - 1), len, Direction::Down), len - 1);
        assert_eq!(keep_visible(0, 0, len, 5, Direction::Up), 0);
        assert_eq!(keep_visible(len - 1, 0, len, 5, Direction::Down), len - 5);
    }

    #[test]
    fn viewport_scrolls_and_clamps_at_both_ends() {
        assert_eq!(keep_visible(8, 0, 20, 6, Direction::Down), 4);
        assert_eq!(keep_visible(3, 5, 20, 6, Direction::Up), 1);
        assert_eq!(keep_visible(19, 18, 20, 6, Direction::Down), 14);
    }

    #[test]
    fn picker_reserves_two_additional_terminal_rows() {
        assert_eq!(viewport_height(24), 20);
        assert_eq!(viewport_height(3), 1);
    }

    #[test]
    fn ctrl_c_is_a_cancel_key_only_with_control_modifier() {
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let plain_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE);
        assert!(is_ctrl_c(&ctrl_c));
        assert!(!is_ctrl_c(&plain_c));
    }
}
