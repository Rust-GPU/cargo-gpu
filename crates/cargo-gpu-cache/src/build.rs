#![allow(clippy::shadow_reuse, reason = "let's not be silly")]
#![allow(clippy::unwrap_used, reason = "this is basically a test")]
//! `cargo gpu build`, analogous to `cargo build`

use anyhow::Context as _;
use spirv_builder::{CompileResult, ModuleResult, SpirvBuilder};
use std::io::Write as _;
use std::path::PathBuf;

use crate::install::Install;
use crate::linkage::Linkage;
use crate::lockfile::LockfileMismatchHandler;

/// Args for just a build
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "clap", derive(clap::Parser))]
pub struct BuildArgs {
    /// Path to the output directory for the compiled shaders.
    #[cfg_attr(feature = "clap", clap(long, short, default_value = "./"))]
    pub output_dir: PathBuf,

    /// Watch the shader crate directory and automatically recompile on changes.
    #[cfg(feature = "watch")]
    #[cfg_attr(feature = "clap", clap(long, short, action))]
    pub watch: bool,

    /// the flattened [`SpirvBuilder`]
    #[cfg_attr(feature = "clap", clap(flatten))]
    #[serde(flatten)]
    pub spirv_builder: SpirvBuilder,

    ///Renames the manifest.json file to the given name
    #[cfg_attr(feature = "clap", clap(long, short, default_value = "manifest.json"))]
    pub manifest_file: String,
}

impl Default for BuildArgs {
    #[inline]
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("./"),
            #[cfg(feature = "watch")]
            watch: false,
            spirv_builder: SpirvBuilder::default(),
            manifest_file: String::from("manifest.json"),
        }
    }
}

/// `cargo build` subcommands
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "clap", derive(clap::Parser))]
pub struct Build {
    /// CLI args for install the `rust-gpu` compiler and components
    #[cfg_attr(feature = "clap", clap(flatten))]
    pub install: Install,

    /// CLI args for configuring the build of the shader
    #[cfg_attr(feature = "clap", clap(flatten))]
    pub build: BuildArgs,
}

impl Build {
    /// Entrypoint
    pub fn run(&mut self) -> anyhow::Result<()> {
        let installed_backend = self.install.run()?;

        let _lockfile_mismatch_handler = LockfileMismatchHandler::new(
            &self.install.shader_crate,
            &installed_backend.toolchain_channel,
            self.install.force_overwrite_lockfiles_v4_to_v3,
        )?;

        let builder = &mut self.build.spirv_builder;
        builder.path_to_crate = Some(self.install.shader_crate.clone());
        installed_backend.configure_spirv_builder(builder)?;

        // Ensure the shader output dir exists
        log::debug!(
            "ensuring output-dir '{}' exists",
            self.build.output_dir.display()
        );
        std::fs::create_dir_all(&self.build.output_dir)?;
        let canonicalized = dunce::canonicalize(&self.build.output_dir)?;
        log::debug!("canonicalized output dir: {}", canonicalized.display());
        self.build.output_dir = canonicalized;

        // Ensure the shader crate exists
        self.install.shader_crate = dunce::canonicalize(&self.install.shader_crate)?;
        anyhow::ensure!(
            self.install.shader_crate.exists(),
            "shader crate '{}' does not exist. (Current dir is '{}')",
            self.install.shader_crate.display(),
            std::env::current_dir()?.display()
        );

        #[cfg(feature = "watch")]
        let watching = self.build.watch;
        #[cfg(not(feature = "watch"))]
        let watching = false;
        if watching {
            return self.watch();
        }

        self.build()
    }

    /// Builds shader crate using [`SpirvBuilder`].
    fn build(&self) -> anyhow::Result<()> {
        crate::user_output!(
            "Compiling shaders at {}...\n",
            self.install.shader_crate.display()
        );
        let result = self.build.spirv_builder.build()?;
        self.parse_compilation_result(&result)?;
        Ok(())
    }

    /// Watches shader crate for changes using [`SpirvBuilder`]
    /// or returns an error depending on presence of `watch` feature.
    fn watch(&self) -> anyhow::Result<()> {
        #[cfg(feature = "watch")]
        {
            let this = self.clone();
            self.build
                .spirv_builder
                .watch(move |result, accept| {
                    let parse_result = this.parse_compilation_result(&result);
                    if let Some(accept) = accept {
                        accept.submit(parse_result);
                    }
                })?
                .context("should always return the first compile result")
                .flatten()?;
            anyhow::bail!("unexpected end of watch")
        }

        #[cfg(not(feature = "watch"))]
        anyhow::bail!("cannot watch for changes without the `watch` feature")
    }

    /// Parses compilation result from [`SpirvBuilder`] and writes it out to a file
    fn parse_compilation_result(&self, result: &CompileResult) -> anyhow::Result<()> {
        let shaders = match &result.module {
            ModuleResult::MultiModule(modules) => {
                anyhow::ensure!(!modules.is_empty(), "No shader modules were compiled");
                modules.iter().collect::<Vec<_>>()
            }
            ModuleResult::SingleModule(filepath) => result
                .entry_points
                .iter()
                .map(|entry| (entry, filepath))
                .collect::<Vec<_>>(),
        };
        let mut linkage: Vec<Linkage> = shaders
            .into_iter()
            .map(|(entry, filepath)| -> anyhow::Result<Linkage> {
                use relative_path::PathExt as _;
                let path = self.build.output_dir.join(
                    filepath
                        .file_name()
                        .context("Couldn't parse file name from shader module path")?,
                );
                log::debug!("copying {} to {}", filepath.display(), path.display());
                std::fs::copy(filepath, &path)?;
                log::debug!(
                    "linkage of {} relative to {}",
                    path.display(),
                    self.install.shader_crate.display()
                );
                let spv_path = path
                    .relative_to(&self.install.shader_crate)
                    .map_or(path, |path_relative_to_shader_crate| {
                        path_relative_to_shader_crate.to_path("")
                    });
                Ok(Linkage::new(entry, spv_path))
            })
            .collect::<anyhow::Result<Vec<Linkage>>>()?;
        // Sort the contents so the output is deterministic
        linkage.sort();

        // Write the shader manifest json file
        let manifest_path = self.build.output_dir.join(&self.build.manifest_file);
        let json = serde_json::to_string_pretty(&linkage)?;
        let mut file = std::fs::File::create(&manifest_path).with_context(|| {
            format!(
                "could not create shader manifest file '{}'",
                manifest_path.display(),
            )
        })?;
        file.write_all(json.as_bytes()).with_context(|| {
            format!(
                "could not write shader manifest file '{}'",
                manifest_path.display(),
            )
        })?;

        log::info!("wrote manifest to '{}'", manifest_path.display());
        Ok(())
    }
}

#[cfg(test)]
mod test {
    #![cfg(feature = "clap")]

    use clap::Parser as _;

    use crate::{Cli, Command};

    #[test_log::test]
    fn builder_from_params() {
        crate::test::tests_teardown();

        let shader_crate_path = crate::test::shader_crate_template_path();
        let output_dir = shader_crate_path.join("shaders");

        let args = [
            "target/debug/cargo-gpu",
            "build",
            "--shader-crate",
            &format!("{}", shader_crate_path.display()),
            "--output-dir",
            &format!("{}", output_dir.display()),
        ];
        if let Cli {
            command: Command::Build(build),
        } = Cli::parse_from(args)
        {
            assert_eq!(shader_crate_path, build.install.shader_crate);
            assert_eq!(output_dir, build.build.output_dir);

            // TODO:
            // For some reason running a full build (`build.run()`) inside tests fails on Windows.
            // The error is in the `build.rs` step of compiling `spirv-tools-sys`. It is not clear
            // from the logged error what the problem is. For now we'll just run a full build
            // outside the tests environment, see `xtask`'s `test-build`.
        } else {
            panic!("was not a build command");
        }
    }
}
