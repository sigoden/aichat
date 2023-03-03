use crossterm::{
    cursor,
    event::{DisableMouseCapture, EnableMouseCapture},
    execute, queue, style,
    terminal::{
        self, disable_raw_mode, enable_raw_mode, size, ClearType, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use is_terminal::IsTerminal;
use std::{
    io::{self, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Receiver,
        Arc,
    },
};

use crate::repl::ReplyEvent;

pub fn run(rx: Receiver<ReplyEvent>, ctrlc: Arc<AtomicBool>) -> anyhow::Result<()> {
    // setup terminal
    enable_raw_mode()?;
    let mut output = String::new();
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    while let Ok(ev) = rx.recv() {
        if ctrlc.load(Ordering::SeqCst) {
            break;
        }
        match ev {
            ReplyEvent::Text(text) => {
                queue!(
                    stdout,
                    style::ResetColor,
                    terminal::Clear(ClearType::All),
                    cursor::Hide,
                    cursor::MoveTo(0, 0)
                )?;
                output.push_str(&text);
                let rows = size()?.1 as usize;
                let lines: Vec<&str> = output.split('\n').collect();
                let len = lines.len();
                let skip = if len > rows { len - rows } else { 0 };
                let mut selected_lines = vec![];
                let mut count_begin_code = 0;
                for (index, line) in lines.iter().enumerate() {
                    if index < skip {
                        if line.starts_with("```") {
                            count_begin_code += 1;
                        }
                    } else {
                        selected_lines.push(*line);
                    }
                }
                let mut md = selected_lines.join("\n");
                if count_begin_code % 2 == 1 {
                    md = format!("```{md}");
                };
                let md = termimad::inline(&md).to_string();
                for line in md.split('\n') {
                    queue!(stdout, style::Print(line), cursor::MoveToNextLine(1))?;
                }

                stdout.flush()?;
            }
            ReplyEvent::Done => {
                break;
            }
        }
    }

    execute!(stdout, style::ResetColor, cursor::Show)?;

    // restore terminal
    disable_raw_mode()?;
    execute!(stdout, LeaveAlternateScreen, DisableMouseCapture)?;

    Ok(())
}

pub fn print(output: &str, is_render: bool) {
    if is_render && io::stdout().is_terminal() {
        termimad::print_inline(output);
        println!()
    } else {
        println!("{output}");
    }
}
