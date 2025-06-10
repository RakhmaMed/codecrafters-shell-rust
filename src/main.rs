#![allow(clippy::comparison_to_empty)] // Allow Err("") for external command failure status

mod parser;
mod redirect;
mod exec;
mod builtins;

use std::fs::{File, OpenOptions};
use std::io::{self, Write};

use parser::parse_tokens;
use redirect::{parse_redirections, Redirections, RedirectionMode};
use exec::{find_exec_in_path, execute_external_command};
use builtins::{handle_echo, handle_pwd, handle_type, handle_cd, handle_exit};

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

/// Main shell loop
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