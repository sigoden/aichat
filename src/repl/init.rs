use super::REPL_COMMANDS;

use crate::config::{Config, SharedConfig};

use anyhow::{Context, Result};
use nu_ansi_term::Color;
use reedline::{
    default_emacs_keybindings, ColumnarMenu, DefaultCompleter, DefaultValidator, Emacs,
    ExampleHighlighter, FileBackedHistory, KeyCode, KeyModifiers, Keybindings, Reedline,
    ReedlineEvent, ReedlineMenu,
};

const MENU_NAME: &str = "completion_menu";
const MATCH_COLOR: Color = Color::Green;
const NEUTRAL_COLOR: Color = Color::White;
const NEUTRAL_COLOR_LIGHT: Color = Color::Black;

pub struct Repl {
    pub editor: Reedline,
}

impl Repl {
    pub fn init(config: SharedConfig) -> Result<Self> {
        let commands: Vec<String> = REPL_COMMANDS
            .into_iter()
            .map(|(v, _)| v.to_string())
            .collect();

        let completer = Self::create_completer(config.clone(), &commands);
        let highlighter = Self::create_highlighter(config, &commands);
        let keybindings = Self::create_keybindings();
        let history = Self::create_history()?;
        let menu = Self::create_menu();
        let edit_mode = Box::new(Emacs::new(keybindings));
        let editor = Reedline::create()
            .with_completer(Box::new(completer))
            .with_highlighter(Box::new(highlighter))
            .with_history(history)
            .with_menu(menu)
            .with_edit_mode(edit_mode)
            .with_quick_completions(true)
            .with_partial_completions(true)
            .with_validator(Box::new(DefaultValidator))
            .with_ansi_colors(true);
        Ok(Self { editor })
    }

    fn create_completer(config: SharedConfig, commands: &[String]) -> DefaultCompleter {
        let mut completion = commands.to_vec();
        completion.extend(config.lock().repl_completions());
        let mut completer = DefaultCompleter::with_inclusions(&['.', '-', '_']).set_min_word_len(2);
        completer.insert(completion.clone());
        completer
    }

    fn create_highlighter(config: SharedConfig, commands: &[String]) -> ExampleHighlighter {
        let mut highlighter = ExampleHighlighter::new(commands.to_vec());
        if config.lock().light_theme {
            highlighter.change_colors(MATCH_COLOR, NEUTRAL_COLOR_LIGHT, NEUTRAL_COLOR_LIGHT);
        } else {
            highlighter.change_colors(MATCH_COLOR, NEUTRAL_COLOR, NEUTRAL_COLOR);
        }
        highlighter
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
