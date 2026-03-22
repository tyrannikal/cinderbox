use std::io;
use strum::{Display, EnumCount, VariantArray};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Layout},
    style::Stylize,
    text::Line,
    widgets::{Block, Paragraph},
};

fn main() -> io::Result<()> {
    ratatui::run(|terminal| App::default().run(terminal))
}

#[derive(Debug, Default)]
struct ProjectConfig {
    project_type: Option<ProjectType>,
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
            Self::ProjectType => ProjectType::COUNT,
            Self::Vcs => Vcs::COUNT,
            Self::Languages => Language::COUNT,
            Self::Database => Database::COUNT,
            Self::Remotes => Remote::COUNT,
            Self::Extras => Extra::COUNT,
            Self::Summary => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, VariantArray, EnumCount, Display)]
enum ProjectType {
    New,
    Existing,
}

#[derive(Debug, Clone, Copy, PartialEq, VariantArray, EnumCount, Display)]
enum Vcs {
    Git,
    #[strum(to_string = "Jujutsu (jj)")]
    Jujutsu,
    #[strum(to_string = "Subversion (svn)")]
    Svn,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, VariantArray, EnumCount, Display)]
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

#[derive(Debug, Clone, Copy, PartialEq, VariantArray, EnumCount, Display)]
enum Database {
    PostgreSQL,
    MySQL,
    SQLite,
    MongoDB,
    Redis,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, VariantArray, EnumCount, Display)]
enum Remote {
    GitHub,
    Codeberg,
    GitLab,
    Bitbucket,
    #[strum(to_string = "Self-hosted")]
    SelfHosted,
}

#[derive(Debug, Clone, Copy, PartialEq, VariantArray, EnumCount, Display)]
enum Extra {
    #[strum(to_string = ".gitignore")]
    Gitignore,
    README,
    LICENSE,
}
#[derive(Debug, Default)]
struct App {
    step_index: usize,
    cursor: usize,
    config: ProjectConfig,
    selected_languages: Vec<Language>,
    selected_remotes: Vec<Remote>,
    selected_extras: Vec<Extra>,
    exit: bool,
}

impl App {
    fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
        }
        Ok(())
    }

    fn draw(&self, frame: &mut Frame) {
        let [wizard_area, config_area] =
            Layout::horizontal([Constraint::Ratio(2, 3), Constraint::Ratio(1, 3)])
                .areas(frame.area());

        // Wizard panel (left 2/3)
        let title = Line::from(format!(" cinderbox — {} ", self.current_step())).bold();
        let mut instruction_spans = vec![" Back ".into(), "<Left/H> ".blue().bold()];
        if matches!(
            self.current_step(),
            WizardStep::Languages | WizardStep::Remotes | WizardStep::Extras
        ) {
            instruction_spans.push(" Toggle ".into());
            instruction_spans.push("<Enter> ".blue().bold());
            instruction_spans.push(" Confirm ".into());
            instruction_spans.push("<Right/L> ".blue().bold());
        } else {
            instruction_spans.push(" Next ".into());
            instruction_spans.push("<Right/L> ".blue().bold());
        }
        instruction_spans.push(" Quit ".into());
        instruction_spans.push("<Q> ".blue().bold());
        let instructions = Line::from(instruction_spans);

        let wizard_block = Block::bordered()
            .title(title.centered())
            .title_bottom(instructions.centered());

        let content = self.step_content();
        let wizard = Paragraph::new(content).block(wizard_block);
        frame.render_widget(wizard, wizard_area);

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
            WizardStep::ProjectType => self.render_select_list(ProjectType::VARIANTS),
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
            _ => format!("Step: {}", self.current_step()),
        }
    }

    fn config_summary(&self) -> String {
        let mut lines = Vec::new();

        match &self.config.project_type {
            Some(pt) => lines.push(format!("Type: {pt}")),
            None => lines.push("Type: —".to_string()),
        }

        match &self.config.vcs {
            Some(vcs) => lines.push(format!("VCS: {vcs}")),
            None => lines.push("VCS: —".to_string()),
        }

        if self.config.languages.is_empty() {
            lines.push("Languages: —".to_string());
        } else {
            let langs: Vec<String> = self
                .config
                .languages
                .iter()
                .map(|l| l.to_string())
                .collect();
            lines.push(format!("Languages: {}", langs.join(", ")));
        }

        match &self.config.database {
            Some(db) => lines.push(format!("Database: {db}")),
            None => lines.push("Database: —".to_string()),
        }

        if self.config.remotes.is_empty() {
            lines.push("Remotes: —".to_string());
        } else {
            let remotes: Vec<String> = self.config.remotes.iter().map(|r| r.to_string()).collect();
            lines.push(format!("Remotes: {}", remotes.join(", ")));
        }

        if self.config.extras.is_empty() {
            lines.push("Extras: —".to_string());
        } else {
            let extras: Vec<String> = self.config.extras.iter().map(|e| e.to_string()).collect();
            lines.push(format!("Extras: {}", extras.join(", ")));
        }

        if lines.iter().all(|l| l.ends_with('—')) {
            return "No selections yet.".to_string();
        }

        lines.join("\n")
    }

    fn handle_events(&mut self) -> io::Result<()> {
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Char('q') => self.exit = true,
                KeyCode::Right | KeyCode::Char('l') => self.select_or_next(),
                KeyCode::Left | KeyCode::Char('h') => self.prev(),
                KeyCode::Down | KeyCode::Char('j') => self.cursor_down(),
                KeyCode::Up | KeyCode::Char('k') => self.cursor_up(),
                KeyCode::Enter | KeyCode::Char(' ') => self.select(),
                _ => {}
            },
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
            WizardStep::ProjectType | WizardStep::Vcs | WizardStep::Database => self.select(),
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
            _ => self.next(),
        }
    }

    fn select(&mut self) {
        match self.current_step() {
            WizardStep::ProjectType => {
                self.config.project_type = Some(ProjectType::VARIANTS[self.cursor]);
                self.next();
            }
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
            _ => {}
        }
    }

    fn restore_cursor(&mut self) {
        self.cursor = match self.current_step() {
            WizardStep::ProjectType => self
                .config
                .project_type
                .and_then(|pt| ProjectType::VARIANTS.iter().position(|v| *v == pt))
                .unwrap_or(0),
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
}
