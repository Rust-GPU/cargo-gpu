//! Legacy target specs are spec jsons for versions before `rustc_codegen_spirv-types` came
//! bundled with them. Instead, cargo gpu needs to bundle these legacy spec files. Luckily,
//! they are the same for all versions, as bundling with `rustc_codegen_spirv-types` was
//! introduced before the first target spec update.

use anyhow::Context as _;
use log::info;
use std::path::Path;

/// Extract legacy target specs from our executable into some directory
pub fn write_legacy_target_specs(target_spec_dir: &Path, rebuild: bool) -> anyhow::Result<()> {
    info!(
        "Writing legacy target specs to {}",
        target_spec_dir.display()
    );
    std::fs::create_dir_all(target_spec_dir)?;
    for (filename, contents) in LEGACY_TARGET_SPECS {
        let path = target_spec_dir.join(filename);
        if !path.is_file() || rebuild {
            std::fs::write(&path, contents.as_bytes()).with_context(|| {
                format!("writing legacy target spec file at [{}]", path.display())
            })?;
        }
    }
    Ok(())
}

/// Legacy target specs bundled with cargo gpu
pub const LEGACY_TARGET_SPECS: &[(&str, &str)] = &[
    (
        "spirv-unknown-opengl4.0.json",
        include_str!("../legacy-target-specs/spirv-unknown-opengl4.0.json"),
    ),
    (
        "spirv-unknown-opengl4.1.json",
        include_str!("../legacy-target-specs/spirv-unknown-opengl4.1.json"),
    ),
    (
        "spirv-unknown-opengl4.2.json",
        include_str!("../legacy-target-specs/spirv-unknown-opengl4.2.json"),
    ),
    (
        "spirv-unknown-opengl4.3.json",
        include_str!("../legacy-target-specs/spirv-unknown-opengl4.3.json"),
    ),
    (
        "spirv-unknown-opengl4.5.json",
        include_str!("../legacy-target-specs/spirv-unknown-opengl4.5.json"),
    ),
    (
        "spirv-unknown-spv1.0.json",
        include_str!("../legacy-target-specs/spirv-unknown-spv1.0.json"),
    ),
    (
        "spirv-unknown-spv1.1.json",
        include_str!("../legacy-target-specs/spirv-unknown-spv1.1.json"),
    ),
    (
        "spirv-unknown-spv1.2.json",
        include_str!("../legacy-target-specs/spirv-unknown-spv1.2.json"),
    ),
    (
        "spirv-unknown-spv1.3.json",
        include_str!("../legacy-target-specs/spirv-unknown-spv1.3.json"),
    ),
    (
        "spirv-unknown-spv1.4.json",
        include_str!("../legacy-target-specs/spirv-unknown-spv1.4.json"),
    ),
    (
        "spirv-unknown-spv1.5.json",
        include_str!("../legacy-target-specs/spirv-unknown-spv1.5.json"),
    ),
    (
        "spirv-unknown-vulkan1.0.json",
        include_str!("../legacy-target-specs/spirv-unknown-vulkan1.0.json"),
    ),
    (
        "spirv-unknown-vulkan1.1.json",
        include_str!("../legacy-target-specs/spirv-unknown-vulkan1.1.json"),
    ),
    (
        "spirv-unknown-vulkan1.1spv1.4.json",
        include_str!("../legacy-target-specs/spirv-unknown-vulkan1.1spv1.4.json"),
    ),
    (
        "spirv-unknown-vulkan1.2.json",
        include_str!("../legacy-target-specs/spirv-unknown-vulkan1.2.json"),
    ),
];
