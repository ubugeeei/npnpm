use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_npmrc::PackageImportMethod;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Error type for [`link_file`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum LinkFileError {
    #[display("fail to import file from {from:?} to {to:?} with {method:?}: {error}")]
    CreateLink {
        from: PathBuf,
        to: PathBuf,
        method: PackageImportMethod,
        #[error(source)]
        error: io::Error,
    },
}

/// Import a single file from the store into the virtual store.
///
/// * If `target_link` already exists, do nothing.
/// * The parent directory of `target_link` is assumed to already exist.
pub fn link_file(
    import_method: PackageImportMethod,
    source_file: &Path,
    target_link: &Path,
) -> Result<(), LinkFileError> {
    if target_link.exists() {
        return Ok(());
    }

    // NOTE: once lifecycle scripts start mutating installed files, hardlinking should be
    // selectively disabled for packages with side effects just like pnpm does.
    let import_result = match import_method {
        PackageImportMethod::Auto => reflink_copy::reflink(source_file, target_link)
            .or_else(|_| fs::hard_link(source_file, target_link))
            .or_else(|_| fs::copy(source_file, target_link).map(|_| ())),
        PackageImportMethod::Hardlink => fs::hard_link(source_file, target_link),
        PackageImportMethod::Copy => fs::copy(source_file, target_link).map(|_| ()),
        PackageImportMethod::Clone => reflink_copy::reflink(source_file, target_link),
        PackageImportMethod::CloneOrCopy => {
            reflink_copy::reflink_or_copy(source_file, target_link).map(|_| ())
        }
    };
    import_result.map_err(|error| LinkFileError::CreateLink {
        from: source_file.to_path_buf(),
        to: target_link.to_path_buf(),
        method: import_method,
        error,
    })?;

    Ok(())
}
