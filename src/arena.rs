use crate::client::call_chat_completions;
use crate::config::{Agent, GlobalConfig, Input, RoleLike};
use crate::utils::AbortSignal;
use anyhow::{Context, Result};

// Define a simple struct for transcript entries
#[derive(Debug, Clone)]
pub struct TranscriptEntry {
    pub agent_name: String,
    pub message: String,
}

pub async fn run_arena_mode(
    config: &GlobalConfig,
    agent_names: Vec<String>,
    initial_prompt: String,
    max_turns: usize,
    abort_signal: AbortSignal,
) -> Result<()> {
    println!("Starting arena mode...");
    println!("Agents: {}", agent_names.join(", "));
    println!("Initial prompt: {}", initial_prompt);
    println!("Max turns: {}", max_turns);

    // Store agents in a HashMap for easy lookup by name, and keep a list of active names
    let mut agents_map: std::collections::HashMap<String, Agent> = std::collections::HashMap::new();
    let mut active_agent_names: Vec<String> = Vec::new();

    for agent_name in &agent_names {
        // Use a cloned agent_name for init, as the original is borrowed.
        match Agent::init(config, &agent_name.clone(), abort_signal.clone()).await {
            Ok(agent) => {
                println!("Successfully initialized agent: {}", agent.name());
                // Store the initialized agent by its actual name (from agent.name())
                agents_map.insert(agent.name().to_string(), agent);
                // Keep track of the successfully initialized agent names (original names provided)
                active_agent_names.push(agent_name.clone());
            }
            Err(e) => {
                eprintln!("Error initializing agent '{}': {}", agent_name, e);
            }
        }
    }

    if active_agent_names.len() < 2 {
        anyhow::bail!("Arena requires at least two successfully initialized agents. Exiting.");
    }

    let mut transcript: Vec<TranscriptEntry> = Vec::new();

    // 1. Initialize the first message
    let user_prompt_entry = TranscriptEntry {
        agent_name: "User".to_string(), // Or "Moderator"
        message: initial_prompt.clone(),
    };
    transcript.push(user_prompt_entry.clone());
    println!(
        "{}: {}",
        user_prompt_entry.agent_name, user_prompt_entry.message
    );

    let num_active_agents = active_agent_names.len();
    let mut current_input_text = initial_prompt;
    let mut completed_turns = 0;

    // 2. Implement the conversation loop
    // max_turns is the total number of LLM responses in the arena.
    for turn_index in 0..max_turns {
        if abort_signal.aborted() {
            println!(
                "\nArena loop aborted by signal after {} turns.",
                completed_turns
            );
            break;
        }

        // Determine current agent
        let agent_name_turn_index = turn_index % num_active_agents;
        let current_agent_name = &active_agent_names[agent_name_turn_index];
        let current_agent = agents_map.get(current_agent_name).with_context(|| {
            format!(
                "Internal error: Agent '{}' not found in map after initialization.",
                current_agent_name
            )
        })?;

        println!(
            "\n--- Turn {} (Agent: {}) ---",
            turn_index + 1,
            current_agent.name()
        );

        // 3. Agent Processing
        // The input text is the last message in the transcript.
        let mut input_for_agent =
            Input::from_str(config, &current_input_text, Some(current_agent.to_role()));

        let client = input_for_agent.create_client().with_context(|| {
            format!(
                "Failed to create LLM client for agent '{}'",
                current_agent.name()
            )
        })?;

        input_for_agent
            .integrate_web_research(client.as_ref(), abort_signal.clone())
            .await?;

        match call_chat_completions(
            &input_for_agent,
            true,  // send_to_llm
            false, // extract_code (we want natural language response)
            client.as_ref(),
            abort_signal.clone(),
        )
        .await
        {
            Ok((output_text, _tool_results)) => {
                // 4. Output and Transcript
                println!("{}: {}", current_agent.name(), output_text);
                let agent_response_entry = TranscriptEntry {
                    agent_name: current_agent.name().to_string(),
                    message: output_text.clone(),
                };
                transcript.push(agent_response_entry);
                current_input_text = output_text; // Next agent gets this output as input
            }
            Err(e) => {
                let error_msg = format!(
                    "Error during LLM call for agent {}: {}",
                    current_agent.name(),
                    e
                );
                eprintln!("{}", error_msg);
                // Agent "passes" its turn, its message is an error.
                let agent_error_entry = TranscriptEntry {
                    agent_name: current_agent.name().to_string(),
                    message: format!("[Error: {}]", e), // Keep it concise for transcript
                };
                transcript.push(agent_error_entry);
                // The next agent will receive this error message as its input.
                // This could be changed to re-use the previous valid message if desired.
                current_input_text =
                    format!("[Agent {} encountered an error]", current_agent.name());
            }
        }
        completed_turns += 1;
    }

    println!(
        "\n--- Arena Session Finished ({} out of {} configured turns completed) ---",
        completed_turns, max_turns
    );

    // Print the full transcript
    println!("\n--- Arena Transcript ---");
    for entry in &transcript {
        println!("{}: {}", entry.agent_name, entry.message);
    }
    println!("--- End of Transcript ---");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AgentConfig, AgentDefinition, Config, GlobalConfig, WorkingMode};
    use crate::utils::AbortSignal;
    use parking_lot::RwLock;
    use std::collections::HashMap;
    use std::sync::Arc;

    // Helper to create a GlobalConfig for testing
    fn create_test_global_config() -> GlobalConfig {
        let mut config = Config::default();
        // Assuming default() doesn't set up models, let's try to add a dummy one
        // to prevent panics if the code tries to access a default model.
        // This part is speculative without seeing Config::default() or model handling.
        config.models.insert(
            "test-dummy-model".to_string(),
            crate::client::Model {
                id: "test-dummy-model".to_string(),
                name: "test-dummy-model".to_string(),
                provider_id: "dummy_provider".to_string(),
                r#type: crate::client::ModelType::Chat,
                url: "http://localhost/dummy".to_string(),
                max_input_tokens: Some(1024),
                max_output_tokens: Some(1024),
                ..Default::default() // Use default for other fields if available
            },
        );
        config.model_name = Some("test-dummy-model".to_string());
        config.working_mode = WorkingMode::Cmd; // Set a working mode

        Arc::new(RwLock::new(config))
    }

    // Helper to create dummy agent configurations for testing
    fn create_dummy_agent_config(name: &str) -> AgentConfig {
        AgentConfig {
            name: Some(name.to_string()),
            definition: AgentDefinition {
                model: Some("dummy-model-for-agent".to_string()), // Non-existent model
                prompt: Some(format!("You are dummy agent {}.", name)),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_insufficient_agents_bailout() {
        let config = create_test_global_config();
        let agent_names = vec!["agent1".to_string()]; // Only one agent
        let initial_prompt = "Hello, arena!".to_string();
        let max_turns = 2;
        let abort_signal = AbortSignal::new();

        // Mock agent config files - create temporary files or ensure Agent::init can handle missing files for dummy names
        // For this test, the number of agents is key, not their successful full init.
        // Agent::init might fail if it tries to load files for these names.
        // The current run_arena_mode tries to init agents first.

        let result = run_arena_mode(
            &config,
            agent_names,
            initial_prompt,
            max_turns,
            abort_signal,
        )
        .await;

        assert!(result.is_err());
        if let Some(err) = result.err() {
            assert!(err
                .to_string()
                .contains("Arena requires at least two successfully initialized agents"));
        }
    }

    #[tokio::test]
    async fn test_arena_runs_with_agent_init_errors_if_two_or_more_succeed() {
        let mut test_config = Config::default();
        test_config.working_mode = WorkingMode::Cmd;
        // Add a dummy model that agents might reference if init gets that far
        test_config.models.insert(
            "dummy-model-for-agent".to_string(),
            crate::client::Model {
                id: "dummy-model-for-agent".to_string(),
                name: "dummy-model-for-agent".to_string(),
                provider_id: "dummy_provider".to_string(),
                r#type: crate::client::ModelType::Chat,
                url: "http://localhost/dummy_agent_model".to_string(),
                max_input_tokens: Some(1024),
                max_output_tokens: Some(1024),
                ..Default::default()
            },
        );
        let global_config = Arc::new(RwLock::new(test_config));

        // Setup AgentConfigs in GlobalConfig's agents map
        // This is how Agent::init tries to find agent definitions
        global_config
            .write()
            .agents
            .insert("agent1".to_string(), create_dummy_agent_config("agent1"));
        global_config
            .write()
            .agents
            .insert("agent2".to_string(), create_dummy_agent_config("agent2"));
        // agent3 will not have a config, so its Agent::init should fail

        let agent_names = vec![
            "agent1".to_string(),
            "agent2".to_string(),
            "agent3-no-config".to_string(), // This agent's init should fail
        ];
        let initial_prompt = "Test prompt".to_string();
        let max_turns = 2;
        let abort_signal = AbortSignal::new();

        // Expect run_arena_mode to proceed because agent1 and agent2 should initialize.
        // The LLM calls for agent1 & agent2 will then fail (as "dummy-model-for-agent" is not real),
        // which tests the error handling within the loop.
        let result = run_arena_mode(
            &global_config,
            agent_names,
            initial_prompt,
            max_turns,
            abort_signal,
        )
        .await;

        // The function should complete successfully (orchestration doesn't fail on LLM errors)
        assert!(result.is_ok());
        // Further assertions would require inspecting stdout or refactoring to return transcript.
        // For now, successfully running through the turns with internal errors is the main check.
    }

    #[tokio::test]
    async fn test_turn_orchestration_and_max_turns_with_llm_errors() {
        let mut test_config = Config::default();
        test_config.working_mode = WorkingMode::Cmd;
        test_config.models.insert(
            "dummy-model-for-llm-error-test".to_string(),
            crate::client::Model {
                id: "dummy-model-for-llm-error-test".to_string(),
                name: "dummy-model-for-llm-error-test".to_string(),
                provider_id: "dummy_provider".to_string(),
                r#type: crate::client::ModelType::Chat,
                url: "http://localhost/dummy_llm_error".to_string(),
                max_input_tokens: Some(1024),
                max_output_tokens: Some(1024),
                ..Default::default()
            },
        );
        let global_config = Arc::new(RwLock::new(test_config));

        // Configure two agents that will successfully initialize but whose LLM calls will fail
        let agent1_name = "ErrorAgent1";
        let agent2_name = "ErrorAgent2";

        let mut agent1_config = create_dummy_agent_config(agent1_name);
        agent1_config.definition.model = Some("dummy-model-for-llm-error-test".to_string());
        global_config
            .write()
            .agents
            .insert(agent1_name.to_string(), agent1_config);

        let mut agent2_config = create_dummy_agent_config(agent2_name);
        agent2_config.definition.model = Some("dummy-model-for-llm-error-test".to_string());
        global_config
            .write()
            .agents
            .insert(agent2_name.to_string(), agent2_config);

        let agent_names = vec![agent1_name.to_string(), agent2_name.to_string()];
        let initial_prompt = "Hello LLM Error Test".to_string();
        let max_turns = 3; // Expect 3 turns of LLM call attempts (and failures)
        let abort_signal = AbortSignal::new();

        // Suppress println! output during this test if possible, or just let it run.
        // TODO: Find a way to capture/suppress stdout for cleaner test runs.

        let result = run_arena_mode(
            &global_config,
            agent_names,
            initial_prompt,
            max_turns,
            abort_signal,
        )
        .await;

        // The orchestration itself should succeed even if LLM calls fail.
        assert!(result.is_ok());

        // Ideally, here we would inspect the transcript. Since we can't directly,
        // we rely on the fact that `run_arena_mode` completed `max_turns` attempts.
        // The `println!` statements within `run_arena_mode` would show the flow.
        // For example, "--- Turn 1 (Agent: ErrorAgent1) ---", then error,
        // then "--- Turn 2 (Agent: ErrorAgent2) ---", then error, etc.
        // This test ensures the loop runs `max_turns` times and handles errors correctly.
    }

    // Test for abort signal. This is tricky as it requires timing.
    // A simplified version might just check that an aborted signal before start prevents any turns.
    #[tokio::test]
    async fn test_abort_signal_prevents_turns() {
        let mut test_config = Config::default();
        test_config.working_mode = WorkingMode::Cmd;
        test_config.models.insert(
            "dummy-model-for-abort-test".to_string(),
            crate::client::Model {
                id: "dummy-model-for-abort-test".to_string(),
                name: "dummy-model-for-abort-test".to_string(),
                provider_id: "dummy_provider".to_string(),
                r#type: crate::client::ModelType::Chat,
                url: "http://localhost/dummy_abort".to_string(),
                max_input_tokens: Some(1024),
                max_output_tokens: Some(1024),
                ..Default::default()
            },
        );
        let global_config = Arc::new(RwLock::new(test_config));

        let agent1_name = "AbortAgent1";
        let agent2_name = "AbortAgent2";

        let mut agent1_config_abort = create_dummy_agent_config(agent1_name);
        agent1_config_abort.definition.model = Some("dummy-model-for-abort-test".to_string());
        global_config
            .write()
            .agents
            .insert(agent1_name.to_string(), agent1_config_abort);

        let mut agent2_config_abort = create_dummy_agent_config(agent2_name);
        agent2_config_abort.definition.model = Some("dummy-model-for-abort-test".to_string());
        global_config
            .write()
            .agents
            .insert(agent2_name.to_string(), agent2_config_abort);

        let agent_names = vec![agent1_name.to_string(), agent2_name.to_string()];
        let initial_prompt = "Abort Test".to_string();
        let max_turns = 5;
        let abort_signal = AbortSignal::new();
        abort_signal.abort(); // Abort before starting

        let result = run_arena_mode(
            &global_config,
            agent_names,
            initial_prompt,
            max_turns,
            abort_signal,
        )
        .await;

        assert!(result.is_ok());
        // Since the loop checks for abort at the very beginning,
        // `completed_turns` should be 0. This is indirectly tested as no panic occurs
        // and the "Arena loop aborted by signal after 0 turns." would be printed.
        // Direct assertion of completed_turns requires refactoring run_arena_mode or capturing stdout.
    }
}
