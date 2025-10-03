//! `cargo gpu build`, analogous to `cargo build`

use core::convert::Infallible;
use std::{io::Write as _, panic, path::PathBuf};

use anyhow::Context as _;
use cargo_gpu_build::{
    build::{CargoGpuBuilder, CargoGpuBuilderParams},
    spirv_builder::{CompileResult, ModuleResult, SpirvBuilder},
};

use crate::{install::Install, linkage::Linkage, user_consent::ask_for_user_consent};

/// Args for just a build.
#[derive(Debug, Clone, clap::Parser, serde::Deserialize, serde::Serialize)]
#[non_exhaustive]
#[expect(clippy::module_name_repetitions, reason = "it is intended")]
pub struct BuildArgs {
    /// Path to the output directory for the compiled shaders.
    #[clap(long, short, default_value = "./")]
    pub output_dir: PathBuf,

    /// Watch the shader crate directory and automatically recompile on changes.
    #[clap(long, short, action)]
    pub watch: bool,

    /// The flattened [`SpirvBuilder`].
    #[clap(flatten)]
    #[serde(flatten)]
    pub spirv_builder: SpirvBuilder,

    /// Renames the manifest.json file to the given name.
    #[clap(long, short, default_value = "manifest.json")]
    pub manifest_file: String,
}

impl Default for BuildArgs {
    #[inline]
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("./"),
            watch: false,
            spirv_builder: SpirvBuilder::default(),
            manifest_file: String::from("manifest.json"),
        }
    }
}

/// `cargo build` subcommands.
#[derive(Clone, Debug, clap::Parser, serde::Deserialize, serde::Serialize)]
#[non_exhaustive]
pub struct Build {
    /// CLI args for install the `rust-gpu` compiler and components.
    #[clap(flatten)]
    pub install: Install,

    /// CLI args for configuring the build of the shader.
    #[clap(flatten)]
    pub build: BuildArgs,
}

impl Build {
    /// Builds the shader crate.
    ///
    /// # Errors
    ///
    /// Returns an error if the build process fails somehow.
    #[inline]
    pub fn run(&mut self) -> anyhow::Result<()> {
        self.build.spirv_builder.path_to_crate = Some(self.install.shader_crate.clone());

        let halt = ask_for_user_consent(self.install.auto_install_rust_toolchain);
        let crate_builder_params = CargoGpuBuilderParams::from(self.build.spirv_builder.clone())
            .install(self.install.spirv_installer.clone())
            .force_overwrite_lockfiles_v4_to_v3(self.install.force_overwrite_lockfiles_v4_to_v3)
            .halt(halt);
        let crate_builder = CargoGpuBuilder::new(crate_builder_params)?;

        self.install.spirv_installer = crate_builder.installer.clone();
        self.build.spirv_builder = crate_builder.builder.clone();

        // Ensure the shader output dir exists
        log::debug!(
            "ensuring output-dir '{}' exists",
            self.build.output_dir.display()
        );
        std::fs::create_dir_all(&self.build.output_dir)?;
        let canonicalized = dunce::canonicalize(&self.build.output_dir)?;
        log::debug!("canonicalized output dir: {}", canonicalized.display());
        self.build.output_dir = canonicalized;

        if self.build.watch {
            let never = self.watch(crate_builder)?;
            match never {}
        }
        self.build(crate_builder)
    }

    /// Builds shader crate using [`CargoGpuBuilder`].
    fn build(&self, mut crate_builder: CargoGpuBuilder) -> anyhow::Result<()> {
        let result = crate_builder.build()?;
        self.parse_compilation_result(&result)?;
        Ok(())
    }

    /// Watches shader crate for changes using [`CargoGpuBuilder`].
    fn watch(&self, mut crate_builder: CargoGpuBuilder) -> anyhow::Result<Infallible> {
        let this = self.clone();
        let mut watcher = crate_builder.watch()?;
        let watch_thread = std::thread::spawn(move || -> ! {
            loop {
                let compile_result = match watcher.recv() {
                    Ok(compile_result) => compile_result,
                    Err(err) => {
                        log::error!("{err}");
                        continue;
                    }
                };
                if let Err(err) = this.parse_compilation_result(&compile_result) {
                    log::error!("{err}");
                }
            }
        });
        match watch_thread.join() {
            Ok(never) => never,
            Err(payload) => {
                log::error!("watch thread panicked");
                panic::resume_unwind(payload)
            }
        }
    }

    /// Parses compilation result from [`SpirvBuilder`] and writes it out to a file.
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
