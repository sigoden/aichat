use super::{MarkdownRender, ReplyEvent};

use crate::utils::{split_line_sematic, split_line_tail, AbortSignal};

use anyhow::Result;
use crossbeam::channel::Receiver;
use textwrap::core::display_width;

pub fn cmd_render_stream(
    rx: &Receiver<ReplyEvent>,
    render: &mut MarkdownRender,
    abort: &AbortSignal,
) -> Result<()> {
    let mut buffer = String::new();
    let mut indent = 0;
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
                        let output = render.render_with_indent(head, indent);
                        println!("{}", output);
                        indent = 0;
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
                                let output = render.render_with_indent(&head, indent);
                                let (_, tail) = split_line_tail(&output);
                                if let Some(width) = render.wrap_width() {
                                    if output.contains('\n') {
                                        indent = display_width(tail);
                                    } else {
                                        indent += display_width(&output);
                                    }
                                    indent %= width as usize;
                                }
                                print!("{}", output);
                            }
                        }
                    }
                }
                ReplyEvent::Done => {
                    let output = render.render_with_indent(&buffer, indent);
                    println!("{}", output);
                    break;
                }
            }
        }
    }
    Ok(())
}
