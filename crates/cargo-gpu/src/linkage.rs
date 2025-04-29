/// Shader source and entry point that can be used to create shader linkage.
#[derive(serde::Serialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Linkage {
    pub source_path: String,
    pub entry_point: String,
    pub wgsl_entry_point: String,
}

impl Linkage {
    pub fn new(entry_point: impl AsRef<str>, source_path: impl AsRef<std::path::Path>) -> Self {
        Self {
            // Force a forward slash convention here so it works on all OSs
            source_path: source_path
                .as_ref()
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/"),
            wgsl_entry_point: entry_point.as_ref().replace("::", ""),
            entry_point: entry_point.as_ref().to_string(),
        }
    }

    pub fn fn_name(&self) -> &str {
        self.entry_point.split("::").last().unwrap()
    }
}

/// A built shader entry-point, used in `spirv-builder-cli` to generate
/// a `build-manifest.json` used by `cargo-gpu`.
#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ShaderModule {
    pub entry: String,
    pub path: std::path::PathBuf,
}

impl ShaderModule {
    pub fn new(entry: impl AsRef<str>, path: impl AsRef<std::path::Path>) -> Self {
        Self {
            entry: entry.as_ref().into(),
            path: path.as_ref().into(),
        }
    }
}
