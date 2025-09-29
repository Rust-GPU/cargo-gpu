//! This module deals with target specs, which are json metadata files that need to be passed to
//! rustc to add foreign targets such as `spirv_unknown_vulkan1.2`.
//!
//! There are 4 version ranges of `rustc_codegen_spirv` and they all need different handling of
//! their target specs:
//! * "ancient" versions such as 0.9.0 or earlier do not need target specs, just passing the target
//!   string (`spirv-unknown-vulkan1.2`) directly is sufficient. We still prep target-specs for them
//!   like the "legacy" variant below, spirv-builder
//!   [will just ignore it](https://github.com/Rust-GPU/rust-gpu/blob/369122e1703c0c32d3d46f46fa11ccf12667af03/crates/spirv-builder/src/lib.rs#L987)
//! * "legacy" versions require target specs to compile, which is a requirement introduced by some
//!   rustc version. Back then it was decided that cargo gpu would ship them, as they'd probably
//!   never change, right? So now we're stuck with having to ship these "legacy" target specs with
//!   cargo gpu *forever*. These are the symbol `legacy_target_specs::TARGET_SPECS`, with
//!   `legacy_target_specs` being a **fixed** version of `rustc_codegen_spirv-target-specs`,
//!   which must **never** update.
//! * As of [PR 256](https://github.com/Rust-GPU/rust-gpu/pull/256), `rustc_codegen_spirv` now has
//!   a direct dependency on `rustc_codegen_spirv-target-specs`, allowing cargo gpu to pull the
//!   required target specs directly from that dependency. At this point, the target specs are
//!   still the same as the legacy target specs.
//! * The [edition 2024 PR](https://github.com/Rust-GPU/rust-gpu/pull/249) must update the
//!   target specs to comply with newly added validation within rustc. This is why the new system
//!   was implemented, so we can support both old and new target specs without having to worry
//!   which version of cargo gpu you are using. It'll "just work".

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use cargo_metadata::Metadata;

use crate::{
    cache::cache_dir,
    spirv_source::{FindPackage as _, SpirvSource},
};

/// Extract legacy target specs from our executable into some directory
#[inline]
#[expect(clippy::module_name_repetitions, reason = "such naming is intentional")]
pub fn write_legacy_target_specs(target_spec_dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(target_spec_dir)?;
    for (filename, contents) in legacy_target_specs::TARGET_SPECS {
        let path = target_spec_dir.join(filename);
        std::fs::write(&path, contents.as_bytes())
            .with_context(|| format!("writing legacy target spec file at [{}]", path.display()))?;
    }
    Ok(())
}

/// Copy spec files from one dir to another, assuming no subdirectories
fn copy_spec_files(src: &Path, dst: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dst)?;
    let dir = std::fs::read_dir(src)?;
    for dir_entry in dir {
        let file = dir_entry?;
        let file_path = file.path();
        if file_path.is_file() {
            std::fs::copy(file_path, dst.join(file.file_name()))?;
        }
    }
    Ok(())
}

/// Computes the `target-specs` directory to use and updates the target spec files, if enabled.
#[inline]
pub fn update_target_specs_files(
    source: &SpirvSource,
    dummy_metadata: &Metadata,
    update_files: bool,
) -> anyhow::Result<PathBuf> {
    log::info!(
        "target-specs: Resolving target specs `{}`",
        if update_files {
            "and update them"
        } else {
            "without updating"
        }
    );

    let mut target_specs_dst = source.install_dir()?.join("target-specs");
    if let Ok(target_specs) = dummy_metadata.find_package("rustc_codegen_spirv-target-specs") {
        log::info!(
            "target-specs: found crate `rustc_codegen_spirv-target-specs` with manifest at `{}`",
            target_specs.manifest_path
        );

        let target_specs_src = target_specs
                .manifest_path
                .as_std_path()
                .parent()
                .and_then(|root| {
                    let src = root.join("target-specs");
                    src.is_dir().then_some(src)
                })
                .context("Could not find `target-specs` directory within `rustc_codegen_spirv-target-specs` dependency")?;
        log::info!(
            "target-specs: found `rustc_codegen_spirv-target-specs` with `target-specs` directory `{}`",
            target_specs_dst.display()
        );

        if source.is_path() {
            // skip copy
            log::info!(
                "target-specs resolution: source is local path, use target-specs directly from `{}`",
                target_specs_dst.display()
            );
            target_specs_dst = target_specs_src;
        } else {
            // copy over the target-specs
            log::info!(
                "target-specs resolution: copying target-specs from `{}`{}",
                target_specs_dst.display(),
                if update_files { "" } else { " was skipped" }
            );
            if update_files {
                copy_spec_files(&target_specs_src, &target_specs_dst)
                    .context("copying target-specs json files")?;
            }
        }
    } else {
        // use legacy target specs bundled with cargo gpu
        if source.is_path() {
            // This is a stupid situation:
            // * We can't be certain that there are `target-specs` in the local checkout (there may be some in `spirv-builder`)
            // * We can't dump our legacy ones into the `install_dir`, as that would modify the local rust-gpu checkout
            // -> do what the old cargo gpu did, one global dir for all target specs
            // and hope parallel runs don't shred each other
            target_specs_dst = cache_dir()?.join("legacy-target-specs-for-local-checkout");
        }
        log::info!(
            "target-specs resolution: legacy target specs in directory `{}`",
            target_specs_dst.display()
        );
        if update_files {
            log::info!(
                "target-specs: writing legacy target specs into `{}`",
                target_specs_dst.display()
            );
            write_legacy_target_specs(&target_specs_dst)?;
        }
    }

    Ok(target_specs_dst)
}
