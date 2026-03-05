use std::fs;
use std::io::{self, Write, BufRead};
use std::process::Command;
use std::path::Path;
use std::env;

const CHAT_HISTORY_FILE: &str = ".config/aichat/messages.md";

fn load_chat_history() -> Vec<String> {
    let contents = fs::read_to_string(CHAT_HISTORY_FILE)
        .expect("Something went wrong reading the file");

    contents.lines().map(|s| s.to_string()).collect()
}

fn save_chat_history(chat_history: &Vec<String>) {
    let contents = chat_history.join("\n");
    fs::write(CHAT_HISTORY_FILE, contents).expect("Unable to write file");
}

fn call_aichat(prompt: &str) -> String {
    let output = Command::new("aichat")
        .arg(prompt)
        .output()
        .expect("Failed to execute command");

    if !output.status.success() {
        eprintln!("Error calling aichat: {}", output.status);
        return String::new();
    }

    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn main() {
    let mut chat_history = load_chat_history();

    loop {
        print!("You: ");
        io::stdout().flush().unwrap();

        let mut user_input = String::new();
        io::stdin().read_line(&mut user_input).unwrap();
        let user_input = user_input.trim();

        if user_input == "quit" {
            break;
        }

        chat_history.push(format!("You: {}", user_input));

        let prompt = chat_history.iter().skip(chat_history.len().saturating_sub(1000)).fold(String::new(), |a, b| a + " " + b);
        let ai_response = call_aichat(&prompt);
        println!("AI Chat: {}", ai_response);

        chat_history.push(format!("AI Chat: {}", ai_response));
        save_chat_history(&chat_history);
    }
}
