//! `cargo gpu build`, analogous to `cargo build`

use std::io::Write as _;

use clap::Parser;
use spirv_builder_cli::{Linkage, ShaderModule};

use crate::{install::Install, target_spec_dir};

/// `cargo build` subcommands
#[derive(Parser, Debug)]
pub struct Build {
    /// Install the `rust-gpu` compiler and components
    #[clap(flatten)]
    pub install: Install,

    /// Shader target.
    #[clap(long, default_value = "spirv-unknown-vulkan1.2")]
    shader_target: String,

    /// Set cargo default-features.
    #[clap(long)]
    no_default_features: bool,

    /// Set cargo features.
    #[clap(long)]
    features: Vec<String>,

    /// Path to the output directory for the compiled shaders.
    #[clap(long, short, default_value = "./")]
    pub output_dir: std::path::PathBuf,
}

impl Build {
    /// Entrypoint
    pub fn run(&mut self) {
        let (dylib_path, spirv_builder_cli_path) = self.install.run();

        // Ensure the shader output dir exists
        log::debug!("ensuring output-dir '{}' exists", self.output_dir.display());
        std::fs::create_dir_all(&self.output_dir).unwrap();
        self.output_dir = self.output_dir.canonicalize().unwrap();

        // Ensure the shader crate exists
        self.install.shader_crate = self.install.shader_crate.canonicalize().unwrap();
        assert!(
            self.install.shader_crate.exists(),
            "shader crate '{}' does not exist. (Current dir is '{}')",
            self.install.shader_crate.display(),
            std::env::current_dir().unwrap().display()
        );

        let spirv_builder_args = spirv_builder_cli::Args {
            dylib_path,
            shader_crate: self.install.shader_crate.clone(),
            shader_target: self.shader_target.clone(),
            path_to_target_spec: target_spec_dir().join(format!("{}.json", self.shader_target)),
            no_default_features: self.no_default_features,
            features: self.features.clone(),
            output_dir: self.output_dir.clone(),
        };

        // UNWRAP: safe because we know this always serializes
        let arg = serde_json::to_string_pretty(&spirv_builder_args).unwrap();
        log::info!("using spirv-builder-cli arg: {arg}");

        // Call spirv-builder-cli to compile the shaders.
        let output = std::process::Command::new(spirv_builder_cli_path)
            .arg(arg)
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .output()
            .unwrap();
        assert!(output.status.success(), "build failed");

        let spirv_manifest = self.output_dir.join("spirv-manifest.json");
        if spirv_manifest.is_file() {
            log::debug!(
                "successfully built shaders, raw manifest is at '{}'",
                spirv_manifest.display()
            );
        } else {
            log::error!("missing raw manifest '{}'", spirv_manifest.display());
            panic!("missing raw manifest");
        }

        let shaders: Vec<ShaderModule> =
            serde_json::from_reader(std::fs::File::open(&spirv_manifest).unwrap()).unwrap();

        let mut linkage: Vec<_> = shaders
            .into_iter()
            .map(
                |ShaderModule {
                     entry,
                     path: filepath,
                 }| {
                    use relative_path::PathExt as _;
                    let path = self.output_dir.join(filepath.file_name().unwrap());
                    std::fs::copy(&filepath, &path).unwrap();
                    let path_relative_to_shader_crate = path
                        .relative_to(&self.install.shader_crate)
                        .unwrap()
                        .to_path("");
                    Linkage::new(entry, path_relative_to_shader_crate)
                },
            )
            .collect();

        // Write the shader manifest json file
        let manifest_path = self.output_dir.join("manifest.json");
        // Sort the contents so the output is deterministic
        linkage.sort();
        // UNWRAP: safe because we know this always serializes
        let json = serde_json::to_string_pretty(&linkage).unwrap();
        let mut file = std::fs::File::create(&manifest_path).unwrap_or_else(|error| {
            log::error!(
                "could not create shader manifest file '{}': {error}",
                manifest_path.display(),
            );
            panic!("{error}")
        });
        file.write_all(json.as_bytes()).unwrap_or_else(|error| {
            log::error!(
                "could not write shader manifest file '{}': {error}",
                manifest_path.display(),
            );
            panic!("{error}")
        });

        log::info!("wrote manifest to '{}'", manifest_path.display());

        if spirv_manifest.is_file() {
            log::debug!(
                "removing spirv-manifest.json file '{}'",
                spirv_manifest.display()
            );
            std::fs::remove_file(spirv_manifest).unwrap();
        }
    }
}

#[cfg(test)]
mod test {
    use crate::{Cli, Command};

    use super::*;

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
            command: Command::Build(mut build),
        } = Cli::parse_from(args)
        {
            assert_eq!(shader_crate_path, build.install.shader_crate);
            assert_eq!(output_dir, build.output_dir);

            build.run();
        } else {
            panic!("was not a build command");
        }
    }
}
