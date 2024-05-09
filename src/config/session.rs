use super::input::resolve_data_url;
use super::{Config, Input, Model};

use crate::client::{Message, MessageContent, MessageRole};
use crate::render::MarkdownRender;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fs::{self, read_to_string};
use std::path::Path;

pub const TEMP_SESSION_NAME: &str = "temp";

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Session {
    #[serde(rename(serialize = "model", deserialize = "model"))]
    model_id: String,
    temperature: Option<f64>,
    top_p: Option<f64>,
    #[serde(default)]
    save_session: Option<bool>,
    messages: Vec<Message>,
    #[serde(default)]
    data_urls: HashMap<String, String>,
    #[serde(default)]
    compressed_messages: Vec<Message>,
    compress_threshold: Option<usize>,
    #[serde(skip)]
    pub name: String,
    #[serde(skip)]
    pub path: Option<String>,
    #[serde(skip)]
    pub dirty: bool,
    #[serde(skip)]
    pub compressing: bool,
    #[serde(skip)]
    pub model: Model,
}

impl Session {
    pub fn new(config: &Config, name: &str) -> Self {
        Self {
            model_id: config.model.id(),
            temperature: config.temperature,
            top_p: config.top_p,
            save_session: config.save_session,
            messages: vec![],
            compressed_messages: vec![],
            compress_threshold: None,
            data_urls: Default::default(),
            name: name.to_string(),
            path: None,
            dirty: false,
            compressing: false,
            model: config.model.clone(),
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
        &self.model_id
    }

    pub fn temperature(&self) -> Option<f64> {
        self.temperature
    }

    pub fn top_p(&self) -> Option<f64> {
        self.top_p
    }

    pub fn save_session(&self) -> Option<bool> {
        self.save_session
    }

    pub fn need_compress(&self, current_compress_threshold: usize) -> bool {
        let threshold = self
            .compress_threshold
            .unwrap_or(current_compress_threshold);
        threshold >= 1000 && self.tokens() > threshold
    }

    pub fn tokens(&self) -> usize {
        self.model.total_tokens(&self.messages)
    }

    pub fn user_messages_len(&self) -> usize {
        self.messages.iter().filter(|v| v.role.is_user()).count()
    }

    pub fn export(&self) -> Result<String> {
        if self.path.is_none() {
            bail!("Not found session '{}'", self.name)
        }
        let (tokens, percent) = self.tokens_and_percent();
        let mut data = json!({
            "path": self.path,
            "model": self.model(),
        });
        if let Some(temperature) = self.temperature() {
            data["temperature"] = temperature.into();
        }
        if let Some(top_p) = self.top_p() {
            data["top_p"] = top_p.into();
        }
        if let Some(save_session) = self.save_session() {
            data["save_session"] = save_session.into();
        }
        data["total_tokens"] = tokens.into();
        if let Some(context_window) = self.model.max_input_tokens {
            data["max_input_tokens"] = context_window.into();
        }
        if percent != 0.0 {
            data["total/max"] = format!("{}%", percent).into();
        }
        data["messages"] = json!(self.messages);

        let output = serde_yaml::to_string(&data)
            .with_context(|| format!("Unable to show info about session {}", &self.name))?;
        Ok(output)
    }

    pub fn info(&self, render: &mut MarkdownRender) -> Result<String> {
        let mut items = vec![];

        if let Some(path) = &self.path {
            items.push(("path", path.to_string()));
        }

        items.push(("model", self.model.id()));

        if let Some(temperature) = self.temperature() {
            items.push(("temperature", temperature.to_string()));
        }
        if let Some(top_p) = self.top_p() {
            items.push(("top_p", top_p.to_string()));
        }

        if let Some(save_session) = self.save_session() {
            items.push(("save_session", save_session.to_string()));
        }

        if let Some(compress_threshold) = self.compress_threshold {
            items.push(("compress_threshold", compress_threshold.to_string()));
        }

        if let Some(max_input_tokens) = self.model.max_input_tokens {
            items.push(("max_input_tokens", max_input_tokens.to_string()));
        }

        let mut lines: Vec<String> = items
            .iter()
            .map(|(name, value)| format!("{name:<20}{value}"))
            .collect();

        if !self.is_empty() {
            lines.push("".into());
            let resolve_url_fn = |url: &str| resolve_data_url(&self.data_urls, url.to_string());

            for message in &self.messages {
                match message.role {
                    MessageRole::System => {
                        lines.push(render.render(&message.content.render_input(resolve_url_fn)));
                    }
                    MessageRole::Assistant => {
                        if let MessageContent::Text(text) = &message.content {
                            lines.push(render.render(text));
                        }
                        lines.push("".into());
                    }
                    MessageRole::User => {
                        lines.push(format!(
                            "{}）{}",
                            self.name,
                            message.content.render_input(resolve_url_fn)
                        ));
                    }
                }
            }
        }

        let output = lines.join("\n");
        Ok(output)
    }

    pub fn tokens_and_percent(&self) -> (usize, f32) {
        let tokens = self.tokens();
        let max_input_tokens = self.model.max_input_tokens.unwrap_or_default();
        let percent = if max_input_tokens == 0 {
            0.0
        } else {
            let percent = tokens as f32 / max_input_tokens as f32 * 100.0;
            (percent * 100.0).round() / 100.0
        };
        (tokens, percent)
    }

    pub fn set_temperature(&mut self, value: Option<f64>) {
        if self.temperature != value {
            self.temperature = value;
            self.dirty = true;
        }
    }

    pub fn set_top_p(&mut self, value: Option<f64>) {
        if self.top_p != value {
            self.top_p = value;
            self.dirty = true;
        }
    }

    pub fn set_save_session(&mut self, value: Option<bool>) {
        if self.save_session != value {
            self.save_session = value;
            self.dirty = true;
        }
    }

    pub fn set_compress_threshold(&mut self, value: Option<usize>) {
        if self.compress_threshold != value {
            self.compress_threshold = value;
            self.dirty = true;
        }
    }

    pub fn set_model(&mut self, model: Model) -> Result<()> {
        let model_id = model.id();
        if self.model_id != model_id {
            self.model_id = model_id;
            self.dirty = true;
        }
        self.model = model;
        Ok(())
    }

    pub fn compress(&mut self, prompt: String) {
        self.compressed_messages.append(&mut self.messages);
        self.messages.push(Message {
            role: MessageRole::System,
            content: MessageContent::Text(prompt),
        });
        self.dirty = true;
    }

    pub fn save(&mut self, session_path: &Path) -> Result<()> {
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
        self.messages.is_empty() && self.compressed_messages.is_empty()
    }

    pub fn add_message(&mut self, input: &Input, output: &str) -> Result<()> {
        let mut need_add_msg = true;
        if self.messages.is_empty() {
            if let Some(role) = input.role() {
                self.messages.extend(role.build_messages(input));
                need_add_msg = false;
            }
        }
        if need_add_msg {
            self.messages.push(Message {
                role: MessageRole::User,
                content: input.to_message_content(),
            });
        }
        self.data_urls.extend(input.data_urls());
        self.messages.push(Message {
            role: MessageRole::Assistant,
            content: MessageContent::Text(output.to_string()),
        });
        self.dirty = true;
        Ok(())
    }

    pub fn clear_messages(&mut self) {
        self.messages.clear();
        self.compressed_messages.clear();
        self.data_urls.clear();
        self.dirty = true;
    }

    pub fn echo_messages(&self, input: &Input) -> String {
        let messages = self.build_messages(input);
        serde_yaml::to_string(&messages).unwrap_or_else(|_| "Unable to echo message".into())
    }

    pub fn build_messages(&self, input: &Input) -> Vec<Message> {
        let mut messages = self.messages.clone();
        let mut need_add_msg = true;
        let len = messages.len();
        if len == 0 {
            if let Some(role) = input.role() {
                messages = role.build_messages(input);
                need_add_msg = false;
            }
        } else if len == 1 && self.compressed_messages.len() >= 2 {
            messages
                .extend(self.compressed_messages[self.compressed_messages.len() - 2..].to_vec());
        }
        if need_add_msg {
            messages.push(Message {
                role: MessageRole::User,
                content: input.to_message_content(),
            });
        }
        messages
    }
}
