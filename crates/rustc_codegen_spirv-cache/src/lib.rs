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

#![expect(clippy::missing_errors_doc, reason = "temporary allow this")] // TODO: remove this & fix documentation
#![expect(clippy::pub_use, reason = "part of public API")]

pub use cargo_metadata;
pub use spirv_builder;

pub mod backend;
pub mod cache;
pub mod spirv_source;
pub mod target_specs;
pub mod toolchain;

/// Writes formatted user output into a [writer](std::io::Write).
#[macro_export]
macro_rules! user_output {
    ($dst:expr, $($args:tt)*) => {{
        #[allow(
            clippy::allow_attributes,
            clippy::useless_attribute,
            unused_imports,
            reason = "`std::io::Write` is only sometimes called??"
        )]
        use ::std::io::Write as _;

        let mut writer = $dst;
        #[expect(clippy::non_ascii_literal, reason = "CRAB GOOD. CRAB IMPORTANT.")]
        ::std::write!(writer, "🦀 ")
            .and_then(|()| ::std::write!(writer, $($args)*))
            .and_then(|()| ::std::io::Write::flush(&mut writer))
    }};
}
