use self::bm25::*;
use self::loader::*;
use self::splitter::*;

use crate::client::*;
use crate::config::*;
use crate::utils::*;

mod bm25;
mod loader;
mod splitter;

use anyhow::bail;
use anyhow::{anyhow, Context, Result};
use hnsw_rs::prelude::*;
use indexmap::{IndexMap, IndexSet};
use inquire::{required, validator::Validation, Select, Text};
use path_absolutize::Absolutize;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::{fmt::Debug, io::BufReader, path::Path};

pub struct Rag {
    name: String,
    path: String,
    embedding_model: Model,
    hnsw: Hnsw<'static, f32, DistCosine>,
    bm25: BM25<DocumentId>,
    data: RagData,
    embedding_client: Box<dyn Client>,
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

impl Rag {
    pub async fn init(
        config: &GlobalConfig,
        name: &str,
        save_path: &Path,
        doc_paths: &[String],
        abort_signal: AbortSignal,
    ) -> Result<Self> {
        debug!("init rag: {name}");
        let (embedding_model, chunk_size, chunk_overlap) = Self::config(config)?;
        let data = RagData::new(embedding_model.id(), chunk_size, chunk_overlap);
        let mut rag = Self::create(config, name, save_path, data)?;
        let mut paths = doc_paths.to_vec();
        if paths.is_empty() {
            paths = add_document_paths()?;
        };
        debug!("doc paths: {paths:?}");
        let loaders = config.read().document_loaders.clone();
        let spinner = create_spinner("Starting").await;
        tokio::select! {
            ret = rag.add_paths(loaders, &paths, Some(spinner.clone())) => {
                spinner.stop();
                ret?;
            }
            _ = watch_abort_signal(abort_signal) => {
                spinner.stop();
                bail!("Aborted!")
            },
        };
        if !rag.is_temp() {
            rag.save(save_path)?;
            println!("✨ Saved rag to '{}'", save_path.display());
        }
        Ok(rag)
    }

    pub fn load(config: &GlobalConfig, name: &str, path: &Path) -> Result<Self> {
        let err = || format!("Failed to load rag '{name}'");
        let file = std::fs::File::open(path).with_context(err)?;
        let reader = BufReader::new(file);
        let data: RagData = bincode::deserialize_from(reader).with_context(err)?;
        Self::create(config, name, path, data)
    }

    pub fn create(config: &GlobalConfig, name: &str, path: &Path, data: RagData) -> Result<Self> {
        let hnsw = data.build_hnsw();
        let bm25 = data.build_bm25();
        let embedding_model = Model::retrieve_embedding(&config.read(), &data.embedding_model)?;
        let embedding_client = init_client(config, Some(embedding_model.clone()))?;
        let rag = Rag {
            name: name.to_string(),
            path: path.display().to_string(),
            data,
            embedding_model,
            hnsw,
            bm25,
            embedding_client,
        };
        Ok(rag)
    }

    pub fn config(config: &GlobalConfig) -> Result<(Model, usize, usize)> {
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

    pub fn save(&self, path: &Path) -> Result<()> {
        ensure_parent_exists(path)?;
        let mut file = std::fs::File::create(path)?;
        bincode::serialize_into(&mut file, &self.data)
            .with_context(|| format!("Failed to save rag '{}'", self.name))?;
        Ok(())
    }

    pub fn export(&self) -> Result<String> {
        let files: Vec<_> = self.data.files.iter().map(|v| &v.path).collect();
        let data = json!({
            "path": self.path,
            "embedding_model": self.embedding_model.id(),
            "chunk_size": self.data.chunk_size,
            "chunk_overlap": self.data.chunk_overlap,
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
        rerank: Option<(Box<dyn Client>, f32)>,
        abort_signal: AbortSignal,
    ) -> Result<String> {
        let spinner = create_spinner("Searching").await;
        let ret = tokio::select! {
            ret = self.hybird_search(text, top_k, min_score_vector_search, min_score_keyword_search, rerank) => {
                ret
            }
            _ = watch_abort_signal(abort_signal) => {
                bail!("Aborted!")
            },
        };
        spinner.stop();
        let output = ret?.join("\n\n");
        Ok(output)
    }

    pub async fn add_paths<T: AsRef<str>>(
        &mut self,
        loaders: HashMap<String, String>,
        paths: &[T],
        spinner: Option<Spinner>,
    ) -> Result<()> {
        let mut rag_files = vec![];

        // List files
        let mut new_paths = vec![];
        progress(&spinner, "Gathering paths".into());
        for path in paths {
            let path = path.as_ref();
            if Self::is_url_path(path) {
                if let Some(path) = path.strip_suffix("**") {
                    new_paths.push((path.to_string(), RECURSIVE_URL_LOADER.into()));
                } else {
                    new_paths.push((path.to_string(), URL_LOADER.into()))
                }
            } else {
                let path = Path::new(path);
                let path = path
                    .absolutize()
                    .with_context(|| anyhow!("Invalid path '{}'", path.display()))?;
                let path_str = path.display().to_string();
                if self.data.files.iter().any(|v| v.path == path_str) {
                    continue;
                }
                let (path_str, suffixes) = parse_glob(&path_str)?;
                let suffixes = if suffixes.is_empty() {
                    None
                } else {
                    Some(&suffixes)
                };
                let mut file_paths = vec![];
                list_files(&mut file_paths, Path::new(&path_str), suffixes).await?;
                for file_path in file_paths {
                    let loader_name = Path::new(&file_path)
                        .extension()
                        .map(|v| v.to_string_lossy().to_lowercase())
                        .unwrap_or_default();
                    new_paths.push((file_path, loader_name))
                }
            }
        }

        // Load files
        let new_paths_len = new_paths.len();
        if new_paths_len > 0 {
            if let Some(spinner) = &spinner {
                let _ = spinner.set_message(String::new());
            }
            for (index, (path, extension)) in new_paths.into_iter().enumerate() {
                println!("Loading {path} [{}/{new_paths_len}]", index + 1);
                let documents = load(&loaders, &path, &extension)
                    .await
                    .with_context(|| format!("Failed to load '{path}'"))?;
                let splitted_documents: Vec<_> = documents
                    .into_iter()
                    .flat_map(|mut document| {
                        let extension = document
                            .metadata
                            .swap_remove(EXTENSION_METADATA)
                            .unwrap_or_else(|| extension.clone());
                        let separator = get_separators(&extension);
                        let splitter = RecursiveCharacterTextSplitter::new(
                            self.data.chunk_size,
                            self.data.chunk_overlap,
                            &separator,
                        );
                        let metadata = document
                            .metadata
                            .iter()
                            .map(|(k, v)| format!("{k}: {v}\n"))
                            .collect::<Vec<String>>()
                            .join("");
                        let split_options = SplitterChunkHeaderOptions::default()
                            .with_chunk_header(&format!(
                                "<document_metadata>\n{metadata}</document_metadata>\n\n"
                            ));
                        splitter.split_documents(&[document], &split_options)
                    })
                    .collect();
                let display_path = if extension == RECURSIVE_URL_LOADER {
                    format!("{path}**")
                } else {
                    path
                };
                rag_files.push(RagFile {
                    path: display_path,
                    documents: splitted_documents,
                })
            }
        }

        if rag_files.is_empty() {
            return Ok(());
        }

        // Convert vectors
        let mut vector_ids = vec![];
        let mut texts = vec![];
        for (file_index, file) in rag_files.iter().enumerate() {
            for (document_index, document) in file.documents.iter().enumerate() {
                vector_ids.push(combine_document_id(file_index, document_index));
                texts.push(document.page_content.clone())
            }
        }

        let embeddings_data = EmbeddingsData::new(texts, false);
        let embeddings = self
            .create_embeddings(embeddings_data, spinner.clone())
            .await?;

        self.data.add(rag_files, vector_ids, embeddings);
        progress(&spinner, "Building vector store".into());
        self.hnsw = self.data.build_hnsw();
        self.bm25 = self.data.build_bm25();

        Ok(())
    }

    pub fn is_url_path(path: &str) -> bool {
        path.starts_with("http://") || path.starts_with("https://")
    }

    async fn hybird_search(
        &self,
        query: &str,
        top_k: usize,
        min_score_vector_search: f32,
        min_score_keyword_search: f32,
        rerank: Option<(Box<dyn Client>, f32)>,
    ) -> Result<Vec<String>> {
        let (vector_search_result, text_search_result) = tokio::join!(
            self.vector_search(query, top_k, min_score_vector_search),
            self.keyword_search(query, top_k, min_score_keyword_search)
        );
        let vector_search_ids = vector_search_result?;
        let keyword_search_ids = text_search_result?;
        debug!(
            "vector_search_ids: {vector_search_ids:?}, keyword_search_ids: {keyword_search_ids:?}"
        );
        let ids = match rerank {
            Some((client, min_score)) => {
                let min_score = min_score as f64;
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
                let list = client.rerank(data).await?;
                let ids = list
                    .into_iter()
                    .take(top_k)
                    .filter_map(|item| {
                        if item.relevance_score < min_score {
                            None
                        } else {
                            documents_ids.get(item.index).cloned()
                        }
                    })
                    .collect();
                debug!("rerank_ids: {ids:?}");
                ids
            }
            None => {
                let ids = reciprocal_rank_fusion(
                    vec![vector_search_ids, keyword_search_ids],
                    vec![1.0, 1.0],
                    top_k,
                );
                debug!("rrf_ids: {ids:?}");
                ids
            }
        };
        let output = ids
            .into_iter()
            .filter_map(|id| {
                let document = self.data.get(id)?;
                Some(document.page_content.clone())
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
        let EmbeddingsData { texts, query } = data;
        let mut output = vec![];
        let batch_chunks = texts.chunks(self.embedding_model.max_batch_size());
        let batch_chunks_len = batch_chunks.len();
        for (index, texts) in batch_chunks.enumerate() {
            progress(
                &spinner,
                format!("Creating embeddings [{}/{batch_chunks_len}]", index + 1),
            );
            let chunk_data = EmbeddingsData {
                texts: texts.to_vec(),
                query,
            };
            let chunk_output = self
                .embedding_client
                .embeddings(chunk_data)
                .await
                .context("Failed to create embedding")?;
            output.extend(chunk_output);
        }
        Ok(output)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagData {
    pub embedding_model: String,
    pub chunk_size: usize,
    pub chunk_overlap: usize,
    pub files: Vec<RagFile>,
    pub vectors: IndexMap<DocumentId, Vec<f32>>,
}

impl RagData {
    pub fn new(embedding_model: String, chunk_size: usize, chunk_overlap: usize) -> Self {
        Self {
            embedding_model,
            chunk_size,
            chunk_overlap,
            files: Default::default(),
            vectors: Default::default(),
        }
    }

    pub fn get(&self, id: DocumentId) -> Option<&RagDocument> {
        let (file_index, document_index) = split_document_id(id);
        let file = self.files.get(file_index)?;
        let document = file.documents.get(document_index)?;
        Some(document)
    }

    pub fn add(
        &mut self,
        files: Vec<RagFile>,
        vector_ids: Vec<DocumentId>,
        embeddings: EmbeddingsOutput,
    ) {
        self.files.extend(files);
        self.vectors.extend(vector_ids.into_iter().zip(embeddings));
    }

    pub fn build_hnsw(&self) -> Hnsw<'static, f32, DistCosine> {
        let hnsw = Hnsw::new(32, self.vectors.len(), 16, 200, DistCosine {});
        let list: Vec<_> = self.vectors.iter().map(|(k, v)| (v, *k)).collect();
        hnsw.parallel_insert(&list);
        hnsw
    }

    pub fn build_bm25(&self) -> BM25<DocumentId> {
        let mut corpus = vec![];
        for (file_index, file) in self.files.iter().enumerate() {
            for (document_index, document) in file.documents.iter().enumerate() {
                let id = combine_document_id(file_index, document_index);
                corpus.push((id, document.page_content.clone()));
            }
        }
        BM25::new(corpus, BM25Options::default())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagFile {
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

    pub fn with_metadata(mut self, metadata: RagMetadata) -> Self {
        self.metadata = metadata;
        self
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
        .max_input_tokens()
        .map(|v| format!("The model's max_input_token is {v}"));

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

fn add_document_paths() -> Result<Vec<String>> {
    let text = Text::new("Add documents:")
        .with_validator(required!("This field is required"))
        .with_help_message("e.g. file;dir/;dir/**/*.md;url;sites/**")
        .prompt()?;
    let paths = text.split(';').map(|v| v.trim().to_string()).collect();
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
