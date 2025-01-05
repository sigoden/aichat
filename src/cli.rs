use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Select a LLM model
    #[clap(short, long)]
    pub model: Option<String>,
    /// Use the system prompt
    #[clap(long)]
    pub prompt: Option<String>,
    /// Select a role
    #[clap(short, long)]
    pub role: Option<String>,
    /// Start or join a session
    #[clap(short = 's', long)]
    pub session: Option<Option<String>>,
    /// Ensure the session is empty
    #[clap(long)]
    pub empty_session: bool,
    /// Ensure the new conversation is saved to the session
    #[clap(long)]
    pub save_session: bool,
    /// Start a agent
    #[clap(short = 'a', long)]
    pub agent: Option<String>,
    /// Set agent variables
    #[clap(long, value_names = ["NAME", "VALUE"], num_args = 2)]
    pub agent_variable: Vec<String>,
    /// Start a RAG
    #[clap(long)]
    pub rag: Option<String>,
    /// Rebuild the RAG to sync document changes
    #[clap(long)]
    pub rebuild_rag: bool,
    /// Serve the LLM API and WebAPP
    #[clap(long, value_name = "ADDRESS")]
    pub serve: Option<Option<String>>,
    /// Execute commands in natural language
    #[clap(short = 'e', long)]
    pub execute: bool,
    /// Output code only
    #[clap(short = 'c', long)]
    pub code: bool,
    /// Include files, directories, or URLs
    #[clap(short = 'f', long, value_name = "FILE")]
    pub file: Vec<String>,
    /// Turn off stream mode
    #[clap(short = 'S', long)]
    pub no_stream: bool,
    /// Display the message without sending it
    #[clap(long)]
    pub dry_run: bool,
    /// Display information
    #[clap(long)]
    pub info: bool,
    /// List all available chat models
    #[clap(long)]
    pub list_models: bool,
    /// List all roles
    #[clap(long)]
    pub list_roles: bool,
    /// List all sessions
    #[clap(long)]
    pub list_sessions: bool,
    /// List all agents
    #[clap(long)]
    pub list_agents: bool,
    /// List all RAGs
    #[clap(long)]
    pub list_rags: bool,
    /// Input text
    #[clap(trailing_var_arg = true)]
    text: Vec<String>,
}

impl Cli {
    pub fn text(&self) -> Option<String> {
        let text = self.text.to_vec().join(" ");
        if text.is_empty() {
            return None;
        }
        Some(text)
    }
}
