//! Legacy target specs are spec jsons for versions before `rustc_codegen_spirv-target-specs`
//! came bundled with them. Instead, cargo gpu needs to bundle these legacy spec files. Luckily,
//! they are the same for all versions, as bundling target specs with the codegen backend was
//! introduced before the first target spec update.

use anyhow::Context as _;
use std::path::Path;

/// Extract legacy target specs from our executable into some directory
pub fn write_legacy_target_specs(target_spec_dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(target_spec_dir)?;
    for (filename, contents) in legacy_target_specs::TARGET_SPECS {
        let path = target_spec_dir.join(filename);
        std::fs::write(&path, contents.as_bytes())
            .with_context(|| format!("writing legacy target spec file at [{}]", path.display()))?;
    }
    Ok(())
}
