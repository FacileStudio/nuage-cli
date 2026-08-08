#![allow(dead_code)]

use std::env;
use std::io::{self, IsTerminal};
use std::sync::atomic::{AtomicBool, Ordering};

static FORCED_OFF: AtomicBool = AtomicBool::new(false);

pub fn disable_color() {
    FORCED_OFF.store(true, Ordering::Relaxed);
}

fn paint(is_tty: bool, code: &str, text: &str) -> String {
    if FORCED_OFF.load(Ordering::Relaxed) || !is_tty || env::var_os("NO_COLOR").is_some() {
        text.to_string()
    } else {
        format!("\x1b[{code}m{text}\x1b[0m")
    }
}

fn out_tty() -> bool {
    io::stdout().is_terminal()
}

fn err_tty() -> bool {
    io::stderr().is_terminal()
}

pub fn step(msg: &str) {
    println!("{} {msg}", paint(out_tty(), "36", "\u{25b8}"));
}

pub fn success(msg: &str) {
    println!("{} {msg}", paint(out_tty(), "32", "\u{2713}"));
}

pub fn warn(msg: &str) {
    eprintln!("{} {msg}", paint(err_tty(), "33", "!"));
}

pub fn error(msg: &str) {
    eprintln!("{} {msg}", paint(err_tty(), "31", "\u{2717}"));
}

pub fn hint(msg: &str) {
    println!("  {}", paint(out_tty(), "2", msg));
}

pub fn dim(text: &str) -> String {
    paint(out_tty(), "2", text)
}

pub fn green(text: &str) -> String {
    paint(out_tty(), "32", text)
}

pub fn bold(text: &str) -> String {
    paint(out_tty(), "1", text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paint_is_a_noop_without_a_tty() {
        assert_eq!(paint(false, "31", "x"), "x");
        assert_eq!(paint(true, "31", "x"), "\x1b[31mx\x1b[0m");
    }
}
