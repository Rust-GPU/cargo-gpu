//! naga transpiling to wgsl support, hidden behind feature `naga`

use anyhow::Context as _;
use naga::error::ShaderError;
use naga::valid::Capabilities;
use naga::valid::ModuleInfo;
use naga::Module;
use spirv_builder::{CompileResult, GenericCompileResult};
use std::path::{Path, PathBuf};

pub use naga;

/// Naga [`Module`] with [`ModuleInfo`]
#[derive(Clone, Debug)]
#[expect(
    clippy::exhaustive_structs,
    reason = "never adding private members to this struct"
)]
pub struct NagaModule {
    /// path to the original spv
    pub spv_path: PathBuf,
    /// naga shader [`Module`]
    pub module: Module,
    /// naga [`ModuleInfo`] from validation
    pub info: ModuleInfo,
}

/// convert a single spv file to a wgsl file using naga
fn parse_spv(spv_src: &Path, capabilities: Capabilities) -> anyhow::Result<NagaModule> {
    let inner = || -> anyhow::Result<_> {
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
        Ok(NagaModule {
            module,
            info,
            spv_path: PathBuf::from(spv_src),
        })
    };
    inner().with_context(|| format!("parsing spv '{}' failed", spv_src.display()))
}

/// Extension trait for naga transpiling
pub trait CompileResultNagaExt {
    /// Transpile the spirv binaries to some other format using [`naga`].
    ///
    /// # Errors
    /// [`naga`] transpile may error in various ways
    fn naga_transpile(&self, capabilities: Capabilities) -> anyhow::Result<NagaTranspile>;
}

impl CompileResultNagaExt for CompileResult {
    #[inline]
    fn naga_transpile(&self, capabilities: Capabilities) -> anyhow::Result<NagaTranspile> {
        Ok(NagaTranspile(self.try_map(
            |entry| Ok(entry.clone()),
            |spv| parse_spv(spv, capabilities),
        )?))
    }
}

/// Main struct for naga transpilation
#[expect(
    clippy::exhaustive_structs,
    reason = "never adding private members to this struct"
)]
pub struct NagaTranspile(pub GenericCompileResult<NagaModule>);

/// feature gate `wgsl-out`
#[cfg(feature = "wgsl-out")]
mod wgsl_out {
    use crate::NagaTranspile;
    use anyhow::Context as _;
    use naga::back::wgsl::WriterFlags;
    use spirv_builder::CompileResult;

    impl NagaTranspile {
        /// Transpile to wgsl source code, typically for webgpu compatibility.
        ///
        /// Returns a [`CompileResult`] of wgsl source code files and their associated wgsl entry points.
        ///
        /// # Errors
        /// converting naga module to wgsl may fail
        #[inline]
        pub fn to_wgsl(&self, writer_flags: WriterFlags) -> anyhow::Result<CompileResult> {
            self.0.try_map(
                |entry| Ok(crate::linkage::spv_entry_point_to_wgsl(entry)),
                |module| {
                    let inner = || -> anyhow::Result<_> {
                        let wgsl_dst = module.spv_path.with_extension("wgsl");
                        let wgsl = naga::back::wgsl::write_string(
                            &module.module,
                            &module.info,
                            writer_flags,
                        )
                        .context("naga conversion to wgsl failed")?;
                        std::fs::write(&wgsl_dst, wgsl).context("failed to write wgsl file")?;
                        Ok(wgsl_dst)
                    };
                    inner().with_context(|| {
                        format!("transpiling to wgsl '{}'", module.spv_path.display())
                    })
                },
            )
        }
    }
}
