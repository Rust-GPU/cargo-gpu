//! Test shaders.
//!
//! Here we have some shaders that have been problematic in the past.
#![no_std]
#![expect(
    unexpected_cfgs,
    reason = "rust-gpu uses the spirv attribute macro heavily"
)]
use spirv_std::{glam::Vec4, spirv};

/// This shader has the same name as the vertex shader below it.
/// It should be compiled and then named something so that the two
/// don't clobber each other.
/// See <https://github.com/Rust-GPU/cargo-gpu/issues/85>
#[inline(never)]
#[spirv(fragment(entry_point_name = "duplicate_main"))]
pub const fn dupe_frag(in_color: Vec4, output: &mut Vec4) {
    *output = in_color;
}

/// This shader has the same name as the fragment shader above it.
/// It should be compiled and then named something so that the two
/// don't clobber each other.
/// See <https://github.com/Rust-GPU/cargo-gpu/issues/85>
#[spirv(vertex(entry_point_name = "duplicate_main"))]
pub const fn implicit_isosceles_vertex(
    // Which vertex within the render unit are we rendering
    #[spirv(vertex_index)] vertex_index: u32,
    #[spirv(position)] clip_pos: &mut Vec4,
) {
    *clip_pos = Vec4::splat(vertex_index as f32);
}
