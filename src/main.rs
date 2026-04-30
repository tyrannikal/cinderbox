use std::io;
use strum::{Display, VariantArray};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Layout},
    style::Stylize,
    text::Line,
    widgets::{Block, Paragraph},
};

mod db_registry;
mod registry;
mod steps;
mod widgets;

use steps::{
    CURSOR_BLANK, CURSOR_MARKER, StepHandler, StepResult, database::DatabaseHandler,
    languages::LanguagesHandler, project_type::ProjectTypeHandler, vcs::VcsHandler,
    workflows::WorkflowsHandler,
};

fn main() -> io::Result<()> {
    let mut app = App::default();
    ratatui::run(|terminal| app.run(terminal))?;
    if app.confirmed {
        println!("{}", app.final_summary());
    }
    Ok(())
}

#[derive(Debug, Default)]
pub struct ProjectConfig {
    project_type: Option<ProjectType>,
    project_name: String,
    project_location: String,
    vcs: Option<Vcs>,
    default_branch: String,
    jj_colocate: bool,
    language_configs: Vec<LanguageConfig>,
    workflows: WorkflowConfig,
    database: DatabaseConfig,
    remotes: Vec<Remote>,
    extras: Vec<Extra>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct DatabaseConfig {
    pub(crate) database: Option<Database>,
    /// `Some(_)` only when `database` supports a run mode (server DBs).
    /// SQLite and `None` always have `run_mode == None`.
    pub(crate) run_mode: Option<RunMode>,
    /// Toggled drivers, paired with the language each is meant for. Drivers
    /// are scoped to languages the user already selected upstream — if the
    /// user never picked Python, no Python drivers can appear here.
    pub(crate) drivers: Vec<(Language, &'static str)>,
    /// Empty = "use the database's default port". Validated by
    /// `port_problem`; out-of-range values block Enter from advancing.
    pub(crate) port: String,
}

#[derive(Debug, Clone, Copy, PartialEq, VariantArray, Display)]
pub enum RunMode {
    Docker,
    Native,
    Managed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LanguageConfig {
    pub(crate) language: Language,
    pub(crate) tools: Vec<&'static str>,
    pub(crate) common_deps: Vec<&'static str>,
    pub(crate) custom_deps: String,
}

impl LanguageConfig {
    pub(crate) fn new(language: Language) -> Self {
        Self {
            language,
            tools: Vec::new(),
            common_deps: Vec::new(),
            custom_deps: String::new(),
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct WorkflowConfig {
    pub(crate) ci: Option<CiProvider>,
    pub(crate) pre_commit: Option<PreCommitFramework>,
}

#[derive(Debug, Clone, Copy, PartialEq, VariantArray, Display)]
pub enum CiProvider {
    None,
    #[strum(to_string = "GitHub Actions")]
    GitHubActions,
    #[strum(to_string = "GitLab CI")]
    GitLab,
    Woodpecker,
}

#[derive(Debug, Clone, Copy, PartialEq, VariantArray, Display)]
pub enum PreCommitFramework {
    #[strum(to_string = "pre-commit")]
    PreCommit,
    #[strum(to_string = "lefthook")]
    Lefthook,
    None,
}

#[derive(Debug, Default, VariantArray, Display)]
enum WizardStep {
    #[default]
    #[strum(to_string = "Project Type")]
    ProjectType,
    #[strum(to_string = "Version Control System")]
    Vcs,
    Languages,
    Workflows,
    Database,
    Remotes,
    Extras,
    Summary,
}

impl WizardStep {
    fn option_count(&self) -> usize {
        match self {
            Self::ProjectType => ProjectType::VARIANTS.len(),
            Self::Vcs => Vcs::VARIANTS.len(),
            Self::Languages => Language::VARIANTS.len(),
            Self::Workflows => 0,
            Self::Database => Database::VARIANTS.len(),
            Self::Remotes => Remote::VARIANTS.len(),
            Self::Extras => Extra::VARIANTS.len(),
            Self::Summary => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, VariantArray, Display)]
enum ProjectType {
    New,
    Existing,
}

#[derive(Debug, Clone, Copy, PartialEq, VariantArray, Display)]
pub enum Vcs {
    Git,
    #[strum(to_string = "Jujutsu (jj)")]
    Jujutsu,
    #[strum(to_string = "Skip")]
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, VariantArray, Display)]
enum Language {
    Rust,
    Python,
    Go,
    JavaScript,
    TypeScript,
    Java,
    #[strum(to_string = "C#")]
    CSharp,
    #[strum(to_string = "C/C++")]
    Cpp,
    Ruby,
    Zig,
    Haskell,
    Lua,
}

#[derive(Debug, Clone, Copy, PartialEq, VariantArray, Display)]
enum Database {
    PostgreSQL,
    MySQL,
    SQLite,
    MongoDB,
    Redis,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, VariantArray, Display)]
enum Remote {
    GitHub,
    Codeberg,
    GitLab,
    Bitbucket,
    #[strum(to_string = "Self-hosted")]
    SelfHosted,
}

#[derive(Debug, Clone, Copy, PartialEq, VariantArray, Display)]
enum Extra {
    #[strum(to_string = ".gitignore")]
    Gitignore,
    #[strum(to_string = "README")]
    Readme,
    #[strum(to_string = "LICENSE")]
    License,
}
#[derive(Debug, Default)]
struct App {
    step_index: usize,
    cursor: usize,
    config: ProjectConfig,
    selected_remotes: Vec<Remote>,
    selected_extras: Vec<Extra>,
    confirmed: bool,
    exit: bool,
    project_type_handler: ProjectTypeHandler,
    vcs_handler: VcsHandler,
    languages_handler: LanguagesHandler,
    workflows_handler: WorkflowsHandler,
    database_handler: DatabaseHandler,
}

impl App {
    fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
        }

        if self.confirmed {
            self.project_type_handler.execute(&self.config)?;
            self.vcs_handler.execute(&self.config)?;
            self.languages_handler.execute(&self.config)?;
            self.workflows_handler.execute(&self.config)?;
            self.database_handler.execute(&self.config)?;
        }
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        let [wizard_area, config_area] =
            Layout::horizontal([Constraint::Ratio(2, 3), Constraint::Ratio(1, 3)])
                .areas(frame.area());

        // Wizard panel (left 2/3)
        let title = Line::from(format!(" cinderbox — {} ", self.current_step())).bold();
        let mut instruction_spans = vec![];
        let handler = self.current_handler();
        let in_details = handler.is_some_and(|h| h.in_details());
        let is_expanded = handler.is_some_and(|h| h.is_expanded());
        if in_details {
            instruction_spans.push(" Back ".into());
            instruction_spans.push("<Esc> ".blue().bold());
        } else if is_expanded {
            instruction_spans.push(" Collapse ".into());
            instruction_spans.push("<←/H> ".blue().bold());
        } else if self.step_index > 0 {
            instruction_spans.push(" Back ".into());
            instruction_spans.push("<←/H> ".blue().bold());
        }
        match self.current_step() {
            WizardStep::Remotes | WizardStep::Extras => {
                instruction_spans.push(" Toggle ".into());
                instruction_spans.push("<Enter> ".blue().bold());
                instruction_spans.push(" Confirm ".into());
                instruction_spans.push("<→/L> ".blue().bold());
            }
            // Languages at Choice focus: row 0 is "Next" (advance); rows 1..= are
            // languages where Enter/→/L checks + expands the highlighted supported
            // language (no-op on unsupported), Space deselects-only (uncheck a
            // checked language, no-op otherwise). SubField focus falls through
            // to "Confirm <Enter>" below.
            WizardStep::Languages if !in_details => {
                instruction_spans.push(" Select/Next ".into());
                instruction_spans.push("<Enter/→/L> ".blue().bold());
                instruction_spans.push(" Deselect ".into());
                instruction_spans.push("<Space> ".blue().bold());
            }
            WizardStep::Languages if in_details => {
                instruction_spans.push(" Confirm ".into());
                instruction_spans.push("<Enter> ".blue().bold());
            }
            WizardStep::Summary => {
                instruction_spans.push(" Confirm ".into());
                instruction_spans.push("<Enter> ".blue().bold());
            }
            _ if in_details => {
                // SubField focus: only Enter advances; →/L are captured by the focused
                // sub-field (text input cursor, colocate radio, or 2D cursor columns).
                instruction_spans.push(" Next ".into());
                instruction_spans.push("<Enter> ".blue().bold());
            }
            _ => {
                instruction_spans.push(" Next ".into());
                instruction_spans.push("<Enter/→/L> ".blue().bold());
            }
        }
        if !matches!(self.current_step(), WizardStep::Summary) {
            instruction_spans.push(" Navigate ".into());
            instruction_spans.push("<↑/K/↓/J> ".blue().bold());
        }
        instruction_spans.push(" Peek ".into());
        instruction_spans.push("<Shift+←/→> ".blue().bold());
        instruction_spans.push(" Quit ".into());
        instruction_spans.push("<Q> ".blue().bold());

        let instructions = Line::from(instruction_spans);
        let wizard_block = Block::bordered()
            .title(title.centered())
            .title_bottom(instructions.centered());

        // For handler-backed steps, delegate rendering
        match self.current_step() {
            WizardStep::ProjectType => {
                let inner = wizard_block.inner(wizard_area);
                frame.render_widget(wizard_block, wizard_area);
                self.project_type_handler.render(frame, inner);
            }
            WizardStep::Vcs => {
                let inner = wizard_block.inner(wizard_area);
                frame.render_widget(wizard_block, wizard_area);
                self.vcs_handler.render(frame, inner);
            }
            WizardStep::Languages => {
                let inner = wizard_block.inner(wizard_area);
                frame.render_widget(wizard_block, wizard_area);
                self.languages_handler.render(frame, inner);
            }
            WizardStep::Workflows => {
                let inner = wizard_block.inner(wizard_area);
                frame.render_widget(wizard_block, wizard_area);
                self.workflows_handler.render(frame, inner);
            }
            WizardStep::Database => {
                let inner = wizard_block.inner(wizard_area);
                frame.render_widget(wizard_block, wizard_area);
                self.database_handler.render(frame, inner);
            }
            _ => {
                let content = self.step_content();
                let wizard = Paragraph::new(content).block(wizard_block);
                frame.render_widget(wizard, wizard_area);
            }
        }

        // Config panel (right 1/3)
        let config_block = Block::bordered().title(Line::from(" Config ").bold().centered());

        let config_text = self.config_summary();
        let config = Paragraph::new(config_text).block(config_block);
        frame.render_widget(config, config_area);

        // Overlays render last so they can dim everything that came before.
        if self.step_index == 0 && self.project_type_handler.is_browsing() {
            self.project_type_handler.render_overlay(frame, wizard_area);
        }
    }

    fn render_multi_select_list<T: std::fmt::Display + PartialEq>(
        &self,
        variants: &[T],
        selected: &[T],
    ) -> String {
        variants
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let cursor = if i == self.cursor { CURSOR_MARKER } else { CURSOR_BLANK };
                let check = if selected.contains(v) { "[x]" } else { "[ ]" };
                format!("{cursor}{check} {v}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn step_content(&self) -> String {
        match self.current_step() {
            WizardStep::ProjectType => String::new(), // handled by ProjectTypeHandler
            WizardStep::Vcs => String::new(),          // handled by VcsHandler
            WizardStep::Languages => String::new(),    // handled by LanguagesHandler
            WizardStep::Workflows => String::new(),    // handled by WorkflowsHandler
            WizardStep::Database => String::new(),     // handled by DatabaseHandler
            WizardStep::Remotes => {
                self.render_multi_select_list(Remote::VARIANTS, &self.selected_remotes)
            }
            WizardStep::Extras => {
                self.render_multi_select_list(Extra::VARIANTS, &self.selected_extras)
            }
            WizardStep::Summary => self.summary_content(),
        }
    }

    fn config_summary(&self) -> String {
        self.get_summary().join("\n")
    }

    fn final_summary(&self) -> String {
        let lines = self.get_summary();

        format!("cinderbox — Project Configuration\n{}", lines.join("\n"))
    }

    fn get_summary(&self) -> Vec<String> {
        let c = &self.config;
        let name_display = if c.project_name.is_empty() {
            "—".to_string()
        } else {
            c.project_name.clone()
        };
        let location_display = if c.project_location.is_empty() {
            "—".to_string()
        } else {
            c.project_location.clone()
        };
        let mut lines = vec![
            format!(
                "Project Type: {}",
                c.project_type.map_or("—".to_string(), |v| v.to_string())
            ),
            format!("  Name: {name_display}"),
            format!("  Location: {location_display}"),
        ];
        lines.push(format!(
            "VCS: {}",
            c.vcs.map_or("—".to_string(), |v| v.to_string())
        ));
        match c.vcs {
            Some(Vcs::Git) => {
                let branch = if c.default_branch.is_empty() {
                    "(default)".to_string()
                } else {
                    c.default_branch.clone()
                };
                lines.push(format!("  Default branch: {branch}"));
            }
            Some(Vcs::Jujutsu) => {
                lines.push(format!(
                    "  Mode: {}",
                    if c.jj_colocate { "Colocated with git" } else { "Native" }
                ));
                if c.jj_colocate {
                    let branch = if c.default_branch.is_empty() {
                        "(default)".to_string()
                    } else {
                        c.default_branch.clone()
                    };
                    lines.push(format!("  Default branch: {branch}"));
                }
            }
            _ => {}
        }
        // Languages (multi-line: one block per selected language)
        if c.language_configs.is_empty() {
            lines.push("Languages: —".to_string());
        } else {
            lines.push("Languages:".to_string());
            for lc in &c.language_configs {
                lines.push(format!("  {}:", lc.language));
                if !lc.tools.is_empty() {
                    lines.push(format!("    Tools: {}", lc.tools.join(", ")));
                }
                let mut deps: Vec<String> =
                    lc.common_deps.iter().map(|d| (*d).to_string()).collect();
                if !lc.custom_deps.trim().is_empty() {
                    for dep in lc.custom_deps.split(',').map(|s| s.trim()) {
                        if !dep.is_empty() {
                            deps.push(dep.to_string());
                        }
                    }
                }
                if !deps.is_empty() {
                    lines.push(format!("    Dependencies: {}", deps.join(", ")));
                }
            }
        }
        // Workflows
        let ci = c.workflows.ci.map_or("—".to_string(), |v| v.to_string());
        let pre = c
            .workflows
            .pre_commit
            .map_or("—".to_string(), |v| v.to_string());
        lines.push("Workflows:".to_string());
        lines.push(format!("  CI: {ci}"));
        lines.push(format!("  Pre-commit: {pre}"));
        // Database (multi-line for selected DB; includes run mode, port, drivers per language)
        let db = &c.database;
        match db.database {
            None => lines.push("Database: —".to_string()),
            Some(Database::None) => lines.push("Database: None".to_string()),
            Some(database) => {
                lines.push(format!("Database: {database}"));
                if let Some(rm) = db.run_mode {
                    lines.push(format!("  Run mode: {rm}"));
                }
                let spec = db_registry::spec_for(database);
                if let Some(default_port) = spec.default_port {
                    let port_display = if db.port.is_empty() {
                        format!("{default_port} (default)")
                    } else {
                        db.port.clone()
                    };
                    lines.push(format!("  Port: {port_display}"));
                }
                if !db.drivers.is_empty() {
                    lines.push("  Drivers:".to_string());
                    for lang in [
                        Language::Rust,
                        Language::Python,
                        Language::Go,
                        Language::JavaScript,
                        Language::TypeScript,
                        Language::Java,
                        Language::CSharp,
                        Language::Cpp,
                        Language::Ruby,
                        Language::Zig,
                        Language::Haskell,
                        Language::Lua,
                    ] {
                        let labels: Vec<&'static str> = db
                            .drivers
                            .iter()
                            .filter(|(l, _)| *l == lang)
                            .filter_map(|(_, id)| {
                                db_registry::driver_by_id(lang, id).map(|d| d.label)
                            })
                            .collect();
                        if !labels.is_empty() {
                            lines.push(format!("    {lang}: {}", labels.join(", ")));
                        }
                    }
                }
            }
        }
        lines.push(Self::format_config_list("Remotes", &c.remotes, "—"));
        lines.push(Self::format_config_list("Extras", &c.extras, "—"));
        lines
    }

    fn format_config_list<T: std::fmt::Display>(label: &str, items: &[T], none: &str) -> String {
        if items.is_empty() {
            format!("{label}: {none}")
        } else {
            let joined: Vec<String> = items.iter().map(|i| i.to_string()).collect();
            format!("{label}: {}", joined.join(", "))
        }
    }

    fn summary_content(&self) -> String {
        let mut lines = vec!["Review your selections:\n".to_string()];
        lines.extend(self.get_summary());

        let mut actions = self.project_type_handler.planned_actions(&self.config);
        actions.extend(self.vcs_handler.planned_actions(&self.config));
        actions.extend(self.languages_handler.planned_actions(&self.config));
        actions.extend(self.workflows_handler.planned_actions(&self.config));
        actions.extend(self.database_handler.planned_actions(&self.config));
        if !actions.is_empty() {
            lines.push(String::new());
            lines.push("Planned actions:".to_string());
            for action in actions {
                lines.push(format!("  • {action}"));
            }
        }

        lines.push(String::new());
        lines.push("Press Enter to confirm.".to_string());

        lines.join("\n")
    }

    fn handle_events(&mut self) -> io::Result<()> {
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                let shift = key.modifiers.contains(KeyModifiers::SHIFT);

                // Global peek navigation (Shift+Left/Right) — always works,
                // bypasses handlers and validation so users can freely move between steps
                if shift {
                    match key.code {
                        KeyCode::Left => {
                            self.prev();
                            return Ok(());
                        }
                        KeyCode::Right => {
                            self.next();
                            return Ok(());
                        }
                        _ => {}
                    }
                }

                // Delegate to step handlers (before global keys, since text
                // inputs need to capture all keys including 'q')
                match self.current_step() {
                    WizardStep::ProjectType => {
                        match self
                            .project_type_handler
                            .handle_input(key, &mut self.config)
                        {
                            StepResult::Done => self.next(),
                            StepResult::Back => self.prev(),
                            StepResult::Quit => self.exit = true,
                            StepResult::Continue => {}
                        }
                        return Ok(());
                    }
                    WizardStep::Vcs => {
                        match self.vcs_handler.handle_input(key, &mut self.config) {
                            StepResult::Done => self.next(),
                            StepResult::Back => self.prev(),
                            StepResult::Quit => self.exit = true,
                            StepResult::Continue => {}
                        }
                        return Ok(());
                    }
                    WizardStep::Languages => {
                        match self.languages_handler.handle_input(key, &mut self.config) {
                            StepResult::Done => self.next(),
                            StepResult::Back => self.prev(),
                            StepResult::Quit => self.exit = true,
                            StepResult::Continue => {}
                        }
                        return Ok(());
                    }
                    WizardStep::Workflows => {
                        match self.workflows_handler.handle_input(key, &mut self.config) {
                            StepResult::Done => self.next(),
                            StepResult::Back => self.prev(),
                            StepResult::Quit => self.exit = true,
                            StepResult::Continue => {}
                        }
                        return Ok(());
                    }
                    WizardStep::Database => {
                        match self.database_handler.handle_input(key, &mut self.config) {
                            StepResult::Done => self.next(),
                            StepResult::Back => self.prev(),
                            StepResult::Quit => self.exit = true,
                            StepResult::Continue => {}
                        }
                        return Ok(());
                    }
                    _ => {}
                }

                // Inline handling for other steps (to be migrated later)
                match key.code {
                    KeyCode::Char('q') => self.exit = true,
                    KeyCode::Right | KeyCode::Char('l') => self.select_or_next(),
                    KeyCode::Left | KeyCode::Char('h') => self.prev(),
                    KeyCode::Down | KeyCode::Char('j') => self.cursor_down(),
                    KeyCode::Up | KeyCode::Char('k') => self.cursor_up(),
                    KeyCode::Enter | KeyCode::Char(' ') => self.select(),
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn current_step(&self) -> &WizardStep {
        debug_assert!(self.step_index < WizardStep::VARIANTS.len());
        &WizardStep::VARIANTS[self.step_index]
    }

    /// Returns the `StepHandler` trait object for the current step, if one exists.
    /// Steps that are still inline in `main.rs` (Database, Remotes, Extras, Summary)
    /// return `None`; they'll return `Some` once extracted.
    fn current_handler(&self) -> Option<&dyn StepHandler> {
        match self.current_step() {
            WizardStep::ProjectType => Some(&self.project_type_handler),
            WizardStep::Vcs => Some(&self.vcs_handler),
            WizardStep::Languages => Some(&self.languages_handler),
            WizardStep::Workflows => Some(&self.workflows_handler),
            WizardStep::Database => Some(&self.database_handler),
            _ => None,
        }
    }

    fn cursor_down(&mut self) {
        if self.cursor + 1 < self.current_step().option_count() {
            self.cursor += 1;
        }
    }

    fn cursor_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn select_or_next(&mut self) {
        match self.current_step() {
            // handler-owned steps: input goes through handle_events's delegation branch
            WizardStep::ProjectType
            | WizardStep::Vcs
            | WizardStep::Languages
            | WizardStep::Workflows
            | WizardStep::Database => {}
            WizardStep::Remotes => {
                self.config.remotes = std::mem::take(&mut self.selected_remotes);
                self.next();
            }
            WizardStep::Extras => {
                self.config.extras = std::mem::take(&mut self.selected_extras);
                self.next();
            }
            WizardStep::Summary => {}
        }
    }

    fn select(&mut self) {
        debug_assert!(
            self.cursor < self.current_step().option_count()
                || matches!(self.current_step(), WizardStep::Summary)
        );
        match self.current_step() {
            // handler-owned steps
            WizardStep::ProjectType
            | WizardStep::Vcs
            | WizardStep::Languages
            | WizardStep::Workflows
            | WizardStep::Database => {}
            WizardStep::Remotes => {
                let remote = Remote::VARIANTS[self.cursor];
                if let Some(pos) = self.selected_remotes.iter().position(|r| *r == remote) {
                    self.selected_remotes.remove(pos);
                } else {
                    self.selected_remotes.push(remote);
                }
            }
            WizardStep::Extras => {
                let extra = Extra::VARIANTS[self.cursor];
                if let Some(pos) = self.selected_extras.iter().position(|e| *e == extra) {
                    self.selected_extras.remove(pos);
                } else {
                    self.selected_extras.push(extra);
                }
            }
            WizardStep::Summary => {
                self.confirmed = true;
                self.exit = true;
            }
        }
    }

    fn restore_cursor(&mut self) {
        self.cursor = match self.current_step() {
            WizardStep::ProjectType => {
                self.project_type_handler.restore_from_config(&self.config);
                return;
            }
            WizardStep::Vcs => {
                self.vcs_handler.restore_from_config(&self.config);
                return;
            }
            WizardStep::Languages => {
                self.languages_handler.restore_from_config(&self.config);
                return;
            }
            WizardStep::Workflows => {
                self.workflows_handler.restore_from_config(&self.config);
                return;
            }
            WizardStep::Database => {
                self.database_handler.restore_from_config(&self.config);
                return;
            }
            WizardStep::Remotes => {
                self.selected_remotes.clone_from(&self.config.remotes);
                0
            }
            WizardStep::Extras => {
                self.selected_extras.clone_from(&self.config.extras);
                0
            }
            _ => 0,
        };
    }

    fn next(&mut self) {
        if self.step_index + 1 < WizardStep::VARIANTS.len() {
            self.step_index += 1;
            self.restore_cursor();
        }
    }

    fn prev(&mut self) {
        if self.step_index > 0 {
            self.step_index -= 1;
            self.restore_cursor();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- ProjectConfig defaults ---

    #[test]
    fn project_config_defaults_are_none_or_empty() {
        let c = ProjectConfig::default();
        assert!(c.project_type.is_none());
        assert!(c.project_name.is_empty());
        assert!(c.project_location.is_empty());
        assert!(c.vcs.is_none());
        assert!(c.default_branch.is_empty());
        assert!(!c.jj_colocate);
        assert!(c.language_configs.is_empty());
        assert!(c.workflows.ci.is_none());
        assert!(c.workflows.pre_commit.is_none());
        assert!(c.database.database.is_none());
        assert!(c.database.run_mode.is_none());
        assert!(c.database.drivers.is_empty());
        assert!(c.database.port.is_empty());
        assert!(c.remotes.is_empty());
        assert!(c.extras.is_empty());
    }

    #[test]
    fn project_config_fields_are_independent() {
        let mut c = ProjectConfig {
            project_type: Some(ProjectType::New),
            project_name: "test".to_string(),
            ..Default::default()
        };
        assert!(c.vcs.is_none());
        assert!(c.language_configs.is_empty());

        c.vcs = Some(Vcs::Git);
        c.default_branch = "develop".to_string();
        assert_eq!(c.project_name, "test");
        assert!(c.database.database.is_none());
    }

    // --- Enum Display (strum) ---

    #[test]
    fn project_type_display() {
        assert_eq!(ProjectType::New.to_string(), "New");
        assert_eq!(ProjectType::Existing.to_string(), "Existing");
    }

    #[test]
    fn vcs_display() {
        assert_eq!(Vcs::Git.to_string(), "Git");
        assert_eq!(Vcs::Jujutsu.to_string(), "Jujutsu (jj)");
        assert_eq!(Vcs::None.to_string(), "Skip");
    }

    #[test]
    fn language_display_special_cases() {
        assert_eq!(Language::CSharp.to_string(), "C#");
        assert_eq!(Language::Cpp.to_string(), "C/C++");
        assert_eq!(Language::Rust.to_string(), "Rust");
    }

    #[test]
    fn extra_display() {
        assert_eq!(Extra::Gitignore.to_string(), ".gitignore");
        assert_eq!(Extra::Readme.to_string(), "README");
        assert_eq!(Extra::License.to_string(), "LICENSE");
    }

    // --- WizardStep option_count ---

    #[test]
    fn wizard_step_option_counts() {
        assert_eq!(WizardStep::ProjectType.option_count(), ProjectType::VARIANTS.len());
        assert_eq!(WizardStep::Vcs.option_count(), Vcs::VARIANTS.len());
        assert_eq!(WizardStep::Languages.option_count(), Language::VARIANTS.len());
        assert_eq!(WizardStep::Database.option_count(), Database::VARIANTS.len());
        assert_eq!(WizardStep::Remotes.option_count(), Remote::VARIANTS.len());
        assert_eq!(WizardStep::Extras.option_count(), Extra::VARIANTS.len());
        assert_eq!(WizardStep::Summary.option_count(), 0);
    }

    // --- App step navigation ---

    #[test]
    fn app_starts_at_step_zero() {
        let app = App::default();
        assert_eq!(app.step_index, 0);
        assert!(matches!(app.current_step(), WizardStep::ProjectType));
    }

    #[test]
    fn next_advances_step() {
        let mut app = App::default();
        app.next();
        assert_eq!(app.step_index, 1);
        assert!(matches!(app.current_step(), WizardStep::Vcs));
    }

    #[test]
    fn next_clamps_at_last_step() {
        let mut app = App::default();
        for _ in 0..100 {
            app.next();
        }
        assert_eq!(app.step_index, WizardStep::VARIANTS.len() - 1);
        assert!(matches!(app.current_step(), WizardStep::Summary));
    }

    #[test]
    fn prev_clamps_at_zero() {
        let mut app = App::default();
        app.prev();
        assert_eq!(app.step_index, 0);
    }

    #[test]
    fn prev_goes_back() {
        let mut app = App::default();
        app.next();
        app.next();
        assert_eq!(app.step_index, 2);
        app.prev();
        assert_eq!(app.step_index, 1);
    }

    // --- Cursor movement ---

    fn step_index_of(step: WizardStep) -> usize {
        WizardStep::VARIANTS
            .iter()
            .position(|s| std::mem::discriminant(s) == std::mem::discriminant(&step))
            .unwrap()
    }

    #[test]
    fn cursor_down_respects_option_count() {
        // Remotes is still inline (uses App.cursor); Database moved to a handler.
        let mut app = App {
            step_index: step_index_of(WizardStep::Remotes),
            cursor: 0,
            ..Default::default()
        };
        let count = app.current_step().option_count();
        for _ in 0..count + 5 {
            app.cursor_down();
        }
        assert_eq!(app.cursor, count - 1);
    }

    #[test]
    fn cursor_up_clamps_at_zero() {
        let mut app = App {
            step_index: step_index_of(WizardStep::Remotes),
            cursor: 2,
            ..Default::default()
        };
        app.cursor_up();
        assert_eq!(app.cursor, 1);
        app.cursor_up();
        assert_eq!(app.cursor, 0);
        app.cursor_up();
        assert_eq!(app.cursor, 0);
    }

    // Language multi-select behavior is now tested in steps::languages::tests.

    // Database step behavior is tested in steps::database::tests.

    // --- Summary confirm ---

    #[test]
    fn summary_select_confirms() {
        let mut app = App {
            step_index: WizardStep::VARIANTS.len() - 1, // Summary
            ..Default::default()
        };
        assert!(!app.confirmed);
        app.select();
        assert!(app.confirmed);
        assert!(app.exit);
    }

    // select_or_next for Languages is now tested in steps::languages::tests
    // (right_commits_selected_and_returns_done).

    // --- Config summary formatting ---

    #[test]
    fn config_summary_shows_dash_for_unset() {
        let app = App::default();
        let summary = app.config_summary();
        assert!(summary.contains("Project Type: —"));
        assert!(summary.contains("Name: —"));
        assert!(summary.contains("VCS: —"));
    }

    #[test]
    fn config_summary_shows_set_values() {
        let mut app = App::default();
        app.config.project_type = Some(ProjectType::New);
        app.config.project_name = "myproj".to_string();
        app.config.project_location = "/tmp".to_string();
        app.config.vcs = Some(Vcs::Git);
        app.config.default_branch = "main".to_string();
        let summary = app.config_summary();
        assert!(summary.contains("Project Type: New"));
        assert!(summary.contains("Name: myproj"));
        assert!(summary.contains("Location: /tmp"));
        assert!(summary.contains("VCS: Git"));
        assert!(summary.contains("Default branch: main"));
    }

    #[test]
    fn config_summary_jj_colocated_shows_mode() {
        let mut app = App::default();
        app.config.vcs = Some(Vcs::Jujutsu);
        app.config.jj_colocate = true;
        app.config.default_branch = "trunk".to_string();
        let summary = app.config_summary();
        assert!(summary.contains("Colocated with git"));
        assert!(summary.contains("Default branch: trunk"));
    }

    #[test]
    fn config_summary_jj_native_hides_branch() {
        let mut app = App::default();
        app.config.vcs = Some(Vcs::Jujutsu);
        app.config.jj_colocate = false;
        app.config.default_branch = "trunk".to_string();
        let summary = app.config_summary();
        assert!(summary.contains("Mode: Native"));
        assert!(!summary.contains("Default branch:"));
    }

    // --- format_config_list ---

    // --- Workflows step placement + count ---

    #[test]
    fn step_count_is_eight() {
        assert_eq!(WizardStep::VARIANTS.len(), 8);
    }

    #[test]
    fn workflows_step_is_inserted_between_languages_and_database() {
        let languages = step_index_of(WizardStep::Languages);
        let workflows = step_index_of(WizardStep::Workflows);
        let database = step_index_of(WizardStep::Database);
        assert_eq!(workflows, languages + 1);
        assert_eq!(database, workflows + 1);
    }

    #[test]
    fn summary_shows_database_block() {
        let mut app = App::default();
        app.config.database = DatabaseConfig {
            database: Some(Database::PostgreSQL),
            run_mode: Some(RunMode::Docker),
            drivers: vec![(Language::Python, "psycopg"), (Language::Rust, "sqlx")],
            port: "5433".to_string(),
        };
        let summary = app.config_summary();
        assert!(summary.contains("Database: PostgreSQL"));
        assert!(summary.contains("Run mode: Docker"));
        assert!(summary.contains("Port: 5433"));
        assert!(summary.contains("Rust: sqlx"));
        assert!(summary.contains("Python: psycopg"));
    }

    #[test]
    fn summary_shows_database_default_port_when_empty() {
        let mut app = App::default();
        app.config.database = DatabaseConfig {
            database: Some(Database::PostgreSQL),
            run_mode: Some(RunMode::Docker),
            drivers: vec![],
            port: String::new(),
        };
        let summary = app.config_summary();
        assert!(summary.contains("Port: 5432 (default)"));
    }

    #[test]
    fn summary_shows_database_none_inline() {
        let mut app = App::default();
        app.config.database = DatabaseConfig {
            database: Some(Database::None),
            ..Default::default()
        };
        let summary = app.config_summary();
        assert!(summary.contains("Database: None"));
        assert!(!summary.contains("Run mode"));
        assert!(!summary.contains("Port:"));
    }

    #[test]
    fn summary_shows_database_dash_when_unset() {
        let app = App::default();
        let summary = app.config_summary();
        assert!(summary.contains("Database: —"));
    }

    #[test]
    fn summary_shows_language_tools_and_workflows() {
        let mut app = App::default();
        app.config.language_configs = vec![LanguageConfig {
            language: Language::Python,
            tools: vec!["ruff", "pytest"],
            common_deps: vec!["fastapi"],
            custom_deps: "my-lib".to_string(),
        }];
        app.config.workflows = WorkflowConfig {
            ci: Some(CiProvider::GitHubActions),
            pre_commit: Some(PreCommitFramework::PreCommit),
        };
        let summary = app.config_summary();
        assert!(summary.contains("Python:"));
        assert!(summary.contains("Tools: ruff, pytest"));
        assert!(summary.contains("Dependencies: fastapi, my-lib"));
        assert!(summary.contains("CI: GitHub Actions"));
        assert!(summary.contains("Pre-commit: pre-commit"));
    }

    // --- format_config_list ---

    #[test]
    fn format_config_list_empty() {
        let empty: Vec<Language> = vec![];
        assert_eq!(App::format_config_list("Languages", &empty, "—"), "Languages: —");
    }

    #[test]
    fn format_config_list_multiple() {
        let items = vec![Language::Rust, Language::Go];
        assert_eq!(App::format_config_list("Languages", &items, "—"), "Languages: Rust, Go");
    }

    // --- render_multi_select_list ---

    #[test]
    fn render_multi_select_list_shows_checks() {
        let app = App {
            cursor: 0,
            ..Default::default()
        };
        let variants = [Language::Rust, Language::Go];
        let selected = [Language::Go];
        let output = app.render_multi_select_list(&variants, &selected);
        assert!(output.contains("[x] Go"));
        assert!(output.contains("[ ] Rust"));
    }
}
