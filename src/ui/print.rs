use super::paint::{err_tty, out_tty, paint};

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
