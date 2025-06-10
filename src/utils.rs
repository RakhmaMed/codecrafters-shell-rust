//! Utility macros and functions for the rust shell.
//!
//! This module provides shared utilities like raw mode printing macros
//! that can be used across different modules in the shell.

// raw_print macro for stdout in raw mode
#[macro_export]
macro_rules! raw_print {
    ($($arg:tt)*) => {{
         use std::io::Write;
         use termion::raw::IntoRawMode;
         let mut stdout = std::io::stdout().into_raw_mode().unwrap();
         write!(stdout, $($arg)*).unwrap();
         stdout.flush().unwrap();
    }};
}

// raw_println macro appends "\r\n"
#[macro_export]
macro_rules! raw_println {
    ($($arg:tt)*) => {{
         $crate::raw_print!("{}{}\r\n", format!($($arg)*), "")
    }};
}

// raw_eprint macro for stderr in raw mode
#[macro_export]
macro_rules! raw_eprint {
    ($($arg:tt)*) => {{
         use std::io::Write;
         use termion::raw::IntoRawMode;
         let mut stderr = std::io::stderr().into_raw_mode().unwrap();
         write!(stderr, $($arg)*).unwrap();
         stderr.flush().unwrap();
    }};
}

// raw_eprintln macro appends "\r\n"
#[macro_export]
macro_rules! raw_eprintln {
    ($($arg:tt)*) => {{
         $crate::raw_eprint!("{}{}\r\n", format!($($arg)*), "")
    }};
}