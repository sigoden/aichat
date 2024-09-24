use self::bm25::*;
use self::loader::*;
use self::splitter::*;

use crate::client::*;
use crate::config::*;
use crate::utils::*;

mod bm25;
mod loader;
mod serde_vectors;
mod splitter;

use anyhow::{anyhow, bail, Context, Result};
use hnsw_rs::prelude::*;
use indexmap::{IndexMap, IndexSet};
use inquire::{required, validator::Validation, Confirm, Select, Text};
use parking_lot::RwLock;
use path_absolutize::Absolutize;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::HashMap, env, fmt::Debug, fs, path::Path, time::Duration};
use tokio::time::sleep;

pub struct Rag {
    config: GlobalConfig,
    name: String,
    path: String,
    embedding_model: Model,
    hnsw: Hnsw<'static, f32, DistCosine>,
    bm25: BM25<DocumentId>,
    data: RagData,
    last_sources: RwLock<Option<String>>,
}

impl Debug for Rag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rag")
            .field("name", &self.name)
            .field("path", &self.path)
            .field("embedding_model", &self.embedding_model)
            .field("data", &self.data)
            .finish()
    }
}

impl Clone for Rag {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            name: self.name.clone(),
            path: self.path.clone(),
            embedding_model: self.embedding_model.clone(),
            hnsw: self.data.build_hnsw(),
            bm25: self.bm25.clone(),
            data: self.data.clone(),
            last_sources: RwLock::new(None),
        }
    }
}

impl Rag {
    pub async fn init(
        config: &GlobalConfig,
        name: &str,
        save_path: &Path,
        doc_paths: &[String],
        abort_signal: AbortSignal,
    ) -> Result<Self> {
        debug!("init rag: {name}");
        let (embedding_model, chunk_size, chunk_overlap) = Self::create_config(config)?;
        let (reranker_model, top_k) = {
            let config = config.read();
            (config.rag_reranker_model.clone(), config.rag_top_k)
        };
        let data = RagData::new(
            embedding_model.id(),
            chunk_size,
            chunk_overlap,
            reranker_model,
            top_k,
            embedding_model.max_batch_size(),
        );
        let mut rag = Self::create(config, name, save_path, data)?;
        let mut paths = doc_paths.to_vec();
        if paths.is_empty() {
            paths = add_documents()?;
        };
        debug!("doc paths: {paths:?}");
        let loaders = config.read().document_loaders.clone();
        let spinner = create_spinner("Starting").await;
        tokio::select! {
            ret = rag.sync_documents(loaders, &paths, Some(spinner.clone())) => {
                spinner.stop();
                ret?;
            }
            _ = watch_abort_signal(abort_signal) => {
                spinner.stop();
                bail!("Aborted!")
            },
        };
        if rag.save()? {
            println!("✨ Saved rag to '{}'", save_path.display());
        }
        Ok(rag)
    }

    pub fn load(config: &GlobalConfig, name: &str, path: &Path) -> Result<Self> {
        let err = || format!("Failed to load rag '{name}' at '{}'", path.display());
        let content = fs::read_to_string(path).with_context(err)?;
        let data: RagData = serde_yaml::from_str(&content).with_context(err)?;
        Self::create(config, name, path, data)
    }

    pub fn create(config: &GlobalConfig, name: &str, path: &Path, data: RagData) -> Result<Self> {
        let hnsw = data.build_hnsw();
        let bm25 = data.build_bm25();
        let embedding_model = Model::retrieve_embedding(&config.read(), &data.embedding_model)?;
        let rag = Rag {
            config: config.clone(),
            name: name.to_string(),
            path: path.display().to_string(),
            data,
            embedding_model,
            hnsw,
            bm25,
            last_sources: RwLock::new(None),
        };
        Ok(rag)
    }

    pub async fn rebuild(
        &mut self,
        config: &GlobalConfig,
        abort_signal: AbortSignal,
    ) -> Result<()> {
        debug!("rebuild rag: {}", self.name);
        let loaders = config.read().document_loaders.clone();
        let spinner = create_spinner("Starting").await;
        let paths = self.data.document_paths.clone();
        tokio::select! {
            ret = self.sync_documents(loaders, &paths, Some(spinner.clone())) => {
                spinner.stop();
                ret?;
            }
            _ = watch_abort_signal(abort_signal) => {
                spinner.stop();
                bail!("Aborted!")
            },
        };
        if self.save()? {
            println!("✨ Saved rag to '{}'", self.path);
        }
        Ok(())
    }

    pub fn create_config(config: &GlobalConfig) -> Result<(Model, usize, usize)> {
        let (embedding_model_id, chunk_size, chunk_overlap) = {
            let config = config.read();
            (
                config.rag_embedding_model.clone(),
                config.rag_chunk_size,
                config.rag_chunk_overlap,
            )
        };
        let embedding_model_id = match embedding_model_id {
            Some(value) => {
                println!("Select embedding model: {value}");
                value
            }
            None => {
                let models = list_embedding_models(&config.read());
                if models.is_empty() {
                    bail!("No available embedding model");
                }
                if *IS_STDOUT_TERMINAL {
                    select_embedding_model(&models)?
                } else {
                    let value = models[0].id();
                    println!("Select embedding model: {value}");
                    value
                }
            }
        };
        let embedding_model = Model::retrieve_embedding(&config.read(), &embedding_model_id)?;

        let chunk_size = match chunk_size {
            Some(value) => {
                println!("Set chunk size: {value}");
                value
            }
            None => {
                if *IS_STDOUT_TERMINAL {
                    set_chunk_size(&embedding_model)?
                } else {
                    let value = embedding_model.default_chunk_size();
                    println!("Set chunk size: {value}");
                    value
                }
            }
        };
        let chunk_overlap = match chunk_overlap {
            Some(value) => {
                println!("Set chunk overlay: {value}");
                value
            }
            None => {
                let value = chunk_size / 20;
                if *IS_STDOUT_TERMINAL {
                    set_chunk_overlay(value)?
                } else {
                    println!("Set chunk overlay: {value}");
                    value
                }
            }
        };

        Ok((embedding_model, chunk_size, chunk_overlap))
    }

    pub fn get_config(&self) -> (Option<String>, usize) {
        (self.data.reranker_model.clone(), self.data.top_k)
    }

    pub fn get_last_sources(&self) -> Option<String> {
        self.last_sources.read().clone()
    }

    pub fn set_last_sources(&self, ids: &[DocumentId]) {
        let sources: IndexSet<_> = ids
            .iter()
            .filter_map(|id| {
                let (file_index, _) = split_document_id(*id);
                let file = self.data.files.get(&file_index)?;
                Some(file.path.clone())
            })
            .collect();
        let sources = if sources.is_empty() {
            None
        } else {
            Some(sources.into_iter().collect::<Vec<_>>().join("\n"))
        };
        *self.last_sources.write() = sources;
    }

    pub fn set_reranker_model(&mut self, reranker_model: Option<String>) -> Result<()> {
        self.data.reranker_model = reranker_model;
        self.save()?;
        Ok(())
    }

    pub fn set_top_k(&mut self, top_k: usize) -> Result<()> {
        self.data.top_k = top_k;
        self.save()?;
        Ok(())
    }

    pub fn save(&self) -> Result<bool> {
        if self.is_temp() {
            return Ok(false);
        }
        let path = Path::new(&self.path);
        ensure_parent_exists(path)?;

        let content = serde_yaml::to_string(&self.data)
            .with_context(|| format!("Failed to serde rag '{}'", self.name))?;
        fs::write(path, content).with_context(|| {
            format!("Failed to save rag '{}' to '{}'", self.name, path.display())
        })?;

        Ok(true)
    }

    pub fn export(&self) -> Result<String> {
        let files: Vec<_> = self
            .data
            .files
            .iter()
            .map(|(_, v)| {
                json!({
                    "path": v.path,
                    "num_chunks": v.documents.len(),
                })
            })
            .collect();
        let data = json!({
            "path": self.path,
            "embedding_model": self.embedding_model.id(),
            "chunk_size": self.data.chunk_size,
            "chunk_overlap": self.data.chunk_overlap,
            "reranker_model": self.data.reranker_model,
            "top_k": self.data.top_k,
            "batch_size": self.data.batch_size,
            "document_paths": self.data.document_paths,
            "files": files,
        });
        let output = serde_yaml::to_string(&data)
            .with_context(|| format!("Unable to show info about rag '{}'", self.name))?;
        Ok(output)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn is_temp(&self) -> bool {
        self.name == TEMP_RAG_NAME
    }

    pub async fn search(
        &self,
        text: &str,
        top_k: usize,
        min_score_vector_search: f32,
        min_score_keyword_search: f32,
        rerank_model: Option<&str>,
        abort_signal: AbortSignal,
    ) -> Result<(String, Vec<DocumentId>)> {
        let spinner = create_spinner("Searching").await;
        let ret = tokio::select! {
            ret = self.hybird_search(text, top_k, min_score_vector_search, min_score_keyword_search, rerank_model) => {
                ret
            }
            _ = watch_abort_signal(abort_signal) => {
                bail!("Aborted!")
            },
        };
        spinner.stop();
        let (ids, documents): (Vec<_>, Vec<_>) = ret?.into_iter().unzip();
        let embeddings = documents.join("\n\n");
        Ok((embeddings, ids))
    }

    pub async fn sync_documents<T: AsRef<str>>(
        &mut self,
        loaders: HashMap<String, String>,
        paths: &[T],
        spinner: Option<Spinner>,
    ) -> Result<()> {
        if let Some(spinner) = &spinner {
            let _ = spinner.set_message(String::new());
        }

        let mut document_paths = vec![];
        let mut files = vec![];
        let paths_len = paths.len();
        let mut has_error = false;
        for (index, path) in paths.iter().enumerate() {
            let path = path.as_ref();
            println!("Load {path} [{}/{paths_len}]", index + 1);
            let (path, document_files) = load_document(&loaders, path, &mut has_error).await;
            files.extend(document_files);
            document_paths.push(path);
        }

        if has_error {
            let mut aborted = true;
            if *IS_STDOUT_TERMINAL && !document_paths.is_empty() {
                let ans = Confirm::new("Some documents failed to load. Continue?")
                    .with_default(false)
                    .prompt()?;
                aborted = !ans;
            }
            if aborted {
                bail!("Aborted");
            }
        }

        let mut to_deleted: IndexMap<String, FileId> = Default::default();
        for (file_id, file) in &self.data.files {
            to_deleted.insert(file.hash.clone(), *file_id);
        }

        let mut rag_files = vec![];
        for (contents, mut metadata) in files {
            let path = match metadata.swap_remove(PATH_METADATA) {
                Some(v) => v,
                None => continue,
            };
            let hash = sha256(&contents);
            if let Some(file_id) = to_deleted.get(&hash) {
                if self.data.files[file_id].path == path {
                    to_deleted.swap_remove(&hash);
                    continue;
                }
            }
            let extension = metadata
                .swap_remove(EXTENSION_METADATA)
                .unwrap_or_else(|| DEFAULT_EXTENSION.into());
            let separator = get_separators(&extension);
            let splitter = RecursiveCharacterTextSplitter::new(
                self.data.chunk_size,
                self.data.chunk_overlap,
                &separator,
            );

            let metadata = metadata
                .iter()
                .map(|(k, v)| format!("{k}: {v}\n"))
                .collect::<Vec<String>>()
                .join("");
            let split_options = SplitterChunkHeaderOptions::default().with_chunk_header(&format!(
                "<document_metadata>\npath: {path}\n{metadata}</document_metadata>\n\n"
            ));
            let document = RagDocument::new(contents);
            let split_documents = splitter.split_documents(&[document], &split_options);
            rag_files.push(RagFile {
                hash: hash.clone(),
                path,
                documents: split_documents,
            });
        }

        let mut next_file_id = self.data.next_file_id;
        let mut files = vec![];
        let mut document_ids = vec![];
        let mut embeddings = vec![];

        if !rag_files.is_empty() {
            let mut texts = vec![];
            for file in rag_files.into_iter() {
                for (document_index, document) in file.documents.iter().enumerate() {
                    document_ids.push(combine_document_id(next_file_id, document_index));
                    texts.push(document.page_content.clone())
                }
                files.push((next_file_id, file));
                next_file_id += 1;
            }

            let embeddings_data = EmbeddingsData::new(texts, false);
            embeddings = self
                .create_embeddings(embeddings_data, spinner.clone())
                .await?;
        }

        self.data.del(to_deleted.values().cloned().collect());
        self.data.add(next_file_id, files, document_ids, embeddings);
        self.data.document_paths = document_paths;

        if self.data.files.is_empty() {
            bail!("No RAG files");
        }

        progress(&spinner, "Building store".into());
        self.hnsw = self.data.build_hnsw();
        self.bm25 = self.data.build_bm25();

        Ok(())
    }

    async fn hybird_search(
        &self,
        query: &str,
        top_k: usize,
        min_score_vector_search: f32,
        min_score_keyword_search: f32,
        rerank_model: Option<&str>,
    ) -> Result<Vec<(DocumentId, String)>> {
        let (vector_search_result, text_search_result) = tokio::join!(
            self.vector_search(query, top_k, min_score_vector_search),
            self.keyword_search(query, top_k, min_score_keyword_search)
        );
        let vector_search_ids = vector_search_result?;
        let keyword_search_ids = text_search_result?;
        debug!(
            "vector_search_ids: {:?}, keyword_search_ids: {:?}",
            pretty_document_ids(&vector_search_ids),
            pretty_document_ids(&keyword_search_ids)
        );
        let ids = match rerank_model {
            Some(model_id) => {
                let model = Model::retrieve_reranker(&self.config.read(), model_id)?;
                let client = init_client(&self.config, Some(model))?;
                let ids: IndexSet<DocumentId> = [vector_search_ids, keyword_search_ids]
                    .concat()
                    .into_iter()
                    .collect();
                let mut documents = vec![];
                let mut documents_ids = vec![];
                for id in ids {
                    if let Some(document) = self.data.get(id) {
                        documents_ids.push(id);
                        documents.push(document.page_content.to_string());
                    }
                }
                let data = RerankData::new(query.to_string(), documents, top_k);
                let list = client.rerank(&data).await.context("Failed to rerank")?;
                let ids: Vec<_> = list
                    .into_iter()
                    .take(top_k)
                    .filter_map(|item| documents_ids.get(item.index).cloned())
                    .collect();
                debug!("rerank_ids: {:?}", pretty_document_ids(&ids));
                ids
            }
            None => {
                let ids = reciprocal_rank_fusion(
                    vec![vector_search_ids, keyword_search_ids],
                    vec![1.0, 1.0],
                    top_k,
                );
                debug!("rrf_ids: {:?}", pretty_document_ids(&ids));
                ids
            }
        };
        let output = ids
            .into_iter()
            .filter_map(|id| {
                let document = self.data.get(id)?;
                Some((id, document.page_content.clone()))
            })
            .collect();
        Ok(output)
    }

    async fn vector_search(
        &self,
        query: &str,
        top_k: usize,
        min_score: f32,
    ) -> Result<Vec<DocumentId>> {
        let splitter = RecursiveCharacterTextSplitter::new(
            self.data.chunk_size,
            self.data.chunk_overlap,
            &DEFAULT_SEPARATES,
        );
        let texts = splitter.split_text(query);
        let embeddings_data = EmbeddingsData::new(texts, true);
        let embeddings = self.create_embeddings(embeddings_data, None).await?;
        let output = self
            .hnsw
            .parallel_search(&embeddings, top_k, 30)
            .into_iter()
            .flat_map(|list| {
                list.into_iter()
                    .filter_map(|v| {
                        if v.distance < min_score {
                            return None;
                        }
                        Some(v.d_id)
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        Ok(output)
    }

    async fn keyword_search(
        &self,
        query: &str,
        top_k: usize,
        min_score: f32,
    ) -> Result<Vec<DocumentId>> {
        let output = self.bm25.search(query, top_k, Some(min_score as f64));
        Ok(output)
    }

    async fn create_embeddings(
        &self,
        data: EmbeddingsData,
        spinner: Option<Spinner>,
    ) -> Result<EmbeddingsOutput> {
        let embedding_client = init_client(&self.config, Some(self.embedding_model.clone()))?;
        let EmbeddingsData { texts, query } = data;
        let batch_size = self
            .data
            .batch_size
            .or_else(|| self.embedding_model.max_batch_size());
        let batch_size = match self.embedding_model.max_input_tokens() {
            Some(max_input_tokens) => {
                let x = max_input_tokens / self.data.chunk_size;
                match batch_size {
                    Some(y) => x.min(y),
                    None => x,
                }
            }
            None => batch_size.unwrap_or(1),
        };
        let mut output = vec![];
        let batch_chunks = texts.chunks(batch_size.max(1));
        let batch_chunks_len = batch_chunks.len();
        let retry_limit = env::var(get_env_name("embeddings_retry_limit"))
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(2);
        for (index, texts) in batch_chunks.enumerate() {
            progress(
                &spinner,
                format!("Creating embeddings [{}/{batch_chunks_len}]", index + 1),
            );
            let chunk_data = EmbeddingsData {
                texts: texts.to_vec(),
                query,
            };
            let mut retry = 0;
            let chunk_output = loop {
                retry += 1;
                match embedding_client.embeddings(&chunk_data).await {
                    Ok(v) => break v,
                    Err(e) if retry < retry_limit => {
                        debug!("retry {} failed: {}", retry, e);
                        sleep(Duration::from_secs(2u64.pow(retry - 1))).await;
                        continue;
                    }
                    Err(e) => {
                        return Err(e).with_context(|| {
                            format!("Failed to create embedding after {retry_limit} attempts")
                        })?
                    }
                }
            };
            output.extend(chunk_output);
        }
        Ok(output)
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RagData {
    pub embedding_model: String,
    pub chunk_size: usize,
    pub chunk_overlap: usize,
    pub reranker_model: Option<String>,
    pub top_k: usize,
    pub batch_size: Option<usize>,
    pub next_file_id: FileId,
    pub document_paths: Vec<String>,
    pub files: IndexMap<FileId, RagFile>,
    #[serde(with = "serde_vectors")]
    pub vectors: IndexMap<DocumentId, Vec<f32>>,
}

impl Debug for RagData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RagData")
            .field("embedding_model", &self.embedding_model)
            .field("chunk_size", &self.chunk_size)
            .field("chunk_overlap", &self.chunk_overlap)
            .field("reranker_model", &self.reranker_model)
            .field("top_k", &self.top_k)
            .field("batch_size", &self.batch_size)
            .field("next_file_id", &self.next_file_id)
            .field("document_paths", &self.document_paths)
            .field("files", &self.files)
            .finish()
    }
}

impl RagData {
    pub fn new(
        embedding_model: String,
        chunk_size: usize,
        chunk_overlap: usize,
        reranker_model: Option<String>,
        top_k: usize,
        batch_size: Option<usize>,
    ) -> Self {
        Self {
            embedding_model,
            chunk_size,
            chunk_overlap,
            reranker_model,
            top_k,
            batch_size,
            next_file_id: 0,
            document_paths: Default::default(),
            files: Default::default(),
            vectors: Default::default(),
        }
    }

    pub fn get(&self, id: DocumentId) -> Option<&RagDocument> {
        let (file_index, document_index) = split_document_id(id);
        let file = self.files.get(&file_index)?;
        let document = file.documents.get(document_index)?;
        Some(document)
    }

    pub fn del(&mut self, file_ids: Vec<FileId>) {
        for file_id in file_ids {
            if let Some(file) = self.files.swap_remove(&file_id) {
                for (document_index, _) in file.documents.iter().enumerate() {
                    let document_id = combine_document_id(file_id, document_index);
                    self.vectors.swap_remove(&document_id);
                }
            }
        }
    }

    pub fn add(
        &mut self,
        next_file_id: FileId,
        files: Vec<(FileId, RagFile)>,
        document_ids: Vec<DocumentId>,
        embeddings: EmbeddingsOutput,
    ) {
        self.next_file_id = next_file_id;
        self.files.extend(files);
        self.vectors
            .extend(document_ids.into_iter().zip(embeddings));
    }

    pub fn build_hnsw(&self) -> Hnsw<'static, f32, DistCosine> {
        let hnsw = Hnsw::new(32, self.vectors.len(), 16, 200, DistCosine {});
        let list: Vec<_> = self.vectors.iter().map(|(k, v)| (v, *k)).collect();
        hnsw.parallel_insert(&list);
        hnsw
    }

    pub fn build_bm25(&self) -> BM25<DocumentId> {
        let mut corpus = vec![];
        for (file_index, file) in self.files.iter() {
            for (document_index, document) in file.documents.iter().enumerate() {
                let id = combine_document_id(*file_index, document_index);
                corpus.push((id, document.page_content.clone()));
            }
        }
        BM25::new(corpus, BM25Options::default())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagFile {
    hash: String,
    path: String,
    documents: Vec<RagDocument>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RagDocument {
    pub page_content: String,
    pub metadata: RagMetadata,
}

impl RagDocument {
    pub fn new<S: Into<String>>(page_content: S) -> Self {
        RagDocument {
            page_content: page_content.into(),
            metadata: IndexMap::new(),
        }
    }
}

impl Default for RagDocument {
    fn default() -> Self {
        RagDocument {
            page_content: "".to_string(),
            metadata: IndexMap::new(),
        }
    }
}

pub type RagMetadata = IndexMap<String, String>;

pub type FileId = usize;
pub type DocumentId = usize;

pub fn combine_document_id(file_index: usize, document_index: usize) -> DocumentId {
    file_index << (usize::BITS / 2) | document_index
}

pub fn split_document_id(value: DocumentId) -> (usize, usize) {
    let low_mask = (1 << (usize::BITS / 2)) - 1;
    let low = value & low_mask;
    let high = value >> (usize::BITS / 2);
    (high, low)
}

fn pretty_document_ids(ids: &[DocumentId]) -> Vec<String> {
    ids.iter()
        .map(|v| {
            let (h, l) = split_document_id(*v);
            format!("{h}-{l}")
        })
        .collect()
}

fn select_embedding_model(models: &[&Model]) -> Result<String> {
    let models: Vec<_> = models
        .iter()
        .map(|v| SelectOption::new(v.id(), v.description()))
        .collect();
    let result = Select::new("Select embedding model:", models).prompt()?;
    Ok(result.value)
}

fn set_chunk_size(model: &Model) -> Result<usize> {
    let default_value = model.default_chunk_size().to_string();
    let help_message = model
        .max_tokens_per_chunk()
        .map(|v| format!("The model's max_tokens is {v}"));

    let mut text = Text::new("Set chunk size:")
        .with_default(&default_value)
        .with_validator(move |text: &str| {
            let out = match text.parse::<usize>() {
                Ok(_) => Validation::Valid,
                Err(_) => Validation::Invalid("Must be a integer".into()),
            };
            Ok(out)
        });
    if let Some(help_message) = &help_message {
        text = text.with_help_message(help_message);
    }
    let value = text.prompt()?;
    value.parse().map_err(|_| anyhow!("Invalid chunk_size"))
}

fn set_chunk_overlay(default_value: usize) -> Result<usize> {
    let value = Text::new("Set chunk overlay:")
        .with_default(&default_value.to_string())
        .with_validator(move |text: &str| {
            let out = match text.parse::<usize>() {
                Ok(_) => Validation::Valid,
                Err(_) => Validation::Invalid("Must be a integer".into()),
            };
            Ok(out)
        })
        .prompt()?;
    value.parse().map_err(|_| anyhow!("Invalid chunk_overlay"))
}

fn add_documents() -> Result<Vec<String>> {
    let text = Text::new("Add documents:")
        .with_validator(required!("This field is required"))
        .with_help_message("e.g. file;dir/;dir/**/*.{md,mdx};solo-url;site-url/**")
        .prompt()?;
    let paths = text
        .split(';')
        .filter_map(|v| {
            let v = v.trim().to_string();
            if v.is_empty() {
                None
            } else {
                Some(v)
            }
        })
        .collect();
    Ok(paths)
}

fn progress(spinner: &Option<Spinner>, message: String) {
    if let Some(spinner) = spinner {
        let _ = spinner.set_message(message);
    }
}

fn reciprocal_rank_fusion(
    list_of_document_ids: Vec<Vec<DocumentId>>,
    list_of_weights: Vec<f32>,
    top_k: usize,
) -> Vec<DocumentId> {
    let rrf_k = top_k * 2;
    let mut map: IndexMap<DocumentId, f32> = IndexMap::new();
    for (document_ids, weight) in list_of_document_ids
        .into_iter()
        .zip(list_of_weights.into_iter())
    {
        for (index, &item) in document_ids.iter().enumerate() {
            *map.entry(item).or_default() += (1.0 / ((rrf_k + index + 1) as f32)) * weight;
        }
    }
    let mut sorted_items: Vec<(DocumentId, f32)> = map.into_iter().collect();
    sorted_items.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    sorted_items
        .into_iter()
        .take(top_k)
        .map(|(v, _)| v)
        .collect()
}
