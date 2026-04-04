use super::*;
use crate::client::common::Client;
use crate::config::Input;
use anyhow::{anyhow, bail, Result};
use reqwest::Client as ReqwestClient;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, RwLock};
use std::time::Duration;
use tokio::time::sleep;

const API_BASE: &str = "https://jules.googleapis.com/v1alpha";

static SESSION_MAP: LazyLock<RwLock<HashMap<String, String>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

#[derive(Debug, Clone, Deserialize, Default)]
pub struct JulesConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    pub api_base: Option<String>,
    pub source: Option<String>,
    pub starting_branch: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelData>,
    pub patch: Option<RequestPatch>,
    pub extra: Option<ExtraConfig>,
}

impl JulesClient {
    config_get_fn!(api_key, get_api_key);
    config_get_fn!(api_base, get_api_base);
    config_get_fn!(source, get_source);
    config_get_fn!(starting_branch, get_starting_branch);

    pub const PROMPTS: [PromptAction<'static>; 1] = [("api_key", "API Key", None)];

    fn get_session_id(&self, session_name: &str) -> Option<String> {
        SESSION_MAP.read().unwrap().get(session_name).cloned()
    }

    fn set_session_id(&self, session_name: &str, session_id: String) {
        SESSION_MAP
            .write()
            .unwrap()
            .insert(session_name.to_string(), session_id);
    }
}

#[async_trait::async_trait]
impl Client for JulesClient {
    client_common_fns!();

    async fn chat_completions_inner(
        &self,
        _client: &ReqwestClient,
        _data: ChatCompletionsData,
    ) -> Result<ChatCompletionsOutput> {
        bail!("Jules client only supports streaming chat completions via `chat_completions_streaming` override")
    }

    async fn chat_completions_streaming_inner(
        &self,
        _client: &ReqwestClient,
        _handler: &mut SseHandler,
        _data: ChatCompletionsData,
    ) -> Result<()> {
         bail!("Jules client only supports streaming chat completions via `chat_completions_streaming` override")
    }

    async fn chat_completions_streaming(
        &self,
        input: &Input,
        handler: &mut SseHandler,
    ) -> Result<()> {
        let client = self.build_client()?;
        let api_key = self.get_api_key()?;
        let api_base = self.get_api_base().unwrap_or_else(|_| API_BASE.to_string());
        let source = self.get_source()
            .map_err(|_| anyhow!("Missing 'source' in jules config. Please set it in config.yaml like `source: sources/github/owner/repo`."))?;
        let starting_branch = self.get_starting_branch().unwrap_or_else(|_| "main".to_string());

        // Determine if we reuse a session or create a new one
        let session_key = input
            .session(&self.global_config.read().session)
            .map(|s| s.name().to_string());

        let jules_session_id = if let Some(ref key) = session_key {
            self.get_session_id(key)
        } else {
            None
        };

        let prompt = input.text();

        let session_id = if let Some(id) = jules_session_id {
            // Send message to existing session
            let url = format!("{}/sessions/{}:sendMessage", api_base, id);
            let body = json!({ "prompt": prompt });
            let res = client
                .post(&url)
                .header("X-Goog-Api-Key", &api_key)
                .json(&body)
                .send()
                .await?;

             if !res.status().is_success() {
                 let text = res.text().await?;
                 bail!("Failed to send message: {}", text);
             }
             id
        } else {
            // Create new session
            let url = format!("{}/sessions", api_base);
            let body = json!({
                "prompt": prompt,
                "sourceContext": {
                    "source": source,
                     "githubRepoContext": {
                        "startingBranch": starting_branch
                    }
                }
            });
            let res = client
                .post(&url)
                .header("X-Goog-Api-Key", &api_key)
                .json(&body)
                .send()
                .await?;

            if !res.status().is_success() {
                let text = res.text().await?;
                bail!("Failed to create session: {}", text);
            }

            let data: Value = res.json().await?;
            let name = data["name"]
                .as_str()
                .ok_or_else(|| anyhow!("Invalid session response"))?
                .to_string();
            // extract ID from name "sessions/{id}"
            let id = name.split('/').next_back().unwrap_or(&name).to_string();

            if let Some(key) = session_key {
                self.set_session_id(&key, id.clone());
            }
            id
        };

        // Polling loop
        let mut processed_activities = HashSet::new();
        let mut loop_count = 0;
        let max_loops = 600; // 600 * 2s = 20 minutes timeout

        loop {
            if loop_count > max_loops {
                handler.text("\n[Timeout waiting for agent]\n")?;
                break;
            }
            loop_count += 1;
            sleep(Duration::from_secs(2)).await;

            let url = format!("{}/sessions/{}/activities?pageSize=100", api_base, session_id);
            let res = client
                .get(&url)
                .header("X-Goog-Api-Key", &api_key)
                .send()
                .await?;

            if !res.status().is_success() {
                continue; // Retry?
            }

            let data: Value = res.json().await?;
            let activities = data["activities"].as_array();

            if let Some(list) = activities {
                // Process oldest first
                for activity in list.iter().rev() {
                    let id = activity["name"].as_str().unwrap_or("");
                    if processed_activities.contains(id) {
                        continue;
                    }
                    processed_activities.insert(id.to_string());

                    // Check for agent activity
                    let _originator = activity["originator"].as_str().unwrap_or("");

                    if let Some(plan) = activity["planGenerated"]["plan"].as_object() {
                         handler.text("\n**Plan Generated:**\n")?;
                         if let Some(steps) = plan["steps"].as_array() {
                             for step in steps {
                                 let title = step["title"].as_str().unwrap_or("");
                                 handler.text(&format!("- {}\n", title))?;
                             }
                         }
                         handler.text("\n")?;
                    }

                    if let Some(progress) = activity["progressUpdated"].as_object() {
                        let title = progress["title"].as_str().unwrap_or("");
                        let desc = progress["description"].as_str().unwrap_or("");
                        handler.text(&format!("> {} {}\n", title, desc))?;
                    }

                    if let Some(artifacts) = activity["artifacts"].as_array() {
                         for artifact in artifacts {
                             if let Some(bash) = artifact["bashOutput"].as_object() {
                                 let cmd = bash["command"].as_str().unwrap_or("");
                                 let out = bash["output"].as_str().unwrap_or("");
                                 handler.text(&format!("```bash\n$ {}\n{}\n```\n", cmd, out))?;
                             }
                             if let Some(_changeset) = artifact["changeSet"].as_object() {
                                 // TODO: format patch details?
                                 handler.text("```diff\n[Code Change Applied]\n```\n")?;
                             }
                         }
                    }

                    if activity.get("sessionCompleted").is_some() {
                        handler.done();
                        return Ok(());
                    }

                    // If we see a "message" from agent or generic completion signal not covered by sessionCompleted
                    // The docs example ends with sessionCompleted.
                }
            }
        }

        Ok(())
    }
}
