use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::registry::tool_by_id;
use crate::{CiProvider, PreCommitFramework, ProjectConfig, WorkflowConfig};

use super::{StepHandler, StepResult};

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

#[derive(Debug)]
pub struct WorkflowsHandler {
    row_cursor: usize,
    ci_cursor: usize,
    pre_commit_cursor: usize,
}

impl Default for WorkflowsHandler {
    fn default() -> Self {
        Self {
            row_cursor: 0,
            // Default to GitHub Actions (index 1 in CI_CHOICES)
            ci_cursor: 1,
            // Default to pre-commit (index 0 in PRE_COMMIT_CHOICES)
            pre_commit_cursor: 0,
        }
    }
}

impl WorkflowsHandler {
    /// Restore state from existing config when navigating back.
    pub fn restore_from_config(&mut self, config: &ProjectConfig) {
        if let Some(ci) = config.workflows.ci
            && let Some(idx) = CI_CHOICES.iter().position(|c| *c == ci)
        {
            self.ci_cursor = idx;
        }
        if let Some(pc) = config.workflows.pre_commit
            && let Some(idx) = PRE_COMMIT_CHOICES.iter().position(|p| *p == pc)
        {
            self.pre_commit_cursor = idx;
        }
        self.row_cursor = 0;
    }

    fn commit_to_config(&self, config: &mut ProjectConfig) {
        config.workflows = WorkflowConfig {
            ci: Some(CI_CHOICES[self.ci_cursor]),
            pre_commit: Some(PRE_COMMIT_CHOICES[self.pre_commit_cursor]),
        };
    }

    #[cfg(test)]
    fn ci_selection(&self) -> CiProvider {
        CI_CHOICES[self.ci_cursor]
    }

    #[cfg(test)]
    fn pre_commit_selection(&self) -> PreCommitFramework {
        PRE_COMMIT_CHOICES[self.pre_commit_cursor]
    }

    fn render_radio_row<T: PartialEq + std::fmt::Display + Copy>(
        &self,
        frame: &mut Frame,
        area: Rect,
        choices: &[T],
        selected_idx: usize,
        row_focused: bool,
    ) {
        let mut spans: Vec<Span> = Vec::new();
        for (i, choice) in choices.iter().enumerate() {
            let marker = if i == selected_idx { "●" } else { "○" };
            let label = format!("{marker} {choice}");
            let base = if row_focused {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let style = if row_focused && i == selected_idx {
                base.add_modifier(Modifier::BOLD)
            } else {
                base
            };
            spans.push(Span::from(label).style(style));
            if i + 1 < choices.len() {
                spans.push(Span::from("   "));
            }
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
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

        if y < bottom {
            let rect = Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            };
            frame.render_widget(
                Paragraph::new(Line::from("CI provider").style(label_style)),
                rect,
            );
            y += 1;
        }
        if y < bottom {
            let rect = Rect {
                x: area.x + 2,
                y,
                width: area.width.saturating_sub(2),
                height: 1,
            };
            self.render_radio_row(frame, rect, &CI_CHOICES, self.ci_cursor, self.row_cursor == 0);
            y += 2;
        }
        if y < bottom {
            let rect = Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            };
            frame.render_widget(
                Paragraph::new(Line::from("Pre-commit framework").style(label_style)),
                rect,
            );
            y += 1;
        }
        if y < bottom {
            let rect = Rect {
                x: area.x + 2,
                y,
                width: area.width.saturating_sub(2),
                height: 1,
            };
            self.render_radio_row(
                frame,
                rect,
                &PRE_COMMIT_CHOICES,
                self.pre_commit_cursor,
                self.row_cursor == 1,
            );
            y += 2;
        }

        // Empty config-panel still paints the Derived block for clarity (empty list),
        // but the caller will have hidden the lines when the selection is None.
        // We read config via `planned_actions` pathway; here we only show the derived
        // preview if the caller provides a config. Since `render` doesn't take config,
        // we paint a placeholder label and trust the instruction bar for real info.
        // Derived lines shown below come from cached state passed in the constructor —
        // for MVP we compute lazily from the config at the summary step instead.
        if y < bottom {
            let rect = Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            };
            let hint_style = Style::default().fg(Color::DarkGray);
            frame.render_widget(
                Paragraph::new(
                    Line::from("(Derived tool placement shown on the Summary screen)")
                        .style(hint_style),
                ),
                rect,
            );
        }
    }

    fn handle_input(&mut self, key: KeyEvent, config: &mut ProjectConfig) -> StepResult {
        match key.code {
            KeyCode::Char('q') => StepResult::Quit,
            KeyCode::Up | KeyCode::Char('k') => {
                self.row_cursor = self.row_cursor.saturating_sub(1);
                StepResult::Continue
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.row_cursor + 1 < 2 {
                    self.row_cursor += 1;
                }
                StepResult::Continue
            }
            KeyCode::Left | KeyCode::Char('h') => {
                if self.row_cursor == 0 {
                    self.ci_cursor = self.ci_cursor.saturating_sub(1);
                } else {
                    self.pre_commit_cursor = self.pre_commit_cursor.saturating_sub(1);
                }
                StepResult::Continue
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.row_cursor == 0 {
                    if self.ci_cursor + 1 < CI_CHOICES.len() {
                        self.ci_cursor += 1;
                    }
                } else if self.pre_commit_cursor + 1 < PRE_COMMIT_CHOICES.len() {
                    self.pre_commit_cursor += 1;
                }
                StepResult::Continue
            }
            KeyCode::Enter => {
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
        assert_eq!(h.row_cursor, 0);
        assert_eq!(h.ci_selection(), CiProvider::GitHubActions);
        assert_eq!(h.pre_commit_selection(), PreCommitFramework::PreCommit);
    }

    #[test]
    fn down_moves_to_pre_commit_row() {
        let mut h = WorkflowsHandler::default();
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Down), &mut c);
        assert_eq!(h.row_cursor, 1);
        h.handle_input(key(KeyCode::Char('j')), &mut c);
        assert_eq!(h.row_cursor, 1); // clamped at 1
    }

    #[test]
    fn up_returns_to_ci_row() {
        let mut h = WorkflowsHandler {
            row_cursor: 1,
            ..Default::default()
        };
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Up), &mut c);
        assert_eq!(h.row_cursor, 0);
        h.handle_input(key(KeyCode::Char('k')), &mut c);
        assert_eq!(h.row_cursor, 0);
    }

    #[test]
    fn left_right_cycles_ci_radio() {
        let mut h = WorkflowsHandler::default();
        let mut c = ProjectConfig::default();
        // Default ci_cursor = 1 (GitHub Actions)
        h.handle_input(key(KeyCode::Right), &mut c);
        assert_eq!(h.ci_selection(), CiProvider::GitLab);
        h.handle_input(key(KeyCode::Char('l')), &mut c);
        assert_eq!(h.ci_selection(), CiProvider::Woodpecker);
        h.handle_input(key(KeyCode::Right), &mut c);
        assert_eq!(h.ci_selection(), CiProvider::Woodpecker); // clamped
        h.handle_input(key(KeyCode::Left), &mut c);
        assert_eq!(h.ci_selection(), CiProvider::GitLab);
        h.handle_input(key(KeyCode::Char('h')), &mut c);
        assert_eq!(h.ci_selection(), CiProvider::GitHubActions);
    }

    #[test]
    fn left_right_cycles_pre_commit_radio() {
        let mut h = WorkflowsHandler {
            row_cursor: 1,
            ..Default::default()
        };
        let mut c = ProjectConfig::default();
        // Default pre_commit_cursor = 0 (pre-commit)
        h.handle_input(key(KeyCode::Right), &mut c);
        assert_eq!(h.pre_commit_selection(), PreCommitFramework::Lefthook);
        h.handle_input(key(KeyCode::Right), &mut c);
        assert_eq!(h.pre_commit_selection(), PreCommitFramework::None);
        h.handle_input(key(KeyCode::Right), &mut c);
        assert_eq!(h.pre_commit_selection(), PreCommitFramework::None); // clamped
        h.handle_input(key(KeyCode::Left), &mut c);
        assert_eq!(h.pre_commit_selection(), PreCommitFramework::Lefthook);
    }

    #[test]
    fn enter_commits_and_advances() {
        let mut h = WorkflowsHandler::default();
        let mut c = ProjectConfig::default();
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Done));
        assert_eq!(c.workflows.ci, Some(CiProvider::GitHubActions));
        assert_eq!(c.workflows.pre_commit, Some(PreCommitFramework::PreCommit));
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
