#[cfg(feature = "spirv-builder-pre-cli")]
use spirv_0_2 as spirv;

#[cfg(any(feature = "spirv-builder-0_10", feature = "rspirv-latest"))]
use spirv_0_3 as spirv;

use std::str::FromStr as _;

#[derive(clap::Parser, Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct AllArgs {
    #[clap(flatten)]
    pub build: BuildArgs,

    #[clap(flatten)]
    pub install: InstallArgs,
}

/// Options for the `--spirv-metadata` command
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub enum SpirvMetadata {
    /// Don't log any metadata (the default)
    None,
    /// Only log named variables
    NameVariables,
    /// Log all metadata
    Full,
}

#[derive(clap::Parser, Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct BuildArgs {
    /// Path to the output directory for the compiled shaders.
    #[clap(long, short, default_value = "./")]
    pub output_dir: std::path::PathBuf,

    /// Watch the shader crate directory and automatically recompile on changes.
    #[clap(long, short, action)]
    pub watch: bool,

    /// Set shader crate's cargo default-features.
    #[clap(long)]
    pub no_default_features: bool,

    /// Set shader crate's cargo features.
    #[clap(long)]
    pub features: Vec<String>,

    /// `rust-gpu` compile target.
    /// TODO: deprecate completely
    #[arg(hide(true), default_value = "spirv-unknown-vulkan1.2")]
    pub target: String,

    /// Shader target.
    // TODO: how to list the available options? Would be nice to have a command like:
    //   `cargo gpu show targets`
    #[clap(long, default_value = "spirv-unknown-vulkan1.2")]
    pub shader_target: String,

    /// Treat warnings as errors during compilation.
    #[arg(long, default_value = "false")]
    pub deny_warnings: bool,

    /// Compile shaders in debug mode.
    #[arg(long, default_value = "false")]
    pub debug: bool,

    /// Enables the provided SPIR-V capabilities.
    /// See: `cargo gpu show capabilities`
    #[arg(long, value_parser=Self::spirv_capability)]
    pub capability: Vec<spirv::Capability>,

    /// Enables the provided SPIR-V extensions.
    /// See <https://github.com/KhronosGroup/SPIRV-Registry> for all extensions
    #[arg(long)]
    pub extension: Vec<String>,

    /// Compile one .spv file per entry point.
    #[arg(long, default_value = "false")]
    pub multimodule: bool,

    /// Set the level of metadata included in the SPIR-V binary.
    #[arg(long, value_parser=Self::spirv_metadata, default_value = "none")]
    pub spirv_metadata: SpirvMetadata,

    /// Allow store from one struct type to a different type with compatible layout and members.
    #[arg(long, default_value = "false")]
    pub relax_struct_store: bool,

    /// Allow allocating an object of a pointer type and returning a pointer value from a function
    /// in logical addressing mode.
    #[arg(long, default_value = "false")]
    pub relax_logical_pointer: bool,

    /// Enable `VK_KHR_relaxed_block_layout` when checking standard uniform,
    /// storage buffer, and push constant layouts.
    /// This is the default when targeting Vulkan 1.1 or later.
    #[arg(long, default_value = "false")]
    pub relax_block_layout: bool,

    /// Enable `VK_KHR_uniform_buffer_standard_layout` when checking standard uniform buffer layouts.
    #[arg(long, default_value = "false")]
    pub uniform_buffer_standard_layout: bool,

    /// Enable `VK_EXT_scalar_block_layout` when checking standard uniform, storage buffer, and push
    /// constant layouts.
    /// Scalar layout rules are more permissive than relaxed block layout so in effect this will
    /// override the --relax-block-layout option.
    #[arg(long, default_value = "false")]
    pub scalar_block_layout: bool,

    /// Skip checking standard uniform / storage buffer layout. Overrides any --relax-block-layout
    /// or --scalar-block-layout option.
    #[arg(long, default_value = "false")]
    pub skip_block_layout: bool,

    /// Preserve unused descriptor bindings. Useful for reflection.
    #[arg(long, default_value = "false")]
    pub preserve_bindings: bool,

    ///Renames the manifest.json file to the given name
    #[clap(long, short, default_value = "manifest.json")]
    pub manifest_file: String,
}

impl BuildArgs {
    /// Clap value parser for `SpirvMetadata`.
    fn spirv_metadata(metadata: &str) -> Result<SpirvMetadata, clap::Error> {
        match metadata {
            "none" => Ok(SpirvMetadata::None),
            "name-variables" => Ok(SpirvMetadata::NameVariables),
            "full" => Ok(SpirvMetadata::Full),
            _ => Err(clap::Error::new(clap::error::ErrorKind::InvalidValue)),
        }
    }

    /// Clap value parser for `Capability`.
    fn spirv_capability(capability: &str) -> Result<spirv::Capability, clap::Error> {
        spirv::Capability::from_str(capability).map_or_else(
            |()| Err(clap::Error::new(clap::error::ErrorKind::InvalidValue)),
            Ok,
        )
    }
}

#[derive(clap::Parser, Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct InstallArgs {
    #[clap(long, hide(true), default_value = "INTERNALLY_SET")]
    pub dylib_path: std::path::PathBuf,

    /// Directory containing the shader crate to compile.
    #[clap(long, default_value = "./")]
    pub shader_crate: std::path::PathBuf,

    /// Source of `spirv-builder` dependency
    /// Eg: "https://github.com/Rust-GPU/rust-gpu"
    #[clap(long)]
    pub spirv_builder_source: Option<String>,

    /// Version of `spirv-builder` dependency.
    /// * If `--spirv-builder-source` is not set, then this is assumed to be a crates.io semantic
    ///   version such as "0.9.0".
    /// * If `--spirv-builder-source` is set, then this is assumed to be a Git "commitsh", such
    ///   as a Git commit hash or a Git tag, therefore anything that `git checkout` can resolve.
    #[clap(long, verbatim_doc_comment)]
    pub spirv_builder_version: Option<String>,

    /// Rust toolchain channel to use to build `spirv-builder`.
    ///
    /// This must be compatible with the `spirv_builder` argument as defined in the `rust-gpu` repo.
    #[clap(long)]
    pub rust_toolchain: Option<String>,

    /// Force `spirv-builder-cli` and `rustc_codegen_spirv` to be rebuilt.
    #[clap(long)]
    pub force_spirv_cli_rebuild: bool,

    /// Assume "yes" to "Install Rust toolchain: [y/n]" prompt.
    #[clap(long, action)]
    pub auto_install_rust_toolchain: bool,

    /// There is a tricky situation where a shader crate that depends on workspace config can have
    /// a different `Cargo.lock` lockfile version from the the workspace's `Cargo.lock`. This can
    /// prevent builds when an old Rust toolchain doesn't recognise the newer lockfile version.
    ///
    /// The ideal way to resolve this would be to match the shader crate's toolchain with the
    /// workspace's toolchain. However, that is not always possible. Another solution is to
    /// `exclude = [...]` the problematic shader crate from the workspace. This also may not be a
    /// suitable solution if there are a number of shader crates all sharing similar config and
    /// you don't want to have to copy/paste and maintain that config across all the shaders.
    ///
    /// So a somewhat hacky workaround is to have `cargo gpu` overwrite lockfile versions. Enabling
    /// this flag will only come into effect if there are a mix of v3/v4 lockfiles. It will also
    /// only overwrite versions for the duration of a build. It will attempt to return the versions
    /// to their original values once the build is finished. However, of course, unexpected errors
    /// can occur and the overwritten values can remain. Hence why this behaviour is not enabled by
    /// default.
    ///
    /// This hack is possible because the change from v3 to v4 only involves a minor change to the
    /// way source URLs are encoded. See these PRs for more details:
    ///   * https://github.com/rust-lang/cargo/pull/12280
    ///   * https://github.com/rust-lang/cargo/pull/14595
    #[clap(long, action, verbatim_doc_comment)]
    pub force_overwrite_lockfiles_v4_to_v3: bool,
}
