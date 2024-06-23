use super::*;

use crate::{
    client::{Message, MessageContent, MessageRole, Model},
    function::{FunctionsFilter, SELECTED_ALL_FUNCTIONS},
    utils::{detect_os, detect_shell},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const SHELL_ROLE: &str = "%shell%";
pub const EXPLAIN_SHELL_ROLE: &str = "%explain-shell%";
pub const CODE_ROLE: &str = "%code%";

pub const INPUT_PLACEHOLDER: &str = "__INPUT__";

pub trait RoleLike {
    fn to_role(&self) -> Role;
    fn model(&self) -> &Model;
    fn model_mut(&mut self) -> &mut Model;
    fn temperature(&self) -> Option<f64>;
    fn top_p(&self) -> Option<f64>;
    fn functions_filter(&self) -> Option<FunctionsFilter>;
    fn set_model(&mut self, model: &Model);
    fn set_temperature(&mut self, value: Option<f64>);
    fn set_top_p(&mut self, value: Option<f64>);
    fn set_functions_filter(&mut self, value: Option<FunctionsFilter>);
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
    functions_filter: Option<FunctionsFilter>,

    #[serde(skip)]
    model: Model,
}

impl Role {
    pub fn new(name: &str, prompt: &str) -> Self {
        Self {
            name: name.into(),
            prompt: prompt.into(),
            ..Default::default()
        }
    }

    pub fn builtin() -> Vec<Role> {
        [
            (SHELL_ROLE, shell_prompt(), None),
            (
                EXPLAIN_SHELL_ROLE,
                r#"Provide a terse, single sentence description of the given shell command.
Describe each argument and option of the command.
Provide short responses in about 80 words.
APPLY MARKDOWN formatting when possible."#
                    .into(),
                None,
            ),
            (
                CODE_ROLE,
                r#"Provide only code without comments or explanations.
### INPUT:
async sleep in js
### OUTPUT:
```javascript
async function timeout(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}
```
"#
                .into(),
                None,
            ),
            (
                "%functions%",
                String::new(),
                Some(SELECTED_ALL_FUNCTIONS.into()),
            ),
        ]
        .into_iter()
        .map(|(name, prompt, functions_filter)| Self {
            name: name.into(),
            prompt,
            functions_filter,
            ..Default::default()
        })
        .collect()
    }

    pub fn export(&self) -> Result<String> {
        let output = serde_yaml::to_string(&self)
            .with_context(|| format!("Unable to show info about role {}", &self.name))?;
        Ok(output.trim_end().to_string())
    }

    pub fn sync<T: RoleLike>(&mut self, role_like: &T) {
        let model = role_like.model();
        let temperature = role_like.temperature();
        let top_p = role_like.top_p();
        let functions_filter = role_like.functions_filter();
        self.batch_set(model, temperature, top_p, functions_filter);
    }

    pub fn batch_set(
        &mut self,
        model: &Model,
        temperature: Option<f64>,
        top_p: Option<f64>,
        functions_filter: Option<FunctionsFilter>,
    ) {
        self.set_model(model);
        if temperature.is_some() {
            self.set_temperature(temperature);
        }
        if top_p.is_some() {
            self.set_top_p(top_p);
        }
        if functions_filter.is_some() {
            self.set_functions_filter(functions_filter);
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
        if self.is_empty_prompt() {
            input_markdown
        } else if self.is_embedded_prompt() {
            self.prompt.replace(INPUT_PLACEHOLDER, &input_markdown)
        } else {
            format!("{}\n\n{}", self.prompt, input.render())
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

    fn functions_filter(&self) -> Option<FunctionsFilter> {
        self.functions_filter.clone()
    }

    fn set_model(&mut self, model: &Model) {
        self.model = model.clone();
    }

    fn set_temperature(&mut self, value: Option<f64>) {
        self.temperature = value;
    }

    fn set_top_p(&mut self, value: Option<f64>) {
        self.top_p = value;
    }

    fn set_functions_filter(&mut self, value: Option<FunctionsFilter>) {
        self.functions_filter = value;
    }
}

fn complete_prompt_args(prompt: &str, name: &str) -> String {
    let mut prompt = prompt.trim().to_string();
    for (i, arg) in name.split(':').skip(1).enumerate() {
        prompt = prompt.replace(&format!("__ARG{}__", i + 1), arg);
    }
    prompt
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

fn shell_prompt() -> String {
    let os = detect_os();
    let shell = detect_shell();
    let shell = shell.name.as_str();
    let combinator = if shell == "powershell" {
        "\nIf multiple steps required try to combine them together using ';'.\nIf it already combined with '&&' try to replace it with ';'.".to_string()
    } else {
        "\nIf multiple steps required try to combine them together using '&&'.".to_string()
    };
    format!(
        r#"Provide only {shell} commands for {os} without any description.
Ensure the output is a valid {shell} command. {combinator}
If there is a lack of details, provide most logical solution.
Output plain text only, without any markdown formatting."#
    )
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
