mod cmd;
mod markdown;
mod repl;
mod wrap;

use self::cmd::cmd_render_stream;
#[allow(clippy::module_name_repetitions)]
pub use self::markdown::{MarkdownRender, MarkdownTheme};
use self::repl::repl_render_stream;
pub use self::wrap::Wrap;

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
    let (highlight, light_theme, wrap) = config.read().get_render_options();
    let mut stream_handler = {
        let (tx, rx) = unbounded();
        let abort_clone = abort.clone();
        spawn(move || {
            let err = match (highlight, repl) {
                (false, _) => {
                    let theme = MarkdownTheme::No;
                    let mut render = MarkdownRender::new(theme, wrap);
                    cmd_render_stream(&rx, &mut render, &abort)
                }
                (true, false) => {
                    let theme = MarkdownTheme::new(light_theme);
                    let mut render = MarkdownRender::new(theme, wrap);
                    cmd_render_stream(&rx, &mut render, &abort)
                }
                (true, true) => {
                    let theme = MarkdownTheme::new(light_theme);
                    let mut render = MarkdownRender::new(theme, wrap);
                    repl_render_stream(&rx, &mut render, &abort)
                }
            };
            if let Err(err) = err {
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
