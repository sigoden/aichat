use super::message::{Message, MessageRole};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const INPUT_PLACEHOLDER: &str = "__INPUT__";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Role {
    /// Role name
    pub name: String,
    /// Prompt text
    pub prompt: String,
    /// What sampling temperature to use, between 0 and 2
    pub temperature: Option<f64>,
}

impl Role {
    pub fn info(&self) -> Result<String> {
        let output = serde_yaml::to_string(&self)
            .with_context(|| format!("Unable to show info about role {}", &self.name))?;
        Ok(output)
    }

    pub fn embeded(&self) -> bool {
        self.prompt.contains(INPUT_PLACEHOLDER)
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

    pub fn echo_messages(&self, content: &str) -> String {
        if self.embeded() {
            merge_prompt_content(&self.prompt, content)
        } else {
            format!("{}\n{content}", self.prompt)
        }
    }

    pub fn build_messages(&self, content: &str) -> Vec<Message> {
        if self.embeded() {
            let content = merge_prompt_content(&self.prompt, content);
            vec![Message {
                role: MessageRole::User,
                content,
            }]
        } else {
            vec![
                Message {
                    role: MessageRole::System,
                    content: self.prompt.clone(),
                },
                Message {
                    role: MessageRole::User,
                    content: content.to_string(),
                },
            ]
        }
    }
}

fn merge_prompt_content(prompt: &str, content: &str) -> String {
    prompt.replace(INPUT_PLACEHOLDER, content)
}

fn complete_prompt_args(prompt: &str, name: &str) -> String {
    let mut prompt = prompt.to_string();
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
