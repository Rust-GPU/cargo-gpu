//! Cacher of `rust-gpu` codegen for required toolchain.
//!
//! This library manages installations of `rustc_codegen_spirv`,
//! the codegen backend of rust-gpu to generate SPIR-V shader binaries.
//!
//! # How it works
//!
//! The codegen backend builds on internal, ever-changing interfaces of rustc,
//! which requires fixing a version of rust-gpu to a specific version of the rustc compiler.
//! Usually, this would require you to fix your entire project to that specific
//! toolchain, but this project loosens that requirement by managing installations
//! of `rustc_codegen_spirv` and their associated toolchains for you.
//!
//! ## Where the binaries are
//!
//! Prebuilt binaries are stored in the default [cache directory](crate::cache_dir()) of your OS:
//! * Windows: `C:/users/<user>/AppData/Local/rust-gpu`
//! * Mac: `~/Library/Caches/rust-gpu`
//! * Linux: `~/.cache/rust-gpu`

#![expect(clippy::pub_use, reason = "re-export for convenience")]

pub use self::cache_dir::{cache_dir, CacheDirError};

mod cache_dir;
