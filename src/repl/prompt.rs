use crate::config::GlobalConfig;

use crossterm::style::Color;
use reedline::{Prompt, PromptHistorySearch, PromptHistorySearchStatus};
use std::borrow::Cow;

const PROMPT_COLOR: Color = Color::Green;
const PROMPT_MULTILINE_COLOR: nu_ansi_term::Color = nu_ansi_term::Color::LightBlue;
const INDICATOR_COLOR: Color = Color::Cyan;
const PROMPT_RIGHT_COLOR: Color = Color::AnsiValue(5);

#[derive(Clone)]
pub struct ReplPrompt {
    config: GlobalConfig,
}

impl ReplPrompt {
    pub fn new(config: &GlobalConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }
}

impl Prompt for ReplPrompt {
    fn render_prompt_left(&self) -> Cow<str> {
        if let Some(session) = &self.config.read().session {
            Cow::Owned(session.name().to_string())
        } else if let Some(role) = &self.config.read().role {
            Cow::Owned(role.name.clone())
        } else {
            Cow::Borrowed("")
        }
    }

    fn render_prompt_right(&self) -> Cow<str> {
        Cow::Owned(self.config.read().render_prompt_right())
    }

    fn render_prompt_indicator(&self, _prompt_mode: reedline::PromptEditMode) -> Cow<str> {
        if self.config.read().session.is_some() {
            Cow::Borrowed("）")
        } else {
            Cow::Borrowed("〉")
        }
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<str> {
        Cow::Borrowed("")
    }

    fn render_prompt_history_search_indicator(
        &self,
        history_search: PromptHistorySearch,
    ) -> Cow<str> {
        let prefix = match history_search.status {
            PromptHistorySearchStatus::Passing => "",
            PromptHistorySearchStatus::Failing => "failing ",
        };
        // NOTE: magic strings, given there is logic on how these compose I am not sure if it
        // is worth extracting in to static constant
        Cow::Owned(format!(
            "({}reverse-search: {}) ",
            prefix, history_search.term
        ))
    }

    fn get_prompt_color(&self) -> Color {
        PROMPT_COLOR
    }
    /// Get the default multiline prompt color
    fn get_prompt_multiline_color(&self) -> nu_ansi_term::Color {
        PROMPT_MULTILINE_COLOR
    }
    /// Get the default indicator color
    fn get_indicator_color(&self) -> Color {
        INDICATOR_COLOR
    }
    /// Get the default right prompt color
    fn get_prompt_right_color(&self) -> Color {
        PROMPT_RIGHT_COLOR
    }
}
