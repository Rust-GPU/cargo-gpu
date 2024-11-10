//! Rust GPU shader crate builder.
//!
//! This program manages installations of `spirv-builder-cli` and `rustc_codegen_spirv`.
//! It uses these tools to compile Rust code into SPIR-V.
//!
//! # How it works
//!
//! In order to build shader crates, we must invoke cargo/rustc with a special backend
//! that performs the SPIR-V code generation. This backend is a dynamic library known
//! by its project name `rustc_codegen_spirv`. The name of the artifact itself is
//! OS-dependent.
//!
//! There are a lot of special flags to wrangle and so we use a command line program
//! that wraps `cargo` to perform the building of shader crates. This cli program is
//! called `spirv-builder-cli`, which itself is a cli wrapper around the `spirv-builder`
//! library.
//!
//! ## Where the binaries are
//!
//! `cargo-gpu` maintains different versions `spirv-builder-cli` and `rustc_codegen_spirv`
//! in a cache dir. The location is OS-dependent, for example on macOS it's in
//! `~/Library/Caches/rust-gpu`. Specific versions live inside the cache dir, prefixed
//! by their `spirv-builder` cargo dependency and rust toolchain pair.
//!
//! Building a specific "binary pair" of `spirv-builder-cli` and `rustc_codegen_spirv`
//! happens when there is no existing pair that matches the computed prefix, or if
//! a force rebuild is specified on the command line.
//!
//! ## Building the "binary pairs"
//!
//! The source of `spirv-builder-cli` lives alongside this source file, in crate that
//! is not included in the workspace. That same source code is also included statically
//! in **this** source file.
//!
//! When `spirv-builder-cli` needs to be built, a new directory is created in the cache
//! where the source to `spirv-builder-cli` is copied into, containing the specific cargo
//! dependency for `spirv-builder` and the matching rust toolchain channel.
//!
//! Then `cargo` is invoked in that cache directory to build the pair of artifacts, which
//! are then put into the top level of that cache directory.
//!
//! This pair of artifacts is then used to build shader crates.
//!
//! ## Building shader crates
//!
//! `cargo-gpu` takes a path to a shader crate to build, as well as a path to a directory
//! to put the compiled `spv` source files. It also takes a path to an output mainifest
//! file where all shader entry points will be mapped to their `spv` source files. This
//! manifest file can be used by build scripts (`build.rs` files) to generate linkage or
//! conduct other post-processing, like converting the `spv` files into `wgsl` files,
//! for example.
use std::io::Write;

use cargo_gpu::{spirv_builder_cli::ShaderModule, Linkage};
use clap::{Parser, Subcommand};

const SPIRV_BUILDER_CLI_CARGO_TOML: &str = include_str!("../../spirv-builder-cli/Cargo.toml");
const SPIRV_BUILDER_CLI_MAIN: &str = include_str!("../../spirv-builder-cli/src/main.rs");
const SPIRV_BUILDER_CLI_LIB: &str = include_str!("lib.rs");
const SPIRV_BUILDER_FILES: &[(&str, &str)] = &[
    ("Cargo.toml", SPIRV_BUILDER_CLI_CARGO_TOML),
    ("src/main.rs", SPIRV_BUILDER_CLI_MAIN),
    ("src/lib.rs", SPIRV_BUILDER_CLI_LIB),
];

const SPIRV_STD_TOOLCHAIN_PAIRS: &[(&str, &str)] = &[("0.10", "nightly-2024-04-24")];

/// Location of `cargo-gpu` source, which contains `crates/spirv-builder-cli`.
#[derive(Debug, Clone)]
struct SpirvCli {
    dep: String,
    channel: String,
}

impl core::fmt::Display for SpirvCli {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        format!("{}+{}", self.dep, self.channel).fmt(f)
    }
}

impl SpirvCli {
    /// Returns a string suitable to use as a directory.
    ///
    /// Created from the spirv-builder source dep and the rustc channel.
    fn to_dirname(&self) -> String {
        self.to_string()
            .replace([std::path::MAIN_SEPARATOR, '.', ':', '@', '='], "_")
            .split(['{', '}', ' ', '\n', '"', '\''])
            .collect::<Vec<_>>()
            .concat()
    }

    fn cached_checkout_path(&self) -> std::path::PathBuf {
        let checkout_dir = cache_dir().join(self.to_dirname());
        std::fs::create_dir_all(&checkout_dir).unwrap_or_else(|e| {
            log::error!(
                "could not create checkout dir '{}': {e}",
                checkout_dir.display()
            );
            panic!("could not create checkout dir");
        });

        checkout_dir
    }

    fn ensure_version_channel_compatibility(&self) {
        for (version, channel) in SPIRV_STD_TOOLCHAIN_PAIRS.iter() {
            if version.starts_with(&self.dep) && channel != &self.channel {
                panic!("expected spirv-std version to be matched with rust toolchain channel {channel}");
            }
        }
    }
}

#[derive(Parser)]
struct Install {
    /// spirv-builder dependency, written just like in a Cargo.toml file.
    #[clap(
        long,
        default_value = r#"{ git = "https://github.com/Rust-GPU/rust-gpu.git" }"#
    )]
    spirv_builder: String,

    /// Rust toolchain channel to use to build `spirv-builder`.
    ///
    /// This must match the `spirv_builder` argument.
    #[clap(long, default_value = "nightly-2024-04-24")]
    rust_toolchain: String,

    /// Force `spirv-builder-cli` and `rustc_codegen_spirv` to be rebuilt.
    #[clap(long)]
    force_spirv_cli_rebuild: bool,
}

impl Install {
    fn spirv_cli(&self) -> SpirvCli {
        SpirvCli {
            dep: self.spirv_builder.clone(),
            channel: self.rust_toolchain.clone(),
        }
    }

    fn write_source_files(&self) {
        let cli = self.spirv_cli();
        let checkout = cli.cached_checkout_path();
        std::fs::create_dir_all(checkout.join("src")).unwrap();
        for (filename, contents) in SPIRV_BUILDER_FILES.iter() {
            log::debug!("writing {filename}");
            let path = checkout.join(filename);
            let mut file = std::fs::File::create(&path).unwrap();
            let replaced_contents = contents
                .replace("${SPIRV_BUILDER_SOURCE}", &cli.dep)
                .replace("${CHANNEL}", &cli.channel);
            file.write_all(replaced_contents.as_bytes()).unwrap();
        }
    }

    // Install the binary pair and return the paths, (dylib, cli).
    fn run(&self) -> (std::path::PathBuf, std::path::PathBuf) {
        // Ensure the cache dir exists
        let cache_dir = cache_dir();
        std::fs::create_dir_all(&cache_dir).unwrap_or_else(|e| {
            log::error!(
                "could not create cache directory '{}': {e}",
                cache_dir.display()
            );
            panic!("could not create cache dir");
        });

        let spirv_version = self.spirv_cli();
        spirv_version.ensure_version_channel_compatibility();

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

            log::debug!("building artifacts");
            let output = std::process::Command::new("cargo")
                .current_dir(&checkout)
                .arg(format!("+{}", spirv_version.channel))
                .args(["build", "--release"])
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

            let cli_path = release.join("spirv-builder-cli");
            if cli_path.is_file() {
                log::info!("successfully built {}", cli_path.display());
                std::fs::rename(&cli_path, &dest_cli_path).unwrap();
            } else {
                log::error!("could not find {}", cli_path.display());
                panic!("spirv-builder-cli build failed");
            }
        }
        (dest_dylib_path, dest_cli_path)
    }
}

#[derive(Parser)]
struct Build {
    #[clap(flatten)]
    install: Install,

    /// Directory containing the shader crate to compile.
    #[clap(long, default_value = "./")]
    shader_crate: std::path::PathBuf,

    /// Shader target.
    #[clap(long, default_value = "spirv-unknown-vulkan1.2")]
    shader_target: String,

    /// Path to the output JSON manifest file where the paths to .spv files
    /// and the names of their entry points will be saved.
    #[clap(long)]
    shader_manifest: Option<std::path::PathBuf>,

    /// Set cargo default-features.
    #[clap(long)]
    no_default_features: bool,

    /// Set cargo features.
    #[clap(long)]
    features: Vec<String>,

    /// Path to the output directory for the compiled shaders.
    #[clap(long, short, default_value = "./")]
    output_dir: std::path::PathBuf,

    /// If set the shaders will be compiled but not put into place.
    #[clap(long, short)]
    dry_run: bool,
}

impl Build {
    fn run(&self) {
        let (dylib_path, spirv_builder_cli_path) = self.install.run();

        // Ensure the shader output dir exists
        std::fs::create_dir_all(&self.output_dir).unwrap();

        assert!(
            self.shader_crate.exists(),
            "shader crate '{}' does not exist. (Current dir is '{}')",
            self.shader_crate.display(),
            std::env::current_dir().unwrap().display()
        );

        let spirv_builder_args = cargo_gpu::spirv_builder_cli::Args {
            dylib_path,
            shader_crate: self.shader_crate.clone(),
            shader_target: self.shader_target.clone(),
            no_default_features: self.no_default_features,
            features: self.features.clone(),
            output_dir: self.output_dir.clone(),
            dry_run: self.dry_run,
        };

        // UNWRAP: safe because we know this always serializes
        let arg = serde_json::to_string_pretty(&spirv_builder_args).unwrap();
        log::info!("using spirv-builder-cli arg: {arg}");

        // Call spirv-builder-cli to compile the shaders.
        let output = std::process::Command::new(&spirv_builder_cli_path)
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
                    let path = self.output_dir.join(filepath.file_name().unwrap());
                    if !self.dry_run {
                        std::fs::copy(filepath, &path).unwrap();
                    }
                    Linkage::new(entry, path)
                },
            )
            .collect();

        // Write the shader manifest json file
        if !self.dry_run {
            if let Some(manifest_path) = &self.shader_manifest {
                // Sort the contents so the output is deterministic
                linkage.sort();
                // UNWRAP: safe because we know this always serializes
                let json = serde_json::to_string_pretty(&linkage).unwrap();
                let mut file = std::fs::File::create(manifest_path).unwrap_or_else(|e| {
                    log::error!(
                        "could not create shader manifest file '{}': {e}",
                        manifest_path.display(),
                    );
                    panic!("{e}")
                });
                file.write_all(json.as_bytes()).unwrap_or_else(|e| {
                    log::error!(
                        "could not write shader manifest file '{}': {e}",
                        manifest_path.display(),
                    );
                    panic!("{e}")
                });

                log::info!("wrote manifest to '{}'", manifest_path.display());
            }
        }
    }
}

#[derive(Subcommand)]
enum Command {
    /// Install rust-gpu compiler artifacts.
    Install(Install),

    /// Compile a shader crate to SPIR-V.
    Build(Build),
}

#[derive(Parser)]
#[clap(author, version, about, subcommand_required = true)]
struct Cli {
    /// The command to run.
    #[clap(subcommand)]
    command: Command,
}

fn cache_dir() -> std::path::PathBuf {
    directories::BaseDirs::new()
        .unwrap_or_else(|| {
            log::error!("could not find the user home directory");
            panic!("cache_dir failed");
        })
        .cache_dir()
        .join("rust-gpu")
}

fn main() {
    env_logger::builder().init();

    let cli = Cli::parse_from(std::env::args().filter(|p| {
        // Calling cargo-gpu as the cargo subcommand "cargo gpu" passes "gpu"
        // as the first parameter, which we want to ignore.
        p != "gpu"
    }));

    match cli.command {
        Command::Install(install) => {
            let _ = install.run();
        }
        Command::Build(build) => build.run(),
    }
}