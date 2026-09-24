#![allow(dead_code, unused_imports)]

mod color;
mod paint;
mod print;

pub use color::{bold, dim, green};
pub use paint::disable_color;
pub use print::{error, hint, step, success, warn};
