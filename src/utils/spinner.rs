use anyhow::Result;
use crossterm::{cursor, queue, style, terminal};
use std::{
    io::{stdout, Stdout, Write},
    time::Duration,
};
use tokio::{
    sync::{mpsc, oneshot},
    time::interval,
};

pub struct Spinner {
    index: usize,
    message: String,
    stopped: bool,
}

impl Spinner {
    const DATA: [&'static str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

    pub fn new(message: &str) -> Self {
        Spinner {
            index: 0,
            message: message.to_string(),
            stopped: false,
        }
    }

    pub fn set_message(&mut self, message: &str) {
        self.message = format!(" {message}");
    }

    pub fn step(&mut self, writer: &mut Stdout) -> Result<()> {
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

    pub fn stop(&mut self, writer: &mut Stdout) -> Result<()> {
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

pub async fn run_spinner(message: &str) -> (oneshot::Sender<()>, mpsc::UnboundedSender<String>) {
    let message = format!(" {message}");
    let (stop_tx, stop_rx) = oneshot::channel();
    let (message_tx, message_rx) = mpsc::unbounded_channel();
    tokio::spawn(run_spinner_inner(message, stop_rx, message_rx));
    (stop_tx, message_tx)
}

async fn run_spinner_inner(
    message: String,
    stop_rx: oneshot::Receiver<()>,
    mut message_rx: mpsc::UnboundedReceiver<String>,
) -> Result<()> {
    let mut writer = stdout();
    let mut spinner = Spinner::new(&message);
    let mut interval = interval(Duration::from_millis(50));
    tokio::select! {
        _ = async {
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let _ = spinner.step(&mut writer);
                    }
                    message = message_rx.recv() => {
                        if let Some(message) = message {
                            spinner.set_message(&message);
                        }
                    }
                }
            }
        } => {}
        _ = stop_rx => {
            spinner.stop(&mut writer)?;
        }
    }
    Ok(())
}
