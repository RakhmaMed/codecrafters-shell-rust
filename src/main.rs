use std::env;
use std::fs;
use std::io::{self, Write};
use std::process::Command;

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
        if ["echo", "exit", "type", "pwd"].contains(&first) {
            return format!("{} is a shell builtin", name);
        }
    }

    if let Some(path) = find_exec_in_path(name) {
        return format!("{} is {}/{}", name, path, name);
    }

    format!("{} not found", name)
}

fn try_call(command: &str, arg1: &str) -> Result<(), String> {
    let mut output = Command::new(command)
        .arg(arg1)
        .spawn()
        .map_err(|err| err.to_string())?;

    output.wait().map_err(|err| err.to_string())?;

    Ok(())
}

fn main() {
    loop {
        print!("$ ");
        io::stdout().flush().unwrap();

        // Wait for user input
        let stdin = io::stdin();
        let mut input = String::new();
        stdin.read_line(&mut input).unwrap();
        let input = input.trim();
        if input.is_empty() {
            continue;
        }

        let mut full_command = input.split_whitespace();
        let command = full_command.next();
        let args: Vec<&str> = full_command.collect();

        match (command, args.as_slice()) {
            (Some("exit"), [exit_code, ..]) => std::process::exit(exit_code.parse().unwrap_or(-1)),
            (Some("echo"), [_, ..]) => println!("{}", args.join(" ")),
            (Some("pwd"), []) => println!("{}", env::current_dir().unwrap_or_default().display()),
            (Some("type"), [cmd, ..]) => {
                let msg = type_buildin(cmd);
                println!("{}", msg);
            }
            (Some(cmd), [arg1, ..]) => try_call(cmd, arg1).unwrap(),
            _ => println!("{}: command not found", input.trim()),
        }
    }
}
