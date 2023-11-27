use super::{MarkdownRender, ReplyEvent};

use crate::utils::AbortSignal;

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
    ops::Div,
    time::{Duration, Instant},
};
use textwrap::core::display_width;

pub fn markdown_stream(
    rx: &Receiver<ReplyEvent>,
    render: &mut MarkdownRender,
    abort: &AbortSignal,
) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();

    let ret = markdown_stream_inner(rx, render, abort, &mut stdout);

    disable_raw_mode()?;

    ret
}

pub fn raw_stream(rx: &Receiver<ReplyEvent>, abort: &AbortSignal) -> Result<()> {
    loop {
        if abort.aborted() {
            return Ok(());
        }
        if let Ok(evt) = rx.try_recv() {
            match evt {
                ReplyEvent::Text(text) => {
                    print!("{}", text);
                }
                ReplyEvent::Done => {
                    break;
                }
            }
        }
    }
    Ok(())
}

fn markdown_stream_inner(
    rx: &Receiver<ReplyEvent>,
    render: &mut MarkdownRender,
    abort: &AbortSignal,
    writer: &mut Stdout,
) -> Result<()> {
    let mut last_tick = Instant::now();
    let tick_rate = Duration::from_millis(50);

    let mut buffer = String::new();
    let mut buffer_rows = 1;

    let columns = terminal::size()?.0;

    let mut spinner = Spinner::new(" Generating");

    'outer: loop {
        if abort.aborted() {
            return Ok(());
        }
        spinner.step(writer)?;

        for reply_event in gather_events(rx) {
            spinner.stop(writer)?;

            match reply_event {
                ReplyEvent::Text(text) => {
                    let (col, mut row) = cursor::position()?;

                    // Fix unexpected duplicate lines on kitty, see https://github.com/sigoden/aichat/issues/105
                    if col == 0 && row > 0 && display_width(&buffer) == columns as usize {
                        row -= 1;
                    }

                    if row + 1 >= buffer_rows {
                        queue!(writer, cursor::MoveTo(0, row + 1 - buffer_rows),)?;
                    } else {
                        let scroll_rows = buffer_rows - row - 1;
                        queue!(
                            writer,
                            terminal::ScrollUp(scroll_rows),
                            cursor::MoveTo(0, 0),
                        )?;
                    }

                    // No guarantee that text returned by render will not be re-layouted, so it is better to clear it.
                    queue!(writer, terminal::Clear(terminal::ClearType::FromCursorDown))?;

                    if text.contains('\n') {
                        let text = format!("{buffer}{text}");
                        let (head, tail) = split_line_tail(&text);
                        let output = render.render(head);
                        print_block(writer, &output, columns)?;
                        buffer = tail.to_string();
                    } else {
                        buffer = format!("{buffer}{text}");
                    }

                    let output = render.render_line(&buffer);
                    if output.contains('\n') {
                        let (head, tail) = split_line_tail(&output);
                        buffer_rows = print_block(writer, head, columns)?;
                        queue!(writer, style::Print(&tail),)?;

                        // No guarantee the buffer width of the buffer will not exceed the number of columns.
                        // So we calculate the number of rows needed, rather than setting it directly to 1.
                        buffer_rows += need_rows(tail, columns);
                    } else {
                        queue!(writer, style::Print(&output))?;
                        buffer_rows = need_rows(&output, columns);
                    }

                    writer.flush()?;
                }
                ReplyEvent::Done => {
                    break 'outer;
                }
            }
        }

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| tick_rate.div(2));
        if crossterm::event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                        abort.set_ctrlc();
                        break;
                    }
                    KeyCode::Char('d') if key.modifiers == KeyModifiers::CONTROL => {
                        abort.set_ctrld();
                        break;
                    }
                    _ => {}
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }

    spinner.stop(writer)?;

    Ok(())
}

struct Spinner {
    index: usize,
    message: String,
    stopped: bool,
}

impl Spinner {
    const DATA: [&'static str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

    fn new(message: &str) -> Self {
        Spinner {
            index: 0,
            message: message.to_string(),
            stopped: false,
        }
    }

    fn step(&mut self, writer: &mut Stdout) -> Result<()> {
        if self.stopped {
            return Ok(());
        }
        let frame = Self::DATA[self.index % Self::DATA.len()];
        let dots = ".".repeat((self.index / 5) % 4);
        let line = format!("{frame}{}{:<3}", self.message, dots);
        queue!(writer, cursor::MoveToColumn(0), style::Print(line),)?;
        if self.index == 0 {
            queue!(writer, cursor::Hide)?;
        }
        writer.flush()?;
        self.index += 1;
        Ok(())
    }

    fn stop(&mut self, writer: &mut Stdout) -> Result<()> {
        if self.stopped {
            return Ok(());
        }
        self.stopped = true;
        queue!(
            writer,
            cursor::MoveToColumn(0),
            terminal::Clear(terminal::ClearType::FromCursorDown),
            cursor::Show
        )?;
        writer.flush()?;
        Ok(())
    }
}

fn gather_events(rx: &Receiver<ReplyEvent>) -> Vec<ReplyEvent> {
    let mut texts = vec![];
    let mut done = false;
    for reply_event in rx.try_iter() {
        match reply_event {
            ReplyEvent::Text(v) => texts.push(v),
            ReplyEvent::Done => {
                done = true;
            }
        }
    }
    let mut events = vec![];
    if !texts.is_empty() {
        events.push(ReplyEvent::Text(texts.join("")))
    }
    if done {
        events.push(ReplyEvent::Done)
    }
    events
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

fn split_line_tail(text: &str) -> (&str, &str) {
    if let Some((head, tail)) = text.rsplit_once('\n') {
        (head, tail)
    } else {
        ("", text)
    }
}

fn need_rows(text: &str, columns: u16) -> u16 {
    let buffer_width = display_width(text).max(1) as u16;
    (buffer_width + columns - 1) / columns
}
