#![allow(clippy::comparison_to_empty)] // Allow Err("") for external command failure status

use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt; // For execute bits
#[cfg(unix)]
use std::os::unix::process::CommandExt; // For arg0
use std::process::{Command, Stdio};

// --- Constants ---
const BACKSLASH: char = '\\';
const SINGLE_QUOTE: char = '\'';
const DOUBLE_QUOTE: char = '"';

// --- Argument Parsing ---

/// Parses a command line string into arguments, respecting shell quoting and escaping.
/// Handles single quotes (''), double quotes (""), and backslash (\) escapes.
/// Returns Err on unterminated quotes.
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
                    current_arg.push(c);
                } else {
                    in_single_quotes = !in_single_quotes; // Toggle state, don't add quote
                }
            }
            DOUBLE_QUOTE => {
                if in_single_quotes {
                    current_arg.push(c);
                } else {
                    in_double_quotes = !in_double_quotes; // Toggle state, don't add quote
                }
            }
            BACKSLASH => {
                // Backslash behavior depends on quoting context
                let next_char_opt = chars.peek().copied();
                let ch_to_push = if in_single_quotes {
                    c // Literal backslash inside single quotes
                } else if in_double_quotes {
                    // Limited escaping inside double quotes
                    if let Some(next) = next_char_opt {
                        match next {
                            '$' | '`' | '"' | '\\' => {
                                chars.next();
                                next
                            } // Consume escaped char
                            _ => c, // Literal backslash otherwise
                        }
                    } else {
                        c
                    } // Literal backslash at end
                } else {
                    // Unquoted: escape next char, or literal backslash if at end
                    if let Some(next) = next_char_opt {
                        chars.next();
                        next
                    } else {
                        c
                    }
                };
                current_arg.push(ch_to_push);
            }
            ws if ws.is_whitespace() && !in_single_quotes && !in_double_quotes => {
                // Whitespace outside quotes delimits arguments
                if !current_arg.is_empty() {
                    args.push(std::mem::take(&mut current_arg));
                }
                // Consume subsequent whitespace
                while chars.peek().map_or(false, |ch| ch.is_whitespace()) {
                    chars.next();
                }
            }
            _ => {
                // Any other character is part of the current argument
                current_arg.push(c);
            }
        }
    }

    // Add the last argument if any
    if !current_arg.is_empty() {
        args.push(current_arg);
    }

    // Check for parsing errors (unclosed quotes)
    if in_double_quotes {
        Err("Unterminated double quote in arguments".to_string())
    } else if in_single_quotes {
        Err("Unterminated single quote in arguments".to_string())
    } else {
        Ok(args)
    }
}

// --- Command Searching ---

/// Searches a single directory for an executable file name. Checks execute bits on Unix.
/// Skips directories that are NotFound or inaccessible, returns other IO errors.
fn find_exec_in_dir(dir_path: &str, name: &str) -> io::Result<Option<String>> {
    let entries = match fs::read_dir(dir_path) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None), // Skip non-existent dirs in PATH
        Err(e) => return Err(e),                                      // Propagate other errors
    };

    for entry_result in entries {
        if let Ok(entry) = entry_result {
            // Ignore errors reading specific entries
            if entry.file_name().to_string_lossy() == name {
                if let Ok(metadata) = entry.metadata() {
                    // Ignore errors getting metadata
                    if metadata.is_file() {
                        #[cfg(unix)]
                        {
                            // Check execute permission (user, group, or other)
                            if (metadata.permissions().mode() & 0o111) != 0 {
                                return Ok(Some(entry.path().to_string_lossy().into_owned()));
                            }
                        }
                        #[cfg(not(unix))]
                        {
                            // Assume file is executable on non-Unix
                            return Ok(Some(entry.path().to_string_lossy().into_owned()));
                        }
                    }
                }
            }
        }
    }
    Ok(None) // Not found in this directory
}

/// Finds an executable: checks direct path if `name` contains '/', otherwise searches PATH env var.
fn find_exec_in_path(name: &str) -> Option<String> {
    if name.contains('/') {
        // Direct path check
        if let Ok(metadata) = fs::metadata(name) {
            if metadata.is_file() {
                #[cfg(unix)]
                {
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
        return None; // Direct path invalid or not found
    }

    // Search PATH
    if let Ok(path_var) = env::var("PATH") {
        for dir in path_var.split(':').filter(|d| !d.is_empty()) {
            match find_exec_in_dir(dir, name) {
                Ok(Some(full_path)) => return Some(full_path),
                Ok(None) => continue, // Check next directory
                Err(_) => continue,   // Ignore errors searching specific PATH dirs
            }
        }
    }

    None // Not found
}

// --- Builtin Command Handlers ---
// Convention: Result<Option<String>, String>
// Ok(Some(output)): Success, print output (unless redirected)
// Ok(None):          Success, no output to print (cd, redirected external)
// Err(message):      Failure (built-in/shell), print message to stderr (unless redirected)
// Err(""):           Failure (external non-zero exit), shell prints nothing further.

fn handle_echo(args: &[String]) -> Result<Option<String>, String> {
    Ok(Some(format!("{}\n", args.join(" "))))
}

fn handle_pwd(_args: &[String]) -> Result<Option<String>, String> {
    match env::current_dir() {
        Ok(dir) => Ok(Some(format!("{}\n", dir.display()))),
        Err(e) => Err(format!("pwd: error getting current directory: {}", e)),
    }
}

/// Helper: Generates the string for the 'type' command.
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
    match args {
        [name] => Ok(Some(format!("{}\n", type_info_string(name)))),
        [] => Err("type: missing argument".to_string()),
        _ => Err("type: too many arguments".to_string()),
    }
}

/// Helper: Performs the directory change for `cd`. Handles `~`.
fn change_dir(target_path_str: &str) -> Result<(), String> {
    // Expand `~` or `~/`
    let target_path = if target_path_str == "~" || target_path_str.starts_with("~/") {
        match env::var("HOME") {
            Ok(home_dir) => {
                if target_path_str.starts_with("~/") {
                    let mut path = std::path::PathBuf::from(home_dir);
                    path.push(&target_path_str[2..]); // Append path after '~/'
                    path.to_string_lossy().into_owned()
                } else {
                    home_dir // Just HOME
                }
            }
            Err(_) => return Err("cd: HOME environment variable not set".to_string()),
        }
    } else {
        target_path_str.to_string()
    };

    // Attempt change and map specific errors to expected messages
    env::set_current_dir(&target_path).map_err(|e| {
        // *** FIX: Format error message based on Kind to match test expectation ***
        let err_description = match e.kind() {
            ErrorKind::NotFound => "No such file or directory".to_string(),
            ErrorKind::PermissionDenied => "Permission denied".to_string(),
            // ErrorKind::NotADirectory => "Not a directory".to_string(),
            // Fallback for other IO errors
            _ => e.to_string(),
        };
        format!("cd: {}: {}", target_path, err_description)
    })
}

fn handle_cd(args: &[String]) -> Result<Option<String>, String> {
    let target_path_str = match args {
        [] => "~", // Default to home
        [path] => path.as_str(),
        _ => return Err("cd: too many arguments".to_string()),
    };
    // Map Ok(()) to Ok(None) for handler convention
    change_dir(target_path_str).map(|_| None)
}

// --- External Command Execution ---

/// Executes an external command, handling args, stdio redirection, and waiting.
/// Returns Ok(None) on success (exit 0), Err("") on failure (non-zero exit),
/// or Err(message) on spawn/wait errors.
fn execute_external_command(
    command_name: &str, // For arg0 and errors
    command_path: &str, // Full path to exec
    args: &[String],
    redirections: &Redirections,
) -> Result<Option<String>, String> {
    let mut command = Command::new(command_path);
    #[cfg(unix)]
    {
        command.arg0(command_name);
    } // Set argv[0] on Unix
    command.args(args);

    // --- Configure Stdio ---
    let mut stdout_handle: Option<File> = None; // Keep handles alive until wait()
    let mut stderr_handle: Option<File> = None;

    // Stdout: Redirect to file or pipe
    let stdout_stdio = match &redirections.stdout_redirect {
        Some(stdout) => match OpenOptions::new()
            .read(false)
            .write(true)
            .create(true)
            .truncate(stdout.mode == RedirectionMode::Overwrite)
            .append(stdout.mode == RedirectionMode::Append)
            .open(&stdout.filename)
        {
            Ok(file) => match file.try_clone() {
                Ok(cloned) => {
                    stdout_handle = Some(file);
                    Stdio::from(cloned)
                }
                Err(e) => return Err(format!("failed to clone stdout file handle: {}", e)),
            },
            Err(e) => {
                return Err(format!(
                    "failed to open stdout redirect file '{}': {}",
                    stdout.filename, e
                ))
            }
        },
        None => Stdio::piped(), // Pipe if not redirecting
    };
    command.stdout(stdout_stdio);

    // Stderr: Redirect to file or inherit
    let stderr_stdio = match &redirections.stderr_redirect {
        Some(stderr) => match OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(stderr.mode == RedirectionMode::Overwrite)
            .append(stderr.mode == RedirectionMode::Append)
            .open(&stderr.filename)
        {
            Ok(file) => match file.try_clone() {
                Ok(cloned) => {
                    stderr_handle = Some(file);
                    Stdio::from(cloned)
                }
                Err(e) => return Err(format!("failed to clone stderr file handle: {}", e)),
            },
            Err(e) => {
                return Err(format!(
                    "failed to open stderr redirect file '{}': {}",
                    stderr.filename, e
                ))
            }
        },
        None => Stdio::inherit(), // Inherit shell's stderr if not redirecting
    };
    command.stderr(stderr_stdio);

    // --- Spawn and Wait ---
    let mut child = command.spawn().map_err(|e| {
        match e.kind() {
            ErrorKind::NotFound => format!("{}: command not found (spawn error)", command_name), // Should be rare
            ErrorKind::PermissionDenied => format!("{}: Permission denied", command_name),
            _ => format!("failed to execute command '{}': {}", command_name, e),
        }
    })?;

    // Capture stdout only if it was piped
    let mut captured_stdout = String::new();
    if redirections.stdout_redirect.is_none() {
        if let Some(mut child_stdout) = child.stdout.take() {
            if let Err(e) = child_stdout.read_to_string(&mut captured_stdout) {
                // Non-fatal error reading pipe, warn but proceed
                eprintln!("shell: warning: error reading command stdout pipe: {}", e);
            }
        }
    }

    // Wait for the command to finish and get exit status
    let status = child
        .wait()
        .map_err(|e| format!("failed to wait for command '{}': {}", command_name, e))?;

    // Ensure handles are dropped *after* wait()
    drop(stdout_handle);
    drop(stderr_handle);

    // Print captured stdout if any *before* checking status
    if !captured_stdout.is_empty() {
        print!("{}", captured_stdout);
        io::stdout()
            .flush()
            .unwrap_or_else(|e| eprintln!("shell: error flushing stdout: {}", e));
    }

    // --- Return status ---
    if status.success() {
        Ok(None) // Success, output handled
    } else {
        Err(String::new()) // Failure (non-zero exit), signal shell not to print more errors
    }
}

// --- Main Loop Logic Extraction ---

#[derive(PartialEq, Eq)]
pub enum RedirectionMode {
    Overwrite,
    Append,
}

pub struct RedirectFile {
    pub filename: String,
    pub mode: RedirectionMode,
}

#[derive(Default)]
pub struct Redirections {
    pub stdout_redirect: Option<RedirectFile>,
    pub stderr_redirect: Option<RedirectFile>,
}

/// Parses redirection operators (>, 1>, 2>) from the end of a token list.
/// Returns the remaining arguments and optional filenames for stdout/stderr redirection.
fn parse_redirections(args_slice: &[String]) -> (Vec<String>, Redirections) {
    let mut command_args = args_slice.to_vec(); // Clone to modify
    let mut red = Redirections::default();

    // Loop backwards checking for `op filename` patterns
    loop {
        let len = command_args.len();
        if len < 2 {
            break;
        } // Need op + file

        let op = &command_args[len - 2];
        let filename = &command_args[len - 1];

        match op.as_str() {
            ">" | "1>" => {
                red.stdout_redirect = Some(RedirectFile {
                    filename: filename.clone(),
                    mode: RedirectionMode::Overwrite,
                });
                command_args.truncate(len - 2); // Remove op + file
            }
            "2>" => {
                red.stderr_redirect = Some(RedirectFile {
                    filename: filename.clone(),
                    mode: RedirectionMode::Overwrite,
                });
                command_args.truncate(len - 2); // Remove op + file
            }
            ">>" | "1>>" => {
                red.stdout_redirect = Some(RedirectFile {
                    filename: filename.clone(),
                    mode: RedirectionMode::Append,
                });
                command_args.truncate(len - 2); // Remove op + file
            }
            "2>>" => {
                red.stderr_redirect = Some(RedirectFile {
                    filename: filename.clone(),
                    mode: RedirectionMode::Append,
                });
                command_args.truncate(len - 2); // Remove op + file
            }
            _ => break, // Not a redirection operator
        }
    }
    (command_args, red)
}

/// Dispatches the command to the appropriate handler (built-in or external).
fn dispatch_command(
    command_name: &str,
    command_args: &[String],
    redirections: &Redirections,
) -> Result<Option<String>, String> {
    match command_name {
        // --- Built-in Commands ---
        "exit" => {
            let code = command_args
                .first()
                .and_then(|s| s.parse::<i32>().ok())
                .unwrap_or(0);
            std::process::exit(code);
        }
        "echo" => handle_echo(command_args),
        "pwd" => handle_pwd(command_args),
        "cd" => handle_cd(command_args),
        "type" => handle_type(command_args),
        // --- External Command ---
        cmd => match find_exec_in_path(cmd) {
            Some(full_path) => {
                execute_external_command(cmd, &full_path, command_args, redirections)
            }
            None => Err(format!("{}: command not found", cmd)),
        },
    }
}

/// Handles the result from dispatch_command, printing output/errors appropriately
/// respecting redirection settings.
fn handle_command_result(result: Result<Option<String>, String>, redirections: &Redirections) {
    match result {
        Ok(Some(output_str)) => {
            // Success with output (built-in, or external without '>')
            if let Some(stdout) = &redirections.stdout_redirect {
                // Redirect BUILT-IN output
                match OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(stdout.mode == RedirectionMode::Overwrite)
                    .append(stdout.mode == RedirectionMode::Append)
                    .open(&stdout.filename)
                {
                    Ok(mut file) => {
                        if let Err(e) = file.write_all(output_str.as_bytes()) {
                            eprintln!(
                                "shell: error writing built-in stdout to '{}': {}",
                                &stdout.filename, e
                            );
                        }
                    }
                    Err(e) => eprintln!(
                        "shell: failed to open stdout redirect file '{}': {}",
                        &stdout.filename, e
                    ),
                }
                // Ensure stderr file exists if 2> also used
                if let Some(err_f) = &redirections.stderr_redirect {
                    if OpenOptions::new()
                        .create(true)
                        .write(true)
                        .truncate(err_f.mode == RedirectionMode::Overwrite)
                        .append(err_f.mode == RedirectionMode::Append)
                        .open(&err_f.filename)
                        .is_err()
                    { /* ignore */ }
                }
            } else {
                // Print output to terminal stdout
                print!("{}", output_str);
                io::stdout()
                    .flush()
                    .unwrap_or_else(|e| eprintln!("shell: error flushing stdout: {}", e));
                // Ensure stderr file exists if 2> also used
                if let Some(err_f) = &redirections.stderr_redirect {
                    if OpenOptions::new()
                        .create(true)
                        .write(true)
                        .truncate(err_f.mode == RedirectionMode::Overwrite)
                        .append(err_f.mode == RedirectionMode::Append)
                        .open(&err_f.filename)
                        .is_err()
                    { /* ignore */ }
                }
            }
        }
        Ok(None) => {
            // Success, no direct output string (cd, external with '>', external no output)
            // Ensure stderr file exists if 2> used
            if let Some(err_f) = &redirections.stderr_redirect {
                if OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(err_f.mode == RedirectionMode::Overwrite)
                    .append(err_f.mode == RedirectionMode::Append)
                    .open(&err_f.filename)
                    .is_err()
                { /* ignore */ }
            }
            // Stdout redirection for external commands was handled internally
        }
        Err(err_msg) => {
            // Command failed
            if !err_msg.is_empty() {
                // Built-in or shell error (e.g., "not found", "cd failed")
                // Handle stderr redirection for this error message
                if let Some(stderr) = &redirections.stderr_redirect {
                    match File::create(&stderr.filename) {
                        Ok(mut file) => {
                            if let Err(e) = writeln!(file, "{}", err_msg) {
                                eprintln!(
                                    "shell: error writing error to stderr redirect file '{}': {}",
                                    &stderr.filename, e
                                );
                            }
                        }
                        Err(e) => {
                            // Failed to open redirect file, print original error to terminal
                            eprintln!(
                                "shell: failed to open stderr redirect file '{}': {}",
                                &stderr.filename, e
                            );
                            eprintln!("{}", err_msg);
                        }
                    }
                } else {
                    // Print error to terminal stderr
                    eprintln!("{}", err_msg);
                }
                // Ensure stdout file exists if > was used with a failed built-in/shell command
                if let Some(out_f) = &redirections.stdout_redirect {
                    if OpenOptions::new()
                        .create(true)
                        .write(true)
                        .truncate(out_f.mode == RedirectionMode::Overwrite)
                        .append(out_f.mode == RedirectionMode::Append)
                        .open(&out_f.filename)
                        .is_err()
                    { /* ignore */ }
                }
            }
            // else: err_msg is empty, indicating external command failed (non-zero exit).
            // Stderr/stdout were already handled by execute_external_command. Do nothing here.
        }
    }
}

// --- Refactored Main Shell Loop ---
fn main() {
    loop {
        // 1. Print prompt
        print!("$ ");
        io::stdout().flush().unwrap();

        // 2. Read input
        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(0) => {
                println!();
                break;
            } // EOF
            Ok(_) => {}
            Err(e) => {
                eprintln!("shell: input error: {}", e);
                break;
            }
        }

        // 3. Basic trimming and empty check
        let trimmed_input = input.trim();
        if trimmed_input.is_empty() {
            continue;
        }

        // 4. Parse input into tokens
        let tokens: Vec<String> = match parse_tokens(trimmed_input) {
            Ok(parsed) if parsed.is_empty() => continue, // e.g., input was `""`
            Ok(parsed) => parsed,
            Err(e) => {
                eprintln!("shell: parse error: {}", e);
                continue;
            }
        };
        let (command_name, args_slice) = tokens.split_first().unwrap(); // Safe due to empty check

        // 5. Parse redirections from arguments
        let (command_args, redirections) = parse_redirections(args_slice);

        // 6. Dispatch command (built-in or external)
        let result = dispatch_command(
            command_name,
            &command_args, // Use args *after* redirection parsing
            &redirections,
        );

        // 7. Handle the result (print output/errors, respect redirection)
        handle_command_result(result, &redirections);
    }
}
