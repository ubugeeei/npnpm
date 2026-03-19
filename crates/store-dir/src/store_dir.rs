use derive_more::From;
use serde::{Deserialize, Serialize};
use sha2::{digest, Digest, Sha512};
use std::fmt::Write;
use std::path::{self, PathBuf};

/// Content hash of a file.
pub type FileHash = digest::Output<Sha512>;

/// Represent a store directory.
///
/// * The store directory stores all files that were acquired by installing packages with pacquet or pnpm.
/// * The files in `node_modules` directories are hardlinks or reflinks to the files in the store directory.
/// * The store directory can and often act as a global shared cache of all installation of different workspaces.
/// * The location of the store directory can be customized by `store-dir` field.
#[derive(Debug, PartialEq, Eq, From, Deserialize, Serialize)]
#[serde(transparent)]
pub struct StoreDir {
    /// Path to the root of the store directory from which all sub-paths are derived.
    ///
    /// Consumer of this struct should interact with the sub-paths instead of this path.
    root: PathBuf,
}

impl StoreDir {
    /// Construct an instance of [`StoreDir`].
    pub fn new(root: impl Into<PathBuf>) -> Self {
        root.into().into()
    }

    /// Create an object that [displays](std::fmt::Display) the root of the store directory.
    pub fn display(&self) -> path::Display {
        self.root.display()
    }

    /// Get `{store}/v3`.
    fn v3(&self) -> PathBuf {
        self.root.join("v3")
    }

    /// Directory used for metadata caches.
    pub(crate) fn metadata(&self) -> PathBuf {
        self.v3().join("metadata")
    }

    /// The directory that contains all files from the once-installed packages.
    pub(crate) fn files(&self) -> PathBuf {
        self.v3().join("files")
    }

    /// Path to a file in the store directory.
    ///
    /// **Parameters:**
    /// * `head` is the first 2 hexadecimal digit of the file address.
    /// * `tail` is the rest of the address and an optional suffix.
    fn file_path_by_head_tail(&self, head: &str, tail: &str) -> PathBuf {
        self.files().join(head).join(tail)
    }

    /// Path to a file in the store directory.
    pub(crate) fn file_path_by_hex_str(&self, hex: &str, suffix: &'static str) -> PathBuf {
        let head = &hex[..2];
        let middle = &hex[2..];
        let tail = format!("{middle}{suffix}");
        self.file_path_by_head_tail(head, &tail)
    }

    /// Path to the temporary directory inside the store.
    pub fn tmp(&self) -> PathBuf {
        self.v3().join("tmp")
    }

    /// Path to a registry metadata cache file.
    pub fn registry_metadata_file_path(&self, registry: &str, name: &str) -> PathBuf {
        let key = format!("{registry}\n{name}");
        let hash = Sha512::digest(key.as_bytes());
        let mut hex = String::with_capacity(hash.len() * 2);
        for byte in hash {
            write!(&mut hex, "{byte:02x}").expect("write hex");
        }
        let head = &hex[..2];
        let tail = &hex[2..];
        self.metadata().join(head).join(format!("{tail}.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pipe_trait::Pipe;
    use pretty_assertions::assert_eq;

    #[test]
    fn file_path_by_head_tail() {
        let received = "/home/user/.local/share/pnpm/store"
            .pipe(StoreDir::new)
            .file_path_by_head_tail("3e", "f722d37b016c63ac0126cfdcec");
        let expected = PathBuf::from(
            "/home/user/.local/share/pnpm/store/v3/files/3e/f722d37b016c63ac0126cfdcec",
        );
        assert_eq!(&received, &expected);
    }

    #[test]
    fn tmp() {
        let received = StoreDir::new("/home/user/.local/share/pnpm/store").tmp();
        let expected = PathBuf::from("/home/user/.local/share/pnpm/store/v3/tmp");
        assert_eq!(&received, &expected);
    }

    #[test]
    fn registry_metadata_file_path() {
        let received = StoreDir::new("/home/user/.local/share/pnpm/store")
            .registry_metadata_file_path("https://registry.npmjs.org/", "left-pad");
        assert!(received.starts_with(
            "/home/user/.local/share/pnpm/store/v3/metadata".pipe(PathBuf::from).as_path()
        ));
        assert_eq!(received.extension().and_then(|ext| ext.to_str()), Some("json"));
    }
}
