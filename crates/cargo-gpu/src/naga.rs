//! naga transpiling to wgsl support, hidden behind feature `naga`

use crate::linkage::spv_entry_point_to_wgsl;
use anyhow::Context as _;
use naga::error::ShaderError;
pub use naga::valid::Capabilities;
use spirv_builder::{CompileResult, ModuleResult};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// convert a single spv file to a wgsl file using naga
fn spv_to_wgsl(spv_src: &Path, wgsl_dst: &Path, capabilities: Capabilities) -> anyhow::Result<()> {
    let inner = || -> anyhow::Result<()> {
        let spv_bytes = std::fs::read(spv_src).context("could not read spv file")?;
        let opts = naga::front::spv::Options::default();
        let module = naga::front::spv::parse_u8_slice(&spv_bytes, &opts)
            .map_err(|err| ShaderError {
                source: String::new(),
                label: None,
                inner: Box::new(err),
            })
            .context("naga could not parse spv")?;
        let mut validator =
            naga::valid::Validator::new(naga::valid::ValidationFlags::default(), capabilities);
        let info = validator
            .validate(&module)
            .map_err(|err| ShaderError {
                source: String::new(),
                label: None,
                inner: Box::new(err),
            })
            .context("validation of naga module failed")?;
        let wgsl =
            naga::back::wgsl::write_string(&module, &info, naga::back::wgsl::WriterFlags::empty())
                .context("naga conversion to wgsl failed")?;
        std::fs::write(wgsl_dst, wgsl).context("failed to write wgsl file")?;
        Ok(())
    };
    inner().with_context(|| {
        format!(
            "converting spv '{}' to wgsl '{}' failed    ",
            spv_src.display(),
            wgsl_dst.display()
        )
    })
}

/// convert spv file path to a valid unique wgsl file path
fn wgsl_file_name(path: &Path) -> PathBuf {
    path.with_extension("wgsl")
}

/// Extension trait for naga transpiling
pub trait CompileResultNagaExt {
    /// Transpile the spirv binaries to wgsl source code using [`naga`], typically for webgpu compatibility.
    ///
    /// Converts this [`CompileResult`] of spirv binaries and entry points to a [`CompileResult`] pointing to wgsl source code files and their associated wgsl entry
    /// points.
    ///
    /// # Errors
    /// [`naga`] transpile may error in various ways
    fn transpile_to_wgsl(&self, capabilities: Capabilities) -> anyhow::Result<CompileResult>;
}

impl CompileResultNagaExt for CompileResult {
    #[inline]
    fn transpile_to_wgsl(&self, capabilities: Capabilities) -> anyhow::Result<CompileResult> {
        Ok(match &self.module {
            ModuleResult::SingleModule(spv) => {
                let wgsl = wgsl_file_name(spv);
                spv_to_wgsl(spv, &wgsl, capabilities)?;
                let entry_points = self
                    .entry_points
                    .iter()
                    .map(|entry| spv_entry_point_to_wgsl(entry))
                    .collect();
                Self {
                    entry_points,
                    module: ModuleResult::SingleModule(wgsl),
                }
            }
            ModuleResult::MultiModule(map) => {
                let new_map: BTreeMap<String, PathBuf> = map
                    .iter()
                    .map(|(entry_point, spv)| {
                        let wgsl = wgsl_file_name(spv);
                        spv_to_wgsl(spv, &wgsl, capabilities)?;
                        Ok((spv_entry_point_to_wgsl(entry_point), wgsl))
                    })
                    .collect::<anyhow::Result<_>>()?;
                Self {
                    entry_points: new_map.keys().cloned().collect(),
                    module: ModuleResult::MultiModule(new_map),
                }
            }
        })
    }
}
