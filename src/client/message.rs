use super::Model;

use crate::{function::ToolResult, multiline_text, utils::dimmed_text};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: MessageContent,
}

impl Default for Message {
    fn default() -> Self {
        Self {
            role: MessageRole::User,
            content: MessageContent::Text(String::new()),
        }
    }
}

impl Message {
    pub fn new(role: MessageRole, content: MessageContent) -> Self {
        Self { role, content }
    }

    pub fn merge_system(&mut self, system: MessageContent) {
        match (&mut self.content, system) {
            (MessageContent::Text(text), MessageContent::Text(system_text)) => {
                self.content = MessageContent::Array(vec![
                    MessageContentPart::Text { text: system_text },
                    MessageContentPart::Text {
                        text: text.to_string(),
                    },
                ])
            }
            (MessageContent::Array(list), MessageContent::Text(system_text)) => {
                list.insert(0, MessageContentPart::Text { text: system_text })
            }
            (MessageContent::Text(text), MessageContent::Array(mut system_list)) => {
                system_list.push(MessageContentPart::Text {
                    text: text.to_string(),
                });
                self.content = MessageContent::Array(system_list);
            }
            (MessageContent::Array(list), MessageContent::Array(mut system_list)) => {
                system_list.append(list);
                self.content = MessageContent::Array(system_list);
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    Assistant,
    User,
    Tool,
}

#[allow(dead_code)]
impl MessageRole {
    pub fn is_system(&self) -> bool {
        matches!(self, MessageRole::System)
    }

    pub fn is_user(&self) -> bool {
        matches!(self, MessageRole::User)
    }

    pub fn is_assistant(&self) -> bool {
        matches!(self, MessageRole::Assistant)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Array(Vec<MessageContentPart>),
    // Note: This type is primarily for convenience and does not exist in OpenAI's API.
    ToolCalls(MessageContentToolCalls),
}

impl MessageContent {
    pub fn render_input(
        &self,
        resolve_url_fn: impl Fn(&str) -> String,
        agent_info: &Option<(String, Vec<String>)>,
    ) -> String {
        match self {
            MessageContent::Text(text) => multiline_text(text),
            MessageContent::Array(list) => {
                let (mut concated_text, mut files) = (String::new(), vec![]);
                for item in list {
                    match item {
                        MessageContentPart::Text { text } => {
                            concated_text = format!("{concated_text} {text}")
                        }
                        MessageContentPart::ImageUrl { image_url } => {
                            files.push(resolve_url_fn(&image_url.url))
                        }
                    }
                }
                if !concated_text.is_empty() {
                    concated_text = format!(" -- {}", multiline_text(&concated_text))
                }
                format!(".file {}{}", files.join(" "), concated_text)
            }
            MessageContent::ToolCalls(MessageContentToolCalls {
                tool_results, text, ..
            }) => {
                let mut lines = vec![];
                if !text.is_empty() {
                    lines.push(text.clone())
                }
                for tool_result in tool_results {
                    let mut parts = vec!["Call".to_string()];
                    if let Some((agent_name, functions)) = agent_info {
                        if functions.contains(&tool_result.call.name) {
                            parts.push(agent_name.clone())
                        }
                    }
                    parts.push(tool_result.call.name.clone());
                    parts.push(tool_result.call.arguments.to_string());
                    lines.push(dimmed_text(&parts.join(" ")));
                }
                lines.join("\n")
            }
        }
    }

    pub fn merge_prompt(&mut self, replace_fn: impl Fn(&str) -> String) {
        match self {
            MessageContent::Text(text) => *text = replace_fn(text),
            MessageContent::Array(list) => {
                if list.is_empty() {
                    list.push(MessageContentPart::Text {
                        text: replace_fn(""),
                    })
                } else if let Some(MessageContentPart::Text { text }) = list.get_mut(0) {
                    *text = replace_fn(text)
                }
            }
            MessageContent::ToolCalls(_) => {}
        }
    }

    pub fn to_text(&self) -> String {
        match self {
            MessageContent::Text(text) => text.to_string(),
            MessageContent::Array(list) => {
                let mut parts = vec![];
                for item in list {
                    if let MessageContentPart::Text { text } = item {
                        parts.push(text.clone())
                    }
                }
                parts.join("\n\n")
            }
            MessageContent::ToolCalls(_) => String::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageContentPart {
    Text { text: String },
    ImageUrl { image_url: ImageUrl },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ImageUrl {
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MessageContentToolCalls {
    pub tool_results: Vec<ToolResult>,
    pub text: String,
    pub sequence: bool,
}

impl MessageContentToolCalls {
    pub fn new(tool_results: Vec<ToolResult>, text: String) -> Self {
        Self {
            tool_results,
            text,
            sequence: false,
        }
    }

    pub fn merge(&mut self, tool_results: Vec<ToolResult>, _text: String) {
        self.tool_results.extend(tool_results);
        self.text.clear();
        self.sequence = true;
    }
}

pub fn patch_messages(messages: &mut Vec<Message>, model: &Model) {
    if messages.is_empty() {
        return;
    }
    if let Some(prefix) = model.system_prompt_prefix() {
        if messages[0].role.is_system() {
            messages[0].merge_system(MessageContent::Text(prefix.to_string()));
        } else {
            messages.insert(
                0,
                Message {
                    role: MessageRole::System,
                    content: MessageContent::Text(prefix.to_string()),
                },
            );
        }
    }
    if model.no_system_message() && messages[0].role.is_system() {
        let system_message = messages.remove(0);
        if let (Some(message), system) = (messages.get_mut(0), system_message.content) {
            message.merge_system(system);
        }
    }
}

pub fn extract_system_message(messages: &mut Vec<Message>) -> Option<String> {
    if messages[0].role.is_system() {
        let system_message = messages.remove(0);
        return Some(system_message.content.to_text());
    }
    None
}
