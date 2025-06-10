//! Command line parsing module for the rust shell.
//! 
//! This module handles parsing command line input into tokens, respecting
//! shell quoting rules and escape sequences.



// --- Constants ---
pub const BACKSLASH: char = '\\';
pub const SINGLE_QUOTE: char = '\'';
pub const DOUBLE_QUOTE: char = '"';

/// Parses a command line string into arguments, respecting shell quoting and escaping.
/// Handles single quotes (''), double quotes (""), and backslash (\) escapes.
/// Returns Err on unterminated quotes.
pub fn parse_tokens(input_args: &str) -> Result<Vec<String>, String> {
    let mut args: Vec<String> = Vec::new();
    let mut current_arg = String::new();
    let mut in_double_quotes = false;
    let mut in_single_quotes = false;
    let mut chars = input_args.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            // Handle backslash escapes
            BACKSLASH => {
                if let Some(&next_char) = chars.peek() {
                    if in_single_quotes {
                        // Inside single quotes, backslashes are literal
                        current_arg.push(c);
                    } else {
                        // Outside single quotes or inside double quotes, escape the next character
                        chars.next(); // Consume the escaped character
                        current_arg.push(next_char);
                    }
                } else {
                    // Backslash at end of input - treat as literal
                    current_arg.push(c);
                }
            }
            // Handle single quotes
            SINGLE_QUOTE => {
                if in_double_quotes {
                    // Inside double quotes, single quotes are literal
                    current_arg.push(c);
                } else {
                    // Toggle single quote state
                    in_single_quotes = !in_single_quotes;
                }
            }
            // Handle double quotes
            DOUBLE_QUOTE => {
                if in_single_quotes {
                    // Inside single quotes, double quotes are literal
                    current_arg.push(c);
                } else {
                    // Toggle double quote state
                    in_double_quotes = !in_double_quotes;
                }
            }
            // Handle whitespace
            ' ' | '\t' => {
                if in_single_quotes || in_double_quotes {
                    // Inside quotes, whitespace is literal
                    current_arg.push(c);
                } else {
                    // Outside quotes, whitespace separates arguments
                    if !current_arg.is_empty() {
                        args.push(current_arg);
                        current_arg = String::new();
                    }
                    // Skip additional whitespace
                    while let Some(&next_char) = chars.peek() {
                        if next_char == ' ' || next_char == '\t' {
                            chars.next();
                        } else {
                            break;
                        }
                    }
                }
            }
            // Regular characters
            _ => {
                current_arg.push(c);
            }
        }
    }

    // Add the final argument if it's not empty
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_parsing() {
        assert_eq!(
            parse_tokens("echo hello world").unwrap(),
            vec!["echo", "hello", "world"]
        );
    }

    #[test]
    fn test_double_quotes() {
        assert_eq!(
            parse_tokens(r#"echo "hello world""#).unwrap(),
            vec!["echo", "hello world"]
        );
    }

    #[test]
    fn test_single_quotes() {
        assert_eq!(
            parse_tokens("echo 'hello world'").unwrap(),
            vec!["echo", "hello world"]
        );
    }

    #[test]
    fn test_backslash_escape() {
        assert_eq!(
            parse_tokens(r"echo hello\ world").unwrap(),
            vec!["echo", "hello world"]
        );
    }

    #[test]
    fn test_unterminated_double_quote() {
        assert!(parse_tokens(r#"echo "hello"#).is_err());
    }

    #[test]
    fn test_unterminated_single_quote() {
        assert!(parse_tokens("echo 'hello").is_err());
    }

    #[test]
    fn test_empty_input() {
        assert_eq!(parse_tokens("").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn test_whitespace_only() {
        assert_eq!(parse_tokens("   ").unwrap(), Vec::<String>::new());
    }
}