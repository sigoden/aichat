use super::message::{num_tokens_from_messages, Message, MessageRole};
use super::role::Role;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::{self, read_to_string};
use std::path::Path;

pub const TEMP_SESSION_NAME: &str = "temp";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Session {
    pub path: Option<String>,
    pub model: String,
    pub tokens: usize,
    pub temperature: Option<f64>,
    pub messages: Vec<Message>,
    #[serde(skip)]
    pub dirty: bool,
    #[serde(skip)]
    pub role: Option<Role>,
    #[serde(skip)]
    pub name: String,
}

impl Session {
    pub fn new(name: &str, model: &str, role: Option<Role>) -> Self {
        let temperature = role.as_ref().and_then(|v| v.temperature);
        let mut value = Self {
            path: None,
            model: model.to_string(),
            temperature,
            tokens: 0,
            messages: vec![],
            dirty: false,
            role,
            name: name.to_string(),
        };
        value.update_tokens();
        value
    }

    pub fn load(name: &str, path: &Path) -> Result<Self> {
        let content = read_to_string(path)
            .with_context(|| format!("Failed to load session {} at {}", name, path.display()))?;
        let mut session: Self =
            serde_yaml::from_str(&content).with_context(|| format!("Invalid sesion {}", name))?;

        session.name = name.to_string();
        session.path = Some(path.display().to_string());

        Ok(session)
    }

    pub fn info(&self) -> Result<String> {
        self.guard_save()?;
        let output = serde_yaml::to_string(&self)
            .with_context(|| format!("Unable to show info about session {}", &self.name))?;
        Ok(output)
    }

    pub fn update_role(&mut self, role: Option<Role>) -> Result<()> {
        self.guard_empty()?;
        self.temperature = role.as_ref().and_then(|v| v.temperature);
        self.role = role;
        self.update_tokens();
        Ok(())
    }

    pub fn set_model(&mut self, model: &str) -> Result<()> {
        self.model = model.to_string();
        self.update_tokens();
        Ok(())
    }

    pub fn save(&mut self, session_path: &Path) -> Result<()> {
        if !self.should_save() {
            return Ok(());
        }
        self.dirty = false;
        let content = serde_yaml::to_string(&self)
            .with_context(|| format!("Failed to serde session {}", self.name))?;
        fs::write(session_path, content).with_context(|| {
            format!(
                "Failed to write session {} to {}",
                self.name,
                session_path.display()
            )
        })?;
        Ok(())
    }

    pub fn should_save(&self) -> bool {
        !self.is_empty() && self.dirty
    }

    pub fn guard_save(&self) -> Result<()> {
        if self.path.is_none() {
            bail!("Not found session '{}'", self.name)
        }
        Ok(())
    }

    pub fn guard_empty(&self) -> Result<()> {
        if !self.is_empty() {
            bail!("Cannot perform this action in a session with messages")
        }
        Ok(())
    }

    pub fn is_temp(&self) -> bool {
        self.name == TEMP_SESSION_NAME
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    pub fn update_tokens(&mut self) {
        self.tokens = num_tokens_from_messages(&self.build_emssages(""));
    }

    pub fn add_message(&mut self, input: &str, output: &str) -> Result<()> {
        let mut need_add_msg = true;
        if self.messages.is_empty() {
            if let Some(role) = self.role.as_ref() {
                self.messages.extend(role.build_messages(input));
                need_add_msg = false;
            }
        }
        if need_add_msg {
            self.messages.push(Message {
                role: MessageRole::User,
                content: input.to_string(),
            });
        }
        self.messages.push(Message {
            role: MessageRole::Assistant,
            content: output.to_string(),
        });
        self.tokens = num_tokens_from_messages(&self.messages);
        self.dirty = true;
        Ok(())
    }

    pub fn echo_messages(&self, content: &str) -> String {
        let messages = self.build_emssages(content);
        serde_yaml::to_string(&messages).unwrap_or_else(|_| "Unable to echo message".into())
    }

    pub fn build_emssages(&self, content: &str) -> Vec<Message> {
        let mut messages = self.messages.clone();
        let mut need_add_msg = true;
        if messages.is_empty() {
            if let Some(role) = self.role.as_ref() {
                messages = role.build_messages(content);
                need_add_msg = false;
            }
        };
        if need_add_msg {
            messages.push(Message {
                role: MessageRole::User,
                content: content.into(),
            });
        }
        messages
    }
}
