use super::Input;
use crate::{
    client::{Message, MessageContent, MessageRole},
    utils::{detect_os, detect_shell},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const SHELL_ROLE: &str = "%shell%";
pub const EXPLAIN_ROLE: &str = "%explain%";
pub const CODE_ROLE: &str = "%code%";

pub const INPUT_PLACEHOLDER: &str = "__INPUT__";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Role {
    /// Role name
    pub name: String,
    /// Prompt text
    pub prompt: String,
    /// Temperature value
    pub temperature: Option<f64>,
}

impl Role {
    pub fn find_system_role(name: &str) -> Option<Self> {
        match name {
            SHELL_ROLE => Some(Self::shell()),
            EXPLAIN_ROLE => Some(Self::explain()),
            CODE_ROLE => Some(Self::code()),
            _ => None,
        }
    }

    pub fn shell() -> Self {
        let os = detect_os();
        let (detected_shell, _, _) = detect_shell();
        let (shell, use_semicolon) = match (detected_shell.as_str(), os.as_str()) {
            // GPT doesn’t know much about nushell
            ("nushell", "windows") => ("cmd", true),
            ("nushell", _) => ("bash", true),
            ("powershell", _) => ("powershell", true),
            ("pwsh", _) => ("powershell", false),
            _ => (detected_shell.as_str(), false),
        };
        let combine = if use_semicolon {
            "\nIf multiple steps required try to combine them together using ';'.\nIf it already combined with '&&' try to replace it with ';'.".to_string()
        } else {
            "\nIf multiple steps required try to combine them together using &&.".to_string()
        };
        Self {
            name: SHELL_ROLE.into(),
            prompt: format!(
                r#"Provide only {shell} commands for {os} without any description.
Ensure the output is a valid {shell} command. {combine}
If there is a lack of details, provide most logical solution.
Output plain text only, without any markdown formatting."#
            ),
            temperature: None,
        }
    }

    pub fn explain() -> Self {
        Self {
            name: EXPLAIN_ROLE.into(),
            prompt: r#"Provide a terse, single sentence description of the given shell command.
Describe each argument and option of the command.
Provide short responses in about 80 words.
APPLY MARKDOWN formatting when possible."#
                .into(),
            temperature: None,
        }
    }

    pub fn code() -> Self {
        Self {
            name: CODE_ROLE.into(),
            prompt: r#"Provide only code, without comments or explanations.
If there is a lack of details, provide most logical solution, without requesting further clarification."#
                .into(),
            temperature: None,
        }
    }

    pub fn export(&self) -> Result<String> {
        let output = serde_yaml::to_string(&self)
            .with_context(|| format!("Unable to show info about role {}", &self.name))?;
        Ok(output.trim_end().to_string())
    }

    pub fn embedded(&self) -> bool {
        self.prompt.contains(INPUT_PLACEHOLDER)
    }

    pub fn set_temperature(&mut self, value: Option<f64>) {
        self.temperature = value;
    }

    pub fn complete_prompt_args(&mut self, name: &str) {
        self.name = name.to_string();
        self.prompt = complete_prompt_args(&self.prompt, &self.name);
    }

    pub fn match_name(&self, name: &str) -> bool {
        if self.name.contains(':') {
            let role_name_parts: Vec<&str> = self.name.split(':').collect();
            let name_parts: Vec<&str> = name.split(':').collect();
            role_name_parts[0] == name_parts[0] && role_name_parts.len() == name_parts.len()
        } else {
            self.name == name
        }
    }

    pub fn echo_messages(&self, input: &Input) -> String {
        let input_markdown = input.render();
        if self.embedded() {
            self.prompt.replace(INPUT_PLACEHOLDER, &input_markdown)
        } else {
            format!("{}\n\n{}", self.prompt, input.render())
        }
    }

    pub fn build_messages(&self, input: &Input) -> Vec<Message> {
        let mut content = input.to_message_content();

        if self.embedded() {
            content.merge_prompt(|v: &str| self.prompt.replace(INPUT_PLACEHOLDER, v));
            vec![Message {
                role: MessageRole::User,
                content,
            }]
        } else {
            vec![
                Message {
                    role: MessageRole::System,
                    content: MessageContent::Text(self.prompt.clone()),
                },
                Message {
                    role: MessageRole::User,
                    content,
                },
            ]
        }
    }
}

fn complete_prompt_args(prompt: &str, name: &str) -> String {
    let mut prompt = prompt.trim().to_string();
    for (i, arg) in name.split(':').skip(1).enumerate() {
        prompt = prompt.replace(&format!("__ARG{}__", i + 1), arg);
    }
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_prompt_name() {
        assert_eq!(
            complete_prompt_args("convert __ARG1__", "convert:foo"),
            "convert foo"
        );
        assert_eq!(
            complete_prompt_args("convert __ARG1__ to __ARG2__", "convert:foo:bar"),
            "convert foo to bar"
        );
    }
}
