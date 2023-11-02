use std::collections::HashMap;

use super::{parse_command, REPL_COMMANDS};

use crate::config::GlobalConfig;

use reedline::{Completer, Span, Suggestion};

impl Completer for ReplCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        let mut suggestions = vec![];
        if line.len() != pos {
            return suggestions;
        }
        let line = &line[0..pos];
        if let Some((cmd, args)) = parse_command(line) {
            let commands: Vec<_> = self
                .commands
                .iter()
                .filter(|(cmd_name, _)| match args {
                    Some(args) => cmd_name.starts_with(&format!("{cmd} {args}")),
                    None => cmd_name.starts_with(cmd),
                })
                .collect();

            if args.is_some() || line.ends_with(' ') {
                let args = args.unwrap_or_default();
                let start = line.chars().take_while(|c| *c == ' ').count() + cmd.len() + 1;
                let span = Span::new(start, pos);
                suggestions.extend(
                    self.config
                        .read()
                        .repl_complete(cmd, args)
                        .iter()
                        .map(|name| create_suggestion(name.clone(), None, span)),
                )
            }

            if suggestions.is_empty() {
                let start = line.chars().take_while(|c| *c == ' ').count();
                let span = Span::new(start, pos);
                suggestions.extend(commands.iter().map(|(name, desc)| {
                    let has_group = self.groups.get(name).map(|v| *v > 1).unwrap_or_default();
                    let name = if has_group {
                        name.to_string()
                    } else {
                        format!("{name} ")
                    };
                    create_suggestion(name, Some(desc.to_string()), span)
                }))
            }
        }
        suggestions
    }
}

pub struct ReplCompleter {
    config: GlobalConfig,
    commands: Vec<(&'static str, &'static str)>,
    groups: HashMap<&'static str, usize>,
}

impl ReplCompleter {
    pub fn new(config: &GlobalConfig) -> Self {
        let mut groups = HashMap::new();

        let mut commands = REPL_COMMANDS.to_vec();
        commands.sort_by(|(a, _), (b, _)| a.cmp(b));

        for (name, _) in REPL_COMMANDS.iter() {
            if let Some(count) = groups.get(name) {
                groups.insert(*name, count + 1);
            } else {
                groups.insert(*name, 1);
            }
        }

        Self {
            config: config.clone(),
            commands,
            groups,
        }
    }
}

fn create_suggestion(value: String, description: Option<String>, span: Span) -> Suggestion {
    Suggestion {
        value,
        description,
        extra: None,
        span,
        append_whitespace: false,
    }
}
