//! Args for building and installing. Previously in `spirv-tools-cli` but got moved here.

use spirv_builder::SpirvBuilder;

/// All args for a build and install
#[derive(clap::Parser, Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct AllArgs {
    /// build args
    #[clap(flatten)]
    pub build: BuildArgs,

    /// install args
    #[clap(flatten)]
    pub install: InstallArgs,
}

/// Args for just a build
#[derive(clap::Parser, Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct BuildArgs {
    /// Path to the output directory for the compiled shaders.
    #[clap(long, short, default_value = "./")]
    pub output_dir: std::path::PathBuf,

    /// Watch the shader crate directory and automatically recompile on changes.
    #[clap(long, short, action)]
    pub watch: bool,

    /// the flattened [`SpirvBuilder`]
    #[clap(flatten)]
    #[serde(flatten)]
    pub spirv_builder: SpirvBuilder,

    ///Renames the manifest.json file to the given name
    #[clap(long, short, default_value = "manifest.json")]
    pub manifest_file: String,
}

/// Args for an install
#[derive(clap::Parser, Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct InstallArgs {
    /// path to the `rustc_codegen_spirv` dylib
    #[clap(long, hide(true), default_value = "INTERNALLY_SET")]
    pub dylib_path: std::path::PathBuf,

    /// Directory containing the shader crate to compile.
    #[clap(long, default_value = "./")]
    pub shader_crate: std::path::PathBuf,

    #[expect(
        clippy::doc_markdown,
        reason = "The URL should appear literally like this. But Clippy wants it to be a in markdown clickable link"
    )]
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
    ///   * <https://github.com/rust-lang/cargo/pull/12280>
    ///   * <https://github.com/rust-lang/cargo/pull/14595>
    #[clap(long, action, verbatim_doc_comment)]
    pub force_overwrite_lockfiles_v4_to_v3: bool,
}
