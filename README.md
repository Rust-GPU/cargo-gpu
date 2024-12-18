# cargo-gpu

Command line tool for building Rust shaders using rust-gpu.

## Getting Started

### Installation

To install the tool ensure you have `rustup`. Then run:

```
cargo install --git https://github.com/rust-gpu/cargo-gpu
```

After that you can use `cargo gpu` to compile your shader crates with:

```
cargo gpu build
```

This plain invocation will compile the crate in the current directory and
place the compiled shaders in the current directory.

Use `cargo gpu help` to see other options :)

### Next Steps

You can try this out using the example repo at <https://github.com/rust-GPU/shader-crate-template>.
Keep in mind <https://github.com/rust-GPU/shader-crate-template> is _not_ yet a cargo generate template,
it's just a normal repo.

```
git clone https://github.com/rust-GPU/shader-crate-template
cd shader-crate-template
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

````
Commands:
  install  Install rust-gpu compiler artifacts
  build    Compile a shader crate to SPIR-V
  toml     Compile a shader crate according to the `cargo gpu build` parameters found in the given toml file
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

      --shader-target <SHADER_TARGET>
          Shader target

          [default: spirv-unknown-vulkan1.2]

      --no-default-features
          Set cargo default-features

      --features <FEATURES>
          Set cargo features

  -o, --output-dir <OUTPUT_DIR>
          Path to the output directory for the compiled shaders

          [default: ./]

  -h, --help
          Print help (see a summary with '-h')


* Toml

Compile a shader crate according to the `cargo gpu build` parameters found in the given toml file

Usage: cargo-gpu toml [PATH]

Arguments:
  [PATH]
          Path to a workspace or package Cargo.toml file.

          Must include a [[workspace | package].metadata.rust-gpu.build] section where
          arguments to `cargo gpu build` are listed.

          Path arguments like `output-dir` and `shader-manifest` must be relative to
          the location of the Cargo.toml file.

          Example:

          ```toml
              [package.metadata.rust-gpu.build.spirv-builder]
              git = "https://github.com/Rust-GPU/rust-gpu.git"
              rev = "0da80f8"

              [package.metadata.rust-gpu.build]
              output-dir = "shaders"
              shader-manifest = "shaders/manifest.json"
          ```

          Calling `cargo gpu toml {path/to/Cargo.toml}` with a Cargo.toml that
          contains the example above would compile the crate and place the compiled
          `.spv` files and manifest in a directory "shaders".

          [default: ./Cargo.toml]

Options:
  -h, --help
          Print help (see a summary with '-h')


* Show

Show some useful values

Usage: cargo-gpu show [OPTIONS]

Options:
      --cache-directory
          Displays the location of the cache directory

  -h, --help
          Print help

````
