//! `cargo gpu build`, analogous to `cargo build`

use core::convert::Infallible;
use std::{
    io::Write as _,
    panic,
    path::{Path, PathBuf},
    thread,
};

use anyhow::Context as _;

use crate::{
    cargo_gpu_build::{
        build::{CargoGpuBuildMetadata, CargoGpuBuilder, CargoGpuBuilderParams},
        metadata::{RustGpuMetadata, RustGpuMetadataSource},
        spirv_builder::{CompileResult, ModuleResult},
    },
    install::InstallArgs,
    linkage::Linkage,
    user_consent::ask_for_user_consent,
};

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

    /// The flattened [`CargoGpuBuildMetadata`].
    #[clap(flatten)]
    #[serde(flatten)]
    pub build_meta: CargoGpuBuildMetadata,

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
            build_meta: CargoGpuBuildMetadata::default(),
            manifest_file: String::from("manifest.json"),
        }
    }
}

/// `cargo build` subcommands.
#[derive(Clone, Debug, Default, clap::Parser, serde::Deserialize, serde::Serialize)]
#[non_exhaustive]
pub struct Build {
    /// CLI args for install the `rust-gpu` compiler and components.
    #[clap(flatten)]
    pub install: InstallArgs,

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
        let Self { install, build } = self;
        let InstallArgs { install_meta, .. } = install;
        let BuildArgs { build_meta, .. } = build;

        build_meta.spirv_builder.path_to_crate = Some(install.shader_crate.clone());

        let skip_consent = install.auto_install_rust_toolchain;
        let halt = ask_for_user_consent(skip_consent);
        let crate_builder_params = CargoGpuBuilderParams::from(build_meta.clone())
            .install(install_meta.clone())
            .halt(halt);
        let crate_builder = CargoGpuBuilder::new(crate_builder_params)?;

        install_meta.spirv_installer = crate_builder.installer.clone();
        build_meta.spirv_builder = crate_builder.builder.clone();

        // Ensure the shader output dir exists
        log::debug!(
            "ensuring output-dir '{}' exists",
            build.output_dir.display()
        );
        std::fs::create_dir_all(&build.output_dir)?;
        let canonicalized = dunce::canonicalize(&build.output_dir)?;
        log::debug!("canonicalized output dir: {}", canonicalized.display());
        build.output_dir = canonicalized;

        if build.watch {
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
        let watch_thread = thread::spawn(move || -> ! {
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

impl RustGpuMetadata for Build {
    #[inline]
    fn patch<P>(&mut self, shader_crate: P, source: RustGpuMetadataSource<'_>)
    where
        P: AsRef<Path>,
    {
        let RustGpuMetadataSource::Crate(_) = source else {
            return;
        };

        let output_dir = self.build.output_dir.as_path();
        log::debug!(
            "found output dir path in crate metadata: {}",
            output_dir.display()
        );

        let new_output_dir = shader_crate.as_ref().join(output_dir);
        log::debug!(
            "setting that to be relative to the Cargo.toml it was found in: {}",
            new_output_dir.display()
        );

        self.build.output_dir = new_output_dir;
    }
}

#[cfg(test)]
mod test {
    use clap::Parser as _;
    use serde_json::json;

    use crate::{
        cargo_gpu_build::{
            metadata::{from_cargo_metadata, with_shader_crate},
            spirv_builder::Capability,
            spirv_cache::metadata::query_metadata,
        },
        test::{
            overwrite_shader_cargo_toml, shader_crate_template_path, shader_crate_test_path,
            tests_teardown,
        },
        Cli, Command,
    };

    use super::*;

    const MANIFEST: &str = env!("CARGO_MANIFEST_DIR");
    const PACKAGE: &str = env!("CARGO_PKG_NAME");

    #[test_log::test]
    fn metadata_default() {
        let mut metadata = query_metadata(MANIFEST).unwrap();
        metadata.packages.first_mut().unwrap().metadata = json!({});

        let configs = from_cargo_metadata::<Build, _>(&metadata, Path::new("./")).unwrap();
        assert!(configs.build.build_meta.spirv_builder.release);
        assert!(!configs.install.auto_install_rust_toolchain);
    }

    #[test_log::test]
    fn metadata_override_from_workspace() {
        let mut metadata = query_metadata(MANIFEST).unwrap();
        metadata.workspace_metadata = json!({
            "rust-gpu": {
                "build": {
                    "release": false
                },
                "install": {
                    "auto-install-rust-toolchain": true
                }
            }
        });

        let configs = from_cargo_metadata::<Build, _>(&metadata, Path::new("./")).unwrap();
        assert!(!configs.build.build_meta.spirv_builder.release);
        assert!(configs.install.auto_install_rust_toolchain);
    }

    #[test_log::test]
    fn metadata_override_from_crate() {
        let mut metadata = query_metadata(MANIFEST).unwrap();
        let cargo_gpu = metadata
            .packages
            .iter_mut()
            .find(|package| package.name.contains(PACKAGE))
            .unwrap();
        cargo_gpu.metadata = json!({
            "rust-gpu": {
                "build": {
                    "release": false
                },
                "install": {
                    "auto-install-rust-toolchain": true
                }
            }
        });

        let configs = from_cargo_metadata::<Build, _>(&metadata, Path::new(".")).unwrap();
        assert!(!configs.build.build_meta.spirv_builder.release);
        assert!(configs.install.auto_install_rust_toolchain);
    }

    #[test_log::test]
    fn booleans_from_cli() {
        let shader_crate_path = shader_crate_test_path();
        let config = Build::parse_from([
            "gpu".as_ref(),
            // "build".as_ref(),
            "--shader-crate".as_ref(),
            shader_crate_path.as_os_str(),
            "--debug".as_ref(),
            "--auto-install-rust-toolchain".as_ref(),
        ]);

        let args = with_shader_crate(&config, &shader_crate_path).unwrap();
        assert!(!args.build.build_meta.spirv_builder.release);
        assert!(args.install.auto_install_rust_toolchain);
    }

    #[test_log::test]
    fn booleans_from_cargo() {
        let shader_crate_path = shader_crate_test_path();

        let mut file = overwrite_shader_cargo_toml(&shader_crate_path);
        file.write_all(
            [
                "[package.metadata.rust-gpu.build]",
                "release = false",
                "[package.metadata.rust-gpu.install]",
                "auto-install-rust-toolchain = true",
            ]
            .join("\n")
            .as_bytes(),
        )
        .unwrap();

        let config = Build::parse_from([
            "gpu".as_ref(),
            // "build".as_ref(),
            "--shader-crate".as_ref(),
            shader_crate_path.as_os_str(),
        ]);

        let args = with_shader_crate(&config, &shader_crate_path).unwrap();
        assert!(!args.build.build_meta.spirv_builder.release);
        assert!(args.install.auto_install_rust_toolchain);
    }

    fn update_cargo_output_dir() -> PathBuf {
        let shader_crate_path = shader_crate_test_path();
        let mut file = overwrite_shader_cargo_toml(&shader_crate_path);
        file.write_all(
            [
                "[package.metadata.rust-gpu.build]",
                "output-dir = \"/the/moon\"",
            ]
            .join("\n")
            .as_bytes(),
        )
        .unwrap();
        shader_crate_path
    }

    #[test_log::test]
    fn string_from_cargo() {
        let shader_crate_path = update_cargo_output_dir();
        let config = Build::parse_from([
            "gpu".as_ref(),
            // "build".as_ref(),
            "--shader-crate".as_ref(),
            shader_crate_path.as_os_str(),
        ]);

        let args = with_shader_crate(&config, &shader_crate_path).unwrap();
        if cfg!(target_os = "windows") {
            assert_eq!(args.build.output_dir, Path::new("C:/the/moon"));
        } else {
            assert_eq!(args.build.output_dir, Path::new("/the/moon"));
        }
    }

    #[test_log::test]
    fn string_from_cargo_overwritten_by_cli() {
        let shader_crate_path = update_cargo_output_dir();
        let config = Build::parse_from([
            "gpu".as_ref(),
            // "build".as_ref(),
            "--shader-crate".as_ref(),
            shader_crate_path.as_os_str(),
            "--output-dir".as_ref(),
            "/the/river".as_ref(),
        ]);

        let args = with_shader_crate(&config, &shader_crate_path).unwrap();
        assert_eq!(args.build.output_dir, Path::new("/the/river"));
    }

    #[test_log::test]
    fn arrays_from_cargo() {
        let shader_crate_path = shader_crate_test_path();

        let mut file = overwrite_shader_cargo_toml(&shader_crate_path);
        file.write_all(
            [
                "[package.metadata.rust-gpu.build]",
                "capabilities = [\"AtomicStorage\", \"Matrix\"]",
            ]
            .join("\n")
            .as_bytes(),
        )
        .unwrap();

        let config = Build::parse_from([
            "gpu".as_ref(),
            // "build".as_ref(),
            "--shader-crate".as_ref(),
            shader_crate_path.as_os_str(),
        ]);

        let args = with_shader_crate(&config, &shader_crate_path).unwrap();
        assert_eq!(
            args.build.build_meta.spirv_builder.capabilities,
            [Capability::AtomicStorage, Capability::Matrix]
        );
    }

    #[test_log::test]
    fn rename_manifest_parse() {
        let shader_crate_path = shader_crate_test_path();
        let config = Build::parse_from([
            "gpu".as_ref(),
            // "build".as_ref(),
            "--shader-crate".as_ref(),
            shader_crate_path.as_os_str(),
            "--manifest-file".as_ref(),
            "mymanifest".as_ref(),
        ]);

        let args = with_shader_crate(&config, &shader_crate_path).unwrap();
        assert_eq!(args.build.manifest_file, "mymanifest".to_owned());
    }

    #[test_log::test]
    fn builder_from_params() {
        tests_teardown();

        let shader_crate_path = shader_crate_template_path();
        let output_dir = shader_crate_path.join("shaders");

        let args = [
            "target/debug/cargo-gpu".as_ref(),
            "build".as_ref(),
            "--shader-crate".as_ref(),
            shader_crate_path.as_os_str(),
            "--output-dir".as_ref(),
            output_dir.as_os_str(),
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
