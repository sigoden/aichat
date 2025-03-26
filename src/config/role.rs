use super::*;

use crate::client::{Message, MessageContent, MessageRole, Model};

use anyhow::Result;
use fancy_regex::Regex;
use rust_embed::Embed;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::LazyLock;

pub const SHELL_ROLE: &str = "%shell%";
pub const EXPLAIN_SHELL_ROLE: &str = "%explain-shell%";
pub const CODE_ROLE: &str = "%code%";
pub const CREATE_TITLE_ROLE: &str = "%create-title%";

pub const INPUT_PLACEHOLDER: &str = "__INPUT__";

#[derive(Embed)]
#[folder = "assets/roles/"]
struct RolesAsset;

static RE_METADATA: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)-{3,}\s*(.*?)\s*-{3,}\s*(.*)").unwrap());

pub trait RoleLike {
    fn to_role(&self) -> Role;
    fn model(&self) -> &Model;
    fn model_mut(&mut self) -> &mut Model;
    fn temperature(&self) -> Option<f64>;
    fn top_p(&self) -> Option<f64>;
    fn use_tools(&self) -> Option<String>;
    fn set_model(&mut self, model: &Model);
    fn set_temperature(&mut self, value: Option<f64>);
    fn set_top_p(&mut self, value: Option<f64>);
    fn set_use_tools(&mut self, value: Option<String>);
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Role {
    name: String,
    #[serde(default)]
    prompt: String,
    #[serde(
        rename(serialize = "model", deserialize = "model"),
        skip_serializing_if = "Option::is_none"
    )]
    model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    use_tools: Option<String>,

    #[serde(skip)]
    model: Model,
}

impl Role {
    pub fn new(name: &str, content: &str) -> Self {
        let mut metadata = "";
        let mut prompt = content.trim();
        if let Ok(Some(caps)) = RE_METADATA.captures(content) {
            if let (Some(metadata_value), Some(prompt_value)) = (caps.get(1), caps.get(2)) {
                metadata = metadata_value.as_str().trim();
                prompt = prompt_value.as_str().trim();
            }
        }
        let mut prompt = prompt.to_string();
        interpolate_variables(&mut prompt);
        let mut role = Self {
            name: name.to_string(),
            prompt,
            ..Default::default()
        };
        if !metadata.is_empty() {
            if let Ok(value) = serde_yaml::from_str::<Value>(metadata) {
                if let Some(value) = value.as_object() {
                    for (key, value) in value {
                        match key.as_str() {
                            "model" => role.model_id = value.as_str().map(|v| v.to_string()),
                            "temperature" => role.temperature = value.as_f64(),
                            "top_p" => role.top_p = value.as_f64(),
                            "use_tools" => role.use_tools = value.as_str().map(|v| v.to_string()),
                            _ => (),
                        }
                    }
                }
            }
        }
        role
    }

    pub fn builtin(name: &str) -> Result<Self> {
        let content = RolesAsset::get(&format!("{name}.md"))
            .ok_or_else(|| anyhow!("Unknown role `{name}`"))?;
        let content = unsafe { std::str::from_utf8_unchecked(&content.data) };
        Ok(Role::new(name, content))
    }

    pub fn list_builtin_role_names() -> Vec<String> {
        RolesAsset::iter()
            .filter_map(|v| v.strip_suffix(".md").map(|v| v.to_string()))
            .collect()
    }

    pub fn list_builtin_roles() -> Vec<Self> {
        RolesAsset::iter()
            .filter_map(|v| Role::builtin(&v).ok())
            .collect()
    }

    pub fn has_args(&self) -> bool {
        self.name.contains('#')
    }

    pub fn export(&self) -> String {
        let mut metadata = vec![];
        if let Some(model) = self.model_id() {
            metadata.push(format!("model: {}", model));
        }
        if let Some(temperature) = self.temperature() {
            metadata.push(format!("temperature: {}", temperature));
        }
        if let Some(top_p) = self.top_p() {
            metadata.push(format!("top_p: {}", top_p));
        }
        if let Some(use_tools) = self.use_tools() {
            metadata.push(format!("use_tools: {}", use_tools));
        }
        if metadata.is_empty() {
            format!("{}\n", self.prompt)
        } else if self.prompt.is_empty() {
            format!("---\n{}\n---\n", metadata.join("\n"))
        } else {
            format!("---\n{}\n---\n\n{}\n", metadata.join("\n"), self.prompt)
        }
    }

    pub fn save(&mut self, role_name: &str, role_path: &Path, is_repl: bool) -> Result<()> {
        ensure_parent_exists(role_path)?;

        let content = self.export();
        std::fs::write(role_path, content).with_context(|| {
            format!(
                "Failed to write role {} to {}",
                self.name,
                role_path.display()
            )
        })?;

        if is_repl {
            println!("✓ Saved role to '{}'.", role_path.display());
        }

        if role_name != self.name {
            self.name = role_name.to_string();
        }

        Ok(())
    }

    pub fn sync<T: RoleLike>(&mut self, role_like: &T) {
        let model = role_like.model();
        let temperature = role_like.temperature();
        let top_p = role_like.top_p();
        let use_tools = role_like.use_tools();
        self.batch_set(model, temperature, top_p, use_tools);
    }

    pub fn batch_set(
        &mut self,
        model: &Model,
        temperature: Option<f64>,
        top_p: Option<f64>,
        use_tools: Option<String>,
    ) {
        self.set_model(model);
        if temperature.is_some() {
            self.set_temperature(temperature);
        }
        if top_p.is_some() {
            self.set_top_p(top_p);
        }
        if use_tools.is_some() {
            self.set_use_tools(use_tools);
        }
    }

    pub fn is_derived(&self) -> bool {
        self.name.is_empty()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn model_id(&self) -> Option<&str> {
        self.model_id.as_deref()
    }

    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    pub fn is_empty_prompt(&self) -> bool {
        self.prompt.is_empty()
    }

    pub fn is_embedded_prompt(&self) -> bool {
        self.prompt.contains(INPUT_PLACEHOLDER)
    }

    pub fn echo_messages(&self, input: &Input) -> String {
        let input_markdown = input.render();
        if self.is_empty_prompt() {
            input_markdown
        } else if self.is_embedded_prompt() {
            self.prompt.replace(INPUT_PLACEHOLDER, &input_markdown)
        } else {
            format!("{}\n\n{}", self.prompt, input_markdown)
        }
    }

    pub fn build_messages(&self, input: &Input) -> Vec<Message> {
        let mut content = input.message_content();
        let mut messages = if self.is_empty_prompt() {
            vec![Message::new(MessageRole::User, content)]
        } else if self.is_embedded_prompt() {
            content.merge_prompt(|v: &str| self.prompt.replace(INPUT_PLACEHOLDER, v));
            vec![Message::new(MessageRole::User, content)]
        } else {
            let mut messages = vec![];
            let (system, cases) = parse_structure_prompt(&self.prompt);
            if !system.is_empty() {
                messages.push(Message::new(
                    MessageRole::System,
                    MessageContent::Text(system.to_string()),
                ));
            }
            if !cases.is_empty() {
                messages.extend(cases.into_iter().flat_map(|(i, o)| {
                    vec![
                        Message::new(MessageRole::User, MessageContent::Text(i.to_string())),
                        Message::new(MessageRole::Assistant, MessageContent::Text(o.to_string())),
                    ]
                }));
            }
            messages.push(Message::new(MessageRole::User, content));
            messages
        };
        if let Some(text) = input.continue_output() {
            messages.push(Message::new(
                MessageRole::Assistant,
                MessageContent::Text(text.into()),
            ));
        }
        messages
    }
}

impl RoleLike for Role {
    fn to_role(&self) -> Role {
        self.clone()
    }

    fn model(&self) -> &Model {
        &self.model
    }

    fn model_mut(&mut self) -> &mut Model {
        &mut self.model
    }

    fn temperature(&self) -> Option<f64> {
        self.temperature
    }

    fn top_p(&self) -> Option<f64> {
        self.top_p
    }

    fn use_tools(&self) -> Option<String> {
        self.use_tools.clone()
    }

    fn set_model(&mut self, model: &Model) {
        if !self.model().id().is_empty() {
            self.model_id = Some(model.id().to_string());
        }
        self.model = model.clone();
    }

    fn set_temperature(&mut self, value: Option<f64>) {
        self.temperature = value;
    }

    fn set_top_p(&mut self, value: Option<f64>) {
        self.top_p = value;
    }

    fn set_use_tools(&mut self, value: Option<String>) {
        self.use_tools = value;
    }
}

fn parse_structure_prompt(prompt: &str) -> (&str, Vec<(&str, &str)>) {
    let mut text = prompt;
    let mut search_input = true;
    let mut system = None;
    let mut parts = vec![];
    loop {
        let search = if search_input {
            "### INPUT:"
        } else {
            "### OUTPUT:"
        };
        match text.find(search) {
            Some(idx) => {
                if system.is_none() {
                    system = Some(&text[..idx])
                } else {
                    parts.push(&text[..idx])
                }
                search_input = !search_input;
                text = &text[(idx + search.len())..];
            }
            None => {
                if !text.is_empty() {
                    if system.is_none() {
                        system = Some(text)
                    } else {
                        parts.push(text)
                    }
                }
                break;
            }
        }
    }
    let parts_len = parts.len();
    if parts_len > 0 && parts_len % 2 == 0 {
        let cases: Vec<(&str, &str)> = parts
            .iter()
            .step_by(2)
            .zip(parts.iter().skip(1).step_by(2))
            .map(|(i, o)| (i.trim(), o.trim()))
            .collect();
        let system = system.map(|v| v.trim()).unwrap_or_default();
        return (system, cases);
    }

    (prompt, vec![])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_structure_prompt1() {
        let prompt = r#"
System message
### INPUT:
Input 1
### OUTPUT:
Output 1
"#;
        assert_eq!(
            parse_structure_prompt(prompt),
            ("System message", vec![("Input 1", "Output 1")])
        );
    }

    #[test]
    fn test_parse_structure_prompt2() {
        let prompt = r#"
### INPUT:
Input 1
### OUTPUT:
Output 1
"#;
        assert_eq!(
            parse_structure_prompt(prompt),
            ("", vec![("Input 1", "Output 1")])
        );
    }

    #[test]
    fn test_parse_structure_prompt3() {
        let prompt = r#"
System message
### INPUT:
Input 1
"#;
        assert_eq!(parse_structure_prompt(prompt), (prompt, vec![]));
    }
}
