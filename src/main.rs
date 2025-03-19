use std::env;
use std::fs;
#[allow(unused_imports)]
use std::io::{self, Write};

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
    let path = env::var("PATH").ok()?;
    find_exec_in_fs(path.as_str(), name).ok()
}

fn type_buildin(name: &str) -> String {
    if let Some(first) = name.split_whitespace().next() {
        if ["echo", "exit", "type"].contains(&first) {
            return format!("{} is a shell buildin", name);
        }
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
        match input.trim() {
            "exit 0" => break,
            input if input.starts_with("echo ") => println!("{}", &input[5..]),
            input if input.starts_with("type ") => {
                let msg = type_buildin(&input[5..]);
                println!("{}", msg);
            }
            _ => println!("{}: command not found", input.trim()),
        }
    }
}
