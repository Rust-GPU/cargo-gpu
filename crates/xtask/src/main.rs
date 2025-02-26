//! Project/repository utilities.
#![allow(clippy::shadow_reuse, reason = "sometimes its nice")]
#![allow(clippy::unwrap_used, reason = "sometimes its good")]
#![allow(clippy::unwrap_in_result, reason = "sometimes that's what you want")]

use anyhow::Context as _;
use clap::Parser as _;

/// Our xtask commands.
#[derive(Debug, clap::Parser)]
enum Cli {
    /// Run a test build of the shader-crate-template project.
    TestBuild,
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
    /// Parsed toml table
    table: toml::Table,
}

impl Drop for ShaderCrateTemplateCargoTomlWriter {
    fn drop(&mut self) {
        log::info!("reverting overwrite of Cargo.toml");
        std::fs::write(Self::PATH, &self.original_shader_crate_template_str).unwrap();
    }
}

impl ShaderCrateTemplateCargoTomlWriter {
    /// Path to the Cargo.toml
    const PATH: &str = "crates/shader-crate-template/Cargo.toml";

    /// Create a new one
    fn new() -> Self {
        let original_shader_crate_template_str = std::fs::read_to_string(Self::PATH).unwrap();
        let table = toml::from_str::<toml::Table>(&original_shader_crate_template_str).unwrap();
        Self {
            original_shader_crate_template_str,
            table,
        }
    }

    /// Replace the output-dir
    fn replace_output_dir(&mut self, path: impl AsRef<std::path::Path>) -> anyhow::Result<()> {
        let package = self
            .table
            .get_mut("package")
            .unwrap()
            .as_table_mut()
            .unwrap();
        let metadata = package.get_mut("metadata").unwrap().as_table_mut().unwrap();
        let rust_gpu = metadata
            .get_mut("rust-gpu")
            .unwrap()
            .as_table_mut()
            .unwrap();
        let build = rust_gpu.get_mut("build").unwrap().as_table_mut().unwrap();
        let output_dir = build.get_mut("output-dir").unwrap();
        *output_dir = toml::Value::String(format!("{}", path.as_ref().display()));
        std::fs::write(
            Self::PATH,
            toml::to_string_pretty(&self.table).context("could not serialize")?,
        )
        .context("could not overwrite path")?;
        Ok(())
    }
}

/// Run the xtask.
fn main() {
    env_logger::builder().init();

    let cli = Cli::parse();

    match cli {
        Cli::TestBuild => {
            log::info!("installing cargo gpu");
            cmd(["cargo", "install", "--path", "crates/cargo-gpu"]).unwrap();

            log::info!("installing cargo gpu artifacts");
            cmd([
                "cargo",
                "gpu",
                "install",
                "--shader-crate",
                "crates/shader-crate-template",
                "--auto-install-rust-toolchain",
            ])
            .unwrap();

            let dir = tempdir::TempDir::new("test-shader-output").unwrap();
            let mut overwriter = ShaderCrateTemplateCargoTomlWriter::new();
            overwriter.replace_output_dir(dir.path()).unwrap();

            cmd([
                "cargo",
                "gpu",
                "build",
                "--shader-crate",
                "crates/shader-crate-template",
                "--force-spirv-cli-rebuild",
            ])
            .unwrap();

            cmd(["ls", "-lah", dir.path().to_str().unwrap()]).unwrap();
            //NOTE: manifest.json is the default value here, which should be valid
            cmd(["cat", dir.path().join("manifest.json").to_str().unwrap()]).unwrap();
        }
    }
}
