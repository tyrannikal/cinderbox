use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use strum::VariantArray;

use crate::registry::{CommonDep, LanguageSpec, Tool, ToolCategory, spec_for};
use crate::widgets::text_input::TextInput;
use crate::{Language, LanguageConfig, ProjectConfig};

use super::{CURSOR_BLANK, CURSOR_MARKER, Focus, StepHandler, StepResult};

const DEPS_PER_ROW: usize = 3;
const INDENT_SUBPANEL: u16 = 4;
const INDENT_ROW: u16 = 6;

/// One interactive row inside an expanded language's sub-panel.
/// Drives the handler's 2D cursor (row_cursor × col_cursor) and the render walk.
#[derive(Debug, Clone, Copy)]
enum NavRow {
    /// The N checkboxes for a single tool category.
    CategoryTools { cat_idx: usize },
    /// A (possibly wrapped) slice of common deps — `[start, end)` into `spec.common_deps`.
    CommonDeps { start: usize, end: usize },
    /// The free-text custom-deps input.
    CustomDepsInput,
}

impl NavRow {
    fn col_count(&self, spec: &LanguageSpec) -> usize {
        match self {
            NavRow::CategoryTools { cat_idx } => spec.categories[*cat_idx].tools.len(),
            NavRow::CommonDeps { start, end } => end - start,
            NavRow::CustomDepsInput => 1,
        }
    }
}

/// Build the interactive rows for a language's spec. Returns an empty vec when the
/// spec has no categories AND no common deps — which is the "empty-spec" case.
fn nav_rows(spec: &LanguageSpec) -> Vec<NavRow> {
    if spec.categories.is_empty() && spec.common_deps.is_empty() {
        return Vec::new();
    }
    let mut rows: Vec<NavRow> = Vec::new();
    for cat_idx in 0..spec.categories.len() {
        rows.push(NavRow::CategoryTools { cat_idx });
    }
    let mut start = 0;
    while start < spec.common_deps.len() {
        let end = (start + DEPS_PER_ROW).min(spec.common_deps.len());
        rows.push(NavRow::CommonDeps { start, end });
        start = end;
    }
    rows.push(NavRow::CustomDepsInput);
    rows
}

#[derive(Debug)]
pub struct LanguagesHandler {
    cursor: usize,
    selected: Vec<Language>,
    expanded: Option<Language>,
    focus: Focus,
    row_cursor: usize,
    col_cursor: usize,
    custom_deps_input: TextInput,
    scratch: Vec<LanguageConfig>,
}

impl Default for LanguagesHandler {
    fn default() -> Self {
        Self {
            cursor: 0,
            selected: Vec::new(),
            expanded: None,
            focus: Focus::Choice,
            row_cursor: 0,
            col_cursor: 0,
            custom_deps_input: TextInput::new("Custom deps (comma-separated)"),
            scratch: Vec::new(),
        }
    }
}

impl LanguagesHandler {
    /// Restore state from existing config when navigating back.
    pub fn restore_from_config(&mut self, config: &ProjectConfig) {
        self.selected.clear();
        self.selected
            .extend(config.language_configs.iter().map(|lc| lc.language));
        self.scratch = config.language_configs.clone();
        self.cursor = 0;
        self.expanded = None;
        self.focus = Focus::Choice;
        self.row_cursor = 0;
        self.col_cursor = 0;
        self.custom_deps_input = TextInput::new("Custom deps (comma-separated)");
    }

    fn is_selected(&self, lang: Language) -> bool {
        self.selected.contains(&lang)
    }

    fn toggle_at_cursor(&mut self) {
        let lang = Language::VARIANTS[self.cursor];
        if let Some(pos) = self.selected.iter().position(|l| *l == lang) {
            self.selected.remove(pos);
        } else {
            self.selected.push(lang);
        }
    }

    fn scratch_for(&self, lang: Language) -> Option<&LanguageConfig> {
        self.scratch.iter().find(|lc| lc.language == lang)
    }

    /// Return (and create if absent) the scratch entry for `lang`.
    fn scratch_mut_for(&mut self, lang: Language) -> &mut LanguageConfig {
        if let Some(pos) = self.scratch.iter().position(|lc| lc.language == lang) {
            &mut self.scratch[pos]
        } else {
            self.scratch.push(LanguageConfig::new(lang));
            let last = self.scratch.len() - 1;
            &mut self.scratch[last]
        }
    }

    fn expand(&mut self, lang: Language) {
        // Ensure scratch has an entry for this language.
        self.scratch_mut_for(lang);
        let starting_value = self
            .scratch_for(lang)
            .map(|lc| lc.custom_deps.clone())
            .unwrap_or_default();
        self.expanded = Some(lang);
        self.focus = Focus::SubField(0);
        self.row_cursor = 0;
        self.col_cursor = 0;
        self.custom_deps_input = TextInput::new("Custom deps (comma-separated)");
        self.custom_deps_input.set_value(starting_value);
    }

    fn collapse_persist(&mut self) {
        if let Some(lang) = self.expanded {
            let value = self.custom_deps_input.value.clone();
            self.scratch_mut_for(lang).custom_deps = value;
        }
        self.expanded = None;
        self.focus = Focus::Choice;
    }

    fn commit_to_config(&mut self, config: &mut ProjectConfig) {
        // Persist the in-progress text input into scratch so the committed
        // value reflects the latest keystrokes (in case Enter was pressed
        // from inside the text input without an explicit collapse first).
        if let Some(lang) = self.expanded {
            let value = self.custom_deps_input.value.clone();
            self.scratch_mut_for(lang).custom_deps = value;
        }
        let mut new_configs: Vec<LanguageConfig> = Vec::with_capacity(self.selected.len());
        for lang in &self.selected {
            let cfg = match self.scratch.iter().position(|lc| lc.language == *lang) {
                Some(idx) => self.scratch[idx].clone(),
                None => LanguageConfig::new(*lang),
            };
            new_configs.push(cfg);
        }
        config.language_configs = new_configs;
    }

    fn current_nav_rows(&self) -> Vec<NavRow> {
        self.expanded.map(|l| nav_rows(spec_for(l))).unwrap_or_default()
    }

    fn current_row(&self) -> Option<NavRow> {
        self.current_nav_rows().get(self.row_cursor).copied()
    }

    fn clamp_col(&mut self, spec: &LanguageSpec) {
        if let Some(row) = self.current_row() {
            let max = row.col_count(spec).saturating_sub(1);
            self.col_cursor = self.col_cursor.min(max);
        } else {
            self.col_cursor = 0;
        }
    }

    /// Toggle the currently-focused checkbox (tool or common dep).
    /// No-op if the focused row is a text input.
    fn toggle_focused_checkbox(&mut self) {
        let Some(lang) = self.expanded else { return };
        let Some(row) = self.current_row() else {
            return;
        };
        let spec = spec_for(lang);
        match row {
            NavRow::CategoryTools { cat_idx } => {
                let tool_id = spec.categories[cat_idx].tools[self.col_cursor].id;
                let scratch = self.scratch_mut_for(lang);
                if let Some(pos) = scratch.tools.iter().position(|t| *t == tool_id) {
                    scratch.tools.remove(pos);
                } else {
                    scratch.tools.push(tool_id);
                }
            }
            NavRow::CommonDeps { start, .. } => {
                let dep_id = spec.common_deps[start + self.col_cursor].id;
                let scratch = self.scratch_mut_for(lang);
                if let Some(pos) = scratch.common_deps.iter().position(|d| *d == dep_id) {
                    scratch.common_deps.remove(pos);
                } else {
                    scratch.common_deps.push(dep_id);
                }
            }
            NavRow::CustomDepsInput => {}
        }
    }

    fn is_tool_checked(&self, lang: Language, tool: &Tool) -> bool {
        self.scratch_for(lang)
            .is_some_and(|lc| lc.tools.contains(&tool.id))
    }

    fn is_common_dep_checked(&self, lang: Language, dep: &CommonDep) -> bool {
        self.scratch_for(lang)
            .is_some_and(|lc| lc.common_deps.contains(&dep.id))
    }

    // --- Input handling ---

    fn handle_choice_input(&mut self, key: KeyCode) -> StepResult {
        match key {
            KeyCode::Char('q') => StepResult::Quit,
            KeyCode::Down | KeyCode::Char('j') => {
                if self.cursor + 1 < Language::VARIANTS.len() {
                    self.cursor += 1;
                }
                StepResult::Continue
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.cursor = self.cursor.saturating_sub(1);
                StepResult::Continue
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.toggle_at_cursor();
                StepResult::Continue
            }
            KeyCode::Right | KeyCode::Char('l') => {
                let lang = Language::VARIANTS[self.cursor];
                if self.is_selected(lang) {
                    self.expand(lang);
                }
                StepResult::Continue
            }
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

    fn handle_subfield_input(&mut self, key: KeyEvent, config: &mut ProjectConfig) -> StepResult {
        let Some(lang) = self.expanded else {
            return StepResult::Continue;
        };
        let spec = spec_for(lang);
        let rows = nav_rows(spec);

        // Empty-spec: only Esc / Shift+arrow-style keys make sense.
        if rows.is_empty() {
            match key.code {
                KeyCode::Esc => {
                    self.focus = Focus::Choice;
                    return StepResult::Continue;
                }
                KeyCode::Char('q') => return StepResult::Quit,
                _ => return StepResult::Continue,
            }
        }

        let row = rows[self.row_cursor];
        let on_text = matches!(row, NavRow::CustomDepsInput);

        // Universal nav keys (apply regardless of row kind).
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
            KeyCode::Enter => {
                self.commit_to_config(config);
                return StepResult::Done;
            }
            _ => {}
        }

        // Row-kind-specific handling.
        if on_text {
            match key.code {
                KeyCode::Char(c) => {
                    self.custom_deps_input.handle_input(KeyCode::Char(c));
                }
                KeyCode::Backspace
                | KeyCode::Delete
                | KeyCode::Left
                | KeyCode::Right
                | KeyCode::Home
                | KeyCode::End => {
                    self.custom_deps_input.handle_input(key.code);
                }
                _ => {}
            }
            // Persist live — scratch stays in sync with what's on screen.
            let value = self.custom_deps_input.value.clone();
            self.scratch_mut_for(lang).custom_deps = value;
            return StepResult::Continue;
        }

        // Checkbox rows (tools or common deps).
        let col_count = row.col_count(spec);
        match key.code {
            KeyCode::Char('q') => return StepResult::Quit,
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
            KeyCode::Left | KeyCode::Char('h') => {
                self.col_cursor = self.col_cursor.saturating_sub(1);
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.col_cursor + 1 < col_count {
                    self.col_cursor += 1;
                }
            }
            KeyCode::Char(' ') => {
                self.toggle_focused_checkbox();
            }
            _ => {}
        }
        StepResult::Continue
    }

    /// Collapse without flipping focus back to Choice — used when Up escapes
    /// from the first sub-field: we want `expanded` to stay set so rendering
    /// still shows the sub-panel (with focus on the choice line above).
    /// Actually keeping expanded lets the user see both — but matches the
    /// ProjectType/VCS convention where Up from row 0 does NOT collapse.
    fn collapse_persist_keep_expanded(&mut self) {
        if let Some(lang) = self.expanded {
            let value = self.custom_deps_input.value.clone();
            self.scratch_mut_for(lang).custom_deps = value;
        }
    }

    /// Advance the 2D cursor as if the nav rows were flattened into a single
    /// linear sequence (row 0 col 0, row 0 col 1, ..., row 1 col 0, ...).
    fn advance_flattened(&mut self, rows: &[NavRow], spec: &LanguageSpec, backward: bool) {
        if rows.is_empty() {
            return;
        }
        let mut flat: Vec<(usize, usize)> = Vec::new();
        for (r, row) in rows.iter().enumerate() {
            for c in 0..row.col_count(spec) {
                flat.push((r, c));
            }
        }
        if flat.is_empty() {
            return;
        }
        let current_pos = flat
            .iter()
            .position(|(r, c)| *r == self.row_cursor && *c == self.col_cursor)
            .unwrap_or(0);
        let next = if backward {
            if current_pos == 0 { flat.len() - 1 } else { current_pos - 1 }
        } else {
            (current_pos + 1) % flat.len()
        };
        let (r, c) = flat[next];
        self.row_cursor = r;
        self.col_cursor = c;
    }

    // --- Rendering ---

    fn render_choice_line(&self, frame: &mut Frame, area: Rect, lang: Language, idx: usize) {
        let cursor_marker = if matches!(self.focus, Focus::Choice) && idx == self.cursor {
            CURSOR_MARKER
        } else {
            CURSOR_BLANK
        };
        let check = if self.is_selected(lang) { "[x]" } else { "[ ]" };
        let text = format!("{cursor_marker}{check} {lang}");
        frame.render_widget(Paragraph::new(text), area);
    }

    fn render_expanded_panel(&self, frame: &mut Frame, lang: Language, area: Rect) -> u16 {
        let spec = spec_for(lang);
        let rows = nav_rows(spec);
        let mut y = area.y;
        let bottom = area.y + area.height;

        if rows.is_empty() {
            if y < bottom {
                let rect = Rect {
                    x: area.x + INDENT_SUBPANEL,
                    y,
                    width: area.width.saturating_sub(INDENT_SUBPANEL),
                    height: 1,
                };
                frame.render_widget(
                    Paragraph::new(Line::from("(no tools yet)").style(Style::default().fg(Color::DarkGray))),
                    rect,
                );
                y += 1;
            }
            return y;
        }

        let mut nav_row_idx = 0;
        for (cat_idx, category) in spec.categories.iter().enumerate() {
            // Category label
            if y >= bottom { return y; }
            let label_rect = Rect {
                x: area.x + INDENT_SUBPANEL,
                y,
                width: area.width.saturating_sub(INDENT_SUBPANEL),
                height: 1,
            };
            frame.render_widget(
                Paragraph::new(Line::from(format!("{}:", category.name)).style(Style::default().add_modifier(Modifier::BOLD))),
                label_rect,
            );
            y += 1;

            // Tool row (checkboxes laid out horizontally)
            if y >= bottom { return y; }
            self.render_tool_row(frame, Rect {
                x: area.x + INDENT_ROW,
                y,
                width: area.width.saturating_sub(INDENT_ROW),
                height: 1,
            }, lang, category, nav_row_idx);
            y += 1;
            nav_row_idx += 1;
            let _ = cat_idx; // intentionally unused
        }

        if !spec.common_deps.is_empty() {
            if y >= bottom { return y; }
            let label_rect = Rect {
                x: area.x + INDENT_SUBPANEL,
                y,
                width: area.width.saturating_sub(INDENT_SUBPANEL),
                height: 1,
            };
            frame.render_widget(
                Paragraph::new(Line::from("Common deps:").style(Style::default().add_modifier(Modifier::BOLD))),
                label_rect,
            );
            y += 1;

            let mut start = 0;
            while start < spec.common_deps.len() {
                if y >= bottom { return y; }
                let end = (start + DEPS_PER_ROW).min(spec.common_deps.len());
                self.render_common_deps_row(
                    frame,
                    Rect {
                        x: area.x + INDENT_ROW,
                        y,
                        width: area.width.saturating_sub(INDENT_ROW),
                        height: 1,
                    },
                    lang,
                    &spec.common_deps[start..end],
                    nav_row_idx,
                );
                y += 1;
                nav_row_idx += 1;
                start = end;
            }
        }

        // Custom deps label + TextInput
        if y >= bottom { return y; }
        let label_rect = Rect {
            x: area.x + INDENT_SUBPANEL,
            y,
            width: area.width.saturating_sub(INDENT_SUBPANEL),
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(Line::from("Custom deps:").style(Style::default().add_modifier(Modifier::BOLD))),
            label_rect,
        );
        y += 1;

        if y + 2 < bottom {
            let input_rect = Rect {
                x: area.x + INDENT_ROW,
                y,
                width: area.width.saturating_sub(INDENT_ROW),
                height: 3,
            };
            let focused = matches!(self.focus, Focus::SubField(_))
                && self.row_cursor == nav_row_idx;
            self.custom_deps_input.render(frame, input_rect, focused);
            y += 3;
        }
        let _ = rows; // shape used via nav_row_idx
        y
    }

    fn render_tool_row(
        &self,
        frame: &mut Frame,
        area: Rect,
        lang: Language,
        category: &ToolCategory,
        nav_row_idx: usize,
    ) {
        let row_focused =
            matches!(self.focus, Focus::SubField(_)) && self.row_cursor == nav_row_idx;
        let mut spans: Vec<Span> = Vec::new();
        for (i, tool) in category.tools.iter().enumerate() {
            let check = if self.is_tool_checked(lang, tool) { "[x]" } else { "[ ]" };
            let is_col_focused = row_focused && self.col_cursor == i;
            let label = format!("{check} {}", tool.label);
            let span = if is_col_focused {
                Span::from(label).style(Style::default().fg(Color::Black).bg(Color::White))
            } else {
                Span::from(label)
            };
            spans.push(span);
            if i + 1 < category.tools.len() {
                spans.push(Span::from("  "));
            }
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_common_deps_row(
        &self,
        frame: &mut Frame,
        area: Rect,
        lang: Language,
        deps: &[CommonDep],
        nav_row_idx: usize,
    ) {
        let row_focused =
            matches!(self.focus, Focus::SubField(_)) && self.row_cursor == nav_row_idx;
        let mut spans: Vec<Span> = Vec::new();
        for (i, dep) in deps.iter().enumerate() {
            let check = if self.is_common_dep_checked(lang, dep) { "[x]" } else { "[ ]" };
            let is_col_focused = row_focused && self.col_cursor == i;
            let label = format!("{check} {}", dep.label);
            let span = if is_col_focused {
                Span::from(label).style(Style::default().fg(Color::Black).bg(Color::White))
            } else {
                Span::from(label)
            };
            spans.push(span);
            if i + 1 < deps.len() {
                spans.push(Span::from("  "));
            }
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }
}

impl StepHandler for LanguagesHandler {
    fn render(&self, frame: &mut Frame, area: Rect) {
        let mut y = area.y;
        let bottom = area.y + area.height;
        for (i, lang) in Language::VARIANTS.iter().enumerate() {
            if y >= bottom {
                break;
            }
            let choice_rect = Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            };
            self.render_choice_line(frame, choice_rect, *lang, i);
            y += 1;
            if self.expanded == Some(*lang) {
                let panel_area = Rect {
                    x: area.x,
                    y,
                    width: area.width,
                    height: bottom.saturating_sub(y),
                };
                y = self.render_expanded_panel(frame, *lang, panel_area);
            }
        }
    }

    fn handle_input(&mut self, key: KeyEvent, config: &mut ProjectConfig) -> StepResult {
        match self.focus {
            Focus::Choice => self.handle_choice_input(key.code),
            Focus::SubField(_) => self.handle_subfield_input(key, config),
            Focus::Browsing => StepResult::Continue,
        }
    }

    fn planned_actions(&self, config: &ProjectConfig) -> Vec<String> {
        config
            .language_configs
            .iter()
            .map(|lc| {
                if lc.tools.is_empty() {
                    format!("Configure {}", lc.language)
                } else {
                    format!("Configure {} ({})", lc.language, lc.tools.join(", "))
                }
            })
            .collect()
    }

    fn execute(&self, _config: &ProjectConfig) -> std::io::Result<()> {
        Ok(())
    }

    fn in_details(&self) -> bool {
        matches!(self.focus, Focus::SubField(_))
    }

    fn is_expanded(&self) -> bool {
        self.expanded.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    // --- Default state ---

    #[test]
    fn default_state() {
        let h = LanguagesHandler::default();
        assert_eq!(h.cursor, 0);
        assert!(h.selected.is_empty());
        assert!(h.expanded.is_none());
        assert_eq!(h.focus, Focus::Choice);
        assert!(!h.in_details());
        assert!(!h.is_expanded());
    }

    // --- Choice-level navigation and toggling ---

    #[test]
    fn cursor_down_and_up_at_choice_level() {
        let mut h = LanguagesHandler::default();
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
        let mut h = LanguagesHandler::default();
        let mut c = ProjectConfig::default();
        for _ in 0..100 {
            h.handle_input(key(KeyCode::Down), &mut c);
        }
        assert_eq!(h.cursor, Language::VARIANTS.len() - 1);
        for _ in 0..100 {
            h.handle_input(key(KeyCode::Up), &mut c);
        }
        assert_eq!(h.cursor, 0);
    }

    #[test]
    fn enter_toggles_checked_state() {
        let mut h = LanguagesHandler::default();
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Enter), &mut c);
        assert_eq!(h.selected, vec![Language::VARIANTS[0]]);
        h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(h.selected.is_empty());
    }

    #[test]
    fn space_also_toggles() {
        let mut h = LanguagesHandler::default();
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert_eq!(h.selected, vec![Language::VARIANTS[0]]);
    }

    #[test]
    fn q_quits() {
        let mut h = LanguagesHandler::default();
        let mut c = ProjectConfig::default();
        let result = h.handle_input(key(KeyCode::Char('q')), &mut c);
        assert!(matches!(result, StepResult::Quit));
    }

    #[test]
    fn left_backs_when_nothing_expanded() {
        let mut h = LanguagesHandler::default();
        let mut c = ProjectConfig::default();
        let result = h.handle_input(key(KeyCode::Left), &mut c);
        assert!(matches!(result, StepResult::Back));
    }

    // --- Expansion semantics ---

    #[test]
    fn right_on_unchecked_language_does_not_expand() {
        let mut h = LanguagesHandler::default();
        let mut c = ProjectConfig::default();
        let result = h.handle_input(key(KeyCode::Right), &mut c);
        assert!(matches!(result, StepResult::Continue));
        assert!(h.expanded.is_none());
        assert_eq!(h.focus, Focus::Choice);
    }

    #[test]
    fn right_on_checked_language_expands() {
        let mut h = with_selected(Language::Python);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        assert_eq!(h.expanded, Some(Language::Python));
        assert_eq!(h.focus, Focus::SubField(0));
        assert!(h.in_details());
        assert!(h.is_expanded());
    }

    #[test]
    fn left_collapses_expanded_then_backs() {
        let mut h = with_selected(Language::Python);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        assert!(h.expanded.is_some());
        // Left from SubField(0) doesn't collapse; user has to Esc first.
        h.focus = Focus::Choice;
        let result = h.handle_input(key(KeyCode::Left), &mut c);
        assert!(h.expanded.is_none());
        assert!(matches!(result, StepResult::Continue));
        let result = h.handle_input(key(KeyCode::Left), &mut c);
        assert!(matches!(result, StepResult::Back));
    }

    #[test]
    fn esc_from_subfield_returns_to_choice() {
        let mut h = with_selected(Language::Python);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        assert_eq!(h.focus, Focus::SubField(0));
        h.handle_input(key(KeyCode::Esc), &mut c);
        assert_eq!(h.focus, Focus::Choice);
        assert_eq!(h.expanded, Some(Language::Python));
    }

    // --- 2D cursor navigation within expanded ---

    #[test]
    fn down_moves_through_nav_rows() {
        let mut h = with_selected(Language::Python);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        assert_eq!(h.row_cursor, 0);
        h.handle_input(key(KeyCode::Down), &mut c);
        assert_eq!(h.row_cursor, 1);
        h.handle_input(key(KeyCode::Char('j')), &mut c);
        assert_eq!(h.row_cursor, 2);
    }

    #[test]
    fn up_from_row_0_returns_to_choice() {
        let mut h = with_selected(Language::Python);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        h.handle_input(key(KeyCode::Up), &mut c);
        assert_eq!(h.focus, Focus::Choice);
        assert_eq!(h.expanded, Some(Language::Python));
    }

    #[test]
    fn horizontal_nav_within_tool_row() {
        let mut h = with_selected(Language::Python);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        assert_eq!(h.col_cursor, 0);
        h.handle_input(key(KeyCode::Right), &mut c);
        assert_eq!(h.col_cursor, 1);
        h.handle_input(key(KeyCode::Char('l')), &mut c);
        assert_eq!(h.col_cursor, 2);
        h.handle_input(key(KeyCode::Left), &mut c);
        assert_eq!(h.col_cursor, 1);
        h.handle_input(key(KeyCode::Char('h')), &mut c);
        assert_eq!(h.col_cursor, 0);
        h.handle_input(key(KeyCode::Left), &mut c);
        assert_eq!(h.col_cursor, 0); // clamps at 0
    }

    // --- Toggle tool / common dep ---

    #[test]
    fn space_toggles_tool_checkbox() {
        let mut h = with_selected(Language::Python);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        let s = h.scratch_for(Language::Python).unwrap();
        assert_eq!(s.tools.len(), 1);
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        let s = h.scratch_for(Language::Python).unwrap();
        assert!(s.tools.is_empty());
    }

    #[test]
    fn toggle_common_dep() {
        let mut h = with_selected(Language::Python);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        let spec = spec_for(Language::Python);
        let rows = nav_rows(spec);
        let first_common_row = rows
            .iter()
            .position(|r| matches!(r, NavRow::CommonDeps { .. }))
            .expect("Python has common deps");
        h.row_cursor = first_common_row;
        h.col_cursor = 0;
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        let s = h.scratch_for(Language::Python).unwrap();
        assert_eq!(s.common_deps.len(), 1);
    }

    // --- Custom deps text input ---

    #[test]
    fn custom_deps_input_captures_characters() {
        let mut h = with_selected(Language::Python);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        let spec = spec_for(Language::Python);
        let rows = nav_rows(spec);
        let text_row = rows
            .iter()
            .position(|r| matches!(r, NavRow::CustomDepsInput))
            .expect("custom deps row exists");
        h.row_cursor = text_row;
        h.col_cursor = 0;
        for ch in "my-lib,other".chars() {
            h.handle_input(key(KeyCode::Char(ch)), &mut c);
        }
        assert_eq!(h.custom_deps_input.value, "my-lib,other");
        assert_eq!(
            h.scratch_for(Language::Python).unwrap().custom_deps,
            "my-lib,other"
        );
    }

    // --- Commit semantics ---

    #[test]
    fn enter_on_subfield_commits_and_advances() {
        let mut h = with_selected(Language::Python);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Done));
        assert_eq!(c.language_configs.len(), 1);
        assert_eq!(c.language_configs[0].language, Language::Python);
        assert_eq!(c.language_configs[0].tools.len(), 1);
    }

    #[test]
    fn commit_drops_unchecked_languages() {
        let mut h = LanguagesHandler {
            selected: vec![Language::Python],
            scratch: vec![LanguageConfig {
                language: Language::Rust,
                tools: vec!["clippy"],
                common_deps: vec![],
                custom_deps: String::new(),
            }],
            ..Default::default()
        };
        let mut c = ProjectConfig::default();
        h.commit_to_config(&mut c);
        assert_eq!(c.language_configs.len(), 1);
        assert_eq!(c.language_configs[0].language, Language::Python);
    }

    #[test]
    fn uncheck_then_recheck_preserves_scratch() {
        let mut h = with_selected(Language::Python);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        h.handle_input(key(KeyCode::Esc), &mut c);
        h.handle_input(key(KeyCode::Left), &mut c);
        h.handle_input(key(KeyCode::Enter), &mut c); // uncheck
        assert!(!h.is_selected(Language::Python));
        assert_eq!(h.scratch_for(Language::Python).unwrap().tools.len(), 1);
        h.handle_input(key(KeyCode::Enter), &mut c); // re-check
        assert!(h.is_selected(Language::Python));
        assert_eq!(h.scratch_for(Language::Python).unwrap().tools.len(), 1);
    }

    #[test]
    fn restore_from_config_rehydrates_selected_and_scratch() {
        let mut h = LanguagesHandler::default();
        let c = ProjectConfig {
            language_configs: vec![LanguageConfig {
                language: Language::Python,
                tools: vec!["ruff"],
                common_deps: vec!["fastapi"],
                custom_deps: "my-lib".to_string(),
            }],
            ..Default::default()
        };
        h.restore_from_config(&c);
        assert_eq!(h.selected, vec![Language::Python]);
        assert_eq!(h.scratch_for(Language::Python).unwrap().tools, vec!["ruff"]);
        assert_eq!(
            h.scratch_for(Language::Python).unwrap().common_deps,
            vec!["fastapi"]
        );
        assert_eq!(
            h.scratch_for(Language::Python).unwrap().custom_deps,
            "my-lib"
        );
    }

    // --- Empty-spec languages ---

    #[test]
    fn empty_spec_language_expands_but_has_no_nav_rows() {
        let mut h = with_selected(Language::Go);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        assert_eq!(h.expanded, Some(Language::Go));
        assert_eq!(h.focus, Focus::SubField(0));
        let rows = h.current_nav_rows();
        assert!(rows.is_empty());
    }

    #[test]
    fn empty_spec_esc_returns_to_choice() {
        let mut h = with_selected(Language::Go);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        h.handle_input(key(KeyCode::Esc), &mut c);
        assert_eq!(h.focus, Focus::Choice);
    }

    // --- Tab cycling flattens 2D cursor ---

    #[test]
    fn tab_advances_linearly_through_columns_then_rows() {
        let mut h = with_selected(Language::Python);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        h.handle_input(key(KeyCode::Tab), &mut c);
        assert_eq!((h.row_cursor, h.col_cursor), (0, 1));
        h.handle_input(key(KeyCode::Tab), &mut c);
        assert_eq!((h.row_cursor, h.col_cursor), (0, 2));
        h.handle_input(key(KeyCode::Tab), &mut c);
        assert_eq!((h.row_cursor, h.col_cursor), (1, 0));
    }

    #[test]
    fn backtab_moves_linearly_backward() {
        let mut h = with_selected(Language::Python);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        h.handle_input(key(KeyCode::BackTab), &mut c);
        let spec = spec_for(Language::Python);
        let rows = nav_rows(spec);
        let last_row = rows.len() - 1;
        assert_eq!((h.row_cursor, h.col_cursor), (last_row, 0));
    }

    // --- planned_actions / execute ---

    #[test]
    fn planned_actions_empty_without_languages() {
        let h = LanguagesHandler::default();
        let c = ProjectConfig::default();
        assert!(h.planned_actions(&c).is_empty());
    }

    #[test]
    fn planned_actions_include_tool_list() {
        let h = LanguagesHandler::default();
        let c = ProjectConfig {
            language_configs: vec![
                LanguageConfig {
                    language: Language::Python,
                    tools: vec!["ruff", "pytest"],
                    common_deps: vec![],
                    custom_deps: String::new(),
                },
                LanguageConfig::new(Language::Rust),
            ],
            ..Default::default()
        };
        let actions = h.planned_actions(&c);
        assert_eq!(actions.len(), 2);
        assert!(actions[0].contains("Python"));
        assert!(actions[0].contains("ruff"));
        assert!(actions[0].contains("pytest"));
        assert!(actions[1].contains("Rust"));
    }

    #[test]
    fn execute_is_ok() {
        let h = LanguagesHandler::default();
        let c = ProjectConfig::default();
        assert!(h.execute(&c).is_ok());
    }

    // --- helpers ---

    fn with_selected(lang: Language) -> LanguagesHandler {
        let idx = Language::VARIANTS.iter().position(|l| *l == lang).unwrap();
        LanguagesHandler {
            cursor: idx,
            selected: vec![lang],
            ..Default::default()
        }
    }
}
