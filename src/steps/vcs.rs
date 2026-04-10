use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::Line,
    widgets::Paragraph,
};

use crate::ProjectConfig;
use crate::Vcs;
use crate::widgets::text_input::TextInput;

use super::{StepHandler, StepResult};

#[derive(Debug, Default, Clone, Copy, PartialEq)]
enum Focus {
    #[default]
    Choice,
    SubField(usize),
}

const VCS_CHOICES: [Vcs; 3] = [Vcs::Git, Vcs::Jujutsu, Vcs::None];

#[derive(Debug)]
pub struct VcsHandler {
    focus: Focus,
    expanded: Option<Vcs>,
    choice_cursor: usize,
    default_branch_input: TextInput,
    jj_colocate: bool,
}

impl Default for VcsHandler {
    fn default() -> Self {
        Self {
            focus: Focus::Choice,
            expanded: None,
            choice_cursor: 0,
            default_branch_input: TextInput::new("Default branch").with_value("main"),
            jj_colocate: true,
        }
    }
}

impl VcsHandler {
    pub fn in_details(&self) -> bool {
        matches!(self.focus, Focus::SubField(_))
    }

    pub fn is_expanded(&self) -> bool {
        self.expanded.is_some()
    }

    /// Restore state from existing config when navigating back
    pub fn restore_from_config(&mut self, config: &ProjectConfig) {
        let Some(vcs) = config.vcs else { return };
        self.choice_cursor = VCS_CHOICES.iter().position(|v| *v == vcs).unwrap_or(0);

        match vcs {
            Vcs::None => {
                // None has no sub-fields; don't expand
                self.expanded = None;
                self.focus = Focus::Choice;
            }
            Vcs::Git | Vcs::Jujutsu => {
                self.expanded = Some(vcs);
                self.focus = Focus::SubField(0);
                self.default_branch_input =
                    TextInput::new("Default branch").with_value(&config.default_branch);
                self.jj_colocate = config.jj_colocate;
            }
        }
    }

    /// Maximum sub-field index for the currently expanded choice.
    /// Git: 0 (default branch only). Jujutsu: 1 (colocate + default branch).
    fn max_subfield(&self) -> usize {
        match self.expanded {
            Some(Vcs::Git) => 0,
            Some(Vcs::Jujutsu) => 1,
            _ => 0,
        }
    }

    /// Returns true if the current sub-field is a text input (as opposed to
    /// the colocate radio row). Only meaningful when `focus == SubField(_)`.
    fn current_subfield_is_text(&self, field: usize) -> bool {
        match self.expanded {
            Some(Vcs::Git) => true,           // field 0 = text input
            Some(Vcs::Jujutsu) => field == 1, // field 0 = radio, field 1 = text input
            _ => false,
        }
    }

    fn commit_to_config(&self, config: &mut ProjectConfig) {
        let vcs = self.expanded.unwrap_or(Vcs::None);
        config.vcs = Some(vcs);
        match vcs {
            Vcs::Git => {
                config.default_branch = self.default_branch_input.value.clone();
                config.jj_colocate = false;
            }
            Vcs::Jujutsu => {
                config.default_branch = self.default_branch_input.value.clone();
                config.jj_colocate = self.jj_colocate;
            }
            Vcs::None => {
                config.default_branch.clear();
                config.jj_colocate = false;
            }
        }
    }

    fn render_choice_line(&self, frame: &mut Frame, area: Rect, choice: Vcs, index: usize) {
        let is_highlighted = matches!(self.focus, Focus::Choice) && self.choice_cursor == index;
        let marker = if is_highlighted { "▸ " } else { "  " };
        let text = format!("{marker}{choice}");
        frame.render_widget(Paragraph::new(text), area);
    }

    fn render_colocate_row(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let yes_marker = if self.jj_colocate { "●" } else { "○" };
        let no_marker = if self.jj_colocate { "○" } else { "●" };
        let text = format!("Colocate with git:  {yes_marker} Yes   {no_marker} No");
        let color = if focused {
            Color::White
        } else {
            Color::DarkGray
        };
        let style = Style::default().fg(color);
        frame.render_widget(Paragraph::new(Line::from(text).style(style)), area);
    }

    fn handle_choice(&mut self, key: KeyCode, config: &mut ProjectConfig) -> StepResult {
        match key {
            KeyCode::Char('q') => StepResult::Quit,
            KeyCode::Down | KeyCode::Char('j') => {
                if self.choice_cursor + 1 < VCS_CHOICES.len() {
                    self.choice_cursor += 1;
                }
                StepResult::Continue
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.choice_cursor = self.choice_cursor.saturating_sub(1);
                StepResult::Continue
            }
            KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Right | KeyCode::Char('l') => {
                let choice = VCS_CHOICES[self.choice_cursor];
                if choice == Vcs::None {
                    // None has no sub-fields — commit immediately and advance
                    self.expanded = None;
                    config.vcs = Some(Vcs::None);
                    config.default_branch.clear();
                    config.jj_colocate = false;
                    return StepResult::Done;
                }
                self.expanded = Some(choice);
                self.focus = Focus::SubField(0);
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

    fn handle_subfield(
        &mut self,
        key: KeyCode,
        field: usize,
        config: &mut ProjectConfig,
    ) -> StepResult {
        let choice = self.expanded.unwrap(); // safe: SubField only reachable when expanded

        // Common navigation keys (apply to any sub-field)
        match key {
            KeyCode::Up => {
                if field == 0 {
                    self.focus = Focus::Choice;
                } else {
                    self.focus = Focus::SubField(field - 1);
                }
                return StepResult::Continue;
            }
            KeyCode::Down => {
                if field < self.max_subfield() {
                    self.focus = Focus::SubField(field + 1);
                } else {
                    // Past last sub-field — jump to next choice in the list
                    let next_idx = VCS_CHOICES.iter().position(|v| *v == choice).unwrap_or(0) + 1;
                    if next_idx < VCS_CHOICES.len() {
                        self.choice_cursor = next_idx;
                        self.focus = Focus::Choice;
                    }
                }
                return StepResult::Continue;
            }
            KeyCode::Tab => {
                if field < self.max_subfield() {
                    self.focus = Focus::SubField(field + 1);
                } else {
                    self.focus = Focus::SubField(0);
                }
                return StepResult::Continue;
            }
            KeyCode::BackTab => {
                if field > 0 {
                    self.focus = Focus::SubField(field - 1);
                } else {
                    self.focus = Focus::SubField(self.max_subfield());
                }
                return StepResult::Continue;
            }
            KeyCode::Enter => {
                self.commit_to_config(config);
                return StepResult::Done;
            }
            KeyCode::Esc => {
                self.focus = Focus::Choice;
                return StepResult::Continue;
            }
            _ => {}
        }

        // Field-specific keys
        if !self.current_subfield_is_text(field) {
            // Colocate radio row (Jujutsu, field 0)
            match key {
                KeyCode::Left | KeyCode::Char('h') => self.jj_colocate = true,
                KeyCode::Right | KeyCode::Char('l') => self.jj_colocate = false,
                KeyCode::Char(' ') => self.jj_colocate = !self.jj_colocate,
                _ => {}
            }
        } else {
            // Default branch text input
            self.default_branch_input.handle_input(key);
        }

        StepResult::Continue
    }
}

impl StepHandler for VcsHandler {
    fn render(&self, frame: &mut Frame, area: Rect) {
        // Build layout constraints dynamically based on expanded state
        let mut constraints: Vec<Constraint> = vec![];

        // "Git" label
        constraints.push(Constraint::Length(1));
        if self.expanded == Some(Vcs::Git) {
            constraints.push(Constraint::Length(3)); // default branch input
        }
        // "Jujutsu" label
        constraints.push(Constraint::Length(1));
        if self.expanded == Some(Vcs::Jujutsu) {
            constraints.push(Constraint::Length(1)); // colocate row
            constraints.push(Constraint::Length(3)); // default branch input
        }
        // "None" label
        constraints.push(Constraint::Length(1));
        // Spacer
        constraints.push(Constraint::Min(1));

        let areas: Vec<Rect> = Layout::vertical(constraints).split(area).to_vec();
        let mut idx = 0;

        // Git
        self.render_choice_line(frame, areas[idx], Vcs::Git, 0);
        idx += 1;
        if self.expanded == Some(Vcs::Git) {
            let indent = 4u16;
            let branch_area = Rect {
                x: areas[idx].x + indent,
                width: areas[idx].width.saturating_sub(indent),
                ..areas[idx]
            };
            self.default_branch_input.render(
                frame,
                branch_area,
                matches!(self.focus, Focus::SubField(0)),
            );
            idx += 1;
        }

        // Jujutsu
        self.render_choice_line(frame, areas[idx], Vcs::Jujutsu, 1);
        idx += 1;
        if self.expanded == Some(Vcs::Jujutsu) {
            let indent = 4u16;
            let colocate_area = Rect {
                x: areas[idx].x + indent,
                width: areas[idx].width.saturating_sub(indent),
                ..areas[idx]
            };
            self.render_colocate_row(
                frame,
                colocate_area,
                matches!(self.focus, Focus::SubField(0)),
            );
            idx += 1;

            let branch_area = Rect {
                x: areas[idx].x + indent,
                width: areas[idx].width.saturating_sub(indent),
                ..areas[idx]
            };
            self.default_branch_input.render(
                frame,
                branch_area,
                matches!(self.focus, Focus::SubField(1)),
            );
            idx += 1;
        }

        // None
        self.render_choice_line(frame, areas[idx], Vcs::None, 2);
    }

    fn handle_input(&mut self, key: KeyEvent, config: &mut ProjectConfig) -> StepResult {
        let key = key.code;
        match self.focus {
            Focus::Choice => self.handle_choice(key, config),
            Focus::SubField(n) => self.handle_subfield(key, n, config),
        }
    }

    fn planned_actions(&self, config: &ProjectConfig) -> Vec<String> {
        match config.vcs {
            Some(Vcs::Git) => {
                let mut actions = vec!["Initialize git repository".to_string()];
                if !config.default_branch.is_empty() {
                    actions.push(format!("Set default branch to '{}'", config.default_branch));
                }
                actions
            }
            Some(Vcs::Jujutsu) => {
                let mode = if config.jj_colocate {
                    "colocated with git"
                } else {
                    "native"
                };
                let mut actions = vec![format!("Initialize jj repository ({mode})")];
                if !config.default_branch.is_empty() && config.jj_colocate {
                    actions.push(format!("Set default branch to '{}'", config.default_branch));
                }
                actions
            }
            Some(Vcs::None) | None => vec![],
        }
    }

    fn execute(&self, _config: &ProjectConfig) -> std::io::Result<()> {
        // VCS initialization is post-MVP; the wizard only collects the choice.
        Ok(())
    }
}
