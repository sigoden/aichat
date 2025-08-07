use crate::config::GlobalConfig;

use reedline::{Prompt, PromptHistorySearch, PromptHistorySearchStatus};
use std::borrow::Cow;

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
    fn render_prompt_left(&self) -> Cow<'_, str> {
        Cow::Owned(self.config.read().render_prompt_left())
    }

    fn render_prompt_right(&self) -> Cow<'_, str> {
        Cow::Owned(self.config.read().render_prompt_right())
    }

    fn render_prompt_indicator(&self, _prompt_mode: reedline::PromptEditMode) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        Cow::Borrowed("... ")
    }

    fn render_prompt_history_search_indicator(
        &self,
        history_search: PromptHistorySearch,
    ) -> Cow<'_, str> {
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
