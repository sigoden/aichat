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
    if *IS_STDOUT_TERMINAL {
        let render_options = config.read().render_options()?;
        let spin = config.read().repl_spinner;
        let mut render = MarkdownRender::init(render_options)?;
        markdown_stream(rx, &mut render, &abort, spin).await
    } else {
        raw_stream(rx, &abort).await
    }
}

pub fn render_error(err: anyhow::Error, highlight: bool) {
    let err = format!("{err:?}");
    if highlight {
        eprintln!("{}", error_text(&err));
    } else {
        eprintln!("{err}");
    }
}
