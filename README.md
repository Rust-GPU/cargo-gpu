# cargo-gpu

Command line tool for building Rust shaders using rust-gpu.

## Getting Started

### Installation

To install the tool ensure you have `rustup`. Then run:

```
cargo install --git https://github.com/rust-gpu/cargo-gpu cargo-gpu
```

After that you can use `cargo gpu` to compile your shader crates with:

```
cargo gpu build
```

This plain invocation will compile the crate in the current directory and
place the compiled shaders in the current directory.

Use `cargo gpu help` to see other options :)

### Next Steps

You can try this out using the example repo at <https://github.com/rust-GPU/cargo-gpu/crates/shader-crate-template>.
Keep in mind <https://github.com/rust-GPU/cargo-gpu/crates/shader-crate-template> is _not_ yet a cargo generate template,
it's just a normal repo.

```
git clone https://github.com/rust-GPU/cargo-gpu
cd cargo-gpu/crates/shader-crate-template
cargo gpu build
```

## How it works

Behind the scenes `cargo gpu` compiles a custom [codegen backend](https://doc.rust-lang.org/beta/unstable-book/compiler-flags/codegen-backend.html)
for `rustc` that allows emitting [SPIR-V](https://www.khronos.org/spir/) assembly, instead of the conventional LLVM assembly. SPIR-V is a dedicated
graphics language that is aimed to be open and portable so that it works with as many drivers and devices as possible.

With the custom codegen backend (`rustc_codegen_spirv`) `cargo gpu` then compiles the shader it is pointed to. However, because custom codegen backends
are currently [an unstable feature](https://github.com/rust-lang/rust/issues/77933), `cargo gpu` also needs to install a "nightly" version of Rust. In
the usage instructions the backend and nightly Rust version are referred to as "artefacts" and can be explicitly managed with the arguments to the
`install` subcommand.

> [!TIP]
> Whilst `cargo gpu` attempts to isolate shader compilation as much possible, if the shader crate is contained in a workspace then it's possible that
> the nightly version required by the shader is, ironically, older than the Rust/Cargo versions required by the workspace. Say for instance the
> workspace might use a newer `Cargo.lock` layout not supported by the pinned version of the shader crate's custom codegen backend. The solution to
> this is to either exclude the shader from the workspace, or upgrade the shader's `spirv-std` dependency to the latest.

## Usage

All the following arguments for the `build` and `install` commands can also be set in the shader crate's `Cargo.toml`
file. In general usage that would be the recommended way to set config. See `crates/shader-crate-template/Cargo.toml`
for an example.

````
  Commands:
    install  Install rust-gpu compiler artifacts
    build    Compile a shader crate to SPIR-V
    show     Show some useful values
    help     Print this message or the help of the given subcommand(s)

  Options:
    -h, --help
            Print help

    -V, --version
            Print version


* Install
  Install rust-gpu compiler artifacts

  Usage: cargo-gpu install [OPTIONS]

  Options:
        --shader-crate <SHADER_CRATE>
            Directory containing the shader crate to compile

            [default: ./]

        --spirv-builder-source <SPIRV_BUILDER_SOURCE>
            Source of `spirv-builder` dependency Eg: "https://github.com/Rust-GPU/rust-gpu"

        --spirv-builder-version <SPIRV_BUILDER_VERSION>
            Version of `spirv-builder` dependency.
            * If `--spirv-builder-source` is not set, then this is assumed to be a crates.io semantic
              version such as "0.9.0".
            * If `--spirv-builder-source` is set, then this is assumed to be a Git "commitsh", such
              as a Git commit hash or a Git tag, therefore anything that `git checkout` can resolve.

        --rust-toolchain <RUST_TOOLCHAIN>
            Rust toolchain channel to use to build `spirv-builder`.

            This must be compatible with the `spirv_builder` argument as defined in the `rust-gpu` repo.

        --force-spirv-cli-rebuild
            Force `spirv-builder-cli` and `rustc_codegen_spirv` to be rebuilt

        --auto-install-rust-toolchain
            Assume "yes" to "Install Rust toolchain: [y/n]" prompt

    -h, --help
            Print help (see a summary with '-h')


* Build
  Compile a shader crate to SPIR-V

  Usage: cargo-gpu build [OPTIONS]

  Options:
        --shader-crate <SHADER_CRATE>
            Directory containing the shader crate to compile

            [default: ./]

        --spirv-builder-source <SPIRV_BUILDER_SOURCE>
            Source of `spirv-builder` dependency Eg: "https://github.com/Rust-GPU/rust-gpu"

        --spirv-builder-version <SPIRV_BUILDER_VERSION>
            Version of `spirv-builder` dependency.
            * If `--spirv-builder-source` is not set, then this is assumed to be a crates.io semantic
              version such as "0.9.0".
            * If `--spirv-builder-source` is set, then this is assumed to be a Git "commitsh", such
              as a Git commit hash or a Git tag, therefore anything that `git checkout` can resolve.

        --rust-toolchain <RUST_TOOLCHAIN>
            Rust toolchain channel to use to build `spirv-builder`.

            This must be compatible with the `spirv_builder` argument as defined in the `rust-gpu` repo.

        --force-spirv-cli-rebuild
            Force `spirv-builder-cli` and `rustc_codegen_spirv` to be rebuilt

        --auto-install-rust-toolchain
            Assume "yes" to "Install Rust toolchain: [y/n]" prompt

    -o, --output-dir <OUTPUT_DIR>
            Path to the output directory for the compiled shaders

            [default: ./]

        --no-default-features
            Set cargo default-features

        --features <FEATURES>
            Set cargo features

        --target <TARGET>
            `rust-gpu` compile target

            [default: spirv-unknown-vulkan1.2]

        --shader-target <SHADER_TARGET>
            Shader target

            [default: spirv-unknown-vulkan1.2]

        --deny-warnings
            Treat warnings as errors during compilation

        --debug
            Compile shaders in debug mode

        --capability <CAPABILITY>
            Enables the provided SPIR-V capabilities. See: `impl core::str::FromStr for spirv_builder::Capability`

        --extension <EXTENSION>
            Enables the provided SPIR-V extensions. See <https://github.com/KhronosGroup/SPIRV-Registry> for all extensions

        --multimodule
            Compile one .spv file per entry point

        --spirv-metadata <SPIRV_METADATA>
            Set the level of metadata included in the SPIR-V binary

            [default: none]

        --relax-struct-store
            Allow store from one struct type to a different type with compatible layout and members

        --relax-logical-pointer
            Allow allocating an object of a pointer type and returning a pointer value from a function in logical addressing mode

        --relax-block-layout
            Enable `VK_KHR_relaxed_block_layout` when checking standard uniform, storage buffer, and push constant layouts. This is the default when targeting Vulkan 1.1 or later

        --uniform-buffer-standard-layout
            Enable `VK_KHR_uniform_buffer_standard_layout` when checking standard uniform buffer layouts

        --scalar-block-layout
            Enable `VK_EXT_scalar_block_layout` when checking standard uniform, storage buffer, and push constant layouts. Scalar layout rules are more permissive than relaxed block layout so in effect this will override the --relax-block-layout option

        --skip-block-layout
            Skip checking standard uniform / storage buffer layout. Overrides any --relax-block-layout or --scalar-block-layout option

        --preserve-bindings
            Preserve unused descriptor bindings. Useful for reflection

    -h, --help
            Print help (see a summary with '-h')


* Show
  Show some useful values

  Usage: cargo-gpu show <COMMAND>

  Commands:
    cache-directory  Displays the location of the cache directory
    spirv-source     The source location of spirv-std
    help             Print this message or the help of the given subcommand(s)

  Options:
    -h, --help
            Print help


        * Cache-directory
          Displays the location of the cache directory

          Usage: cargo-gpu show cache-directory

          Options:
            -h, --help
                    Print help


        * Spirv-source
          The source location of spirv-std

          Usage: cargo-gpu show spirv-source [OPTIONS]

          Options:
                --shader-crate <SHADER_CRATE>
                    The location of the shader-crate to inspect to determine its spirv-std dependency

                    [default: ./]

            -h, --help
                    Print help
````
