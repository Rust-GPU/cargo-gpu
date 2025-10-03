//! `cargo gpu install`

use std::path::PathBuf;

use cargo_gpu_build::spirv_cache::backend::SpirvCodegenBackendInstaller;

/// `cargo gpu install` subcommands.
#[derive(Clone, Debug, clap::Parser, serde::Deserialize, serde::Serialize)]
#[non_exhaustive]
pub struct Install {
    /// The flattened [`SpirvCodegenBackendInstaller`].
    #[clap(flatten)]
    #[serde(flatten)]
    pub spirv_installer: SpirvCodegenBackendInstaller,

    /// Directory containing the shader crate to compile.
    #[clap(long, alias("package"), short_alias('p'), default_value = "./")]
    #[serde(alias = "package")]
    pub shader_crate: PathBuf,

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
    /// So a somewhat hacky workaround is to overwrite lockfile versions. Enabling this flag
    /// will only come into effect if there are a mix of v3/v4 lockfiles. It will also
    /// only overwrite versions for the duration of a build. It will attempt to return the versions
    /// to their original values once the build is finished. However, of course, unexpected errors
    /// can occur and the overwritten values can remain. Hence why this behaviour is not enabled by
    /// default.
    ///
    /// This hack is possible because the change from v3 to v4 only involves a minor change to the
    /// way source URLs are encoded. See these PRs for more details:
    ///   * <https://github.com/rust-lang/cargo/pull/12280>
    ///   * <https://github.com/rust-lang/cargo/pull/14595>
    #[clap(long, action, verbatim_doc_comment)]
    pub force_overwrite_lockfiles_v4_to_v3: bool,

    /// Assume "yes" to "Install Rust toolchain: [y/n]" prompt.
    #[clap(long, action)]
    pub auto_install_rust_toolchain: bool,
}
