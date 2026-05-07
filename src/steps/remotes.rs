use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};

use crate::widgets::text_input::TextInput;
use crate::{ProjectConfig, RemoteConfig, RemoteProvider};

use super::vcs::branch_name_problem;
use super::{CURSOR_BLANK, CURSOR_MARKER, Focus, StepHandler, StepResult};

const INDENT_SUBPANEL: u16 = 4;
const CONFIRM_BUTTON_WIDTH: u16 = 12;
const CONFIRM_BUTTON_HEIGHT: u16 = 3;

const PROVIDER_CHOICES: [RemoteProvider; 5] = [
    RemoteProvider::GitHub,
    RemoteProvider::Codeberg,
    RemoteProvider::GitLab,
    RemoteProvider::Bitbucket,
    RemoteProvider::SelfHosted,
];

pub(crate) fn remote_name_problem(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return Some("Remote name cannot be empty.");
    }
    branch_name_problem(name)
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum NavRow {
    Name,
    Location,
    Operations,
    Confirm,
}


const NAV_ROWS: [NavRow; 4] = [NavRow::Name, NavRow::Location, NavRow::Operations, NavRow::Confirm];

#[derive(Debug, Clone, Copy, PartialEq)]
enum CursorTarget {
    Next,
    Configured(usize),
    Provider(RemoteProvider),
    NoRemotes,
}

#[derive(Debug)]
pub struct RemotesHandler {
    cursor: usize,
    configured: Vec<RemoteConfig>,
    expanded: Option<usize>,
    focus: Focus,
    row_cursor: usize,
    col_cursor: usize,
    name_input: TextInput,
    location_input: TextInput,
}

impl Default for RemotesHandler {
    fn default() -> Self {
        Self {
            cursor: 0,
            configured: Vec::new(),
            expanded: None,
            focus: Focus::Choice,
            row_cursor: 0,
            col_cursor: 0,
            name_input: TextInput::new("Name"),
            location_input: TextInput::new("Location"),
        }
    }
}

impl RemotesHandler {
    pub fn restore_from_config(&mut self, config: &ProjectConfig) {
        self.configured = config.remotes.clone();
        self.cursor = 0;
        self.expanded = None;
        self.focus = Focus::Choice;
        self.row_cursor = 0;
        self.col_cursor = 0;
        self.name_input = TextInput::new("Name");
        self.location_input = TextInput::new("Location");
    }

    fn choice_count(&self) -> usize {
        1 + self.configured.len() + PROVIDER_CHOICES.len() + 1
    }

    fn cursor_target(&self) -> CursorTarget {
        if self.cursor == 0 {
            return CursorTarget::Next;
        }
        let configured_end = 1 + self.configured.len();
        if self.cursor < configured_end {
            return CursorTarget::Configured(self.cursor - 1);
        }
        let provider_end = configured_end + PROVIDER_CHOICES.len();
        if self.cursor < provider_end {
            return CursorTarget::Provider(PROVIDER_CHOICES[self.cursor - configured_end]);
        }
        CursorTarget::NoRemotes
    }

    fn expand(&mut self, idx: usize) {
        self.expanded = Some(idx);
        self.focus = Focus::SubField(0);
        self.row_cursor = 0;
        self.col_cursor = 0;
        self.name_input = TextInput::new("Name");
        self.name_input.set_value(self.configured[idx].name.clone());
        self.location_input = TextInput::new("Location");
        self.location_input
            .set_value(self.configured[idx].location.clone());
    }

    fn sync_inputs_to_scratch(&mut self) {
        if let Some(idx) = self.expanded {
            self.configured[idx].name = self.name_input.value().to_string();
            self.configured[idx].location = self.location_input.value().to_string();
        }
    }

    fn collapse_persist(&mut self) {
        self.sync_inputs_to_scratch();
        self.expanded = None;
        self.focus = Focus::Choice;
    }

    fn add_remote(&mut self, provider: RemoteProvider) {
        let name = if self.configured.is_empty() {
            "origin".to_string()
        } else {
            String::new()
        };
        let location = provider.url_prefix().to_string();
        self.configured.push(RemoteConfig {
            provider,
            name,
            location,
            fetch: true,
            push: true,
        });
        let idx = self.configured.len() - 1;
        self.cursor = 1 + idx;
        self.expand(idx);
    }

    fn remove_configured(&mut self, idx: usize) {
        if self.expanded == Some(idx) {
            self.expanded = None;
            self.focus = Focus::Choice;
        } else if let Some(exp) = self.expanded
            && exp > idx
        {
            self.expanded = Some(exp - 1);
        }
        self.configured.remove(idx);
        if self.cursor >= self.choice_count() {
            self.cursor = self.choice_count().saturating_sub(1);
        }
    }

    fn commit_to_config(&mut self, config: &mut ProjectConfig) {
        self.sync_inputs_to_scratch();
        config.remotes = self.configured.clone();
    }

    fn validate_remote(remote: &RemoteConfig) -> Option<String> {
        if let Some(msg) = remote_name_problem(&remote.name) {
            return Some(format!("{}: {msg}", remote.provider));
        }
        if remote.location.is_empty() {
            return Some(format!("{}: Location cannot be empty.", remote.provider));
        }
        if !remote.fetch && !remote.push {
            return Some(format!(
                "{}: Must have at least one operation (fetch or push).",
                remote.provider
            ));
        }
        None
    }

    fn validate_all(&self) -> Option<String> {
        for remote in &self.configured {
            if let Some(msg) = Self::validate_remote(remote) {
                return Some(msg);
            }
        }
        None
    }

    fn validate_current_live(&self) -> Option<&'static str> {
        let name = self.name_input.value();
        if let Some(msg) = remote_name_problem(name) {
            return Some(msg);
        }
        if self.location_input.value().is_empty() {
            return Some("Location cannot be empty.");
        }
        if let Some(idx) = self.expanded
            && !self.configured[idx].fetch && !self.configured[idx].push
        {
            return Some("Must have at least one operation (fetch or push).");
        }
        None
    }

    fn current_row(&self) -> NavRow {
        NAV_ROWS[self.row_cursor]
    }

    const FLAT_NAV: [(usize, usize); 5] = [(0, 0), (1, 0), (2, 0), (2, 1), (3, 0)];

    fn advance_flattened(&mut self, backward: bool) {
        let pos = Self::FLAT_NAV
            .iter()
            .position(|(r, c)| *r == self.row_cursor && *c == self.col_cursor)
            .unwrap_or(0);
        let next = if backward {
            if pos == 0 { Self::FLAT_NAV.len() - 1 } else { pos - 1 }
        } else {
            (pos + 1) % Self::FLAT_NAV.len()
        };
        let (r, c) = Self::FLAT_NAV[next];
        self.row_cursor = r;
        self.col_cursor = c;
    }

    // --- Input handling ---

    fn handle_choice_input(&mut self, key: KeyCode, config: &mut ProjectConfig) -> StepResult {
        match key {
            KeyCode::Char('q') => StepResult::Quit,
            KeyCode::Down | KeyCode::Char('j') => {
                if self.cursor + 1 < self.choice_count() {
                    self.cursor += 1;
                }
                StepResult::Continue
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.cursor = self.cursor.saturating_sub(1);
                StepResult::Continue
            }
            KeyCode::Char(' ') => {
                if let CursorTarget::Configured(idx) = self.cursor_target() {
                    self.remove_configured(idx);
                }
                StepResult::Continue
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => match self.cursor_target() {
                CursorTarget::Next => {
                    if self.validate_all().is_some() {
                        return StepResult::Continue;
                    }
                    self.commit_to_config(config);
                    StepResult::Done
                }
                CursorTarget::Configured(idx) => {
                    self.expand(idx);
                    StepResult::Continue
                }
                CursorTarget::Provider(provider) => {
                    self.add_remote(provider);
                    StepResult::Continue
                }
                CursorTarget::NoRemotes => {
                    self.configured.clear();
                    self.commit_to_config(config);
                    StepResult::Done
                }
            },
            KeyCode::Left | KeyCode::Char('h') => {
                if self.expanded.is_some() {
                    self.collapse_persist();
                    StepResult::Continue
                } else {
                    StepResult::Back
                }
            }
            _ => StepResult::Continue,
        }
    }

    fn handle_subfield_input(&mut self, key: KeyEvent) -> StepResult {
        let row = self.current_row();
        let on_confirm = matches!(row, NavRow::Confirm);

        match key.code {
            KeyCode::Esc => {
                self.sync_inputs_to_scratch();
                self.focus = Focus::Choice;
                return StepResult::Continue;
            }
            KeyCode::Up => {
                if self.row_cursor == 0 {
                    self.sync_inputs_to_scratch();
                    self.focus = Focus::Choice;
                } else {
                    self.row_cursor -= 1;
                    self.col_cursor = 0;
                }
                return StepResult::Continue;
            }
            KeyCode::Down => {
                if self.row_cursor + 1 < NAV_ROWS.len() {
                    self.row_cursor += 1;
                    self.col_cursor = 0;
                }
                return StepResult::Continue;
            }
            KeyCode::Tab => {
                self.advance_flattened(false);
                return StepResult::Continue;
            }
            KeyCode::BackTab => {
                self.advance_flattened(true);
                return StepResult::Continue;
            }
            _ => {}
        }

        if on_confirm {
            match key.code {
                KeyCode::Char('q') => return StepResult::Quit,
                KeyCode::Char(' ') | KeyCode::Enter => {
                    if self.validate_current_live().is_none() {
                        self.collapse_persist();
                    }
                }
                KeyCode::Char('k') => {
                    if self.row_cursor > 0 {
                        self.row_cursor -= 1;
                        self.col_cursor = 0;
                    }
                }
                KeyCode::Char('j') => {}
                _ => {}
            }
            return StepResult::Continue;
        }

        match row {
            NavRow::Name | NavRow::Location => {
                let input = if matches!(row, NavRow::Name) {
                    &mut self.name_input
                } else {
                    &mut self.location_input
                };
                match key.code {
                    KeyCode::Enter => {
                        if self.validate_current_live().is_none() {
                            self.collapse_persist();
                        }
                        return StepResult::Continue;
                    }
                    KeyCode::Char(c) => {
                        input.handle_input(KeyCode::Char(c));
                    }
                    KeyCode::Backspace | KeyCode::Delete | KeyCode::Left | KeyCode::Right
                    | KeyCode::Home | KeyCode::End => {
                        input.handle_input(key.code);
                    }
                    _ => {}
                }
                self.sync_inputs_to_scratch();
                StepResult::Continue
            }
            NavRow::Operations => self.handle_operations_key(key.code),
            NavRow::Confirm => unreachable!(),
        }
    }

    fn handle_operations_key(&mut self, key: KeyCode) -> StepResult {
        match key {
            KeyCode::Char('q') => return StepResult::Quit,
            KeyCode::Left | KeyCode::Char('h') => {
                self.col_cursor = 0;
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.col_cursor = 1;
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                if let Some(idx) = self.expanded {
                    match self.col_cursor {
                        0 => self.configured[idx].fetch = !self.configured[idx].fetch,
                        1 => self.configured[idx].push = !self.configured[idx].push,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        StepResult::Continue
    }

    // --- Rendering ---

    fn render_next_line(&self, frame: &mut Frame, area: Rect) {
        let highlighted = matches!(self.focus, Focus::Choice) && self.cursor == 0;
        let cursor_marker = if highlighted { CURSOR_MARKER } else { CURSOR_BLANK };
        let style = Style::default().add_modifier(Modifier::BOLD);
        let text = format!("{cursor_marker}Next →");
        frame.render_widget(Paragraph::new(Line::from(text).style(style)), area);
    }

    fn render_configured_line(&self, frame: &mut Frame, area: Rect, idx: usize) {
        let remote = &self.configured[idx];
        let cursor_idx = 1 + idx;
        let highlighted = matches!(self.focus, Focus::Choice) && self.cursor == cursor_idx;
        let cursor_marker = if highlighted { CURSOR_MARKER } else { CURSOR_BLANK };
        let ops = match (remote.fetch, remote.push) {
            (true, true) => "[F][P]",
            (true, false) => "[F]",
            (false, true) => "[P]",
            (false, false) => "[ ]",
        };
        let truncated = remote.location.len() > 30;
        let loc_display: &str = if truncated {
            &remote.location[..27]
        } else {
            &remote.location
        };
        let ellipsis = if truncated { "..." } else { "" };
        let text = format!(
            "{cursor_marker}[x] {} — {} ({loc_display}{ellipsis}) {ops}",
            remote.name, remote.provider
        );
        frame.render_widget(Paragraph::new(Line::from(text)), area);
    }

    fn render_provider_line(
        &self,
        frame: &mut Frame,
        area: Rect,
        provider: RemoteProvider,
        provider_idx: usize,
    ) {
        let configured_end = 1 + self.configured.len();
        let cursor_idx = configured_end + provider_idx;
        let highlighted = matches!(self.focus, Focus::Choice) && self.cursor == cursor_idx;
        let cursor_marker = if highlighted { CURSOR_MARKER } else { CURSOR_BLANK };
        let text = format!("{cursor_marker}{provider}");
        frame.render_widget(Paragraph::new(Line::from(text)), area);
    }

    fn render_no_remotes_line(&self, frame: &mut Frame, area: Rect) {
        let cursor_idx = self.choice_count() - 1;
        let highlighted = matches!(self.focus, Focus::Choice) && self.cursor == cursor_idx;
        let cursor_marker = if highlighted { CURSOR_MARKER } else { CURSOR_BLANK };
        let text = format!("{cursor_marker}No remotes");
        frame.render_widget(Paragraph::new(Line::from(text)), area);
    }

    fn render_section_header(&self, frame: &mut Frame, area: Rect, label: &str) {
        let style = Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD);
        let text = format!("  --- {label} ---");
        frame.render_widget(Paragraph::new(Line::from(text).style(style)), area);
    }

    fn render_confirm_button(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let block_style = if focused {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let block = Block::bordered().style(block_style);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let label_style = if focused {
            Style::default().fg(Color::Black).bg(Color::White)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        frame.render_widget(
            Paragraph::new(Line::from("Confirm").style(label_style)).centered(),
            inner,
        );
    }

    fn render_operations_row(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let Some(idx) = self.expanded else {
            return;
        };
        let remote = &self.configured[idx];
        let mut spans = vec![Span::raw("Operations: ")];
        let items = [("Fetch", remote.fetch), ("Push", remote.push)];
        for (i, (label, checked)) in items.iter().enumerate() {
            let check = if *checked { "[x]" } else { "[ ]" };
            let cell = format!("{check} {label}");
            let style = if focused && self.col_cursor == i {
                Style::default().fg(Color::Black).bg(Color::White)
            } else if focused {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            spans.push(Span::styled(cell, style));
            if i == 0 {
                spans.push(Span::raw("   "));
            }
        }
        let line_style = if focused {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        frame.render_widget(Paragraph::new(Line::from(spans).style(line_style)), area);
    }

    fn render_expanded_panel(&self, frame: &mut Frame, area: Rect) -> u16 {
        let mut y = area.y;
        let bottom = area.y + area.height;
        let focused_row =
            |r: usize| matches!(self.focus, Focus::SubField(_)) && self.row_cursor == r;

        for (idx, row) in NAV_ROWS.iter().enumerate() {
            match *row {
                NavRow::Name => {
                    if y + 2 >= bottom {
                        return y;
                    }
                    let row_area = Rect {
                        x: area.x + INDENT_SUBPANEL,
                        y,
                        width: area.width.saturating_sub(INDENT_SUBPANEL),
                        height: 3,
                    };
                    self.name_input.render(frame, row_area, focused_row(idx));
                    y += 3;
                }
                NavRow::Location => {
                    if y + 2 >= bottom {
                        return y;
                    }
                    let row_area = Rect {
                        x: area.x + INDENT_SUBPANEL,
                        y,
                        width: area.width.saturating_sub(INDENT_SUBPANEL),
                        height: 3,
                    };
                    self.location_input.render(frame, row_area, focused_row(idx));
                    y += 3;
                }
                NavRow::Operations => {
                    if y >= bottom {
                        return y;
                    }
                    let row_area = Rect {
                        x: area.x + INDENT_SUBPANEL,
                        y,
                        width: area.width.saturating_sub(INDENT_SUBPANEL),
                        height: 1,
                    };
                    self.render_operations_row(frame, row_area, focused_row(idx));
                    y += 1;
                }
                NavRow::Confirm => {
                    if let Some(msg) = self.validate_current_live()
                        && y < bottom
                    {
                        let rect = Rect {
                            x: area.x + INDENT_SUBPANEL,
                            y,
                            width: area.width.saturating_sub(INDENT_SUBPANEL),
                            height: 1,
                        };
                        frame.render_widget(
                            Paragraph::new(
                                Line::from(format!("⚠  {msg}"))
                                    .style(Style::default().fg(Color::Yellow)),
                            ),
                            rect,
                        );
                        y += 1;
                    }
                    if y + CONFIRM_BUTTON_HEIGHT <= bottom {
                        let button_rect = Rect {
                            x: area.x + INDENT_SUBPANEL,
                            y,
                            width: CONFIRM_BUTTON_WIDTH
                                .min(area.width.saturating_sub(INDENT_SUBPANEL)),
                            height: CONFIRM_BUTTON_HEIGHT,
                        };
                        self.render_confirm_button(frame, button_rect, focused_row(idx));
                        y += CONFIRM_BUTTON_HEIGHT;
                    }
                }
            }
        }
        y
    }
}

impl StepHandler for RemotesHandler {
    fn render(&self, frame: &mut Frame, area: Rect) {
        let mut y = area.y;
        let bottom = area.y + area.height;

        // Next row
        if y < bottom {
            let rect = Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            };
            self.render_next_line(frame, rect);
            y += 1;
        }

        // Configured section
        if !self.configured.is_empty() {
            if y < bottom {
                let rect = Rect {
                    x: area.x,
                    y,
                    width: area.width,
                    height: 1,
                };
                self.render_section_header(frame, rect, "Configured");
                y += 1;
            }
            for idx in 0..self.configured.len() {
                if y >= bottom {
                    break;
                }
                let rect = Rect {
                    x: area.x,
                    y,
                    width: area.width,
                    height: 1,
                };
                self.render_configured_line(frame, rect, idx);
                y += 1;

                if self.expanded == Some(idx) {
                    let panel_area = Rect {
                        x: area.x,
                        y,
                        width: area.width,
                        height: bottom.saturating_sub(y),
                    };
                    y = self.render_expanded_panel(frame, panel_area);
                }
            }
        }

        // Add remote section
        if y < bottom {
            let rect = Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            };
            self.render_section_header(frame, rect, "Add remote");
            y += 1;
        }
        for (i, provider) in PROVIDER_CHOICES.iter().enumerate() {
            if y >= bottom {
                break;
            }
            let rect = Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            };
            self.render_provider_line(frame, rect, *provider, i);
            y += 1;
        }

        // No remotes
        if y < bottom {
            let rect = Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            };
            self.render_no_remotes_line(frame, rect);
            y += 1;
        }

        // Choice-level warning
        if matches!(self.focus, Focus::Choice)
            && let Some(msg) = self.validate_all()
            && y < bottom
        {
            let rect = Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            };
            frame.render_widget(
                Paragraph::new(
                    Line::from(format!("⚠  Cannot advance: {msg}"))
                        .style(Style::default().fg(Color::Yellow)),
                ),
                rect,
            );
        }
    }

    fn handle_input(&mut self, key: KeyEvent, config: &mut ProjectConfig) -> StepResult {
        match self.focus {
            Focus::Choice => self.handle_choice_input(key.code, config),
            Focus::SubField(_) => self.handle_subfield_input(key),
            Focus::Browsing => StepResult::Continue,
        }
    }

    fn in_details(&self) -> bool {
        matches!(self.focus, Focus::SubField(_))
    }

    fn is_expanded(&self) -> bool {
        self.expanded.is_some()
    }

    fn planned_actions(&self, config: &ProjectConfig) -> Vec<String> {
        config
            .remotes
            .iter()
            .map(|r| {
                let ops = match (r.fetch, r.push) {
                    (true, true) => "fetch+push",
                    (true, false) => "fetch",
                    (false, true) => "push",
                    _ => "none",
                };
                format!("Add remote '{}' → {} ({ops})", r.name, r.location)
            })
            .collect()
    }

    fn execute(&self, _config: &ProjectConfig) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn config() -> ProjectConfig {
        ProjectConfig::default()
    }

    // --- Defaults ---

    #[test]
    fn default_state() {
        let h = RemotesHandler::default();
        assert_eq!(h.focus, Focus::Choice);
        assert!(h.expanded.is_none());
        assert_eq!(h.cursor, 0);
        assert!(h.configured.is_empty());
        assert!(!h.in_details());
        assert!(!h.is_expanded());
    }

    // --- Cursor navigation ---

    #[test]
    fn cursor_down_and_up() {
        let mut h = RemotesHandler::default();
        let mut c = config();
        h.handle_input(key(KeyCode::Down), &mut c);
        assert_eq!(h.cursor, 1);
        h.handle_input(key(KeyCode::Up), &mut c);
        assert_eq!(h.cursor, 0);
    }

    #[test]
    fn cursor_clamps_at_bounds() {
        let mut h = RemotesHandler::default();
        let mut c = config();
        for _ in 0..50 {
            h.handle_input(key(KeyCode::Down), &mut c);
        }
        assert_eq!(h.cursor, h.choice_count() - 1);
        for _ in 0..50 {
            h.handle_input(key(KeyCode::Up), &mut c);
        }
        assert_eq!(h.cursor, 0);
    }

    // --- Next row ---

    #[test]
    fn enter_on_next_with_no_remotes_advances() {
        let mut h = RemotesHandler::default();
        let mut c = config();
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Done));
        assert!(c.remotes.is_empty());
    }

    #[test]
    fn enter_on_next_with_valid_remote_commits() {
        let mut h = RemotesHandler::default();
        h.configured.push(RemoteConfig {
            provider: RemoteProvider::GitHub,
            name: "origin".to_string(),
            location: "git@github.com:user/repo".to_string(),
            fetch: true,
            push: true,
        });
        let mut c = config();
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Done));
        assert_eq!(c.remotes.len(), 1);
        assert_eq!(c.remotes[0].name, "origin");
    }

    #[test]
    fn enter_on_next_blocked_by_invalid_remote() {
        let mut h = RemotesHandler::default();
        h.configured.push(RemoteConfig {
            provider: RemoteProvider::GitHub,
            name: String::new(),
            location: "git@github.com:user/repo".to_string(),
            fetch: true,
            push: true,
        });
        let mut c = config();
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Continue));
        assert!(c.remotes.is_empty());
    }

    // --- Add remote via provider ---

    #[test]
    fn enter_on_provider_adds_and_expands() {
        let mut h = RemotesHandler::default();
        let mut c = config();
        // Move to first provider (index = 1 + 0 configured + 0 = 1)
        h.cursor = 1; // GitHub
        h.handle_input(key(KeyCode::Enter), &mut c);
        assert_eq!(h.configured.len(), 1);
        assert_eq!(h.configured[0].provider, RemoteProvider::GitHub);
        assert_eq!(h.configured[0].name, "origin");
        assert!(h.configured[0].location.starts_with("git@github.com:"));
        assert!(h.configured[0].fetch);
        assert!(h.configured[0].push);
        assert_eq!(h.expanded, Some(0));
        assert_eq!(h.focus, Focus::SubField(0));
    }

    #[test]
    fn second_remote_gets_empty_name() {
        let mut h = RemotesHandler::default();
        let mut c = config();
        h.cursor = 1;
        h.handle_input(key(KeyCode::Enter), &mut c); // add first
        h.collapse_persist();
        // Now cursor to second provider (configured shifts things)
        h.cursor = 1 + h.configured.len(); // first provider after configured
        h.handle_input(key(KeyCode::Enter), &mut c);
        assert_eq!(h.configured.len(), 2);
        assert_eq!(h.configured[1].name, "");
    }

    // --- No remotes ---

    #[test]
    fn no_remotes_clears_and_advances() {
        let mut h = RemotesHandler::default();
        h.configured.push(RemoteConfig {
            provider: RemoteProvider::GitHub,
            name: "origin".to_string(),
            location: "git@github.com:test".to_string(),
            fetch: true,
            push: true,
        });
        let mut c = config();
        c.remotes.push(RemoteConfig {
            provider: RemoteProvider::GitHub,
            name: "origin".to_string(),
            location: "git@github.com:test".to_string(),
            fetch: true,
            push: true,
        });
        h.cursor = h.choice_count() - 1; // No remotes
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Done));
        assert!(h.configured.is_empty());
        assert!(c.remotes.is_empty());
    }

    // --- Remove configured ---

    #[test]
    fn space_removes_configured_remote() {
        let mut h = RemotesHandler::default();
        h.configured.push(RemoteConfig {
            provider: RemoteProvider::GitHub,
            name: "origin".to_string(),
            location: "git@github.com:test".to_string(),
            fetch: true,
            push: true,
        });
        let mut c = config();
        h.cursor = 1; // first configured
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert!(h.configured.is_empty());
    }

    #[test]
    fn space_on_provider_is_noop() {
        let mut h = RemotesHandler::default();
        let mut c = config();
        h.cursor = 1; // first provider (no configured)
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert!(h.configured.is_empty());
    }

    // --- Expand / collapse ---

    #[test]
    fn enter_on_configured_expands() {
        let mut h = RemotesHandler::default();
        h.configured.push(RemoteConfig {
            provider: RemoteProvider::GitHub,
            name: "origin".to_string(),
            location: "git@github.com:test".to_string(),
            fetch: true,
            push: true,
        });
        let mut c = config();
        h.cursor = 1;
        h.handle_input(key(KeyCode::Enter), &mut c);
        assert_eq!(h.expanded, Some(0));
        assert_eq!(h.focus, Focus::SubField(0));
        assert_eq!(h.name_input.value(), "origin");
    }

    #[test]
    fn esc_returns_to_choice() {
        let mut h = RemotesHandler::default();
        let mut c = config();
        h.cursor = 1; // first provider
        h.handle_input(key(KeyCode::Enter), &mut c); // add + expand
        assert_eq!(h.focus, Focus::SubField(0));
        h.handle_input(key(KeyCode::Esc), &mut c);
        assert_eq!(h.focus, Focus::Choice);
        assert!(h.expanded.is_some()); // stays expanded
    }

    #[test]
    fn left_collapses_expanded() {
        let mut h = RemotesHandler::default();
        h.configured.push(RemoteConfig {
            provider: RemoteProvider::GitHub,
            name: "origin".to_string(),
            location: "git@github.com:test".to_string(),
            fetch: true,
            push: true,
        });
        let mut c = config();
        h.cursor = 1;
        h.handle_input(key(KeyCode::Enter), &mut c); // expand
        h.focus = Focus::Choice; // simulate Esc first
        let result = h.handle_input(key(KeyCode::Left), &mut c);
        assert!(matches!(result, StepResult::Continue));
        assert!(h.expanded.is_none());
    }

    #[test]
    fn left_backs_when_nothing_expanded() {
        let mut h = RemotesHandler::default();
        let mut c = config();
        let result = h.handle_input(key(KeyCode::Left), &mut c);
        assert!(matches!(result, StepResult::Back));
    }

    // --- SubField navigation ---

    #[test]
    fn down_moves_through_nav_rows() {
        let mut h = RemotesHandler::default();
        let mut c = config();
        h.cursor = 1;
        h.handle_input(key(KeyCode::Enter), &mut c); // add + expand
        assert_eq!(h.row_cursor, 0); // Name
        h.handle_input(key(KeyCode::Down), &mut c);
        assert_eq!(h.row_cursor, 1); // Location
        h.handle_input(key(KeyCode::Down), &mut c);
        assert_eq!(h.row_cursor, 2); // Operations
        h.handle_input(key(KeyCode::Down), &mut c);
        assert_eq!(h.row_cursor, 3); // Confirm
    }

    #[test]
    fn up_from_row_zero_returns_to_choice() {
        let mut h = RemotesHandler::default();
        let mut c = config();
        h.cursor = 1;
        h.handle_input(key(KeyCode::Enter), &mut c);
        h.handle_input(key(KeyCode::Up), &mut c);
        assert_eq!(h.focus, Focus::Choice);
    }

    // --- Operations toggle ---

    #[test]
    fn toggle_fetch_and_push() {
        let mut h = RemotesHandler::default();
        let mut c = config();
        h.cursor = 1;
        h.handle_input(key(KeyCode::Enter), &mut c); // add + expand
        // Move to operations row
        h.row_cursor = 2;
        h.col_cursor = 0;
        // Toggle fetch off
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert!(!h.configured[0].fetch);
        assert!(h.configured[0].push);
        // Move to push
        h.handle_input(key(KeyCode::Right), &mut c);
        assert_eq!(h.col_cursor, 1);
        // Toggle push off
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert!(!h.configured[0].push);
    }

    // --- Confirm validation ---

    #[test]
    fn confirm_blocked_with_empty_name() {
        let mut h = RemotesHandler::default();
        let mut c = config();
        // Add remote with empty name (second remote)
        h.configured.push(RemoteConfig {
            provider: RemoteProvider::GitHub,
            name: "first".to_string(),
            location: "git@github.com:test".to_string(),
            fetch: true,
            push: true,
        });
        h.cursor = 1 + h.configured.len(); // provider
        h.handle_input(key(KeyCode::Enter), &mut c); // add second (empty name)
        // Move to confirm
        h.row_cursor = 3;
        h.handle_input(key(KeyCode::Enter), &mut c);
        // Should still be expanded (blocked)
        assert!(h.expanded.is_some());
    }

    #[test]
    fn confirm_succeeds_with_valid_data() {
        let mut h = RemotesHandler::default();
        let mut c = config();
        h.cursor = 1;
        h.handle_input(key(KeyCode::Enter), &mut c); // add (name="origin", has prefix)
        // origin + github prefix = valid
        h.row_cursor = 3;
        h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(h.expanded.is_none());
        assert_eq!(h.focus, Focus::Choice);
    }

    // --- Name input ---

    #[test]
    fn name_input_captures_chars() {
        let mut h = RemotesHandler::default();
        let mut c = config();
        h.cursor = 1;
        h.handle_input(key(KeyCode::Enter), &mut c);
        // Clear default name and type new one
        h.name_input = TextInput::new("Name");
        for ch in "upstream".chars() {
            h.handle_input(key(KeyCode::Char(ch)), &mut c);
        }
        assert_eq!(h.name_input.value(), "upstream");
        assert_eq!(h.configured[0].name, "upstream");
    }

    // --- Tab cycling ---

    #[test]
    fn tab_cycles_all_positions() {
        let mut h = RemotesHandler::default();
        let mut c = config();
        h.cursor = 1;
        h.handle_input(key(KeyCode::Enter), &mut c);
        // Name(1) + Location(1) + Operations(2) + Confirm(1) = 5 positions
        let mut positions = vec![(h.row_cursor, h.col_cursor)];
        for _ in 0..4 {
            h.handle_input(key(KeyCode::Tab), &mut c);
            positions.push((h.row_cursor, h.col_cursor));
        }
        assert_eq!(positions.len(), 5);
        // Should wrap
        h.handle_input(key(KeyCode::Tab), &mut c);
        assert_eq!((h.row_cursor, h.col_cursor), (0, 0));
    }

    // --- Validation ---

    #[test]
    fn remote_name_problem_empty() {
        assert!(remote_name_problem("").is_some());
    }

    #[test]
    fn remote_name_problem_valid() {
        assert!(remote_name_problem("origin").is_none());
        assert!(remote_name_problem("upstream").is_none());
        assert!(remote_name_problem("my-remote").is_none());
    }

    #[test]
    fn remote_name_problem_whitespace() {
        assert!(remote_name_problem("my remote").is_some());
    }

    #[test]
    fn remote_name_problem_special_chars() {
        assert!(remote_name_problem("my~remote").is_some());
        assert!(remote_name_problem("my:remote").is_some());
    }

    // --- Quit ---

    #[test]
    fn q_quits_at_choice() {
        let mut h = RemotesHandler::default();
        let mut c = config();
        let result = h.handle_input(key(KeyCode::Char('q')), &mut c);
        assert!(matches!(result, StepResult::Quit));
    }

    // --- Restore from config ---

    #[test]
    fn restore_from_config_rehydrates() {
        let mut h = RemotesHandler::default();
        h.configured.push(RemoteConfig {
            provider: RemoteProvider::GitHub,
            name: "stale".to_string(),
            location: "stale".to_string(),
            fetch: true,
            push: false,
        });
        let c = ProjectConfig {
            remotes: vec![RemoteConfig {
                provider: RemoteProvider::GitLab,
                name: "origin".to_string(),
                location: "git@gitlab.com:org/repo".to_string(),
                fetch: true,
                push: true,
            }],
            ..Default::default()
        };
        h.restore_from_config(&c);
        assert_eq!(h.configured.len(), 1);
        assert_eq!(h.configured[0].provider, RemoteProvider::GitLab);
        assert_eq!(h.configured[0].name, "origin");
        assert_eq!(h.cursor, 0);
        assert!(h.expanded.is_none());
    }

    // --- planned_actions ---

    #[test]
    fn planned_actions_empty() {
        let h = RemotesHandler::default();
        let c = config();
        assert!(h.planned_actions(&c).is_empty());
    }

    #[test]
    fn planned_actions_with_remotes() {
        let h = RemotesHandler::default();
        let c = ProjectConfig {
            remotes: vec![RemoteConfig {
                provider: RemoteProvider::GitHub,
                name: "origin".to_string(),
                location: "git@github.com:user/repo".to_string(),
                fetch: true,
                push: true,
            }],
            ..Default::default()
        };
        let actions = h.planned_actions(&c);
        assert_eq!(actions.len(), 1);
        assert!(actions[0].contains("origin"));
        assert!(actions[0].contains("fetch+push"));
    }

    // ----------------------------------------------------------------
    // Additional coverage tests
    // ----------------------------------------------------------------

    // --- Vim j/k navigation ---

    #[test]
    fn vim_j_moves_cursor_down() {
        // 'j' must behave identically to Down in choice mode.
        let mut h = RemotesHandler::default();
        let mut c = config();
        h.handle_input(key(KeyCode::Char('j')), &mut c);
        assert_eq!(h.cursor, 1, "j should increment cursor by 1");
        h.handle_input(key(KeyCode::Char('j')), &mut c);
        assert_eq!(h.cursor, 2, "second j should increment again");
    }

    #[test]
    fn vim_k_moves_cursor_up() {
        // 'k' must behave identically to Up in choice mode.
        let mut h = RemotesHandler::default();
        let mut c = config();
        h.cursor = 3;
        h.handle_input(key(KeyCode::Char('k')), &mut c);
        assert_eq!(h.cursor, 2, "k should decrement cursor by 1");
    }

    #[test]
    fn vim_k_clamps_at_zero() {
        // Saturating sub means k at 0 stays at 0.
        let mut h = RemotesHandler::default();
        let mut c = config();
        h.handle_input(key(KeyCode::Char('k')), &mut c);
        assert_eq!(h.cursor, 0, "k at cursor 0 must not underflow");
    }

    // --- Right/l key same as Enter in choice mode ---

    #[test]
    fn right_key_on_provider_adds_remote() {
        // Right must trigger the same provider-add path as Enter.
        let mut h = RemotesHandler::default();
        let mut c = config();
        h.cursor = 1; // first provider (GitHub), no configured remotes yet
        h.handle_input(key(KeyCode::Right), &mut c);
        assert_eq!(h.configured.len(), 1, "Right on provider should add a remote");
        assert_eq!(
            h.configured[0].provider,
            RemoteProvider::GitHub,
            "added remote must be GitHub"
        );
        assert_eq!(h.expanded, Some(0), "newly added remote should be expanded");
    }

    #[test]
    fn l_key_on_provider_adds_remote() {
        // 'l' must trigger the same provider-add path as Enter.
        let mut h = RemotesHandler::default();
        let mut c = config();
        h.cursor = 1; // first provider (GitHub), no configured remotes yet
        h.handle_input(key(KeyCode::Char('l')), &mut c);
        assert_eq!(h.configured.len(), 1, "l on provider should add a remote");
        assert_eq!(h.configured[0].provider, RemoteProvider::GitHub);
    }

    #[test]
    fn l_key_on_next_with_valid_remote_commits() {
        // 'l' on Next must commit and return Done just like Enter.
        let mut h = RemotesHandler::default();
        h.configured.push(RemoteConfig {
            provider: RemoteProvider::GitHub,
            name: "origin".to_string(),
            location: "git@github.com:user/repo".to_string(),
            fetch: true,
            push: true,
        });
        let mut c = config();
        h.cursor = 0; // Next
        let result = h.handle_input(key(KeyCode::Char('l')), &mut c);
        assert!(
            matches!(result, StepResult::Done),
            "l on Next should return Done"
        );
        assert_eq!(c.remotes.len(), 1, "config should be committed");
    }

    // --- h key same as Left in choice mode ---

    #[test]
    fn h_key_collapses_when_expanded() {
        // 'h' must collapse a currently-expanded panel (same as Left).
        let mut h = RemotesHandler::default();
        h.configured.push(RemoteConfig {
            provider: RemoteProvider::GitHub,
            name: "origin".to_string(),
            location: "git@github.com:test".to_string(),
            fetch: true,
            push: true,
        });
        let mut c = config();
        h.cursor = 1;
        h.handle_input(key(KeyCode::Enter), &mut c); // expand
        h.focus = Focus::Choice; // simulate Esc (returns focus to Choice)
        let result = h.handle_input(key(KeyCode::Char('h')), &mut c);
        assert!(
            matches!(result, StepResult::Continue),
            "h while expanded should not go Back"
        );
        assert!(h.expanded.is_none(), "panel should collapse on h");
    }

    #[test]
    fn h_key_backs_when_nothing_expanded() {
        // 'h' with no expansion must return Back (same as Left).
        let mut h = RemotesHandler::default();
        let mut c = config();
        let result = h.handle_input(key(KeyCode::Char('h')), &mut c);
        assert!(
            matches!(result, StepResult::Back),
            "h with no expansion should return Back"
        );
    }

    // --- Validation: Next blocked by empty location ---

    #[test]
    fn next_blocked_when_location_empty() {
        // A remote whose location is empty must prevent Next from committing.
        let mut h = RemotesHandler::default();
        h.configured.push(RemoteConfig {
            provider: RemoteProvider::SelfHosted,
            name: "origin".to_string(),
            location: String::new(), // SelfHosted has empty url_prefix
            fetch: true,
            push: true,
        });
        let mut c = config();
        h.cursor = 0; // Next
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(
            matches!(result, StepResult::Continue),
            "Next should be blocked when location is empty"
        );
        assert!(
            c.remotes.is_empty(),
            "config must not be written when validation fails"
        );
    }

    // --- Validation: Next blocked when no operations checked ---

    #[test]
    fn next_blocked_when_no_operations() {
        // A remote with both fetch and push false must prevent Next from committing.
        let mut h = RemotesHandler::default();
        h.configured.push(RemoteConfig {
            provider: RemoteProvider::GitHub,
            name: "origin".to_string(),
            location: "git@github.com:user/repo".to_string(),
            fetch: false,
            push: false,
        });
        let mut c = config();
        h.cursor = 0; // Next
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(
            matches!(result, StepResult::Continue),
            "Next should be blocked when no operations are selected"
        );
        assert!(
            c.remotes.is_empty(),
            "config must not be written when validation fails"
        );
    }

    // --- validate_current_live: direct unit tests ---

    #[test]
    fn validate_current_live_empty_location_reports_error() {
        // The sub-panel live validator must catch an empty location field.
        let mut h = RemotesHandler::default();
        h.configured.push(RemoteConfig {
            provider: RemoteProvider::SelfHosted,
            name: "origin".to_string(),
            location: String::new(),
            fetch: true,
            push: true,
        });
        h.expanded = Some(0);
        h.focus = Focus::SubField(0);
        h.name_input = TextInput::new("Name");
        h.name_input.set_value("origin".to_string());
        h.location_input = TextInput::new("Location");
        // location_input left at default empty value
        let msg = h.validate_current_live();
        assert!(
            msg.is_some(),
            "live validator must flag an empty location"
        );
        assert_eq!(
            msg,
            Some("Location cannot be empty."),
            "error message must identify the empty location"
        );
    }

    #[test]
    fn validate_current_live_no_operations_reports_error() {
        // The sub-panel live validator must catch both-ops-off.
        let mut h = RemotesHandler::default();
        h.configured.push(RemoteConfig {
            provider: RemoteProvider::GitHub,
            name: "origin".to_string(),
            location: "git@github.com:user/repo".to_string(),
            fetch: false,
            push: false,
        });
        h.expanded = Some(0);
        h.focus = Focus::SubField(0);
        h.name_input = TextInput::new("Name");
        h.name_input.set_value("origin".to_string());
        h.location_input = TextInput::new("Location");
        h.location_input
            .set_value("git@github.com:user/repo".to_string());
        let msg = h.validate_current_live();
        assert!(
            msg.is_some(),
            "live validator must flag when no operations are selected"
        );
        assert_eq!(
            msg,
            Some("Must have at least one operation (fetch or push)."),
            "error message must identify missing operations"
        );
    }

    // --- Edge: removing the currently-expanded remote ---

    #[test]
    fn remove_expanded_remote_clears_expansion() {
        // Pressing Space on the currently-expanded remote must clear the expansion
        // and keep Focus::Choice without panicking.
        let mut h = RemotesHandler::default();
        h.configured.push(RemoteConfig {
            provider: RemoteProvider::GitHub,
            name: "origin".to_string(),
            location: "git@github.com:test".to_string(),
            fetch: true,
            push: true,
        });
        let mut c = config();
        // Expand the remote
        h.cursor = 1;
        h.handle_input(key(KeyCode::Enter), &mut c);
        assert_eq!(h.expanded, Some(0), "precondition: remote must be expanded");
        // Return focus to Choice so Space removal is routed via handle_choice_input
        h.focus = Focus::Choice;
        // Remove while still logically expanded
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert!(
            h.configured.is_empty(),
            "configured list should be empty after removal"
        );
        assert!(
            h.expanded.is_none(),
            "expansion must be cleared when expanded remote is removed"
        );
        assert_eq!(
            h.focus,
            Focus::Choice,
            "focus must remain Choice after removing expanded remote"
        );
    }

    // --- Edge: cursor clamping when last item removed ---

    #[test]
    fn remove_last_remote_clamps_cursor() {
        // After removing the only configured remote while cursor points at it,
        // the cursor must remain within the (now-smaller) choice list.
        let mut h = RemotesHandler::default();
        h.configured.push(RemoteConfig {
            provider: RemoteProvider::GitHub,
            name: "origin".to_string(),
            location: "git@github.com:test".to_string(),
            fetch: true,
            push: true,
        });
        let mut c = config();
        h.cursor = 1; // pointing at the only configured remote
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert!(
            h.cursor < h.choice_count(),
            "cursor ({}) must be within choice_count ({}) after removal",
            h.cursor,
            h.choice_count()
        );
    }

    // --- Edge: removing an earlier remote shifts expansion index ---

    #[test]
    fn remove_earlier_remote_shifts_expanded_index() {
        // When remote[0] is removed and remote[1] is expanded,
        // the expanded index must shift from 1 to 0.
        let mut h = RemotesHandler::default();
        h.configured.push(RemoteConfig {
            provider: RemoteProvider::GitHub,
            name: "first".to_string(),
            location: "git@github.com:first".to_string(),
            fetch: true,
            push: true,
        });
        h.configured.push(RemoteConfig {
            provider: RemoteProvider::Codeberg,
            name: "second".to_string(),
            location: "git@codeberg.org:second".to_string(),
            fetch: true,
            push: true,
        });
        let mut c = config();
        // Expand the second remote (index 1, cursor 2)
        h.cursor = 2;
        h.handle_input(key(KeyCode::Enter), &mut c);
        assert_eq!(h.expanded, Some(1), "precondition: second remote should be expanded");
        // Return focus to Choice so Space removal is routed correctly
        h.focus = Focus::Choice;
        // Remove the first remote (cursor 1) while second is expanded
        h.cursor = 1;
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert_eq!(h.configured.len(), 1, "first remote should be gone");
        assert_eq!(
            h.configured[0].name, "second",
            "remaining remote should be the second one"
        );
        assert_eq!(
            h.expanded,
            Some(0),
            "expanded index must shift from 1 to 0 after earlier remote is removed"
        );
    }

    // --- Multiple configured remotes: cursor_target correctness ---

    #[test]
    fn cursor_target_with_multiple_configured() {
        // With two configured remotes the cursor positions must map to the right targets.
        // 0->Next, 1->Configured(0), 2->Configured(1), 3->Provider(GitHub), last->NoRemotes
        let mut h = RemotesHandler::default();
        h.configured.push(RemoteConfig {
            provider: RemoteProvider::GitHub,
            name: "origin".to_string(),
            location: "git@github.com:a".to_string(),
            fetch: true,
            push: true,
        });
        h.configured.push(RemoteConfig {
            provider: RemoteProvider::GitLab,
            name: "upstream".to_string(),
            location: "git@gitlab.com:b".to_string(),
            fetch: true,
            push: true,
        });

        h.cursor = 0;
        assert_eq!(h.cursor_target(), CursorTarget::Next, "cursor 0 must be Next");

        h.cursor = 1;
        assert_eq!(
            h.cursor_target(),
            CursorTarget::Configured(0),
            "cursor 1 must be Configured(0)"
        );

        h.cursor = 2;
        assert_eq!(
            h.cursor_target(),
            CursorTarget::Configured(1),
            "cursor 2 must be Configured(1)"
        );

        h.cursor = 3;
        assert_eq!(
            h.cursor_target(),
            CursorTarget::Provider(RemoteProvider::GitHub),
            "cursor 3 must be the first provider (GitHub)"
        );

        h.cursor = h.choice_count() - 1;
        assert_eq!(
            h.cursor_target(),
            CursorTarget::NoRemotes,
            "last cursor position must be NoRemotes"
        );
    }

    // --- BackTab cycles backward through sub-panel positions ---

    #[test]
    fn backtab_cycles_backward() {
        // After Tab advances forward one step, BackTab must retreat to the start.
        let mut h = RemotesHandler::default();
        let mut c = config();
        h.cursor = 1; // first provider
        h.handle_input(key(KeyCode::Enter), &mut c); // add + expand, starts at (0,0)

        // Advance one step with Tab
        h.handle_input(key(KeyCode::Tab), &mut c);
        let after_tab = (h.row_cursor, h.col_cursor);

        // BackTab must return to start
        h.handle_input(key(KeyCode::BackTab), &mut c);
        assert_eq!(
            (h.row_cursor, h.col_cursor),
            (0, 0),
            "BackTab after one Tab must return to (row=0, col=0); after_tab was {after_tab:?}"
        );
    }

    #[test]
    fn backtab_wraps_from_first_to_last() {
        // BackTab at position (0,0) must wrap around to the last flat position.
        let mut h = RemotesHandler::default();
        let mut c = config();
        h.cursor = 1;
        h.handle_input(key(KeyCode::Enter), &mut c); // expand, starts at (0,0)

        // BackTab from (0,0) must jump to the last flat position.
        h.handle_input(key(KeyCode::BackTab), &mut c);

        // FLAT_NAV: (0,0),(1,0),(2,0),(2,1),(3,0) — 5 positions.
        // Last is Confirm: row=3, col=0.
        assert_eq!(
            h.row_cursor, 3,
            "BackTab from first position must land on Confirm row (row=3)"
        );
        assert_eq!(
            h.col_cursor, 0,
            "BackTab from first position must land at col 0 of Confirm row"
        );
    }

    // --- Location text input captures characters ---

    #[test]
    fn location_input_captures_chars() {
        // The location sub-field must forward typed characters to location_input
        // and sync them into configured[].location.
        let mut h = RemotesHandler::default();
        let mut c = config();
        h.cursor = 1; // first provider (GitHub)
        h.handle_input(key(KeyCode::Enter), &mut c); // add + expand; row_cursor=0 (Name)

        // Move down to Location row
        h.handle_input(key(KeyCode::Down), &mut c);
        assert_eq!(h.row_cursor, 1, "should be on Location row after Down");

        // Clear the provider-seeded prefix and type a fresh value
        h.location_input = TextInput::new("Location");
        for ch in "git@example.com:org/repo".chars() {
            h.handle_input(key(KeyCode::Char(ch)), &mut c);
        }
        assert_eq!(
            h.location_input.value(),
            "git@example.com:org/repo",
            "location_input must contain all typed characters"
        );
        assert_eq!(
            h.configured[0].location,
            "git@example.com:org/repo",
            "typed text must be synced into configured[0].location"
        );
    }
}
