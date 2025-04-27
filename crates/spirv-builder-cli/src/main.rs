/// NB: For developing this file it will probably help to temporarily move the `"crates/spirv-builder-cli"`
/// line from the `exclude` to `members` section of the root `Cargo.toml` file. This will allow
/// `rust-analyzer` to run on the file. We can't permanently keep it there because each of the
/// `spirv-builder-*` features depends on a different Rust toolchain which `cargo check/clippy`
/// can't build all at once.
pub mod args;

#[cfg(feature = "spirv-builder-pre-cli")]
use spirv_builder_pre_cli as spirv_builder;

#[cfg(feature = "spirv-builder-0_10")]
use spirv_builder_0_10 as spirv_builder;

use spirv_builder::{CompileResult, MetadataPrintout, ModuleResult, SpirvBuilder};
use spirv_builder_cli::ShaderModule;

const RUSTC_NIGHTLY_CHANNEL: &str = "${CHANNEL}";

fn set_rustup_toolchain() {
    let toolchain = RUSTC_NIGHTLY_CHANNEL.trim_matches('"');

    log::trace!("setting RUSTUP_TOOLCHAIN = '{}'", toolchain);
    std::env::set_var("RUSTUP_TOOLCHAIN", toolchain);
    std::env::remove_var("RUSTC");
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
    let existing_paths_str = std::env::var(env_var).unwrap();
    let mut dylib_paths = std::env::split_paths(&existing_paths_str).collect::<Vec<_>>();

    let dylib_path = dylib_path.parent().unwrap().to_path_buf();
    dylib_paths.insert(0, dylib_path);

    let path = std::env::join_paths(dylib_paths)
        .unwrap()
        .into_string()
        .unwrap();

    log::debug!("Setting OS-dependent DLL ENV path ({env_var}) to: {path}");
    std::env::set_var(env_var, path);
}

fn handle_compile_result(result: &CompileResult, args: &args::AllArgs) {
    log::debug!("found entry points: {:#?}", result.entry_points);

    let dir = &args.build.output_dir;
    let mut shaders = vec![];
    match &result.module {
        ModuleResult::MultiModule(modules) => {
            assert!(!modules.is_empty(), "No shader modules to compile");
            for (entry, filepath) in modules.clone().into_iter() {
                log::debug!("compiled {entry} {}", filepath.display());
                shaders.push(ShaderModule::new(entry, filepath));
            }
        }
        ModuleResult::SingleModule(filepath) => {
            for entry in result.entry_points.clone() {
                shaders.push(ShaderModule::new(entry, filepath.clone()));
            }
        }
    }

    use std::io::Write;
    let mut file = std::fs::File::create(dir.join("spirv-manifest.json")).unwrap();
    file.write_all(&serde_json::to_vec(&shaders).unwrap())
        .unwrap();
}

pub fn main() {
    env_logger::builder().init();

    set_rustup_toolchain();

    let args = std::env::args().collect::<Vec<_>>();
    log::debug!(
        "running spirv-builder-cli from '{}'",
        std::env::current_dir().unwrap().display()
    );
    log::debug!("with args: {args:#?}");
    let args: args::AllArgs = serde_json::from_str(&args[1]).unwrap();
    let args_for_result = args.clone();

    let spirv_metadata = match args.build.spirv_metadata {
        args::SpirvMetadata::None => spirv_builder::SpirvMetadata::None,
        args::SpirvMetadata::NameVariables => spirv_builder::SpirvMetadata::NameVariables,
        args::SpirvMetadata::Full => spirv_builder::SpirvMetadata::Full,
    };

    let mut builder = SpirvBuilder::new(args.install.shader_crate, &args.build.target)
        .deny_warnings(args.build.deny_warnings)
        .release(!args.build.debug)
        .multimodule(args.build.multimodule)
        .spirv_metadata(spirv_metadata)
        .relax_struct_store(args.build.relax_struct_store)
        .relax_logical_pointer(args.build.relax_logical_pointer)
        .relax_block_layout(args.build.relax_block_layout)
        .uniform_buffer_standard_layout(args.build.uniform_buffer_standard_layout)
        .scalar_block_layout(args.build.scalar_block_layout)
        .skip_block_layout(args.build.skip_block_layout)
        .preserve_bindings(args.build.preserve_bindings)
        .print_metadata(spirv_builder::MetadataPrintout::None);

    for capability in &args.build.capability {
        builder = builder.capability(*capability);
    }

    for extension in &args.build.extension {
        builder = builder.extension(extension);
    }

    #[cfg(feature = "spirv-builder-pre-cli")]
    {
        log::debug!("using spirv-builder-pre-cli");
        set_codegen_spirv_location(args.install.dylib_path);
    }

    #[cfg(feature = "spirv-builder-0_10")]
    {
        log::debug!("using spirv-builder-0_10");
        builder = builder
            .rustc_codegen_spirv_location(args.install.dylib_path)
            .target_spec(args.build.shader_target);

        if args.build.no_default_features {
            log::info!("setting cargo --no-default-features");
            builder = builder.shader_crate_default_features(false);
        }
        if !args.build.features.is_empty() {
            log::info!("setting --features {:?}", args.build.features);
            builder = builder.shader_crate_features(args.build.features);
        }
    }

    log::debug!("Calling `rust-gpu`'s `spirv-builder` library");

    if args.build.watch {
        println!("🦀 Watching and recompiling shader on changes...");
        builder.watch(move |compile_result| {
            handle_compile_result(&compile_result, &args_for_result);
        });
        std::thread::park();
    } else {
        let result = builder.build().unwrap();
        handle_compile_result(&result, &args_for_result);
    }
}
