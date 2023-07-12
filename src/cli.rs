use clap::Parser;

#[allow(clippy::struct_excessive_bools, clippy::module_name_repetitions)]
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Choose a model
    #[clap(short, long)]
    pub model: Option<String>,
    /// Add a GPT prompt
    #[clap(short, long)]
    pub prompt: Option<String>,
    /// Disable syntax highlighting
    #[clap(short = 'H', long)]
    pub no_highlight: bool,
    /// No stream output
    #[clap(short = 'S', long)]
    pub no_stream: bool,
    /// List all roles
    #[clap(long)]
    pub list_roles: bool,
    /// List all models
    #[clap(long)]
    pub list_models: bool,
    /// Select a role
    #[clap(short, long)]
    pub role: Option<String>,
    /// Print system-wide information
    #[clap(long)]
    pub info: bool,
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
