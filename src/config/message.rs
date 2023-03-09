use serde::{Deserialize, Serialize};

pub const MESSAGE_EXTRA_TOKENS: usize = 6;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

impl Message {
    pub fn new(content: &str) -> Self {
        Self {
            role: MessageRole::User,
            content: content.to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    Assistant,
    User,
}

#[test]
fn test_serde() {
    assert_eq!(
        serde_json::to_string(&Message::new("Hello World")).unwrap(),
        "{\"role\":\"user\",\"content\":\"Hello World\"}"
    )
}
