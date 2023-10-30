use crate::config::SharedConfig;

use crossterm::style::Color;
use reedline::{Prompt, PromptHistorySearch, PromptHistorySearchStatus};
use std::borrow::Cow;

const PROMPT_COLOR: Color = Color::Green;
const PROMPT_MULTILINE_COLOR: nu_ansi_term::Color = nu_ansi_term::Color::LightBlue;
const INDICATOR_COLOR: Color = Color::Cyan;
const PROMPT_RIGHT_COLOR: Color = Color::AnsiValue(5);

#[derive(Clone)]
pub struct ReplPrompt {
    config: SharedConfig,
    prompt_color: Color,
    prompt_multiline_color: nu_ansi_term::Color,
    indicator_color: Color,
    prompt_right_color: Color,
}

impl ReplPrompt {
    pub fn new(config: SharedConfig) -> Self {
        let (prompt_color, prompt_multiline_color, indicator_color, prompt_right_color) =
            Self::get_colors(&config);
        Self {
            config,
            prompt_color,
            prompt_multiline_color,
            indicator_color,
            prompt_right_color,
        }
    }
    pub fn sync_config(&mut self) {
        let (prompt_color, prompt_multiline_color, indicator_color, prompt_right_color) =
            Self::get_colors(&self.config);
        self.prompt_color = prompt_color;
        self.prompt_multiline_color = prompt_multiline_color;
        self.indicator_color = indicator_color;
        self.prompt_right_color = prompt_right_color;
    }

    pub fn get_colors(config: &SharedConfig) -> (Color, nu_ansi_term::Color, Color, Color) {
        let render_options = config.read().get_render_options();
        if render_options.highlight {
            (
                PROMPT_COLOR,
                PROMPT_MULTILINE_COLOR,
                INDICATOR_COLOR,
                PROMPT_RIGHT_COLOR,
            )
        } else if render_options.light_theme {
            (
                Color::Black,
                nu_ansi_term::Color::Black,
                Color::Black,
                Color::Black,
            )
        } else {
            (
                Color::White,
                nu_ansi_term::Color::White,
                Color::White,
                Color::White,
            )
        }
    }
}

impl Prompt for ReplPrompt {
    fn render_prompt_left(&self) -> Cow<str> {
        if let Some(session) = &self.config.read().session {
            Cow::Owned(session.name.clone())
        } else if let Some(role) = &self.config.read().role {
            Cow::Owned(role.name.clone())
        } else {
            Cow::Borrowed("")
        }
    }

    fn render_prompt_right(&self) -> Cow<str> {
        if self.config.read().session.is_none() {
            Cow::Borrowed("")
        } else {
            self.config.read().get_reamind_tokens().to_string().into()
        }
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
        self.prompt_color
    }
    /// Get the default multiline prompt color
    fn get_prompt_multiline_color(&self) -> nu_ansi_term::Color {
        self.prompt_multiline_color
    }
    /// Get the default indicator color
    fn get_indicator_color(&self) -> Color {
        self.indicator_color
    }
    /// Get the default right prompt color
    fn get_prompt_right_color(&self) -> Color {
        self.prompt_right_color
    }
}
