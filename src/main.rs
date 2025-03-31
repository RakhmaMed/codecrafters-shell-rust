#![allow(clippy::comparison_to_empty)] // Allow compating String to "" for specific error handling

use std::env;
use std::fs;
use std::fs::File;
use std::io::{self, ErrorKind, Read, Write};
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
                        return Ok(Some(path.to_string_lossy().into_owned()));
                    }
                }
            } else {
                eprintln!(
                    "shell: warning: could not get metadata for '{}'",
                    path.display()
                );
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
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if (metadata.permissions().mode() & 0o111) != 0 {
                        return Some(name.to_string());
                    }
                }
                #[cfg(not(unix))]
                {
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
    let target_path_str = match args.iter().as_slice() {
        [] => "~", // Default to home directory
        [path] => path.as_str(),
        _ => return Err("cd: too many arguments".to_string()),
    };
    // change_dir returns Result<(), String>. Map Ok(()) to Ok(None).
    change_dir(target_path_str).map(|_| None)
}

fn execute_external_command(
    command_name: &str,
    command_path: &str,
    args: &[String],
    stdout_redirection_file: Option<&str>, // Renamed for clarity
    stderr_redirection_file: Option<&str>, // New parameter
) -> Result<Option<String>, String> {
    // Return type remains the same

    let mut command = Command::new(command_path);

    // Conditionally set argv[0] on Unix systems
    #[cfg(unix)]
    {
        command.arg0(command_name);
    }
    // Add the rest of the arguments
    command.args(args);

    // --- Configure stdout ---
    let mut stdout_redir_file_handle: Option<File> = None; // Keep handle alive
    let stdout_config = match stdout_redirection_file {
        Some(filename) => {
            let file = File::create(filename).map_err(|e| {
                format!("failed to open stdout redirect file '{}': {}", filename, e)
            })?;
            let stdio_handle = file
                .try_clone()
                .map_err(|e| format!("failed to clone stdout file handle for redirect: {}", e))?;
            stdout_redir_file_handle = Some(file); // Store original handle
            Stdio::from(stdio_handle)
        }
        None => Stdio::piped(), // Capture stdout if not redirecting
    };
    command.stdout(stdout_config);

    // --- Configure stderr ---
    let mut stderr_redir_file_handle: Option<File> = None; // Keep handle alive
    let stderr_config = match stderr_redirection_file {
        Some(filename) => {
            let file = File::create(filename).map_err(|e| {
                format!("failed to open stderr redirect file '{}': {}", filename, e)
            })?;
            let stdio_handle = file
                .try_clone()
                .map_err(|e| format!("failed to clone stderr file handle for redirect: {}", e))?;
            stderr_redir_file_handle = Some(file); // Store original handle
            Stdio::from(stdio_handle)
        }
        None => Stdio::inherit(), // Default: Inherit stderr if not redirecting
    };
    command.stderr(stderr_config);

    // Spawn the command
    let mut child = command.spawn().map_err(|e| {
        if e.kind() == ErrorKind::NotFound {
            format!("{}: command not found", command_name)
        } else {
            format!("failed to execute command '{}': {}", command_name, e)
        }
    })?;

    // Handle output / waiting
    let mut captured_stdout = String::new();
    // Only read stdout if it was piped (i.e., not redirected to a file)
    if stdout_redirection_file.is_none() {
        if let Some(mut child_stdout) = child.stdout.take() {
            // Reading can fail, report but proceed to wait for status
            if let Err(e) = child_stdout.read_to_string(&mut captured_stdout) {
                // Use eprintln directly as this is an intermediate shell warning
                // This warning itself won't be redirected by 2> applied to the child command.
                eprintln!("shell: warning: error reading command stdout pipe: {}", e);
            }
        }
    }
    // Note: We are not capturing stderr if it was piped (we set it to inherit or file).

    // Wait for the command to complete to get its exit status
    let status = child
        .wait()
        .map_err(|e| format!("failed to wait for command '{}': {}", command_name, e))?;

    // Ensure file handles are dropped *after* the child process finishes.
    drop(stdout_redir_file_handle);
    drop(stderr_redir_file_handle);

    // Check if stdout was captured and print it *before* determining Ok/Err based on status
    let mut stdout_printed = false;
    if stdout_redirection_file.is_none() && !captured_stdout.is_empty() {
        // Stdout was piped (not redirected by >) and we captured something. Print it now.
        print!("{}", captured_stdout);
        // Flush stdout to ensure it's visible immediately
        io::stdout()
            .flush()
            .unwrap_or_else(|e| eprintln!("shell: error flushing stdout: {}", e));
        stdout_printed = true;
    }

    // Determine final result based ONLY on exit status now
    if status.success() {
        // If success and we already printed captured stdout, return Ok(None).
        // If success and stdout was redirected by >, return Ok(None).
        Ok(None) // Simplified: Success means no further action needed from main regarding output.
    } else {
        // Command failed (non-zero exit).
        // Stdout (if captured) was already printed above.
        // Stderr was handled via inheritance or redirection to file.
        // Signal failure (non-zero exit) without the shell printing more error info.
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
            Ok(0) => {
                // EOF detected
                println!(); // Print a newline for clean exit
                break;
            }
            Ok(_) => {} // Successfully read line
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

        // --- Redirection Parsing (Handles >/>1/2> at the end) ---
        let mut remaining_args = args_slice.to_vec(); // Clone args to modify
        let mut stdout_redirect_filename: Option<String> = None;
        let mut stderr_redirect_filename: Option<String> = None;

        // Loop backwards through arguments checking for redirection operators
        // This handles cases like `cmd arg1 > out.txt 2> err.txt` or `cmd arg1 2> err.txt > out.txt`
        loop {
            let len = remaining_args.len();
            if len < 2 {
                break; // Need at least operator + filename
            }

            let operator = &remaining_args[len - 2];
            let filename = &remaining_args[len - 1];

            match operator.as_str() {
                ">" | "1>" => {
                    // Silently overwrite previous stdout redirect if found again
                    stdout_redirect_filename = Some(filename.clone());
                    remaining_args.truncate(len - 2); // Remove processed args
                }
                "2>" => {
                    // Silently overwrite previous stderr redirect if found again
                    stderr_redirect_filename = Some(filename.clone());
                    remaining_args.truncate(len - 2); // Remove processed args
                }
                _ => {
                    // Last two args are not a recognized redirection, stop parsing redirects
                    break;
                }
            }
        }
        // `remaining_args` now contains only the actual command arguments
        let command_args = remaining_args;
        // Get Option<&str> versions for passing to functions if needed
        let stdout_redirect_ref = stdout_redirect_filename.as_deref();
        let stderr_redirect_ref = stderr_redirect_filename.as_deref();
        // --- End Redirection Parsing ---

        let result: Result<Option<String>, String> = match command_name.as_str() {
            "exit" => {
                // Use command_args which excludes redirection parts
                let code = command_args
                    .first()
                    .and_then(|s| s.parse::<i32>().ok())
                    .unwrap_or(0);
                std::process::exit(code);
            }
            // Built-in handlers still only take command_args
            "echo" => handle_echo(&command_args),
            "pwd" => handle_pwd(&command_args),
            "cd" => handle_cd(&command_args),
            "type" => handle_type(&command_args),
            // --- External Command ---
            cmd => {
                match find_exec_in_path(cmd) {
                    Some(full_path) => {
                        // Pass both stdout and stderr redirection filenames
                        execute_external_command(
                            cmd,
                            &full_path,
                            &command_args,
                            stdout_redirect_ref, // Pass ref
                            stderr_redirect_ref, // Pass ref
                        )
                    }
                    None => Err(format!("{}: command not found", cmd)),
                }
            }
        };

        // Handle command result (Output or Error)
        // Handle command result (Output or Error)
        match result {
            Ok(Some(output_str)) => {
                // Output received. Could be from:
                // 1. Built-in success (e.g., echo, pwd).
                // 2. External command success where stdout was piped/captured.
                if let Some(filename) = stdout_redirect_ref {
                    // Stdout redirection ('>') requested.
                    // This indicates output from a BUILT-IN should be redirected.
                    match File::create(filename) {
                        Ok(mut file) => {
                            if let Err(e) = file.write_all(output_str.as_bytes()) {
                                eprintln!("shell: error writing built-in output to stdout redirect file '{}': {}", filename, e);
                            }
                        }
                        Err(e) => {
                            eprintln!(
                                "shell: failed to open stdout redirect file '{}': {}",
                                filename, e
                            );
                        }
                    }
                    // Ensure stderr file exists if 2> was also used with a successful built-in
                    if let Some(filename) = stderr_redirect_ref {
                        if File::create(filename).is_err() { /* Optional warning */ }
                    }
                } else {
                    // No stdout redirection ('>'). Print the output string directly to terminal.
                    // (From successful built-in OR successful external command with captured stdout)
                    print!("{}", output_str);
                    io::stdout()
                        .flush()
                        .unwrap_or_else(|e| eprintln!("shell: error flushing stdout: {}", e));
                    // Ensure stderr file exists if 2> was used (e.g. successful external cmd with 2>)
                    if let Some(filename) = stderr_redirect_ref {
                        if File::create(filename).is_err() { /* Optional warning */ }
                    }
                }
            }
            Ok(None) => {
                // Command succeeded. No output string returned by execute_external_command/builtin handler.
                // Means:
                // 1. External command successfully redirected stdout via '>'. File writing completed internally.
                // 2. Built-in command had no output (e.g., cd).
                // We only need to ensure the *stderr* file exists if `2>` was specified.
                if let Some(filename) = stderr_redirect_ref {
                    // Ensure file exists (e.g., for `ls > out.txt 2> err.txt` or `cd 2> err.txt`)
                    if File::create(filename).is_err() { /* Optional warning */ }
                }
                // *** DO NOT TOUCH stdout_redirect_ref file here ***
            }
            Err(err_msg) => {
                // Command failed (external non-zero exit OR built-in returned Err)
                // External command output/error (stdout print, stderr inherit/redirect) handled by execute_external_command.

                if !err_msg.is_empty() {
                    // Non-empty means a BUILT-IN command failed or shell error. Handle its stderr redirection.
                    if let Some(filename) = stderr_redirect_ref {
                        match File::create(filename) {
                            // Create/truncate stderr file for built-in error
                            Ok(mut file) => {
                                if let Err(_) = writeln!(file, "{}", err_msg) { /* ... error handling ... */
                                }
                            }
                            Err(_) => { /* ... error handling ... */ }
                        }
                    } else {
                        // No stderr redirection for built-in error, print to terminal stderr.
                        eprintln!("{}", err_msg);
                    }
                    // Ensure stdout file exists if > was specified when a built-in failed
                    if let Some(filename) = stdout_redirect_ref {
                        if File::create(filename).is_err() { /* Optional warning */ }
                    }
                }
                // else: err_msg is empty (external command failed).
                // Its stdout (if captured) was printed by execute_external_command.
                // Its stdout/stderr redirection was handled via Stdio in execute_external_command.
                // *** DO NOT TOUCH stdout_redirect_ref or stderr_redirect_ref files here ***
            }
        }
        // match end
    }
}
