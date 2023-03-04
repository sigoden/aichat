use std::io::{self, Stdout, Write};

use anyhow::Result;

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers},
    queue, style,
    terminal::{self, disable_raw_mode, enable_raw_mode},
};

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
                    // TODO: paste
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
    }
}

struct Session<'a, T: Write> {
    writer: &'a mut T,
    buffer: String,
}

impl<'a, T: Write> Session<'a, T> {
    fn new<'b: 'a>(writer: &'b mut T) -> Self {
        Self {
            buffer: String::new(),
            writer,
        }
    }
    fn push(&mut self, ch: char) -> io::Result<()> {
        if ch == '\n' {
            let (_, y) = cursor::position()?;
            let (_, h) = terminal::size()?;
            if y == h - 1 {
                queue!(self.writer, terminal::ScrollUp(1), cursor::MoveTo(0, y))?;
            } else {
                queue!(self.writer, cursor::MoveToNextLine(1))?;
            }
        } else {
            queue!(self.writer, style::Print(ch))?;
        }
        self.buffer.push(ch);
        self.writer.flush()?;
        Ok(())
    }
}
