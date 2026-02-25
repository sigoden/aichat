use super::REPL_COMMANDS;

use nu_ansi_term::{Color, Style};
use reedline::{Hinter, History};

pub struct ReplHinter {
    style: Style,
    current_hint: String,
}

impl ReplHinter {
    pub fn new() -> Self {
        Self {
            style: Style::new().fg(Color::LightGray),
            current_hint: String::new(),
        }
    }

    fn compute_hint(&self, line: &str, pos: usize) -> String {
        let prefix = line.get(0..pos).unwrap_or(line);
        let trimmed = prefix.trim_start();
        if trimmed.is_empty() {
            return String::new();
        }
        let first_char = trimmed.chars().next().unwrap_or_default();
        if first_char != '.' && first_char != '/' && first_char != 'r' {
            return String::new();
        }
        let mut candidates: Vec<&str> = REPL_COMMANDS
            .iter()
            .map(|cmd| cmd.name)
            .filter(|name| name.starts_with(trimmed))
            .collect();
        candidates.sort_unstable_by_key(|v| v.len());
        let best = match candidates.first() {
            Some(v) => v,
            None => return String::new(),
        };
        best.get(trimmed.len()..).unwrap_or_default().to_string()
    }
}

impl Hinter for ReplHinter {
    fn handle(
        &mut self,
        line: &str,
        pos: usize,
        _history: &dyn History,
        use_ansi_coloring: bool,
        _cwd: &str,
    ) -> String {
        self.current_hint = self.compute_hint(line, pos);
        if use_ansi_coloring && !self.current_hint.is_empty() {
            self.style.paint(&self.current_hint).to_string()
        } else {
            self.current_hint.clone()
        }
    }

    fn complete_hint(&self) -> String {
        self.current_hint.clone()
    }

    fn next_hint_token(&self) -> String {
        first_token(&self.current_hint)
    }
}

fn first_token(s: &str) -> String {
    s.split_whitespace().next().unwrap_or_default().to_string()
}
