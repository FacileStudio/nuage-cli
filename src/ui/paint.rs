use std::env;
use std::io::{self, IsTerminal};
use std::sync::atomic::{AtomicBool, Ordering};

static FORCED_OFF: AtomicBool = AtomicBool::new(false);

pub fn disable_color() {
    FORCED_OFF.store(true, Ordering::Relaxed);
}

pub(super) fn paint(is_tty: bool, code: &str, text: &str) -> String {
    if FORCED_OFF.load(Ordering::Relaxed) || !is_tty || env::var_os("NO_COLOR").is_some() {
        text.to_string()
    } else {
        format!("\x1b[{code}m{text}\x1b[0m")
    }
}

pub(super) fn out_tty() -> bool {
    io::stdout().is_terminal()
}

pub(super) fn err_tty() -> bool {
    io::stderr().is_terminal()
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
