use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Turn off highlight
    #[clap(short = 'H', long)]
    pub no_highlight: bool,
    /// List all roles
    #[clap(short = 'L', long)]
    pub list_roles: bool,
    /// Select a role
    #[clap(short, long)]
    pub role: Option<String>,
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
