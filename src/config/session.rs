use super::input::resolve_data_url;
use super::{Config, Input, Model, Role};

use crate::client::{Message, MessageContent, MessageRole};
use crate::render::MarkdownRender;

use anyhow::{bail, Context, Result};
use inquire::{Confirm, Text};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fs::{self, create_dir_all, read_to_string};
use std::path::Path;

pub const TEMP_SESSION_NAME: &str = "temp";

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Session {
    #[serde(rename(serialize = "model", deserialize = "model"))]
    model_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    function_matcher: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    save_session: Option<bool>,
    messages: Vec<Message>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    data_urls: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    compressed_messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
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
        let name = if name.is_empty() {
            TEMP_SESSION_NAME
        } else {
            name
        };
        let save_session = if name == TEMP_SESSION_NAME {
            None
        } else {
            config.save_session
        };
        let mut session = Self {
            model_id: config.model.id(),
            temperature: config.temperature,
            top_p: config.top_p,
            function_matcher: None,
            save_session,
            messages: Default::default(),
            compressed_messages: Default::default(),
            compress_threshold: None,
            data_urls: Default::default(),
            name: name.to_string(),
            path: None,
            dirty: false,
            compressing: false,
            model: config.model.clone(),
        };
        if let Some(role) = &config.role {
            session.set_role_properties(role);
        }
        session
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

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn temperature(&self) -> Option<f64> {
        self.temperature
    }

    pub fn top_p(&self) -> Option<f64> {
        self.top_p
    }

    pub fn function_matcher(&self) -> Option<&str> {
        self.function_matcher.as_deref()
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
            "model": self.model_id(),
        });
        if let Some(temperature) = self.temperature() {
            data["temperature"] = temperature.into();
        }
        if let Some(top_p) = self.top_p() {
            data["top_p"] = top_p.into();
        }
        if let Some(function_matcher) = self.function_matcher() {
            data["function_matcher"] = function_matcher.into();
        }
        if let Some(save_session) = self.save_session() {
            data["save_session"] = save_session.into();
        }
        data["total_tokens"] = tokens.into();
        if let Some(max_input_tokens) = self.model.max_input_tokens() {
            data["max_input_tokens"] = max_input_tokens.into();
        }
        if percent != 0.0 {
            data["total/max"] = format!("{}%", percent).into();
        }
        data["messages"] = json!(self.messages);

        let output = serde_yaml::to_string(&data)
            .with_context(|| format!("Unable to show info about session '{}'", &self.name))?;
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

        if let Some(function_matcher) = self.function_matcher() {
            items.push(("function_matcher", function_matcher.into()));
        }

        if let Some(save_session) = self.save_session() {
            items.push(("save_session", save_session.to_string()));
        }

        if let Some(compress_threshold) = self.compress_threshold {
            items.push(("compress_threshold", compress_threshold.to_string()));
        }

        if let Some(max_input_tokens) = self.model.max_input_tokens() {
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
        let max_input_tokens = self.model.max_input_tokens().unwrap_or_default();
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

    pub fn set_function_matcher(&mut self, function_matcher: Option<&str>) {
        self.function_matcher = function_matcher.map(|v| v.to_string());
    }

    pub fn set_role_properties(&mut self, role: &Role) {
        self.set_temperature(role.temperature);
        self.set_top_p(role.top_p);
        self.set_function_matcher(role.function_matcher.as_deref());
    }

    pub fn set_save_session(&mut self, value: Option<bool>) {
        if self.name == TEMP_SESSION_NAME {
            return;
        }
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

    pub fn set_model(&mut self, model: &Model) {
        let model_id = model.id();
        if self.model_id != model_id {
            self.model_id = model_id;
            self.dirty = true;
        }
        self.model = model.clone();
    }

    pub fn compress(&mut self, prompt: String) {
        self.compressed_messages.append(&mut self.messages);
        self.messages.push(Message::new(
            MessageRole::System,
            MessageContent::Text(prompt),
        ));
        self.dirty = true;
    }

    pub fn exit(&mut self, sessions_dir: &Path, is_repl: bool) -> Result<()> {
        let save_session = self.save_session();
        if self.dirty && save_session != Some(false) {
            if save_session.is_none() {
                if !is_repl {
                    return Ok(());
                }
                let ans = Confirm::new("Save session?").with_default(false).prompt()?;
                if !ans {
                    return Ok(());
                }
                while self.is_temp() {
                    self.name = Text::new("Session name:").prompt()?;
                }
            }
            self.save(sessions_dir)?;
        }
        Ok(())
    }

    pub fn save(&mut self, sessions_dir: &Path) -> Result<()> {
        let mut session_path = sessions_dir.to_path_buf();
        session_path.push(format!("{}.yaml", self.name()));
        if !sessions_dir.exists() {
            create_dir_all(sessions_dir).with_context(|| {
                format!("Failed to create session_dir '{}'", sessions_dir.display())
            })?;
        }

        self.path = Some(session_path.display().to_string());

        let content = serde_yaml::to_string(&self)
            .with_context(|| format!("Failed to serde session {}", self.name))?;
        fs::write(&session_path, content).with_context(|| {
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
            self.messages
                .push(Message::new(MessageRole::User, input.message_content()));
        }
        self.data_urls.extend(input.data_urls());
        self.messages.push(Message::new(
            MessageRole::Assistant,
            MessageContent::Text(output.to_string()),
        ));
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
            messages.push(Message::new(MessageRole::User, input.message_content()));
        }
        messages
    }
}
