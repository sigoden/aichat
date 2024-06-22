use inquire::{required, validator::Validation, Text};

const MSG_REQUIRED: &str = "This field is required";
const MSG_OPTIONAL: &str = "Optional field - Press ↵ to skip";

pub fn prompt_input_string(desc: &str, required: bool) -> anyhow::Result<String> {
    let mut text = Text::new(desc);
    if required {
        text = text.with_validator(required!(MSG_REQUIRED))
    } else {
        text = text.with_help_message(MSG_OPTIONAL)
    }
    let text = text.prompt()?;
    Ok(text)
}

pub fn prompt_input_integer(desc: &str, required: bool) -> anyhow::Result<String> {
    let mut text = Text::new(desc);
    if required {
        text = text.with_validator(|text: &str| {
            let out = if text.is_empty() {
                Validation::Invalid(MSG_REQUIRED.into())
            } else {
                validate_integer(text)
            };
            Ok(out)
        })
    } else {
        text = text
            .with_validator(|text: &str| {
                let out = if text.is_empty() {
                    Validation::Valid
                } else {
                    validate_integer(text)
                };
                Ok(out)
            })
            .with_help_message(MSG_OPTIONAL)
    }
    let text = text.prompt()?;
    Ok(text)
}

#[derive(Debug, Clone, Copy)]
pub enum PromptKind {
    String,
    Integer,
}

fn validate_integer(text: &str) -> Validation {
    if text.parse::<i32>().is_err() {
        Validation::Invalid("Must be a integer".into())
    } else {
        Validation::Valid
    }
}

#[derive(Debug)]
pub struct SelectOption {
    pub value: String,
    pub description: String,
}

impl SelectOption {
    pub fn new(value: String, description: String) -> Self {
        Self { value, description }
    }
}

impl std::fmt::Display for SelectOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.value, self.description)
    }
}
