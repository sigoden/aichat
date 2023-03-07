mod markdown;

pub use self::markdown::MarkdownRender;
use crate::repl::ReplyStreamEvent;

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
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use unicode_width::UnicodeWidthStr;

pub fn render_stream(rx: Receiver<ReplyStreamEvent>, ctrlc: Arc<AtomicBool>) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    queue!(stdout, event::DisableMouseCapture)?;

    let ret = render_stream_inner(rx, ctrlc, &mut stdout);

    queue!(stdout, event::DisableMouseCapture)?;
    disable_raw_mode()?;

    ret
}

pub fn render_stream_inner(
    rx: Receiver<ReplyStreamEvent>,
    ctrlc: Arc<AtomicBool>,
    writer: &mut Stdout,
) -> Result<()> {
    let mut last_tick = Instant::now();
    let tick_rate = Duration::from_millis(200);
    let mut buffer = String::new();
    let mut markdown_render = MarkdownRender::new();
    let terminal_columns = terminal::size()?.0;
    loop {
        if ctrlc.load(Ordering::SeqCst) {
            return Ok(());
        }

        if let Ok(evt) = rx.try_recv() {
            recover_cursor(writer, terminal_columns, &buffer)?;

            match evt {
                ReplyStreamEvent::Text(text) => {
                    if text.contains('\n') {
                        let text = format!("{buffer}{text}");
                        let mut lines: Vec<&str> = text.split('\n').collect();
                        buffer = lines.pop().unwrap_or_default().to_string();
                        let output = markdown_render.render(&lines.join("\n"));
                        queue!(
                            writer,
                            style::Print(output),
                            style::Print("\n"),
                            style::Print(&buffer),
                        )?;
                    } else {
                        buffer = format!("{buffer}{text}");
						let output = markdown_render.render_line_stateless(&buffer);
                        queue!(writer, style::Print(&output))?;
                    }
                    writer.flush()?;
                }
                ReplyStreamEvent::Done => {
                    let output = markdown_render.render_line_stateless(&buffer);
                    queue!(writer, style::Print(output), style::Print("\n"))?;
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
                        ctrlc.store(true, Ordering::SeqCst);
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

fn recover_cursor(writer: &mut Stdout, terminal_columns: u16, buffer: &str) -> Result<()> {
    let buffer_rows = (buffer.width() as u16 + terminal_columns - 1) / terminal_columns;
    let (_, row) = cursor::position()?;
    if row + 1 >= buffer_rows {
        queue!(writer, cursor::MoveTo(0, row + 1 - buffer_rows))?;
    } else {
        queue!(
            writer,
            terminal::ScrollUp(buffer_rows - 1 - row),
            cursor::MoveTo(0, 0)
        )?;
    }
    Ok(())
}
