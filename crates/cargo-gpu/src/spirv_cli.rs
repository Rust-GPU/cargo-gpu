//! Query the shader crate to find what version of `rust-gpu` it depends on.
//! Then ensure that the relevant Rust toolchain and components are installed.

use crate::spirv_source::SpirvSource;

/// Cargo dependency for `spirv-builder` and the rust toolchain channel.
#[derive(Debug, Clone)]
pub struct SpirvCli {
    #[expect(
        clippy::doc_markdown,
        reason = "The URL should appear literally like this. But Clippy wants it to be a in markdown clickable link"
    )]
    /// The source and version of `rust-gpu`.
    /// Eg:
    ///   * From crates.io with version "0.10.0"
    ///   * From Git with:
    ///     - a repo of "https://github.com/Rust-GPU/rust-gpu.git"
    ///     - a revision of "abc213"
    pub source: SpirvSource,
    /// The toolchain channel that `rust-gpu` uses, eg "nightly-2024-04-24"
    pub channel: String,
    /// The date of the pinned version of `rust-gpu`
    pub date: chrono::NaiveDate,
}

impl core::fmt::Display for SpirvCli {
    #[expect(
        clippy::min_ident_chars,
        reason = "It's a core library trait implementation"
    )]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        format!("{}+{}", self.source, self.channel).fmt(f)
    }
}

impl SpirvCli {
    /// Create instance
    pub fn new(
        shader_crate_path: &std::path::PathBuf,
        maybe_rust_gpu_source: Option<String>,
        maybe_rust_gpu_version: Option<String>,
        maybe_rust_gpu_channel: Option<String>,
    ) -> Self {
        let (default_rust_gpu_source, rust_gpu_date, default_rust_gpu_channel) =
            SpirvSource::get_rust_gpu_deps_from_shader(shader_crate_path);

        let mut maybe_spirv_source: Option<SpirvSource> = None;
        if let Some(rust_gpu_version) = maybe_rust_gpu_version {
            let mut source = SpirvSource::CratesIO(rust_gpu_version.clone());
            if let Some(rust_gpu_source) = maybe_rust_gpu_source {
                source = SpirvSource::Git {
                    url: rust_gpu_source,
                    rev: rust_gpu_version,
                };
            }
            maybe_spirv_source = Some(source);
        }

        Self {
            source: maybe_spirv_source.unwrap_or(default_rust_gpu_source),
            channel: maybe_rust_gpu_channel.unwrap_or(default_rust_gpu_channel),
            date: rust_gpu_date,
        }
    }

    /// Create and/or return the cache directory
    pub fn cached_checkout_path(&self) -> std::path::PathBuf {
        let checkout_dir = crate::cache_dir()
            .join("spirv-builder-cli")
            .join(crate::to_dirname(self.to_string().as_ref()));
        std::fs::create_dir_all(&checkout_dir).unwrap_or_else(|error| {
            log::error!(
                "could not create checkout dir '{}': {error}",
                checkout_dir.display()
            );
            panic!("could not create checkout dir");
        });

        checkout_dir
    }

    /// Use `rustup` to install the toolchain and components, if not already installed.
    ///
    /// Pretty much runs:
    ///
    /// * rustup toolchain add nightly-2024-04-24
    /// * rustup component add --toolchain nightly-2024-04-24 rust-src rustc-dev llvm-tools
    pub fn ensure_toolchain_and_components_exist(&self) {
        // Check for the required toolchain
        let output_toolchain_list = std::process::Command::new("rustup")
            .args(["toolchain", "list"])
            .output()
            .unwrap();
        assert!(
            output_toolchain_list.status.success(),
            "could not list installed toolchains"
        );
        let string_toolchain_list = String::from_utf8_lossy(&output_toolchain_list.stdout);
        if string_toolchain_list
            .split_whitespace()
            .any(|toolchain| toolchain.starts_with(&self.channel))
        {
            log::debug!("toolchain {} is already installed", self.channel);
        } else {
            let output_toolchain_add = std::process::Command::new("rustup")
                .args(["toolchain", "add"])
                .arg(&self.channel)
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .output()
                .unwrap();
            assert!(
                output_toolchain_add.status.success(),
                "could not install required toolchain"
            );
        }

        // Check for the required components
        let output_component_list = std::process::Command::new("rustup")
            .args(["component", "list", "--toolchain"])
            .arg(&self.channel)
            .output()
            .unwrap();
        assert!(
            output_component_list.status.success(),
            "could not list installed components"
        );
        let string_component_list = String::from_utf8_lossy(&output_component_list.stdout);
        let required_components = ["rust-src", "rustc-dev", "llvm-tools"];
        let installed_components = string_component_list.lines().collect::<Vec<_>>();
        let all_components_installed = required_components.iter().all(|component| {
            installed_components.iter().any(|installed_component| {
                let is_component = installed_component.starts_with(component);
                let is_installed = installed_component.ends_with("(installed)");
                is_component && is_installed
            })
        });
        if all_components_installed {
            log::debug!("all required components are installed");
        } else {
            let output_component_add = std::process::Command::new("rustup")
                .args(["component", "add", "--toolchain"])
                .arg(&self.channel)
                .args(["rust-src", "rustc-dev", "llvm-tools"])
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .output()
                .unwrap();
            assert!(
                output_component_add.status.success(),
                "could not install required components"
            );
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test_log::test]
    fn cached_checkout_dir_sanity() {
        let shader_template_path = crate::test::shader_crate_template_path();
        let spirv = SpirvCli::new(&shader_template_path, None, None, None);
        let dir = spirv.cached_checkout_path();
        let name = dir
            .file_name()
            .unwrap()
            .to_str()
            .map(std::string::ToString::to_string)
            .unwrap();
        assert_eq!(
            "https___github_com_Rust-GPU_rust-gpu+82a0f69+nightly-2024-04-24",
            &name
        );
    }
}
