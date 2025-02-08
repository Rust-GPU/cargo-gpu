#!/bin/sh

set -e

RUST_GPU_VERSION=${1:-latest}
SHADER_CRATE=crates/shader-crate-template
SHADER_CRATE_CARGO_TOML=$SHADER_CRATE/Cargo.toml
# Matching windows paths when they're at root (/) causes problems with how we canoniclaize paths.
TMP_DIR=".$(mktemp --directory)"

# We update config in the shader crate's `Cargo.toml` rather than just using the simpler
# CLI args, because we want to smoke test that setting config in `Cargo.toml` works.
update_config() {
	key=$1
	value=$2

	# Unix and OSX have different versions of this argument.
	SED_INPLACE="-i"
	if [ "$(uname)" = "Darwin" ]; then
		SED_INPLACE="-i ''"
	fi

	echo "Updating shader crate's 'Cargo.toml' with '$key =	\"$value\"'"
	sed "$SED_INPLACE" "s/^# $key/$key/" $SHADER_CRATE_CARGO_TOML
	sed "$SED_INPLACE" "s#^$key =.*#$key = \"$value\"#" $SHADER_CRATE_CARGO_TOML
	echo "Updated line:"
	grep "$key" $SHADER_CRATE_CARGO_TOML
}

cargo install --path crates/cargo-gpu

update_config "output-dir" "$TMP_DIR"
if [ "$RUST_GPU_VERSION" != "latest" ]; then
	update_config "spirv-std" "$RUST_GPU_VERSION"

	# Downgrading `spirv-std` can cause a conflict in the `Cargo.lock` manifest version.
	rm $SHADER_CRATE/Cargo.lock
fi

cargo gpu install \
	--shader-crate $SHADER_CRATE \
	--auto-install-rust-toolchain \
	--force-overwrite-lockfiles-v4-to-v3
cargo gpu build \
	--shader-crate $SHADER_CRATE \
	--force-spirv-cli-rebuild \
	--force-overwrite-lockfiles-v4-to-v3
ls -lah "$TMP_DIR"
cat "$TMP_DIR"/manifest.json
