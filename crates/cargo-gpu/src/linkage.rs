//! Mainly for the Linkage struct, which is written to a json file. Previously in `spirv-tools-cli` but got moved here.

/// Shader source and entry point that can be used to create shader linkage.
#[derive(serde::Serialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Linkage {
    /// File path to the entry point's source file
    pub source_path: String,
    /// Name of the entry point for spirv and vulkan
    pub entry_point: String,
    /// Name of the entry point for wgsl, where `::` characters have been removed
    pub wgsl_entry_point: String,
}

impl Linkage {
    /// Make a new `Linkage` from an entry point and source path
    #[expect(clippy::impl_trait_in_params, reason = "just a struct new")]
    pub fn new(entry_point: impl AsRef<str>, source_path: impl AsRef<std::path::Path>) -> Self {
        Self {
            // Force a forward slash convention here so it works on all OSs
            source_path: source_path
                .as_ref()
                .components()
                .map(|comp| comp.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/"),
            wgsl_entry_point: entry_point.as_ref().replace("::", ""),
            entry_point: entry_point.as_ref().to_owned(),
        }
    }

    /// The entry point function name, without the fully qualified mod path
    #[expect(clippy::unwrap_used, reason = "unreachable")]
    pub fn fn_name(&self) -> &str {
        self.entry_point.split("::").last().unwrap()
    }
}

/// A built shader entry-point, used in `spirv-builder-cli` to generate
/// a `build-manifest.json` used by `cargo-gpu`.
#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ShaderModule {
    /// Name of the entry point for spirv and vulkan
    pub entry: String,
    /// File path to the entry point's source file
    pub path: std::path::PathBuf,
}

impl ShaderModule {
    /// Make a new `ShaderModule` from an entry point and source path
    #[expect(clippy::impl_trait_in_params, reason = "just a struct new")]
    pub fn new(entry: impl AsRef<str>, path: impl AsRef<std::path::Path>) -> Self {
        Self {
            entry: entry.as_ref().into(),
            path: path.as_ref().into(),
        }
    }
}
