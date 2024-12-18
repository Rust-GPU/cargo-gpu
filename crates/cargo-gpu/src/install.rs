//! Install a dedicated per-shader crate that has the `rust-gpu` compiler in it.
use std::io::Write as _;

use crate::{cache_dir, spirv_cli::SpirvCli, spirv_source::SpirvSource, target_spec_dir};

/// These are the files needed to create the dedicated, per-shader `rust-gpu` builder create.
const SPIRV_BUILDER_FILES: &[(&str, &str)] = &[
    (
        "Cargo.toml",
        include_str!("../../spirv-builder-cli/Cargo.toml"),
    ),
    (
        "src/main.rs",
        include_str!("../../spirv-builder-cli/src/main.rs"),
    ),
    (
        "src/lib.rs",
        include_str!("../../spirv-builder-cli/src/lib.rs"),
    ),
];

/// Metadata for the compile targets supported by `rust-gpu`
const TARGET_SPECS: &[(&str, &str)] = &[
    (
        "spirv-unknown-opengl4.0.json",
        include_str!("../target-specs/spirv-unknown-opengl4.0.json"),
    ),
    (
        "spirv-unknown-opengl4.1.json",
        include_str!("../target-specs/spirv-unknown-opengl4.1.json"),
    ),
    (
        "spirv-unknown-opengl4.2.json",
        include_str!("../target-specs/spirv-unknown-opengl4.2.json"),
    ),
    (
        "spirv-unknown-opengl4.3.json",
        include_str!("../target-specs/spirv-unknown-opengl4.3.json"),
    ),
    (
        "spirv-unknown-opengl4.5.json",
        include_str!("../target-specs/spirv-unknown-opengl4.5.json"),
    ),
    (
        "spirv-unknown-spv1.0.json",
        include_str!("../target-specs/spirv-unknown-spv1.0.json"),
    ),
    (
        "spirv-unknown-spv1.1.json",
        include_str!("../target-specs/spirv-unknown-spv1.1.json"),
    ),
    (
        "spirv-unknown-spv1.2.json",
        include_str!("../target-specs/spirv-unknown-spv1.2.json"),
    ),
    (
        "spirv-unknown-spv1.3.json",
        include_str!("../target-specs/spirv-unknown-spv1.3.json"),
    ),
    (
        "spirv-unknown-spv1.4.json",
        include_str!("../target-specs/spirv-unknown-spv1.4.json"),
    ),
    (
        "spirv-unknown-spv1.5.json",
        include_str!("../target-specs/spirv-unknown-spv1.5.json"),
    ),
    (
        "spirv-unknown-vulkan1.0.json",
        include_str!("../target-specs/spirv-unknown-vulkan1.0.json"),
    ),
    (
        "spirv-unknown-vulkan1.1.json",
        include_str!("../target-specs/spirv-unknown-vulkan1.1.json"),
    ),
    (
        "spirv-unknown-vulkan1.1spv1.4.json",
        include_str!("../target-specs/spirv-unknown-vulkan1.1spv1.4.json"),
    ),
    (
        "spirv-unknown-vulkan1.2.json",
        include_str!("../target-specs/spirv-unknown-vulkan1.2.json"),
    ),
];

/// `cargo gpu install`
#[derive(clap::Parser, Debug)]
pub struct Install {
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
    spirv_builder_source: Option<String>,

    /// Version of `spirv-builder` dependency.
    /// * If `--spirv-builder-source` is not set, then this is assumed to be a crates.io semantic
    ///   version such as "0.9.0".
    /// * If `--spirv-builder-source` is set, then this is assumed to be a Git "commitsh", such
    ///   as a Git commit hash or a Git tag, therefore anything that `git checkout` can resolve.
    #[clap(long, verbatim_doc_comment)]
    spirv_builder_version: Option<String>,

    /// Rust toolchain channel to use to build `spirv-builder`.
    ///
    /// This must be compatible with the `spirv_builder` argument as defined in the `rust-gpu` repo.
    #[clap(long)]
    rust_toolchain: Option<String>,

    /// Force `spirv-builder-cli` and `rustc_codegen_spirv` to be rebuilt.
    #[clap(long)]
    force_spirv_cli_rebuild: bool,
}

impl Install {
    /// Returns a [`SpirvCLI`] instance, responsible for ensuring the right version of the `spirv-builder-cli` crate.
    fn spirv_cli(&self, shader_crate_path: &std::path::PathBuf) -> SpirvCli {
        SpirvCli::new(
            shader_crate_path,
            self.spirv_builder_source.clone(),
            self.spirv_builder_version.clone(),
            self.rust_toolchain.clone(),
        )
    }

    /// Create the `spirv-builder-cli` crate.
    fn write_source_files(&self) {
        let spirv_cli = self.spirv_cli(&self.shader_crate);
        let checkout = spirv_cli.cached_checkout_path();
        std::fs::create_dir_all(checkout.join("src")).unwrap();
        for (filename, contents) in SPIRV_BUILDER_FILES {
            log::debug!("writing {filename}");
            let path = checkout.join(filename);
            let mut file = std::fs::File::create(&path).unwrap();
            let mut replaced_contents = contents.replace("${CHANNEL}", &spirv_cli.channel);
            if filename == &"Cargo.toml" {
                replaced_contents = Self::update_cargo_toml(&replaced_contents, &spirv_cli.source);
            }
            file.write_all(replaced_contents.as_bytes()).unwrap();
        }
    }

    /// Create the `spirv-builder-cli` crate.
    fn update_cargo_toml(contents: &str, spirv_source: &SpirvSource) -> String {
        let updated = contents.lines().map(|line| {
            if line.contains("${AUTO-REPLACE-SOURCE}") {
                let replaced_line = match spirv_source {
                    SpirvSource::CratesIO(_) => String::new(),
                    SpirvSource::Git((repo, _)) => format!("git = \"{repo}\""),
                    SpirvSource::Path((path, _)) => format!("path = \"{path}\""),
                };
                return format!("{replaced_line}\n");
            }

            if line.contains("${AUTO-REPLACE-VERSION}") {
                let replaced_line = match spirv_source {
                    SpirvSource::CratesIO(version) | SpirvSource::Path((_, version)) => {
                        format!("version = \"{}\"", version.replace('v', ""))
                    }
                    SpirvSource::Git((_, revision)) => format!("rev = \"{revision}\""),
                };
                return format!("{replaced_line}\n");
            }

            format!("{line}\n")
        });

        updated.collect()
    }

    /// Add the target spec files to the crate.
    fn write_target_spec_files(&self) {
        for (filename, contents) in TARGET_SPECS {
            let path = target_spec_dir().join(filename);
            if !path.is_file() || self.force_spirv_cli_rebuild {
                let mut file = std::fs::File::create(&path).unwrap();
                file.write_all(contents.as_bytes()).unwrap();
            }
        }
    }

    /// Install the binary pair and return the paths, (dylib, cli).
    pub fn run(&self) -> (std::path::PathBuf, std::path::PathBuf) {
        // Ensure the cache dir exists
        let cache_dir = cache_dir();
        log::info!("cache directory is '{}'", cache_dir.display());
        std::fs::create_dir_all(&cache_dir).unwrap_or_else(|error| {
            log::error!(
                "could not create cache directory '{}': {error}",
                cache_dir.display()
            );
            panic!("could not create cache dir");
        });

        let spirv_version = self.spirv_cli(&self.shader_crate);
        spirv_version.ensure_toolchain_and_components_exist();

        let checkout = spirv_version.cached_checkout_path();
        let release = checkout.join("target").join("release");

        let dylib_filename = format!(
            "{}rustc_codegen_spirv{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_SUFFIX
        );
        let dylib_path = release.join(&dylib_filename);
        let dest_dylib_path = checkout.join(&dylib_filename);
        let dest_cli_path = checkout.join("spirv-builder-cli");
        if dest_dylib_path.is_file() && dest_cli_path.is_file() {
            log::info!(
                "cargo-gpu artifacts are already installed in '{}'",
                checkout.display()
            );
        }

        if dest_dylib_path.is_file() && dest_cli_path.is_file() && !self.force_spirv_cli_rebuild {
            log::info!("...and so we are aborting the install step.");
        } else {
            log::debug!(
                "writing spirv-builder-cli source files into '{}'",
                checkout.display()
            );
            self.write_source_files();
            self.write_target_spec_files();

            let mut command = std::process::Command::new("cargo");
            command
                .current_dir(&checkout)
                .arg(format!("+{}", spirv_version.channel))
                .args(["build", "--release"])
                .args(["--no-default-features"]);

            command.args([
                "--features",
                &Self::get_required_spirv_builder_version(spirv_version.date),
            ]);

            log::debug!("building artifacts with `{:?}`", command);

            let output = command
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .output()
                .unwrap();
            assert!(output.status.success(), "...build error!");

            if dylib_path.is_file() {
                log::info!("successfully built {}", dylib_path.display());
                std::fs::rename(&dylib_path, &dest_dylib_path).unwrap();
            } else {
                log::error!("could not find {}", dylib_path.display());
                panic!("spirv-builder-cli build failed");
            }

            let cli_path = if cfg!(target_os = "windows") {
                release.join("spirv-builder-cli").with_extension("exe")
            } else {
                release.join("spirv-builder-cli")
            };
            if cli_path.is_file() {
                log::info!("successfully built {}", cli_path.display());
                std::fs::rename(&cli_path, &dest_cli_path).unwrap();
            } else {
                log::error!("could not find {}", cli_path.display());
                log::debug!("contents of '{}':", release.display());
                for maybe_entry in std::fs::read_dir(&release).unwrap() {
                    let entry = maybe_entry.unwrap();
                    log::debug!("{}", entry.file_name().to_string_lossy());
                }
                panic!("spirv-builder-cli build failed");
            }
        }
        (dest_dylib_path, dest_cli_path)
    }

    /// The `spirv-builder` crate from the main `rust-gpu` repo hasn't always been setup to
    /// interact with `cargo-gpu`. Older versions don't have the same `SpirvBuilder` interface. So
    /// here we choose the right Cargo feature to enable/disable code in `spirv-builder-cli`.
    ///
    /// TODO:
    ///   * Warn the user that certain `cargo-gpu` features aren't available when building with
    ///     older versions of `spirv-builder`, eg setting the target spec.
    fn get_required_spirv_builder_version(date: chrono::NaiveDate) -> String {
        let parse_date = chrono::NaiveDate::parse_from_str;
        let pre_cli_date = parse_date("2024-04-24", "%Y-%m-%d").unwrap();

        if date < pre_cli_date {
            "spirv-builder-pre-cli"
        } else {
            "spirv-builder-0_10"
        }
        .into()
    }
}
