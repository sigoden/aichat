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
    /// Forces the session to be saved
    #[clap(long)]
    pub save_session: bool,
    /// Serve the LLM API and WebAPP
    #[clap(long, value_name = "ADDRESS")]
    pub serve: Option<Option<String>>,
    /// Execute commands in natural language
    #[clap(short = 'e', long)]
    pub execute: bool,
    /// Output code only
    #[clap(short = 'c', long)]
    pub code: bool,
    /// Include files with the message
    #[clap(short = 'f', long, value_name = "FILE")]
    pub file: Vec<String>,
    /// Turn off syntax highlighting
    #[clap(short = 'H', long)]
    pub no_highlight: bool,
    /// Turns off stream mode
    #[clap(short = 'S', long)]
    pub no_stream: bool,
    /// Control text wrapping (no, auto, <max-width>)
    #[clap(short = 'w', long)]
    pub wrap: Option<String>,
    /// Use light theme
    #[clap(long)]
    pub light_theme: bool,
    /// Display the message without sending it
    #[clap(long)]
    pub dry_run: bool,
    /// Display information
    #[clap(long)]
    pub info: bool,
    /// List all available models
    #[clap(long)]
    pub list_models: bool,
    /// List all available roles
    #[clap(long)]
    pub list_roles: bool,
    /// List all available sessions
    #[clap(long)]
    pub list_sessions: bool,
    /// Input text
    #[clap(trailing_var_arg = true)]
    text: Vec<String>,
}

impl Cli {
    pub fn text(&self) -> Option<String> {
        let text = self
            .text
            .iter()
            .map(|x| x.trim().to_string())
            .collect::<Vec<String>>()
            .join(" ");
        if text.is_empty() {
            return None;
        }
        Some(text)
    }
}
