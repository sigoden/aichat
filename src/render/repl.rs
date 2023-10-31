use super::MarkdownRender;

use crate::repl::{ReplyStreamEvent, SharedAbortSignal};
use crate::utils::split_line_tail;

use anyhow::Result;
use crossbeam::channel::Receiver;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers},
    queue, style,
    terminal::{self, disable_raw_mode, enable_raw_mode},
};
use std::{
    io::{self, Stdout, Write},
    time::{Duration, Instant},
};
use textwrap::core::display_width;

pub fn repl_render_stream(
    rx: &Receiver<ReplyStreamEvent>,
    render: &mut MarkdownRender,
    abort: &SharedAbortSignal,
) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();

    let ret = repl_render_stream_inner(rx, render, abort, &mut stdout);

    disable_raw_mode()?;

    ret
}

fn repl_render_stream_inner(
    rx: &Receiver<ReplyStreamEvent>,
    render: &mut MarkdownRender,
    abort: &SharedAbortSignal,
    writer: &mut Stdout,
) -> Result<()> {
    let mut last_tick = Instant::now();
    let tick_rate = Duration::from_millis(50);
    let mut buffer = String::new();
    let columns = terminal::size()?.0;

    let mut clear_rows = 0;
    loop {
        if abort.aborted() {
            return Ok(());
        }

        if let Ok(evt) = rx.try_recv() {
            match evt {
                ReplyStreamEvent::Text(text) => {
                    let (col, mut row) = cursor::position()?;

                    // fix unexpected duplicate lines on kitty, see https://github.com/sigoden/aichat/issues/105
                    if col == 0 && row > 0 && display_width(&buffer) == columns as usize {
                        row -= 1;
                    }

                    if row + 1 >= clear_rows {
                        queue!(writer, cursor::MoveTo(0, row - clear_rows))?;
                    } else {
                        let scroll_rows = clear_rows - row - 1;
                        queue!(
                            writer,
                            terminal::ScrollUp(scroll_rows),
                            cursor::MoveTo(0, 0),
                        )?;
                    }

                    if text.contains('\n') {
                        let text = format!("{buffer}{text}");
                        let (head, tail) = split_line_tail(&text);
                        buffer = tail.to_string();
                        let output = render.render(head);
                        print_block(writer, &output, columns)?;
                        queue!(writer, style::Print(&buffer),)?;
                        clear_rows = 0;
                    } else {
                        buffer = format!("{buffer}{text}");
                        let output = render.render_line(&buffer);
                        if output.contains('\n') {
                            let (head, tail) = split_line_tail(&output);
                            clear_rows = print_block(writer, head, columns)?;
                            queue!(writer, style::Print(&tail),)?;
                        } else {
                            queue!(writer, style::Print(&output))?;
                            let buffer_width = display_width(&output) as u16;
                            let need_rows = (buffer_width + columns - 1) / columns;
                            clear_rows = need_rows.saturating_sub(1);
                        }
                    }

                    writer.flush()?;
                }
                ReplyStreamEvent::Done => {
                    #[cfg(target_os = "windows")]
                    let eol = "\n\n";
                    #[cfg(not(target_os = "windows"))]
                    let eol = "\n";
                    queue!(writer, style::Print(eol))?;
                    writer.flush()?;

                    break;
                }
            }
            continue;
        }

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));
        if crossterm::event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                        abort.set_ctrlc();
                        return Ok(());
                    }
                    KeyCode::Char('d') if key.modifiers == KeyModifiers::CONTROL => {
                        abort.set_ctrld();
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }
    Ok(())
}

fn print_block(writer: &mut Stdout, text: &str, columns: u16) -> Result<u16> {
    let mut num = 0;
    for line in text.split('\n') {
        queue!(
            writer,
            style::Print(line),
            style::Print("\n"),
            cursor::MoveLeft(columns),
        )?;
        num += 1;
    }
    Ok(num)
}
