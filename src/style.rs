//! ANSI styling with `NO_COLOR` / non-TTY awareness.
//!
//! Color is decided once at startup: enabled only when stdout is a terminal,
//! `NO_COLOR` is unset, and `--no-color` was not passed. Everything funnels
//! through [`Style::paint`] so tests can assert on plain output.

use std::io::IsTerminal;

#[derive(Clone, Copy)]
pub struct Style {
    enabled: bool,
}

pub const RED: &str = "31";
pub const YELLOW: &str = "33";
pub const GREEN: &str = "32";
pub const CYAN: &str = "36";
pub const BOLD: &str = "1";
pub const DIM: &str = "2";

impl Style {
    pub fn detect(no_color_flag: bool) -> Self {
        let enabled = !no_color_flag
            && std::env::var_os("NO_COLOR").is_none()
            && std::io::stdout().is_terminal();
        Self { enabled }
    }

    pub fn plain() -> Self {
        Self { enabled: false }
    }

    pub fn paint(&self, code: &str, text: &str) -> String {
        if self.enabled {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }
}

/// Strips ASCII control characters from text read out of a scanned project
/// before it reaches the terminal, so a hostile file name or specifier cannot
/// inject escape sequences into the report.
pub fn sanitize(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\t')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_style_passes_through() {
        let s = Style::plain();
        assert_eq!(s.paint(RED, "x"), "x");
    }

    #[test]
    fn sanitize_strips_escapes() {
        assert_eq!(sanitize("a\x1b[31mb\x07c"), "a[31mbc");
        assert_eq!(sanitize("keep\ttabs"), "keep\ttabs");
    }
}
