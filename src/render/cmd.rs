use super::MarkdownRender;
use crate::repl::{ReplyStreamEvent, SharedAbortSignal};
use crate::utils::dump;

use anyhow::Result;
use crossbeam::channel::Receiver;

pub fn cmd_render_stream(rx: Receiver<ReplyStreamEvent>, abort: SharedAbortSignal) -> Result<()> {
    let mut buffer = String::new();
    let mut markdown_render = MarkdownRender::new();
    loop {
        if abort.aborted() {
            return Ok(());
        }
        if let Ok(evt) = rx.try_recv() {
            match evt {
                ReplyStreamEvent::Text(text) => {
                    if text.contains('\n') {
                        let text = format!("{buffer}{text}");
                        let mut lines: Vec<&str> = text.split('\n').collect();
                        buffer = lines.pop().unwrap_or_default().to_string();
                        let output = lines.join("\n");
                        dump(markdown_render.render(&output), 1);
                    } else {
                        buffer = format!("{buffer}{text}");
                    }
                }
                ReplyStreamEvent::Done => {
                    let output = markdown_render.render(&buffer);
                    dump(output, 2);
                    break;
                }
            }
        }
    }
    Ok(())
}
