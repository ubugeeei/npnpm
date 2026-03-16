use std::{io, path::Path};

/// Create a symlink to a file.
///
/// The `link` path will be a symbolic link pointing to `original`.
pub fn symlink_file(original: &Path, link: &Path) -> io::Result<()> {
    #[cfg(unix)]
    return std::os::unix::fs::symlink(original, link);

    #[cfg(windows)]
    return std::os::windows::fs::symlink_file(original, link)
        .or_else(|_| std::fs::hard_link(original, link));
}
