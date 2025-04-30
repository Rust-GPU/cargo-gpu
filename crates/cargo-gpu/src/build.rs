#![allow(clippy::shadow_reuse, reason = "let's not be silly")]
#![allow(clippy::unwrap_used, reason = "this is basically a test")]
//! `cargo gpu build`, analogous to `cargo build`

use crate::args::BuildArgs;
use crate::linkage::Linkage;
use crate::lockfile::LockfileMismatchHandler;
use crate::{install::Install, target_spec_dir};
use anyhow::Context as _;
use spirv_builder::ModuleResult;
use std::io::Write as _;

/// `cargo build` subcommands
#[derive(clap::Parser, Debug, serde::Deserialize, serde::Serialize)]
pub struct Build {
    /// CLI args for install the `rust-gpu` compiler and components
    #[clap(flatten)]
    pub install: Install,

    /// CLI args for configuring the build of the shader
    #[clap(flatten)]
    pub build_args: BuildArgs,
}

impl Build {
    /// Entrypoint
    #[expect(clippy::too_many_lines, reason = "It's not too confusing")]
    pub fn run(&mut self) -> anyhow::Result<()> {
        let (rustc_codegen_spirv_location, toolchain_channel) = self.install.run()?;

        let _lockfile_mismatch_handler = LockfileMismatchHandler::new(
            &self.install.spirv_install.shader_crate,
            &toolchain_channel,
            self.install
                .spirv_install
                .force_overwrite_lockfiles_v4_to_v3,
        )?;

        let builder = &mut self.build_args.spirv_builder;
        builder.rustc_codegen_spirv_location = Some(rustc_codegen_spirv_location);
        builder.toolchain_overwrite = Some(toolchain_channel);
        builder.path_to_crate = Some(self.install.spirv_install.shader_crate.clone());
        builder.path_to_target_spec = Some(target_spec_dir()?.join(format!(
            "{}.json",
            builder.target.as_ref().context("expect target to be set")?
        )));

        // Ensure the shader output dir exists
        log::debug!(
            "ensuring output-dir '{}' exists",
            self.build_args.output_dir.display()
        );
        std::fs::create_dir_all(&self.build_args.output_dir)?;
        let canonicalized = self.build_args.output_dir.canonicalize()?;
        log::debug!("canonicalized output dir: {canonicalized:?}");
        self.build_args.output_dir = canonicalized;

        // Ensure the shader crate exists
        self.install.spirv_install.shader_crate =
            self.install.spirv_install.shader_crate.canonicalize()?;
        anyhow::ensure!(
            self.install.spirv_install.shader_crate.exists(),
            "shader crate '{}' does not exist. (Current dir is '{}')",
            self.install.spirv_install.shader_crate.display(),
            std::env::current_dir()?.display()
        );

        if !self.build_args.watch {
            crate::user_output!(
                "Compiling shaders at {}...\n",
                self.install.spirv_install.shader_crate.display()
            );
        }

        let result = self.build_args.spirv_builder.build()?;

        let shaders = match &result.module {
            ModuleResult::MultiModule(modules) => {
                assert!(!modules.is_empty(), "No shader modules to compile");
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
                let path = self.build_args.output_dir.join(
                    filepath
                        .file_name()
                        .context("Couldn't parse file name from shader module path")?,
                );
                log::debug!("copying {} to {}", filepath.display(), path.display());
                std::fs::copy(&filepath, &path)?;
                log::debug!(
                    "linkage of {} relative to {}",
                    path.display(),
                    self.install.spirv_install.shader_crate.display()
                );
                let spv_path = path
                    .relative_to(&self.install.spirv_install.shader_crate)
                    .map_or(path, |path_relative_to_shader_crate| {
                        path_relative_to_shader_crate.to_path("")
                    });
                Ok(Linkage::new(entry, spv_path))
            })
            .collect::<anyhow::Result<Vec<Linkage>>>()?;
        // Sort the contents so the output is deterministic
        linkage.sort();

        // Write the shader manifest json file
        let manifest_path = self
            .build_args
            .output_dir
            .join(&self.build_args.manifest_file);
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
            assert_eq!(shader_crate_path, build.install.spirv_install.shader_crate);
            assert_eq!(output_dir, build.build_args.output_dir);

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
