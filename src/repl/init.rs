use super::REPL_COMMANDS;

use crate::config::{Config, SharedConfig};

use anyhow::{Context, Result};
use reedline::{
    default_emacs_keybindings, ColumnarMenu, DefaultCompleter, Emacs, FileBackedHistory, KeyCode,
    KeyModifiers, Keybindings, Reedline, ReedlineEvent, ReedlineMenu, ValidationResult, Validator,
};

const MENU_NAME: &str = "completion_menu";

pub struct Repl {
    pub editor: Reedline,
}

impl Repl {
    pub fn init(config: SharedConfig) -> Result<Self> {
        let multiline_commands: Vec<&'static str> = REPL_COMMANDS
            .iter()
            .filter(|(_, _, v)| *v)
            .map(|(v, _, _)| *v)
            .collect();
        let completer = Self::create_completer(config);
        let keybindings = Self::create_keybindings();
        let history = Self::create_history()?;
        let menu = Self::create_menu();
        let edit_mode = Box::new(Emacs::new(keybindings));
        let editor = Reedline::create()
            .with_completer(Box::new(completer))
            .with_history(history)
            .with_menu(menu)
            .with_edit_mode(edit_mode)
            .with_quick_completions(true)
            .with_partial_completions(true)
            .with_validator(Box::new(ReplValidator { multiline_commands }))
            .with_ansi_colors(true);
        Ok(Self { editor })
    }

    fn create_completer(config: SharedConfig) -> DefaultCompleter {
        let mut completion: Vec<String> = REPL_COMMANDS
            .into_iter()
            .map(|(v, _, _)| v.to_string())
            .collect();
        completion.extend(config.lock().repl_completions());
        let mut completer = DefaultCompleter::with_inclusions(&['.', '-', '_']).set_min_word_len(2);
        completer.insert(completion.clone());
        completer
    }

    fn create_keybindings() -> Keybindings {
        let mut keybindings = default_emacs_keybindings();
        keybindings.add_binding(
            KeyModifiers::NONE,
            KeyCode::Tab,
            ReedlineEvent::UntilFound(vec![
                ReedlineEvent::Menu(MENU_NAME.to_string()),
                ReedlineEvent::MenuNext,
            ]),
        );
        keybindings.add_binding(
            KeyModifiers::CONTROL,
            KeyCode::Char('l'),
            ReedlineEvent::ExecuteHostCommand(".clear screen".into()),
        );
        keybindings
    }

    fn create_menu() -> ReedlineMenu {
        let completion_menu = ColumnarMenu::default().with_name(MENU_NAME);
        ReedlineMenu::EngineCompleter(Box::new(completion_menu))
    }

    fn create_history() -> Result<Box<FileBackedHistory>> {
        Ok(Box::new(
            FileBackedHistory::with_file(1000, Config::history_file()?)
                .with_context(|| "Failed to setup history file")?,
        ))
    }
}

struct ReplValidator {
    multiline_commands: Vec<&'static str>,
}

impl Validator for ReplValidator {
    fn validate(&self, line: &str) -> ValidationResult {
        if line.split('"').count() % 2 == 0 || incomplete_brackets(line, &self.multiline_commands) {
            ValidationResult::Incomplete
        } else {
            ValidationResult::Complete
        }
    }
}

fn incomplete_brackets(line: &str, multiline_commands: &[&str]) -> bool {
    let mut balance: Vec<char> = Vec::new();
    let line = line.trim_start();
    if !multiline_commands.iter().any(|v| line.starts_with(v)) {
        return false;
    }

    for c in line.chars() {
        if c == '{' {
            balance.push('}');
        } else if c == '}' {
            if let Some(last) = balance.last() {
                if last == &c {
                    balance.pop();
                }
            }
        }
    }

    !balance.is_empty()
}
