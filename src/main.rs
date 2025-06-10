#![allow(clippy::comparison_to_empty)] // Allow Err("") for external command failure status

mod builtins;
mod exec;
mod parser;
mod redirect;
mod utils;

use std::fs::{File, OpenOptions};
use std::io::{stdin, stdout, Write};
use termion::event::Key;
use termion::input::TermRead;
use termion::raw::IntoRawMode;

use builtins::{handle_cd, handle_echo, handle_exit, handle_pwd, handle_type};
use exec::{execute_external_command, find_exec_in_path};
use parser::parse_tokens;
use redirect::{parse_redirections, RedirectionMode, Redirections};

// Convention: Result<Option<String>, String>
// Ok(Some(output)): Success, print output (unless redirected)
// Ok(None):          Success, no output to print (cd, redirected external)
// Err(message):      Failure (built-in/shell), print message to stderr (unless redirected)
// Err(""):           Failure (external non-zero exit), shell prints nothing further.

/// Dispatches the command to the appropriate handler (built-in or external).
fn dispatch_command(
    command_name: &str,
    command_args: &[String],
    redirections: &Redirections,
) -> Result<Option<String>, String> {
    match command_name {
        // --- Built-in Commands ---
        "exit" => handle_exit(command_args),
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

/// Creates a file with the appropriate mode (overwrite/append) for redirection.
fn create_redirect_file(filename: &str, mode: RedirectionMode) -> Result<File, std::io::Error> {
    // Check if the target is an existing directory
    if let Ok(metadata) = std::fs::metadata(filename) {
        if metadata.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("'{}' is a directory", filename),
            ));
        }
    }

    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(mode == RedirectionMode::Overwrite)
        .append(mode == RedirectionMode::Append)
        .open(filename)
}

/// Ensures a redirect file exists, ignoring errors (used for "touch" behavior).
fn ensure_redirect_file_exists(filename: &str, mode: RedirectionMode) {
    let _ = create_redirect_file(filename, mode);
}

/// Writes output to stdout, either to a redirect file or terminal.
fn write_stdout(output: &str, redirections: &Redirections) {
    if let Some(stdout_redirect) = &redirections.stdout_redirect {
        match create_redirect_file(&stdout_redirect.filename, stdout_redirect.mode) {
            Ok(mut file) => {
                if let Err(e) = file.write_all(output.as_bytes()) {
                    raw_eprintln!(
                        "shell: error writing built-in stdout to '{}': {}",
                        &stdout_redirect.filename,
                        e
                    );
                }
            }
            Err(e) => raw_eprintln!(
                "shell: failed to open stdout redirect file '{}': {}",
                &stdout_redirect.filename,
                e
            ),
        }
    } else {
        raw_print!("{}", output);
    }
}

/// Writes error message to stderr, either to a redirect file or terminal.
fn write_stderr(error_msg: &str, redirections: &Redirections) {
    if let Some(stderr_redirect) = &redirections.stderr_redirect {
        match File::create(&stderr_redirect.filename) {
            Ok(mut file) => {
                if let Err(e) = writeln!(file, "{}", error_msg) {
                    raw_eprintln!(
                        "shell: error writing error to stderr redirect file '{}': {}",
                        &stderr_redirect.filename,
                        e
                    );
                }
            }
            Err(e) => {
                raw_eprintln!(
                    "shell: failed to open stderr redirect file '{}': {}",
                    &stderr_redirect.filename,
                    e
                );
                raw_eprintln!("{}", error_msg);
            }
        }
    } else {
        raw_eprintln!("{}", error_msg);
    }
}

/// Ensures both stdout and stderr redirect files exist if specified.
fn ensure_redirect_files_exist(redirections: &Redirections) {
    if let Some(stdout_redirect) = &redirections.stdout_redirect {
        ensure_redirect_file_exists(&stdout_redirect.filename, stdout_redirect.mode);
    }
    if let Some(stderr_redirect) = &redirections.stderr_redirect {
        ensure_redirect_file_exists(&stderr_redirect.filename, stderr_redirect.mode);
    }
}

/// Handles the result from dispatch_command, printing output/errors appropriately
/// respecting redirection settings.
fn handle_command_result(result: Result<Option<String>, String>, redirections: &Redirections) {
    match result {
        Ok(Some(output_str)) => {
            // Success with output (built-in, or external without '>')
            write_stdout(&output_str, redirections);
            // Ensure stderr file exists if 2> also used
            if let Some(stderr_redirect) = &redirections.stderr_redirect {
                ensure_redirect_file_exists(&stderr_redirect.filename, stderr_redirect.mode);
            }
        }
        Ok(None) => {
            // Success, no direct output string (cd, external with '>', external no output)
            ensure_redirect_files_exist(redirections);
        }
        Err(err_msg) => {
            // Command failed
            if !err_msg.is_empty() {
                // Built-in or shell error (e.g., "not found", "cd failed")
                write_stderr(&err_msg, redirections);
                // Ensure stdout file exists if > was used with a failed built-in/shell command
                if let Some(stdout_redirect) = &redirections.stdout_redirect {
                    ensure_redirect_file_exists(&stdout_redirect.filename, stdout_redirect.mode);
                }
            }
            // else: err_msg is empty, indicating external command failed (non-zero exit).
            // Stderr/stdout were already handled by execute_external_command. Do nothing here.
        }
    }
}

/// Main shell loop
fn main() {
    let builtins = vec!["exit", "echo", "help", "cd"];
    loop {
        // 1. Print prompt
        let stdin = stdin();
        let mut stdout = stdout().into_raw_mode().unwrap();
        write!(stdout, "$ ").unwrap();
        stdout.flush().unwrap();

        // 2. Read input char by char
        let mut input = String::new();
        for key in stdin.keys() {
            if let Ok(key) = key {
                match key {
                    Key::Char('\t') => {
                        let matches = builtins.iter().find(|&builtin| builtin.starts_with(&input));
                        if let Some(matched) = matches {
                            write!(stdout, "{} ", &matched[input.len()..]).unwrap();
                            input = matched.to_string() + " ";
                        }
                        stdout.flush().unwrap();
                    }
                    Key::Char('\n') => {
                        write!(stdout, "\r\n").unwrap();
                        stdout.flush().unwrap();
                        break;
                    }
                    Key::Char(c) => {
                        input.push(c);
                        write!(stdout, "{}", c).unwrap();
                        stdout.flush().unwrap();
                    }
                    _ => {}
                }
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
                raw_eprintln!("shell: parse error: {}", e);
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
