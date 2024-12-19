//! Display various information about `cargo gpu`, eg its cache directory.

use crate::cache_dir;

#[derive(Clone, Debug, clap::Parser)]
pub struct ShowSpirvSource {
    /// The location of the shader-crate to inspect to determine its spirv-std dependency.    
    #[clap(long, default_value = "./")]
    pub shader_crate: std::path::PathBuf,
}

/// Different tidbits of information that can be queried at the command line.
#[derive(Clone, Debug, clap::Subcommand)]
pub enum Info {
    /// Displays the location of the cache directory
    CacheDirectory,
    /// The source location of spirv-std
    SpirvSource(ShowSpirvSource),
}

/// `cargo gpu show`
#[derive(clap::Parser)]
pub struct Show {
    /// Display information about rust-gpu
    #[clap(subcommand)]
    command: Info,
}

impl Show {
    /// Entrypoint
    pub fn run(self) {
        log::info!("{:?}: ", self.command);
        match self.command {
            Info::CacheDirectory => {
                println!("{}", cache_dir().display());
            }
            Info::SpirvSource(ShowSpirvSource { shader_crate }) => {
                let rust_gpu_source =
                    crate::spirv_source::SpirvSource::get_spirv_std_dep_definition(&shader_crate);
                println!("{rust_gpu_source}");
            }
        }
    }
}
