use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::Paragraph,
};
use strum::VariantArray;

use crate::db_registry::{self, DatabaseSpec, DriverGroup};
use crate::widgets::text_input::TextInput;
use crate::{Database, Language, ProjectConfig, RunMode};

use super::{Focus, StepHandler, StepResult, render_choice_line};

const INDENT_SUBPANEL: u16 = 4;
const INDENT_ROW: u16 = 6;
const RUN_MODE_CHOICES: &[RunMode] = &[RunMode::Docker, RunMode::Native, RunMode::Managed];

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
}

#[derive(Debug)]
pub struct DatabaseHandler {
    cursor: usize,
    expanded: Option<Database>,
    focus: Focus,
    row_cursor: usize,
    col_cursor: usize,
    run_mode: RunMode,
    /// Toggled drivers paired with their language. Persisted to
    /// `config.database.drivers` on commit.
    drivers: Vec<(Language, &'static str)>,
    port_input: TextInput,
    /// Snapshot of the languages the user picked on the Languages step.
    /// Cached here because `StepHandler::render` doesn't receive a config —
    /// we need render and input to agree on which driver rows are shown.
    /// Refreshed by `restore_from_config` (entry to this step) and by
    /// every `handle_input` call.
    upstream_languages: Vec<Language>,
}

impl Default for DatabaseHandler {
    fn default() -> Self {
        Self {
            cursor: 0,
            expanded: None,
            focus: Focus::Choice,
            row_cursor: 0,
            col_cursor: 0,
            run_mode: RunMode::Docker,
            drivers: Vec::new(),
            port_input: TextInput::new("Port"),
            upstream_languages: Vec::new(),
        }
    }
}

impl DatabaseHandler {
    /// Restore handler state from the saved config when the user navigates back.
    /// If the previously selected database has sub-fields, re-expand it and
    /// rehydrate run mode / drivers / port from `config.database`.
    pub fn restore_from_config(&mut self, config: &ProjectConfig) {
        self.refresh_upstream(config);
        let Some(database) = config.database.database else {
            return;
        };
        self.cursor = Database::VARIANTS
            .iter()
            .position(|v| *v == database)
            .unwrap_or(0);

        match database {
            Database::None => {
                self.expanded = None;
                self.focus = Focus::Choice;
            }
            db => {
                self.expanded = Some(db);
                self.focus = Focus::SubField(0);
                self.row_cursor = 0;
                self.col_cursor = 0;
                self.run_mode = config.database.run_mode.unwrap_or(RunMode::Docker);
                self.drivers = config.database.drivers.clone();
                self.port_input = TextInput::new("Port");
                self.port_input.set_value(&config.database.port);
            }
        }
    }

    /// Refresh the cached upstream-language snapshot from the config.
    /// Called from `restore_from_config` and at the top of every
    /// `handle_input` so render and input stay in agreement.
    fn refresh_upstream(&mut self, config: &ProjectConfig) {
        self.upstream_languages.clear();
        for lc in &config.language_configs {
            self.upstream_languages.push(lc.language);
        }
    }

    /// Build the nav rows for the currently expanded database, filtering
    /// driver groups down to languages the user picked upstream (cached in
    /// `upstream_languages`). A server DB with no upstream language picks
    /// still shows RunMode + Port.
    fn nav_rows(&self) -> Vec<DbNavRow> {
        let mut rows = Vec::new();
        let Some(db) = self.expanded else { return rows };
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
        rows
    }

    fn current_row(&self) -> Option<DbNavRow> {
        self.nav_rows().get(self.row_cursor).copied()
    }

    fn col_count(&self, row: DbNavRow, spec: &DatabaseSpec) -> usize {
        match row {
            DbNavRow::RunMode => RUN_MODE_CHOICES.len(),
            DbNavRow::Drivers { group_idx, .. } => spec.driver_groups[group_idx].drivers.len(),
            DbNavRow::Port => 1,
        }
    }

    fn clamp_col(&mut self, spec: &DatabaseSpec) {
        if let Some(row) = self.current_row() {
            let max = self.col_count(row, spec).saturating_sub(1);
            self.col_cursor = self.col_cursor.min(max);
        } else {
            self.col_cursor = 0;
        }
    }

    /// True if the (`language`, `id`) pair is currently toggled on.
    fn driver_checked(&self, language: Language, id: &str) -> bool {
        self.drivers.iter().any(|(l, d)| *l == language && *d == id)
    }

    fn toggle_driver(&mut self, language: Language, id: &'static str) {
        if let Some(pos) = self
            .drivers
            .iter()
            .position(|(l, d)| *l == language && *d == id)
        {
            self.drivers.remove(pos);
        } else {
            self.drivers.push((language, id));
        }
    }

    fn port_valid(&self) -> bool {
        port_problem(self.port_input.value()).is_none()
    }

    fn validation_msg(&self) -> &'static str {
        port_problem(self.port_input.value()).unwrap_or("")
    }

    fn commit_to_config(&self, config: &mut ProjectConfig) {
        let database = self.expanded.unwrap_or(Database::None);
        let spec = db_registry::spec_for(database);
        config.database = crate::DatabaseConfig {
            database: Some(database),
            run_mode: spec.supports_run_mode.then_some(self.run_mode),
            drivers: if spec.driver_groups.is_empty() {
                Vec::new()
            } else {
                self.drivers.clone()
            },
            port: if spec.default_port.is_some() {
                self.port_input.value().to_string()
            } else {
                String::new()
            },
        };
    }

    // --- Choice focus ---

    fn handle_choice(&mut self, key: KeyCode, config: &mut ProjectConfig) -> StepResult {
        match key {
            KeyCode::Char('q') => StepResult::Quit,
            KeyCode::Down | KeyCode::Char('j') => {
                if self.cursor + 1 < Database::VARIANTS.len() {
                    self.cursor += 1;
                }
                StepResult::Continue
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.cursor = self.cursor.saturating_sub(1);
                StepResult::Continue
            }
            KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Right | KeyCode::Char('l') => {
                let choice = Database::VARIANTS[self.cursor];
                if choice == Database::None {
                    self.expanded = None;
                    config.database = crate::DatabaseConfig {
                        database: Some(Database::None),
                        run_mode: None,
                        drivers: Vec::new(),
                        port: String::new(),
                    };
                    return StepResult::Done;
                }
                self.expanded = Some(choice);
                self.focus = Focus::SubField(0);
                self.row_cursor = 0;
                self.col_cursor = 0;
                StepResult::Continue
            }
            KeyCode::Left | KeyCode::Char('h') => {
                if self.expanded.is_some() {
                    self.expanded = None;
                    StepResult::Continue
                } else {
                    StepResult::Back
                }
            }
            _ => StepResult::Continue,
        }
    }

    // --- SubField focus ---

    fn handle_subfield(&mut self, key: KeyEvent, config: &mut ProjectConfig) -> StepResult {
        let Some(database) = self.expanded else {
            return StepResult::Continue;
        };
        let spec = db_registry::spec_for(database);
        let rows = self.nav_rows();

        // Universal nav keys.
        match key.code {
            KeyCode::Esc => {
                self.focus = Focus::Choice;
                return StepResult::Continue;
            }
            KeyCode::Up => {
                if self.row_cursor == 0 {
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
            KeyCode::Enter => {
                if !self.port_valid() {
                    return StepResult::Continue;
                }
                self.commit_to_config(config);
                return StepResult::Done;
            }
            _ => {}
        }

        // Row-kind dispatch.
        let Some(row) = rows.get(self.row_cursor).copied() else {
            return StepResult::Continue;
        };
        match row {
            DbNavRow::RunMode => self.handle_run_mode_key(key.code),
            DbNavRow::Drivers { language, group_idx } => {
                self.handle_drivers_key(key.code, language, &spec.driver_groups[group_idx])
            }
            DbNavRow::Port => self.handle_port_key(key.code),
        }
    }

    fn handle_run_mode_key(&mut self, key: KeyCode) -> StepResult {
        match key {
            KeyCode::Char('q') => StepResult::Quit,
            KeyCode::Char('j') | KeyCode::Char('k') => {
                // j/k duplicate Up/Down — handled by the caller.
                StepResult::Continue
            }
            KeyCode::Left | KeyCode::Char('h') => {
                let idx = RUN_MODE_CHOICES
                    .iter()
                    .position(|m| *m == self.run_mode)
                    .unwrap_or(0);
                if idx > 0 {
                    self.run_mode = RUN_MODE_CHOICES[idx - 1];
                }
                StepResult::Continue
            }
            KeyCode::Right | KeyCode::Char('l') => {
                let idx = RUN_MODE_CHOICES
                    .iter()
                    .position(|m| *m == self.run_mode)
                    .unwrap_or(0);
                if idx + 1 < RUN_MODE_CHOICES.len() {
                    self.run_mode = RUN_MODE_CHOICES[idx + 1];
                }
                StepResult::Continue
            }
            KeyCode::Char(' ') => {
                let idx = RUN_MODE_CHOICES
                    .iter()
                    .position(|m| *m == self.run_mode)
                    .unwrap_or(0);
                self.run_mode = RUN_MODE_CHOICES[(idx + 1) % RUN_MODE_CHOICES.len()];
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
            KeyCode::Char(' ') => {
                if let Some(driver) = group.drivers.get(self.col_cursor) {
                    self.toggle_driver(language, driver.id);
                }
                StepResult::Continue
            }
            _ => StepResult::Continue,
        }
    }

    fn handle_port_key(&mut self, key: KeyCode) -> StepResult {
        match key {
            // Letters typed into the port input would be captured by `Char(c)`
            // below — the validator catches non-digit values. q only quits
            // when the input is empty (otherwise the user is typing a port
            // and "q" would be a literal character). For consistency with the
            // VCS text-input convention we forward q to the input as a char.
            KeyCode::Char(c) => {
                self.port_input.handle_input(KeyCode::Char(c));
                StepResult::Continue
            }
            KeyCode::Backspace
            | KeyCode::Delete
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End => {
                self.port_input.handle_input(key);
                StepResult::Continue
            }
            _ => StepResult::Continue,
        }
    }

    /// Cycle the 2D cursor through every (row, col) position as a flat
    /// linear sequence. Used by Tab / BackTab.
    fn advance_flattened(&mut self, rows: &[DbNavRow], spec: &DatabaseSpec, backward: bool) {
        if rows.is_empty() {
            return;
        }
        let mut flat: Vec<(usize, usize)> = Vec::new();
        for (r, row) in rows.iter().enumerate() {
            for c in 0..self.col_count(*row, spec) {
                flat.push((r, c));
            }
        }
        if flat.is_empty() {
            return;
        }
        let pos = flat
            .iter()
            .position(|(r, c)| *r == self.row_cursor && *c == self.col_cursor)
            .unwrap_or(0);
        let next = if backward {
            if pos == 0 { flat.len() - 1 } else { pos - 1 }
        } else {
            (pos + 1) % flat.len()
        };
        let (r, c) = flat[next];
        self.row_cursor = r;
        self.col_cursor = c;
    }

    // --- Rendering ---

    fn render_run_mode_row(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let mut spans = vec![ratatui::text::Span::raw("Run mode: ")];
        for (i, mode) in RUN_MODE_CHOICES.iter().enumerate() {
            let glyph = if *mode == self.run_mode { "●" } else { "○" };
            let cell = format!("{glyph} {mode}");
            let style = if focused && self.col_cursor == i {
                Style::default().fg(Color::Black).bg(Color::White)
            } else if focused {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            spans.push(ratatui::text::Span::styled(cell, style));
            if i + 1 < RUN_MODE_CHOICES.len() {
                spans.push(ratatui::text::Span::raw("   "));
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
        // First sub-row: language label.
        let label_rect = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(
                Line::from(format!("{language}:")).style(Style::default().add_modifier(Modifier::BOLD)),
            ),
            label_rect,
        );

        // Second sub-row: checkboxes.
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
            spans.push(ratatui::text::Span::styled(cell, style));
            if i + 1 < group.drivers.len() {
                spans.push(ratatui::text::Span::raw("   "));
            }
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), row_rect);
    }
}

impl StepHandler for DatabaseHandler {
    fn render(&self, frame: &mut Frame, area: Rect) {
        // Render walks the cached `upstream_languages` snapshot rather than
        // the live config (the trait gives us no `&ProjectConfig`). The
        // snapshot is refreshed by `restore_from_config` on entry to this
        // step and by every `handle_input` call, so it stays in lockstep
        // with what the input layer's `nav_rows()` sees.
        self.render_with_areas(frame, area);
    }

    fn handle_input(&mut self, key: KeyEvent, config: &mut ProjectConfig) -> StepResult {
        // Refresh the upstream-language cache so render and input agree on
        // which driver rows to show. Cheap (a small Vec clone).
        self.refresh_upstream(config);
        match self.focus {
            Focus::Choice => self.handle_choice(key.code, config),
            Focus::SubField(_) => self.handle_subfield(key, config),
            Focus::Browsing => unreachable!(),
        }
    }

    fn in_details(&self) -> bool {
        matches!(self.focus, Focus::SubField(_))
    }

    fn is_expanded(&self) -> bool {
        self.expanded.is_some()
    }

    fn planned_actions(&self, config: &ProjectConfig) -> Vec<String> {
        let Some(database) = config.database.database else {
            return vec![];
        };
        if database == Database::None {
            return vec![];
        }
        let mut actions = vec![format!("Configure {database}")];
        if let Some(rm) = config.database.run_mode {
            actions.push(format!("Run mode: {rm}"));
        }
        if !config.database.drivers.is_empty() {
            let labels: Vec<String> = config
                .database
                .drivers
                .iter()
                .filter_map(|(lang, id)| db_registry::driver_by_id(*lang, id).map(|d| d.label.to_string()))
                .collect();
            if !labels.is_empty() {
                actions.push(format!("Drivers: {}", labels.join(", ")));
            }
        }
        actions
    }

    fn execute(&self, _config: &ProjectConfig) -> std::io::Result<()> {
        // Database setup is post-MVP; the wizard only collects choices.
        Ok(())
    }
}

impl DatabaseHandler {
    /// Walks `Database::VARIANTS`, painting each choice line; an expanded
    /// choice is followed by its sub-panel rendered indented below. Driver
    /// rows are emitted exactly when `nav_rows()` would emit them — i.e.
    /// only for languages in the cached `upstream_languages` snapshot, so
    /// render and input agree on the visible row set.
    fn render_with_areas(&self, frame: &mut Frame, area: Rect) {
        let rows = self.nav_rows();
        let constraints = self.layout_constraints_for(&rows);
        let areas = Layout::vertical(constraints).split(area);
        let mut idx = 0;
        let focused_row =
            |r: usize| matches!(self.focus, Focus::SubField(_)) && self.row_cursor == r;

        for (i, db) in Database::VARIANTS.iter().enumerate() {
            let highlighted = matches!(self.focus, Focus::Choice) && self.cursor == i;
            render_choice_line(frame, areas[idx], db, highlighted);
            idx += 1;

            if self.expanded != Some(*db) {
                continue;
            }

            for (row_idx, row) in rows.iter().enumerate() {
                match *row {
                    DbNavRow::RunMode => {
                        let row_area = Rect {
                            x: areas[idx].x + INDENT_SUBPANEL,
                            width: areas[idx].width.saturating_sub(INDENT_SUBPANEL),
                            ..areas[idx]
                        };
                        self.render_run_mode_row(frame, row_area, focused_row(row_idx));
                        idx += 1;
                    }
                    DbNavRow::Drivers { language, group_idx } => {
                        let spec = db_registry::spec_for(*db);
                        let group = &spec.driver_groups[group_idx];
                        let row_area = Rect {
                            x: areas[idx].x + INDENT_ROW,
                            width: areas[idx].width.saturating_sub(INDENT_ROW),
                            ..areas[idx]
                        };
                        self.render_drivers_row(
                            frame,
                            row_area,
                            language,
                            group,
                            focused_row(row_idx),
                        );
                        idx += 1;
                    }
                    DbNavRow::Port => {
                        let row_area = Rect {
                            x: areas[idx].x + INDENT_SUBPANEL,
                            width: areas[idx].width.saturating_sub(INDENT_SUBPANEL),
                            ..areas[idx]
                        };
                        self.port_input.render(frame, row_area, focused_row(row_idx));
                        idx += 1;
                    }
                }
            }
        }

        idx += 1; // spacer
        if idx < areas.len() {
            let msg = self.validation_msg();
            if !msg.is_empty() {
                let style = Style::default().fg(Color::Yellow);
                frame.render_widget(Paragraph::new(Line::from(msg).style(style)), areas[idx]);
            }
        }
    }

    fn layout_constraints_for(&self, rows: &[DbNavRow]) -> Vec<Constraint> {
        let mut constraints = Vec::new();
        for db in Database::VARIANTS {
            constraints.push(Constraint::Length(1)); // choice line
            if self.expanded == Some(*db) {
                for row in rows {
                    constraints.push(match row {
                        DbNavRow::RunMode => Constraint::Length(1),
                        DbNavRow::Drivers { .. } => Constraint::Length(2),
                        DbNavRow::Port => Constraint::Length(3),
                    });
                }
            }
        }
        constraints.push(Constraint::Length(1)); // spacer
        constraints.push(Constraint::Length(1)); // validation line
        constraints.push(Constraint::Min(0));
        constraints
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DatabaseConfig, LanguageConfig};
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

    fn cursor_for(db: Database) -> usize {
        Database::VARIANTS.iter().position(|v| *v == db).unwrap()
    }

    // --- Defaults ---

    #[test]
    fn default_state() {
        let h = DatabaseHandler::default();
        assert_eq!(h.focus, Focus::Choice);
        assert!(h.expanded.is_none());
        assert_eq!(h.cursor, 0);
        assert_eq!(h.run_mode, RunMode::Docker);
        assert!(h.drivers.is_empty());
        assert_eq!(h.port_input.value(), "");
    }

    // --- Choice navigation ---

    #[test]
    fn choice_down_clamps_at_last() {
        let mut h = DatabaseHandler::default();
        let mut c = ProjectConfig::default();
        for _ in 0..20 {
            h.handle_input(key(KeyCode::Down), &mut c);
        }
        assert_eq!(h.cursor, Database::VARIANTS.len() - 1);
    }

    #[test]
    fn choice_up_clamps_at_zero() {
        let mut h = DatabaseHandler {
            cursor: 3,
            ..Default::default()
        };
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Up), &mut c);
        h.handle_input(key(KeyCode::Up), &mut c);
        h.handle_input(key(KeyCode::Up), &mut c);
        h.handle_input(key(KeyCode::Up), &mut c);
        assert_eq!(h.cursor, 0);
    }

    // --- None commits immediately ---

    #[test]
    fn none_commits_and_returns_done() {
        let mut h = DatabaseHandler::default();
        let mut c = ProjectConfig::default();
        h.cursor = cursor_for(Database::None);
        let r = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(r, StepResult::Done));
        assert_eq!(c.database.database, Some(Database::None));
        assert!(c.database.run_mode.is_none());
        assert!(c.database.drivers.is_empty());
        assert!(c.database.port.is_empty());
    }

    #[test]
    fn space_on_none_also_commits() {
        let mut h = DatabaseHandler::default();
        let mut c = ProjectConfig::default();
        h.cursor = cursor_for(Database::None);
        let r = h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert!(matches!(r, StepResult::Done));
        assert_eq!(c.database.database, Some(Database::None));
    }

    // --- Expansion ---

    #[test]
    fn enter_on_postgres_expands() {
        let mut h = DatabaseHandler::default();
        let mut c = ProjectConfig::default();
        h.cursor = cursor_for(Database::PostgreSQL);
        h.handle_input(key(KeyCode::Enter), &mut c);
        assert_eq!(h.expanded, Some(Database::PostgreSQL));
        assert_eq!(h.focus, Focus::SubField(0));
    }

    #[test]
    fn left_collapses_then_backs() {
        let mut h = DatabaseHandler::default();
        let mut c = ProjectConfig::default();
        h.cursor = cursor_for(Database::PostgreSQL);
        h.handle_input(key(KeyCode::Enter), &mut c); // expand
        h.focus = Focus::Choice;
        let r = h.handle_input(key(KeyCode::Left), &mut c);
        assert!(matches!(r, StepResult::Continue));
        assert!(h.expanded.is_none());
        let r = h.handle_input(key(KeyCode::Left), &mut c);
        assert!(matches!(r, StepResult::Back));
    }

    // --- nav_rows shape ---

    /// Build a handler whose `upstream_languages` snapshot reflects `langs`.
    fn handler_with_upstream(expanded: Database, langs: &[Language]) -> DatabaseHandler {
        let mut h = DatabaseHandler {
            expanded: Some(expanded),
            ..Default::default()
        };
        h.upstream_languages = langs.to_vec();
        h
    }

    #[test]
    fn server_db_with_no_languages_has_run_mode_and_port() {
        let h = handler_with_upstream(Database::PostgreSQL, &[]);
        let rows = h.nav_rows();
        assert_eq!(rows.len(), 2);
        assert!(matches!(rows[0], DbNavRow::RunMode));
        assert!(matches!(rows[1], DbNavRow::Port));
    }

    #[test]
    fn server_db_with_python_includes_python_drivers() {
        let h = handler_with_upstream(Database::PostgreSQL, &[Language::Python]);
        let rows = h.nav_rows();
        assert_eq!(rows.len(), 3);
        assert!(matches!(rows[0], DbNavRow::RunMode));
        assert!(matches!(rows[1], DbNavRow::Drivers { language: Language::Python, .. }));
        assert!(matches!(rows[2], DbNavRow::Port));
    }

    #[test]
    fn sqlite_skips_run_mode_and_port() {
        let h = handler_with_upstream(Database::SQLite, &[Language::Python, Language::Rust]);
        let rows = h.nav_rows();
        assert_eq!(rows.len(), 2);
        assert!(matches!(rows[0], DbNavRow::Drivers { language: Language::Python, .. }));
        assert!(matches!(rows[1], DbNavRow::Drivers { language: Language::Rust, .. }));
    }

    #[test]
    fn drivers_filtered_by_upstream_languages() {
        // User selected only Rust upstream → Python driver row is omitted.
        let h = handler_with_upstream(Database::PostgreSQL, &[Language::Rust]);
        let rows = h.nav_rows();
        let driver_rows: Vec<_> = rows
            .iter()
            .filter(|r| matches!(r, DbNavRow::Drivers { .. }))
            .collect();
        assert_eq!(driver_rows.len(), 1);
        assert!(matches!(
            driver_rows[0],
            DbNavRow::Drivers { language: Language::Rust, .. }
        ));
    }

    // --- Run mode interaction ---

    #[test]
    fn run_mode_left_right() {
        let mut h = DatabaseHandler {
            expanded: Some(Database::PostgreSQL),
            focus: Focus::SubField(0),
            ..Default::default()
        };
        let mut c = ProjectConfig::default();
        // Start: Docker. Right -> Native.
        h.handle_input(key(KeyCode::Right), &mut c);
        assert_eq!(h.run_mode, RunMode::Native);
        // Right -> Managed.
        h.handle_input(key(KeyCode::Right), &mut c);
        assert_eq!(h.run_mode, RunMode::Managed);
        // Right at last is no-op.
        h.handle_input(key(KeyCode::Right), &mut c);
        assert_eq!(h.run_mode, RunMode::Managed);
        // Left -> Native.
        h.handle_input(key(KeyCode::Left), &mut c);
        assert_eq!(h.run_mode, RunMode::Native);
        // Left -> Docker.
        h.handle_input(key(KeyCode::Left), &mut c);
        assert_eq!(h.run_mode, RunMode::Docker);
        // Left at first is no-op.
        h.handle_input(key(KeyCode::Left), &mut c);
        assert_eq!(h.run_mode, RunMode::Docker);
    }

    #[test]
    fn run_mode_space_cycles_with_wrap() {
        let mut h = DatabaseHandler {
            expanded: Some(Database::PostgreSQL),
            focus: Focus::SubField(0),
            run_mode: RunMode::Managed,
            ..Default::default()
        };
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert_eq!(h.run_mode, RunMode::Docker); // wraps
    }

    // --- Driver toggling ---

    #[test]
    fn driver_space_toggles() {
        let mut h = DatabaseHandler {
            expanded: Some(Database::PostgreSQL),
            focus: Focus::SubField(0),
            row_cursor: 1, // RunMode + Drivers(Python) → row 1 = drivers
            ..Default::default()
        };
        let mut c = config_with_languages(&[Language::Python]);
        // col 0 of Python drivers is psycopg.
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert_eq!(h.drivers, vec![(Language::Python, "psycopg")]);
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert!(h.drivers.is_empty());
    }

    #[test]
    fn driver_left_right_moves_col_cursor() {
        let mut h = DatabaseHandler {
            expanded: Some(Database::PostgreSQL),
            focus: Focus::SubField(0),
            row_cursor: 1,
            ..Default::default()
        };
        let mut c = config_with_languages(&[Language::Python]);
        assert_eq!(h.col_cursor, 0);
        h.handle_input(key(KeyCode::Right), &mut c);
        assert_eq!(h.col_cursor, 1);
        h.handle_input(key(KeyCode::Left), &mut c);
        assert_eq!(h.col_cursor, 0);
        h.handle_input(key(KeyCode::Left), &mut c); // clamped at 0
        assert_eq!(h.col_cursor, 0);
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

    #[test]
    fn enter_with_invalid_port_does_not_advance() {
        let mut h = DatabaseHandler {
            expanded: Some(Database::PostgreSQL),
            focus: Focus::SubField(0),
            row_cursor: 1, // no upstream langs → rows = [RunMode, Port]
            ..Default::default()
        };
        h.port_input.set_value("99999");
        let mut c = ProjectConfig::default();
        let r = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(r, StepResult::Continue));
        assert!(c.database.database.is_none(), "config should not be committed");
    }

    #[test]
    fn enter_with_valid_port_commits_and_advances() {
        let mut h = DatabaseHandler {
            expanded: Some(Database::PostgreSQL),
            focus: Focus::SubField(0),
            row_cursor: 1,
            ..Default::default()
        };
        h.port_input.set_value("5433");
        let mut c = ProjectConfig::default();
        let r = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(r, StepResult::Done));
        assert_eq!(c.database.database, Some(Database::PostgreSQL));
        assert_eq!(c.database.run_mode, Some(RunMode::Docker));
        assert_eq!(c.database.port, "5433");
    }

    #[test]
    fn enter_with_empty_port_commits() {
        let mut h = DatabaseHandler {
            expanded: Some(Database::PostgreSQL),
            focus: Focus::SubField(0),
            row_cursor: 1,
            ..Default::default()
        };
        let mut c = ProjectConfig::default();
        let r = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(r, StepResult::Done));
        assert_eq!(c.database.database, Some(Database::PostgreSQL));
        assert!(c.database.port.is_empty());
    }

    // --- SQLite commit ---

    #[test]
    fn sqlite_commits_without_port_or_run_mode() {
        let mut h = DatabaseHandler::default();
        let mut c = config_with_languages(&[Language::Rust]);
        h.cursor = cursor_for(Database::SQLite);
        h.handle_input(key(KeyCode::Enter), &mut c); // expand
        let r = h.handle_input(key(KeyCode::Enter), &mut c); // commit immediately
        assert!(matches!(r, StepResult::Done));
        assert_eq!(c.database.database, Some(Database::SQLite));
        assert!(c.database.run_mode.is_none());
        assert!(c.database.port.is_empty());
    }

    // --- Esc returns to choice ---

    #[test]
    fn esc_returns_to_choice() {
        let mut h = DatabaseHandler::default();
        let mut c = ProjectConfig::default();
        h.cursor = cursor_for(Database::PostgreSQL);
        h.handle_input(key(KeyCode::Enter), &mut c);
        assert_eq!(h.focus, Focus::SubField(0));
        h.handle_input(key(KeyCode::Esc), &mut c);
        assert_eq!(h.focus, Focus::Choice);
        assert_eq!(h.expanded, Some(Database::PostgreSQL));
    }

    // --- Up from row 0 returns to choice ---

    #[test]
    fn up_from_row_zero_returns_to_choice() {
        let mut h = DatabaseHandler::default();
        let mut c = ProjectConfig::default();
        h.cursor = cursor_for(Database::PostgreSQL);
        h.handle_input(key(KeyCode::Enter), &mut c);
        h.handle_input(key(KeyCode::Up), &mut c);
        assert_eq!(h.focus, Focus::Choice);
        assert_eq!(h.expanded, Some(Database::PostgreSQL));
    }

    // --- Tab cycling ---

    #[test]
    fn tab_cycles_flat_positions() {
        let mut h = DatabaseHandler::default();
        let mut c = config_with_languages(&[Language::Python]);
        h.cursor = cursor_for(Database::PostgreSQL);
        h.handle_input(key(KeyCode::Enter), &mut c); // expand → SubField(0)
        // RunMode has 3 cols → 3 flat positions; Drivers(Python) has 4 → 4
        // positions; Port has 1 → total 8.
        let mut seen = vec![(h.row_cursor, h.col_cursor)];
        for _ in 0..7 {
            h.handle_input(key(KeyCode::Tab), &mut c);
            seen.push((h.row_cursor, h.col_cursor));
        }
        assert_eq!(seen.len(), 8);
        h.handle_input(key(KeyCode::Tab), &mut c);
        // Wraps back to (0, 0).
        assert_eq!((h.row_cursor, h.col_cursor), (0, 0));
    }

    // --- Restore from config ---

    #[test]
    fn restore_re_expands_previously_selected_db() {
        let mut h = DatabaseHandler::default();
        let c = ProjectConfig {
            database: DatabaseConfig {
                database: Some(Database::MySQL),
                run_mode: Some(RunMode::Native),
                drivers: vec![(Language::Python, "pymysql")],
                port: "3307".to_string(),
            },
            ..Default::default()
        };
        h.restore_from_config(&c);
        assert_eq!(h.cursor, cursor_for(Database::MySQL));
        assert_eq!(h.expanded, Some(Database::MySQL));
        assert_eq!(h.focus, Focus::SubField(0));
        assert_eq!(h.run_mode, RunMode::Native);
        assert_eq!(h.drivers, vec![(Language::Python, "pymysql")]);
        assert_eq!(h.port_input.value(), "3307");
    }

    #[test]
    fn restore_skips_expand_for_none() {
        let mut h = DatabaseHandler::default();
        let c = ProjectConfig {
            database: DatabaseConfig {
                database: Some(Database::None),
                ..Default::default()
            },
            ..Default::default()
        };
        h.restore_from_config(&c);
        assert_eq!(h.cursor, cursor_for(Database::None));
        assert!(h.expanded.is_none());
        assert_eq!(h.focus, Focus::Choice);
    }

    // --- Quit ---

    #[test]
    fn q_in_choice_quits() {
        let mut h = DatabaseHandler::default();
        let mut c = ProjectConfig::default();
        let r = h.handle_input(key(KeyCode::Char('q')), &mut c);
        assert!(matches!(r, StepResult::Quit));
    }

    // --- planned_actions ---

    #[test]
    fn planned_actions_database_unset() {
        let h = DatabaseHandler::default();
        let c = ProjectConfig::default();
        assert!(h.planned_actions(&c).is_empty());
    }

    #[test]
    fn planned_actions_database_none() {
        let h = DatabaseHandler::default();
        let c = ProjectConfig {
            database: DatabaseConfig {
                database: Some(Database::None),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(h.planned_actions(&c).is_empty());
    }

    #[test]
    fn planned_actions_postgres_with_drivers() {
        let h = DatabaseHandler::default();
        let c = ProjectConfig {
            database: DatabaseConfig {
                database: Some(Database::PostgreSQL),
                run_mode: Some(RunMode::Docker),
                drivers: vec![(Language::Python, "psycopg"), (Language::Rust, "sqlx")],
                port: String::new(),
            },
            ..Default::default()
        };
        let actions = h.planned_actions(&c);
        assert!(actions[0].contains("PostgreSQL"));
        assert!(actions.iter().any(|a| a.contains("Docker")));
        assert!(actions.iter().any(|a| a.contains("psycopg") && a.contains("sqlx")));
    }

    // --- execute is a no-op ---

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
