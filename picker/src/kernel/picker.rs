use std::error::Error;
use std::io::{self, Write};
use std::path::PathBuf;

use crossterm::cursor::{
    Hide, MoveDown, MoveTo, MoveToColumn, MoveToNextLine, MoveUp, RestorePosition, SavePosition,
    Show,
};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor};
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
    fn from_environment(config: &PickerConfig, config_path: &str) -> Result<Self, Box<dyn Error>> {
        if let Some(value) = std::env::var_os("XWS_PICKER_DISPLAY") {
            return Self::parse(&value.to_string_lossy(), "XWS_PICKER_DISPLAY");
        }
        match config.screen_buffer.as_deref() {
            Some(value) => Self::parse(value, config_path),
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum QueryRenderMode {
    Filtered,
    Full,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PickerMode {
    Browse,
    Query,
    Navigate,
}

struct RenderState<'a> {
    workspaces: &'a [String],
    display_indices: &'a [usize],
    selected: Option<usize>,
    first: usize,
    height: usize,
    terminal_size: (u16, u16),
    display: DisplayMode,
    mode: PickerMode,
    query: &'a [char],
    query_cursor: usize,
    query_error: Option<QueryError>,
}

#[derive(Clone, Copy)]
enum QueryError {
    NoMatches,
    IncompleteEscape,
}

impl QueryError {
    fn message(self, query: &[char]) -> String {
        let query: String = query.iter().collect();
        match self {
            Self::NoMatches => format!("E486: Pattern not found: {query}"),
            Self::IncompleteEscape => "Search pattern ends with an incomplete escape".to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PatternToken {
    Literal(char),
    Any,
}

#[derive(Debug)]
struct SearchPattern {
    tokens: Vec<PatternToken>,
    anchored_start: bool,
    anchored_end: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PatternError {
    IncompleteEscape,
}

impl QueryRenderMode {
    fn parse(value: &str, source: &str) -> Result<Self, Box<dyn Error>> {
        match value {
            "filtered" => Ok(Self::Filtered),
            "full" => Ok(Self::Full),
            _ => Err(
                format!("{source}: query_render_mode must be either 'filtered' or 'full'").into(),
            ),
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PickerConfig {
    screen_buffer: Option<String>,
    query_render_mode: Option<String>,
}

impl PickerConfig {
    fn load() -> Result<(Self, PathBuf), Box<dyn Error>> {
        let path = config_path()?;
        let config = match std::fs::read_to_string(&path) {
            Ok(contents) => toml::from_str(&contents)
                .map_err(|error| format!("cannot parse {}: {error}", path.display()))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(error) => {
                return Err(format!("cannot read {}: {error}", path.display()).into());
            }
        };
        Ok((config, path))
    }

    fn query_render_mode(&self, source: &str) -> Result<QueryRenderMode, Box<dyn Error>> {
        self.query_render_mode
            .as_deref()
            .map_or(Ok(QueryRenderMode::Filtered), |value| {
                QueryRenderMode::parse(value, source)
            })
    }
}

fn config_path() -> Result<PathBuf, Box<dyn Error>> {
    let root = match std::env::var_os("XWS_STATE_HOME") {
        Some(path) => PathBuf::from(path),
        None => {
            PathBuf::from(std::env::var_os("HOME").ok_or("HOME is not set; set XWS_STATE_HOME")?)
                .join(".xws")
        }
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

impl TerminalGuard {
    /// Re-reserve the inline block after a resize. The cursor is still on the
    /// block's first row, so clear from there down, then scroll the screen if
    /// the block no longer fits below it, exactly as `enter` does.
    fn reflow_inline(&mut self, inline_rows: u16) -> Result<(), Box<dyn Error>> {
        if self.display != DisplayMode::Inline {
            return Ok(());
        }
        execute!(
            io::stderr(),
            MoveToColumn(0),
            Clear(ClearType::FromCursorDown),
            Print("\r\n".repeat(usize::from(inline_rows))),
            MoveUp(inline_rows)
        )?;
        self.inline_rows = inline_rows;
        Ok(())
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

    let (config, config_path) = PickerConfig::load()?;
    let config_source = config_path.display().to_string();
    let display = DisplayMode::from_environment(&config, &config_source)?;
    let query_render_mode = config.query_render_mode(&config_source)?;
    let (_, rows) = terminal::size()?;
    let initial_height = viewport_height(rows);
    let inline_height = initial_height.min(workspaces.len());
    let inline_rows = inline_block_rows(inline_height, rows);
    let mut selected = active.and_then(|name| workspaces.iter().position(|item| item == name));
    let mut query_origin_selection = selected;
    let mut first_visible = selected.map_or(0, |index| index.saturating_sub(3));
    let mut query = Vec::new();
    let mut query_cursor = 0;
    let mut matches: Vec<usize> = (0..workspaces.len()).collect();
    let mut mode = PickerMode::Browse;
    let mut query_error = None;
    let mut terminal = TerminalGuard::enter(display, inline_rows)?;

    loop {
        let (cols, rows) = terminal::size()?;
        let viewport_height = match display {
            DisplayMode::Alternate => viewport_height(rows),
            DisplayMode::Inline => inline_height.min(viewport_height(rows)),
        };
        let display_indices = match (mode, query_render_mode) {
            (PickerMode::Browse, _) | (_, QueryRenderMode::Full) => {
                (0..workspaces.len()).collect::<Vec<_>>()
            }
            (_, QueryRenderMode::Filtered) => matches.clone(),
        };
        let preview_selection = match mode {
            PickerMode::Browse => selected,
            PickerMode::Query => query_origin_selection
                .filter(|index| matches.contains(index))
                .or_else(|| next_match_after(&matches, query_origin_selection)),
            PickerMode::Navigate => selected,
        };
        let display_selection = preview_selection.and_then(|index| {
            display_indices
                .iter()
                .position(|candidate| *candidate == index)
        });
        first_visible = first_visible.min(display_indices.len().saturating_sub(viewport_height));
        if let Some(index) = display_selection {
            first_visible = if mode == PickerMode::Browse {
                center_active(index, first_visible, display_indices.len(), viewport_height)
            } else {
                ensure_visible(index, first_visible, display_indices.len(), viewport_height)
            };
        }
        render(RenderState {
            workspaces,
            display_indices: &display_indices,
            selected: display_selection,
            first: first_visible,
            height: viewport_height,
            terminal_size: (cols, rows),
            display,
            mode,
            query: &query,
            query_cursor,
            query_error,
        })?;

        match event::read()? {
            Event::Resize(_, rows) => {
                let height = inline_height.min(self::viewport_height(rows));
                terminal.reflow_inline(inline_block_rows(height, rows))?;
                continue;
            }
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                code if code == KeyCode::Up
                    || (code == KeyCode::Char('p')
                        && key.modifiers.contains(KeyModifiers::CONTROL)) =>
                {
                    match mode {
                        PickerMode::Browse => {
                            let index = move_selection(selected, workspaces.len(), Direction::Up);
                            selected = Some(index);
                            first_visible = keep_visible(
                                index,
                                first_visible,
                                workspaces.len(),
                                viewport_height,
                                Direction::Up,
                            );
                        }
                        PickerMode::Query => {
                            enter_navigation(
                                workspaces,
                                &query,
                                &mut matches,
                                query_origin_selection,
                                &mut selected,
                                &mut mode,
                                &mut query_error,
                            );
                            first_visible = 0;
                        }
                        PickerMode::Navigate => {
                            if !matches.is_empty() {
                                selected = Some(move_match(selected, &matches, Direction::Up));
                                first_visible = 0;
                            }
                        }
                    }
                }

                code if code == KeyCode::Down
                    || (code == KeyCode::Char('n')
                        && key.modifiers.contains(KeyModifiers::CONTROL)) =>
                {
                    match mode {
                        PickerMode::Browse => {
                            let index = move_selection(selected, workspaces.len(), Direction::Down);
                            selected = Some(index);
                            first_visible = keep_visible(
                                index,
                                first_visible,
                                workspaces.len(),
                                viewport_height,
                                Direction::Down,
                            );
                        }
                        PickerMode::Query => {
                            enter_navigation(
                                workspaces,
                                &query,
                                &mut matches,
                                query_origin_selection,
                                &mut selected,
                                &mut mode,
                                &mut query_error,
                            );
                            first_visible = 0;
                        }
                        PickerMode::Navigate => {
                            if !matches.is_empty() {
                                selected = Some(move_match(selected, &matches, Direction::Down));
                                first_visible = 0;
                            }
                        }
                    }
                }

                KeyCode::Esc => match mode {
                    PickerMode::Query => {
                        mode = PickerMode::Browse;
                        selected = query_origin_selection;
                        query.clear();
                        query_cursor = 0;
                        query_error = None;
                        matches = (0..workspaces.len()).collect();
                        first_visible = selected.map_or(0, |index| index.saturating_sub(3));
                    }
                    PickerMode::Browse | PickerMode::Navigate => return Ok(None),
                },

                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(None);
                }

                KeyCode::Enter => match mode {
                    PickerMode::Browse | PickerMode::Navigate => {
                        return Ok(selected.map(|index| workspaces[index].clone()));
                    }
                    PickerMode::Query => {
                        enter_navigation(
                            workspaces,
                            &query,
                            &mut matches,
                            query_origin_selection,
                            &mut selected,
                            &mut mode,
                            &mut query_error,
                        );
                        first_visible = 0;
                    }
                },

                code if code == KeyCode::Char('/')
                    || (code == KeyCode::Char('f')
                        && key.modifiers.contains(KeyModifiers::CONTROL)) =>
                {
                    match mode {
                        PickerMode::Browse => {
                            query.clear();
                            query_cursor = 0;
                            query_origin_selection = selected;
                            matches = (0..workspaces.len()).collect();
                            query_error = None;
                            mode = PickerMode::Query;
                            first_visible = 0;
                        }
                        PickerMode::Navigate => {
                            query_cursor = query.len();
                            query_origin_selection = selected;
                            query_error = None;
                            mode = PickerMode::Query;
                            first_visible = 0;
                        }
                        PickerMode::Query => {
                            if query_error.is_some() {
                                query_error = None;
                                query_cursor = query.len();
                            }
                        }
                    }
                }

                KeyCode::Backspace if mode == PickerMode::Query => {
                    if query_cursor > 0 {
                        query.remove(query_cursor - 1);
                        query_cursor -= 1;
                        update_matches(&query, workspaces, &mut matches);
                        query_error = None;
                        first_visible = 0;
                    }
                }

                KeyCode::Delete if mode == PickerMode::Query => {
                    if query_cursor < query.len() {
                        query.remove(query_cursor);
                        update_matches(&query, workspaces, &mut matches);
                        query_error = None;
                        first_visible = 0;
                    }
                }

                KeyCode::Left if mode == PickerMode::Query => {
                    query_cursor = query_cursor.saturating_sub(1);
                    query_error = None;
                }

                KeyCode::Right if mode == PickerMode::Query => {
                    query_cursor = (query_cursor + 1).min(query.len());
                    query_error = None;
                }

                KeyCode::Home if mode == PickerMode::Query => {
                    query_cursor = 0;
                    query_error = None;
                }

                KeyCode::End if mode == PickerMode::Query => {
                    query_cursor = query.len();
                    query_error = None;
                }

                KeyCode::Char(character)
                    if mode == PickerMode::Query
                        && !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    query.insert(query_cursor, character);
                    query_cursor += 1;
                    update_matches(&query, workspaces, &mut matches);
                    query_error = None;
                    first_visible = 0;
                }
                _ => {}
            },
            _ => {}
        }
    }
}

fn filter_workspaces(workspaces: &[String], query: &[char]) -> Result<Vec<usize>, PatternError> {
    let pattern = parse_pattern(query)?;
    Ok(workspaces
        .iter()
        .enumerate()
        .filter_map(|(index, name)| pattern.matches(name).then_some(index))
        .collect())
}

fn parse_pattern(query: &[char]) -> Result<SearchPattern, PatternError> {
    let anchored_start = query.first() == Some(&'^');
    let start = usize::from(anchored_start);
    let anchored_end = query
        .last()
        .is_some_and(|last| *last == '$' && !is_escaped(query, query.len() - 1));
    let end = query.len() - usize::from(anchored_end);
    let mut tokens = Vec::new();
    let mut index = start;

    while index < end {
        match query[index] {
            '\\' => {
                if index + 1 == end {
                    return Err(PatternError::IncompleteEscape);
                }
                tokens.push(PatternToken::Literal(query[index + 1]));
                index += 2;
            }
            '.' => {
                tokens.push(PatternToken::Any);
                index += 1;
            }
            character => {
                tokens.push(PatternToken::Literal(character));
                index += 1;
            }
        }
    }

    Ok(SearchPattern {
        tokens,
        anchored_start,
        anchored_end,
    })
}

fn is_escaped(query: &[char], index: usize) -> bool {
    let mut backslashes = 0;
    let mut previous = index;
    while previous > 0 && query[previous - 1] == '\\' {
        backslashes += 1;
        previous -= 1;
    }
    backslashes % 2 == 1
}

impl SearchPattern {
    fn matches(&self, name: &str) -> bool {
        let name: Vec<char> = name.chars().collect();
        let token_count = self.tokens.len();
        if name.len() < token_count {
            return false;
        }

        if self.anchored_start && self.anchored_end {
            return name.len() == token_count && self.matches_at(&name, 0);
        }
        if self.anchored_start {
            return self.matches_at(&name, 0);
        }
        if self.anchored_end {
            return self.matches_at(&name, name.len() - token_count);
        }
        (0..=name.len() - token_count).any(|start| self.matches_at(&name, start))
    }

    fn matches_at(&self, name: &[char], start: usize) -> bool {
        self.tokens
            .iter()
            .zip(&name[start..start + self.tokens.len()])
            .all(|(token, character)| match token {
                PatternToken::Literal(expected) => expected.eq_ignore_ascii_case(character),
                PatternToken::Any => true,
            })
    }
}

fn update_matches(query: &[char], workspaces: &[String], matches: &mut Vec<usize>) {
    match filter_workspaces(workspaces, query) {
        Ok(updated) => *matches = updated,
        // A trailing escape is pending input; keep the last complete preview.
        Err(PatternError::IncompleteEscape) => {}
    }
}

fn enter_navigation(
    workspaces: &[String],
    query: &[char],
    matches: &mut Vec<usize>,
    origin_selection: Option<usize>,
    selected: &mut Option<usize>,
    mode: &mut PickerMode,
    error: &mut Option<QueryError>,
) {
    let updated = match filter_workspaces(workspaces, query) {
        Ok(updated) => updated,
        Err(PatternError::IncompleteEscape) => {
            *error = Some(QueryError::IncompleteEscape);
            return;
        }
    };
    if updated.is_empty() {
        *matches = updated;
        *error = Some(QueryError::NoMatches);
        return;
    }

    *matches = updated;
    *selected = next_match_after(matches, origin_selection);
    *mode = PickerMode::Navigate;
    *error = None;
}

fn next_match_after(matches: &[usize], origin: Option<usize>) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    let Some(origin) = origin else {
        return matches.first().copied();
    };
    matches
        .iter()
        .copied()
        .find(|index| *index > origin)
        .or_else(|| matches.first().copied())
}

fn move_match(selected: Option<usize>, matches: &[usize], direction: Direction) -> usize {
    let position = matches
        .iter()
        .position(|index| Some(*index) == selected)
        .unwrap_or(0);
    match direction {
        Direction::Up => matches[(position + matches.len() - 1) % matches.len()],
        Direction::Down => matches[(position + 1) % matches.len()],
    }
}

fn ensure_visible(selected: usize, first: usize, len: usize, height: usize) -> usize {
    let first = first.min(len.saturating_sub(height));
    if selected < first {
        selected
    } else if selected >= first.saturating_add(height) {
        selected.saturating_add(1).saturating_sub(height)
    } else {
        first
    }
}

/// Rows an inline picker occupies: the list plus a header and a footer,
/// never more than the window has.
fn inline_block_rows(list_height: usize, terminal_rows: u16) -> u16 {
    let rows = list_height
        .saturating_add(2)
        .min(usize::from(terminal_rows));
    u16::try_from(rows).unwrap_or(terminal_rows)
}

/// Cut `text` to the window width so a row never wraps into the next one.
fn fit(text: &str, cols: u16) -> String {
    text.chars().take(usize::from(cols)).collect()
}

fn query_prompt(query: &[char], cursor: usize, cols: u16) -> (String, u16) {
    let width = usize::from(cols);
    if width == 0 {
        return (String::new(), 0);
    }

    let cursor = cursor.min(query.len());
    if width == 1 {
        return ("/".to_owned(), 0);
    }

    let query_width = width.saturating_sub(2);
    let start = cursor
        .saturating_sub(query_width / 2)
        .min(query.len().saturating_sub(query_width));
    let end = start.saturating_add(query_width).min(query.len());
    let prefix = if start == 0 { '/' } else { '<' };
    let visible_query: String = query[start..end].iter().collect();
    let prompt = format!("{prefix}{visible_query}");
    let cursor_column = (1 + cursor.saturating_sub(start)).min(width - 1);
    (prompt, cursor_column as u16)
}

fn viewport_height(rows: u16) -> usize {
    usize::from(rows.saturating_sub(4)).max(1)
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
    let max_first = len.saturating_sub(height);
    let centered = index.saturating_sub(height / 2);
    // Center the selection on the viewport, but keep the scroll sticky: never
    // scroll backward past the current `first`, and always stay within the
    // valid first-row range. The final `clamp` owns the range invariant, so a
    // future tweak to the centering math cannot let the view scroll past the
    // end of the list.
    centered.max(first).clamp(0, max_first)
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

fn render(state: RenderState<'_>) -> Result<(), Box<dyn Error>> {
    let RenderState {
        workspaces,
        display_indices,
        selected,
        first,
        height,
        terminal_size: (cols, terminal_rows),
        display,
        mode,
        query,
        query_cursor,
        query_error,
    } = state;
    let mut output = io::stderr().lock();
    match display {
        DisplayMode::Alternate => {
            queue!(
                output,
                MoveTo(0, 0),
                Clear(ClearType::All),
                Print(fit("xws — select a workspace", cols))
            )?;
        }
        DisplayMode::Inline => {
            queue!(
                output,
                SavePosition,
                MoveToColumn(0),
                Clear(ClearType::CurrentLine),
                Print(fit("xws — select a workspace", cols)),
                MoveToNextLine(1)
            )?;
        }
    }
    for row in 0..height {
        match display {
            DisplayMode::Alternate => queue!(output, MoveTo(0, (row + 1) as u16))?,
            DisplayMode::Inline => queue!(output, Clear(ClearType::CurrentLine))?,
        }
        if let Some(name) = display_indices
            .get(first + row)
            .and_then(|index| workspaces.get(*index))
        {
            if selected == Some(first + row) {
                queue!(
                    output,
                    SetAttribute(Attribute::Reverse),
                    Print(fit(&format!(" > {name} "), cols)),
                    SetAttribute(Attribute::Reset)
                )?;
            } else {
                queue!(output, Print(fit(&format!("   {name}"), cols)))?;
            }
        } else if row == 0 && mode != PickerMode::Browse && display_indices.is_empty() {
            queue!(output, Print(fit("   (no matching workspaces)", cols)))?;
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
        if let Some(error) = query_error {
            queue!(
                output,
                SetForegroundColor(Color::Red),
                Print(fit(&error.message(query), cols)),
                ResetColor
            )?;
        } else {
            match mode {
                PickerMode::Browse => queue!(
                    output,
                    Print(fit(
                        "↑/↓ or Ctrl-P/Ctrl-N move, Enter select, / or Ctrl-F search, Esc/Ctrl-C cancel",
                        cols
                    ))
                )?,
                PickerMode::Query => {
                    let (prompt, cursor) = query_prompt(query, query_cursor, cols);
                    queue!(output, Print(prompt))?;
                    if cols > 0 {
                        queue!(
                            output,
                            MoveToColumn(cursor),
                            SetAttribute(Attribute::Reverse),
                            Print(" "),
                            SetAttribute(Attribute::Reset)
                        )?;
                    }
                    let help_start = usize::from(cursor).saturating_add(1);
                    if query_cursor == query.len() && help_start < usize::from(cols) {
                        queue!(
                            output,
                            MoveToColumn(help_start as u16),
                            Print(fit(
                                " Enter/↑↓ navigate, Esc reset",
                                cols.saturating_sub(help_start as u16)
                            ))
                        )?;
                    }
                }
                PickerMode::Navigate => {
                    let query: String = query.iter().collect();
                    queue!(
                        output,
                        Print(fit(
                            &format!(
                                "/{query}  ↑/↓ or Ctrl-P/N next, Enter select, / or Ctrl-F edit, Esc cancel"
                            ),
                            cols
                        ))
                    )?;
                }
            }
        }
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

    fn chars(value: &str) -> Vec<char> {
        value.chars().collect()
    }

    fn names(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn search_patterns_match_literals_anchors_and_wildcards() {
        let workspaces = names(&["alpha", "beta", "example.com", "exampleXcom", "api.test"]);

        assert_eq!(
            filter_workspaces(&workspaces, &chars("ALP")).unwrap(),
            vec![0]
        );
        assert_eq!(
            filter_workspaces(&workspaces, &chars("^api")).unwrap(),
            vec![4]
        );
        assert_eq!(
            filter_workspaces(&workspaces, &chars(".com$")).unwrap(),
            vec![2, 3]
        );
        assert_eq!(
            filter_workspaces(&workspaces, &chars(r"^example\.com$")).unwrap(),
            vec![2]
        );
    }

    #[test]
    fn empty_query_and_single_character_wildcard_match_all_names() {
        let workspaces = names(&["alpha", "beta", "example.com"]);

        assert_eq!(filter_workspaces(&workspaces, &[]).unwrap(), vec![0, 1, 2]);
        assert_eq!(
            filter_workspaces(&workspaces, &chars(".")).unwrap(),
            vec![0, 1, 2]
        );
        assert_eq!(
            filter_workspaces(&workspaces, &chars("^")).unwrap(),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn only_a_trailing_unpaired_escape_is_incomplete() {
        assert_eq!(
            parse_pattern(&chars("example\\")).unwrap_err(),
            PatternError::IncompleteEscape
        );
        assert!(parse_pattern(&chars(r"example\\")).is_ok());
        assert!(parse_pattern(&chars(r"example\$")).is_ok());
    }

    #[test]
    fn query_render_mode_accepts_only_supported_values() {
        assert!(matches!(
            QueryRenderMode::parse("filtered", "config"),
            Ok(QueryRenderMode::Filtered)
        ));
        assert!(matches!(
            QueryRenderMode::parse("full", "config"),
            Ok(QueryRenderMode::Full)
        ));
        assert!(QueryRenderMode::parse("other", "config").is_err());
    }

    #[test]
    fn navigation_starts_after_the_previous_selection_and_wraps() {
        let matches = [1, 3, 5];

        assert_eq!(next_match_after(&matches, Some(0)), Some(1));
        assert_eq!(next_match_after(&matches, Some(3)), Some(5));
        assert_eq!(next_match_after(&matches, Some(5)), Some(1));
        assert_eq!(next_match_after(&matches, None), Some(1));
        assert_eq!(next_match_after(&[], Some(5)), None);
        assert_eq!(move_match(Some(1), &matches, Direction::Up), 5);
        assert_eq!(move_match(Some(5), &matches, Direction::Down), 1);
    }

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
    fn center_active_centers_and_stays_within_range() {
        // First display: center the selection on a viewport that fits.
        assert_eq!(center_active(10, 0, 20, 6), 7);

        // Sticky: centering would scroll backward past the current `first`, so
        // it is kept in place.
        assert_eq!(center_active(3, 5, 20, 6), 5);

        // Load-bearing case: a window shrink leaves a stored `first` beyond the
        // new `max_first`; the terminal `clamp` pulls it back to the end of the
        // list rather than scrolling past it.
        assert_eq!(center_active(19, 15, 20, 6), 14);

        // Never leaves the valid first-row range, even at the top.
        assert_eq!(center_active(0, 0, 20, 6), 0);
        assert_eq!(center_active(2, 0, 20, 6), 0);
    }

    #[test]
    fn picker_reserves_two_additional_terminal_rows() {
        assert_eq!(viewport_height(24), 20);
        assert_eq!(viewport_height(3), 1);
    }
}
