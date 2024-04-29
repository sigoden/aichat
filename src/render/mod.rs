mod markdown;
mod stream;

pub use self::markdown::{MarkdownRender, RenderOptions};
use self::stream::{markdown_stream, raw_stream};

use crate::utils::AbortSignal;
use crate::{client::SseEvent, config::GlobalConfig};

use anyhow::Result;
use is_terminal::IsTerminal;
use nu_ansi_term::{Color, Style};
use std::io::stdout;
use tokio::sync::mpsc::UnboundedReceiver;

pub async fn render_stream(
    rx: UnboundedReceiver<SseEvent>,
    config: &GlobalConfig,
    abort: AbortSignal,
) -> Result<()> {
    if stdout().is_terminal() {
        let render_options = config.read().get_render_options()?;
        let mut render = MarkdownRender::init(render_options)?;
        let model_id = config.read().model_id.clone();
        markdown_stream(rx, &mut render, &abort, &model_id).await
    } else {
        raw_stream(rx, &abort).await
    }
}

pub fn render_error(err: anyhow::Error, highlight: bool) {
    let err = format!("{err:?}");
    if highlight {
        let style = Style::new().fg(Color::Red);
        eprintln!("{}", style.paint(err));
    } else {
        eprintln!("{err}");
    }
}
