use anyhow::Result;
use crossterm::{cursor, queue, style, terminal};
use is_terminal::IsTerminal;
use std::{
    io::{stdout, Write},
    time::Duration,
};
use tokio::{sync::mpsc, time::interval};

pub struct SpinnerInner {
    index: usize,
    message: String,
    is_not_terminal: bool,
}

impl SpinnerInner {
    const DATA: [&'static str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

    fn new(message: &str) -> Self {
        SpinnerInner {
            index: 0,
            message: message.to_string(),
            is_not_terminal: !stdout().is_terminal(),
        }
    }

    fn step(&mut self) -> Result<()> {
        if self.is_not_terminal || self.message.is_empty() {
            return Ok(());
        }
        let mut writer = stdout();
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

    fn set_message(&mut self, message: String) -> Result<()> {
        self.clear_message()?;
        if !message.is_empty() {
            self.message = format!(" {message}");
        }
        Ok(())
    }

    fn clear_message(&mut self) -> Result<()> {
        if self.is_not_terminal || self.message.is_empty() {
            return Ok(());
        }
        self.message.clear();
        let mut writer = stdout();
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

#[derive(Clone)]
pub struct Spinner(mpsc::UnboundedSender<SpinnerEvent>);

impl Drop for Spinner {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Spinner {
    pub fn set_message(&self, message: String) -> Result<()> {
        self.0.send(SpinnerEvent::SetMessage(message))?;
        Ok(())
    }

    pub fn stop(&self) {
        let _ = self.0.send(SpinnerEvent::Stop);
    }
}

enum SpinnerEvent {
    SetMessage(String),
    Stop,
}

pub async fn create_spinner(message: &str) -> Spinner {
    let message = format!(" {message}");
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(run_spinner(message, rx));
    Spinner(tx)
}

async fn run_spinner(message: String, mut rx: mpsc::UnboundedReceiver<SpinnerEvent>) -> Result<()> {
    let mut spinner = SpinnerInner::new(&message);
    let mut interval = interval(Duration::from_millis(50));
    loop {
        tokio::select! {
            _ = interval.tick() => {
                let _ = spinner.step();
            }
            evt = rx.recv() => {
                if let Some(evt) = evt {
                    match evt {
                        SpinnerEvent::SetMessage(message) => {
                            spinner.set_message(message)?;
                        }
                        SpinnerEvent::Stop => {
                            spinner.clear_message()?;
                            break;
                        }
                    }

                }
            }
        }
    }
    Ok(())
}
