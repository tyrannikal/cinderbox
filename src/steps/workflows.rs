use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::Paragraph,
};

use crate::registry::tool_by_id;
use crate::{CiProvider, PreCommitFramework, ProjectConfig, WorkflowConfig};

use super::{CURSOR_BLANK, CURSOR_MARKER, StepHandler, StepResult};

const CI_CHOICES: [CiProvider; 4] = [
    CiProvider::None,
    CiProvider::GitHubActions,
    CiProvider::GitLab,
    CiProvider::Woodpecker,
];
const PRE_COMMIT_CHOICES: [PreCommitFramework; 3] = [
    PreCommitFramework::PreCommit,
    PreCommitFramework::Lefthook,
    PreCommitFramework::None,
];
/// Total number of radio options across both sections.
const TOTAL_OPTIONS: usize = CI_CHOICES.len() + PRE_COMMIT_CHOICES.len();

#[derive(Debug)]
pub struct WorkflowsHandler {
    /// Linear cursor index 0..TOTAL_OPTIONS — 0..CI_CHOICES.len() points into
    /// the CI section, the remainder into the pre-commit section.
    cursor: usize,
    /// Selected radio index within `CI_CHOICES`.
    ci_idx: usize,
    /// Selected radio index within `PRE_COMMIT_CHOICES`.
    pre_commit_idx: usize,
}

impl Default for WorkflowsHandler {
    fn default() -> Self {
        Self {
            cursor: 0,
            // Default to GitHub Actions (index 1 in CI_CHOICES)
            ci_idx: 1,
            // Default to pre-commit (index 0 in PRE_COMMIT_CHOICES)
            pre_commit_idx: 0,
        }
    }
}

impl WorkflowsHandler {
    /// Restore state from existing config when navigating back.
    pub fn restore_from_config(&mut self, config: &ProjectConfig) {
        if let Some(ci) = config.workflows.ci
            && let Some(idx) = CI_CHOICES.iter().position(|c| *c == ci)
        {
            self.ci_idx = idx;
        }
        if let Some(pc) = config.workflows.pre_commit
            && let Some(idx) = PRE_COMMIT_CHOICES.iter().position(|p| *p == pc)
        {
            self.pre_commit_idx = idx;
        }
        self.cursor = 0;
    }

    fn commit_to_config(&self, config: &mut ProjectConfig) {
        config.workflows = WorkflowConfig {
            ci: Some(CI_CHOICES[self.ci_idx]),
            pre_commit: Some(PRE_COMMIT_CHOICES[self.pre_commit_idx]),
        };
    }

    /// Set the radio for the section the cursor is currently in.
    fn select_at_cursor(&mut self) {
        if self.cursor < CI_CHOICES.len() {
            self.ci_idx = self.cursor;
        } else {
            self.pre_commit_idx = self.cursor - CI_CHOICES.len();
        }
    }

    #[cfg(test)]
    fn ci_selection(&self) -> CiProvider {
        CI_CHOICES[self.ci_idx]
    }

    #[cfg(test)]
    fn pre_commit_selection(&self) -> PreCommitFramework {
        PRE_COMMIT_CHOICES[self.pre_commit_idx]
    }

    /// Render a single radio option line: cursor marker, radio glyph, then label.
    fn render_radio_option<T: std::fmt::Display>(
        &self,
        frame: &mut Frame,
        area: Rect,
        choice: &T,
        is_selected: bool,
        is_focused: bool,
    ) {
        let cursor_marker = if is_focused { CURSOR_MARKER } else { CURSOR_BLANK };
        let radio = if is_selected { "●" } else { "○" };
        let text = format!("{cursor_marker}{radio} {choice}");
        let mut style = if is_focused {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        if is_focused && is_selected {
            style = style.add_modifier(Modifier::BOLD);
        }
        frame.render_widget(Paragraph::new(Line::from(text).style(style)), area);
    }

    /// Build the "pre-commit runs: X, Y" / "CI runs: X, Y" lines from the
    /// current per-language tool picks. Tools not in the registry are skipped.
    fn derived_lines(config: &ProjectConfig) -> (Vec<&'static str>, Vec<&'static str>) {
        let mut pre_commit: Vec<&'static str> = Vec::new();
        let mut ci: Vec<&'static str> = Vec::new();
        for lc in &config.language_configs {
            for tool_id in &lc.tools {
                if let Some(tool) = tool_by_id(tool_id) {
                    if tool.default_pre_commit && !pre_commit.contains(&tool.label) {
                        pre_commit.push(tool.label);
                    }
                    if tool.default_ci && !ci.contains(&tool.label) {
                        ci.push(tool.label);
                    }
                }
            }
        }
        (pre_commit, ci)
    }
}

impl StepHandler for WorkflowsHandler {
    fn render(&self, frame: &mut Frame, area: Rect) {
        let mut y = area.y;
        let bottom = area.y + area.height;
        let label_style = Style::default().add_modifier(Modifier::BOLD);

        // CI provider section
        if y < bottom {
            frame.render_widget(
                Paragraph::new(Line::from("CI provider").style(label_style)),
                Rect { x: area.x, y, width: area.width, height: 1 },
            );
            y += 1;
        }
        for (i, choice) in CI_CHOICES.iter().enumerate() {
            if y >= bottom { return; }
            self.render_radio_option(
                frame,
                Rect { x: area.x + 2, y, width: area.width.saturating_sub(2), height: 1 },
                choice,
                i == self.ci_idx,
                self.cursor == i,
            );
            y += 1;
        }

        // Spacer between sections
        if y < bottom { y += 1; }

        // Pre-commit framework section
        if y < bottom {
            frame.render_widget(
                Paragraph::new(Line::from("Pre-commit framework").style(label_style)),
                Rect { x: area.x, y, width: area.width, height: 1 },
            );
            y += 1;
        }
        for (i, choice) in PRE_COMMIT_CHOICES.iter().enumerate() {
            if y >= bottom { return; }
            let global_idx = CI_CHOICES.len() + i;
            self.render_radio_option(
                frame,
                Rect { x: area.x + 2, y, width: area.width.saturating_sub(2), height: 1 },
                choice,
                i == self.pre_commit_idx,
                self.cursor == global_idx,
            );
            y += 1;
        }

        // Spacer + hint
        if y < bottom { y += 1; }
        if y < bottom {
            let hint_style = Style::default().fg(Color::DarkGray);
            frame.render_widget(
                Paragraph::new(
                    Line::from("(Derived tool placement shown on the Summary screen)")
                        .style(hint_style),
                ),
                Rect { x: area.x, y, width: area.width, height: 1 },
            );
        }
    }

    fn handle_input(&mut self, key: KeyEvent, config: &mut ProjectConfig) -> StepResult {
        match key.code {
            KeyCode::Char('q') => StepResult::Quit,
            KeyCode::Up | KeyCode::Char('k') => {
                self.cursor = self.cursor.saturating_sub(1);
                StepResult::Continue
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.cursor + 1 < TOTAL_OPTIONS {
                    self.cursor += 1;
                }
                StepResult::Continue
            }
            // Left/H: Back. Previously this was consumed by intra-row navigation,
            // making it impossible to leave the step backwards.
            KeyCode::Left | KeyCode::Char('h') => StepResult::Back,
            KeyCode::Char(' ') => {
                self.select_at_cursor();
                StepResult::Continue
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                // Commit + advance. If the user navigated to an option without
                // pressing Space, also adopt that option as the selection — the
                // common case is "Up/Down to find what I want, Enter to confirm".
                self.select_at_cursor();
                self.commit_to_config(config);
                StepResult::Done
            }
            _ => StepResult::Continue,
        }
    }

    fn planned_actions(&self, config: &ProjectConfig) -> Vec<String> {
        let mut actions = Vec::new();
        let ci = config.workflows.ci.unwrap_or(CiProvider::None);
        let pre_commit = config
            .workflows
            .pre_commit
            .unwrap_or(PreCommitFramework::None);
        if ci != CiProvider::None {
            actions.push(format!("Set up {} workflow", ci));
        }
        if pre_commit != PreCommitFramework::None {
            actions.push(format!("Set up {} hooks", pre_commit));
        }
        let (pre_tools, ci_tools) = Self::derived_lines(config);
        if pre_commit != PreCommitFramework::None && !pre_tools.is_empty() {
            actions.push(format!("Pre-commit runs: {}", pre_tools.join(", ")));
        }
        if ci != CiProvider::None && !ci_tools.is_empty() {
            actions.push(format!("CI runs: {}", ci_tools.join(", ")));
        }
        actions
    }

    fn execute(&self, _config: &ProjectConfig) -> std::io::Result<()> {
        // Workflow file generation is post-MVP.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Language, LanguageConfig};
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn default_state() {
        let h = WorkflowsHandler::default();
        assert_eq!(h.cursor, 0);
        assert_eq!(h.ci_selection(), CiProvider::GitHubActions);
        assert_eq!(h.pre_commit_selection(), PreCommitFramework::PreCommit);
    }

    #[test]
    fn down_walks_through_all_options_then_clamps() {
        let mut h = WorkflowsHandler::default();
        let mut c = ProjectConfig::default();
        for i in 1..TOTAL_OPTIONS {
            h.handle_input(key(KeyCode::Down), &mut c);
            assert_eq!(h.cursor, i);
        }
        // Clamps at TOTAL_OPTIONS - 1
        h.handle_input(key(KeyCode::Down), &mut c);
        assert_eq!(h.cursor, TOTAL_OPTIONS - 1);
    }

    #[test]
    fn up_returns_to_top() {
        let mut h = WorkflowsHandler {
            cursor: TOTAL_OPTIONS - 1,
            ..Default::default()
        };
        let mut c = ProjectConfig::default();
        for _ in 0..(TOTAL_OPTIONS + 5) {
            h.handle_input(key(KeyCode::Char('k')), &mut c);
        }
        assert_eq!(h.cursor, 0);
    }

    #[test]
    fn space_selects_radio_at_cursor_in_ci_section() {
        let mut h = WorkflowsHandler::default();
        let mut c = ProjectConfig::default();
        // Cursor 0 = CI::None
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert_eq!(h.ci_selection(), CiProvider::None);
        h.handle_input(key(KeyCode::Down), &mut c); // cursor 1 = GitHub Actions
        h.handle_input(key(KeyCode::Down), &mut c); // cursor 2 = GitLab
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert_eq!(h.ci_selection(), CiProvider::GitLab);
    }

    #[test]
    fn space_selects_radio_at_cursor_in_pre_commit_section() {
        let mut h = WorkflowsHandler::default();
        let mut c = ProjectConfig::default();
        // Move cursor into pre-commit section (cursor 4 = first pre-commit option)
        for _ in 0..CI_CHOICES.len() {
            h.handle_input(key(KeyCode::Down), &mut c);
        }
        assert_eq!(h.cursor, CI_CHOICES.len());
        h.handle_input(key(KeyCode::Down), &mut c); // cursor 5 = lefthook
        h.handle_input(key(KeyCode::Char(' ')), &mut c);
        assert_eq!(h.pre_commit_selection(), PreCommitFramework::Lefthook);
        // CI section selection unchanged
        assert_eq!(h.ci_selection(), CiProvider::GitHubActions);
    }

    #[test]
    fn left_returns_back() {
        let mut h = WorkflowsHandler::default();
        let mut c = ProjectConfig::default();
        let result = h.handle_input(key(KeyCode::Left), &mut c);
        assert!(matches!(result, StepResult::Back));
        let result = h.handle_input(key(KeyCode::Char('h')), &mut c);
        assert!(matches!(result, StepResult::Back));
    }

    #[test]
    fn enter_commits_and_advances() {
        let mut h = WorkflowsHandler::default();
        let mut c = ProjectConfig::default();
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Done));
        assert_eq!(c.workflows.ci, Some(CiProvider::None)); // cursor 0 = None
        assert_eq!(c.workflows.pre_commit, Some(PreCommitFramework::PreCommit));
    }

    #[test]
    fn right_commits_and_advances() {
        let mut h = WorkflowsHandler::default();
        let mut c = ProjectConfig::default();
        // Move to GitHub Actions (cursor 1) then advance via Right
        h.handle_input(key(KeyCode::Down), &mut c);
        let result = h.handle_input(key(KeyCode::Right), &mut c);
        assert!(matches!(result, StepResult::Done));
        assert_eq!(c.workflows.ci, Some(CiProvider::GitHubActions));
    }

    #[test]
    fn enter_adopts_cursor_position_as_selection() {
        // Even without explicit Space, pressing Enter at a cursor should set
        // the section's radio to that option before committing.
        let mut h = WorkflowsHandler::default();
        let mut c = ProjectConfig::default();
        // cursor 2 = GitLab in CI section
        h.handle_input(key(KeyCode::Down), &mut c);
        h.handle_input(key(KeyCode::Down), &mut c);
        h.handle_input(key(KeyCode::Enter), &mut c);
        assert_eq!(c.workflows.ci, Some(CiProvider::GitLab));
    }

    #[test]
    fn q_quits() {
        let mut h = WorkflowsHandler::default();
        let mut c = ProjectConfig::default();
        let result = h.handle_input(key(KeyCode::Char('q')), &mut c);
        assert!(matches!(result, StepResult::Quit));
    }

    #[test]
    fn restore_from_config_rehydrates() {
        let mut h = WorkflowsHandler::default();
        let c = ProjectConfig {
            workflows: WorkflowConfig {
                ci: Some(CiProvider::Woodpecker),
                pre_commit: Some(PreCommitFramework::Lefthook),
            },
            ..Default::default()
        };
        h.restore_from_config(&c);
        assert_eq!(h.ci_selection(), CiProvider::Woodpecker);
        assert_eq!(h.pre_commit_selection(), PreCommitFramework::Lefthook);
    }

    // --- Derived preview from language_configs ---

    #[test]
    fn derived_lines_reflect_language_tools() {
        let c = ProjectConfig {
            language_configs: vec![LanguageConfig {
                language: Language::Python,
                tools: vec!["ruff", "pytest"],
                common_deps: vec![],
                custom_deps: String::new(),
            }],
            ..Default::default()
        };
        let (pre, ci) = WorkflowsHandler::derived_lines(&c);
        assert!(pre.contains(&"Ruff")); // ruff is default_pre_commit: true
        assert!(!pre.contains(&"pytest")); // pytest default_pre_commit: false
        assert!(ci.contains(&"Ruff"));
        assert!(ci.contains(&"pytest"));
    }

    #[test]
    fn derived_lines_unknown_tool_ids_are_skipped() {
        let c = ProjectConfig {
            language_configs: vec![LanguageConfig {
                language: Language::Go,
                tools: vec!["made-up-tool"],
                common_deps: vec![],
                custom_deps: String::new(),
            }],
            ..Default::default()
        };
        let (pre, ci) = WorkflowsHandler::derived_lines(&c);
        assert!(pre.is_empty());
        assert!(ci.is_empty());
    }

    // --- planned_actions ---

    #[test]
    fn planned_actions_describes_selections() {
        let h = WorkflowsHandler::default();
        let c = ProjectConfig {
            workflows: WorkflowConfig {
                ci: Some(CiProvider::GitHubActions),
                pre_commit: Some(PreCommitFramework::PreCommit),
            },
            language_configs: vec![LanguageConfig {
                language: Language::Rust,
                tools: vec!["clippy"],
                common_deps: vec![],
                custom_deps: String::new(),
            }],
            ..Default::default()
        };
        let actions = h.planned_actions(&c);
        assert!(actions.iter().any(|a| a.contains("GitHub Actions")));
        assert!(actions.iter().any(|a| a.contains("pre-commit")));
        assert!(actions.iter().any(|a| a.contains("clippy")));
    }

    #[test]
    fn planned_actions_empty_when_all_none() {
        let h = WorkflowsHandler::default();
        let c = ProjectConfig {
            workflows: WorkflowConfig {
                ci: Some(CiProvider::None),
                pre_commit: Some(PreCommitFramework::None),
            },
            ..Default::default()
        };
        assert!(h.planned_actions(&c).is_empty());
    }

    #[test]
    fn execute_is_ok() {
        let h = WorkflowsHandler::default();
        let c = ProjectConfig::default();
        assert!(h.execute(&c).is_ok());
    }
}
