//! I/O redirection handling module for the rust shell.
//! 
//! This module handles parsing and managing I/O redirections for commands,
//! including stdout and stderr redirections with overwrite and append modes.

/// Represents the mode of redirection operation.
#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum RedirectionMode {
    /// Overwrite the target file (> or 1> or 2>)
    Overwrite,
    /// Append to the target file (>> or 1>> or 2>>)
    Append,
}

/// Represents a single redirection to a file.
#[derive(Debug, Clone)]
pub struct RedirectFile {
    /// The filename to redirect to
    pub filename: String,
    /// The mode of redirection (overwrite or append)
    pub mode: RedirectionMode,
}

/// Holds all redirection information for a command.
#[derive(Default, Debug)]
pub struct Redirections {
    /// Optional stdout redirection
    pub stdout_redirect: Option<RedirectFile>,
    /// Optional stderr redirection
    pub stderr_redirect: Option<RedirectFile>,
}

/// Parses redirection operators (>, 1>, 2>, >>, 1>>, 2>>) from the end of a token list.
/// Returns the remaining arguments and optional filenames for stdout/stderr redirection.
/// 
/// # Arguments
/// 
/// * `args_slice` - The command arguments to parse redirections from
/// 
/// # Returns
/// 
/// A tuple containing:
/// * `Vec<String>` - The remaining command arguments after removing redirection operators
/// * `Redirections` - The parsed redirection information
/// 
/// # Examples
/// 
/// ```
/// use codecrafters_shell::redirect::parse_redirections;
/// 
/// let args = vec!["ls".to_string(), "-l".to_string(), ">".to_string(), "output.txt".to_string()];
/// let (remaining_args, redirections) = parse_redirections(&args);
/// assert_eq!(remaining_args, vec!["ls", "-l"]);
/// assert!(redirections.stdout_redirect.is_some());
/// ```
pub fn parse_redirections(args_slice: &[String]) -> (Vec<String>, Redirections) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_redirection() {
        let args = vec!["ls".to_string(), "-l".to_string()];
        let (remaining_args, redirections) = parse_redirections(&args);
        assert_eq!(remaining_args, args);
        assert!(redirections.stdout_redirect.is_none());
        assert!(redirections.stderr_redirect.is_none());
    }

    #[test]
    fn test_stdout_overwrite() {
        let args = vec!["echo".to_string(), "hello".to_string(), ">".to_string(), "output.txt".to_string()];
        let (remaining_args, redirections) = parse_redirections(&args);
        assert_eq!(remaining_args, vec!["echo", "hello"]);
        assert!(redirections.stdout_redirect.is_some());
        let stdout = redirections.stdout_redirect.unwrap();
        assert_eq!(stdout.filename, "output.txt");
        assert_eq!(stdout.mode, RedirectionMode::Overwrite);
    }

    #[test]
    fn test_stdout_append() {
        let args = vec!["echo".to_string(), "hello".to_string(), ">>".to_string(), "output.txt".to_string()];
        let (remaining_args, redirections) = parse_redirections(&args);
        assert_eq!(remaining_args, vec!["echo", "hello"]);
        assert!(redirections.stdout_redirect.is_some());
        let stdout = redirections.stdout_redirect.unwrap();
        assert_eq!(stdout.filename, "output.txt");
        assert_eq!(stdout.mode, RedirectionMode::Append);
    }

    #[test]
    fn test_stderr_redirection() {
        let args = vec!["ls".to_string(), "/nonexistent".to_string(), "2>".to_string(), "error.txt".to_string()];
        let (remaining_args, redirections) = parse_redirections(&args);
        assert_eq!(remaining_args, vec!["ls", "/nonexistent"]);
        assert!(redirections.stderr_redirect.is_some());
        let stderr = redirections.stderr_redirect.unwrap();
        assert_eq!(stderr.filename, "error.txt");
        assert_eq!(stderr.mode, RedirectionMode::Overwrite);
    }

    #[test]
    fn test_both_redirections() {
        let args = vec![
            "command".to_string(),
            ">".to_string(),
            "output.txt".to_string(),
            "2>".to_string(),
            "error.txt".to_string()
        ];
        let (remaining_args, redirections) = parse_redirections(&args);
        assert_eq!(remaining_args, vec!["command"]);
        assert!(redirections.stdout_redirect.is_some());
        assert!(redirections.stderr_redirect.is_some());
    }

    #[test]
    fn test_explicit_fd_redirections() {
        let args = vec!["echo".to_string(), "test".to_string(), "1>".to_string(), "out.txt".to_string()];
        let (remaining_args, redirections) = parse_redirections(&args);
        assert_eq!(remaining_args, vec!["echo", "test"]);
        assert!(redirections.stdout_redirect.is_some());
        let stdout = redirections.stdout_redirect.unwrap();
        assert_eq!(stdout.filename, "out.txt");
        assert_eq!(stdout.mode, RedirectionMode::Overwrite);
    }
}