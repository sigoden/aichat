use super::ToolResults;

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

    pub fn merge_system(&mut self, system: &str) {
        match &mut self.content {
            MessageContent::Text(text) => {
                self.content = MessageContent::Array(vec![
                    MessageContentPart::Text {
                        text: system.to_string(),
                    },
                    MessageContentPart::Text {
                        text: text.to_string(),
                    },
                ]);
            }
            MessageContent::Array(list) => {
                list.insert(
                    0,
                    MessageContentPart::Text {
                        text: system.to_string(),
                    },
                );
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
}

#[allow(dead_code)]
impl MessageRole {
    pub fn is_system(&self) -> bool {
        matches!(self, MessageRole::System)
    }

    pub fn is_user(&self) -> bool {
        matches!(self, MessageRole::User)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Array(Vec<MessageContentPart>),
    // Note: This type is primarily for convenience and does not exist in OpenAI's API.
    ToolResults(ToolResults),
}

impl MessageContent {
    pub fn render_input(&self, resolve_url_fn: impl Fn(&str) -> String) -> String {
        match self {
            MessageContent::Text(text) => text.to_string(),
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
                    concated_text = format!(" -- {concated_text}")
                }
                format!(".file {}{}", files.join(" "), concated_text)
            }
            MessageContent::ToolResults(_) => String::new(),
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
            MessageContent::ToolResults(_) => {}
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
            MessageContent::ToolResults(_) => String::new(),
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

pub fn patch_system_message(messages: &mut Vec<Message>) {
    if messages[0].role.is_system() {
        let system_message = messages.remove(0);
        if let (Some(message), MessageContent::Text(system)) =
            (messages.get_mut(0), system_message.content)
        {
            message.merge_system(&system);
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
