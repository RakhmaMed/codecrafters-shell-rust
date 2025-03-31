#![allow(clippy::comparison_to_empty)] // Allow compating String to "" for specific error handling

use std::env;
use std::fs;
use std::fs::File;
use std::io::{self, Write, Read, ErrorKind};
#[cfg(unix)] // for arg0
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

// --- Argument Parsing (with backslash fox) ---

const BACKSLASH: char = '\\';
const SINGLE_QUOTE: char = '\'';
const DOUBLE_QUOTE: char = '"';

// Function to parse arguments respecting single quotes
fn parse_tokens(input_args: &str) -> Result<Vec<String>, String> {
    let mut args: Vec<String> = Vec::new();
    let mut current_arg = String::new();
    let mut in_double_quotes = false;
    let mut in_single_quotes = false;
    let mut chars = input_args.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            SINGLE_QUOTE => {
                if in_double_quotes {
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
                // Peek at the next character for escape sequence handling
                let next_char_opt = chars.peek().copied(); // Use copied for convernience

                let ch_to_push = if in_single_quotes {
                    // Inside single quoutes, backslash is literal unless escaping a signle quoute itself?
                    // Standard sh: No escapes inside single quotes. Let's stick to that.
                    c
                } else if in_double_quotes {
                    // Inside double quotes, only certain characters are escaped by backslash
                    if let Some(next_char) = next_char_opt {
                        match next_char {
                            '$' | '`' | '"' | '\\' => {
                                chars.next(); // Consume the escaped character
                                next_char
                            }
                            _ => c, // Backslash is literal otherwise inside double quotes
                        }
                    } else {
                        c // Backslash at the very end of input inside double quoutes
                    }
                } else {
                    // Outside quotes, backslash escapes the next character
                    if let Some(next_char) = next_char_opt {
                        chars.next(); // Consume the escaped character
                        next_char
                    } else {
                        c // Backslash at the very end of input outside quotes
                    }
                };
                current_arg.push(ch_to_push);
            }
            ws if ws.is_whitespace() && !in_single_quotes && !in_double_quotes => {
                // Whitespace outside quotes acts as a delimiter
                if !current_arg.is_empty() {
                    args.push(std::mem::take(&mut current_arg)); // Push the completed arg
                }
                // Skip consecutive whitespace
                while let Some(&next_char) = chars.peek() {
                    if next_char.is_whitespace() {
                        chars.next();
                    } else {
                        break;
                    }
                }
            }
            _ => {
                // Any other character is part of the current arg
                current_arg.push(c);
            }
        }
    }

    // Add the last argument if it wasn't teminated by whitespace
    if !current_arg.is_empty() {
        args.push(current_arg);
    }

    // Check for unterminated quotes
    if in_double_quotes {
        Err("Unterminated double quote in arguments".to_string())
    } else if in_single_quotes {
        Err("Unterminated single quote in arguments".to_string())
    } else {
        Ok(args)
    }
}

// Find
fn find_exec_in_dir(dir_path: &str, name: &str) -> io::Result<Option<String>> {
    let entries = match fs::read_dir(dir_path) {
        Ok(entries) => entries,
        // Ignore directories in PATH that don't exist or aren't directories
        Err(e) if e.kind() == ErrorKind::NotFound /* || e.kind() == ErrorKind::NotADirectory */ => return Ok(None),
        Err(e) => return Err(e),
    };

    for entry_result in entries {
        // Handle errors reading specific entries within a directory
        let entry = match entry_result {
            Ok(entry) => entry,
            Err(_) => {
                // eprintln!("shell: warning: error reading entry in directory '{}': {}", dir_path, e);
                continue; // Skip this entry, try others
            }   
        };

        if entry.file_name().to_string_lossy() == name {
            let path = entry.path();
            // Handle errors getting meadata for a spceific file
            if let Ok(metadata) = entry.metadata() {
                if metadata.is_file() {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        // Check execute permission for user, group, or other
                        if (metadata.permissions().mode() & 0o111) != 0 {
                            return Ok(Some(path.to_string_lossy().into_owned()));
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        // Basic check for non-unix: just check if it's a file
                        return Ok(Some(path.to_string_lossy().into_owned()))
                    }
                }
            } else {
                eprintln!("shell: warning: could not get metadata for '{}'", path.display());
            }
        }
    }

    Ok(None) // Not found in this directory
}

// Find an executable in PATHs
fn find_exec_in_path(name: &str) -> Option<String> {
    // If name contains '/', treat it as a direct path attempt
    if name.contains('/') {
        // If it's already a path, check directly (basic check)
        if let Ok(metadata) = fs::metadata(name) {
            if metadata.is_file() {
                #[cfg(unix)] {
                    use std::os::unix::fs::PermissionsExt;
                    if (metadata.permissions().mode() & 0o111) != 0 {
                        return Some(name.to_string());
                    }
                }
                #[cfg(not(unix))] {
                    return Some(name.to_string());
                }
            }
        }
        // Direct path provided but not a valid executable file
        return None;
    }

    // Search in PATH environment variable
    if let Ok(path_var) = env::var("PATH") {
        for dir in path_var.split(':').filter(|d| !d.is_empty()) {
            match find_exec_in_dir(dir, name) {
                Ok(Some(full_path)) => return Some(full_path),
                Ok(None) => continue, // Not found in this dir
                Err(_) => {
                    // Optionally log fs errors for specific dirs, but don't halt search
                    // eprintln!("shell: warning: error searching PATH directory '{}': {}", dir, e);
                    continue;
                }
            }
        }
    }

    None // Not found in PATH
}


// --- Builtin Commands ---
// Consistent Return type: Result<Option<String>, String>
// Ok(Some(output)) -> Success, print this output to stdout
// Ok(None)         -> Success, no output to print (e.g., cd, or output redirected)
// Err(message)     -> Failure, print this message to stderr (non-empty message)
// Err("")          -> Failure (non-zero exit), stderr already handled via inherit

fn handle_echo(args: &[String]) -> Result<Option<String>, String> {
    // Standard echo adds a newline
    Ok(Some(format!("{}\n", args.join(" "))))
}

fn handle_pwd(_args: &[String]) -> Result<Option<String>, String> {
    // Ignore args for standard pwd behavior
    match env::current_dir() {
        Ok(dir) => Ok(Some(format!("{}\n", dir.display()))),
        Err(e) => Err(format!("pwd: error getting current directory: {}", e)),
    }
}

// Helper for handle_type - determines the type information string
fn type_info_string(name: &str) -> String {
    if ["echo", "exit", "type", "pwd", "cd"].contains(&name) {
        format!("{} is a shell builtin", name)
    } else if let Some(full_path) = find_exec_in_path(name) {
        format!("{} is {}", name, full_path)
    } else {
        format!("{}: not found", name)
    }
}

fn handle_type(args: &[String]) -> Result<Option<String>, String> {
    match args.iter().as_slice() {
        [name] => Ok(Some(format!("{}\n", type_info_string(name)))),
        [] => Err("type: missing argument".to_string()),
        _ => Err("type: too many arguments".to_string()),
    }
}

// Helper for handle_cd - performs the directory change
fn change_dir(target_path_str: &str) -> Result<(), String> {
    // Resilt "~" to home directory
    let target_path = if target_path_str == "~" || target_path_str == "~/" {
        env::var("HOME").map_err(|_| "cd: HOME environment variable not set".to_string())?
    } else {
        target_path_str.to_string()
    };

    // Atempt to change directory
    env::set_current_dir(&target_path)
        .map_err(|_| format!("cd: {}: No such file or directory", target_path))
}

fn handle_cd(args: &[String]) -> Result<Option<String>, String> {
    let target_path_str  = match args.iter().as_slice() {
        [] => "~", // Default to home directory
        [path] => path.as_str(),
        _ => return Err("cd: too many arguments".to_string())
    };
    // change_dir returns Result<(), String>. Map Ok(()) to Ok(None).
    change_dir(target_path_str).map(|_| None)
}


fn execute_external_command(
    command_name: &str,
    command_path: &str,
    args: &[String],
    redirection_file: Option<&str>,
) -> Result<Option<String>, String> {
    
    let mut command = Command::new(command_path);

    // Conditionally set argv[0] on Unix systems
    #[cfg(unix)]
    {
        command.arg0(command_name);
    }
    // Add the res of the arguments
    command.args(args);

    // Stderr always goes to the terminal in this version
    command.stderr(Stdio::inherit());

    // Configure stdout based on redirection
    let mut redir_file_handle: Option<File> = None;
    let stdout_config = match redirection_file {
        Some(filename) => {
            // Attempt to create/trancate the file for writing
            let file = File::create(filename)
                .map_err(|e| format!("failed to open redirect file '{}': {}", filename, e))?;
            // Clone the handle for Stdio, store original to ensure it stays open
            let stdio_handle = file.try_clone()
                .map_err(|e| format!("failed to clone file handle for redirect: {}", e))?;
            redir_file_handle = Some(file);
            Stdio::from(stdio_handle)
        }
        None => Stdio::piped(), // Capture stdout if not redirecting
    };

    command.stdout(stdout_config);

    // Spawnd the command
    let mut child = command.spawn().map_err(|e| {
        if e.kind() == ErrorKind::NotFound {
            // Use command_name fpr user-facing error, path might be confusing
            format!("{}: command not found", command_name)
        } else {
            format!("failed to execute command '{}': {}", command_name, e)
        }
    })?;

    // Handle output / waiting
    let mut captured_stdout = String::new();
    if redirection_file.is_none() {
        // If stdout is piped, read it *before* waiting
        if let Some(mut child_stdout) = child.stdout.take() {
            // Reading can fail, report but preceed to wait for status
            if let Err(e) = child_stdout.read_to_string(&mut captured_stdout) {
                // Use eprintln directly as this is an intermediate shell warning
                eprintln!("shell: warning: error reading command stdout pipe: {}", e);
            }
        }
    }

    // Wait for the command to complete to get its exit status
    let status = child.wait()
        .map_err(|e| format!("failed to wait for command '{}': {}", command_name, e))?;

    drop(redir_file_handle);

    // Determine final result based on exit status
    if status.success() {
        if redirection_file.is_none() {
            Ok(Some(captured_stdout)) // Success, return captures stdout
        } else {
            Ok(None) // Success, stdout went to file
        }
    } else {
        // Command failed (non-zero exit). Stderr was already printed via inherit.
        // Return an empty error string to signal failure without the shell printing more.
        Err(String::new())
    }
}


// --- Main Loop ---
fn main() {
    loop {
        print!("$ ");
        io::stdout().flush().unwrap();

        // Wait for user input
        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(0) => { // EOF detected
                println!(); // Print a newline for clean exit
                break;
            },
            Ok(_) => {}, // Successfully read line
            Err(e) => {
                eprintln!("shell: input error: {}", e);
                break; // Exit on read error
            }
        }

        let input = input.trim();
        if input.is_empty() {
            continue; // Show prompt again if input is only whitespace
        }

        // Parse tokens
        let tokens: Vec<String> = match parse_tokens(input) {
            Ok(parsed_token) if parsed_token.is_empty() => continue, // Input was only quotes, etc.
            Ok(parsed_tokens) => parsed_tokens,
            Err(e) => {
                println!("Parse error: {}", e);
                continue; // Skip to next command
            }
        };

        // Safe unwrap due to empty check above
        let (command_name, args_slice) = tokens.split_first().unwrap();

        // --- Simple Redirection Parsing (handles "> file" at the end) ---
        let mut command_args: Vec<String> = Vec::new();
        let mut redirection_filename: Option<&str> = None;
        let mut skip_next_arg = false; // Flag to skip filename after '>'

        for (i, arg) in args_slice.iter(). enumerate() {
            if skip_next_arg {
                skip_next_arg = false;
                continue;
            }

            // Check if current arg is '>' or '1>' AND it's the second-to-last arg
            if (arg == ">" || arg == "1>") && i == args_slice.len() - 2 {
                // The next argument is the filename
                redirection_filename = Some(&args_slice[i + 1]);
                skip_next_arg = true; // Skip the filename in the next iteration
            } else {
                // Not a redirection operator at the end, treat as a normal argument
                command_args.push(arg.clone());
            }
        }
        // --- End Redirection Parsing ---

        let result: Result<Option<String>, String> = match command_name.as_str() {
            "exit" => {
                // Use command_args which excludes redirection parts
                let code = command_args.first().and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
                std::process::exit(code);
            }
            "echo" => handle_echo(&command_args),
            "pwd" => handle_pwd(&command_args),
            "cd" => handle_cd(&command_args),
            "type" => handle_type(&command_args),
            // --- External Command ---
            cmd => {
                match find_exec_in_path(cmd) {
                    Some(full_path) => {
                        // Pass the parsed command_args and optional filename
                        execute_external_command(cmd, &full_path, &command_args, redirection_filename)
                    }
                    None => Err(format!("{}: command not found", cmd)),
                }
            }
        };

        // Handle command result (Output or Error)
        match result {
            Ok(Some(output_str)) => {
                // Check if redirection was requested for this command
                if let Some(filename) = redirection_filename {
                    // Attempt to create/truncate the file for writing
                    match File::create(filename) {
                        Ok(mut file) => {
                            // Write the output string (produced by the built-in) to the file
                            if let Err(e) = file.write_all(output_str.as_bytes()) {
                                // Report error writing to the redirected file
                                eprintln!("shell: error writing to redirect file '{}': {}", filename, e);
                                // Potentially set an error status ($?) here in the future
                            }
                            // Successfully wrote (or tried to write) to file, don't print to stdout.
                        }
                        Err(e) => {
                            // Report error opening the redirected file
                            eprintln!("shell: failed to open redirect file '{}': {}", filename, e);
                            // Potentially set an error status ($?) here in the future
                        }
                    }
                } else {
                    // No redirection, print the output to stdout as before
                    print!("{}", output_str);
                    io::stdout().flush().unwrap_or_else(|e| eprintln!("shell: error flushing stdout: {}", e));
                }
            }
            Ok(None) => {
                // Command succeeded, no stdout to print (e.g., cd, external cmd redirected internally)
                // Successfully executed, do nothing more.
            }
            Err(err_msg) => {
                // Command failed OR shell encountered an error executing it.
                if !err_msg.is_empty() {
                    // Print specific error from shell/builtin handlers
                    eprintln!("{}", err_msg);
                }
                // If err_msg is empty, it implies non-zero exit from external command,
                // and stderr was already handled by Stdio::inherit. Don't print anything more.

                // Future enhancement: Set a last exit code variable here ($?).
            }
        }
    }
}
