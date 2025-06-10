//! External command execution module for the rust shell.
//! 
//! This module handles finding executables in the PATH and executing
//! external commands with proper I/O redirection and error handling.

use crate::redirect::{RedirectionMode, Redirections};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt; // For execute bits
#[cfg(unix)]
use std::os::unix::process::CommandExt; // For arg0
use std::process::{Command, Stdio};

/// Searches a single directory for an executable file name. Checks execute bits on Unix.
/// Skips directories that are NotFound or inaccessible, returns other IO errors.
/// 
/// # Arguments
/// 
/// * `dir_path` - The directory path to search in
/// * `name` - The executable name to search for
/// 
/// # Returns
/// 
/// * `Ok(Some(path))` - Found executable at the given path
/// * `Ok(None)` - Executable not found in this directory
/// * `Err(e)` - IO error occurred while searching
pub fn find_exec_in_dir(dir_path: &str, name: &str) -> io::Result<Option<String>> {
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
/// 
/// # Arguments
/// 
/// * `name` - The command name or path to search for
/// 
/// # Returns
/// 
/// * `Some(path)` - Found executable at the given path
/// * `None` - Executable not found
/// 
/// # Examples
/// 
/// ```
/// use codecrafters_shell::exec::find_exec_in_path;
/// 
/// // Direct path
/// let result = find_exec_in_path("/bin/ls");
/// 
/// // Search in PATH
/// let result = find_exec_in_path("ls");
/// ```
pub fn find_exec_in_path(name: &str) -> Option<String> {
    if name.contains('/') {
        // Direct path check
        if let Ok(metadata) = fs::metadata(name) {
            if metadata.is_file() {
                #[cfg(unix)]
                {
                    // Check execute permission
                    if (metadata.permissions().mode() & 0o111) != 0 {
                        return Some(name.to_string());
                    }
                }
                #[cfg(not(unix))]
                {
                    // Assume executable on non-Unix
                    return Some(name.to_string());
                }
            }
        }
        return None; // Direct path not found or not executable
    }

    // Search PATH environment variable
    if let Ok(path_env) = env::var("PATH") {
        for dir_path in path_env.split(':') {
            if let Ok(Some(full_path)) = find_exec_in_dir(dir_path, name) {
                return Some(full_path);
            }
            // Continue searching other directories on error
        }
    }
    None // Not found in PATH or PATH not set
}

/// Executes an external command, handling args, stdio redirection, and waiting.
/// Returns Ok(None) on success (exit 0), Err("") on failure (non-zero exit),
/// or Err(message) on spawn/wait errors.
/// 
/// # Arguments
/// 
/// * `command_name` - The command name for error messages and arg0
/// * `command_path` - The full path to the executable
/// * `args` - The command arguments
/// * `redirections` - The I/O redirection configuration
/// 
/// # Returns
/// 
/// * `Ok(None)` - Command succeeded (exit code 0)
/// * `Err("")` - Command failed with non-zero exit code
/// * `Err(message)` - Error spawning or waiting for command
pub fn execute_external_command(
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_find_exec_in_path_direct() {
        // Test with a known executable (if it exists)
        if Path::new("/bin/ls").exists() {
            let result = find_exec_in_path("/bin/ls");
            assert_eq!(result, Some("/bin/ls".to_string()));
        }
    }

    #[test]
    fn test_find_exec_in_path_nonexistent() {
        let result = find_exec_in_path("/nonexistent/command");
        assert_eq!(result, None);
    }

    #[test]
    fn test_find_exec_in_path_search() {
        // This test might be environment-dependent
        // Just ensure it doesn't panic
        let _result = find_exec_in_path("ls");
    }

    #[test]
    fn test_find_exec_in_dir_nonexistent() {
        let result = find_exec_in_dir("/nonexistent", "command");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);
    }
}