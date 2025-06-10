//! Built-in shell commands module for the rust shell.
//!
//! This module implements all the built-in commands that are handled directly
//! by the shell rather than being executed as external programs.

use crate::exec::find_exec_in_path;
use std::env;
use std::io::ErrorKind;

/// Handles the `echo` command by joining all arguments with spaces.
///
/// # Arguments
///
/// * `args` - The arguments to echo
///
/// # Returns
///
/// * `Ok(Some(output))` - The echoed string with a trailing newline
///
/// # Examples
///
/// ```
/// use codecrafters_shell::builtins::handle_echo;
///
/// let result = handle_echo(&["hello".to_string(), "world".to_string()]);
/// assert_eq!(result.unwrap().unwrap(), "hello world\n");
/// ```
pub fn handle_echo(args: &[String]) -> Result<Option<String>, String> {
    Ok(Some(format!("{}\r\n", args.join(" "))))
}

/// Handles the `pwd` command by returning the current working directory.
///
/// # Arguments
///
/// * `_args` - Unused arguments (pwd takes no arguments)
///
/// # Returns
///
/// * `Ok(Some(path))` - Current directory path with trailing newline
/// * `Err(message)` - Error getting current directory
pub fn handle_pwd(_args: &[String]) -> Result<Option<String>, String> {
    match env::current_dir() {
        Ok(dir) => Ok(Some(format!("{}\r\n", dir.display()))),
        Err(e) => Err(format!("pwd: error getting current directory: {}", e)),
    }
}

/// Helper function that generates the string for the 'type' command.
/// Checks if a command is a built-in or searches for it in PATH.
///
/// # Arguments
///
/// * `name` - The command name to look up
///
/// # Returns
///
/// A formatted string describing where the command is found
fn type_info_string(name: &str) -> String {
    if ["echo", "exit", "type", "pwd", "cd"].contains(&name) {
        format!("{} is a shell builtin", name)
    } else if let Some(full_path) = find_exec_in_path(name) {
        format!("{} is {}", name, full_path)
    } else {
        format!("{}: not found", name)
    }
}

/// Handles the `type` command by showing information about a command.
///
/// # Arguments
///
/// * `args` - Should contain exactly one argument (the command to look up)
///
/// # Returns
///
/// * `Ok(Some(info))` - Information about the command with trailing newline
/// * `Err(message)` - Error for wrong number of arguments
pub fn handle_type(args: &[String]) -> Result<Option<String>, String> {
    match args {
        [name] => Ok(Some(format!("{}\r\n", type_info_string(name)))),
        [] => Err("type: missing argument".to_string()),
        _ => Err("type: too many arguments".to_string()),
    }
}

/// Helper function that performs the directory change for `cd`. Handles `~` expansion.
///
/// # Arguments
///
/// * `target_path_str` - The target directory path (may contain ~ for home)
///
/// # Returns
///
/// * `Ok(())` - Successfully changed directory
/// * `Err(message)` - Error changing directory
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
        // Format error message based on Kind to match test expectation
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

/// Handles the `cd` command by changing the current directory.
///
/// # Arguments
///
/// * `args` - Should contain zero or one argument (the target directory)
///   - No args: change to home directory (~)
///   - One arg: change to specified directory
///
/// # Returns
///
/// * `Ok(None)` - Successfully changed directory (no output)
/// * `Err(message)` - Error changing directory or too many arguments
pub fn handle_cd(args: &[String]) -> Result<Option<String>, String> {
    let target_path_str = match args {
        [] => "~", // Default to home
        [path] => path.as_str(),
        _ => return Err("cd: too many arguments".to_string()),
    };
    change_dir(target_path_str).map(|_| None)
}

/// Handles the `exit` command by terminating the shell process.
///
/// # Arguments
///
/// * `args` - Optional exit code (defaults to 0 if not provided)
///
/// # Note
///
/// This function does not return as it calls `std::process::exit()`.
pub fn handle_exit(args: &[String]) -> ! {
    let code = args
        .first()
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0);
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_echo_empty() {
        let result = handle_echo(&[]);
        assert_eq!(result.unwrap().unwrap(), "\r\n");
    }

    #[test]
    fn test_echo_single_arg() {
        let result = handle_echo(&["hello".to_string()]);
        assert_eq!(result.unwrap().unwrap(), "hello\r\n");
    }

    #[test]
    fn test_echo_multiple_args() {
        let result = handle_echo(&["hello".to_string(), "world".to_string()]);
        assert_eq!(result.unwrap().unwrap(), "hello world\r\n");
    }

    #[test]
    fn test_pwd() {
        let result = handle_pwd(&[]);
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[test]
    fn test_type_builtin() {
        let result = handle_type(&["echo".to_string()]);
        assert_eq!(result.unwrap().unwrap(), "echo is a shell builtin\r\n");
    }

    #[test]
    fn test_type_no_args() {
        let result = handle_type(&[]);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "type: missing argument");
    }

    #[test]
    fn test_type_too_many_args() {
        let result = handle_type(&["echo".to_string(), "pwd".to_string()]);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "type: too many arguments");
    }

    #[test]
    fn test_cd_too_many_args() {
        let result = handle_cd(&["dir1".to_string(), "dir2".to_string()]);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "cd: too many arguments");
    }

    #[test]
    fn test_type_info_string() {
        assert_eq!(type_info_string("echo"), "echo is a shell builtin");
        assert_eq!(
            type_info_string("nonexistent_command_xyz"),
            "nonexistent_command_xyz: not found"
        );
    }
}
