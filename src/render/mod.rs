mod markdown;
mod stream;

pub use self::markdown::{MarkdownRender, RenderOptions};
use self::stream::{markdown_stream, raw_stream};

use crate::utils::{error_text, AbortSignal, IS_STDOUT_TERMINAL};
use crate::{client::SseEvent, config::GlobalConfig};

use anyhow::Result;
use tokio::sync::mpsc::UnboundedReceiver;

pub async fn render_stream(
    rx: UnboundedReceiver<SseEvent>,
    config: &GlobalConfig,
    abort: AbortSignal,
) -> Result<()> {
    let ret = if *IS_STDOUT_TERMINAL {
        let render_options = config.read().render_options()?;
        let mut render = MarkdownRender::init(render_options)?;
        markdown_stream(rx, &mut render, &abort).await
    } else {
        raw_stream(rx, &abort).await
    };
    ret.map_err(|err| err.context("Failed to reader stream"))
}

pub fn render_error(err: anyhow::Error, highlight: bool) {
    let err = format!("Error: {err:?}");
    if highlight {
        eprintln!("{}", error_text(&err));
    } else {
        eprintln!("{err}");
    }
}
