mod cmd;
mod markdown;
mod repl;

use self::cmd::cmd_render_stream;
pub use self::markdown::MarkdownRender;
use self::repl::repl_render_stream;

use crate::client::ChatGptClient;
use crate::config::SharedConfig;
use crate::print_now;
use crate::repl::{ReplyStreamHandler, SharedAbortSignal};

use anyhow::Result;
use crossbeam::channel::unbounded;
use crossbeam::sync::WaitGroup;
use std::thread::spawn;

pub fn render_stream(
    input: &str,
    client: &ChatGptClient,
    config: SharedConfig,
    repl: bool,
    abort: SharedAbortSignal,
    wg: WaitGroup,
) -> Result<String> {
    let (highlight, light_theme) = config.read().get_render_options();
    let mut stream_handler = if highlight {
        let (tx, rx) = unbounded();
        let abort_clone = abort.clone();
        spawn(move || {
            let err = if repl {
                repl_render_stream(rx, light_theme, abort)
            } else {
                cmd_render_stream(rx, light_theme, abort)
            };
            if let Err(err) = err {
                let err = format!("{err:?}");
                print_now!("{}\n\n", err.trim());
            }
            drop(wg);
        });
        ReplyStreamHandler::new(Some(tx), repl, abort_clone)
    } else {
        drop(wg);
        ReplyStreamHandler::new(None, repl, abort)
    };
    client.send_message_streaming(input, &mut stream_handler)?;
    let buffer = stream_handler.get_buffer();
    Ok(buffer.to_string())
}
