//! Display various information about `cargo gpu`, eg its cache directory.

use std::{fs, path::Path};

use anyhow::bail;
use rustc_codegen_spirv_cache::{
    cache::cache_dir,
    spirv_source::{query_metadata, SpirvSource},
    target_specs::update_target_specs_files,
};

/// Show the computed source of the spirv-std dependency.
#[derive(Clone, Debug, clap::Parser)]
pub struct SpirvSourceDep {
    /// The location of the shader-crate to inspect to determine its spirv-std dependency.
    #[clap(long, default_value = "./")]
    pub shader_crate: std::path::PathBuf,
}

/// Different tidbits of information that can be queried at the command line.
#[derive(Clone, Debug, clap::Subcommand)]
pub enum Info {
    /// Displays the location of the cache directory
    CacheDirectory,
    /// The source location of spirv-std
    SpirvSource(SpirvSourceDep),
    /// The git commitsh of this cli tool.
    Commitsh,
    /// All the available SPIR-V capabilities that can be set with `--capabilities`
    Capabilities,

    /// All available SPIR-V targets
    Targets(SpirvSourceDep),
}

/// `cargo gpu show`
#[derive(clap::Parser)]
pub struct Show {
    /// Display information about rust-gpu
    #[clap(subcommand)]
    command: Info,
}

impl Show {
    /// Entrypoint
    pub fn run(&self) -> anyhow::Result<()> {
        log::info!("{:?}: ", self.command);

        #[expect(
            clippy::print_stdout,
            reason = "The output of this command could potentially be used in a script, \
                      so we _don't_ want to use `crate::user_output`, as that prefixes a crab."
        )]
        match &self.command {
            Info::CacheDirectory => {
                println!("{}\n", cache_dir()?.display());
            }
            Info::SpirvSource(SpirvSourceDep { shader_crate }) => {
                let rust_gpu_source = SpirvSource::get_rust_gpu_deps_from_shader(shader_crate)?;
                println!("{rust_gpu_source}\n");
            }
            Info::Commitsh => {
                println!("{}", env!("GIT_HASH"));
            }
            Info::Capabilities => {
                println!("All available options to the `cargo gpu build --capabilities` argument:");
                #[expect(
                    clippy::use_debug,
                    reason = "It's easier to just use `Debug` formatting than implementing `Display`"
                )]
                for capability in Self::capability_variants_iter() {
                    println!("  {capability:?}");
                }
            }
            Info::Targets(SpirvSourceDep { shader_crate }) => {
                let (source, targets) = Self::available_spirv_targets_iter(shader_crate)?;
                println!("All available targets for rust-gpu version '{source}':");
                for target in targets {
                    println!("{target}");
                }
            }
        }

        Ok(())
    }

    /// Iterator over all `Capability` variants.
    fn capability_variants_iter() -> impl Iterator<Item = spirv_builder::Capability> {
        // Since spirv::Capability is repr(u32) we can iterate over
        // u32s until some maximum
        #[expect(clippy::as_conversions, reason = "We know all variants are repr(u32)")]
        let last_capability = spirv_builder::Capability::CacheControlsINTEL as u32;
        (0..=last_capability).filter_map(spirv_builder::Capability::from_u32)
    }

    /// List all available spirv targets, note: the targets from compile time of cargo-gpu and those
    /// in the cache-directory will be picked up.
    fn available_spirv_targets_iter(
        shader_crate: &Path,
    ) -> anyhow::Result<(SpirvSource, impl Iterator<Item = String>)> {
        let source = SpirvSource::new(shader_crate, None, None)?;
        let install_dir = source.install_dir()?;
        if !install_dir.is_dir() {
            bail!("rust-gpu version {} is not installed", source);
        }
        let dummy_metadata = query_metadata(&install_dir)?;
        let target_specs_dir = update_target_specs_files(&source, &dummy_metadata, false)?;

        let mut targets = fs::read_dir(target_specs_dir)?
            .filter_map(|entry| {
                let file = entry.ok()?;
                if file.path().is_file() {
                    if let Some(target) = file.file_name().to_string_lossy().strip_suffix(".json") {
                        return Some(target.to_owned());
                    }
                }
                None
            })
            .collect::<Vec<_>>();
        targets.sort();
        Ok((source, targets.into_iter()))
    }
}
