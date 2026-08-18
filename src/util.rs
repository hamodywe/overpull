//! Small shared helpers: line lookup, path display, byte formatting.

use std::path::{Path, PathBuf};

/// Canonicalizes a path and removes the Windows extended-length prefix.
///
/// `std::fs::canonicalize` returns `\\?\C:\…` on Windows while the resolver
/// returns `C:\…`. Both name the same file, but they are different map keys —
/// left unnormalized, the same module is inserted twice and the graph splits
/// in half. Every path entering the tool goes through here.
pub fn normalize(path: &Path) -> PathBuf {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    strip_verbatim(&canonical)
}

fn strip_verbatim(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\UNC\") {
        Some(rest) => PathBuf::from(format!(r"\\{rest}")),
        None => match text.strip_prefix(r"\\?\") {
            Some(rest) => PathBuf::from(rest),
            None => path.to_path_buf(),
        },
    }
}

/// Maps byte offsets to 1-based line numbers for one source file.
pub struct LineIndex {
    /// Byte offset at which each line starts. First entry is always 0.
    line_starts: Vec<u32>,
}

impl LineIndex {
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0u32];
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' {
                // Offsets in oxc spans are u32; sources larger than 4 GiB are
                // rejected by the parser long before this becomes an issue.
                #[allow(clippy::cast_possible_truncation)]
                line_starts.push(i as u32 + 1);
            }
        }
        Self { line_starts }
    }

    /// 1-based line number containing `offset`.
    pub fn line(&self, offset: u32) -> u32 {
        match self.line_starts.binary_search(&offset) {
            Ok(i) => u32::try_from(i).unwrap_or(u32::MAX) + 1,
            Err(i) => u32::try_from(i).unwrap_or(u32::MAX),
        }
    }
}

/// Project-relative path with forward slashes, for stable display and JSON
/// output across platforms.
pub fn display_path(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.to_string_lossy().replace('\\', "/")
}

/// Human-readable byte count: `912 B`, `41.3 KB`, `2.1 MB`.
pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        #[allow(clippy::cast_precision_loss)]
        let kb = bytes as f64 / 1024.0;
        format!("{kb:.1} KB")
    } else {
        #[allow(clippy::cast_precision_loss)]
        let mb = bytes as f64 / (1024.0 * 1024.0);
        format!("{mb:.1} MB")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_index_maps_offsets() {
        let idx = LineIndex::new("ab\ncd\ne");
        assert_eq!(idx.line(0), 1);
        assert_eq!(idx.line(2), 1);
        assert_eq!(idx.line(3), 2);
        assert_eq!(idx.line(5), 2);
        assert_eq!(idx.line(6), 3);
    }

    #[test]
    fn line_index_empty_source() {
        let idx = LineIndex::new("");
        assert_eq!(idx.line(0), 1);
    }

    #[test]
    fn strips_windows_verbatim_prefix() {
        assert_eq!(
            strip_verbatim(Path::new(r"\\?\C:\a\b")),
            PathBuf::from(r"C:\a\b")
        );
        assert_eq!(
            strip_verbatim(Path::new(r"\\?\UNC\server\share\f")),
            PathBuf::from(r"\\server\share\f")
        );
        assert_eq!(
            strip_verbatim(Path::new("/usr/lib")),
            PathBuf::from("/usr/lib")
        );
    }

    #[test]
    fn bytes_format() {
        assert_eq!(format_bytes(912), "912 B");
        assert_eq!(format_bytes(42_300), "41.3 KB");
        assert_eq!(format_bytes(2_202_009), "2.1 MB");
    }
}
