use crate::config::SharedConfig;

use nu_ansi_term::{Color, Style};
use reedline::{Highlighter, StyledText};

pub struct ReplHighlighter {
    external_commands: Vec<String>,
    config: SharedConfig,
}

impl ReplHighlighter {
    pub fn new(external_commands: Vec<String>, config: SharedConfig) -> Self {
        Self {
            external_commands,
            config,
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

        if self
            .external_commands
            .clone()
            .iter()
            .any(|x| line.contains(x))
        {
            let matches: Vec<&str> = self
                .external_commands
                .iter()
                .filter(|c| line.contains(*c))
                .map(std::ops::Deref::deref)
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
