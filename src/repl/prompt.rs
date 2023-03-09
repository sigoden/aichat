use crate::config::SharedConfig;

use reedline::{Prompt, PromptHistorySearch, PromptHistorySearchStatus};
use std::borrow::Cow;

const DEFAULT_MULTILINE_INDICATOR: &str = "::: ";

#[derive(Clone)]
pub struct ReplPrompt(SharedConfig);

impl ReplPrompt {
    pub fn new(config: SharedConfig) -> Self {
        Self(config)
    }
}

impl Prompt for ReplPrompt {
    fn render_prompt_left(&self) -> Cow<str> {
        let config = self.0.lock();
        if let Some(role) = config.role.as_ref() {
            role.name.to_string().into()
        } else {
            Cow::Borrowed("")
        }
    }

    fn render_prompt_right(&self) -> Cow<str> {
        let config = self.0.lock();
        if let Some(conversation) = config.conversation.as_ref() {
            conversation.reamind_tokens().to_string().into()
        } else {
            Cow::Borrowed("")
        }
    }

    fn render_prompt_indicator(&self, _prompt_mode: reedline::PromptEditMode) -> Cow<str> {
        let config = self.0.lock();
        if config.conversation.is_some() {
            Cow::Borrowed("＄")
        } else {
            Cow::Borrowed("〉")
        }
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<str> {
        Cow::Borrowed(DEFAULT_MULTILINE_INDICATOR)
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
}
