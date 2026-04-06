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

mod steps;
mod widgets;

use steps::{StepHandler, StepResult, project_type::ProjectTypeHandler};

fn main() -> io::Result<()> {
    let mut app = App::default();
    ratatui::run(|terminal| app.run(terminal))?;
    if app.confirmed {
        println!("{}", app.final_summary());
    }
    Ok(())
}

#[derive(Debug, Default)]
struct ProjectConfig {
    project_type: Option<ProjectType>,
    project_name: String,
    project_location: String,
    vcs: Option<Vcs>,
    languages: Vec<Language>,
    database: Option<Database>,
    remotes: Vec<Remote>,
    extras: Vec<Extra>,
}

#[derive(Debug, Default, VariantArray, Display)]
enum WizardStep {
    #[default]
    #[strum(to_string = "Project Type")]
    ProjectType,
    #[strum(to_string = "Version Control System")]
    Vcs,
    Languages,
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
enum Vcs {
    Git,
    #[strum(to_string = "Jujutsu (jj)")]
    Jujutsu,
    #[strum(to_string = "Subversion (svn)")]
    Svn,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, VariantArray, Display)]
enum Language {
    Rust,
    Go,
    Python,
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
    selected_languages: Vec<Language>,
    selected_remotes: Vec<Remote>,
    selected_extras: Vec<Extra>,
    confirmed: bool,
    exit: bool,
    project_type_handler: ProjectTypeHandler,
}

impl App {
    fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
        }

        if self.confirmed {
            self.project_type_handler.execute(&self.config)?;
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
        if self.step_index == 0 && self.project_type_handler.in_details() {
            instruction_spans.push(" Back ".into());
            instruction_spans.push("<Esc> ".blue().bold());
        } else if self.step_index == 0 && self.project_type_handler.is_expanded() {
            instruction_spans.push(" Collapse ".into());
            instruction_spans.push("<←/H> ".blue().bold());
        } else if self.step_index > 0 {
            instruction_spans.push(" Back ".into());
            instruction_spans.push("<←/H> ".blue().bold());
        }
        match self.current_step() {
            WizardStep::Languages | WizardStep::Remotes | WizardStep::Extras => {
                instruction_spans.push(" Toggle ".into());
                instruction_spans.push("<Enter> ".blue().bold());
                instruction_spans.push(" Confirm ".into());
                instruction_spans.push("<→/L> ".blue().bold());
            }
            WizardStep::Summary => {
                instruction_spans.push(" Confirm ".into());
                instruction_spans.push("<Enter> ".blue().bold());
            }
            _ => {
                instruction_spans.push(" Next ".into());
                instruction_spans.push("<Enter/→/L> ".blue().bold());
            }
        }
        instruction_spans.push(" Peek ".into());
        instruction_spans.push("<Shift+←/→> ".blue().bold());
        instruction_spans.push(" Quit ".into());
        instruction_spans.push("<Q> ".blue().bold());

        let instructions = Line::from(instruction_spans);
        let wizard_block = Block::bordered()
            .title(title.centered())
            .title_bottom(instructions.centered());

        // For ProjectType, delegate rendering to the handler
        match self.current_step() {
            WizardStep::ProjectType => {
                let inner = wizard_block.inner(wizard_area);
                frame.render_widget(wizard_block, wizard_area);
                self.project_type_handler.render(frame, inner);
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
    }

    fn render_select_list<T: std::fmt::Display>(&self, variants: &[T]) -> String {
        variants
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let marker = if i == self.cursor { "▸ " } else { "  " };
                format!("{marker}{v}")
            })
            .collect::<Vec<_>>()
            .join("\n")
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
                let cursor = if i == self.cursor { "▸ " } else { "  " };
                let check = if selected.contains(v) { "[x]" } else { "[ ]" };
                format!("{cursor}{check} {v}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn step_content(&self) -> String {
        match self.current_step() {
            WizardStep::ProjectType => String::new(), // handled by ProjectTypeHandler
            WizardStep::Vcs => self.render_select_list(Vcs::VARIANTS),
            WizardStep::Languages => {
                self.render_multi_select_list(Language::VARIANTS, &self.selected_languages)
            }
            WizardStep::Database => self.render_select_list(Database::VARIANTS),
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
        lines.extend([
            format!("VCS: {}", c.vcs.map_or("—".to_string(), |v| v.to_string())),
            Self::format_config_list("Languages", &c.languages, "—"),
            format!(
                "Database: {}",
                c.database.map_or("—".to_string(), |v| v.to_string())
            ),
            Self::format_config_list("Remotes", &c.remotes, "—"),
            Self::format_config_list("Extras", &c.extras, "—"),
        ]);
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

        let actions = self.project_type_handler.planned_actions(&self.config);
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
                            self.peek_prev();
                            return Ok(());
                        }
                        KeyCode::Right => {
                            self.peek_next();
                            return Ok(());
                        }
                        _ => {}
                    }
                }

                // Delegate to step handler for ProjectType (before global keys,
                // since text input needs to capture all keys including 'q')
                if matches!(self.current_step(), WizardStep::ProjectType) {
                    match self
                        .project_type_handler
                        .handle_input(key.code, &mut self.config)
                    {
                        StepResult::Done => self.next(),
                        StepResult::Back => self.prev(),
                        StepResult::Quit => self.exit = true,
                        StepResult::Continue => {}
                    }
                    return Ok(());
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
        &WizardStep::VARIANTS[self.step_index]
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
            WizardStep::ProjectType => {} // handled by ProjectTypeHandler
            WizardStep::Vcs | WizardStep::Database => self.select(),
            WizardStep::Languages => {
                self.config.languages = self.selected_languages.clone();
                self.next();
            }
            WizardStep::Remotes => {
                self.config.remotes = self.selected_remotes.clone();
                self.next();
            }
            WizardStep::Extras => {
                self.config.extras = self.selected_extras.clone();
                self.next();
            }
            WizardStep::Summary => {}
        }
    }

    fn select(&mut self) {
        match self.current_step() {
            WizardStep::ProjectType => {} // handled by ProjectTypeHandler
            WizardStep::Vcs => {
                self.config.vcs = Some(Vcs::VARIANTS[self.cursor]);
                self.next();
            }
            WizardStep::Languages => {
                let lang = Language::VARIANTS[self.cursor];
                if let Some(pos) = self.selected_languages.iter().position(|l| *l == lang) {
                    self.selected_languages.remove(pos);
                } else {
                    self.selected_languages.push(lang);
                }
            }
            WizardStep::Database => {
                self.config.database = Some(Database::VARIANTS[self.cursor]);
                self.next();
            }
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
            WizardStep::Vcs => self
                .config
                .vcs
                .and_then(|vcs| Vcs::VARIANTS.iter().position(|v| *v == vcs))
                .unwrap_or(0),
            WizardStep::Languages => {
                self.selected_languages = self.config.languages.clone();
                0
            }
            WizardStep::Database => self
                .config
                .database
                .and_then(|db| Database::VARIANTS.iter().position(|v| *v == db))
                .unwrap_or(0),
            WizardStep::Remotes => {
                self.selected_remotes = self.config.remotes.clone();
                0
            }
            WizardStep::Extras => {
                self.selected_extras = self.config.extras.clone();
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

    fn peek_next(&mut self) {
        if self.step_index + 1 < WizardStep::VARIANTS.len() {
            self.step_index += 1;
            self.restore_cursor();
        }
    }

    fn peek_prev(&mut self) {
        if self.step_index > 0 {
            self.step_index -= 1;
            self.restore_cursor();
        }
    }
}
