use super::*;

use crate::client::{
    init_client, ChatCompletionsData, Client, ImageUrl, Message, MessageContent,
    MessageContentPart, MessageRole, Model,
};
use crate::function::{ToolResult, ToolResults};
use crate::utils::{base64_encode, sha256, AbortSignal};

use anyhow::{bail, Context, Result};
use fancy_regex::Regex;
use std::{collections::HashMap, fs::File, io::Read, path::Path};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const IMAGE_EXTS: [&str; 5] = ["png", "jpeg", "jpg", "webp", "gif"];

lazy_static::lazy_static! {
    static ref URL_RE: Regex = Regex::new(r"^[A-Za-z0-9_-]{2,}:/").unwrap();
}

#[derive(Debug, Clone)]
pub struct Input {
    config: GlobalConfig,
    text: String,
    patched_text: Option<String>,
    continue_output: Option<String>,
    regenerate: bool,
    medias: Vec<String>,
    data_urls: HashMap<String, String>,
    tool_call: Option<ToolResults>,
    rag_name: Option<String>,
    role: Role,
    with_session: bool,
    with_agent: bool,
}

impl Input {
    pub fn from_str(config: &GlobalConfig, text: &str, role: Option<Role>) -> Self {
        let (role, with_session, with_agent) = resolve_role(&config.read(), role);
        Self {
            config: config.clone(),
            text: text.to_string(),
            patched_text: None,
            continue_output: None,
            regenerate: false,
            medias: Default::default(),
            data_urls: Default::default(),
            tool_call: None,
            rag_name: None,
            role,
            with_session,
            with_agent,
        }
    }

    pub async fn from_files(
        config: &GlobalConfig,
        text: &str,
        paths: Vec<String>,
        role: Option<Role>,
    ) -> Result<Self> {
        let mut texts = vec![];
        if !text.is_empty() {
            texts.push(text.to_string());
        };
        let spinner = create_spinner("Loading files").await;
        let ret = load_paths(config, paths).await;
        spinner.stop();
        let (files, medias, data_urls) = ret?;
        let files_len = files.len();
        if files_len > 0 {
            texts.push(String::new());
        }
        for (path, contents) in files {
            texts.push(format!("`{path}`:\n\n{contents}\n"));
        }
        let (role, with_session, with_agent) = resolve_role(&config.read(), role);
        Ok(Self {
            config: config.clone(),
            text: texts.join("\n"),
            patched_text: None,
            continue_output: None,
            regenerate: false,
            medias,
            data_urls,
            tool_call: Default::default(),
            rag_name: None,
            role,
            with_session,
            with_agent,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.medias.is_empty()
    }

    pub fn data_urls(&self) -> HashMap<String, String> {
        self.data_urls.clone()
    }

    pub fn text(&self) -> String {
        match self.patched_text.clone() {
            Some(text) => text,
            None => self.text.clone(),
        }
    }

    pub fn clear_patch(&mut self) {
        self.patched_text = None;
    }

    pub fn set_text(&mut self, text: String) {
        self.text = text;
    }

    pub fn continue_output(&self) -> Option<&str> {
        self.continue_output.as_deref()
    }

    pub fn set_continue_output(&mut self, output: &str) {
        let output = match &self.continue_output {
            Some(v) => format!("{v}{output}"),
            None => output.to_string(),
        };
        self.continue_output = Some(output);
    }

    pub fn regenerate(&self) -> bool {
        self.regenerate
    }

    pub fn set_regenerate(&mut self) {
        let role = self.config.read().extract_role();
        if role.name() == self.role().name() {
            self.role = role;
        }
        self.regenerate = true;
    }

    pub async fn use_embeddings(&mut self, abort_signal: AbortSignal) -> Result<()> {
        if self.text.is_empty() {
            return Ok(());
        }
        if !self.text.is_empty() {
            let rag = self.config.read().rag.clone();
            if let Some(rag) = rag {
                let result =
                    Config::search_rag(&self.config, &rag, &self.text, abort_signal).await?;
                self.patched_text = Some(result);
                self.rag_name = Some(rag.name().to_string());
            }
        }
        Ok(())
    }

    pub fn rag_name(&self) -> Option<&str> {
        self.rag_name.as_deref()
    }

    pub fn merge_tool_call(mut self, output: String, tool_results: Vec<ToolResult>) -> Self {
        match self.tool_call.as_mut() {
            Some(exist_tool_results) => {
                exist_tool_results.0.extend(tool_results);
                exist_tool_results.1 = output;
            }
            None => self.tool_call = Some((tool_results, output)),
        }
        self
    }

    pub fn create_client(&self) -> Result<Box<dyn Client>> {
        init_client(&self.config, Some(self.role().model().clone()))
    }

    pub fn prepare_completion_data(
        &self,
        model: &Model,
        stream: bool,
    ) -> Result<ChatCompletionsData> {
        if !self.medias.is_empty() && !model.supports_vision() {
            bail!("The current model does not support vision. Is the model configured with `supports_vision: true`?");
        }
        let messages = self.build_messages()?;
        model.guard_max_input_tokens(&messages)?;
        let temperature = self.role().temperature();
        let top_p = self.role().top_p();
        let functions = self.config.read().select_functions(self.role());
        Ok(ChatCompletionsData {
            messages,
            temperature,
            top_p,
            functions,
            stream,
        })
    }

    pub fn build_messages(&self) -> Result<Vec<Message>> {
        let mut messages = if let Some(session) = self.session(&self.config.read().session) {
            session.build_messages(self)
        } else {
            self.role().build_messages(self)
        };
        if let Some(tool_results) = &self.tool_call {
            messages.push(Message::new(
                MessageRole::Assistant,
                MessageContent::ToolResults(tool_results.clone()),
            ))
        }
        Ok(messages)
    }

    pub fn echo_messages(&self) -> String {
        if let Some(session) = self.session(&self.config.read().session) {
            session.echo_messages(self)
        } else {
            self.role().echo_messages(self)
        }
    }

    pub fn role(&self) -> &Role {
        &self.role
    }

    pub fn session<'a>(&self, session: &'a Option<Session>) -> Option<&'a Session> {
        if self.with_session {
            session.as_ref()
        } else {
            None
        }
    }

    pub fn session_mut<'a>(&self, session: &'a mut Option<Session>) -> Option<&'a mut Session> {
        if self.with_session {
            session.as_mut()
        } else {
            None
        }
    }

    pub fn with_agent(&self) -> bool {
        self.with_agent
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
        let text = self.text();
        if self.medias.is_empty() {
            return text;
        }
        let tail_text = if text.is_empty() {
            String::new()
        } else {
            format!(" -- {text}")
        };
        let files: Vec<String> = self
            .medias
            .iter()
            .cloned()
            .map(|url| resolve_data_url(&self.data_urls, url))
            .collect();
        format!(".file {}{}", files.join(" "), tail_text)
    }

    pub fn message_content(&self) -> MessageContent {
        if self.medias.is_empty() {
            MessageContent::Text(self.text())
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
                list.insert(0, MessageContentPart::Text { text: self.text() });
            }
            MessageContent::Array(list)
        }
    }
}

fn resolve_role(config: &Config, role: Option<Role>) -> (Role, bool, bool) {
    match role {
        Some(v) => (v, false, false),
        None => (
            config.extract_role(),
            config.session.is_some(),
            config.agent.is_some(),
        ),
    }
}

async fn load_paths(
    config: &GlobalConfig,
    paths: Vec<String>,
) -> Result<(Vec<(String, String)>, Vec<String>, HashMap<String, String>)> {
    let mut files = vec![];
    let mut medias = vec![];
    let mut data_urls = HashMap::new();
    let loaders = config.read().document_loaders.clone();
    let mut local_paths = vec![];
    let mut remote_urls = vec![];
    for path in paths {
        match resolve_local_path(&path) {
            Some(v) => local_paths.push(v),
            None => remote_urls.push(path),
        }
    }
    let local_files = expand_glob_paths(&local_paths).await?;
    for file_path in local_files {
        if is_image(&file_path) {
            let data_url = read_media_to_data_url(&file_path)
                .with_context(|| format!("Unable to read media file '{file_path}'"))?;
            data_urls.insert(sha256(&data_url), file_path);
            medias.push(data_url)
        } else {
            let text = read_file(&file_path)
                .with_context(|| format!("Unable to read file '{file_path}'"))?;
            files.push((file_path, text));
        }
    }
    for file_url in remote_urls {
        let (contents, extension) = fetch(&loaders, &file_url, true)
            .await
            .with_context(|| format!("Failed to load url '{file_url}'"))?;
        if extension == MEDIA_URL_EXTENSION {
            data_urls.insert(sha256(&contents), file_url);
            medias.push(contents)
        } else {
            files.push((file_url, contents));
        }
    }
    Ok((files, medias, data_urls))
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

fn resolve_local_path(path: &str) -> Option<String> {
    if let Ok(true) = URL_RE.is_match(path) {
        return None;
    }
    let new_path = if let (Some(file), Some(home)) = (path.strip_prefix("~/"), dirs::home_dir()) {
        home.join(file).display().to_string()
    } else {
        path.to_string()
    };
    Some(new_path)
}

fn is_image(path: &str) -> bool {
    get_patch_extension(path)
        .map(|v| IMAGE_EXTS.contains(&v.as_str()))
        .unwrap_or_default()
}

fn read_media_to_data_url(image_path: &str) -> Result<String> {
    let extension = get_patch_extension(image_path).unwrap_or_default();
    let mime_type = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => bail!("Unexpected media type"),
    };
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
