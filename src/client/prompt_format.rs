use super::message::*;

pub struct PromptFormat<'a> {
    pub bos_token: &'a str,
    pub system_pre_message: &'a str,
    pub system_post_message: &'a str,
    pub user_pre_message: &'a str,
    pub user_post_message: &'a str,
    pub assistant_pre_message: &'a str,
    pub assistant_post_message: &'a str,
}

pub const LLAMA2_PROMPT_FORMAT: PromptFormat<'static> = PromptFormat {
    bos_token: "<s>",
    system_pre_message: "[INST] <<SYS>>",
    system_post_message: "<</SYS>> [/INST]",
    user_pre_message: "[INST]",
    user_post_message: "[/INST]",
    assistant_pre_message: "",
    assistant_post_message: "</s>",
};

pub const LLAMA3_PROMPT_FORMAT: PromptFormat<'static> = PromptFormat {
    bos_token: "<|begin_of_text|>",
    system_pre_message: "<|start_header_id|>system<|end_header_id|>\n\n",
    system_post_message: "<|eot_id|>",
    user_pre_message: "<|start_header_id|>user<|end_header_id|>\n\n",
    user_post_message: "<|eot_id|>",
    assistant_pre_message: "<|start_header_id|>assistant<|end_header_id|>\n\n",
    assistant_post_message: "<|eot_id|>",
};

pub fn generate_prompt(messages: &[Message], format: PromptFormat) -> anyhow::Result<String> {
    let PromptFormat {
        bos_token,
        system_pre_message,
        system_post_message,
        user_pre_message,
        user_post_message,
        assistant_pre_message,
        assistant_post_message,
    } = format;
    let mut prompt = bos_token.to_string();
    let mut image_urls = vec![];
    for message in messages {
        let role = &message.role;
        let content = match &message.content {
            MessageContent::Text(text) => text.clone(),
            MessageContent::Array(list) => {
                let mut parts = vec![];
                for item in list {
                    match item {
                        MessageContentPart::Text { text } => parts.push(text.clone()),
                        MessageContentPart::ImageUrl {
                            image_url: ImageUrl { url },
                        } => {
                            image_urls.push(url.clone());
                        }
                    }
                }
                parts.join("\n\n")
            }
        };
        match role {
            MessageRole::System => prompt.push_str(&format!(
                "{system_pre_message}{content}{system_post_message}"
            )),
            MessageRole::Assistant => prompt.push_str(&format!(
                "{assistant_pre_message}{content}{assistant_post_message}"
            )),
            MessageRole::User => {
                prompt.push_str(&format!("{user_pre_message}{content}{user_post_message}"))
            }
        }
    }
    if !image_urls.is_empty() {
        anyhow::bail!("The model does not support images: {:?}", image_urls);
    }
    prompt.push_str(assistant_pre_message);
    Ok(prompt)
}
