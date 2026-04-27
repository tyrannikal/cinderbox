use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use strum::VariantArray;

use crate::registry::{CommonDep, LanguageSpec, Tool, ToolCategory, spec_for};
use crate::widgets::text_input::TextInput;
use crate::{Language, LanguageConfig, ProjectConfig};

use super::{CURSOR_BLANK, CURSOR_MARKER, Focus, StepHandler, StepResult};

const DEPS_PER_ROW: usize = 3;
const INDENT_SUBPANEL: u16 = 4;
const INDENT_ROW: u16 = 6;
const CONFIRM_BUTTON_WIDTH: u16 = 12;
const CONFIRM_BUTTON_HEIGHT: u16 = 3;

/// A language is "supported" when its registry entry has any tool categories
/// or common dependencies. Unsupported languages render grayed-out and
/// cannot be toggled or expanded.
fn is_supported(lang: Language) -> bool {
    let spec = spec_for(lang);
    !spec.categories.is_empty() || !spec.common_deps.is_empty()
}

/// One interactive row inside an expanded language's sub-panel.
/// Drives the handler's 2D cursor (row_cursor × col_cursor) and the render walk.
#[derive(Debug, Clone, Copy, PartialEq)]
enum NavRow {
    /// The N checkboxes for a single tool category.
    CategoryTools { cat_idx: usize },
    /// A (possibly wrapped) slice of common deps — `[start, end)` into `spec.common_deps`.
    CommonDeps { start: usize, end: usize },
    /// The free-text custom-deps input.
    CustomDepsInput,
    /// Bordered "Confirm" button rendered at the bottom of the sub-panel.
    /// Pressing Enter or Space on it collapses the panel (keeping the language checked).
    Confirm,
}

impl NavRow {
    fn col_count(&self, spec: &LanguageSpec) -> usize {
        match self {
            NavRow::CategoryTools { cat_idx } => spec.categories[*cat_idx].tools.len(),
            NavRow::CommonDeps { start, end } => end - start,
            NavRow::CustomDepsInput => 1,
            NavRow::Confirm => 1,
        }
    }
}

/// Build the interactive rows for a language's spec. Always ends with a
/// `NavRow::Confirm` so users have an explicit affordance to close the
/// sub-panel — even for empty-spec languages, where Confirm is the only row.
fn nav_rows(spec: &LanguageSpec) -> Vec<NavRow> {
    let mut rows: Vec<NavRow> = Vec::new();
    if !spec.categories.is_empty() || !spec.common_deps.is_empty() {
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
    }
    rows.push(NavRow::Confirm);
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
            custom_deps_input: TextInput::new("Custom dependencies (comma-separated)"),
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
        self.custom_deps_input = TextInput::new("Custom dependencies (comma-separated)");
    }

    fn is_selected(&self, lang: Language) -> bool {
        self.selected.contains(&lang)
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
        self.custom_deps_input = TextInput::new("Custom dependencies (comma-separated)");
        self.custom_deps_input.set_value(starting_value);
    }

    fn collapse_persist(&mut self) {
        if let Some(lang) = self.expanded {
            let value = self.custom_deps_input.value().to_string();
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
            let value = self.custom_deps_input.value().to_string();
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
            NavRow::CustomDepsInput | NavRow::Confirm => {}
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

    /// Total choice-row count: the "Next" pseudo-row plus one row per language.
    fn choice_count(&self) -> usize {
        Language::VARIANTS.len() + 1
    }

    /// Returns the language for the current cursor, or `None` if the cursor is
    /// on the "Next" pseudo-row at index 0.
    fn cursor_lang(&self) -> Option<Language> {
        if self.cursor == 0 {
            None
        } else {
            Some(Language::VARIANTS[self.cursor - 1])
        }
    }

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
                // Space deselects an already-checked language without expanding it.
                // No-op on the Next pseudo-row, unsupported languages, or unchecked
                // languages — checking + expanding is Enter's job.
                if let Some(lang) = self.cursor_lang()
                    && is_supported(lang)
                    && self.is_selected(lang)
                {
                    self.toggle_lang(lang);
                }
                StepResult::Continue
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => match self.cursor_lang() {
                None => {
                    // "Next" — commit current selections (possibly empty) and advance.
                    self.commit_to_config(config);
                    StepResult::Done
                }
                Some(lang) if is_supported(lang) => {
                    if !self.is_selected(lang) {
                        self.selected.push(lang);
                    }
                    self.expand(lang);
                    StepResult::Continue
                }
                Some(_) => StepResult::Continue, // unsupported: no-op
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

    /// Toggle a specific language's selection. Used by Space at Choice focus.
    fn toggle_lang(&mut self, lang: Language) {
        if let Some(pos) = self.selected.iter().position(|l| *l == lang) {
            self.selected.remove(pos);
        } else {
            self.selected.push(lang);
        }
    }

    fn handle_subfield_input(&mut self, key: KeyEvent, _config: &mut ProjectConfig) -> StepResult {
        let Some(lang) = self.expanded else {
            return StepResult::Continue;
        };
        let spec = spec_for(lang);
        let rows = nav_rows(spec);

        // nav_rows always returns at least Confirm, but guard regardless.
        if rows.is_empty() {
            return StepResult::Continue;
        }

        let row = rows[self.row_cursor];
        let on_text = matches!(row, NavRow::CustomDepsInput);
        let on_confirm = matches!(row, NavRow::Confirm);

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
            _ => {}
        }

        // Confirm-button row: Enter and Space both collapse; horizontal nav is a no-op.
        if on_confirm {
            match key.code {
                KeyCode::Char('q') => return StepResult::Quit,
                KeyCode::Char(' ') | KeyCode::Enter => {
                    self.collapse_persist();
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

        // Row-kind-specific handling.
        if on_text {
            // Enter on the text input row collapses the sub-panel — Space here
            // inserts a literal space (via the Char(c) arm), so Enter is the
            // only "confirm without leaving the input" key.
            if matches!(key.code, KeyCode::Enter) {
                self.collapse_persist();
                return StepResult::Continue;
            }
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
            let value = self.custom_deps_input.value().to_string();
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
            KeyCode::Char(' ') | KeyCode::Enter => {
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
            let value = self.custom_deps_input.value().to_string();
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

    fn render_next_line(&self, frame: &mut Frame, area: Rect) {
        let highlighted = matches!(self.focus, Focus::Choice) && self.cursor == 0;
        let cursor_marker = if highlighted { CURSOR_MARKER } else { CURSOR_BLANK };
        let style = Style::default().add_modifier(Modifier::BOLD);
        let text = format!("{cursor_marker}Next →");
        frame.render_widget(Paragraph::new(Line::from(text).style(style)), area);
    }

    fn render_choice_line(&self, frame: &mut Frame, area: Rect, lang: Language, idx: usize) {
        let cursor_marker = if matches!(self.focus, Focus::Choice) && idx == self.cursor {
            CURSOR_MARKER
        } else {
            CURSOR_BLANK
        };
        let supported = is_supported(lang);
        let check = if self.is_selected(lang) { "[x]" } else { "[ ]" };
        let text = format!("{cursor_marker}{check} {lang}");
        let line = if supported {
            Line::from(text)
        } else {
            Line::from(text).style(Style::default().fg(Color::DarkGray))
        };
        frame.render_widget(Paragraph::new(line), area);
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

    fn render_expanded_panel(&self, frame: &mut Frame, lang: Language, area: Rect) -> u16 {
        let spec = spec_for(lang);
        let _rows = nav_rows(spec);
        let mut y = area.y;
        let bottom = area.y + area.height;
        let has_tools = !spec.categories.is_empty() || !spec.common_deps.is_empty();

        if !has_tools {
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
            // Even empty-spec languages get a Confirm button (the only nav row).
            y = self.render_confirm_at(frame, area, y, bottom, 0);
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
                Paragraph::new(Line::from("Common dependencies:").style(Style::default().add_modifier(Modifier::BOLD))),
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

        // Custom dependencies label + TextInput
        if y >= bottom { return y; }
        let label_rect = Rect {
            x: area.x + INDENT_SUBPANEL,
            y,
            width: area.width.saturating_sub(INDENT_SUBPANEL),
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(Line::from("Custom dependencies:").style(Style::default().add_modifier(Modifier::BOLD))),
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
        nav_row_idx += 1;

        // Confirm button at the bottom of the sub-panel.
        y = self.render_confirm_at(frame, area, y, bottom, nav_row_idx);
        y
    }

    fn render_confirm_at(
        &self,
        frame: &mut Frame,
        area: Rect,
        mut y: u16,
        bottom: u16,
        confirm_row_idx: usize,
    ) -> u16 {
        if y + CONFIRM_BUTTON_HEIGHT > bottom {
            return y;
        }
        let button_rect = Rect {
            x: area.x + INDENT_ROW,
            y,
            width: CONFIRM_BUTTON_WIDTH.min(area.width.saturating_sub(INDENT_ROW)),
            height: CONFIRM_BUTTON_HEIGHT,
        };
        let focused =
            matches!(self.focus, Focus::SubField(_)) && self.row_cursor == confirm_row_idx;
        self.render_confirm_button(frame, button_rect, focused);
        y += CONFIRM_BUTTON_HEIGHT;
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

        // "Next" pseudo-row at index 0 — commits selections (possibly empty) on activation.
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
            // Cursor index 0 is "Next"; languages start at 1.
            self.render_choice_line(frame, choice_rect, *lang, i + 1);
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
            Focus::Choice => self.handle_choice_input(key.code, config),
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
        // Cursor count is 1 ("Next") + Language::VARIANTS.len(), so the max index is `len`.
        assert_eq!(h.cursor, Language::VARIANTS.len());
        for _ in 0..100 {
            h.handle_input(key(KeyCode::Up), &mut c);
        }
        assert_eq!(h.cursor, 0);
    }

    #[test]
    fn enter_on_next_commits_and_advances() {
        // Cursor 0 is the "Next" pseudo-row; pressing Enter there commits the
        // current (possibly empty) selection and advances the wizard.
        let mut h = LanguagesHandler::default();
        let mut c = ProjectConfig::default();
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Done));
        assert!(c.language_configs.is_empty());
    }

    #[test]
    fn right_on_next_commits_and_advances() {
        let mut h = LanguagesHandler::default();
        let mut c = ProjectConfig::default();
        let result = h.handle_input(key(KeyCode::Right), &mut c);
        assert!(matches!(result, StepResult::Done));
    }

    #[test]
    fn enter_on_supported_lang_checks_and_expands() {
        // Cursor on Rust (index 1 in choice rows): Enter checks the language
        // (if not already) AND expands its sub-menu.
        let mut h = with_cursor_on(Language::Rust);
        let mut c = ProjectConfig::default();
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Continue));
        assert!(h.is_selected(Language::Rust));
        assert_eq!(h.expanded, Some(Language::Rust));
        assert_eq!(h.focus, Focus::SubField(0));
    }

    #[test]
    fn enter_on_unsupported_lang_is_noop() {
        let mut h = with_cursor_on(Language::Go);
        let mut c = ProjectConfig::default();
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Continue));
        assert!(!h.is_selected(Language::Go));
        assert!(h.expanded.is_none());
    }

    #[test]
    fn space_deselects_checked_lang() {
        // Space at choice level deselects only — no-op on unchecked languages,
        // unchecks already-selected languages without expanding them.
        let mut h = with_selected(Language::Rust);
        let mut c = ProjectConfig::default();
        // Cursor is on Rust (with_selected places it there).
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert!(h.selected.is_empty());
        assert!(h.expanded.is_none());
        // Pressing Space again on the now-unchecked language is a no-op
        // (Space is deselect-only — it does not check).
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert!(h.selected.is_empty());
        assert!(h.expanded.is_none());
    }

    #[test]
    fn space_on_unchecked_lang_is_noop() {
        // Space on an unchecked supported language does NOT check it — that's
        // Enter's job. Space only deselects.
        let mut h = with_cursor_on(Language::Rust);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert!(h.selected.is_empty());
        assert!(h.expanded.is_none());
    }

    #[test]
    fn space_on_unsupported_lang_is_noop() {
        let mut h = with_cursor_on(Language::Go);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert!(h.selected.is_empty());
    }

    #[test]
    fn space_on_next_is_noop() {
        let mut h = LanguagesHandler::default();
        let mut c = ProjectConfig::default();
        let result = h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert!(matches!(result, StepResult::Continue));
        assert!(h.selected.is_empty());
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
    fn right_on_unsupported_language_is_noop() {
        // Unsupported languages (no registry spec) are unselectable; Right does nothing.
        let mut h = with_cursor_on(Language::Go);
        let mut c = ProjectConfig::default();
        let result = h.handle_input(key(KeyCode::Right), &mut c);
        assert!(matches!(result, StepResult::Continue));
        assert!(h.expanded.is_none());
        assert_eq!(h.focus, Focus::Choice);
    }

    #[test]
    fn right_on_unchecked_supported_language_expands_and_checks() {
        // QA #7: expanding via right arrow / L should work without first toggling
        // the language on. Right both checks and expands in a single step.
        let mut h = with_cursor_on(Language::Python);
        let mut c = ProjectConfig::default();
        let result = h.handle_input(key(KeyCode::Right), &mut c);
        assert!(matches!(result, StepResult::Continue));
        assert!(h.is_selected(Language::Python));
        assert_eq!(h.expanded, Some(Language::Python));
        assert_eq!(h.focus, Focus::SubField(0));
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
        assert_eq!(h.custom_deps_input.value(), "my-lib,other");
        assert_eq!(
            h.scratch_for(Language::Python).unwrap().custom_deps,
            "my-lib,other"
        );
    }

    // --- Commit semantics ---

    #[test]
    fn enter_on_subfield_checkbox_toggles() {
        // Enter at sub-level acts like Space on each row: on a checkbox row it
        // toggles the focused checkbox (does NOT collapse).
        let mut h = with_selected(Language::Python);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c); // expand, focus on first tool row
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Continue));
        // Focus stays in sub-panel; the checkbox is toggled on.
        assert_eq!(h.focus, Focus::SubField(0));
        assert_eq!(h.expanded, Some(Language::Python));
        assert_eq!(h.scratch_for(Language::Python).unwrap().tools.len(), 1);
        // Pressing Enter again toggles it back off.
        h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(h.scratch_for(Language::Python).unwrap().tools.is_empty());
    }

    #[test]
    fn enter_on_text_input_row_collapses() {
        // Option 3: Enter on the custom-deps text input row collapses, since
        // Space there inserts a literal space character. This preserves a way
        // to confirm without first navigating away from a half-typed dep.
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
        for ch in "my-lib".chars() {
            h.handle_input(key(KeyCode::Char(ch)), &mut c);
        }
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Continue));
        assert_eq!(h.focus, Focus::Choice);
        assert!(h.expanded.is_none());
        // The half-typed dep is persisted into scratch.
        assert_eq!(
            h.scratch_for(Language::Python).unwrap().custom_deps,
            "my-lib"
        );
    }

    #[test]
    fn full_flow_configure_and_advance() {
        let mut h = with_cursor_on(Language::Python);
        let mut c = ProjectConfig::default();
        // Enter on Python: check + expand
        h.handle_input(key(KeyCode::Enter), &mut c);
        // Toggle a tool with Space
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        // Navigate to the Confirm row and press Enter to collapse.
        let rows = h.current_nav_rows();
        h.row_cursor = rows.len() - 1;
        h.handle_input(key(KeyCode::Enter), &mut c);
        assert_eq!(h.focus, Focus::Choice);
        // Navigate up to "Next"
        for _ in 0..20 {
            h.handle_input(key(KeyCode::Up), &mut c);
        }
        assert_eq!(h.cursor, 0);
        // Enter on Next: commit + advance
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Done));
        assert_eq!(c.language_configs.len(), 1);
        assert_eq!(c.language_configs[0].language, Language::Python);
        assert_eq!(c.language_configs[0].tools.len(), 1);
    }

    #[test]
    fn confirm_button_collapses_via_enter() {
        let mut h = with_selected(Language::Python);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c); // expand
        // Move to the Confirm row (last nav row)
        let rows = h.current_nav_rows();
        h.row_cursor = rows.len() - 1;
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Continue));
        assert_eq!(h.focus, Focus::Choice);
        assert!(h.expanded.is_none());
        assert!(h.is_selected(Language::Python));
    }

    #[test]
    fn confirm_button_collapses_via_space() {
        let mut h = with_selected(Language::Python);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        let rows = h.current_nav_rows();
        h.row_cursor = rows.len() - 1;
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert_eq!(h.focus, Focus::Choice);
        assert!(h.expanded.is_none());
    }

    #[test]
    fn nav_rows_always_end_with_confirm() {
        let python_rows = nav_rows(spec_for(Language::Python));
        assert!(matches!(python_rows.last(), Some(NavRow::Confirm)));
        let rust_rows = nav_rows(spec_for(Language::Rust));
        assert!(matches!(rust_rows.last(), Some(NavRow::Confirm)));
        let go_rows = nav_rows(spec_for(Language::Go));
        // Empty-spec language: only the Confirm row.
        assert_eq!(go_rows.len(), 1);
        assert!(matches!(go_rows[0], NavRow::Confirm));
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
        // Configure Python (toggle a tool), collapse, uncheck via Space,
        // re-check via Enter — scratch state should persist across the toggle.
        let mut h = with_selected(Language::Python);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c); // expand
        h.handle_input(key(KeyCode::Char(' ')), &mut c); // toggle ruff (sub-level)
        h.handle_input(key(KeyCode::Esc), &mut c); // back to choice
        h.handle_input(key(KeyCode::Left), &mut c); // collapse
        h.handle_input(key(KeyCode::Char(' ')), &mut c); // uncheck via Space
        assert!(!h.is_selected(Language::Python));
        assert_eq!(h.scratch_for(Language::Python).unwrap().tools.len(), 1);
        h.handle_input(key(KeyCode::Enter), &mut c); // re-check via Enter (also expands)
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

    // --- Empty-spec / unsupported languages ---
    //
    // After QA #5, unsupported languages are unselectable from the choice list,
    // so the Right/Enter expansion paths no longer reach them via the keyboard.
    // Direct setup tests still confirm the empty-spec rendering shape.

    #[test]
    fn empty_spec_has_only_confirm_row() {
        let rows = nav_rows(spec_for(Language::Go));
        assert_eq!(rows.len(), 1);
        assert!(matches!(rows[0], NavRow::Confirm));
    }

    // --- Tab cycling flattens 2D cursor ---

    #[test]
    fn tab_advances_linearly_through_columns_then_rows() {
        // Python's first row is Linters with 3 tools (ruff, black, pylint).
        // Cursor starts at (0, 0); Tab walks columns, then wraps to next row.
        let mut h = with_selected(Language::Python);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        let py_first_cat_cols = spec_for(Language::Python).categories[0].tools.len();
        let mut col = 0usize;
        for _ in 0..(py_first_cat_cols - 1) {
            h.handle_input(key(KeyCode::Tab), &mut c);
            col += 1;
            assert_eq!((h.row_cursor, h.col_cursor), (0, col));
        }
        // Next Tab wraps to row 1, col 0
        h.handle_input(key(KeyCode::Tab), &mut c);
        assert_eq!((h.row_cursor, h.col_cursor), (1, 0));
    }

    #[test]
    fn backtab_moves_linearly_backward() {
        // BackTab from (0, 0) wraps to the last position — which is the Confirm row.
        let mut h = with_selected(Language::Python);
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Right), &mut c);
        h.handle_input(key(KeyCode::BackTab), &mut c);
        let rows = nav_rows(spec_for(Language::Python));
        let last_row = rows.len() - 1;
        assert!(matches!(rows[last_row], NavRow::Confirm));
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
        // Cursor 0 is the "Next" pseudo-row; languages occupy 1..=12, so the
        // language at VARIANTS index `i` lives at cursor `i + 1`.
        let idx = Language::VARIANTS.iter().position(|l| *l == lang).unwrap();
        LanguagesHandler {
            cursor: idx + 1,
            selected: vec![lang],
            ..Default::default()
        }
    }

    fn with_cursor_on(lang: Language) -> LanguagesHandler {
        let idx = Language::VARIANTS.iter().position(|l| *l == lang).unwrap();
        LanguagesHandler {
            cursor: idx + 1,
            ..Default::default()
        }
    }
}
