use super::*;

use crate::client::{
    init_client, patch_messages, ChatCompletionsData, Client, ImageUrl, Message, MessageContent,
    MessageContentPart, MessageContentToolCalls, MessageRole, Model,
};
use crate::function::{ToolCall, ToolResult};
use crate::utils::{
    abortable_run_with_spinner, base64_encode, is_loader_protocol, sha256, strip_think_tag,
    AbortSignal,
};

use anyhow::{bail, Context, Result};
use indexmap::IndexSet;
use log::warn;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::Read,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const IMAGE_EXTS: [&str; 5] = ["png", "jpeg", "jpg", "webp", "gif"];
const SUMMARY_MAX_WIDTH: usize = 80;
const MAX_RESEARCH_CYCLES: usize = 3;
const MAX_RESULT_PREVIEW_CHARS: usize = 2000;
const MAX_COMBINED_SUMMARY_CHARS: usize = 4000;

#[derive(Debug, Clone)]
pub struct Input {
    config: GlobalConfig,
    text: String,
    raw: (String, Vec<String>),
    patched_text: Option<String>,
    last_reply: Option<String>,
    continue_output: Option<String>,
    regenerate: bool,
    medias: Vec<String>,
    data_urls: HashMap<String, String>,
    tool_calls: Option<MessageContentToolCalls>,
    role: Role,
    rag_name: Option<String>,
    with_session: bool,
    with_agent: bool,
    history_rag_context: Option<String>,
    web_research_enriched: bool,
}

impl Input {
    pub fn from_str(config: &GlobalConfig, text: &str, role: Option<Role>) -> Self {
        let (role, with_session, with_agent) = resolve_role(&config.read(), role);
        Self {
            config: config.clone(),
            text: text.to_string(),
            raw: (text.to_string(), vec![]),
            patched_text: None,
            last_reply: None,
            continue_output: None,
            regenerate: false,
            medias: Default::default(),
            data_urls: Default::default(),
            tool_calls: None,
            role,
            rag_name: None,
            with_session,
            with_agent,
            history_rag_context: None,
            web_research_enriched: false,
        }
    }

    pub async fn from_files(
        config: &GlobalConfig,
        raw_text: &str,
        paths: Vec<String>,
        role: Option<Role>,
    ) -> Result<Self> {
        let loaders = config.read().document_loaders.clone();
        let (raw_paths, local_paths, remote_urls, external_cmds, protocol_paths, with_last_reply) =
            resolve_paths(&loaders, paths)?;
        let mut last_reply = None;
        let (documents, medias, data_urls) = load_documents(
            &loaders,
            local_paths,
            remote_urls,
            external_cmds,
            protocol_paths,
        )
        .await
        .context("Failed to load files")?;
        let mut texts = vec![];
        if !raw_text.is_empty() {
            texts.push(raw_text.to_string());
        };
        if with_last_reply {
            if let Some(LastMessage { input, output, .. }) = config.read().last_message.as_ref() {
                if !output.is_empty() {
                    last_reply = Some(output.clone())
                } else if let Some(v) = input.last_reply.as_ref() {
                    last_reply = Some(v.clone());
                }
                if let Some(v) = last_reply.clone() {
                    texts.push(format!("\n{v}"));
                }
            }
            if last_reply.is_none() && documents.is_empty() && medias.is_empty() {
                bail!("No last reply found");
            }
        }
        let documents_len = documents.len();
        for (kind, path, contents) in documents {
            if documents_len == 1 {
                texts.push(format!("\n{contents}"));
            } else {
                texts.push(format!(
                    "\n============ {kind}: {path} ============\n{contents}"
                ));
            }
        }
        let (role, with_session, with_agent) = resolve_role(&config.read(), role);
        Ok(Self {
            config: config.clone(),
            text: texts.join("\n"),
            raw: (raw_text.to_string(), raw_paths),
            patched_text: None,
            last_reply,
            continue_output: None,
            regenerate: false,
            medias,
            data_urls,
            tool_calls: Default::default(),
            role,
            rag_name: None,
            with_session,
            with_agent,
            history_rag_context: None,
            web_research_enriched: false,
        })
    }

    pub async fn from_files_with_spinner(
        config: &GlobalConfig,
        raw_text: &str,
        paths: Vec<String>,
        role: Option<Role>,
        abort_signal: AbortSignal,
    ) -> Result<Self> {
        abortable_run_with_spinner(
            Input::from_files(config, raw_text, paths, role),
            "Loading files",
            abort_signal,
        )
        .await
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.medias.is_empty()
    }

    pub fn data_urls(&self) -> HashMap<String, String> {
        self.data_urls.clone()
    }

    pub fn tool_calls(&self) -> &Option<MessageContentToolCalls> {
        &self.tool_calls
    }

    pub fn text(&self) -> String {
        match self.patched_text.clone() {
            Some(text) => text,
            None => self.text.clone(),
        }
    }

    pub fn clear_patch(&mut self) {
        self.patched_text = None;
        self.web_research_enriched = false;
    }

    pub fn set_text(&mut self, text: String) {
        self.text = text;
    }

    pub fn stream(&self) -> bool {
        self.config.read().stream && !self.role().model().no_stream()
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

    pub fn set_history_rag_context(&mut self, context: String) {
        if !context.is_empty() {
            self.history_rag_context = Some(context);
        } else {
            self.history_rag_context = None;
        }
    }

    pub fn merge_tool_results(mut self, output: String, tool_results: Vec<ToolResult>) -> Self {
        match self.tool_calls.as_mut() {
            Some(exist_tool_results) => {
                exist_tool_results.merge(tool_results, output);
            }
            None => self.tool_calls = Some(MessageContentToolCalls::new(tool_results, output)),
        }
        self
    }

    pub fn create_client(&self) -> Result<Box<dyn Client>> {
        init_client(&self.config, Some(self.role().model().clone()))
    }

    pub async fn fetch_chat_text(&self) -> Result<String> {
        let client = self.create_client()?;
        let text = client.chat_completions(self.clone()).await?.text;
        let text = strip_think_tag(&text).to_string();
        Ok(text)
    }

    pub async fn integrate_web_research(
        &mut self,
        client: &dyn Client,
        abort_signal: AbortSignal,
    ) -> Result<()> {
        if self.web_research_enriched {
            return Ok(());
        }

        let request_text = self.text();
        if request_text.trim().is_empty() {
            self.web_research_enriched = true;
            return Ok(());
        }

        let (plan, raw_plan_text) = self
            .research_planning_stage(client, &request_text, abort_signal.clone())
            .await
            .unwrap_or_else(|err| {
                warn!("web research planning failed: {err}");
                (
                    ResearchPlanData {
                        should_search: false,
                        queries: vec![],
                        rationale: format!("planning failed: {err}"),
                        stopping_condition: Some(
                            "fall back to internal knowledge because planning was unavailable"
                                .to_string(),
                        ),
                    },
                    String::new(),
                )
            });

        let mut dossier_sections = vec![format!("planning notes: {}", plan.rationale)];
        let mut executed_query_labels = vec![];
        let mut summary_sections: Vec<String> = vec![];
        let mut final_evaluation: Option<ResearchEvaluationData> = None;
        let mut evaluation_raw = String::new();

        let initial_queries = normalize_queries(plan.queries.clone());
        if plan.should_search && !initial_queries.is_empty() {
            let mut executed_queries: HashSet<String> = HashSet::new();
            let mut next_queries = initial_queries.clone();

            for cycle in 0..MAX_RESEARCH_CYCLES {
                let iteration_index = cycle + 1;
                let current_queries: Vec<String> = next_queries
                    .into_iter()
                    .filter_map(|q| {
                        let trimmed = q.trim().to_string();
                        if trimmed.is_empty() {
                            None
                        } else if executed_queries.insert(trimmed.clone()) {
                            Some(trimmed)
                        } else {
                            None
                        }
                    })
                    .collect();

                if current_queries.is_empty() {
                    break;
                }

                let batch_results = self
                    .collect_search_results(&current_queries, iteration_index, abort_signal.clone())
                    .await;

                if batch_results.is_empty() {
                    dossier_sections.push(format!(
                        "iteration {} search attempts produced no usable results.",
                        iteration_index
                    ));
                    break;
                }

                executed_query_labels.extend(
                    batch_results
                        .iter()
                        .map(|bundle| format!("{}: {}", bundle.label, bundle.query.clone())),
                );

                let compression = match self
                    .research_compression_stage(
                        client,
                        &request_text,
                        iteration_index,
                        &batch_results,
                        abort_signal.clone(),
                    )
                    .await
                {
                    Ok(summary) => summary,
                    Err(err) => {
                        warn!("web research compression failed: {err}");
                        dossier_sections.push(format!(
                            "iteration {} compression failed: {}",
                            iteration_index, err
                        ));
                        break;
                    }
                };

                summary_sections.push(compression.clone());
                dossier_sections.push(format!(
                    "iteration {} summary:\n{}",
                    iteration_index, compression
                ));

                let combined_summary =
                    clamp_text(&summary_sections.join("\n\n"), MAX_COMBINED_SUMMARY_CHARS);

                match self
                    .research_evaluation_stage(
                        client,
                        &request_text,
                        &plan,
                        &combined_summary,
                        iteration_index,
                        abort_signal.clone(),
                    )
                    .await
                {
                    Ok((evaluation, raw)) => {
                        evaluation_raw = raw;
                        dossier_sections.push(format!(
                            "iteration {} evaluation: {} (confidence: {})",
                            iteration_index, evaluation.justification, evaluation.confidence
                        ));
                        let search_needed = !evaluation.sufficient
                            && evaluation
                                .additional_queries
                                .iter()
                                .any(|q| !executed_queries.contains(q.trim()));
                        next_queries = normalize_queries(evaluation.additional_queries.clone());
                        final_evaluation = Some(evaluation);
                        if !search_needed {
                            break;
                        }
                    }
                    Err(err) => {
                        warn!("web research evaluation failed: {err}");
                        dossier_sections.push(format!(
                            "iteration {} evaluation failed: {}",
                            iteration_index, err
                        ));
                        break;
                    }
                }
            }
        } else {
            if plan.should_search {
                dossier_sections.push(
                    "planning recommended research but produced no actionable queries.".to_string(),
                );
            } else {
                dossier_sections.push(
                    "planning determined that existing knowledge is sufficient without web search."
                        .to_string(),
                );
            }
        }

        if executed_query_labels.is_empty() {
            dossier_sections.push("executed searches: none".to_string());
        } else {
            dossier_sections.push(format!(
                "executed searches: {}",
                executed_query_labels.join("; ")
            ));
        }

        if summary_sections.is_empty() {
            dossier_sections.push("combined evidence summary: none".to_string());
        } else {
            let combined = clamp_text(&summary_sections.join("\n\n"), MAX_COMBINED_SUMMARY_CHARS);
            dossier_sections.push(format!("combined evidence summary:\n{}", combined));
        }

        let final_evaluation = final_evaluation.unwrap_or_else(|| ResearchEvaluationData {
            sufficient: !plan.should_search || initial_queries.is_empty(),
            justification: if plan.should_search && initial_queries.is_empty() {
                "proceeding without web data due to lack of actionable queries.".to_string()
            } else {
                "ready to respond with available context.".to_string()
            },
            confidence: if plan.should_search && initial_queries.is_empty() {
                "medium".to_string()
            } else {
                "high".to_string()
            },
            additional_queries: vec![],
        });

        dossier_sections.push(format!(
            "final research readiness: {} (confidence: {}).",
            final_evaluation.justification, final_evaluation.confidence
        ));

        if !final_evaluation.additional_queries.is_empty() {
            dossier_sections.push(format!(
                "remaining research ideas: {}",
                final_evaluation.additional_queries.join("; ")
            ));
        }

        if !evaluation_raw.is_empty() {
            dossier_sections.push(format!("evaluation details: {}", evaluation_raw));
        }

        let plan_stop = plan
            .stopping_condition
            .clone()
            .unwrap_or_else(|| "no explicit stopping condition provided.".to_string());

        let initial_query_line = if initial_queries.is_empty() {
            "initial queries: none".to_string()
        } else {
            format!("initial queries: {}", initial_queries.join("; "))
        };

        let mut research_block = format!(
            "[integrated web research]\nplan rationale: {}\n{}\nstopping condition: {}",
            plan.rationale, initial_query_line, plan_stop
        );

        if !raw_plan_text.is_empty() {
            research_block.push_str(&format!("\nplanner output: {}", raw_plan_text));
        }

        research_block.push('\n');
        research_block.push_str(&dossier_sections.join("\n"));

        let base_text = self.text();
        let enriched_text = format!("{}\n\n{}", research_block, base_text);
        self.patched_text = Some(enriched_text);
        self.web_research_enriched = true;
        Ok(())
    }

    async fn research_planning_stage(
        &self,
        client: &dyn Client,
        request_text: &str,
        abort_signal: AbortSignal,
    ) -> Result<(ResearchPlanData, String)> {
        let mut planner_role = Role::new(
            "%integrated-web-research-plan%",
            "you are Lee Daghlar Ostadi's research planner. decide if web search is required before answering. always return strict JSON with fields should_search, queries, rationale, stopping_condition. limit queries to at most three targeted strings.",
        );
        planner_role.set_model(self.role().model());
        planner_role.set_temperature(Some(0.2));

        let planner_prompt = format!(
            "Original request:\n{request_text}\n\nReturn JSON as specified without extra commentary.",
        );
        let planner_input = Input::from_str(&self.config, &planner_prompt, Some(planner_role));
        let output = abortable_run_with_spinner(
            client.chat_completions(planner_input),
            "Planning web research",
            abort_signal,
        )
        .await?;
        let cleaned = strip_think_tag(&output.text).trim().to_string();
        let mut plan = parse_json_object::<ResearchPlanData>(&cleaned).unwrap_or_else(|| ResearchPlanData {
            should_search: false,
            queries: vec![],
            rationale: cleaned.clone(),
            stopping_condition: Some(
                "planner returned unstructured text; rely on existing knowledge unless more context is requested.".to_string(),
            ),
        });
        if plan.rationale.trim().is_empty() {
            plan.rationale = "planner did not provide a rationale.".to_string();
        }
        if plan.stopping_condition.is_none() {
            plan.stopping_condition =
                Some("planner did not define a stopping condition.".to_string());
        }
        Ok((plan, cleaned))
    }

    async fn collect_search_results(
        &self,
        queries: &[String],
        iteration_index: usize,
        abort_signal: AbortSignal,
    ) -> Vec<SearchResultBundle> {
        let mut results = vec![];
        for (idx, query) in queries.iter().enumerate() {
            let label = format!("Q{}-{}", iteration_index, idx + 1);
            let cloned_query = query.clone();
            let config = self.config.clone();
            let tool_call = ToolCall::new(
                "web_search".to_string(),
                json!({ "query": cloned_query }),
                None,
            );
            let spinner_label = format!("Searching web: {}", query);
            match abortable_run_with_spinner(
                async move {
                    let value = tool_call.eval(&config)?;
                    Ok::<Value, anyhow::Error>(value)
                },
                &spinner_label,
                abort_signal.clone(),
            )
            .await
            {
                Ok(value) => results.push(SearchResultBundle {
                    label,
                    query: query.clone(),
                    payload: value,
                }),
                Err(err) => {
                    warn!("web search for '{query}' failed: {err}");
                    results.push(SearchResultBundle {
                        label,
                        query: query.clone(),
                        payload: json!({
                            "error": err.to_string(),
                        }),
                    });
                }
            }
        }
        results
    }

    async fn research_compression_stage(
        &self,
        client: &dyn Client,
        request_text: &str,
        iteration_index: usize,
        results: &[SearchResultBundle],
        abort_signal: AbortSignal,
    ) -> Result<String> {
        let mut compression_role = Role::new(
            "%integrated-web-research-compress%",
            "you are Lee Daghlar Ostadi's research compression specialist. distill the gathered evidence into a compact digest. cite query labels like [Q1-1]. keep the summary under 250 words.",
        );
        compression_role.set_model(self.role().model());
        compression_role.set_temperature(Some(0.4));

        let evidence = format_search_evidence(results);
        let compression_prompt = format!(
            "Research target:\n{request_text}\n\nEvidence batch {iteration_index}:\n{evidence}\n\nProduce a concise digest that references the query labels and highlights reliable findings, conflicts, and gaps.",
        );
        let compression_input =
            Input::from_str(&self.config, &compression_prompt, Some(compression_role));
        let output = abortable_run_with_spinner(
            client.chat_completions(compression_input),
            "Compressing research evidence",
            abort_signal,
        )
        .await?;
        Ok(strip_think_tag(&output.text).trim().to_string())
    }

    async fn research_evaluation_stage(
        &self,
        client: &dyn Client,
        request_text: &str,
        plan: &ResearchPlanData,
        combined_summary: &str,
        iteration_index: usize,
        abort_signal: AbortSignal,
    ) -> Result<(ResearchEvaluationData, String)> {
        let mut evaluator_role = Role::new(
            "%integrated-web-research-eval%",
            "you are Lee Daghlar Ostadi's evidence reviewer. decide if the gathered research is sufficient to answer with confidence. respond with JSON containing sufficient, justification, confidence, additional_queries. suggest at most two additional_queries when more work is needed.",
        );
        evaluator_role.set_model(self.role().model());
        evaluator_role.set_temperature(Some(0.2));

        let stop_clause = plan
            .stopping_condition
            .clone()
            .unwrap_or_else(|| "no stopping condition provided.".to_string());

        let evaluation_prompt = format!(
            "Research target:\n{request_text}\n\nDeclared stopping condition: {stop_clause}\n\nEvidence digest after iteration {iteration_index}:\n{combined_summary}\n\nReturn strict JSON with the required fields and no commentary.",
        );
        let evaluation_input =
            Input::from_str(&self.config, &evaluation_prompt, Some(evaluator_role));
        let output = abortable_run_with_spinner(
            client.chat_completions(evaluation_input),
            "Evaluating research coverage",
            abort_signal,
        )
        .await?;
        let cleaned = strip_think_tag(&output.text).trim().to_string();
        let mut evaluation =
            parse_json_object::<ResearchEvaluationData>(&cleaned).unwrap_or_else(|| {
                ResearchEvaluationData {
                    sufficient: false,
                    justification: cleaned.clone(),
                    confidence: "low".to_string(),
                    additional_queries: vec![],
                }
            });
        if evaluation.justification.trim().is_empty() {
            evaluation.justification = "the evaluator returned no justification.".to_string();
        }
        if evaluation.confidence.trim().is_empty() {
            evaluation.confidence = "low".to_string();
        }
        Ok((evaluation, cleaned))
    }

    pub fn prepare_completion_data(
        &self,
        model: &Model,
        stream: bool,
    ) -> Result<ChatCompletionsData> {
        let mut messages = self.build_messages()?;
        patch_messages(&mut messages, model);
        model.guard_max_input_tokens(&messages)?;
        let (temperature, top_p) = (self.role().temperature(), self.role().top_p());
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
        // Create a temporary Input that might have text augmented with history_rag_context
        let mut temp_input_for_messages = self.clone();

        let original_text = self.text(); // Uses patched_text if available, else self.text
        if let Some(ctx) = &self.history_rag_context {
            // Important: We need to decide how file RAG (patched_text) and history RAG interact.
            // Option 1: History RAG context prepends to whatever text() returns (including file RAG).
            // Option 2: They are separate, and the LLM gets both contexts distinctly.
            // For now, let's go with Option 1 for simplicity of integration into existing flow.
            temp_input_for_messages.text = format!("{}\n\n{}", ctx, original_text);
            // If original_text was from self.patched_text, we are effectively discarding
            // self.text and using ctx + self.patched_text.
            // If original_text was from self.text, we are using ctx + self.text.
            // This also means `temp_input_for_messages.patched_text` should be None to ensure its `text()` method
            // returns this combined text.
            temp_input_for_messages.patched_text = None;
        }

        let mut messages = if let Some(session) = self.session(&self.config.read().session) {
            session.build_messages(&temp_input_for_messages)
        } else {
            self.role().build_messages(&temp_input_for_messages)
        };
        if let Some(tool_calls) = &self.tool_calls {
            messages.push(Message::new(
                MessageRole::Assistant,
                MessageContent::ToolCalls(tool_calls.clone()),
            ))
        }
        Ok(messages)
    }

    pub fn echo_messages(&self) -> String {
        // For echoing, we should probably show the context as well if it's going to be used.
        let mut temp_input_for_echo = self.clone();
        let original_text_for_echo = self.text(); // Similar logic to build_messages
        if let Some(ctx) = &self.history_rag_context {
            temp_input_for_echo.text = format!("{}\n\n{}", ctx, original_text_for_echo);
            temp_input_for_echo.patched_text = None;
        }

        if let Some(session) = self.session(&self.config.read().session) {
            session.echo_messages(&temp_input_for_echo)
        } else {
            self.role().echo_messages(&temp_input_for_echo)
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
        if text.width_cjk() > SUMMARY_MAX_WIDTH {
            let mut sum_width = 0;
            let mut chars = vec![];
            for c in text.chars() {
                sum_width += c.width_cjk().unwrap_or(1);
                if sum_width > SUMMARY_MAX_WIDTH - 3 {
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

    pub fn raw(&self) -> String {
        let (text, files) = &self.raw;
        let mut segments = files.to_vec();
        if !segments.is_empty() {
            segments.insert(0, ".file".into());
        }
        if !text.is_empty() {
            if !segments.is_empty() {
                segments.push("--".into());
            }
            segments.push(text.clone());
        }
        segments.join(" ")
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

#[derive(Debug, Default, Deserialize, Clone)]
struct ResearchPlanData {
    #[serde(default)]
    should_search: bool,
    #[serde(default)]
    queries: Vec<String>,
    #[serde(default)]
    rationale: String,
    #[serde(default)]
    stopping_condition: Option<String>,
}

#[derive(Debug, Default, Deserialize, Clone)]
struct ResearchEvaluationData {
    #[serde(default)]
    sufficient: bool,
    #[serde(default)]
    justification: String,
    #[serde(default)]
    confidence: String,
    #[serde(default)]
    additional_queries: Vec<String>,
}

#[derive(Debug, Clone)]
struct SearchResultBundle {
    label: String,
    query: String,
    payload: Value,
}

fn normalize_queries(queries: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = vec![];
    for query in queries {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            continue;
        }
        let canonical = trimmed.to_string();
        if seen.insert(canonical.clone()) {
            normalized.push(canonical);
        }
    }
    normalized
}

fn clamp_text(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        return text.to_string();
    }
    let safe_len = max_len.saturating_sub(3);
    let mut truncated = text.chars().take(safe_len).collect::<String>();
    truncated.push_str("...");
    truncated
}

fn format_search_evidence(results: &[SearchResultBundle]) -> String {
    let mut sections = vec![];
    for result in results {
        let mut payload_text = serde_json::to_string_pretty(&result.payload)
            .unwrap_or_else(|_| result.payload.to_string());
        payload_text = clamp_text(&payload_text, MAX_RESULT_PREVIEW_CHARS);
        sections.push(format!(
            "{} :: query: {}\n{}",
            result.label, result.query, payload_text
        ));
    }
    sections.join("\n\n")
}

fn parse_json_object<T>(text: &str) -> Option<T>
where
    T: DeserializeOwned,
{
    if let Ok(value) = serde_json::from_str(text) {
        return Some(value);
    }
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    serde_json::from_str(&text[start..=end]).ok()
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

type ResolvePathsOutput = (
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    bool,
);

fn resolve_paths(
    loaders: &HashMap<String, String>,
    paths: Vec<String>,
) -> Result<ResolvePathsOutput> {
    let mut raw_paths = IndexSet::new();
    let mut local_paths = IndexSet::new();
    let mut remote_urls = IndexSet::new();
    let mut external_cmds = IndexSet::new();
    let mut protocol_paths = IndexSet::new();
    let mut with_last_reply = false;
    for path in paths {
        if path == "%%" {
            with_last_reply = true;
            raw_paths.insert(path);
        } else if path.starts_with('`') && path.len() > 2 && path.ends_with('`') {
            external_cmds.insert(path[1..path.len() - 1].to_string());
            raw_paths.insert(path);
        } else if is_url(&path) {
            if path.strip_suffix("**").is_some() {
                bail!("Invalid website '{path}'");
            }
            remote_urls.insert(path.clone());
            raw_paths.insert(path);
        } else if is_loader_protocol(loaders, &path) {
            protocol_paths.insert(path.clone());
            raw_paths.insert(path);
        } else {
            let resolved_path = resolve_home_dir(&path);
            let absolute_path = to_absolute_path(&resolved_path)
                .with_context(|| format!("Invalid path '{path}'"))?;
            local_paths.insert(resolved_path);
            raw_paths.insert(absolute_path);
        }
    }
    Ok((
        raw_paths.into_iter().collect(),
        local_paths.into_iter().collect(),
        remote_urls.into_iter().collect(),
        external_cmds.into_iter().collect(),
        protocol_paths.into_iter().collect(),
        with_last_reply,
    ))
}

async fn load_documents(
    loaders: &HashMap<String, String>,
    local_paths: Vec<String>,
    remote_urls: Vec<String>,
    external_cmds: Vec<String>,
    protocol_paths: Vec<String>,
) -> Result<(
    Vec<(&'static str, String, String)>,
    Vec<String>,
    HashMap<String, String>,
)> {
    let mut files = vec![];
    let mut medias = vec![];
    let mut data_urls = HashMap::new();

    for cmd in external_cmds {
        let (success, stdout, stderr) =
            run_command_with_output(&SHELL.cmd, &[&SHELL.arg, &cmd], None)?;
        if !success {
            let err = if !stderr.is_empty() { stderr } else { stdout };
            bail!("Failed to run `{cmd}`\n{err}");
        }
        files.push(("CMD", cmd, stdout));
    }

    let local_files = expand_glob_paths(&local_paths, true).await?;
    for file_path in local_files {
        if is_image(&file_path) {
            let contents = read_media_to_data_url(&file_path)
                .with_context(|| format!("Unable to read media '{file_path}'"))?;
            data_urls.insert(sha256(&contents), file_path);
            medias.push(contents)
        } else {
            let document = load_file(loaders, &file_path)
                .await
                .with_context(|| format!("Unable to read file '{file_path}'"))?;
            files.push(("FILE", file_path, document.contents));
        }
    }

    for file_url in remote_urls {
        let (contents, extension) = fetch_with_loaders(loaders, &file_url, true)
            .await
            .with_context(|| format!("Failed to load url '{file_url}'"))?;
        if extension == MEDIA_URL_EXTENSION {
            data_urls.insert(sha256(&contents), file_url);
            medias.push(contents)
        } else {
            files.push(("URL", file_url, contents));
        }
    }

    for protocol_path in protocol_paths {
        let documents = load_protocol_path(loaders, &protocol_path)
            .with_context(|| format!("Failed to load from '{protocol_path}'"))?;
        files.extend(
            documents
                .into_iter()
                .map(|document| ("FROM", document.path, document.contents)),
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{ClientConfig, MessageContent, MessageRole, Model, ModelType};
    use crate::config::{
        Config, GlobalConfig, Role, WorkingMode, RIGOROUS_INTELLECTUAL_PRECISION_ROLE_NAME,
    };
    use parking_lot::RwLock;
    use std::fs;
    use std::sync::Arc;

    fn create_test_config_for_input_tests() -> GlobalConfig {
        let mut config = Config::default();
        // Ensure the rigorous role is set as default for prelude
        config.repl_prelude = Some(format!(
            "role:{}",
            RIGOROUS_INTELLECTUAL_PRECISION_ROLE_NAME
        ));
        config.cmd_prelude = Some(format!(
            "role:{}",
            RIGOROUS_INTELLECTUAL_PRECISION_ROLE_NAME
        ));

        // Manually load the functions because Config::init() is not fully called here.
        // This is important if roles/functions might interact or if model setup depends on it.
        // For this specific test, only role loading matters.
        if let Ok(functions) = Functions::init(&Config::functions_file()) {
            config.functions = functions;
        } else {
            // In a real test setup, might panic or handle error
            eprintln!("Warning: Could not load functions for input test config.");
        }

        Arc::new(RwLock::new(config))
    }

    fn create_dummy_chat_model() -> Model {
        Model::new(
            "test_client",
            "dummy-chat-model",
            ModelType::Chat,
            None,
            ClientConfig::default(),
            Some(4096), // max_input_tokens
            None,       // max_output_tokens
            None,       // max_texts
            false,      // no_stream
            None,       // default_chunk_size
            None,       // max_batch_size
        )
    }

    #[tokio::test]
    async fn test_default_system_prompt_application() -> Result<()> {
        let global_config = create_test_config_for_input_tests();

        // Apply prelude to load the default role
        global_config.write().apply_prelude()?;

        // Create a simple input; it should pick up the default role from global_config
        let input_text = "Hello, world!";
        let input = Input::from_str(&global_config, input_text, None);

        // Prepare completion data
        let dummy_model = create_dummy_chat_model(); // Assuming Role's model isn't critical here, or is set by prelude
        let completion_data = input.prepare_completion_data(&dummy_model, false)?;

        // Assertions
        assert!(
            !completion_data.messages.is_empty(),
            "Messages list should not be empty"
        );

        let first_message = &completion_data.messages[0];
        assert_eq!(
            first_message.role,
            MessageRole::System,
            "First message should be a System message"
        );

        // Load the expected system prompt from the file
        // Note: This assumes the test runner is in the project root.
        let expected_prompt_content = fs::read_to_string(format!("assets/roles/{}.md", RIGOROUS_INTELLECTUAL_PRECISION_ROLE_NAME))
            .with_context(|| format!("Failed to read the rigorous precision role file for test comparison. Current dir: {:?}", std::env::current_dir().unwrap_or_default()))?;

        // Role::new normalizes prompt by trimming.
        // Role::build_messages via parse_structure_prompt also trims the system part.
        let expected_system_prompt = expected_prompt_content.trim();

        match &first_message.content {
            MessageContent::Text(text_content) => {
                assert_eq!(
                    text_content.trim(),
                    expected_system_prompt,
                    "System prompt content does not match the rigorous precision role file."
                );
            }
            _ => panic!("First message content should be Text."),
        }

        // Also check that the user message is present
        let user_message = completion_data
            .messages
            .iter()
            .find(|m| m.role == MessageRole::User);
        assert!(user_message.is_some(), "User message should be present");
        match &user_message.unwrap().content {
            MessageContent::Text(text_content) => {
                // If history_rag_context was empty, original text is input_text.
                // If Input::build_messages prepends context, this would be different.
                // For this test, history_rag_context is None.
                assert_eq!(text_content, input_text);
            }
            _ => panic!("User message content should be Text."),
        }

        Ok(())
    }
}
