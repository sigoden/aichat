use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// List all available models
    #[clap(long)]
    pub list_models: bool,
    /// Choose a LLM model
    #[clap(short, long)]
    pub model: Option<String>,
    /// List all available roles
    #[clap(long)]
    pub list_roles: bool,
    /// Choose a role
    #[clap(short, long)]
    pub role: Option<String>,
    /// List all available sessions
    #[clap(long)]
    pub list_sessions: bool,
    /// Initiate or reuse a session
    #[clap(short = 's', long)]
    pub session: Option<Option<String>>,
    /// Specify the text-wrapping mode (no*, auto, <max-width>)
    #[clap(short = 'w', long)]
    pub wrap: Option<String>,
    /// Print related information
    #[clap(long)]
    pub info: bool,
    /// Use light theme
    #[clap(long)]
    pub light_theme: bool,
    /// Disable syntax highlighting
    #[clap(short = 'H', long)]
    pub no_highlight: bool,
    /// No stream output
    #[clap(short = 'S', long)]
    pub no_stream: bool,
    /// Run in dry run mode
    #[clap(long)]
    pub dry_run: bool,
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
