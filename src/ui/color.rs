use super::paint::{out_tty, paint};

pub fn dim(text: &str) -> String {
    paint(out_tty(), "2", text)
}

pub fn green(text: &str) -> String {
    paint(out_tty(), "32", text)
}

pub fn bold(text: &str) -> String {
    paint(out_tty(), "1", text)
}
