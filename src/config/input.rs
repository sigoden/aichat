use super::{role::Role, session::Session, GlobalConfig};

use crate::client::{
    init_client, list_models, Client, ImageUrl, Message, MessageContent, MessageContentPart, Model,
    ModelCapabilities, SendData,
};
use crate::utils::{base64_encode, sha256};

use anyhow::{bail, Context, Result};
use fancy_regex::Regex;
use lazy_static::lazy_static;
use mime_guess::from_path;
use std::{
    collections::HashMap,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const IMAGE_EXTS: [&str; 5] = ["png", "jpeg", "jpg", "webp", "gif"];

lazy_static! {
    static ref URL_RE: Regex = Regex::new(r"^[A-Za-z0-9_-]{2,}:/").unwrap();
}

#[derive(Debug, Clone)]
pub struct Input {
    config: GlobalConfig,
    text: String,
    medias: Vec<String>,
    data_urls: HashMap<String, String>,
    context: InputContext,
}

impl Input {
    pub fn from_str(config: &GlobalConfig, text: &str, context: Option<InputContext>) -> Self {
        Self {
            config: config.clone(),
            text: text.to_string(),
            medias: Default::default(),
            data_urls: Default::default(),
            context: context.unwrap_or_else(|| InputContext::from_config(config)),
        }
    }

    pub fn new(
        config: &GlobalConfig,
        text: &str,
        files: Vec<String>,
        context: Option<InputContext>,
    ) -> Result<Self> {
        let mut texts = vec![text.to_string()];
        let mut medias = vec![];
        let mut data_urls = HashMap::new();
        let files: Vec<_> = files
            .iter()
            .map(|f| (f, is_image_ext(Path::new(f))))
            .collect();
        let include_filepath = files.iter().filter(|(_, is_image)| !*is_image).count() > 1;
        for (file_item, is_image) in files {
            match resolve_local_file(file_item) {
                Some(file_path) => {
                    if is_image {
                        let data_url = read_media_to_data_url(&file_path)
                            .with_context(|| format!("Unable to read media file '{file_item}'"))?;
                        data_urls.insert(sha256(&data_url), file_path.display().to_string());
                        medias.push(data_url)
                    } else {
                        let text = read_file(&file_path)
                            .with_context(|| format!("Unable to read file '{file_item}'"))?;
                        if include_filepath {
                            texts.push(format!("`{file_item}`:\n~~~~~~\n{text}\n~~~~~~"));
                        } else {
                            texts.push(text);
                        }
                    }
                }
                None => {
                    if is_image {
                        medias.push(file_item.to_string())
                    } else {
                        bail!("Unable to use remote file '{file_item}");
                    }
                }
            }
        }

        Ok(Self {
            config: config.clone(),
            text: texts.join("\n"),
            medias,
            data_urls,
            context: context.unwrap_or_else(|| InputContext::from_config(config)),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.medias.is_empty()
    }

    pub fn data_urls(&self) -> HashMap<String, String> {
        self.data_urls.clone()
    }

    pub fn text(&self) -> String {
        self.text.clone()
    }

    pub fn set_text(&mut self, text: String) {
        self.text = text;
    }

    pub fn model(&self) -> Model {
        let model = self.config.read().model.clone();
        if let Some(model_id) = self.role().and_then(|v| v.model_id.clone()) {
            if model.id() != model_id {
                if let Some(model) = list_models(&self.config.read())
                    .into_iter()
                    .find(|v| v.id() == model_id)
                {
                    return model.clone();
                }
            }
        };
        model
    }

    pub fn create_client(&self) -> Result<Box<dyn Client>> {
        init_client(&self.config, Some(self.model()))
    }

    pub fn prepare_send_data(&self, stream: bool) -> Result<SendData> {
        let messages = self.build_messages()?;
        self.config.read().model.max_input_tokens_limit(&messages)?;
        let (temperature, top_p) = if let Some(session) = self.session(&self.config.read().session)
        {
            (session.temperature(), session.top_p())
        } else if let Some(role) = self.role() {
            (role.temperature, role.top_p)
        } else {
            let config = self.config.read();
            (config.temperature, config.top_p)
        };
        Ok(SendData {
            messages,
            temperature,
            top_p,
            stream,
        })
    }

    pub fn maybe_print_input_tokens(&self) {
        if self.config.read().dry_run {
            if let Ok(messages) = self.build_messages() {
                let tokens = self.config.read().model.total_tokens(&messages);
                println!(">>> This message consumes {tokens} tokens. <<<");
            }
        }
    }

    pub fn build_messages(&self) -> Result<Vec<Message>> {
        let messages = if let Some(session) = self.session(&self.config.read().session) {
            session.build_messages(self)
        } else if let Some(role) = self.role() {
            role.build_messages(self)
        } else {
            let message = Message::new(self);
            vec![message]
        };
        Ok(messages)
    }

    pub fn echo_messages(&self) -> String {
        if let Some(session) = self.session(&self.config.read().session) {
            session.echo_messages(self)
        } else if let Some(role) = self.role() {
            role.echo_messages(self)
        } else {
            self.render()
        }
    }

    pub fn role(&self) -> Option<&Role> {
        self.context.role.as_ref()
    }

    pub fn session<'a>(&self, session: &'a Option<Session>) -> Option<&'a Session> {
        if self.context.session {
            session.as_ref()
        } else {
            None
        }
    }

    pub fn session_mut<'a>(&self, session: &'a mut Option<Session>) -> Option<&'a mut Session> {
        if self.context.session {
            session.as_mut()
        } else {
            None
        }
    }

    pub fn summary(&self) -> String {
        let text: String = self
            .text
            .trim()
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        if text.width_cjk() > 70 {
            let mut sum_width = 0;
            let mut chars = vec![];
            for c in text.chars() {
                sum_width += c.width_cjk().unwrap_or(1);
                if sum_width > 67 {
                    chars.extend(['.', '.', '.']);
                    break;
                }
                chars.push(c);
            }
            chars.into_iter().collect()
        } else {
            text
        }
    }

    pub fn render(&self) -> String {
        if self.medias.is_empty() {
            return self.text.clone();
        }
        let text = if self.text.is_empty() {
            self.text.to_string()
        } else {
            format!(" -- {}", self.text)
        };
        let files: Vec<String> = self
            .medias
            .iter()
            .cloned()
            .map(|url| resolve_data_url(&self.data_urls, url))
            .collect();
        format!(".file {}{}", files.join(" "), text)
    }

    pub fn to_message_content(&self) -> MessageContent {
        if self.medias.is_empty() {
            MessageContent::Text(self.text.clone())
        } else {
            let mut list: Vec<MessageContentPart> = self
                .medias
                .iter()
                .cloned()
                .map(|url| MessageContentPart::ImageUrl {
                    image_url: ImageUrl { url },
                })
                .collect();
            if !self.text.is_empty() {
                list.insert(
                    0,
                    MessageContentPart::Text {
                        text: self.text.clone(),
                    },
                );
            }
            MessageContent::Array(list)
        }
    }

    pub fn required_capabilities(&self) -> ModelCapabilities {
        if !self.medias.is_empty() {
            ModelCapabilities::Vision
        } else {
            ModelCapabilities::Text
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct InputContext {
    role: Option<Role>,
    session: bool,
}

impl InputContext {
    pub fn new(role: Option<Role>, session: bool) -> Self {
        Self { role, session }
    }

    pub fn from_config(config: &GlobalConfig) -> Self {
        let config = config.read();
        InputContext::new(config.role.clone(), config.session.is_some())
    }

    pub fn role(role: Role) -> Self {
        Self {
            role: Some(role),
            session: false,
        }
    }
}

pub fn resolve_data_url(data_urls: &HashMap<String, String>, data_url: String) -> String {
    if data_url.starts_with("data:") {
        let hash = sha256(&data_url);
        if let Some(path) = data_urls.get(&hash) {
            return path.to_string();
        }
        data_url
    } else {
        data_url
    }
}

fn resolve_local_file(file: &str) -> Option<PathBuf> {
    if let Ok(true) = URL_RE.is_match(file) {
        return None;
    }
    let path = if let (Some(file), Some(home)) = (file.strip_prefix("~/"), dirs::home_dir()) {
        home.join(file)
    } else {
        std::env::current_dir().ok()?.join(file)
    };
    Some(path)
}

fn is_image_ext(path: &Path) -> bool {
    path.extension()
        .map(|v| {
            IMAGE_EXTS
                .iter()
                .any(|ext| *ext == v.to_string_lossy().to_lowercase())
        })
        .unwrap_or_default()
}

fn read_media_to_data_url<P: AsRef<Path>>(image_path: P) -> Result<String> {
    let image_path = image_path.as_ref();

    let mime_type = from_path(image_path).first_or_octet_stream().to_string();
    let mut file = File::open(image_path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    let encoded_image = base64_encode(buffer);
    let data_url = format!("data:{};base64,{}", mime_type, encoded_image);

    Ok(data_url)
}

fn read_file<P: AsRef<Path>>(file_path: P) -> Result<String> {
    let file_path = file_path.as_ref();

    let mut text = String::new();
    let mut file = File::open(file_path)?;
    file.read_to_string(&mut text)?;
    Ok(text)
}
