use crate::config::SharedConfig;

use nu_ansi_term::{Color, Style};
use reedline::{Highlighter, StyledText};

const MATCH_COLOR: Color = Color::Green;

pub struct ReplHighlighter {
    external_commands: Vec<String>,
    config: SharedConfig,
}

impl ReplHighlighter {
    /// Construct the default highlighter with a given set of extern commands/keywords to detect and highlight
    pub fn new(config: SharedConfig, external_commands: Vec<String>) -> Self {
        Self {
            external_commands,
            config,
        }
    }
}

impl Highlighter for ReplHighlighter {
    fn highlight(&self, line: &str, _cursor: usize) -> StyledText {
        let mut styled_text = StyledText::new();
        let color = if self.config.read().light_theme {
            Color::Black
        } else {
            Color::White
        };
        let match_color = if self.config.read().highlight {
            MATCH_COLOR
        } else {
            color
        };

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
