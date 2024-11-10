//! Rust GPU shader crate builder.
//!
//! This program manages installations of `spirv-builder-cli` and `rustc_codegen_spirv`.
//! It uses these tools to compile Rust code into SPIR-V.
use std::{io::Write, str::FromStr};

use cargo_gpu::{spirv_builder_cli::ShaderModule, Linkage};
use clap::Parser;

const SPIRV_BUILDER_CLI_CARGO_TOML: &str = include_str!("../../spirv-builder-cli/Cargo.toml");
const SPIRV_BUILDER_CLI_RUST_TOOLCHAIN_TOML: &str =
    include_str!("../../spirv-builder-cli/rust-toolchain.toml");
const SPIRV_BUILDER_CLI_MAIN: &str = include_str!("../../spirv-builder-cli/src/main.rs");
const SPIRV_BUILDER_CLI_LIB: &str = include_str!("lib.rs");
const SPIRV_BUILDER_FILES: &[(&str, &str)] = &[
    ("Cargo.toml", SPIRV_BUILDER_CLI_CARGO_TOML),
    ("rust-toolchain.toml", SPIRV_BUILDER_CLI_RUST_TOOLCHAIN_TOML),
    ("src/main.rs", SPIRV_BUILDER_CLI_MAIN),
    ("src/lib.rs", SPIRV_BUILDER_CLI_LIB),
];

/// Location of `cargo-gpu` source, which contains `crates/spirv-builder-cli`.
#[derive(Debug, Clone)]
struct SpirvCli {
    dep: String,
    channel: String,
}

impl core::fmt::Display for SpirvCli {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        format!("{}+{}", self.dep, self.channel).fmt(f)
    }
}

impl SpirvCli {
    fn to_dirname(&self) -> String {
        self.to_string()
            .replace([std::path::MAIN_SEPARATOR, '.', ':', '@'], "_")
            .to_string()
    }

    fn cached_checkout_path(&self) -> std::path::PathBuf {
        let checkout_dir = cache_dir().join(self.to_dirname());
        std::fs::create_dir_all(&checkout_dir).unwrap_or_else(|e| {
            log::error!(
                "could not create checkout dir '{}': {e}",
                checkout_dir.display()
            );
            panic!("could not create checkout dir");
        });

        checkout_dir
    }

    // fn perform_checkout(&self, refresh_git_source: bool) {
    //     if let SpirvCli::Git { url, source } = self {
    //         let checkout_path = self.cached_checkout_path();
    //         let git_path = checkout_path.join(".git");
    //         if git_path.exists() {
    //             log::info!("found existing .git dir in '{}'", git_path.display());
    //             if refresh_git_source {
    //                 log::info!("refreshing existing checkout");
    //                 let output = std::process::Command::new("git")
    //                     .current_dir(&checkout_path)
    //                     .args(["pull", "origin"])
    //                     .stdout(std::process::Stdio::inherit())
    //                     .stderr(std::process::Stdio::inherit())
    //                     .output()
    //                     .unwrap();
    //                 assert!(output.status.success(), "could not refresh git source");
    //             } else {
    //                 log::info!("...and argument `--refresh-spirv-cli` was not passed, so skipping the refresh");
    //             }
    //         } else {
    //             log::info!("checking out source '{}'", checkout_path.display());
    //             let output = std::process::Command::new("git")
    //                 .arg("clone")
    //                 .arg(url)
    //                 .arg(&checkout_path)
    //                 .stdout(std::process::Stdio::inherit())
    //                 .stderr(std::process::Stdio::inherit())
    //                 .output()
    //                 .unwrap();
    //             assert!(output.status.success(), "could not checkout git source");

    //             log::info!("checking out source '{}'", checkout_path.display());
    //             let output = std::process::Command::new("git")
    //                 .current_dir(&checkout_path)
    //                 .arg("switch")
    //                 .arg(source.to_string())
    //                 .stdout(std::process::Stdio::inherit())
    //                 .stderr(std::process::Stdio::inherit())
    //                 .output()
    //                 .unwrap();
    //             assert!(output.status.success(), "could not checkout {source}");
    //         }
    //         log::info!("...checkout done.");
    //     }
    // }

    // fn toolchain(&self) -> String {
    //     /// Determine the toolchain given a path to `cargo-gpu` source directory
    //     fn toolchain_from_path(path: &std::path::Path) -> String {
    //         let rust_toolchain_toml_path = path
    //             .join("crates")
    //             .join("spirv-builder-cli")
    //             .join("rust-toolchain.toml");
    //         let rust_toolchain_toml = std::fs::read_to_string(rust_toolchain_toml_path).unwrap();
    //         let table = toml::Table::from_str(&rust_toolchain_toml)
    //             .unwrap_or_else(|e| panic!("could not parse rust-toolchain.toml: {e}"));
    //         let toolchain = table
    //             .get("toolchain")
    //             .unwrap_or_else(|| panic!("rust-toolchain.toml is missing 'toolchain'"))
    //             .as_table()
    //             .unwrap();
    //         toolchain
    //             .get("channel")
    //             .unwrap_or_else(|| panic!("rust-toolchain.toml is missing 'channel'"))
    //             .as_str()
    //             .unwrap()
    //             .into()
    //     }

    //     let path = match self {
    //         SpirvCli::LocalPath(path) => path.clone(),
    //         _ => self.cached_checkout_path(),
    //     };
    //     toolchain_from_path(&path)
    // }

    fn write_source_files(&self) {
        let checkout = self.cached_checkout_path();
        std::fs::create_dir_all(checkout.join("src")).unwrap();
        for (filename, contents) in SPIRV_BUILDER_FILES.iter() {
            let path = checkout.join(filename);
            let mut file = std::fs::File::create(&path).unwrap();
            let replaced_contents = contents
                .replace("${SPIRV_BUILDER_SOURCE}", &self.dep)
                .replace("${CHANNEL}", &self.channel);
            file.write_all(replaced_contents.as_bytes()).unwrap();
        }
    }

    fn build(&self, force: bool) -> (std::path::PathBuf, std::path::PathBuf) {
        let checkout = self.cached_checkout_path();
        let release = checkout.join("target").join("release");

        let dylib_filename = format!(
            "{}rustc_codegen_spirv{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_SUFFIX
        );
        let dylib_path = release.join(&dylib_filename);
        let dest_dylib_path = checkout.join(&dylib_filename);
        let dest_cli_path = checkout.join("spirv-builder-cli");
        if dest_dylib_path.is_file() && dest_cli_path.is_file() {
            log::info!("cargo-gpu artifacts are already installed");
        }

        if dest_dylib_path.is_file() && dest_cli_path.is_file() && !force {
            log::info!("...and so we are aborting the install step.");
        } else {
            log::debug!(
                "writing spirv-builder-cli source files into '{}'",
                checkout.display()
            );
            self.write_source_files();

            log::debug!("building artifacts");
            let output = std::process::Command::new("cargo")
                .current_dir(&checkout)
                .args(["build", "--release"])
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .output()
                .unwrap();
            assert!(output.status.success(), "...build error!");

            if dylib_path.is_file() {
                log::info!("successfully built {}", dylib_path.display());
                std::fs::rename(&dylib_path, &dest_dylib_path).unwrap();
            } else {
                log::error!("could not find {}", dylib_path.display());
                panic!("spirv-builder-cli build failed");
            }

            let cli_path = release.join("spirv-builder-cli");
            if cli_path.is_file() {
                log::info!("successfully built {}", cli_path.display());
                std::fs::rename(&cli_path, &dest_cli_path).unwrap();
            } else {
                log::error!("could not find {}", cli_path.display());
                panic!("spirv-builder-cli build failed");
            }
        }
        (dest_dylib_path, dest_cli_path)
    }
}

#[derive(Parser)]
#[clap(author, version, about)]
struct Cli {
    /// spirv-builder dependency, written just like in a Cargo.toml file.
    #[clap(
        long,
        default_value = r#"{ git = "https://github.com/Rust-GPU/rust-gpu.git" }"#
    )]
    spirv_builder: String,

    /// Rust toolchain channel to use to build `spirv-builder`.
    ///
    /// This must match the `spirv_builder` argument.
    #[clap(long, default_value = "nightly-2024-04-24")]
    rust_toolchain: String,

    /// Force `spirv-builder-cli` and `rustc_codegen_spirv` to be rebuilt.
    #[clap(long)]
    force_spirv_cli_rebuild: bool,

    /// Directory containing the shader crate to compile.
    #[clap(long, default_value = "./")]
    shader_crate: std::path::PathBuf,

    /// Shader target.
    #[clap(long, default_value = "spirv-unknown-vulkan1.2")]
    shader_target: String,

    /// Path to the output JSON manifest file where the paths to .spv files
    /// and the names of their entry points will be saved.
    #[clap(long)]
    shader_manifest: Option<std::path::PathBuf>,

    /// Set cargo default-features.
    #[clap(long)]
    no_default_features: bool,

    /// Set cargo features.
    #[clap(long)]
    features: Vec<String>,

    /// Path to the output directory for the compiled shaders.
    #[clap(long, short, default_value = "./")]
    output_dir: std::path::PathBuf,

    /// If set the shaders will be compiled but not put into place.
    #[clap(long, short)]
    dry_run: bool,
}

fn cache_dir() -> std::path::PathBuf {
    directories::BaseDirs::new()
        .unwrap_or_else(|| {
            log::error!("could not find the user home directory");
            panic!("cache_dir failed");
        })
        .cache_dir()
        .join("rust-gpu")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::builder().init();

    let Cli {
        spirv_builder,
        rust_toolchain,
        force_spirv_cli_rebuild,
        shader_crate,
        shader_target,
        no_default_features,
        features,
        output_dir,
        shader_manifest: output_manifest,
        dry_run,
    } = Cli::parse_from(std::env::args().filter(|p| {
        // Calling cargo-gpu as the cargo subcommand "cargo gpu" passes "gpu"
        // as the first parameter, which we want to ignore.
        p != "gpu"
    }));

    log::info!("using spirv-std version 'spirv-builder = {spirv_builder}'");

    // Ensure the cache dir exists
    let cache_dir = cache_dir();
    std::fs::create_dir_all(&cache_dir).unwrap_or_else(|e| {
        log::error!(
            "could not create cache directory '{}': {e}",
            cache_dir.display()
        );
        panic!("could not create cache dir");
    });

    // Check out the spirv-builder-cli source into the cache dir with a prefix and build it.
    let spirv_version = SpirvCli {
        dep: spirv_builder,
        channel: rust_toolchain,
    };
    let (dylib_path, spirv_builder_cli_path) = spirv_version.build(force_spirv_cli_rebuild);

    // Ensure the shader output dir exists
    std::fs::create_dir_all(&output_dir).unwrap();

    assert!(
        shader_crate.exists(),
        "shader crate '{}' does not exist. (Current dir is '{}')",
        shader_crate.display(),
        std::env::current_dir().unwrap().display()
    );

    let spirv_builder_args = cargo_gpu::spirv_builder_cli::Args {
        dylib_path,
        shader_crate,
        shader_target,
        no_default_features,
        features,
        output_dir: output_dir.clone(),
        dry_run,
    };

    // let mut file = std::fs::File::create(dir.join("build-manifest.json")).unwrap();
    // file.write_all(&serde_json::to_vec(&shaders).unwrap());
    // UNWRAP: safe because we know this always serializes
    let arg = serde_json::to_string_pretty(&spirv_builder_args).unwrap();
    log::info!("using spirv-builder-cli arg: {arg}");

    // Call spirv-builder-cli to compile the shaders.
    let output = std::process::Command::new(&spirv_builder_cli_path)
        .arg(arg)
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .output()
        .unwrap();
    assert!(output.status.success(), "build failed");

    let spirv_manifest = output_dir.join("spirv-manifest.json");
    if spirv_manifest.is_file() {
        log::debug!(
            "successfully built shaders, raw manifest is at '{}'",
            spirv_manifest.display()
        );
    } else {
        log::error!("missing raw manifest '{}'", spirv_manifest.display());
        panic!("missing raw manifest");
    }

    let shaders: Vec<ShaderModule> =
        serde_json::from_reader(std::fs::File::open(&spirv_manifest).unwrap()).unwrap();

    let mut linkage: Vec<_> = shaders
        .into_iter()
        .map(
            |ShaderModule {
                 entry,
                 path: filepath,
             }| {
                let path = output_dir.join(filepath.file_name().unwrap());
                if !dry_run {
                    std::fs::copy(filepath, &path).unwrap();
                }
                Linkage::new(entry, path)
            },
        )
        .collect();

    // Write the shader manifest json file
    if !dry_run {
        if let Some(manifest_path) = output_manifest {
            // Sort the contents so the output is deterministic
            linkage.sort();
            // UNWRAP: safe because we know this always serializes
            let json = serde_json::to_string_pretty(&linkage).unwrap();
            let mut file = std::fs::File::create(&manifest_path).unwrap_or_else(|e| {
                log::error!(
                    "could not create shader manifest file '{}': {e}",
                    manifest_path.display(),
                );
                panic!("{e}")
            });
            file.write_all(json.as_bytes()).unwrap_or_else(|e| {
                log::error!(
                    "could not write shader manifest file '{}': {e}",
                    manifest_path.display(),
                );
                panic!("{e}")
            });

            log::info!("wrote manifest to '{}'", manifest_path.display());
        }
    }

    Ok(())
}
