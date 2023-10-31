use super::MarkdownRender;

use crate::print_now;
use crate::repl::{ReplyStreamEvent, SharedAbortSignal};
use crate::utils::{spaces, split_line_sematic, split_line_tail};

use anyhow::Result;
use crossbeam::channel::Receiver;
use textwrap::core::display_width;

pub fn cmd_render_stream(
    rx: &Receiver<ReplyStreamEvent>,
    render: &mut MarkdownRender,
    abort: &SharedAbortSignal,
) -> Result<()> {
    let mut buffer = String::new();
    let mut col = 0;
    loop {
        if abort.aborted() {
            return Ok(());
        }
        if let Ok(evt) = rx.try_recv() {
            match evt {
                ReplyStreamEvent::Text(text) => {
                    if text.contains('\n') {
                        let text = format!("{buffer}{text}");
                        let (head, tail) = split_line_tail(&text);
                        buffer = tail.to_string();
                        let input = format!("{}{head}", spaces(col));
                        let output = render.render(&input);
                        print_now!("{}\n", &output[col..]);
                        col = 0;
                    } else {
                        buffer = format!("{buffer}{text}");
                        if !(render.is_code()
                            || buffer.len() < 40
                            || buffer.starts_with('#')
                            || buffer.starts_with('>')
                            || buffer.starts_with('|'))
                        {
                            if let Some((head, remain)) = split_line_sematic(&buffer) {
                                buffer = remain;
                                let input = format!("{}{head}", spaces(col));
                                let output = render.render(&input);
                                let output = &output[col..];
                                let (_, tail) = split_line_tail(output);
                                if render.wrap_width().is_some() {
                                    if output.contains('\n') {
                                        col = display_width(tail);
                                    } else {
                                        col += display_width(output);
                                    }
                                }
                                print_now!("{}", output);
                            }
                        }
                    }
                }
                ReplyStreamEvent::Done => {
                    let input = format!("{}{buffer}", spaces(col));
                    print_now!("{}\n", render.render(&input));
                    break;
                }
            }
        }
    }
    Ok(())
}
