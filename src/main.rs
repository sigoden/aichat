mod arena;
mod cli;
mod client;
mod config;
mod core; // Added
mod daemon; // Added
mod function;
mod history_reader;
mod rag;
mod terminal_rag;
mod render;
mod repl;
mod serve;
#[macro_use]
mod utils;

#[macro_use]
extern crate log;

use crate::cli::Cli;
use crate::client::{
    call_chat_completions, call_chat_completions_streaming, list_models, ModelType,
};
use crate::config::{
    ensure_parent_exists, list_agents, load_env_file, macro_execute, Config, GlobalConfig, Input,
    WorkingMode, CODE_ROLE, EXPLAIN_SHELL_ROLE, SHELL_ROLE, TEMP_SESSION_NAME,
};
use crate::render::render_error;
use crate::repl::Repl;
use crate::utils::*;

use anyhow::{bail, Result};
use clap::Parser;
use inquire::validator::Validation;
use inquire::Text;
use is_terminal::IsTerminal;
use parking_lot::RwLock;
use simplelog::{format_description, ConfigBuilder, LevelFilter, SimpleLogger, WriteLogger};
use std::{env, io::stdin, process, sync::Arc};

#[tokio::main]
async fn main() -> Result<()> {
    load_env_file()?;
    let cli = Cli::parse();

    if cli.daemon {
        // Initialize logger for daemon mode
        env_logger::init(); // Basic setup, can be configured further
        info!("Starting aichat in daemon mode.");
        // Potentially adjust setup_logger or have a daemon-specific logger setup
        // For now, env_logger::init() will provide some logging.
        // The existing setup_logger might be too CLI-focused.
        if let Err(e) = daemon::start_daemon() {
            eprintln!("Daemon failed to start: {}", e);
            std::process::exit(1);
        }
        return Ok(());
    }

    let text = cli.text()?;
    let working_mode = if cli.serve.is_some() {
        WorkingMode::Serve
    } else if text.is_none() && cli.file.is_empty() {
        WorkingMode::Repl
    } else {
        WorkingMode::Cmd
    };
    let info_flag = cli.info
        || cli.sync_models
        || cli.list_models
        || cli.list_roles
        || cli.list_agents
        || cli.list_rags
        || cli.list_macros
        || cli.list_sessions;
    setup_logger(working_mode.is_serve())?; // Existing logger for CLI mode
    let config = Arc::new(RwLock::new(Config::init(working_mode, info_flag).await?));
    if let Err(err) = run(config, cli, text).await {
        render_error(err);
        std::process::exit(1);
    }
    Ok(())
}

async fn check_terminal_history_consent(config: &GlobalConfig) -> Result<()> {
    let (enabled, consent_given, is_interactive_terminal) = {
        let cfg = config.read();
        (
            cfg.terminal_history_rag.enabled,
            cfg.terminal_history_rag.consent_given,
            *IS_STDOUT_TERMINAL, // Check if running in an interactive terminal
        )
    };

    if enabled && !consent_given {
        if !is_interactive_terminal {
            // Non-interactive mode, cannot ask for consent.
            // Feature remains disabled for this session if consent was not previously given.
            // Log this situation if a logger is available and configured for info/warn level.
            // For now, we assume no verbose logging for this specific case to avoid clutter
            // if aichat is used in scripts. The feature simply won't activate.
            return Ok(());
        }
        println!("{}", yellow_text("Terminal History RAG - Consent Required:"));
        println!("`aichat` can enhance its contextual understanding by using your terminal command history.");
        println!("This involves reading your shell history file (e.g., ~/.bash_history, ~/.zsh_history),");
        println!("processing commands locally, and potentially including relevant command snippets in prompts to the AI model.");
        println!("{}", red_text("WARNING: Terminal history can contain sensitive information, including commands, arguments, paths, and even accidentally typed passwords. By enabling this, you acknowledge these risks."));
        
        let ans = inquire::Confirm::new("Allow `aichat` to access and process your terminal command history for this purpose?")
            .with_default(false)
            .with_help_message("If you choose 'no', this feature will remain disabled. You can grant consent later by editing the configuration file or (if implemented) using a specific command.")
            .prompt()?;

        if ans {
            let mut cfg_write = config.write();
            cfg_write.terminal_history_rag.consent_given = true;
            if let Err(e) = cfg_write.save_config_file() {
                warn!("Failed to save configuration after granting consent for terminal history RAG: {}. Consent might be lost for future sessions.", e);
            } else {
                info!("Consent for terminal history RAG granted and saved to configuration.");
            }
        } else {
            // User explicitly denied consent. We can also set 'enabled = false' to prevent re-prompting
            // until the user manually re-enables it in the config.
            let mut cfg_write = config.write();
            cfg_write.terminal_history_rag.enabled = false; 
            cfg_write.terminal_history_rag.consent_given = false; // Ensure consent is false
            if let Err(e) = cfg_write.save_config_file() {
                 warn!("Failed to save configuration after denying consent for terminal history RAG: {}. Feature status might not persist as disabled.", e);
            }
            info!("Terminal history RAG feature will remain disabled as consent was not granted.");
        }
    }
    Ok(())
}


async fn run(config: GlobalConfig, cli: Cli, text: Option<String>) -> Result<()> {
    let abort_signal = create_abort_signal();

    // Arena mode check should be early
    if let Some(arena_args) = &cli.arena {
        // The presence of `arena_args` (due to `clap(flatten)`) means arena mode is intended.
        // `agents` (min 2) and `prompt` are required by clap for ArenaArgs.
        // `max_turns` has a default.
        return crate::arena::run_arena_mode(
            &config,
            arena_args.agents.clone(), // run_arena_mode expects Vec<String>
            arena_args.prompt.clone(),  // run_arena_mode expects String
            arena_args.max_turns as usize, // run_arena_mode expects usize
            abort_signal.clone(),
        )
        .await;
    }

    // Prompt for terminal history RAG consent if needed
    if config.read().working_mode.is_repl() || config.read().working_mode.is_cmd() { // Only prompt in interactive modes
        if let Err(e) = check_terminal_history_consent(&config).await {
            warn!("Error during terminal history consent check: {}", e);
            // Decide if this error is critical enough to halt execution. For now, just warn.
        }
    }

    if let Some(query) = cli.search_query {
        use crate::function::ToolCall;
        use serde_json::json;

        let arguments = json!({"query": query});
        let tool_call = ToolCall::new("web_search".to_string(), arguments, None);

        match tool_call.eval(&config.read()) {
            Ok(value) => {
                if let Ok(pretty_json) = serde_json::to_string_pretty(&value) {
                    println!("{}", pretty_json);
                } else {
                    eprintln!("Failed to serialize search results: {:?}", value);
                }
            }
            Err(e) => {
                eprintln!("Error during web search: {}", e);
            }
        }
        return Ok(());
    }

    if let Some(command_string) = cli.exec_command {
        use crate::function::ToolCall;
        use serde_json::json;

        let arguments = json!({"command": command_string});
        let tool_call = ToolCall::new("execute_shell_command".to_string(), arguments, None);

        match tool_call.eval(&config.read()) {
            Ok(value) => {
                if let Ok(pretty_json) = serde_json::to_string_pretty(&value) {
                    println!("{}", pretty_json);
                } else {
                    eprintln!("Failed to serialize command execution results: {:?}", value);
                }
            }
            Err(e) => {
                eprintln!("Error during command execution: {}", e);
            }
        }
        return Ok(());
    }

    if cli.sync_models {
        let url = config.read().sync_models_url();
        return Config::sync_models(&url, abort_signal.clone()).await;
    }

    if cli.list_models {
        for model in list_models(&config.read(), ModelType::Chat) {
            println!("{}", model.id());
        }
        return Ok(());
    }
    if cli.list_roles {
        let roles = Config::list_roles(true).join("\n");
        println!("{roles}");
        return Ok(());
    }
    if cli.list_agents {
        let agents = list_agents().join("\n");
        println!("{agents}");
        return Ok(());
    }
    if cli.list_rags {
        let rags = Config::list_rags().join("\n");
        println!("{rags}");
        return Ok(());
    }
    if cli.list_macros {
        let macros = Config::list_macros().join("\n");
        println!("{macros}");
        return Ok(());
    }

    if cli.dry_run {
        config.write().dry_run = true;
    }

    if let Some(agent) = &cli.agent {
        let session = cli.session.as_ref().map(|v| match v {
            Some(v) => v.as_str(),
            None => TEMP_SESSION_NAME,
        });
        if !cli.agent_variable.is_empty() {
            config.write().agent_variables = Some(
                cli.agent_variable
                    .chunks(2)
                    .map(|v| (v[0].to_string(), v[1].to_string()))
                    .collect(),
            );
        }

        let ret = Config::use_agent(&config, agent, session, abort_signal.clone()).await;
        config.write().agent_variables = None;
        ret?;
    } else {
        if let Some(prompt) = &cli.prompt {
            config.write().use_prompt(prompt)?;
        } else if let Some(name) = &cli.role {
            config.write().use_role(name)?;
        } else if cli.execute {
            config.write().use_role(SHELL_ROLE)?;
        } else if cli.code {
            config.write().use_role(CODE_ROLE)?;
        }
        if let Some(session) = &cli.session {
            config
                .write()
                .use_session(session.as_ref().map(|v| v.as_str()))?;
        }
        if let Some(rag) = &cli.rag {
            Config::use_rag(&config, Some(rag), abort_signal.clone()).await?;
        }
    }
    if cli.list_sessions {
        let sessions = config.read().list_sessions().join("\n");
        println!("{sessions}");
        return Ok(());
    }
    if let Some(model_id) = &cli.model {
        config.write().set_model(model_id)?;
    }
    if cli.no_stream {
        config.write().stream = false;
    }
    if cli.empty_session {
        config.write().empty_session()?;
    }
    if cli.save_session {
        config.write().set_save_session_this_time()?;
    }
    if cli.info {
        let info = config.read().info()?;
        println!("{}", info);
        return Ok(());
    }
    if let Some(addr) = cli.serve {
        return serve::run(config, addr).await;
    }
    // Note: `text` here is Option<String> from `cli.text()?`
    // `cli.agent` is Option<String> for the agent name.

    // If an agent is specified, and it's a ReAct agent, handle it here before REPL/CMD split.
    if let Some(agent_name) = &cli.agent {
        // We need to get the agent instance that was initialized by Config::use_agent or init it here.
        // Config::use_agent already called Agent::init and set it as current_agent.
        // We need a mutable reference to it.
        let mut agent_instance_opt = config.write().current_agent.take(); // Take ownership from config

        if let Some(mut agent) = agent_instance_opt {
            if agent.definition().is_react_agent() {
                let query = text.clone().unwrap_or_default(); // Use text from CLI or default to empty
                if query.is_empty() && agent.conversation_staters().is_empty() {
                     // For ReAct agent, if no query and no conversation starters, it might be ambiguous what to do.
                     // However, the ReAct loop expects an initial query.
                     // If `text` is None, perhaps prompt the user or use a default starter if available.
                     // For now, if text is empty, we'll let it proceed, ReAct loop might need a query.
                     // Or, more robustly:
                    if query.is_empty() {
                        // This is a command-line execution of a ReAct agent without a direct query.
                        // This scenario might require prompting or specific handling.
                        // For now, we'll just print the agent's banner and exit,
                        // as ReAct loop needs an initial goal/query.
                        // Or, we can try to run it with an empty query if the agent is designed for it.
                        // The subtask implies `text` is the query.
                        // If text is None, it means `aichat --agent react_search_agent` without further input.
                        // This should probably show help or error.
                        // For now, let's assume `text` must be provided for a ReAct agent in non-REPL.
                        if !config.read().working_mode.is_repl() {
                            // In CMD mode, a query is expected for ReAct.
                            // If `text` (from `cli.text()?`) is None, it means no direct query was passed.
                            // We could bail, or use a default conversation starter if any.
                            // The `run_react_loop` takes `current_user_query` and `initial_goal`.
                            // These would come from `text`.
                            if text.is_none() {
                                bail!("ReAct agent '{}' in command mode requires an initial query.", agent_name);
                            }
                        }
                        // If it's REPL mode, the query will be asked inside the REPL loop.
                        // So, if text is None AND it's CMD mode, that's an issue.
                    }
                }

                // If not in REPL mode, execute the ReAct loop directly.
                if !config.read().working_mode.is_repl() {
                    let client = client::LLMClient::from_config(&config, Some(agent.model().clone()))?;
                    let current_query = text.unwrap_or_default(); // text is already Option<String>
                    let initial_goal = current_query.clone(); 

                    let result = agent.run_react_loop(current_query, initial_goal, &client, &config, true).await?;
                    println!("{}", result);
                    
                    config.write().current_agent = Some(agent); // Put agent back if needed, or handle session exit
                    config.write().exit_session()?; // Properly exit session, clears agent.react_scratchpad
                    return Ok(());
                } else {
                    // In REPL mode, the ReAct agent will be handled by the REPL's loop.
                    // Put the agent back into the config so REPL can use it.
                    config.write().current_agent = Some(agent);
                }
            } else {
                // Not a ReAct agent, put it back
                config.write().current_agent = Some(agent);
            }
        }
        // If agent_instance_opt was None, it means Config::use_agent didn't set current_agent,
        // or it was already taken. This would be an issue with internal logic flow.
    }

    let is_repl = config.read().working_mode.is_repl();
    if cli.rebuild_rag {
        Config::rebuild_rag(&config, abort_signal.clone()).await?;
        if is_repl {
            return Ok(());
        }
    }
    if let Some(name) = &cli.macro_name {
        macro_execute(&config, name, text.as_deref(), abort_signal.clone()).await?;
        return Ok(());
    }
    if cli.execute && !is_repl {
        if cfg!(target_os = "macos") && !stdin().is_terminal() {
            bail!("Unable to read the pipe for shell execution on MacOS")
        }
        let input = create_input(&config, text, &cli.file, abort_signal.clone()).await?;
        shell_execute(&config, &SHELL, input, abort_signal.clone()).await?;
        return Ok(());
    }
    config.write().apply_prelude()?;

    // Terminal History RAG Indexing
    if config.read().terminal_history_rag.enabled && config.read().terminal_history_rag.consent_given {
        match crate::history_reader::get_terminal_history(&config) {
            Ok(history_entries) => {
                if !history_entries.is_empty() {
                    debug!("Read {} terminal history entries. Building index...", history_entries.len());
                    match crate::terminal_rag::TerminalHistoryIndexer::build_index(history_entries, &config).await {
                        Ok(indexer) => {
                            config.write().terminal_history_indexer = Some(Arc::new(indexer));
                            debug!("Terminal history RAG index built successfully.");
                        }
                        Err(e) => warn!("Failed to build terminal history RAG index: {}", e),
                    }
                } else {
                    debug!("No terminal history entries found to build RAG index.");
                }
            }
            Err(e) => warn!("Failed to read terminal history for RAG: {}", e),
        }
    }

    match is_repl {
        false => {
            let mut input = create_input(&config, text, &cli.file, abort_signal.clone()).await?;
            
            // Augment with file-based RAG first (if any)
            input.use_embeddings(abort_signal.clone()).await?;

            // Then, augment with terminal history RAG (if indexer is available)
            if let Some(indexer) = config.read().terminal_history_indexer.clone() {
                let query_for_history_rag = input.text(); // Use the current text (potentially augmented by file RAG) as query
                let top_k = config.read().terminal_history_rag.top_k;
                match indexer.search(&query_for_history_rag, top_k).await {
                    Ok(history_results) => {
                        if !history_results.is_empty() {
                            let context_str = history_results.iter()
                                .map(|entry| format!("$ {}", entry.command)) // Simple formatting
                                .collect::<Vec<String>>()
                                .join("\n");
                            let full_context = format!("--- Relevant Terminal History Snippets ---\n{}\n--- End of History Snippets ---", context_str);
                            input.set_history_rag_context(full_context);
                            debug!("Augmented input with {} terminal history snippets.", history_results.len());
                        }
                    }
                    Err(e) => warn!("Terminal history RAG search failed: {}", e),
                }
            }
            start_directive(&config, input, cli.code, abort_signal).await
        }
        true => {
            // For REPL mode, the input creation and RAG augmentation will happen inside the REPL loop.
            // The index (if built) is available in `config.read().terminal_history_indexer`.
            if !*IS_STDOUT_TERMINAL {
                bail!("No TTY for REPL")
            }
            start_interactive(&config).await
        }
    }
}

#[async_recursion::async_recursion]
async fn start_directive(
    config: &GlobalConfig,
    input: Input,
    code_mode: bool,
    abort_signal: AbortSignal,
) -> Result<()> {
    let client = input.create_client()?;
    let extract_code = !*IS_STDOUT_TERMINAL && code_mode;
    config.write().before_chat_completion(&input)?;
    let (output, tool_results) = if !input.stream() || extract_code {
        call_chat_completions(
            &input,
            true,
            extract_code,
            client.as_ref(),
            abort_signal.clone(),
        )
        .await?
    } else {
        call_chat_completions_streaming(&input, client.as_ref(), abort_signal.clone()).await?
    };
    config
        .write()
        .after_chat_completion(&input, &output, &tool_results)?;

    if !tool_results.is_empty() {
        start_directive(
            config,
            input.merge_tool_results(output, tool_results),
            code_mode,
            abort_signal,
        )
        .await?;
    }

    config.write().exit_session()?;
    Ok(())
}

async fn start_interactive(config: &GlobalConfig) -> Result<()> {
    let mut repl: Repl = Repl::init(config)?;
    repl.run().await
}

#[async_recursion::async_recursion]
async fn shell_execute(
    config: &GlobalConfig,
    shell: &Shell,
    mut input: Input,
    abort_signal: AbortSignal,
) -> Result<()> {
    let client = input.create_client()?;
    config.write().before_chat_completion(&input)?;
    let (eval_str, _) =
        call_chat_completions(&input, false, true, client.as_ref(), abort_signal.clone()).await?;

    config
        .write()
        .after_chat_completion(&input, &eval_str, &[])?;
    if eval_str.is_empty() {
        bail!("No command generated");
    }
    if config.read().dry_run {
        config.read().print_markdown(&eval_str)?;
        return Ok(());
    }
    if *IS_STDOUT_TERMINAL {
        let options = ["execute", "revise", "describe", "copy", "quit"];
        let command = color_text(eval_str.trim(), nu_ansi_term::Color::Rgb(255, 165, 0));
        let first_letter_color = nu_ansi_term::Color::Cyan;
        let prompt_text = options
            .iter()
            .map(|v| format!("{}{}", color_text(&v[0..1], first_letter_color), &v[1..]))
            .collect::<Vec<String>>()
            .join(&dimmed_text(" | "));
        loop {
            println!("{command}");
            let answer = Text::new(&format!("{prompt_text}:"))
                .with_default("e")
                .with_validator(
                    |input: &str| match matches!(input, "e" | "r" | "d" | "c" | "q") {
                        true => Ok(Validation::Valid),
                        false => Ok(Validation::Invalid(
                            "Invalid option, choice one of e, r, d, c or q".into(),
                        )),
                    },
                )
                .prompt()?;

            match answer.as_str() {
                "e" => {
                    debug!("{} {:?}", shell.cmd, &[&shell.arg, &eval_str]);
                    let code = run_command(&shell.cmd, &[&shell.arg, &eval_str], None)?;
                    if code == 0 && config.read().save_shell_history {
                        let _ = append_to_shell_history(&shell.name, &eval_str, code);
                    }
                    process::exit(code);
                }
                "r" => {
                    let revision = Text::new("Enter your revision:").prompt()?;
                    let text = format!("{}\n{revision}", input.text());
                    input.set_text(text);
                    return shell_execute(config, shell, input, abort_signal.clone()).await;
                }
                "d" => {
                    let role = config.read().retrieve_role(EXPLAIN_SHELL_ROLE)?;
                    let input = Input::from_str(config, &eval_str, Some(role));
                    if input.stream() {
                        call_chat_completions_streaming(
                            &input,
                            client.as_ref(),
                            abort_signal.clone(),
                        )
                        .await?;
                    } else {
                        call_chat_completions(
                            &input,
                            true,
                            false,
                            client.as_ref(),
                            abort_signal.clone(),
                        )
                        .await?;
                    }
                    println!();
                    continue;
                }
                "c" => {
                    set_text(&eval_str)?;
                    println!("{}", dimmed_text("✓ Copied the command."));
                }
                _ => {}
            }
            break;
        }
    } else {
        println!("{}", eval_str);
    }
    Ok(())
}

async fn create_input(
    config: &GlobalConfig,
    text: Option<String>,
    file: &[String],
    abort_signal: AbortSignal,
) -> Result<Input> {
    let input = if file.is_empty() {
        Input::from_str(config, &text.unwrap_or_default(), None)
    } else {
        Input::from_files_with_spinner(
            config,
            &text.unwrap_or_default(),
            file.to_vec(),
            None,
            abort_signal,
        )
        .await?
    };
    if input.is_empty() {
        bail!("No input");
    }
    Ok(input)
}

fn setup_logger(is_serve: bool) -> Result<()> {
    let (log_level, log_path) = Config::log_config(is_serve)?;
    if log_level == LevelFilter::Off {
        return Ok(());
    }
    let crate_name = env!("CARGO_CRATE_NAME");
    let log_filter = match std::env::var(get_env_name("log_filter")) {
        Ok(v) => v,
        Err(_) => match is_serve {
            true => format!("{crate_name}::serve"),
            false => crate_name.into(),
        },
    };
    let config = ConfigBuilder::new()
        .add_filter_allow(log_filter)
        .set_time_format_custom(format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
        ))
        .set_thread_level(LevelFilter::Off)
        .build();
    match log_path {
        None => {
            SimpleLogger::init(log_level, config)?;
        }
        Some(log_path) => {
            ensure_parent_exists(&log_path)?;
            let log_file = std::fs::File::create(log_path)?;
            WriteLogger::init(log_level, config, log_file)?;
        }
    }
    Ok(())
}
