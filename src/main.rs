#[allow(unused_imports)]
use std::io::{self, Write};
use std::env;
use std::fs;

fn find_exec_in_fs(path: &str, name: &str) -> io::Result<String> {
    for p in path.split(":") {
        let entries = fs::read_dir(p)?;

        for entry in entries {
            let entry = entry?;
            let filename = entry.file_name();
            if filename == name {
                return Ok(p.to_string());
            }
        }
    }

    Err(io::Error::new(io::ErrorKind::NotFound, "File not found"))
}

fn find_exec_in_path(name: &str) -> Option<String> {
    match env::var("PATH") {
        Ok(path) => {
            match find_exec_in_fs(path.as_str(), name) {
                Ok(path) => Some(path),
                Err(_) => None,
            }
        }
        Err(err) => None
    }
}

fn type_buildin(name: &str) -> String {
    if name.starts_with("echo") || name.starts_with("exit") || name.starts_with("type") {
        return format!("{} is a shell builtin", name);
    }
    if let Some(path) = find_exec_in_path(name) {
        return format!("{} is {}/{}", name, path, name);
    }
    format!("{} not found", name)
}

fn main() {
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
            let msg = type_buildin(&input[5..].trim());
            println!("{}", msg);
        } else {
            println!("{}: command not found", input.trim());
        }
    }
}
