#[allow(unused_imports)]
use std::io::{self, Write};

fn type_buildin(name: &str) -> String {
    if name.starts_with("echo") || name.starts_with("exit") || name.starts_with("type") {
        format!("{} is a shell builtin", name.trim())
    } else {
        format!("{}: not found", name.trim())
    }
}

fn main() {
    // Uncomment this block to pass the first stage
    loop {
        print!("$ ");
        io::stdout().flush().unwrap();

        // Wait for user input
        let stdin = io::stdin();
        let mut input = String::new();
        stdin.read_line(&mut input).unwrap();
        if input.trim() == "exit 0" {
            std::process::exit(0);
        } else if input.starts_with("echo ") {
            println!("{}", &input[5..].trim());
        } else if input.starts_with("type ") {
            let msg = type_buildin(&input[5..]);
            println!("{}", msg);
        } else {
            println!("{}: command not found", input.trim());
        }
    }
}
