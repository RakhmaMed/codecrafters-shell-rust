use std::env;
use std::fs;
use std::io::{self, Write};
use std::process::Command;
use std::os::unix::process::CommandExt;

const BACKSLASH: char = '\\';
const SINGLE_QUOTE: char = '\'';
const DOUBLE_QUOTE: char = '"';

// Function to parse arguments respecting single quotes
fn parse_arguments(input_args: &str) -> Result<Vec<String>, String> {
    
    let mut args: Vec<String> = Vec::new();
    let mut current_arg = String::new();
    let mut in_double_quotes = false;
    let mut in_single_quotes = false;
    let mut chars = input_args.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            SINGLE_QUOTE => {
                if in_double_quotes{
                    current_arg.push(SINGLE_QUOTE);
                } else {
                    // Togle quote state. Don't add the quote itself to the argument.
                    in_single_quotes = !in_single_quotes;
                }
            }
            DOUBLE_QUOTE => {
                if in_single_quotes {
                    current_arg.push(DOUBLE_QUOTE);
                } else {
                    in_double_quotes = !in_double_quotes;
                }
            }
            BACKSLASH => {
                let ch = if let Some(&next_char) = chars.peek() {
                    if in_single_quotes {
                        c
                    } else if in_double_quotes {
                        if [BACKSLASH, '$', DOUBLE_QUOTE].contains(&next_char) {
                            chars.next();
                            next_char
                        } else {
                            c
                        }
                    } else {
                        chars.next();
                        next_char
                    }
                } else {
                    continue;
                };

                current_arg.push(ch);
            }
            c if c.is_whitespace() && !in_single_quotes && !in_double_quotes => {
                if !current_arg.is_empty() { // If we hit whitespace outside quotes, it's a delimiter
                    args.push(std::mem::take(&mut current_arg)); // Push the completed arg
                }

                while let Some(&next_char) = chars.peek() {
                    if next_char.is_whitespace() {
                        chars.next();
                    } else {
                        break;
                    }
                }
            }
            _ => {
                // Any other character (or whitespace inside quotes) is part of the current arg
                current_arg.push(c);
            }
        }
    }

    // Add the last argument if it wasn't teminated by whitespace
    if !current_arg.is_empty() {
        args.push(current_arg);
    }

    // Check for unterminated quotes
    if in_double_quotes || in_single_quotes {
        Err("Unterminated single quote in arguments".to_string())
    } else {
        Ok(args)
    }
}

// Find 
fn find_exec_in_fs(path: &str, name: &str) -> io::Result<String> {
    for p in path.split(":") {
        // Use metadata to check if if's a directory and handle potential errors
        if let Ok(metadata) = fs::metadata(p) {
            if !metadata.is_dir() {
                continue; // Skip if not a directory
            }
        } else {
            continue; // Skip if error reading metadata (e.g., path doesn't exist)
        }

        if let Ok(entries) = fs::read_dir(p) { // Handle potential read_dir error
            for entry in entries {
                if let Ok(entry) = entry { // Handle potential entry error
                    let filename = entry.file_name();
                    // Ensure comparison works correctly (OsString to &str)
                    if filename.to_string_lossy() == name {
                        // Construct the full path
                        let full_path = entry.path().to_string_lossy().into_owned();
                        // Basic check if it's executable (more robust checks exist)
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            if let Ok(metadata) = entry.metadata() {
                                if metadata.is_file() && (metadata.permissions().mode() & 0o111) != 0 {
                                    return Ok(full_path);
                                }
                            }
                        }
                        #[cfg(not(unix))] // Basic check for non-unix
                        {
                            if let Ok(metadata) = entry.metadata() {
                                if metadata.is_file() {
                                    return Ok(full_path); // Simplistic check non-unix
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Err(io::Error::new(io::ErrorKind::NotFound, "Executable not found in path"))
}


// Find an executable in PATHs
fn find_exec_in_path(name: &str) -> Option<String> {
    if name.contains('/') { // If it's already a path, check directly (basic check)
        if let Ok(metadata) = fs::metadata(name) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.is_file() && (metadata.permissions().mode() & 0o111) != 0 {
                    return Some(name.to_string());
                }
            }
            #[cfg(not(unix))]
            {
                if metadata.is_file() {
                    return Some(name.to_string());
                }
            }
        }
        return None; // Not a valid executable path
    }

    let path_var = env::var("PATH").ok()?;
    find_exec_in_fs(&path_var, name).ok()
}


fn type_buildin(name: &str) -> String {
    // Check builtins first
    if ["echo", "exit", "type", "pwd", "cd"].contains(&name) {
        return format!("{} is a shell builtin", name);
    }

    // Check executable in PATH
    if let Some(full_path) = find_exec_in_path(name) {
        return format!("{} is {}", name, full_path);
    }

    format!("{} not found", name)
}

// Update try_call to set argv[0] correctly using arg0 on Unix
#[cfg(unix)] // Use arg0 method only on Unix platforms
fn try_call(command_name: &str, command_path: &str, args: &[String]) -> Result<(), String> {
    let mut command_proc = Command::new(command_path); // Specify the *path* to execute
    command_proc.arg0(command_name); // Specify the desired argv[0]
    command_proc.args(args);         // Specify argv[1], argv[2], ...

    let mut child = command_proc.spawn().map_err(|err| {
        if err.kind() == io::ErrorKind::NotFound {
           format!("Failed to execute '{}': {}", command_path, err)
        } else {
            format!("Failed to spawn process '{}': {}", command_path, err)
        }
    })?;

    match child.wait() {
       Ok(status) => {
           if status.success() {
               Ok(())
           } else {
               // Optionally suppress status errors unless debugging, or just return Ok(())
               // to mimic basic shell behavior (doesn't usually print errors for non-zero exits)
               // For now, let's keep the error message for clarity during development:
                Err(format!("Command '{}' exited with status: {}", command_name, status))
           }
       },
       Err(err) => Err(format!("Failed to wait for command '{}': {}", command_name, err)),
    }
}

#[cfg(not(unix))] // Fallback for non-Unix platforms (Windows, etc.)
fn try_call(_command_name: &str, command_path: &str, args: &[String]) -> Result<(), String> {
    // On non-Unix, arg0 is not available.
    // We fall back to the previous behavior where argv[0] might be the full path.
    // This might not pass the specific test requirement if the test runs on non-Unix,
    // but the test environment (like CodeCrafters) is typically Unix-based.
    let mut command_proc = Command::new(command_path);
    command_proc.args(args);

    let mut child = command_proc.spawn().map_err(|err| {
        if err.kind() == io::ErrorKind::NotFound {
           format!("Failed to execute '{}': {}", command_path, err)
        } else {
            format!("Failed to spawn process '{}': {}", command_path, err)
        }
    })?;

     match child.wait() {
       Ok(status) => {
           if status.success() {
               Ok(())
           } else {
               Err(format!("Command '{}' exited with status: {}", command_path, status)) // Use command_path here as name isn't guaranteed correct
           }
       },
       Err(err) => Err(format!("Failed to wait for command '{}': {}", command_path, err)),
    }
}

fn change_dir(path: &str) {
    let target_path = if path == "~" {
        match env::var("HOME") {
            Ok(home) => home,
            Err(_) => {
                println!("cd: HOME environment variable not set");
                return;
            }
        }
    } else {
        path.to_string()
    };

    if env::set_current_dir(&target_path).is_err() {
        println!("cd: {}: No such file or directory", target_path);
    }
}

fn handle_echo(args: &[String]) {
    if args.is_empty() {
        println!();
    } else {
        // Join the already parsed arguments with a single space
        println!("{}", args.join(" "));
    }
}

fn main() {
    loop {
        print!("$ ");
        io::stdout().flush().unwrap();

        // Wait for user input
        let stdin = io::stdin();
        let mut input = String::new();
        if stdin.read_line(&mut input).unwrap() == 0 {
            println!(); // Handle CTRL + D (EOF) gracefully
            break;
        }

        let input = input.trim();
        if input.is_empty() {
            continue;
        }

        let tokens: Vec<String> = match parse_arguments(input) {
            Ok(parsed_tokens) => {
                if parsed_tokens.is_empty() {
                    // If parsing results in nothing (e.g. input was just quotes), continue
                    continue;
                }
                parsed_tokens
            }
            Err(e) => {
                println!("Parse error: {}", e);
                continue; // Skip to next command on parse error
            }
        };

        let (command_name, args) = tokens.split_first().unwrap(); // Safe dut to empty check above

        match command_name.as_str() {
            "exit" => {
                // Default exit code 0 if not specified or invalid
                let code = args.get(0).and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
                std::process::exit(code);
            }
            "echo" => handle_echo(args), // Pass the parsed String args
            "pwd" => {
                if args.is_empty() {
                    match env::current_dir() {
                        Ok(dir) => println!("{}", dir.display()),
                        Err(e) => println!("pwd: error getting current directory: {}", e),
                    }
                } else {
                    println!("pwd: too many arguments");
                }
            }
            "cd" => {
                if args.len() == 1 {
                    change_dir(&args[0]);
                } else if args.is_empty() {
                    // Go home if 'cd' is called with no arguments
                    change_dir("~");
                } else {
                    println!("cd: too many arguments");
                }
            }
            "type" => {
                if args.len() == 1 {
                    println!("{}", type_buildin(&args[0]));
                } else {
                    println!("type: requires exactly one argument");
                }
            }
            // Handle external commands
            cmd => {
                // Find the executable path using the updated function
                match find_exec_in_path(cmd) {
                    Some(full_path) => {
                        // Call try_call with the full path and parsed args
                        if let Err(e) = try_call(cmd, &full_path, args) {
                            println!("{}", e); // Print execution errors
                        }
                    }
                    None => {
                        println!("{}: command not found", cmd);
                    }
                }
            }
        }
    }
}
