#[allow(unused_imports)]
use std::io::{self, Write};

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
        } else {
            println!("{}: command not found", input.trim());
        }
    }
}
