//! Display various information about `cargo gpu`, eg its cache directory.

use crate::cache_dir;

/// Show the computed source of the spirv-std dependency.
#[derive(Clone, Debug, clap::Parser)]
pub struct SpirvSourceDep {
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
    SpirvSource(SpirvSourceDep),
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
    pub fn run(self) -> anyhow::Result<()> {
        log::info!("{:?}: ", self.command);

        #[expect(
            clippy::print_stdout,
            reason = "The output of this command could potentially be used in a script, \
                      so we _don't_ want to use `crate::user_output`, as that prefixes a crab."
        )]
        match self.command {
            Info::CacheDirectory => {
                println!("{}\n", cache_dir()?.display());
            }
            Info::SpirvSource(SpirvSourceDep { shader_crate }) => {
                let rust_gpu_source =
                    crate::spirv_source::SpirvSource::get_spirv_std_dep_definition(&shader_crate)?;
                {
                    println!("{rust_gpu_source}\n");
                }
            }
        }

        Ok(())
    }
}
