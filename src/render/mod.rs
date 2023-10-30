mod cmd;
mod markdown;
mod repl;

use self::cmd::cmd_render_stream;
#[allow(clippy::module_name_repetitions)]
pub use self::markdown::{MarkdownRender, RenderOptions};
use self::repl::repl_render_stream;

use crate::client::Client;
use crate::config::SharedConfig;
use crate::print_now;
use crate::repl::{ReplyStreamHandler, SharedAbortSignal};

use anyhow::Result;
use crossbeam::channel::unbounded;
use crossbeam::sync::WaitGroup;
use std::thread::spawn;

#[allow(clippy::module_name_repetitions)]
pub fn render_stream(
    input: &str,
    client: &dyn Client,
    config: &SharedConfig,
    repl: bool,
    abort: SharedAbortSignal,
    wg: WaitGroup,
) -> Result<String> {
    let render_options = config.read().get_render_options();
    let mut stream_handler = {
        let (tx, rx) = unbounded();
        let abort_clone = abort.clone();
        spawn(move || {
            let run = move || {
                if repl {
                    let mut render = MarkdownRender::init(render_options)?;
                    repl_render_stream(&rx, &mut render, &abort)
                } else {
                    let mut render = MarkdownRender::init(render_options)?;
                    cmd_render_stream(&rx, &mut render, &abort)
                }
            };
            if let Err(err) = run() {
                let err = format!("{err:?}");
                print_now!("{}\n\n", err.trim());
            }
            drop(wg);
        });
        ReplyStreamHandler::new(tx, abort_clone)
    };
    client.send_message_streaming(input, &mut stream_handler)?;
    let buffer = stream_handler.get_buffer();
    Ok(buffer.to_string())
}
