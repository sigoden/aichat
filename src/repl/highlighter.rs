use super::REPL_COMMANDS;

use crate::config::GlobalConfig;

use nu_ansi_term::{Color, Style};
use reedline::{Highlighter, StyledText};

pub struct ReplHighlighter {
    config: GlobalConfig,
}

impl ReplHighlighter {
    pub fn new(config: &GlobalConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }
}

impl Highlighter for ReplHighlighter {
    fn highlight(&self, line: &str, _cursor: usize) -> StyledText {
        let color = Color::Default;
        let match_color = if self.config.read().highlight {
            Color::Green
        } else {
            color
        };

        let mut styled_text = StyledText::new();

        if REPL_COMMANDS.iter().any(|cmd| line.contains(cmd.name)) {
            let matches: Vec<&str> = REPL_COMMANDS
                .iter()
                .filter(|cmd| line.contains(cmd.name))
                .map(|cmd| cmd.name)
                .collect();
            let longest_match = matches.iter().fold(String::new(), |acc, &item| {
                if item.len() > acc.len() {
                    item.to_string()
                } else {
                    acc
                }
            });
            let buffer_split: Vec<&str> = line.splitn(2, &longest_match).collect();

            styled_text.push((Style::new().fg(color), buffer_split[0].to_string()));
            styled_text.push((Style::new().fg(match_color), longest_match));
            styled_text.push((Style::new().fg(color), buffer_split[1].to_string()));
        } else {
            styled_text.push((Style::new().fg(color), line.to_string()));
        }

        styled_text
    }
}
