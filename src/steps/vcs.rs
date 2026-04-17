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

use super::{Focus, StepHandler, StepResult, render_choice_line};

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
                self.default_branch_input.set_value(&config.default_branch);
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
        let choice = self
            .expanded
            .expect("SubField focus implies expanded is Some");

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
        let highlighted = matches!(self.focus, Focus::Choice) && self.choice_cursor == 0;
        render_choice_line(frame, areas[idx], &Vcs::Git, highlighted);
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
        let highlighted = matches!(self.focus, Focus::Choice) && self.choice_cursor == 1;
        render_choice_line(frame, areas[idx], &Vcs::Jujutsu, highlighted);
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
        let highlighted = matches!(self.focus, Focus::Choice) && self.choice_cursor == 2;
        render_choice_line(frame, areas[idx], &Vcs::None, highlighted);
    }

    fn handle_input(&mut self, key: KeyEvent, config: &mut ProjectConfig) -> StepResult {
        let key = key.code;
        match self.focus {
            Focus::Choice => self.handle_choice(key, config),
            Focus::SubField(n) => self.handle_subfield(key, n, config),
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
        let h = VcsHandler::default();
        assert_eq!(h.focus, Focus::Choice);
        assert!(h.expanded.is_none());
        assert_eq!(h.choice_cursor, 0);
        assert_eq!(h.default_branch_input.value, "main");
        assert!(h.jj_colocate);
    }

    // --- Choice navigation ---

    #[test]
    fn choice_down_and_up() {
        let mut h = VcsHandler::default();
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Down), &mut c);
        assert_eq!(h.choice_cursor, 1);
        h.handle_input(key(KeyCode::Down), &mut c);
        assert_eq!(h.choice_cursor, 2);
        h.handle_input(key(KeyCode::Down), &mut c);
        assert_eq!(h.choice_cursor, 2); // clamped
        h.handle_input(key(KeyCode::Up), &mut c);
        assert_eq!(h.choice_cursor, 1);
    }

    #[test]
    fn choice_j_k_navigation() {
        let mut h = VcsHandler::default();
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Char('j')), &mut c);
        assert_eq!(h.choice_cursor, 1);
        h.handle_input(key(KeyCode::Char('k')), &mut c);
        assert_eq!(h.choice_cursor, 0);
    }

    // --- Selecting "None" commits immediately ---

    #[test]
    fn selecting_none_commits_and_returns_done() {
        let mut h = VcsHandler::default();
        let mut c = ProjectConfig::default();
        h.choice_cursor = 2; // None
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Done));
        assert_eq!(c.vcs, Some(Vcs::None));
        assert!(c.default_branch.is_empty());
        assert!(!c.jj_colocate);
    }

    // --- Expanding Git ---

    #[test]
    fn enter_on_git_expands_to_subfield() {
        let mut h = VcsHandler::default();
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Enter), &mut c);
        assert_eq!(h.expanded, Some(Vcs::Git));
        assert_eq!(h.focus, Focus::SubField(0));
    }

    #[test]
    fn git_enter_in_subfield_commits_config() {
        let mut h = VcsHandler::default();
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Enter), &mut c); // expand Git
        h.handle_input(key(KeyCode::Enter), &mut c); // confirm
        assert_eq!(c.vcs, Some(Vcs::Git));
        assert_eq!(c.default_branch, "main");
        assert!(!c.jj_colocate);
    }

    // --- Expanding Jujutsu ---

    #[test]
    fn jujutsu_has_two_subfields() {
        let mut h = VcsHandler::default();
        let mut c = ProjectConfig::default();
        h.choice_cursor = 1;
        h.handle_input(key(KeyCode::Enter), &mut c); // expand Jujutsu
        assert_eq!(h.expanded, Some(Vcs::Jujutsu));
        assert_eq!(h.max_subfield(), 1);
    }

    #[test]
    fn jujutsu_colocate_toggle() {
        let mut h = VcsHandler::default();
        let mut c = ProjectConfig::default();
        h.choice_cursor = 1;
        h.handle_input(key(KeyCode::Enter), &mut c); // expand Jujutsu
        assert_eq!(h.focus, Focus::SubField(0)); // colocate row
        assert!(h.jj_colocate);
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert!(!h.jj_colocate);
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert!(h.jj_colocate);
    }

    #[test]
    fn jujutsu_colocate_left_right() {
        let mut h = VcsHandler::default();
        let mut c = ProjectConfig::default();
        h.choice_cursor = 1;
        h.handle_input(key(KeyCode::Enter), &mut c);
        h.handle_input(key(KeyCode::Right), &mut c); // colocate = false
        assert!(!h.jj_colocate);
        h.handle_input(key(KeyCode::Left), &mut c); // colocate = true
        assert!(h.jj_colocate);
    }

    #[test]
    fn jujutsu_commit_with_colocate() {
        let mut h = VcsHandler::default();
        let mut c = ProjectConfig::default();
        h.choice_cursor = 1;
        h.handle_input(key(KeyCode::Enter), &mut c); // expand
        h.handle_input(key(KeyCode::Enter), &mut c); // confirm
        assert_eq!(c.vcs, Some(Vcs::Jujutsu));
        assert!(c.jj_colocate);
        assert_eq!(c.default_branch, "main");
    }

    // --- Tab/BackTab cycling ---

    #[test]
    fn tab_cycles_subfields_jujutsu() {
        let mut h = VcsHandler::default();
        let mut c = ProjectConfig::default();
        h.choice_cursor = 1;
        h.handle_input(key(KeyCode::Enter), &mut c);
        assert_eq!(h.focus, Focus::SubField(0));
        h.handle_input(key(KeyCode::Tab), &mut c);
        assert_eq!(h.focus, Focus::SubField(1));
        h.handle_input(key(KeyCode::Tab), &mut c);
        assert_eq!(h.focus, Focus::SubField(0)); // wraps
    }

    #[test]
    fn backtab_cycles_subfields_jujutsu() {
        let mut h = VcsHandler::default();
        let mut c = ProjectConfig::default();
        h.choice_cursor = 1;
        h.handle_input(key(KeyCode::Enter), &mut c);
        assert_eq!(h.focus, Focus::SubField(0));
        h.handle_input(key(KeyCode::BackTab), &mut c);
        assert_eq!(h.focus, Focus::SubField(1)); // wraps back
    }

    // --- Esc returns to choice ---

    #[test]
    fn esc_from_subfield_returns_to_choice() {
        let mut h = VcsHandler::default();
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Enter), &mut c); // expand Git
        assert_eq!(h.focus, Focus::SubField(0));
        h.handle_input(key(KeyCode::Esc), &mut c);
        assert_eq!(h.focus, Focus::Choice);
    }

    // --- Left collapses, then backs ---

    #[test]
    fn left_collapses_then_backs() {
        let mut h = VcsHandler::default();
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Enter), &mut c); // expand
        h.focus = Focus::Choice;
        let result = h.handle_input(key(KeyCode::Left), &mut c);
        assert!(h.expanded.is_none()); // collapsed
        assert!(matches!(result, StepResult::Continue));
        let result = h.handle_input(key(KeyCode::Left), &mut c);
        assert!(matches!(result, StepResult::Back));
    }

    // --- Quit ---

    #[test]
    fn q_in_choice_quits() {
        let mut h = VcsHandler::default();
        let mut c = ProjectConfig::default();
        let result = h.handle_input(key(KeyCode::Char('q')), &mut c);
        assert!(matches!(result, StepResult::Quit));
    }

    // --- planned_actions ---

    #[test]
    fn planned_actions_git() {
        let h = VcsHandler::default();
        let c = ProjectConfig {
            vcs: Some(Vcs::Git),
            default_branch: "main".to_string(),
            ..Default::default()
        };
        let actions = h.planned_actions(&c);
        assert_eq!(actions.len(), 2);
        assert!(actions[0].contains("git"));
        assert!(actions[1].contains("main"));
    }

    #[test]
    fn planned_actions_jj_colocated() {
        let h = VcsHandler::default();
        let c = ProjectConfig {
            vcs: Some(Vcs::Jujutsu),
            jj_colocate: true,
            default_branch: "trunk".to_string(),
            ..Default::default()
        };
        let actions = h.planned_actions(&c);
        assert!(actions[0].contains("colocated"));
        assert!(actions[1].contains("trunk"));
    }

    #[test]
    fn planned_actions_jj_native() {
        let h = VcsHandler::default();
        let c = ProjectConfig {
            vcs: Some(Vcs::Jujutsu),
            jj_colocate: false,
            default_branch: "main".to_string(),
            ..Default::default()
        };
        let actions = h.planned_actions(&c);
        assert_eq!(actions.len(), 1);
        assert!(actions[0].contains("native"));
    }

    #[test]
    fn planned_actions_none() {
        let h = VcsHandler::default();
        let c = ProjectConfig {
            vcs: Some(Vcs::None),
            ..Default::default()
        };
        assert!(h.planned_actions(&c).is_empty());
    }

    #[test]
    fn planned_actions_unset() {
        let h = VcsHandler::default();
        let c = ProjectConfig::default();
        assert!(h.planned_actions(&c).is_empty());
    }

    // --- restore_from_config ---

    #[test]
    fn restore_from_config_git() {
        let mut h = VcsHandler::default();
        let c = ProjectConfig {
            vcs: Some(Vcs::Git),
            default_branch: "develop".to_string(),
            ..Default::default()
        };
        h.restore_from_config(&c);
        assert_eq!(h.choice_cursor, 0);
        assert_eq!(h.expanded, Some(Vcs::Git));
        assert_eq!(h.focus, Focus::SubField(0));
        assert_eq!(h.default_branch_input.value, "develop");
    }

    #[test]
    fn restore_from_config_none_does_not_expand() {
        let mut h = VcsHandler::default();
        let c = ProjectConfig {
            vcs: Some(Vcs::None),
            ..Default::default()
        };
        h.restore_from_config(&c);
        assert_eq!(h.choice_cursor, 2);
        assert!(h.expanded.is_none());
        assert_eq!(h.focus, Focus::Choice);
    }

    // --- execute is no-op ---

    #[test]
    fn execute_is_ok() {
        let h = VcsHandler::default();
        let c = ProjectConfig::default();
        assert!(h.execute(&c).is_ok());
    }
}
