use super::message::*;

pub struct PromptFormat<'a> {
    pub begin: &'a str,
    pub system_pre_message: &'a str,
    pub system_post_message: &'a str,
    pub user_pre_message: &'a str,
    pub user_post_message: &'a str,
    pub assistant_pre_message: &'a str,
    pub assistant_post_message: &'a str,
    pub end: &'a str,
}

pub const GENERIC_PROMPT_FORMAT: PromptFormat<'static> = PromptFormat {
    begin: "",
    system_pre_message: "",
    system_post_message: "\n",
    user_pre_message: "### Instruction:\n",
    user_post_message: "\n",
    assistant_pre_message: "### Response:\n",
    assistant_post_message: "\n",
    end: "### Response:\n",
};

pub const MISTRAL_PROMPT_FORMAT: PromptFormat<'static> = PromptFormat {
    begin: "",
    system_pre_message: "[INST] <<SYS>>",
    system_post_message: "<</SYS>> [/INST]",
    user_pre_message: "[INST]",
    user_post_message: "[/INST]",
    assistant_pre_message: "",
    assistant_post_message: "",
    end: "",
};

pub const LLAMA3_PROMPT_FORMAT: PromptFormat<'static> = PromptFormat {
    begin: "<|begin_of_text|>",
    system_pre_message: "<|start_header_id|>system<|end_header_id|>\n\n",
    system_post_message: "<|eot_id|>",
    user_pre_message: "<|start_header_id|>user<|end_header_id|>\n\n",
    user_post_message: "<|eot_id|>",
    assistant_pre_message: "<|start_header_id|>assistant<|end_header_id|>\n\n",
    assistant_post_message: "<|eot_id|>",
    end: "<|start_header_id|>assistant<|end_header_id|>\n\n",
};

pub const PHI3_PROMPT_FORMAT: PromptFormat<'static> = PromptFormat {
    begin: "",
    system_pre_message: "<|system|>\n",
    system_post_message: "<|end|>\n",
    user_pre_message: "<|user|>\n",
    user_post_message: "<|end|>\n",
    assistant_pre_message: "<|assistant|>\n",
    assistant_post_message: "<|end|>\n",
    end: "<|assistant|>\n",
};

pub const COMMAND_R_PROMPT_FORMAT: PromptFormat<'static> = PromptFormat {
    begin: "",
    system_pre_message: "<|START_OF_TURN_TOKEN|><|SYSTEM_TOKEN|>",
    system_post_message: "<|END_OF_TURN_TOKEN|>",
    user_pre_message: "<|START_OF_TURN_TOKEN|><|USER_TOKEN|>",
    user_post_message: "<|END_OF_TURN_TOKEN|>",
    assistant_pre_message: "<|START_OF_TURN_TOKEN|><|CHATBOT_TOKEN|>",
    assistant_post_message: "<|END_OF_TURN_TOKEN|>",
    end: "<|START_OF_TURN_TOKEN|><|CHATBOT_TOKEN|>",
};

pub const QWEN_PROMPT_FORMAT: PromptFormat<'static> = PromptFormat {
    begin: "",
    system_pre_message: "<|im_start|>system\n",
    system_post_message: "<|im_end|>",
    user_pre_message: "<|im_start|>user\n",
    user_post_message: "<|im_end|>",
    assistant_pre_message: "<|im_start|>assistant\n",
    assistant_post_message: "<|im_end|>",
    end: "<|im_start|>assistant\n",
};

pub fn generate_prompt(messages: &[Message], format: PromptFormat) -> anyhow::Result<String> {
    let PromptFormat {
        begin,
        system_pre_message,
        system_post_message,
        user_pre_message,
        user_post_message,
        assistant_pre_message,
        assistant_post_message,
        end,
    } = format;
    let mut prompt = begin.to_string();
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
            MessageContent::ToolResults(_) => String::new(),
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
    prompt.push_str(end);
    Ok(prompt)
}

pub fn smart_prompt_format(model_name: &str) -> PromptFormat<'static> {
    if model_name.contains("llama3") || model_name.contains("llama-3") {
        LLAMA3_PROMPT_FORMAT
    } else if model_name.contains("llama2")
        || model_name.contains("llama-2")
        || model_name.contains("mistral")
        || model_name.contains("mixtral")
    {
        MISTRAL_PROMPT_FORMAT
    } else if model_name.contains("phi3") || model_name.contains("phi-3") {
        PHI3_PROMPT_FORMAT
    } else if model_name.contains("command-r") {
        COMMAND_R_PROMPT_FORMAT
    } else if model_name.contains("qwen") {
        QWEN_PROMPT_FORMAT
    } else {
        GENERIC_PROMPT_FORMAT
    }
}
