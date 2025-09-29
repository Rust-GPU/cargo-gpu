//! utilities for tests

#![cfg(test)]

use std::{cell::RefCell, io::Write as _};

fn copy_dir_all(
    src: impl AsRef<std::path::Path>,
    dst: impl AsRef<std::path::Path>,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(&dst)?;
    for maybe_entry in std::fs::read_dir(src)? {
        let entry = maybe_entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            std::fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}
pub fn shader_crate_template_path() -> std::path::PathBuf {
    let project_base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    project_base.join("../shader-crate-template")
}

thread_local! {
    static TEMPDIR: RefCell<Option<tempfile::TempDir>> = RefCell::new(Some(
        tempfile::TempDir::with_prefix("shader_crate").unwrap(),
    ));
}

pub fn shader_crate_test_path() -> std::path::PathBuf {
    TEMPDIR.with_borrow(|tempdir| {
        let shader_crate_path = tempdir.as_ref().unwrap().path();
        copy_dir_all(shader_crate_template_path(), shader_crate_path).unwrap();
        shader_crate_path.to_path_buf()
    })
}

pub fn overwrite_shader_cargo_toml(shader_crate_path: &std::path::Path) -> std::fs::File {
    let cargo_toml = shader_crate_path.join("Cargo.toml");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(cargo_toml)
        .unwrap();
    writeln!(file, "[package]").unwrap();
    writeln!(file, "name = \"test\"").unwrap();
    file
}

pub fn tests_teardown() {
    TEMPDIR.with_borrow_mut(|tempdir| {
        tempdir.take().unwrap().close().unwrap();
    });
}
