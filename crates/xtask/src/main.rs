//! Project/repository utilities.
#![allow(
    clippy::shadow_reuse,
    clippy::unwrap_used,
    clippy::unwrap_in_result,
    reason = "This is just a workflow tool"
)]

use anyhow::Context as _;
use clap::Parser as _;

/// Path to the shader crate
const SHADER_CRATE_PATH: &str = "crates/shader-crate-template";

/// Our xtask commands.
#[derive(Debug, clap::Parser)]
enum Cli {
    /// Run a test build of the shader-crate-template project.
    TestBuild {
        /// Build using the specified version of `spirv-std`.
        #[clap(long)]
        rust_gpu_version: Option<String>,
    },
}

fn cmd(args: impl IntoIterator<Item = impl AsRef<str>>) -> anyhow::Result<()> {
    let mut args = args.into_iter();
    let mut cmd = std::process::Command::new(args.next().context("no args")?.as_ref());
    for arg in args {
        cmd.arg(arg.as_ref());
    }

    let output = cmd
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .output()
        .context("cmd failed")?;
    anyhow::ensure!(output.status.success());

    Ok(())
}

/// Overwrites a toml file's output-dir field, and reverts that on drop.
struct ShaderCrateTemplateCargoTomlWriter {
    /// Original string
    original_shader_crate_template_str: String,
    /// Original lockfile
    original_shader_crate_lock_file: String,
    /// Parsed toml table
    table: toml::Table,
}

impl Drop for ShaderCrateTemplateCargoTomlWriter {
    fn drop(&mut self) {
        log::info!("reverting overwrite of Cargo.toml");
        std::fs::write(
            format!("{SHADER_CRATE_PATH}/Cargo.toml"),
            &self.original_shader_crate_template_str,
        )
        .unwrap();
        log::info!("reverting overwrite of Cargo.lock");
        std::fs::write(
            format!("{SHADER_CRATE_PATH}/Cargo.lock"),
            &self.original_shader_crate_lock_file,
        )
        .unwrap();
    }
}

impl ShaderCrateTemplateCargoTomlWriter {
    /// Create a new one
    fn new() -> Self {
        let original_shader_crate_template_str =
            std::fs::read_to_string(format!("{SHADER_CRATE_PATH}/Cargo.toml")).unwrap();
        let table = toml::from_str::<toml::Table>(&original_shader_crate_template_str).unwrap();
        let original_shader_crate_lock_file =
            std::fs::read_to_string(format!("{SHADER_CRATE_PATH}/Cargo.lock")).unwrap();
        Self {
            original_shader_crate_template_str,
            original_shader_crate_lock_file,
            table,
        }
    }

    /// Get the `[dependencies]` section of the shader's `Cargo.toml`.
    fn get_cargo_dependencies_table(&mut self) -> &mut toml::Table {
        self.table
            .get_mut("dependencies")
            .unwrap()
            .as_table_mut()
            .unwrap()
    }

    /// Get the `[package.metadata.rust-gpu.build]` section of the shader's `Cargo.toml`.
    fn get_rust_gpu_table(&mut self) -> &mut toml::Table {
        let package = self
            .table
            .get_mut("package")
            .unwrap()
            .as_table_mut()
            .unwrap();
        let metadata = package.get_mut("metadata").unwrap().as_table_mut().unwrap();
        metadata
            .get_mut("rust-gpu")
            .unwrap()
            .as_table_mut()
            .unwrap()
    }

    /// Write any temporary changes to the shader crate's `Cargo.toml` that are needed to run e2e
    /// tests.
    fn write_shader_crate_cargo_toml_changes(&self) -> anyhow::Result<()> {
        std::fs::write(
            format!("{SHADER_CRATE_PATH}/Cargo.toml"),
            toml::to_string_pretty(&self.table).context("could not serialize")?,
        )
        .context("could not overwrite path")?;
        Ok(())
    }

    /// Replace the output-dir
    fn replace_output_dir(&mut self, path: impl AsRef<std::path::Path>) -> anyhow::Result<()> {
        let rust_gpu = self.get_rust_gpu_table();
        let build = rust_gpu.get_mut("build").unwrap().as_table_mut().unwrap();
        let output_dir = build.get_mut("output-dir").unwrap();
        *output_dir = toml::Value::String(format!("{}", path.as_ref().display()));
        self.write_shader_crate_cargo_toml_changes()?;
        Ok(())
    }

    /// Replace the `spirv-std` dependency version
    fn replace_spirv_std_version(&mut self, version: String) -> anyhow::Result<()> {
        let dependencies = self.get_cargo_dependencies_table();
        let spirv_std = dependencies.get_mut("spirv-std").unwrap();
        *spirv_std = toml::Value::String(version);
        self.write_shader_crate_cargo_toml_changes()?;
        Ok(())
    }
}

/// Run the xtask.
fn main() {
    env_logger::builder().init();

    let cli = Cli::parse();

    match cli {
        Cli::TestBuild {
            rust_gpu_version: maybe_rust_gpu_version,
        } => {
            log::info!("installing cargo gpu");
            cmd(["cargo", "install", "--path", "crates/cargo-gpu"]).unwrap();

            log::info!("installing cargo gpu artifacts");
            cmd([
                "cargo",
                "gpu",
                "install",
                "--shader-crate",
                SHADER_CRATE_PATH,
                "--auto-install-rust-toolchain",
                "--force-overwrite-lockfiles-v4-to-v3",
            ])
            .unwrap();

            let dir = tempdir::TempDir::new("test-shader-output").unwrap();
            let mut overwriter = ShaderCrateTemplateCargoTomlWriter::new();
            overwriter.replace_output_dir(dir.path()).unwrap();

            if let Some(rust_gpu_version) = maybe_rust_gpu_version {
                if rust_gpu_version != "latest" {
                    overwriter
                        .replace_spirv_std_version(rust_gpu_version)
                        .unwrap();
                }
            }

            cmd([
                "cargo",
                "gpu",
                "build",
                "--shader-crate",
                SHADER_CRATE_PATH,
                "--auto-install-rust-toolchain",
                "--force-spirv-cli-rebuild",
                "--force-overwrite-lockfiles-v4-to-v3",
            ])
            .unwrap();

            cmd(["ls", "-lah", dir.path().to_str().unwrap()]).unwrap();
            //NOTE: manifest.json is the default value here, which should be valid
            cmd(["cat", dir.path().join("manifest.json").to_str().unwrap()]).unwrap();
        }
    }
}
