use super::{
    highlighter::ReplHighlighter, prompt::ReplPrompt, validator::ReplValidator, REPL_COMMANDS,
};

use crate::config::{Config, SharedConfig};

use anyhow::{Context, Result};
use reedline::{
    default_emacs_keybindings, ColumnarMenu, DefaultCompleter, Emacs, FileBackedHistory, KeyCode,
    KeyModifiers, Keybindings, Reedline, ReedlineEvent, ReedlineMenu,
};

const MENU_NAME: &str = "completion_menu";

pub struct Repl {
    pub(crate) editor: Reedline,
    pub(crate) prompt: ReplPrompt,
}

impl Repl {
    pub fn init(config: SharedConfig) -> Result<Self> {
        let commands: Vec<String> = REPL_COMMANDS
            .into_iter()
            .map(|(v, _)| v.to_string())
            .collect();

        let completer = Self::create_completer(config.clone(), &commands);
        let highlighter = ReplHighlighter::new(config.clone(), commands);
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
            .with_validator(Box::new(ReplValidator))
            .with_ansi_colors(true);
        let prompt = ReplPrompt::new(config);
        Ok(Self { editor, prompt })
    }

    fn create_completer(config: SharedConfig, commands: &[String]) -> DefaultCompleter {
        let mut completion = commands.to_vec();
        completion.extend(config.read().repl_completions());
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
