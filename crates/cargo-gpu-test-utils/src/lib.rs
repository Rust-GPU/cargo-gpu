//! Shared code of `cargo-gpu` crates for testing.

#![allow(clippy::unwrap_used, reason = "this is executing only inside of tests")]
#![allow(clippy::missing_panics_doc, reason = "this is expected to panic")]

use std::{
    fs,
    io::Write as _,
    path::{Path, PathBuf},
};

use tempfile::TempDir;

/// Path to the shader crate template directory.
#[inline]
#[must_use]
pub fn shader_crate_template_path() -> PathBuf {
    let project_base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    project_base.join("../shader-crate-template")
}

thread_local! {
    static TEMPDIR: TempDir = TempDir::with_prefix("shader_crate").unwrap();
}

/// Path to the local copy of the shader crate template used for testing.
#[inline]
#[must_use]
pub fn shader_crate_test_path() -> PathBuf {
    TEMPDIR.with(|tempdir| {
        let shader_crate_path = tempdir.path();
        copy_dir_all(shader_crate_template_path(), shader_crate_path).unwrap();
        shader_crate_path.to_path_buf()
    })
}

/// Overwrites the `Cargo.toml` in the shader crate test path
/// with the most basic one.
#[inline]
#[must_use]
pub fn overwrite_shader_cargo_toml(shader_crate_path: &Path) -> fs::File {
    let cargo_toml = shader_crate_path.join("Cargo.toml");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(cargo_toml)
        .unwrap();
    writeln!(file, "[package]").unwrap();
    writeln!(file, "name = \"test\"").unwrap();
    file
}

/// Deletes the temporary shader crate test directory.
#[inline]
pub fn tests_teardown() {
    let dir = shader_crate_test_path();
    if !dir.exists() {
        return;
    }
    fs::remove_dir_all(dir).unwrap();
}

/// Recursively copies all the contents of `src` directory to `dst` directory.
fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> anyhow::Result<()> {
    fs::create_dir_all(&dst)?;
    for maybe_entry in fs::read_dir(src)? {
        let entry = maybe_entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}
