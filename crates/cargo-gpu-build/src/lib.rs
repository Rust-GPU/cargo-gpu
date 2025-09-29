//! Rust GPU shader crate builder.
//!
//! This library allows you to easily compile your `rust-gpu` shaders,
//! without requiring you to fix your entire project to a specific toolchain.
//!
//! # How it works
//!
//! This library manages installations of `rustc_codegen_spirv`
//! using [`rustc_codegen_spirv-cache`](spirv_cache) crate.
//!
//! Then is uses [`spirv-builder`](spirv_builder) crate
//! to pass the many additional parameters required to configure rustc and our codegen backend,
//! but provide you with a toolchain-agnostic version that you may use from stable rustc.

#![expect(clippy::missing_errors_doc, reason = "temporary allow this")] // TODO: remove this & fix documentation
#![expect(clippy::pub_use, reason = "part of public API")]

pub use rustc_codegen_spirv_cache as spirv_cache;
pub use rustc_codegen_spirv_cache::spirv_builder;

pub mod lockfile;
