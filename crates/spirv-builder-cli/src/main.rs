//! This program builds rust-gpu shader crates and writes generated spv files
//! into the main source repo.

#[cfg(feature = "spirv-builder-pre-cli")]
use spirv_builder_pre_cli as spirv_builder;

#[cfg(feature = "spirv-builder-0_10")]
use spirv_builder_0_10 as spirv_builder;

use spirv_builder::{CompileResult, MetadataPrintout, ModuleResult, SpirvBuilder};

use spirv_builder_cli::{Args, ShaderModule};

const RUSTC_NIGHTLY_CHANNEL: &str = "${CHANNEL}";

fn set_rustup_toolchain() {
    log::trace!(
        "setting RUSTUP_TOOLCHAIN = '{}'",
        RUSTC_NIGHTLY_CHANNEL.trim_matches('"')
    );
    std::env::set_var("RUSTUP_TOOLCHAIN", RUSTC_NIGHTLY_CHANNEL.trim_matches('"'));
}

/// Get the OS-dependent ENV variable name for the list of paths pointing to .so/.dll files
const fn dylib_path_envvar() -> &'static str {
    if cfg!(windows) {
        "PATH"
    } else if cfg!(target_os = "macos") {
        "DYLD_FALLBACK_LIBRARY_PATH"
    } else {
        "LD_LIBRARY_PATH"
    }
}

fn set_codegen_spirv_location(dylib_path: std::path::PathBuf) {
    let env_var = dylib_path_envvar();
    let path = dylib_path.parent().unwrap().display().to_string();
    log::debug!("Setting OS-dependent DLL ENV path ({env_var}) to: {path}");
    std::env::set_var(env_var, path);
}

fn main() {
    env_logger::builder().init();

    set_rustup_toolchain();

    let args = std::env::args().collect::<Vec<_>>();
    log::debug!(
        "running spirv-builder-cli from '{}'",
        std::env::current_dir().unwrap().display()
    );
    let args = serde_json::from_str(&args[1]).unwrap();
    log::debug!("compiling with args: {args:#?}");
    let Args {
        dylib_path,
        shader_crate,
        shader_target,
        path_to_target_spec,
        no_default_features,
        features,
        output_dir,
    } = args;

    let CompileResult {
        entry_points,
        module,
    } = {
        let mut builder = SpirvBuilder::new(shader_crate, &shader_target)
            .print_metadata(MetadataPrintout::None)
            .multimodule(true);

        #[cfg(feature = "spirv-builder-pre-cli")]
        {
            set_codegen_spirv_location(dylib_path);
        }

        #[cfg(feature = "spirv-builder-0_10")]
        {
            builder = builder
                .rustc_codegen_spirv_location(dylib_path)
                .target_spec(path_to_target_spec);

            if no_default_features {
                log::info!("setting cargo --no-default-features");
                builder = builder.shader_crate_default_features(false);
            }
            if !features.is_empty() {
                log::info!("setting --features {features:?}");
                builder = builder.shader_crate_features(features);
            }
        }

        log::debug!("Calling `rust-gpu`'s `spirv-builder` library");
        builder.build().unwrap()
    };

    let dir = output_dir;
    let mut shaders = vec![];
    match module {
        ModuleResult::MultiModule(modules) => {
            assert!(!modules.is_empty(), "No shader modules to compile");
            for (entry, filepath) in modules.into_iter() {
                shaders.push(ShaderModule::new(entry, filepath));
            }
        }
        ModuleResult::SingleModule(filepath) => {
            for entry in entry_points {
                shaders.push(ShaderModule::new(entry, filepath.clone()));
            }
        }
    }

    use std::io::Write;
    let mut file = std::fs::File::create(dir.join("spirv-manifest.json")).unwrap();
    file.write_all(&serde_json::to_vec(&shaders).unwrap())
        .unwrap();
}
