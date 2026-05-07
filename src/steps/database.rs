use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};

use crate::db_registry::{self, DatabaseSpec, DriverGroup};
use crate::widgets::text_input::TextInput;
use crate::{Database, DatabaseConfig, Language, ProjectConfig, RunMode};

use super::{CURSOR_BLANK, CURSOR_MARKER, Focus, StepHandler, StepResult};

const INDENT_SUBPANEL: u16 = 4;
const INDENT_ROW: u16 = 6;
const CONFIRM_BUTTON_WIDTH: u16 = 12;
const CONFIRM_BUTTON_HEIGHT: u16 = 3;
const RUN_MODE_CHOICES: &[RunMode] = &[RunMode::Docker, RunMode::Native, RunMode::Managed];

const DB_CHOICES: [Database; 5] = [
    Database::PostgreSQL,
    Database::MySQL,
    Database::SQLite,
    Database::MongoDB,
    Database::Redis,
];

/// Validates a port string typed into the port input.
///
/// - Empty input is accepted (= "use the database's default port").
/// - Non-empty input must be all digits and parse to a `u16` in `1..=65535`.
pub(crate) fn port_problem(value: &str) -> Option<&'static str> {
    if value.is_empty() {
        return None;
    }
    if !value.bytes().all(|b| b.is_ascii_digit()) {
        return Some("Port must be a number.");
    }
    match value.parse::<u32>() {
        Ok(n) if (1..=65535).contains(&n) => None,
        _ => Some("Port must be between 1 and 65535."),
    }
}

/// One interactive row inside an expanded database's sub-panel. Built per
/// frame from the spec + the user's upstream language picks. Drives the
/// 2D cursor (`row_cursor` × `col_cursor`).
#[derive(Debug, Clone, Copy, PartialEq)]
enum DbNavRow {
    /// Single-row 3-way radio (Docker / Native / Managed). Only present when
    /// the spec's `supports_run_mode` is true.
    RunMode,
    /// One row of driver checkboxes for `language`. `group_idx` indexes into
    /// `spec.driver_groups` so the renderer can recover the underlying slice.
    Drivers { language: Language, group_idx: usize },
    /// Port text input (single row). Only present when the spec has a
    /// `default_port`.
    Port,
    /// Bordered "Confirm" button at the bottom of the sub-panel.
    Confirm,
}

impl DbNavRow {
    fn col_count(self, spec: &DatabaseSpec) -> usize {
        match self {
            Self::RunMode => RUN_MODE_CHOICES.len(),
            Self::Drivers { group_idx, .. } => spec.driver_groups[group_idx].drivers.len(),
            Self::Port => 1,
            Self::Confirm => 1,
        }
    }
}

#[derive(Debug)]
pub struct DatabaseHandler {
    cursor: usize,
    selected: Vec<Database>,
    expanded: Option<Database>,
    focus: Focus,
    row_cursor: usize,
    col_cursor: usize,
    port_input: TextInput,
    scratch: Vec<DatabaseConfig>,
    upstream_languages: Vec<Language>,
}

impl Default for DatabaseHandler {
    fn default() -> Self {
        Self {
            cursor: 0,
            selected: Vec::new(),
            expanded: None,
            focus: Focus::Choice,
            row_cursor: 0,
            col_cursor: 0,
            port_input: TextInput::new("Port"),
            scratch: Vec::new(),
            upstream_languages: Vec::new(),
        }
    }
}

impl DatabaseHandler {
    pub fn restore_from_config(&mut self, config: &ProjectConfig) {
        self.refresh_upstream(config);
        self.selected.clear();
        self.selected
            .extend(config.database_configs.iter().map(|dc| dc.database));
        self.scratch = config.database_configs.clone();
        self.cursor = 0;
        self.expanded = None;
        self.focus = Focus::Choice;
        self.row_cursor = 0;
        self.col_cursor = 0;
        self.port_input = TextInput::new("Port");
    }

    fn refresh_upstream(&mut self, config: &ProjectConfig) {
        self.upstream_languages.clear();
        self.upstream_languages
            .extend(config.language_configs.iter().map(|lc| lc.language));
    }

    fn is_selected(&self, db: Database) -> bool {
        self.selected.contains(&db)
    }

    fn scratch_for(&self, db: Database) -> Option<&DatabaseConfig> {
        self.scratch.iter().find(|dc| dc.database == db)
    }

    fn scratch_mut_for(&mut self, db: Database) -> &mut DatabaseConfig {
        if let Some(pos) = self.scratch.iter().position(|dc| dc.database == db) {
            &mut self.scratch[pos]
        } else {
            let spec = db_registry::spec_for(db);
            self.scratch
                .push(DatabaseConfig::default_for(db, spec.supports_run_mode));
            self.scratch.last_mut().unwrap()
        }
    }

    fn expand(&mut self, db: Database) {
        self.scratch_mut_for(db);
        let port_value = self
            .scratch_for(db)
            .map(|s| s.port.clone())
            .unwrap_or_default();
        self.expanded = Some(db);
        self.focus = Focus::SubField(0);
        self.row_cursor = 0;
        let spec = db_registry::spec_for(db);
        self.col_cursor = if spec.supports_run_mode {
            let run_mode = self
                .scratch_for(db)
                .and_then(|s| s.run_mode)
                .unwrap_or(RunMode::Docker);
            RUN_MODE_CHOICES
                .iter()
                .position(|m| *m == run_mode)
                .unwrap_or(0)
        } else {
            0
        };
        self.port_input = TextInput::new("Port");
        self.port_input.set_value(port_value);
    }

    fn sync_port_to_scratch(&mut self) {
        if let Some(db) = self.expanded {
            let value = self.port_input.value().to_string();
            self.scratch_mut_for(db).port = value;
        }
    }

    fn collapse_persist(&mut self) {
        self.sync_port_to_scratch();
        self.expanded = None;
        self.focus = Focus::Choice;
    }

    fn collapse_persist_keep_expanded(&mut self) {
        self.sync_port_to_scratch();
    }

    fn commit_to_config(&mut self, config: &mut ProjectConfig) {
        self.sync_port_to_scratch();
        config.database_configs = self
            .selected
            .iter()
            .map(|db| {
                debug_assert!(self.scratch_for(*db).is_some());
                self.scratch
                    .iter()
                    .find(|s| s.database == *db)
                    .cloned()
                    .expect("scratch guaranteed by expand")
            })
            .collect();
    }

    fn choice_count(&self) -> usize {
        DB_CHOICES.len() + 1
    }

    fn cursor_db(&self) -> Option<Database> {
        if self.cursor == 0 {
            None
        } else {
            Some(DB_CHOICES[self.cursor - 1])
        }
    }

    /// Build the nav rows for the currently expanded database, filtering
    /// driver groups down to languages the user picked upstream.
    fn nav_rows(&self) -> Vec<DbNavRow> {
        let mut rows = Vec::new();
        let Some(db) = self.expanded else {
            return rows;
        };
        let spec = db_registry::spec_for(db);

        if spec.supports_run_mode {
            rows.push(DbNavRow::RunMode);
        }
        for (group_idx, group) in spec.driver_groups.iter().enumerate() {
            if !self.upstream_languages.contains(&group.language) {
                continue;
            }
            if group.drivers.is_empty() {
                continue;
            }
            rows.push(DbNavRow::Drivers {
                language: group.language,
                group_idx,
            });
        }
        if spec.default_port.is_some() {
            rows.push(DbNavRow::Port);
        }
        rows.push(DbNavRow::Confirm);
        rows
    }

    fn current_row(&self) -> Option<DbNavRow> {
        self.nav_rows().get(self.row_cursor).copied()
    }

    fn clamp_col(&mut self, spec: &DatabaseSpec) {
        if let Some(row) = self.current_row() {
            let max = row.col_count(spec).saturating_sub(1);
            self.col_cursor = self.col_cursor.min(max);
        } else {
            self.col_cursor = 0;
        }
    }

    fn current_run_mode(&self) -> RunMode {
        self.expanded
            .and_then(|db| self.scratch_for(db))
            .and_then(|s| s.run_mode)
            .unwrap_or(RunMode::Docker)
    }

    fn driver_checked(&self, language: Language, id: &str) -> bool {
        self.expanded
            .and_then(|db| self.scratch_for(db))
            .is_some_and(|s| s.drivers.iter().any(|(l, d)| *l == language && *d == id))
    }

    fn toggle_driver(&mut self, language: Language, id: &'static str) {
        if let Some(db) = self.expanded {
            let s = self.scratch_mut_for(db);
            if let Some(pos) = s.drivers.iter().position(|(l, d)| *l == language && *d == id) {
                s.drivers.remove(pos);
            } else {
                s.drivers.push((language, id));
            }
        }
    }

    fn live_port_problem(&self) -> Option<&'static str> {
        port_problem(self.port_input.value())
    }

    fn selected_port_problem(&self) -> Option<String> {
        self.selected.iter().find_map(|db| {
            let port = self.scratch_for(*db).map_or("", |s| s.port.as_str());
            port_problem(port).map(|msg| format!("{db}: {msg}"))
        })
    }

    fn toggle_db(&mut self, db: Database) {
        if let Some(pos) = self.selected.iter().position(|d| *d == db) {
            self.selected.remove(pos);
        } else {
            self.selected.push(db);
        }
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
                if let Some(db) = self.cursor_db()
                    && self.is_selected(db)
                {
                    self.toggle_db(db);
                }
                StepResult::Continue
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => match self.cursor_db() {
                None => {
                    if self.selected_port_problem().is_some() {
                        return StepResult::Continue;
                    }
                    self.commit_to_config(config);
                    StepResult::Done
                }
                Some(db) => {
                    if !self.is_selected(db) {
                        self.selected.push(db);
                    }
                    self.expand(db);
                    StepResult::Continue
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

    fn handle_subfield_input(&mut self, key: KeyEvent, _config: &mut ProjectConfig) -> StepResult {
        let Some(db) = self.expanded else {
            return StepResult::Continue;
        };
        let spec = db_registry::spec_for(db);
        let rows = self.nav_rows();

        if rows.is_empty() {
            return StepResult::Continue;
        }

        let row = rows[self.row_cursor];
        let on_port = matches!(row, DbNavRow::Port);
        let on_confirm = matches!(row, DbNavRow::Confirm);

        // Universal nav keys.
        match key.code {
            KeyCode::Esc => {
                self.focus = Focus::Choice;
                return StepResult::Continue;
            }
            KeyCode::Up => {
                if self.row_cursor == 0 {
                    self.collapse_persist_keep_expanded();
                    self.focus = Focus::Choice;
                } else {
                    self.row_cursor -= 1;
                    self.clamp_col(spec);
                }
                return StepResult::Continue;
            }
            KeyCode::Down => {
                if self.row_cursor + 1 < rows.len() {
                    self.row_cursor += 1;
                    self.clamp_col(spec);
                }
                return StepResult::Continue;
            }
            KeyCode::Tab => {
                self.advance_flattened(&rows, spec, false);
                return StepResult::Continue;
            }
            KeyCode::BackTab => {
                self.advance_flattened(&rows, spec, true);
                return StepResult::Continue;
            }
            _ => {}
        }

        // Confirm button row.
        if on_confirm {
            match key.code {
                KeyCode::Char('q') => return StepResult::Quit,
                KeyCode::Char(' ') | KeyCode::Enter => {
                    if self.live_port_problem().is_none() {
                        self.collapse_persist();
                    }
                }
                KeyCode::Char('k') => {
                    if self.row_cursor == 0 {
                        self.collapse_persist_keep_expanded();
                        self.focus = Focus::Choice;
                    } else {
                        self.row_cursor -= 1;
                        self.clamp_col(spec);
                    }
                }
                KeyCode::Char('j') => {
                    if self.row_cursor + 1 < rows.len() {
                        self.row_cursor += 1;
                        self.clamp_col(spec);
                    }
                }
                _ => {}
            }
            return StepResult::Continue;
        }

        // Port text input row.
        if on_port {
            if matches!(key.code, KeyCode::Enter) {
                if self.live_port_problem().is_none() {
                    self.collapse_persist();
                }
                return StepResult::Continue;
            }
            match key.code {
                KeyCode::Char(c) => {
                    self.port_input.handle_input(KeyCode::Char(c));
                }
                KeyCode::Backspace
                | KeyCode::Delete
                | KeyCode::Left
                | KeyCode::Right
                | KeyCode::Home
                | KeyCode::End => {
                    self.port_input.handle_input(key.code);
                }
                _ => {}
            }
            self.sync_port_to_scratch();
            return StepResult::Continue;
        }

        // RunMode and Drivers rows.
        match row {
            DbNavRow::RunMode => self.handle_run_mode_key(key.code),
            DbNavRow::Drivers {
                language,
                group_idx,
            } => self.handle_drivers_key(key.code, language, &spec.driver_groups[group_idx]),
            _ => StepResult::Continue,
        }
    }

    fn handle_run_mode_key(&mut self, key: KeyCode) -> StepResult {
        match key {
            KeyCode::Char('q') => StepResult::Quit,
            KeyCode::Char('j') | KeyCode::Char('k') => StepResult::Continue,
            KeyCode::Left | KeyCode::Char('h') => {
                let run_mode = self.current_run_mode();
                let idx = RUN_MODE_CHOICES
                    .iter()
                    .position(|m| *m == run_mode)
                    .unwrap_or(0);
                if idx > 0
                    && let Some(db) = self.expanded
                {
                    self.scratch_mut_for(db).run_mode = Some(RUN_MODE_CHOICES[idx - 1]);
                    self.col_cursor = idx - 1;
                }
                StepResult::Continue
            }
            KeyCode::Right | KeyCode::Char('l') => {
                let run_mode = self.current_run_mode();
                let idx = RUN_MODE_CHOICES
                    .iter()
                    .position(|m| *m == run_mode)
                    .unwrap_or(0);
                if idx + 1 < RUN_MODE_CHOICES.len()
                    && let Some(db) = self.expanded
                {
                    self.scratch_mut_for(db).run_mode = Some(RUN_MODE_CHOICES[idx + 1]);
                    self.col_cursor = idx + 1;
                }
                StepResult::Continue
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                let run_mode = self.current_run_mode();
                let idx = RUN_MODE_CHOICES
                    .iter()
                    .position(|m| *m == run_mode)
                    .unwrap_or(0);
                let next_idx = (idx + 1) % RUN_MODE_CHOICES.len();
                if let Some(db) = self.expanded {
                    self.scratch_mut_for(db).run_mode = Some(RUN_MODE_CHOICES[next_idx]);
                    self.col_cursor = next_idx;
                }
                StepResult::Continue
            }
            _ => StepResult::Continue,
        }
    }

    fn handle_drivers_key(
        &mut self,
        key: KeyCode,
        language: Language,
        group: &DriverGroup,
    ) -> StepResult {
        match key {
            KeyCode::Char('q') => StepResult::Quit,
            KeyCode::Left | KeyCode::Char('h') => {
                self.col_cursor = self.col_cursor.saturating_sub(1);
                StepResult::Continue
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.col_cursor + 1 < group.drivers.len() {
                    self.col_cursor += 1;
                }
                StepResult::Continue
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                if let Some(driver) = group.drivers.get(self.col_cursor) {
                    self.toggle_driver(language, driver.id);
                }
                StepResult::Continue
            }
            _ => StepResult::Continue,
        }
    }

    fn advance_flattened(&mut self, rows: &[DbNavRow], spec: &DatabaseSpec, backward: bool) {
        if rows.is_empty() {
            return;
        }
        let flat: Vec<(usize, usize)> = rows
            .iter()
            .enumerate()
            .flat_map(|(r, row)| (0..row.col_count(spec)).map(move |c| (r, c)))
            .collect();
        if flat.is_empty() {
            return;
        }
        let pos = flat
            .iter()
            .position(|(r, c)| *r == self.row_cursor && *c == self.col_cursor)
            .unwrap_or(0);
        let next = if backward {
            if pos == 0 {
                flat.len() - 1
            } else {
                pos - 1
            }
        } else {
            (pos + 1) % flat.len()
        };
        let (r, c) = flat[next];
        self.row_cursor = r;
        self.col_cursor = c;
    }

    // --- Rendering ---

    fn render_next_line(&self, frame: &mut Frame, area: Rect) {
        let highlighted = matches!(self.focus, Focus::Choice) && self.cursor == 0;
        let cursor_marker = if highlighted {
            CURSOR_MARKER
        } else {
            CURSOR_BLANK
        };
        let style = Style::default().add_modifier(Modifier::BOLD);
        let text = format!("{cursor_marker}Next →");
        frame.render_widget(Paragraph::new(Line::from(text).style(style)), area);
    }

    fn render_choice_line(&self, frame: &mut Frame, area: Rect, db: Database, idx: usize) {
        let cursor_marker = if matches!(self.focus, Focus::Choice) && idx == self.cursor {
            CURSOR_MARKER
        } else {
            CURSOR_BLANK
        };
        let check = if self.is_selected(db) { "[x]" } else { "[ ]" };
        let text = format!("{cursor_marker}{check} {db}");
        frame.render_widget(Paragraph::new(Line::from(text)), area);
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

    fn render_run_mode_row(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let run_mode = self.current_run_mode();
        let mut spans = vec![Span::raw("Run mode: ")];
        for (i, mode) in RUN_MODE_CHOICES.iter().enumerate() {
            let glyph = if *mode == run_mode { "●" } else { "○" };
            let cell = format!("{glyph} {mode}");
            let style = if focused && self.col_cursor == i {
                Style::default().fg(Color::Black).bg(Color::White)
            } else if focused {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            spans.push(Span::styled(cell, style));
            if i + 1 < RUN_MODE_CHOICES.len() {
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

    fn render_drivers_row(
        &self,
        frame: &mut Frame,
        area: Rect,
        language: Language,
        group: &DriverGroup,
        focused_row: bool,
    ) {
        let label_rect = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(
                Line::from(format!("{language}:"))
                    .style(Style::default().add_modifier(Modifier::BOLD)),
            ),
            label_rect,
        );

        let row_rect = Rect {
            x: area.x + 2,
            y: area.y + 1,
            width: area.width.saturating_sub(2),
            height: 1,
        };
        let mut spans = Vec::new();
        for (i, driver) in group.drivers.iter().enumerate() {
            let check = if self.driver_checked(language, driver.id) {
                "[x]"
            } else {
                "[ ]"
            };
            let cell = format!("{check} {}", driver.label);
            let style = if focused_row && self.col_cursor == i {
                Style::default().fg(Color::Black).bg(Color::White)
            } else if focused_row {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            spans.push(Span::styled(cell, style));
            if i + 1 < group.drivers.len() {
                spans.push(Span::raw("   "));
            }
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), row_rect);
    }

    fn render_expanded_panel(&self, frame: &mut Frame, db: Database, area: Rect) -> u16 {
        let spec = db_registry::spec_for(db);
        let rows = self.nav_rows();
        let mut y = area.y;
        let bottom = area.y + area.height;
        let focused_row =
            |r: usize| matches!(self.focus, Focus::SubField(_)) && self.row_cursor == r;

        for (idx, row) in rows.iter().enumerate() {
            match *row {
                DbNavRow::RunMode => {
                    if y >= bottom {
                        return y;
                    }
                    let row_area = Rect {
                        x: area.x + INDENT_SUBPANEL,
                        y,
                        width: area.width.saturating_sub(INDENT_SUBPANEL),
                        height: 1,
                    };
                    self.render_run_mode_row(frame, row_area, focused_row(idx));
                    y += 1;
                }
                DbNavRow::Drivers {
                    language,
                    group_idx,
                } => {
                    if y + 1 >= bottom {
                        return y;
                    }
                    let group = &spec.driver_groups[group_idx];
                    let row_area = Rect {
                        x: area.x + INDENT_ROW,
                        y,
                        width: area.width.saturating_sub(INDENT_ROW),
                        height: 2,
                    };
                    self.render_drivers_row(frame, row_area, language, group, focused_row(idx));
                    y += 2;
                }
                DbNavRow::Port => {
                    if y + 2 >= bottom {
                        return y;
                    }
                    let row_area = Rect {
                        x: area.x + INDENT_SUBPANEL,
                        y,
                        width: area.width.saturating_sub(INDENT_SUBPANEL),
                        height: 3,
                    };
                    self.port_input.render(frame, row_area, focused_row(idx));
                    y += 3;
                }
                DbNavRow::Confirm => {
                    if let Some(msg) = self.live_port_problem()
                        && y < bottom
                    {
                        let rect = Rect {
                            x: area.x + INDENT_ROW,
                            y,
                            width: area.width.saturating_sub(INDENT_ROW),
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
                            x: area.x + INDENT_ROW,
                            y,
                            width: CONFIRM_BUTTON_WIDTH
                                .min(area.width.saturating_sub(INDENT_ROW)),
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

impl StepHandler for DatabaseHandler {
    fn render(&self, frame: &mut Frame, area: Rect) {
        let mut y = area.y;
        let bottom = area.y + area.height;

        // "Next" pseudo-row at index 0.
        if y < bottom {
            let next_rect = Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            };
            self.render_next_line(frame, next_rect);
            y += 1;
        }

        for (i, db) in DB_CHOICES.iter().enumerate() {
            if y >= bottom {
                break;
            }
            let choice_rect = Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            };
            self.render_choice_line(frame, choice_rect, *db, i + 1);
            y += 1;
            if self.expanded == Some(*db) {
                let panel_area = Rect {
                    x: area.x,
                    y,
                    width: area.width,
                    height: bottom.saturating_sub(y),
                };
                y = self.render_expanded_panel(frame, *db, panel_area);
            }
        }

        // Choice-level warning when any selected database has invalid port.
        if let Some(msg) = self.selected_port_problem()
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
        self.refresh_upstream(config);
        match self.focus {
            Focus::Choice => self.handle_choice_input(key.code, config),
            Focus::SubField(_) => self.handle_subfield_input(key, config),
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
        let mut actions = Vec::new();
        for dc in &config.database_configs {
            actions.push(format!("Configure {}", dc.database));
            if let Some(rm) = dc.run_mode {
                actions.push(format!("Run mode: {rm}"));
            }
            if !dc.drivers.is_empty() {
                let labels: Vec<String> = dc
                    .drivers
                    .iter()
                    .filter_map(|(lang, id)| {
                        db_registry::driver_by_id(*lang, id).map(|d| d.label.to_string())
                    })
                    .collect();
                if !labels.is_empty() {
                    actions.push(format!("Drivers: {}", labels.join(", ")));
                }
            }
        }
        actions
    }

    fn execute(&self, _config: &ProjectConfig) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LanguageConfig;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn config_with_languages(langs: &[Language]) -> ProjectConfig {
        ProjectConfig {
            language_configs: langs
                .iter()
                .map(|l| LanguageConfig::new(*l))
                .collect(),
            ..Default::default()
        }
    }

    fn with_cursor_on(db: Database) -> DatabaseHandler {
        let idx = DB_CHOICES.iter().position(|d| *d == db).unwrap();
        DatabaseHandler {
            cursor: idx + 1,
            ..Default::default()
        }
    }

    fn with_selected(db: Database) -> DatabaseHandler {
        let idx = DB_CHOICES.iter().position(|d| *d == db).unwrap();
        DatabaseHandler {
            cursor: idx + 1,
            selected: vec![db],
            ..Default::default()
        }
    }

    // --- Defaults ---

    #[test]
    fn default_state() {
        let h = DatabaseHandler::default();
        assert_eq!(h.focus, Focus::Choice);
        assert!(h.expanded.is_none());
        assert_eq!(h.cursor, 0);
        assert!(h.selected.is_empty());
        assert!(h.scratch.is_empty());
        assert!(!h.in_details());
        assert!(!h.is_expanded());
    }

    // --- Choice navigation ---

    #[test]
    fn cursor_down_and_up_at_choice_level() {
        let mut h = DatabaseHandler::default();
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Down), &mut c);
        assert_eq!(h.cursor, 1);
        h.handle_input(key(KeyCode::Char('j')), &mut c);
        assert_eq!(h.cursor, 2);
        h.handle_input(key(KeyCode::Up), &mut c);
        assert_eq!(h.cursor, 1);
        h.handle_input(key(KeyCode::Char('k')), &mut c);
        assert_eq!(h.cursor, 0);
    }

    #[test]
    fn cursor_clamps_at_bounds() {
        let mut h = DatabaseHandler::default();
        let mut c = ProjectConfig::default();
        for _ in 0..20 {
            h.handle_input(key(KeyCode::Down), &mut c);
        }
        assert_eq!(h.cursor, DB_CHOICES.len());
        for _ in 0..20 {
            h.handle_input(key(KeyCode::Up), &mut c);
        }
        assert_eq!(h.cursor, 0);
    }

    // --- Next row ---

    #[test]
    fn enter_on_next_commits_and_advances() {
        let mut h = DatabaseHandler::default();
        let mut c = ProjectConfig::default();
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Done));
        assert!(c.database_configs.is_empty());
    }

    #[test]
    fn right_on_next_commits_and_advances() {
        let mut h = DatabaseHandler::default();
        let mut c = ProjectConfig::default();
        let result = h.handle_input(key(KeyCode::Right), &mut c);
        assert!(matches!(result, StepResult::Done));
    }

    #[test]
    fn enter_on_next_with_selections_commits_all() {
        let mut h = DatabaseHandler {
            selected: vec![Database::PostgreSQL, Database::Redis],
            ..Default::default()
        };
        h.scratch_mut_for(Database::PostgreSQL);
        h.scratch_mut_for(Database::Redis);
        let mut c = ProjectConfig::default();
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Done));
        assert_eq!(c.database_configs.len(), 2);
        assert_eq!(c.database_configs[0].database, Database::PostgreSQL);
        assert_eq!(c.database_configs[1].database, Database::Redis);
    }

    // --- Select and expand ---

    #[test]
    fn enter_on_db_checks_and_expands() {
        let mut h = with_cursor_on(Database::PostgreSQL);
        let mut c = ProjectConfig::default();
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Continue));
        assert!(h.is_selected(Database::PostgreSQL));
        assert_eq!(h.expanded, Some(Database::PostgreSQL));
        assert_eq!(h.focus, Focus::SubField(0));
    }

    #[test]
    fn space_deselects_checked_db() {
        let mut h = with_selected(Database::PostgreSQL);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert!(h.selected.is_empty());
        assert!(h.expanded.is_none());
    }

    #[test]
    fn space_on_unchecked_db_is_noop() {
        let mut h = with_cursor_on(Database::PostgreSQL);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert!(h.selected.is_empty());
    }

    #[test]
    fn space_on_next_is_noop() {
        let mut h = DatabaseHandler::default();
        let mut c = ProjectConfig::default();
        let result = h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert!(matches!(result, StepResult::Continue));
        assert!(h.selected.is_empty());
    }

    // --- Left / collapse / back ---

    #[test]
    fn left_collapses_expanded_then_backs() {
        let mut h = with_selected(Database::PostgreSQL);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        assert!(h.expanded.is_some());
        h.focus = Focus::Choice;
        let result = h.handle_input(key(KeyCode::Left), &mut c);
        assert!(h.expanded.is_none());
        assert!(matches!(result, StepResult::Continue));
        let result = h.handle_input(key(KeyCode::Left), &mut c);
        assert!(matches!(result, StepResult::Back));
    }

    #[test]
    fn left_backs_when_nothing_expanded() {
        let mut h = DatabaseHandler::default();
        let mut c = ProjectConfig::default();
        let result = h.handle_input(key(KeyCode::Left), &mut c);
        assert!(matches!(result, StepResult::Back));
    }

    // --- Esc ---

    #[test]
    fn esc_from_subfield_returns_to_choice() {
        let mut h = with_selected(Database::PostgreSQL);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        assert_eq!(h.focus, Focus::SubField(0));
        h.handle_input(key(KeyCode::Esc), &mut c);
        assert_eq!(h.focus, Focus::Choice);
        assert_eq!(h.expanded, Some(Database::PostgreSQL));
    }

    // --- Up from row 0 ---

    #[test]
    fn up_from_row_zero_returns_to_choice() {
        let mut h = with_selected(Database::PostgreSQL);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        h.handle_input(key(KeyCode::Up), &mut c);
        assert_eq!(h.focus, Focus::Choice);
        assert_eq!(h.expanded, Some(Database::PostgreSQL));
    }

    // --- nav_rows shape ---

    fn handler_with_upstream(expanded: Database, langs: &[Language]) -> DatabaseHandler {
        let mut h = DatabaseHandler {
            expanded: Some(expanded),
            ..Default::default()
        };
        h.scratch_mut_for(expanded);
        h.upstream_languages = langs.to_vec();
        h
    }

    #[test]
    fn server_db_with_no_languages_has_run_mode_port_confirm() {
        let h = handler_with_upstream(Database::PostgreSQL, &[]);
        let rows = h.nav_rows();
        assert_eq!(rows.len(), 3);
        assert!(matches!(rows[0], DbNavRow::RunMode));
        assert!(matches!(rows[1], DbNavRow::Port));
        assert!(matches!(rows[2], DbNavRow::Confirm));
    }

    #[test]
    fn server_db_with_python_includes_python_drivers() {
        let h = handler_with_upstream(Database::PostgreSQL, &[Language::Python]);
        let rows = h.nav_rows();
        assert_eq!(rows.len(), 4);
        assert!(matches!(rows[0], DbNavRow::RunMode));
        assert!(matches!(
            rows[1],
            DbNavRow::Drivers {
                language: Language::Python,
                ..
            }
        ));
        assert!(matches!(rows[2], DbNavRow::Port));
        assert!(matches!(rows[3], DbNavRow::Confirm));
    }

    #[test]
    fn sqlite_skips_run_mode_and_port() {
        let h = handler_with_upstream(Database::SQLite, &[Language::Python, Language::Rust]);
        let rows = h.nav_rows();
        assert_eq!(rows.len(), 3);
        assert!(matches!(
            rows[0],
            DbNavRow::Drivers {
                language: Language::Python,
                ..
            }
        ));
        assert!(matches!(
            rows[1],
            DbNavRow::Drivers {
                language: Language::Rust,
                ..
            }
        ));
        assert!(matches!(rows[2], DbNavRow::Confirm));
    }

    #[test]
    fn drivers_filtered_by_upstream_languages() {
        let h = handler_with_upstream(Database::PostgreSQL, &[Language::Rust]);
        let rows = h.nav_rows();
        let driver_rows: Vec<_> = rows
            .iter()
            .filter(|r| matches!(r, DbNavRow::Drivers { .. }))
            .collect();
        assert_eq!(driver_rows.len(), 1);
        assert!(matches!(
            driver_rows[0],
            DbNavRow::Drivers {
                language: Language::Rust,
                ..
            }
        ));
    }

    #[test]
    fn nav_rows_always_end_with_confirm() {
        let h1 = handler_with_upstream(Database::PostgreSQL, &[]);
        let rows1 = h1.nav_rows();
        assert!(matches!(rows1.last(), Some(DbNavRow::Confirm)));

        let h2 = handler_with_upstream(Database::SQLite, &[Language::Python]);
        let rows2 = h2.nav_rows();
        assert!(matches!(rows2.last(), Some(DbNavRow::Confirm)));
    }

    // --- Run mode interaction ---

    #[test]
    fn run_mode_left_right() {
        let mut h = with_selected(Database::PostgreSQL);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c); // expand
        // Start: Docker. Right -> Native.
        h.handle_input(key(KeyCode::Right), &mut c);
        assert_eq!(h.current_run_mode(), RunMode::Native);
        // Right -> Managed.
        h.handle_input(key(KeyCode::Right), &mut c);
        assert_eq!(h.current_run_mode(), RunMode::Managed);
        // Right at last is no-op.
        h.handle_input(key(KeyCode::Right), &mut c);
        assert_eq!(h.current_run_mode(), RunMode::Managed);
        // Left -> Native.
        h.handle_input(key(KeyCode::Left), &mut c);
        assert_eq!(h.current_run_mode(), RunMode::Native);
        // Left -> Docker.
        h.handle_input(key(KeyCode::Left), &mut c);
        assert_eq!(h.current_run_mode(), RunMode::Docker);
        // Left at first is no-op.
        h.handle_input(key(KeyCode::Left), &mut c);
        assert_eq!(h.current_run_mode(), RunMode::Docker);
    }

    #[test]
    fn run_mode_space_cycles_with_wrap() {
        let mut h = with_selected(Database::PostgreSQL);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        // Set to Managed via scratch
        h.scratch_mut_for(Database::PostgreSQL).run_mode = Some(RunMode::Managed);
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert_eq!(h.current_run_mode(), RunMode::Docker); // wraps
    }

    // --- Driver toggling ---

    #[test]
    fn driver_space_toggles() {
        let mut h = with_selected(Database::PostgreSQL);
        let mut c = config_with_languages(&[Language::Python]);
        h.handle_input(key(KeyCode::Right), &mut c); // expand
        // Move to the first driver row
        h.handle_input(key(KeyCode::Down), &mut c); // row 1 = Python drivers
        h.handle_input(key(KeyCode::Char(' ')), &mut c); // toggle psycopg
        assert!(h.driver_checked(Language::Python, "psycopg"));
        h.handle_input(key(KeyCode::Char(' ')), &mut c); // untoggle
        assert!(!h.driver_checked(Language::Python, "psycopg"));
    }

    #[test]
    fn enter_on_driver_row_toggles_not_advances() {
        let mut h = with_selected(Database::PostgreSQL);
        let mut c = config_with_languages(&[Language::Python]);
        h.handle_input(key(KeyCode::Right), &mut c);
        h.handle_input(key(KeyCode::Down), &mut c); // driver row
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Continue));
        assert!(h.driver_checked(Language::Python, "psycopg"));
        assert_eq!(h.focus, Focus::SubField(0)); // still in sub-panel
    }

    #[test]
    fn enter_on_run_mode_cycles() {
        let mut h = with_selected(Database::PostgreSQL);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c); // expand, row 0 = RunMode
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Continue));
        assert_eq!(h.current_run_mode(), RunMode::Native); // cycled from Docker
    }

    // --- Port validation ---

    #[test]
    fn port_problem_empty_is_valid() {
        assert!(port_problem("").is_none());
    }

    #[test]
    fn port_problem_letters_invalid() {
        assert!(port_problem("abc").is_some());
        assert!(port_problem("80a").is_some());
    }

    #[test]
    fn port_problem_zero_invalid() {
        assert!(port_problem("0").is_some());
    }

    #[test]
    fn port_problem_in_range_valid() {
        assert!(port_problem("1").is_none());
        assert!(port_problem("5432").is_none());
        assert!(port_problem("65535").is_none());
    }

    #[test]
    fn port_problem_out_of_range_invalid() {
        assert!(port_problem("65536").is_some());
        assert!(port_problem("100000").is_some());
    }

    // --- Confirm button ---

    #[test]
    fn confirm_button_collapses_via_enter() {
        let mut h = with_selected(Database::PostgreSQL);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        let rows = h.nav_rows();
        h.row_cursor = rows.len() - 1; // Confirm row
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Continue));
        assert_eq!(h.focus, Focus::Choice);
        assert!(h.expanded.is_none());
        assert!(h.is_selected(Database::PostgreSQL));
    }

    #[test]
    fn confirm_button_collapses_via_space() {
        let mut h = with_selected(Database::PostgreSQL);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        let rows = h.nav_rows();
        h.row_cursor = rows.len() - 1;
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert_eq!(h.focus, Focus::Choice);
        assert!(h.expanded.is_none());
    }

    #[test]
    fn confirm_blocked_with_invalid_port() {
        let mut h = with_selected(Database::PostgreSQL);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        h.port_input.set_value("99999");
        let rows = h.nav_rows();
        h.row_cursor = rows.len() - 1;
        h.handle_input(key(KeyCode::Enter), &mut c);
        assert_eq!(h.expanded, Some(Database::PostgreSQL), "Confirm should not collapse");
    }

    // --- Next blocked with invalid port ---

    #[test]
    fn next_blocked_when_selected_db_has_invalid_port() {
        let mut h = with_selected(Database::PostgreSQL);
        h.scratch_mut_for(Database::PostgreSQL).port = "99999".to_string();
        let mut c = ProjectConfig::default();
        h.cursor = 0; // Next
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Continue));
        assert!(c.database_configs.is_empty());
    }

    #[test]
    fn next_advances_when_invalid_port_db_is_unchecked() {
        let mut h = DatabaseHandler::default();
        // Add scratch with bad port but don't select the DB
        h.scratch_mut_for(Database::PostgreSQL).port = "99999".to_string();
        let mut c = ProjectConfig::default();
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Done));
    }

    // --- Port text input ---

    #[test]
    fn enter_on_port_row_collapses() {
        let mut h = with_selected(Database::PostgreSQL);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        // Find the port row
        let rows = h.nav_rows();
        let port_row = rows
            .iter()
            .position(|r| matches!(r, DbNavRow::Port))
            .unwrap();
        h.row_cursor = port_row;
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Continue));
        assert_eq!(h.focus, Focus::Choice);
        assert!(h.expanded.is_none());
    }

    #[test]
    fn enter_on_port_with_invalid_value_does_not_collapse() {
        let mut h = with_selected(Database::PostgreSQL);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        let rows = h.nav_rows();
        let port_row = rows
            .iter()
            .position(|r| matches!(r, DbNavRow::Port))
            .unwrap();
        h.row_cursor = port_row;
        // Type invalid port
        for ch in "99999".chars() {
            h.handle_input(key(KeyCode::Char(ch)), &mut c);
        }
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Continue));
        assert_eq!(h.expanded, Some(Database::PostgreSQL)); // still expanded
    }

    #[test]
    fn port_chars_are_captured() {
        let mut h = with_selected(Database::PostgreSQL);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        let rows = h.nav_rows();
        let port_row = rows
            .iter()
            .position(|r| matches!(r, DbNavRow::Port))
            .unwrap();
        h.row_cursor = port_row;
        for ch in "5433".chars() {
            h.handle_input(key(KeyCode::Char(ch)), &mut c);
        }
        assert_eq!(h.port_input.value(), "5433");
        // Scratch is synced
        assert_eq!(h.scratch_for(Database::PostgreSQL).unwrap().port, "5433");
    }

    // --- Full flow ---

    #[test]
    fn full_flow_configure_and_advance() {
        let mut h = with_cursor_on(Database::PostgreSQL);
        let mut c = config_with_languages(&[Language::Python]);
        // Enter: check + expand
        h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(h.is_selected(Database::PostgreSQL));
        assert_eq!(h.focus, Focus::SubField(0));
        // Toggle a driver
        h.handle_input(key(KeyCode::Down), &mut c); // driver row
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        // Navigate to Confirm
        let rows = h.nav_rows();
        h.row_cursor = rows.len() - 1;
        h.handle_input(key(KeyCode::Enter), &mut c); // collapse
        assert_eq!(h.focus, Focus::Choice);
        // Navigate up to Next
        for _ in 0..20 {
            h.handle_input(key(KeyCode::Up), &mut c);
        }
        assert_eq!(h.cursor, 0);
        // Enter on Next: commit + advance
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Done));
        assert_eq!(c.database_configs.len(), 1);
        assert_eq!(
            c.database_configs[0].database,
            Database::PostgreSQL
        );
    }

    #[test]
    fn multi_select_two_databases() {
        let mut h = DatabaseHandler::default();
        let mut c = ProjectConfig::default();
        // Select PostgreSQL
        h.cursor = DB_CHOICES.iter().position(|d| *d == Database::PostgreSQL).unwrap() + 1;
        h.handle_input(key(KeyCode::Enter), &mut c); // check + expand
        h.handle_input(key(KeyCode::Esc), &mut c); // back to choice
        // Select Redis
        h.cursor = DB_CHOICES.iter().position(|d| *d == Database::Redis).unwrap() + 1;
        h.handle_input(key(KeyCode::Enter), &mut c);
        h.handle_input(key(KeyCode::Esc), &mut c);
        assert_eq!(h.selected.len(), 2);
        // Next
        h.cursor = 0;
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Done));
        assert_eq!(c.database_configs.len(), 2);
    }

    // --- Uncheck then recheck preserves scratch ---

    #[test]
    fn uncheck_then_recheck_preserves_scratch() {
        let mut h = with_selected(Database::PostgreSQL);
        let mut c = config_with_languages(&[Language::Python]);
        h.handle_input(key(KeyCode::Right), &mut c); // expand
        h.handle_input(key(KeyCode::Down), &mut c); // driver row
        h.handle_input(key(KeyCode::Char(' ')), &mut c); // toggle driver
        h.handle_input(key(KeyCode::Esc), &mut c); // back to choice
        h.handle_input(key(KeyCode::Left), &mut c); // collapse
        h.handle_input(key(KeyCode::Char(' ')), &mut c); // uncheck
        assert!(!h.is_selected(Database::PostgreSQL));
        assert!(h.scratch_for(Database::PostgreSQL).unwrap().drivers.len() == 1);
        h.handle_input(key(KeyCode::Enter), &mut c); // re-check via Enter (also expands)
        assert!(h.is_selected(Database::PostgreSQL));
        assert!(h.scratch_for(Database::PostgreSQL).unwrap().drivers.len() == 1);
    }

    // --- Restore ---

    #[test]
    fn restore_from_config_rehydrates() {
        let mut h = DatabaseHandler::default();
        let c = ProjectConfig {
            database_configs: vec![DatabaseConfig {
                database: Database::MySQL,
                run_mode: Some(RunMode::Native),
                drivers: vec![(Language::Python, "pymysql")],
                port: "3307".to_string(),
            }],
            ..Default::default()
        };
        h.restore_from_config(&c);
        assert_eq!(h.selected, vec![Database::MySQL]);
        assert_eq!(h.scratch.len(), 1);
        assert_eq!(h.scratch[0].run_mode, Some(RunMode::Native));
        assert_eq!(h.scratch[0].drivers, vec![(Language::Python, "pymysql")]);
        assert_eq!(h.scratch[0].port, "3307");
    }

    // --- Tab cycling ---

    #[test]
    fn tab_cycles_flat_positions() {
        let mut h = with_selected(Database::PostgreSQL);
        let mut c = config_with_languages(&[Language::Python]);
        h.handle_input(key(KeyCode::Right), &mut c); // expand
        // RunMode(3) + Drivers(4) + Port(1) + Confirm(1) = 9 flat positions
        let mut seen = vec![(h.row_cursor, h.col_cursor)];
        for _ in 0..8 {
            h.handle_input(key(KeyCode::Tab), &mut c);
            seen.push((h.row_cursor, h.col_cursor));
        }
        assert_eq!(seen.len(), 9);
        h.handle_input(key(KeyCode::Tab), &mut c);
        assert_eq!((h.row_cursor, h.col_cursor), (0, 0)); // wraps
    }

    // --- Quit ---

    #[test]
    fn q_quits() {
        let mut h = DatabaseHandler::default();
        let mut c = ProjectConfig::default();
        let result = h.handle_input(key(KeyCode::Char('q')), &mut c);
        assert!(matches!(result, StepResult::Quit));
    }

    // --- planned_actions ---

    #[test]
    fn planned_actions_empty_when_no_databases() {
        let h = DatabaseHandler::default();
        let c = ProjectConfig::default();
        assert!(h.planned_actions(&c).is_empty());
    }

    #[test]
    fn planned_actions_postgres_with_drivers() {
        let h = DatabaseHandler::default();
        let c = ProjectConfig {
            database_configs: vec![DatabaseConfig {
                database: Database::PostgreSQL,
                run_mode: Some(RunMode::Docker),
                drivers: vec![(Language::Python, "psycopg"), (Language::Rust, "sqlx")],
                port: String::new(),
            }],
            ..Default::default()
        };
        let actions = h.planned_actions(&c);
        assert!(actions[0].contains("PostgreSQL"));
        assert!(actions.iter().any(|a| a.contains("Docker")));
        assert!(actions
            .iter()
            .any(|a| a.contains("psycopg") && a.contains("sqlx")));
    }

    // --- execute ---

    #[test]
    fn execute_is_ok() {
        let h = DatabaseHandler::default();
        let c = ProjectConfig::default();
        assert!(h.execute(&c).is_ok());
    }

    // --- in_details / is_expanded ---

    #[test]
    fn in_details_false_at_choice() {
        let h = DatabaseHandler::default();
        assert!(!h.in_details());
    }

    #[test]
    fn in_details_true_in_subfield() {
        let h = DatabaseHandler {
            focus: Focus::SubField(0),
            ..Default::default()
        };
        assert!(h.in_details());
    }

    #[test]
    fn is_expanded_tracks_expanded_field() {
        let mut h = DatabaseHandler::default();
        assert!(!h.is_expanded());
        h.expanded = Some(Database::PostgreSQL);
        assert!(h.is_expanded());
    }
}
