#![allow(unused_imports)]

mod matcher;
mod temp;

pub use matcher::IgnoreRules;
pub use temp::is_temp_artifact;

#[cfg(test)]
mod tests;
