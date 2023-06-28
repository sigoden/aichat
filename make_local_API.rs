use std::process::Command;

fn main() {
    let output = Command::new("ls")
        .arg("-l")
        .output()
        .expect("failed to execute process");

    if output.status.success() {
        let result = String::from_utf8_lossy(&output.stdout);
        println!("Command output:\n{}", result);
    } else {
        let error = String::from_utf8_lossy(&output.stderr);
        println!("Command failed:\n{}", error);
    }
}
```
