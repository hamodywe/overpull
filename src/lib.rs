//! overpull — measures what your imports really load.
//!
//! The library half exists so the CLI can be tested as a process while the
//! analyses stay unit-testable. See `src/main.rs` for the entry point.

pub mod barrels;
pub mod baseline;
pub mod cli;
pub mod cost;
pub mod cycles;
pub mod entries;
pub mod graph;
pub mod model;
pub mod parse;
pub mod report;
pub mod resolve;
pub mod run;
pub mod style;
pub mod util;
pub mod walk;
pub mod why;
