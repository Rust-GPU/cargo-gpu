//! Query the shader crate to find what version of `rust-gpu` it depends on.
//! Then ensure that the relevant Rust toolchain and components are installed.

use std::io::Write as _;

use anyhow::Context as _;

use crate::spirv_source::SpirvSource;

/// `Cargo.lock` manifest version 4 became the default in Rust 1.83.0. Conflicting manifest
/// versions between the workspace and the shader crate, can cause problems.
const RUST_VERSION_THAT_USES_V4_CARGO_LOCKS: &str = "1.83.0";

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
    /// `Cargo.lock`s that have had their manifest versions changed by us and need changing back.
    pub cargo_lock_files_with_changed_manifest_versions: Vec<std::path::PathBuf>,
    /// Has the user overridden the toolchain consent prompt
    is_toolchain_install_consent: bool,
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
        shader_crate_path: &std::path::Path,
        maybe_rust_gpu_source: Option<String>,
        maybe_rust_gpu_version: Option<String>,
        maybe_rust_gpu_channel: Option<String>,
        is_toolchain_install_consent: bool,
        is_force_overwrite_lockfiles_v4_to_v3: bool,
    ) -> anyhow::Result<Self> {
        let mut cargo_lock_files_with_changed_manifest_versions = vec![];

        let maybe_shader_crate_lock =
            Self::ensure_workspace_rust_version_doesnt_conflict_with_shader(
                shader_crate_path,
                is_force_overwrite_lockfiles_v4_to_v3,
            )?;

        if let Some(shader_crate_lock) = maybe_shader_crate_lock {
            cargo_lock_files_with_changed_manifest_versions.push(shader_crate_lock);
        }

        let (default_rust_gpu_source, rust_gpu_date, default_rust_gpu_channel) =
            SpirvSource::get_rust_gpu_deps_from_shader(shader_crate_path)?;

        let maybe_workspace_crate_lock =
            Self::ensure_shader_rust_version_doesnt_conflict_with_any_cargo_locks(
                shader_crate_path,
                default_rust_gpu_channel.clone(),
                is_force_overwrite_lockfiles_v4_to_v3,
            )?;

        if let Some(workspace_crate_lock) = maybe_workspace_crate_lock {
            cargo_lock_files_with_changed_manifest_versions.push(workspace_crate_lock);
        }

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

        Ok(Self {
            source: maybe_spirv_source.unwrap_or(default_rust_gpu_source),
            channel: maybe_rust_gpu_channel.unwrap_or(default_rust_gpu_channel),
            date: rust_gpu_date,
            is_toolchain_install_consent,
            cargo_lock_files_with_changed_manifest_versions,
        })
    }

    /// Create and/or return the cache directory
    pub fn cached_checkout_path(&self) -> anyhow::Result<std::path::PathBuf> {
        let checkout_dir = crate::cache_dir()?
            .join("spirv-builder-cli")
            .join(crate::to_dirname(self.to_string().as_ref()));
        std::fs::create_dir_all(&checkout_dir).with_context(|| {
            format!("could not create checkout dir '{}'", checkout_dir.display())
        })?;

        Ok(checkout_dir)
    }

    /// Use `rustup` to install the toolchain and components, if not already installed.
    ///
    /// Pretty much runs:
    ///
    /// * rustup toolchain add nightly-2024-04-24
    /// * rustup component add --toolchain nightly-2024-04-24 rust-src rustc-dev llvm-tools
    pub fn ensure_toolchain_and_components_exist(&self) -> anyhow::Result<()> {
        // Check for the required toolchain
        let output_toolchain_list = std::process::Command::new("rustup")
            .args(["toolchain", "list"])
            .output()?;
        anyhow::ensure!(
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
            let message = format!("Rust {} with `rustup`", self.channel);
            self.get_consent_for_toolchain_install(format!("Install {message}").as_ref())?;
            crate::user_output!("Installing {message}\n");

            let output_toolchain_add = std::process::Command::new("rustup")
                .args(["toolchain", "add"])
                .arg(&self.channel)
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .output()?;
            anyhow::ensure!(
                output_toolchain_add.status.success(),
                "could not install required toolchain"
            );
        }

        // Check for the required components
        let output_component_list = std::process::Command::new("rustup")
            .args(["component", "list", "--toolchain"])
            .arg(&self.channel)
            .output()?;
        anyhow::ensure!(
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
            let message = "toolchain components (rust-src, rustc-dev, llvm-tools) with `rustup`";
            self.get_consent_for_toolchain_install(format!("Install {message}").as_ref())?;
            crate::user_output!("Installing {message}\n");

            let output_component_add = std::process::Command::new("rustup")
                .args(["component", "add", "--toolchain"])
                .arg(&self.channel)
                .args(["rust-src", "rustc-dev", "llvm-tools"])
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .output()?;
            anyhow::ensure!(
                output_component_add.status.success(),
                "could not install required components"
            );
        }

        Ok(())
    }

    /// Prompt user if they want to install a new Rust toolchain.
    fn get_consent_for_toolchain_install(&self, prompt: &str) -> anyhow::Result<()> {
        if self.is_toolchain_install_consent {
            return Ok(());
        }
        log::debug!("asking for consent to install the required toolchain");
        crossterm::terminal::enable_raw_mode()?;
        crate::user_output!("{prompt} [y/n]: ");
        let input = crossterm::event::read()?;
        crossterm::terminal::disable_raw_mode()?;

        if let crossterm::event::Event::Key(crossterm::event::KeyEvent {
            code: crossterm::event::KeyCode::Char('y'),
            ..
        }) = input
        {
            Ok(())
        } else {
            crate::user_output!("Exiting...\n");
            std::process::exit(0);
        }
    }

    /// See docs for `force_overwrite_lockfiles_v4_to_v3` flag for why we do this.
    fn ensure_workspace_rust_version_doesnt_conflict_with_shader(
        shader_crate_path: &std::path::Path,
        is_force_overwrite_lockfiles_v4_to_v3: bool,
    ) -> anyhow::Result<Option<std::path::PathBuf>> {
        log::debug!("Ensuring no v3/v4 `Cargo.lock` conflicts from workspace Rust...");
        let workspace_rust_version = Self::get_rustc_version(None)?;
        if version_check::Version::at_least(
            &workspace_rust_version,
            RUST_VERSION_THAT_USES_V4_CARGO_LOCKS,
        ) {
            log::debug!(
                "user's Rust is v{}, so no v3/v4 conflicts possible.",
                workspace_rust_version
            );
            return Ok(None);
        }

        Self::handle_conflicting_cargo_lock_v4(
            shader_crate_path,
            is_force_overwrite_lockfiles_v4_to_v3,
        )?;

        if is_force_overwrite_lockfiles_v4_to_v3 {
            Ok(Some(shader_crate_path.join("Cargo.lock")))
        } else {
            Ok(None)
        }
    }

    /// See docs for `force_overwrite_lockfiles_v4_to_v3` flag for why we do this.
    fn ensure_shader_rust_version_doesnt_conflict_with_any_cargo_locks(
        shader_crate_path: &std::path::Path,
        channel: String,
        is_force_overwrite_lockfiles_v4_to_v3: bool,
    ) -> anyhow::Result<Option<std::path::PathBuf>> {
        log::debug!("Ensuring no v3/v4 `Cargo.lock` conflicts from shader's Rust...");
        let shader_rust_version = Self::get_rustc_version(Some(channel))?;
        if version_check::Version::at_least(
            &shader_rust_version,
            RUST_VERSION_THAT_USES_V4_CARGO_LOCKS,
        ) {
            log::debug!(
                "shader's Rust is v{}, so no v3/v4 conflicts possible.",
                shader_rust_version
            );
            return Ok(None);
        }

        log::debug!(
            "shader's Rust is v{}, so checking both shader and workspace `Cargo.lock` manifest versions...",
            shader_rust_version
        );

        if shader_crate_path.join("Cargo.lock").exists() {
            // Note that we don't return the `Cargo.lock` here (so that it's marked for reversion
            // after the build), because we can be sure that updating it now is actually updating it
            // to the state it should have been all along. Therefore it doesn't need reverting once
            // fixed.
            Self::handle_conflicting_cargo_lock_v4(
                shader_crate_path,
                is_force_overwrite_lockfiles_v4_to_v3,
            )?;
        }

        if let Some(workspace_root) = Self::get_workspace_root(shader_crate_path)? {
            Self::handle_conflicting_cargo_lock_v4(
                workspace_root,
                is_force_overwrite_lockfiles_v4_to_v3,
            )?;
            return Ok(Some(workspace_root.join("Cargo.lock")));
        }

        Ok(None)
    }

    /// Get the path to the shader crate's workspace, if it has one. We can't use the traditional
    /// `cargo metadata` because if the workspace has a conflicting `Cargo.lock` manifest version
    /// then that command won't work. Instead we do an old school recursive file tree walk.
    fn get_workspace_root(
        shader_crate_path: &std::path::Path,
    ) -> anyhow::Result<Option<&std::path::Path>> {
        let shader_cargo_toml = std::fs::read_to_string(shader_crate_path.join("Cargo.toml"))?;
        if !shader_cargo_toml.contains("workspace = true") {
            return Ok(None);
        }

        let mut current_path = shader_crate_path;
        #[expect(clippy::default_numeric_fallback, reason = "It's just a loop")]
        for _ in 0..15 {
            if let Some(parent_path) = current_path.parent() {
                if parent_path.join("Cargo.lock").exists() {
                    return Ok(Some(parent_path));
                }
                current_path = parent_path;
            } else {
                break;
            }
        }

        Ok(None)
    }

    /// When Rust < 1.83.0 is being used an error will occur if it tries to parse `Cargo.lock`
    /// files that use lockfile manifest version 4. Here we check and handle that.
    fn handle_conflicting_cargo_lock_v4(
        folder: &std::path::Path,
        is_force_overwrite_lockfiles_v4_to_v3: bool,
    ) -> anyhow::Result<()> {
        let shader_cargo_lock_path = folder.join("Cargo.lock");
        let shader_cargo_lock = std::fs::read_to_string(shader_cargo_lock_path.clone())?;
        let third_line = shader_cargo_lock.lines().nth(2).context("")?;
        if third_line.contains("version = 4") {
            Self::handle_v3v4_conflict(
                &shader_cargo_lock_path,
                is_force_overwrite_lockfiles_v4_to_v3,
            )?;
            return Ok(());
        }
        if third_line.contains("version = 3") {
            return Ok(());
        }
        anyhow::bail!(
            "Unrecognized `Cargo.lock` manifest version at: {}",
            folder.display()
        )
    }

    /// Handle conflicting `Cargo.lock` manifest versions by either overwriting the manifest
    /// version or exiting with advice on how to handle the conflict.
    fn handle_v3v4_conflict(
        offending_cargo_lock: &std::path::Path,
        is_force_overwrite_lockfiles_v4_to_v3: bool,
    ) -> anyhow::Result<()> {
        if !is_force_overwrite_lockfiles_v4_to_v3 {
            Self::exit_with_v3v4_hack_suggestion();
        }

        Self::replace_cargo_lock_manifest_version(offending_cargo_lock, "4", "3")?;

        Ok(())
    }

    /// Once all install and builds have completed put their manifest versions back to how they
    /// were.
    pub fn revert_cargo_lock_manifest_versions(&self) -> anyhow::Result<()> {
        for offending_cargo_lock in &self.cargo_lock_files_with_changed_manifest_versions {
            log::debug!("Reverting: {}", offending_cargo_lock.display());
            Self::replace_cargo_lock_manifest_version(offending_cargo_lock, "3", "4")?;
        }

        Ok(())
    }

    /// Replace the manifest version, eg `version = 4`, in a `Cargo.lock` file.
    fn replace_cargo_lock_manifest_version(
        offending_cargo_lock: &std::path::Path,
        from_version: &str,
        to_version: &str,
    ) -> anyhow::Result<()> {
        log::warn!(
            "Replacing manifest version 'version = {}' with 'version = {}' in: {}",
            from_version,
            to_version,
            offending_cargo_lock.display()
        );
        let old_contents = std::fs::read_to_string(offending_cargo_lock)?;
        let new_contents = old_contents.replace(
            &format!("\nversion = {from_version}\n"),
            &format!("\nversion = {to_version}\n"),
        );

        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(offending_cargo_lock)?;
        file.write_all(new_contents.as_bytes())?;

        Ok(())
    }

    /// Exit and give the user advice on how to deal with the infamous v3/v4 Cargo lockfile version
    /// problem.
    #[expect(clippy::non_ascii_literal, reason = "It's CLI output")]
    fn exit_with_v3v4_hack_suggestion() {
        crate::user_output!(
            "Conflicting `Cargo.lock` versions detected ⚠️\n\
            Because `cargo gpu` uses a dedicated Rust toolchain for compiling shaders\n\
            it's possible that the `Cargo.lock` manifest version of the shader crate\n\
            does not match the `Cargo.lock` manifest version of the workspace. This is\n\
            due to a change in the defaults introduced in Rust 1.83.0.\n\
            \n\
            One way to resolve this is to force the workspace to use the same version\n\
            of Rust as required by the shader. However that is not often ideal or even\n\
            possible. Another way is to exlude the shader from the workspace. This is\n\
            also not ideal if you have many shaders sharing config from the workspace.\n\
            \n\
            Therefore `cargo gpu build/install` offers a workaround with the argument:\n\
              --force-overwrite-lockfiles-v4-to-v3\n\
            \n\
            See `cargo gpu build --help` for more information.\n\
            "
        );
        std::process::exit(1);
    }

    /// Get the version of `rustc`.
    fn get_rustc_version(
        maybe_toolchain: Option<String>,
    ) -> anyhow::Result<version_check::Version> {
        let mut maybe_current_env_toolchain: Option<std::ffi::OsString> = None;
        if let Some(toolchain) = maybe_toolchain {
            maybe_current_env_toolchain = std::env::var_os("RUSTUP_TOOLCHAIN");
            std::env::set_var("RUSTUP_TOOLCHAIN", toolchain);
        }

        let Some(version) = version_check::Version::read() else {
            anyhow::bail!("Couldn't get `rustc --version`");
        };

        if let Some(current_env_toolchain) = maybe_current_env_toolchain {
            std::env::set_var("RUSTUP_TOOLCHAIN", current_env_toolchain);
        }

        Ok(version)
    }
}

impl Drop for SpirvCli {
    fn drop(&mut self) {
        let result = self.revert_cargo_lock_manifest_versions();
        if let Err(error) = result {
            log::error!("Couldn't revert some or all of the shader `Cargo.lock` files: {error}");
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test_log::test]
    fn cached_checkout_dir_sanity() {
        let shader_template_path = crate::test::shader_crate_template_path();
        // TODO: This downloads the `rust-gpu` repo which slows the test down. Can we avoid that
        // just to get the sanity check?
        let spirv = SpirvCli::new(&shader_template_path, None, None, None, true, false).unwrap();
        let dir = spirv.cached_checkout_path().unwrap();
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
