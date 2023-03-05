use std::io::{self, Stdout, Write};

use anyhow::Result;

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers},
    queue, style,
    terminal::{self, disable_raw_mode, enable_raw_mode, ClearType},
};

use crate::utils::paste;

pub fn edit() -> Result<String> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();

    let ret = edit_inner(&mut stdout);

    // restore terminal
    disable_raw_mode()?;

    ret
}

fn edit_inner(writer: &mut Stdout) -> Result<String> {
    let mut session = Session::new(writer);

    loop {
        let evt = event::read()?;
        if let Event::Key(key) = evt {
            match key.code {
                KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                    // quit
                    return Ok(String::new());
                }
                KeyCode::Char('d') if key.modifiers == KeyModifiers::CONTROL => {
                    // submit
                    return Ok(session.buffer);
                }
                KeyCode::Char('v') if key.modifiers == KeyModifiers::CONTROL => {
                    let content = paste()?;
                    session.push_str(&content)?;
                }
                KeyCode::Char(c)
                    if matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT) =>
                {
                    session.push(c)?;
                }
                KeyCode::Enter => {
                    session.push('\n')?;
                }
                _ => {}
            }
        }
        session.flush()?;
    }
}

struct Session<'a, T: Write> {
    writer: &'a mut T,
    buffer: String,
    dirty: bool,
}

impl<'a, T: Write> Session<'a, T> {
    fn new<'b: 'a>(writer: &'b mut T) -> Self {
        Self {
            buffer: String::new(),
            writer,
            dirty: false,
        }
    }
    fn push(&mut self, ch: char) -> io::Result<()> {
        if ch == '\n' {
            self.new_line()?;
        } else {
            queue!(self.writer, style::Print(ch))?;
        }
        self.buffer.push(ch);
        self.dirty = true;
        Ok(())
    }
    fn push_str(&mut self, text: &str) -> io::Result<()> {
        for line in text.lines() {
            if !line.is_empty() {
                queue!(self.writer, style::Print(line))?;
            }
            self.new_line()?;
        }

        Ok(())
    }
    fn new_line(&mut self) -> io::Result<()> {
        let (_, y) = cursor::position()?;
        let (_, h) = terminal::size()?;
        if y == h - 1 {
            queue!(self.writer, terminal::ScrollUp(1), cursor::MoveTo(0, y))?;
        } else {
            queue!(self.writer, cursor::MoveToNextLine(1))?;
        }
        Ok(())
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.dirty {
            return self.writer.flush();
        }
        Ok(())
    }
}

pub fn clear_screen(keep_lines: u16) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();

    let ret = clear_screen_inner(&mut stdout, keep_lines);

    // restore terminal
    disable_raw_mode()?;

    ret
}

fn clear_screen_inner(writer: &mut Stdout, keep_lines: u16) -> Result<()> {
    let (_, h) = terminal::size()?;
    queue!(
        writer,
        style::Print("\n".repeat((h - 2).into())),
        terminal::ScrollUp(2),
        cursor::MoveTo(0, 0),
        terminal::Clear(ClearType::FromCursorDown),
        terminal::ScrollUp(keep_lines),
    )?;
    writer.flush()?;
    Ok(())
}
