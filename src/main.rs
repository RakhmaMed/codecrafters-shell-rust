use std::env;
use std::fs;
use std::fs::File;
use std::io::{self, Write};
use std::os::unix::process::CommandExt;
use std::process::Command;

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
                if !current_arg.is_empty() {
                    // If we hit whitespace outside quotes, it's a delimiter
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

        if let Ok(entries) = fs::read_dir(p) {
            // Handle potential read_dir error
            for entry in entries {
                if let Ok(entry) = entry {
                    // Handle potential entry error
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
                                if metadata.is_file()
                                    && (metadata.permissions().mode() & 0o111) != 0
                                {
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

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "Executable not found in path",
    ))
}

// Find an executable in PATHs
fn find_exec_in_path(name: &str) -> Option<String> {
    if name.contains('/') {
        // If it's already a path, check directly (basic check)
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

/// Executes a command, captures its output, and return stdout on success.
/// 
/// On Unix it sets argv[0] using `command_name`. On other platforms,
/// argv[0] will typically be `command_path`.
/// 
/// # Arguments
/// * `command_name` - The name to use for argv[0] (primarily on Unix).
/// * `command_path` - The actual path to the executable file.
/// * `args` - A slice of strings representing the arguments (argv[1], ...).
/// 
/// # Returns
/// * `Ok(String)` containing the captured standard output if he command runs successfully (exit code 0).
/// * `Err(String)` containing an error message if the command fails to spawn,
///   doesn't execute sucessfully (non-zero exit code), or if stdouut is not valid UTF-8.
fn try_call(command_name: &str, command_path: &str, args: &[String]) -> Result<String, String> {
    let mut command_proc = Command::new(command_path);

    // Conditionally set argv[0] on Unix systems
    #[cfg(unix)]
    {
        command_proc.arg0(command_name);
    }

    // Add the res of the arguments
    command_proc.args(args);

    // Execute the command and capture its output (stdout, stderr, status)
    match command_proc.output() {
        Ok(output) => {
            if output.status.success() {
                // Command executed successfully (exit code 0)
                // Comvert stdout bytes to a String. Use lossy conversion for robustness.
                let stdout_str: String = String::from_utf8_lossy(&output.stdout).to_string();
                Ok(stdout_str)
            } else {
                // Command execuuted but return a non-zero exit code
                let stderr_str = String::from_utf8_lossy(&output.stderr).to_string();
                Err(format!("{}", stderr_str.trim_end())) // Trim trailing whitespace/newline often found in stderr
            }
        }
        Err(err) => {
            // Failed to spawn or run the command itself (e.g., file not founc)
            // Determine which name to report in the error message
            #[cfg(unix)]
            let name_to_report = command_name;
            #[cfg(not(unix))]
            let name_to_report = command_path;

            if err.kind() == io::ErrorKind::NotFound {
                // Specifically handle "command not found" using the path
                Err(format!("Failed to execute command: '{}' not found. Error: {}", command_path, err))
            } else {
                Err(format!("Failed to spawn process '{}': {}", name_to_report, err))
            }
        }
    }
}


fn change_dir(path: &str) -> Result<(), String> {
    let target_path = if path == "~" {
        match env::var("HOME") {
            Ok(home) => home,
            Err(_) => {
                return Err(format!("cd: HOME environment variable not set"));
            }
        }
    } else {
        path.to_string()
    };

    if env::set_current_dir(&target_path).is_err() {
        return Err(format!("cd: {}: No such file or directory", target_path));
    }

    Ok(())
}

fn print_to_file(text: &str, file_path: &str) -> std::io::Result<()> {
    // Open the file with options:
    // - create(true): Create the file if it doesn't exist.
    // - write(true): Allow writing to the file.
    // - truncate(true): If the file exists, clear its contents before writing.
    let mut file = File::options()
        .create(true)
        .write(true)
        .truncate(true) // This is the key change for overwriting
        .open(file_path)?;

    // Write the entire text to the (now potentially empty) file.
    file.write_all(text.as_bytes())?; // Added ? for error propagation

    Ok(()) // Indicate success
}

fn handle_echo(args: &[String]) -> String {
    match args {
        // Case 1: No arguments -> print newline
        [] => "\n".to_string(),
        // Case 2: All other argument combinations -> join and print to stdout
        _ => format!("{}\n", args.join(" "))
    }
}

fn handle_pwd(args: &[String]) -> Result<String, String> {
    match args {
        [] => match env::current_dir() {
            Ok(dir) => Ok(format!("{}\n", dir.display())),
            Err(e) => Err(format!("pwd: error getting current directory: {}\n", e)),
        },
        _ => Err(format!("pwd: too many arguments\n"))
    }
}

fn handle_cd(args: &[String]) -> Result<String, String> {
    let res = match args {
        [] => change_dir("~"),
        [path] => change_dir(path),
        _ => Err(format!("cd: too many arguments"))
    };

    match res {
        Ok(_) => Ok("\n".to_string()),
        Err(e) => Ok(format!("cd: {}\n", e)),
    }
}

fn handle_type(args: &[String]) -> Result<String, String> {
    match args {
        [name] => Ok(format!("{}\n", type_buildin(name))),
        _ => Err(format!("type: requires exactly one argument\n"))
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

        let tokens: Vec<String> = match parse_tokens(input) {
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

        let (command_name, args) = tokens.split_first().unwrap(); // Safe due to empty check above

        let (args, redirection_part) = match args.iter().position(|arg| arg == ">" || arg == "1>") {
            // Operator found: Check if it's at the second-to-last position
            Some(index) if args.len() >= 2 && index == args.len() - 2 => {
                // Condition met: Split args at the operator index
                // The command part is everything before the operator.
                // The redirection part is the operator and the filename after it.
                (&args[..index], &args[index..])
            }
            // Catch-all for other cases:
            // - Operator not found (None)
            // - Operator found, but not at the required position (Some(index) where guard is false)
            _ => {
                // Treat the entire args slice as the command part, no redirection
                (args, &[] as &[String])
            }
        };
        
        let result = match command_name.as_str() {
            "exit" => {
                // Default exit code 0 if not specified or invalid
                let code = args.get(0).and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
                std::process::exit(code);
            }
            "echo" => Ok(handle_echo(args)), // Pass the parsed String args
            "pwd" => handle_pwd(args),
            "cd" => handle_cd(args),
            "type" => handle_type(args),
            // Handle external commands
            cmd => {
                // Find the executable path using the updated function
                match find_exec_in_path(cmd) {
                    Some(full_path) => {
                        // Call try_call with the full path and parsed args
                        try_call(cmd, &full_path, args)
                    }
                    None => {
                        Err(format!("{}: command not found", cmd))
                    }
                }
            }
        };

        match result {
            Ok(output) => {
                match redirection_part {
                    [sign, filename] if sign == ">" || sign == "1>" => {
                        match print_to_file(&output, filename) {
                            Err(e) => eprintln!("{}\n", e),
                            _ => ()
                        }
                    }
                    _ => { 
                        print!("{}", output);
                        io::stdout().flush().unwrap(); // Ensure output is written immediately
                    }
                }
            },
            Err(err) => {
                eprintln!("{}", err);
            }
        }
    }
}
