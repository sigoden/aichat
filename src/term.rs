use anyhow::Result;
use crossterm::{
    cursor, queue, style,
    terminal::{self, disable_raw_mode, enable_raw_mode, ClearType},
};
use std::io::{self, Stdout, Write};

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
