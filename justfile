[group: 'ci']
build-shader-template rust-gpu-version="latest":
  scripts/build_shader_template.sh {{rust-gpu-version}}

[group: 'ci']
setup-lints:
	cargo binstall cargo-shear

[group: 'ci']
lints:
  cargo clippy -- --deny warnings
  cargo fmt --check
  # Look for unused crates
  cargo shear

