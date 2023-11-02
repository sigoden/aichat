use super::{MarkdownRender, ReplyEvent};

use crate::utils::{spaces, split_line_sematic, split_line_tail, AbortSignal};

use anyhow::Result;
use crossbeam::channel::Receiver;
use textwrap::core::display_width;

pub fn cmd_render_stream(
    rx: &Receiver<ReplyEvent>,
    render: &mut MarkdownRender,
    abort: &AbortSignal,
) -> Result<()> {
    let mut buffer = String::new();
    let mut col = 0;
    loop {
        if abort.aborted() {
            return Ok(());
        }
        if let Ok(evt) = rx.try_recv() {
            match evt {
                ReplyEvent::Text(text) => {
                    if text.contains('\n') {
                        let text = format!("{buffer}{text}");
                        let (head, tail) = split_line_tail(&text);
                        buffer = tail.to_string();
                        let input = format!("{}{head}", spaces(col));
                        let output = render.render(&input);
                        println!("{}", &output[col..]);
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
                                print!("{}", output);
                            }
                        }
                    }
                }
                ReplyEvent::Done => {
                    let input = format!("{}{buffer}", spaces(col));
                    let output = render.render(&input);
                    let output = &output[col..];
                    println!("{}", output);
                    break;
                }
            }
        }
    }
    Ok(())
}
