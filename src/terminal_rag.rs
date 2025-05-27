use anyhow::Result;
use std::sync::Arc;

use crate::config::GlobalConfig;
use crate::history_reader::CommandHistoryEntry;
use crate::client::RAGEmbeddingModel; // Assuming this or similar can be used for embeddings

use crate::client::{create_embedding_client, EmbeddingClient, Model, ModelType}; // Actual client for embeddings
use crate::utils::now_millis; // For potential timing/logging if needed

// Actual embedding vector type
pub type Embedding = Vec<f32>;

#[derive(Debug, Clone)]
pub struct IndexedCommandHistoryEntry {
    pub entry: CommandHistoryEntry,
    pub embedding: Embedding,
}

pub struct TerminalHistoryIndexer {
    indexed_entries: Vec<IndexedCommandHistoryEntry>,
    embedding_client: Arc<dyn EmbeddingClient>, // Use the actual EmbeddingClient trait
    model: Model, // Store the model info used for embeddings
}

impl TerminalHistoryIndexer {
    pub fn new(embedding_client: Arc<dyn EmbeddingClient>, model: Model) -> Self {
        Self {
            indexed_entries: Vec::new(),
            embedding_client,
            model,
        }
    }

    // build_index is now a method that populates the index for an existing instance.
    pub async fn build_index_from_entries(
        &mut self,
        entries: Vec<CommandHistoryEntry>,
        config: &GlobalConfig, // For include_timestamps setting
    ) -> Result<()> {
        self.indexed_entries.clear(); // Clear existing index before building
        
        let cfg_reader = config.read();
        let include_timestamps = cfg_reader.terminal_history_rag.include_timestamps;
        // No need to drop cfg_reader explicitly here if it's only used for include_timestamps

        let mut texts_to_embed = Vec::new();
        for entry in &entries { // Borrow entries first to build texts
            let mut text = entry.command.clone();
            if include_timestamps {
                if let Some(ts) = entry.timestamp {
                    text = format!("{}\n(executed around timestamp: {})", entry.command, ts);
                }
            }
            texts_to_embed.push(text);
        }

        let text_refs: Vec<&str> = texts_to_embed.iter().map(|s| s.as_str()).collect();
        // Use the client stored in self
        let embeddings = self.embedding_client.generate_embeddings(text_refs, self.model.name()).await?;

        if embeddings.len() != entries.len() {
            anyhow::bail!("Mismatch between number of entries and generated embeddings.");
        }

        for (entry, embedding) in entries.into_iter().zip(embeddings.into_iter()) {
            self.indexed_entries.push(IndexedCommandHistoryEntry {
                entry,
                embedding,
            });
        }
        
        Ok(())
    }

    pub async fn search(
        &self,
        query_text: &str,
        top_k: usize,
    ) -> Result<Vec<CommandHistoryEntry>> {
        if self.indexed_entries.is_empty() {
            return Ok(Vec::new());
        }

        // Embed the query_text using the stored embedding client and model name
        let query_embedding_vec = self.embedding_client.generate_embeddings(vec![query_text], self.model.name()).await?;
        if query_embedding_vec.is_empty() || query_embedding_vec[0].is_empty() {
            anyhow::bail!("Failed to generate embedding for query text or got empty embedding.");
        }
        let query_embedding = &query_embedding_vec[0];


        // Perform cosine similarity search (simplified)
        let mut scored_entries: Vec<(f32, &CommandHistoryEntry)> = self
            .indexed_entries
            .iter()
            .map(|indexed_entry| {
                let score = cosine_similarity(query_embedding, &indexed_entry.embedding);
                (score, &indexed_entry.entry)
            })
            .collect();

        // Sort by score in descending order
        scored_entries.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // Take top_k
        let results = scored_entries
            .into_iter()
            .filter(|(score, _)| *score > 0.0) // Optionally filter out zero/negative scores
            .take(top_k)
            .map(|(_, entry)| entry.clone())
            .collect();

        Ok(results)
    }
}

// Cosine similarity function
fn cosine_similarity(a: &Embedding, b: &Embedding) -> f32 {
    if a.is_empty() || b.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot_product / (norm_a * norm_b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, GlobalConfig, TerminalHistoryRagConfig, WorkingMode}; // For GlobalConfig
    use crate::history_reader::CommandHistoryEntry;
    use crate::client::{ClientConfig, EmbeddingClient, Model, ModelType, Message, MessageRole, MessageContent}; // For Model, EmbeddingClient
    use parking_lot::RwLock;
    use std::sync::Arc;
    use anyhow::Result;

    // --- Mock Embedding Client ---
    #[derive(Clone)]
    struct MockEmbeddingClient {
        embeddings_to_return: Vec<Embedding>, // Predefined embeddings to return
        expected_texts: Option<Vec<String>>,  // Optional: texts we expect to receive
    }

    impl MockEmbeddingClient {
        fn new(embeddings: Vec<Embedding>, expected_texts: Option<Vec<String>>) -> Self {
            Self { embeddings_to_return: embeddings, expected_texts }
        }
    }

    #[async_trait::async_trait]
    impl EmbeddingClient for MockEmbeddingClient {
        async fn generate_embeddings(&self, texts: Vec<&str>, _model_name: &str) -> Result<Vec<Embedding>> {
            if let Some(expected) = &self.expected_texts {
                let received_texts: Vec<String> = texts.iter().map(|s| s.to_string()).collect();
                assert_eq!(&received_texts, expected, "MockEmbeddingClient received unexpected texts.");
            }
            // Return a slice of the predefined embeddings, matching the number of input texts
            Ok(self.embeddings_to_return.iter().take(texts.len()).cloned().collect())
        }
    }

    // --- Test Helper for GlobalConfig ---
    fn create_test_global_config(terminal_history_config: TerminalHistoryRagConfig) -> GlobalConfig {
        let mut config = Config::default();
        config.terminal_history_rag = terminal_history_config;
        // Ensure a dummy rag_embedding_model is set so that Model::retrieve_model doesn't fail early
        // This model won't actually be used if we inject MockEmbeddingClient correctly.
        config.rag_embedding_model = Some("mock-embedding-model".to_string()); 
        Arc::new(RwLock::new(config))
    }
    
    // --- Helper to create a dummy Model for testing ---
    fn create_dummy_embedding_model() -> Model {
        Model::new(
            "test_client",
            "dummy-embedding-model",
            ModelType::Embedding,
            None,
            ClientConfig::default(), // Add default ClientConfig
            None, // max_input_tokens
            None, // max_output_tokens
            None, // max_texts
            false, // no_stream
            None, // default_chunk_size
            None, // max_batch_size
        )
    }


    #[tokio::test]
    async fn test_build_index_empty_entries() -> Result<()> {
        let mock_embeddings = vec![];
        let mock_client = Arc::new(MockEmbeddingClient::new(mock_embeddings, None));
        let dummy_model = create_dummy_embedding_model();
        let mut indexer = TerminalHistoryIndexer::new(mock_client, dummy_model);
        
        let entries = Vec::new();
        let global_config = create_test_global_config(TerminalHistoryRagConfig::default());
        
        indexer.build_index_from_entries(entries, &global_config).await?;
        assert_eq!(indexer.indexed_entries.len(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn test_build_index_with_entries() -> Result<()> {
        let entry1 = CommandHistoryEntry { command: "ls -l".to_string(), timestamp: Some(100), shell: "bash".to_string() };
        let entry2 = CommandHistoryEntry { command: "pwd".to_string(), timestamp: None, shell: "bash".to_string() };
        let entries = vec![entry1.clone(), entry2.clone()];

        let expected_texts = vec![
            "ls -l\n(executed around timestamp: 100)".to_string(), // Assuming include_timestamps = true
            "pwd".to_string(),
        ];
        let mock_embeddings = vec![vec![0.1, 0.2], vec![0.3, 0.4]];
        let mock_client = Arc::new(MockEmbeddingClient::new(mock_embeddings.clone(), Some(expected_texts)));
        let dummy_model = create_dummy_embedding_model();

        let mut indexer = TerminalHistoryIndexer::new(mock_client, dummy_model);
        let global_config = create_test_global_config(TerminalHistoryRagConfig {
            include_timestamps: true, ..Default::default()
        });

        indexer.build_index_from_entries(entries, &global_config).await?;
        
        assert_eq!(indexer.indexed_entries.len(), 2);
        assert_eq!(indexer.indexed_entries[0].entry, entry1);
        assert_eq!(indexer.indexed_entries[0].embedding, mock_embeddings[0]);
        assert_eq!(indexer.indexed_entries[1].entry, entry2);
        assert_eq!(indexer.indexed_entries[1].embedding, mock_embeddings[1]);
        Ok(())
    }

    #[tokio::test]
    async fn test_build_index_no_timestamps_in_text() -> Result<()> {
        let entry1 = CommandHistoryEntry { command: "git status".to_string(), timestamp: Some(200), shell: "zsh".to_string() };
        let entries = vec![entry1.clone()];

        let expected_texts = vec!["git status".to_string()]; // Timestamps not included in text
        let mock_embeddings = vec![vec![0.5, 0.6]];
        let mock_client = Arc::new(MockEmbeddingClient::new(mock_embeddings.clone(), Some(expected_texts)));
        let dummy_model = create_dummy_embedding_model();
        let mut indexer = TerminalHistoryIndexer::new(mock_client, dummy_model);
        
        let global_config = create_test_global_config(TerminalHistoryRagConfig {
            include_timestamps: false, // Key for this test
            ..Default::default()
        });

        indexer.build_index_from_entries(entries, &global_config).await?;
        
        assert_eq!(indexer.indexed_entries.len(), 1);
        assert_eq!(indexer.indexed_entries[0].entry, entry1); // Entry still has timestamp
        assert_eq!(indexer.indexed_entries[0].embedding, mock_embeddings[0]);
        // The text used for embedding was "git status", not the one with timestamp.
        Ok(())
    }

    #[tokio::test]
    async fn test_search_empty_index() -> Result<()> {
        let mock_client = Arc::new(MockEmbeddingClient::new(vec![], None));
        let dummy_model = create_dummy_embedding_model();
        let indexer = TerminalHistoryIndexer::new(mock_client, dummy_model); // Index is empty

        let results = indexer.search("any query", 3).await?;
        assert!(results.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn test_search_simple_retrieval() -> Result<()> {
        let cmd1_text = "ls -la";
        let cmd2_text = "docker ps";
        let query_text = "list containers";

        // Mock embeddings: query, cmd1, cmd2
        // Let query be most similar to cmd2_text
        let query_embedding = vec![0.1, 0.8, 0.1];
        let cmd1_embedding = vec![0.7, 0.1, 0.1]; // less similar
        let cmd2_embedding = vec![0.1, 0.7, 0.2]; // more similar
        
        // Mock client will return query_embedding when search() calls it for the query.
        // For build_index, it will return cmd1_embedding and cmd2_embedding.
        let mock_client_for_search = Arc::new(MockEmbeddingClient::new(vec![query_embedding.clone()], Some(vec![query_text.to_string()])));
        
        // Setup indexer with pre-defined embeddings for entries
        let entry1 = CommandHistoryEntry { command: cmd1_text.to_string(), timestamp: None, shell: "bash".to_string() };
        let entry2 = CommandHistoryEntry { command: cmd2_text.to_string(), timestamp: None, shell: "bash".to_string() };
        
        let mut indexer = TerminalHistoryIndexer::new(mock_client_for_search.clone(), create_dummy_embedding_model());
        // Manually insert indexed entries for this test, as build_index_from_entries would need its own mock client setup for that phase.
        indexer.indexed_entries = vec![
            IndexedCommandHistoryEntry { entry: entry1.clone(), embedding: cmd1_embedding.clone() },
            IndexedCommandHistoryEntry { entry: entry2.clone(), embedding: cmd2_embedding.clone() },
        ];

        let results = indexer.search(query_text, 1).await?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].command, cmd2_text); // cmd2 should be more similar

        let results_top2 = indexer.search(query_text, 2).await?;
        assert_eq!(results_top2.len(), 2);
        assert_eq!(results_top2[0].command, cmd2_text); // cmd2 first
        assert_eq!(results_top2[1].command, cmd1_text); // then cmd1
        
        Ok(())
    }
     #[tokio::test]
    async fn test_search_filters_by_top_k() -> Result<()> {
        let query_text = "query";
        let query_embedding = vec![1.0, 0.0, 0.0];

        let entries_data = vec![
            ("cmd1", vec![0.9, 0.1, 0.0]), // score ~0.9
            ("cmd2", vec![0.8, 0.2, 0.0]), // score ~0.8
            ("cmd3", vec![0.7, 0.3, 0.0]), // score ~0.7
            ("cmd4", vec![0.6, 0.4, 0.0]), // score ~0.6
        ];

        let mock_client = Arc::new(MockEmbeddingClient::new(vec![query_embedding.clone()], Some(vec![query_text.to_string()])));
        let dummy_model = create_dummy_embedding_model();
        let mut indexer = TerminalHistoryIndexer::new(mock_client, dummy_model);

        for (cmd, emb) in entries_data {
            indexer.indexed_entries.push(IndexedCommandHistoryEntry {
                entry: CommandHistoryEntry { command: cmd.to_string(), timestamp: None, shell: "bash".to_string() },
                embedding: emb,
            });
        }
        
        // Test top_k = 2
        let results_top2 = indexer.search(query_text, 2).await?;
        assert_eq!(results_top2.len(), 2);
        assert_eq!(results_top2[0].command, "cmd1");
        assert_eq!(results_top2[1].command, "cmd2");

        // Test top_k = 1
        let results_top1 = indexer.search(query_text, 1).await?;
        assert_eq!(results_top1.len(), 1);
        assert_eq!(results_top1[0].command, "cmd1");
        
        // Test top_k > num_items
        let results_top_all = indexer.search(query_text, 5).await?;
        assert_eq!(results_top_all.len(), 4);
        assert_eq!(results_top_all[0].command, "cmd1");

        Ok(())
    }
}
