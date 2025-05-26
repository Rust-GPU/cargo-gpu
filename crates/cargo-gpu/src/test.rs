//! utilities for tests
#![cfg(test)]

use crate::cache_dir;
use std::io::Write as _;

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

pub fn shader_crate_test_path() -> std::path::PathBuf {
    let shader_crate_path = crate::cache_dir().unwrap().join("shader_crate");
    copy_dir_all(shader_crate_template_path(), shader_crate_path.clone()).unwrap();
    shader_crate_path
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
    let cache_dir = cache_dir().unwrap();
    if !cache_dir.exists() {
        return;
    }
    std::fs::remove_dir_all(cache_dir).unwrap();
}
