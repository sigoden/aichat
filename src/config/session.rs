use super::message::{Message, MessageRole};
use super::role::Role;
use super::ModelInfo;

use crate::render::MarkdownRender;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs::{self, read_to_string};
use std::path::Path;

pub const TEMP_SESSION_NAME: &str = "temp";

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Session {
    model: String,
    temperature: Option<f64>,
    messages: Vec<Message>,
    #[serde(skip)]
    pub name: String,
    #[serde(skip)]
    pub path: Option<String>,
    #[serde(skip)]
    pub dirty: bool,
    #[serde(skip)]
    pub role: Option<Role>,
    #[serde(skip)]
    pub model_info: ModelInfo,
}

impl Session {
    pub fn new(name: &str, model_info: ModelInfo, role: Option<Role>) -> Self {
        let temperature = role.as_ref().and_then(|v| v.temperature);
        Self {
            model: model_info.full_name(),
            temperature,
            messages: vec![],
            name: name.to_string(),
            path: None,
            dirty: false,
            role,
            model_info,
        }
    }

    pub fn load(name: &str, path: &Path) -> Result<Self> {
        let content = read_to_string(path)
            .with_context(|| format!("Failed to load session {} at {}", name, path.display()))?;
        let mut session: Self =
            serde_yaml::from_str(&content).with_context(|| format!("Invalid session {}", name))?;

        session.name = name.to_string();
        session.path = Some(path.display().to_string());

        Ok(session)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn temperature(&self) -> Option<f64> {
        self.temperature
    }

    pub fn tokens(&self) -> usize {
        self.model_info.totatl_tokens(&self.messages)
    }

    pub fn export(&self) -> Result<String> {
        self.guard_save()?;
        let (tokens, percent) = self.tokens_and_percent();
        let mut data = json!({
            "path": self.path,
            "model": self.model(),
        });
        if let Some(temperature) = self.temperature() {
            data["temperature"] = temperature.into();
        }
        data["total-tokens"] = tokens.into();
        if let Some(max_tokens) = self.model_info.max_tokens {
            data["max-tokens"] = max_tokens.into();
        }
        if percent != 0.0 {
            data["total/max-tokens"] = format!("{}%", percent).into();
        }
        data["messages"] = json!(self.messages);

        let output = serde_yaml::to_string(&data)
            .with_context(|| format!("Unable to show info about session {}", &self.name))?;
        Ok(output)
    }

    pub fn render(&self, render: &mut MarkdownRender) -> Result<String> {
        let path = self.path.clone().unwrap_or_else(|| "-".to_string());

        let temperature = self
            .temperature()
            .map_or_else(|| String::from("-"), |v| v.to_string());

        let max_tokens = self
            .model_info
            .max_tokens
            .map(|v| v.to_string())
            .unwrap_or_else(|| '-'.to_string());

        let items = vec![
            ("path", path),
            ("model", self.model().to_string()),
            ("temperature", temperature),
            ("max_tokens", max_tokens),
        ];
        let mut lines = vec![];
        for (name, value) in items {
            lines.push(format!("{name:<20}{value}"));
        }
        lines.push("".into());
        for message in &self.messages {
            match message.role {
                MessageRole::System => {
                    continue;
                }
                MessageRole::Assistant => {
                    lines.push(render.render(&message.content));
                    lines.push("".into());
                }
                MessageRole::User => {
                    lines.push(format!("{}）{}", self.name, message.content));
                }
            }
        }
        let output = lines.join("\n");
        Ok(output)
    }

    pub fn tokens_and_percent(&self) -> (usize, f32) {
        let tokens = self.tokens();
        let max_tokens = self.model_info.max_tokens.unwrap_or_default();
        let percent = if max_tokens == 0 {
            0.0
        } else {
            let percent = tokens as f32 / max_tokens as f32 * 100.0;
            (percent * 100.0).round() / 100.0
        };
        (tokens, percent)
    }

    pub fn update_role(&mut self, role: Option<Role>) -> Result<()> {
        self.guard_empty()?;
        self.temperature = role.as_ref().and_then(|v| v.temperature);
        self.role = role;
        Ok(())
    }

    pub fn set_temperature(&mut self, value: Option<f64>) {
        self.temperature = value;
    }

    pub fn set_model(&mut self, model_info: ModelInfo) -> Result<()> {
        self.model = model_info.full_name();
        self.model_info = model_info;
        Ok(())
    }

    pub fn save(&mut self, session_path: &Path) -> Result<()> {
        if !self.should_save() {
            return Ok(());
        }
        self.path = Some(session_path.display().to_string());

        let content = serde_yaml::to_string(&self)
            .with_context(|| format!("Failed to serde session {}", self.name))?;
        fs::write(session_path, content).with_context(|| {
            format!(
                "Failed to write session {} to {}",
                self.name,
                session_path.display()
            )
        })?;

        self.dirty = false;

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
