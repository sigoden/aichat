use self::loader::*;
use self::splitter::*;

use crate::client::*;
use crate::config::*;
use crate::utils::*;

mod loader;
mod splitter;

use anyhow::bail;
use anyhow::{anyhow, Context, Result};
use hnsw_rs::prelude::*;
use indexmap::IndexMap;
use inquire::{required, validator::Validation, Select, Text};
use path_absolutize::Absolutize;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fmt::Debug;
use std::{io::BufReader, path::Path};
use tokio::sync::mpsc;

pub const TEMP_RAG_NAME: &str = "temp";
pub const CHUNK_OVERLAP: usize = 20;
pub const SIMILARITY_THRESHOLD: f32 = 0.25;

pub struct Rag {
    client: Box<dyn Client>,
    name: String,
    path: String,
    model: Model,
    hnsw: Hnsw<'static, f32, DistCosine>,
    data: RagData,
}

impl Debug for Rag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rag")
            .field("name", &self.name)
            .field("path", &self.path)
            .field("model", &self.model)
            .field("data", &self.data)
            .finish()
    }
}

impl Rag {
    pub async fn init(
        config: &GlobalConfig,
        name: &str,
        path: &Path,
        abort_signal: AbortSignal,
    ) -> Result<Self> {
        debug!("init rag: {name}");
        let model = select_embedding_model(config)?;
        let chunk_size = model.default_chunk_size();
        let chunk_size = set_chunk_size(chunk_size)?;
        let data = RagData::new(&model.id(), chunk_size);
        let mut rag = Self::create(config, name, path, data)?;
        let paths = add_document_paths()?;
        debug!("document paths: {paths:?}");
        let (stop_spinner_tx, set_spinner_message_tx) = run_spinner("Starting").await;
        tokio::select! {
            ret = rag.add_paths(&paths, Some(set_spinner_message_tx)) => {
                let _ = stop_spinner_tx.send(());
                ret?;
            }
            _ = watch_abort_signal(abort_signal) => {
                let _ = stop_spinner_tx.send(());
                bail!("Aborted!")
            },
        };
        if !rag.is_temp() {
            rag.save(path)?;
            println!("✨ Saved rag to '{}'", path.display());
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
        let model = retrieve_embedding_model(&config.read(), &data.model)?;
        let client = init_client(config, Some(model.clone()))?;
        let rag = Rag {
            client,
            name: name.to_string(),
            path: path.display().to_string(),
            data,
            model,
            hnsw,
        };
        Ok(rag)
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
            "model": self.model.id(),
            "chunk_size": self.data.chunk_size,
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
        abort_signal: AbortSignal,
    ) -> Result<String> {
        let (stop_spinner_tx, _) = run_spinner("Embedding").await;
        let ret = tokio::select! {
            ret = self.search_impl(text, top_k) => {
                ret
            }
            _ = watch_abort_signal(abort_signal) => {
                bail!("Aborted!")
            },
        };
        let _ = stop_spinner_tx.send(());
        let output = ret?.join("\n\n");
        Ok(output)
    }

    pub async fn add_paths<T: AsRef<Path>>(
        &mut self,
        paths: &[T],
        progress_tx: Option<mpsc::UnboundedSender<String>>,
    ) -> Result<()> {
        // List files
        let mut file_paths = vec![];
        progress(&progress_tx, "Listing paths".into());
        for path in paths {
            let path = path
                .as_ref()
                .absolutize()
                .with_context(|| anyhow!("Invalid path '{}'", path.as_ref().display()))?;
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
            list_files(&mut file_paths, Path::new(&path_str), suffixes).await?;
        }

        // Load files
        let mut rag_files = vec![];
        let file_paths_len = file_paths.len();
        progress(&progress_tx, format!("Loading files [1/{file_paths_len}]"));
        for path in file_paths {
            let extension = Path::new(&path)
                .extension()
                .map(|v| v.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let separator = autodetect_separator(&extension);
            let splitter = Splitter::new(self.data.chunk_size, CHUNK_OVERLAP, separator);
            let documents = load(&path, &extension)
                .await
                .with_context(|| format!("Failed to load text at '{path}'"))?;
            let documents =
                splitter.split_documents(&documents, &SplitterChunkHeaderOptions::default());
            rag_files.push(RagFile { path, documents });
            progress(
                &progress_tx,
                format!("Loading files [{}/{file_paths_len}]", rag_files.len()),
            );
        }

        if rag_files.is_empty() {
            return Ok(());
        }

        // Convert vectors
        let mut vector_ids = vec![];
        let mut texts = vec![];
        for (file_index, file) in rag_files.iter().enumerate() {
            for (document_index, doc) in file.documents.iter().enumerate() {
                vector_ids.push(combine_vector_id(file_index, document_index));
                texts.push(doc.page_content.clone())
            }
        }

        let embeddings_data = EmbeddingsData::new(texts, false);
        let embeddings = self
            .create_embeddings(embeddings_data, progress_tx.clone())
            .await?;

        self.data.add(rag_files, vector_ids, embeddings);
        progress(&progress_tx, "Building vector store".into());
        self.hnsw = self.data.build_hnsw();

        Ok(())
    }

    async fn search_impl(&self, text: &str, top_k: usize) -> Result<Vec<String>> {
        let splitter = Splitter::new(self.data.chunk_size, CHUNK_OVERLAP, &DEFAULT_SEPARATES);
        let texts = splitter.split_text(text);
        let embeddings_data = EmbeddingsData::new(texts, true);
        let embeddings = self.create_embeddings(embeddings_data, None).await?;
        let output = self
            .hnsw
            .parallel_search(&embeddings, top_k, 30)
            .into_iter()
            .flat_map(|list| {
                list.into_iter()
                    .filter_map(|v| {
                        if v.distance < SIMILARITY_THRESHOLD {
                            return None;
                        }
                        let (file_index, document_index) = split_vector_id(v.d_id);
                        let text = self.data.files[file_index].documents[document_index]
                            .page_content
                            .clone();
                        Some(text)
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        Ok(output)
    }

    async fn create_embeddings(
        &self,
        data: EmbeddingsData,
        progress_tx: Option<mpsc::UnboundedSender<String>>,
    ) -> Result<EmbeddingsOutput> {
        let EmbeddingsData { texts, query } = data;
        let mut output = vec![];
        let chunks = texts.chunks(self.model.max_concurrent_chunks());
        let chunks_len = chunks.len();
        progress(
            &progress_tx,
            format!("Creating embeddings [1/{chunks_len}]"),
        );
        for (index, texts) in chunks.enumerate() {
            let chunk_data = EmbeddingsData {
                texts: texts.to_vec(),
                query,
            };
            let chunk_output = self
                .client
                .embeddings(chunk_data)
                .await
                .context("Failed to create embedding")?;
            output.extend(chunk_output);
            progress(
                &progress_tx,
                format!("Creating embeddings [{}/{chunks_len}]", index + 1),
            );
        }
        Ok(output)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagData {
    pub model: String,
    pub chunk_size: usize,
    pub files: Vec<RagFile>,
    pub vectors: IndexMap<VectorID, Vec<f32>>,
}

impl RagData {
    pub fn new(model: &str, chunk_size: usize) -> Self {
        Self {
            model: model.to_string(),
            chunk_size,
            files: Default::default(),
            vectors: Default::default(),
        }
    }

    pub fn add(
        &mut self,
        files: Vec<RagFile>,
        vector_ids: Vec<VectorID>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagFile {
    path: String,
    documents: Vec<RagDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

    #[allow(unused)]
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

pub type VectorID = usize;

pub fn combine_vector_id(file_index: usize, document_index: usize) -> VectorID {
    file_index << (usize::BITS / 2) | document_index
}

pub fn split_vector_id(value: VectorID) -> (usize, usize) {
    let low_mask = (1 << (usize::BITS / 2)) - 1;
    let low = value & low_mask;
    let high = value >> (usize::BITS / 2);
    (high, low)
}

fn retrieve_embedding_model(config: &Config, model_id: &str) -> Result<Model> {
    let model = Model::find(&list_embedding_models(config), model_id)
        .ok_or_else(|| anyhow!("No embedding model '{model_id}'"))?;
    Ok(model)
}

fn select_embedding_model(config: &GlobalConfig) -> Result<Model> {
    let config = config.read();
    let model = match config.embedding_model.clone() {
        Some(model_id) => retrieve_embedding_model(&config, &model_id)?,
        None => {
            let models = list_embedding_models(&config);
            if models.is_empty() {
                bail!("No embedding model");
            }
            let model_ids: Vec<_> = models.iter().map(|v| v.id()).collect();
            let model_id = Select::new("Select embedding model:", model_ids).prompt()?;
            retrieve_embedding_model(&config, &model_id)?
        }
    };
    Ok(model)
}

fn set_chunk_size(chunk_size: usize) -> Result<usize> {
    let value = Text::new("Set chunk size:")
        .with_default(&chunk_size.to_string())
        .with_validator(move |text: &str| {
            let out = match text.parse::<usize>() {
                Ok(_) => Validation::Valid,
                Err(_) => Validation::Invalid("Must be a integer".into()),
            };
            Ok(out)
        })
        .prompt()?;
    value.parse().map_err(|_| anyhow!("Invalid chunk_size"))
}

fn add_document_paths() -> Result<Vec<String>> {
    let text = Text::new("Add document paths:")
        .with_validator(required!("This field is required"))
        .with_help_message("e.g. file1;dir2/;dir3/**/*.md")
        .prompt()?;
    let paths = text.split(';').map(|v| v.to_string()).collect();
    Ok(paths)
}

fn progress(spinner_message_tx: &Option<mpsc::UnboundedSender<String>>, message: String) {
    if let Some(tx) = spinner_message_tx {
        let _ = tx.send(message);
    }
}
