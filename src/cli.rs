use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Choose a LLM model
    #[clap(short, long)]
    pub model: Option<String>,
    /// Choose a role
    #[clap(short, long)]
    pub role: Option<String>,
    /// Create or reuse a session
    #[clap(short = 's', long)]
    pub session: Option<Option<String>>,
    /// Attach files to the message to be sent.
    #[clap(short = 'f', long, num_args = 1.., value_name = "FILE")]
    pub file: Option<Vec<String>>,
    /// Disable syntax highlighting
    #[clap(short = 'H', long)]
    pub no_highlight: bool,
    /// No stream output
    #[clap(short = 'S', long)]
    pub no_stream: bool,
    /// Specify the text-wrapping mode (no, auto, <max-width>)
    #[clap(short = 'w', long)]
    pub wrap: Option<String>,
    /// Use light theme
    #[clap(long)]
    pub light_theme: bool,
    /// Run in dry run mode
    #[clap(long)]
    pub dry_run: bool,
    /// Print related information
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
