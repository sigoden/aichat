use anyhow::Result;
use crossterm::{
    cursor,
    event::{DisableMouseCapture, EnableMouseCapture},
    execute, queue, style,
    terminal::{
        self, disable_raw_mode, enable_raw_mode, size, ClearType, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use mdcat::{
    push_tty,
    terminal::{TerminalProgram, TerminalSize},
    Environment, ResourceAccess, Settings,
};
use pulldown_cmark::Parser;
use std::{
    io::{self, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Receiver,
        Arc,
    },
};
use syntect::parsing::SyntaxSet;

use crate::repl::{dump, ReplyEvent};

pub fn render_stream(
    rx: Receiver<ReplyEvent>,
    ctrlc: Arc<AtomicBool>,
    markdown_render: Arc<MarkdownRender>,
) -> Result<()> {
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
                let mut code = None;
                for (index, line) in lines.iter().enumerate() {
                    if index < skip {
                        if line.starts_with("```") {
                            count_begin_code += 1;
                            code = Some(*line);
                        }
                    } else {
                        selected_lines.push(*line);
                    }
                }
                if count_begin_code % 2 == 1 {
                    if let Some(code) = code {
                        selected_lines[0] = code
                    }
                };
                let content = selected_lines.join("\n");
                for line in markdown_render.render(&content)?.split('\n') {
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

pub struct MarkdownRender {
    env: Environment,
    settings: Settings,
}

impl MarkdownRender {
    pub fn init() -> Result<Self> {
        let terminal = TerminalProgram::detect();
        let env =
            Environment::for_local_directory(&std::env::current_dir().expect("Working directory"))?;
        let settings = Settings {
            resource_access: ResourceAccess::LocalOnly,
            syntax_set: SyntaxSet::load_defaults_newlines(),
            terminal_capabilities: terminal.capabilities(),
            terminal_size: TerminalSize::default(),
        };
        Ok(Self { env, settings })
    }

    pub fn print(&self, input: &str) -> Result<()> {
        let markdown = self.render(input)?;
        dump(markdown, 0);
        Ok(())
    }

    pub fn render(&self, input: &str) -> Result<String> {
        let source = Parser::new(input);
        let mut sink = Vec::new();
        push_tty(&self.settings, &self.env, &mut sink, source)?;
        Ok(String::from_utf8_lossy(&sink).into())
    }
}
