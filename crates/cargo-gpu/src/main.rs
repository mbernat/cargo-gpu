//! Rust GPU shader crate builder.
//!
//! This program manages installations of `spirv-builder-cli` and `rustc_codegen_spirv`.
//! It uses these tools to compile Rust code into SPIR-V.
//!
//! # How it works
//!
//! In order to build shader crates, we must invoke cargo/rustc with a special backend
//! that performs the SPIR-V code generation. This backend is a dynamic library known
//! by its project name `rustc_codegen_spirv`. The name of the artifact itself is
//! OS-dependent.
//!
//! There are a lot of special flags to wrangle and so we use a command line program
//! that wraps `cargo` to perform the building of shader crates. This cli program is
//! called `spirv-builder-cli`, which itself is a cli wrapper around the `spirv-builder`
//! library.
//!
//! ## Where the binaries are
//!
//! `cargo-gpu` maintains different versions `spirv-builder-cli` and `rustc_codegen_spirv`
//! in a cache dir. The location is OS-dependent, for example on macOS it's in
//! `~/Library/Caches/rust-gpu`. Specific versions live inside the cache dir, prefixed
//! by their `spirv-builder` cargo dependency and rust toolchain pair.
//!
//! Building a specific "binary pair" of `spirv-builder-cli` and `rustc_codegen_spirv`
//! happens when there is no existing pair that matches the computed prefix, or if
//! a force rebuild is specified on the command line.
//!
//! ## Building the "binary pairs"
//!
//! The source of `spirv-builder-cli` lives alongside this source file, in crate that
//! is not included in the workspace. That same source code is also included statically
//! in **this** source file.
//!
//! When `spirv-builder-cli` needs to be built, a new directory is created in the cache
//! where the source to `spirv-builder-cli` is copied into, containing the specific cargo
//! dependency for `spirv-builder` and the matching rust toolchain channel.
//!
//! Then `cargo` is invoked in that cache directory to build the pair of artifacts, which
//! are then put into the top level of that cache directory.
//!
//! This pair of artifacts is then used to build shader crates.
//!
//! ## Building shader crates
//!
//! `cargo-gpu` takes a path to a shader crate to build, as well as a path to a directory
//! to put the compiled `spv` source files. It also takes a path to an output mainifest
//! file where all shader entry points will be mapped to their `spv` source files. This
//! manifest file can be used by build scripts (`build.rs` files) to generate linkage or
//! conduct other post-processing, like converting the `spv` files into `wgsl` files,
//! for example.
use std::io::Write;

use cargo_gpu::{spirv_builder_cli::ShaderModule, Linkage};
use clap::{Parser, Subcommand};

const SPIRV_BUILDER_CLI_CARGO_TOML: &str = include_str!("../../spirv-builder-cli/Cargo.toml");
const SPIRV_BUILDER_CLI_MAIN: &str = include_str!("../../spirv-builder-cli/src/main.rs");
const SPIRV_BUILDER_CLI_LIB: &str = include_str!("lib.rs");
const SPIRV_BUILDER_FILES: &[(&str, &str)] = &[
    ("Cargo.toml", SPIRV_BUILDER_CLI_CARGO_TOML),
    ("src/main.rs", SPIRV_BUILDER_CLI_MAIN),
    ("src/lib.rs", SPIRV_BUILDER_CLI_LIB),
];

const SPIRV_STD_TOOLCHAIN_PAIRS: &[(&str, &str)] = &[("0.10", "nightly-2024-04-24")];

const TARGET_SPECS: &[(&str, &str)] = &[
    (
        "spirv-unknown-opengl4.0.json",
        include_str!("../target-specs/spirv-unknown-opengl4.0.json"),
    ),
    (
        "spirv-unknown-opengl4.1.json",
        include_str!("../target-specs/spirv-unknown-opengl4.1.json"),
    ),
    (
        "spirv-unknown-opengl4.2.json",
        include_str!("../target-specs/spirv-unknown-opengl4.2.json"),
    ),
    (
        "spirv-unknown-opengl4.3.json",
        include_str!("../target-specs/spirv-unknown-opengl4.3.json"),
    ),
    (
        "spirv-unknown-opengl4.5.json",
        include_str!("../target-specs/spirv-unknown-opengl4.5.json"),
    ),
    (
        "spirv-unknown-spv1.0.json",
        include_str!("../target-specs/spirv-unknown-spv1.0.json"),
    ),
    (
        "spirv-unknown-spv1.1.json",
        include_str!("../target-specs/spirv-unknown-spv1.1.json"),
    ),
    (
        "spirv-unknown-spv1.2.json",
        include_str!("../target-specs/spirv-unknown-spv1.2.json"),
    ),
    (
        "spirv-unknown-spv1.3.json",
        include_str!("../target-specs/spirv-unknown-spv1.3.json"),
    ),
    (
        "spirv-unknown-spv1.4.json",
        include_str!("../target-specs/spirv-unknown-spv1.4.json"),
    ),
    (
        "spirv-unknown-spv1.5.json",
        include_str!("../target-specs/spirv-unknown-spv1.5.json"),
    ),
    (
        "spirv-unknown-vulkan1.0.json",
        include_str!("../target-specs/spirv-unknown-vulkan1.0.json"),
    ),
    (
        "spirv-unknown-vulkan1.1.json",
        include_str!("../target-specs/spirv-unknown-vulkan1.1.json"),
    ),
    (
        "spirv-unknown-vulkan1.1spv1.4.json",
        include_str!("../target-specs/spirv-unknown-vulkan1.1spv1.4.json"),
    ),
    (
        "spirv-unknown-vulkan1.2.json",
        include_str!("../target-specs/spirv-unknown-vulkan1.2.json"),
    ),
];

/// Cargo dependency for `spirv-builder` and the rust toolchain channel.
#[derive(Debug, Clone)]
struct Spirv {
    dep: String,
    channel: String,
}

impl core::fmt::Display for Spirv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        format!("{}+{}", self.dep, self.channel).fmt(f)
    }
}

impl Spirv {
    /// Returns a string suitable to use as a directory.
    ///
    /// Created from the spirv-builder source dep and the rustc channel.
    fn to_dirname(&self) -> String {
        self.to_string()
            .replace([std::path::MAIN_SEPARATOR, '.', ':', '@', '='], "_")
            .split(['{', '}', ' ', '\n', '"', '\''])
            .collect::<Vec<_>>()
            .concat()
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

    fn ensure_version_channel_compatibility(&self) {
        for (version, channel) in SPIRV_STD_TOOLCHAIN_PAIRS.iter() {
            if version.starts_with(&self.dep) && channel != &self.channel {
                panic!("expected spirv-std version to be matched with rust toolchain channel {channel}");
            }
        }
    }

    /// Use `rustup` to install the toolchain and components, if not already installed.
    ///
    /// Pretty much runs:
    ///
    /// * rustup toolchain add nightly-2024-04-24
    /// * rustup component add --toolchain nightly-2024-04-24 rust-src rustc-dev llvm-tools
    fn ensure_toolchain_and_components_exist(&self) {
        // Check for the required toolchain
        let output = std::process::Command::new("rustup")
            .args(["toolchain", "list"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "could not list installed toolchains"
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout
            .split_whitespace()
            .any(|toolchain| toolchain.starts_with(&self.channel))
        {
            log::debug!("toolchain {} is already installed", self.channel);
        } else {
            let output = std::process::Command::new("rustup")
                .args(["toolchain", "add"])
                .arg(&self.channel)
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "could not install required toolchain"
            );
        }

        // Check for the required components
        let output = std::process::Command::new("rustup")
            .args(["component", "list", "--toolchain"])
            .arg(&self.channel)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "could not list installed components"
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let required_components = ["rust-src", "rustc-dev", "llvm-tools"];
        let installed_components = stdout.lines().collect::<Vec<_>>();
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
            let output = std::process::Command::new("rustup")
                .args(["component", "add", "--toolchain"])
                .arg(&self.channel)
                .args(["rust-src", "rustc-dev", "llvm-tools"])
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "could not install required components"
            );
        }
    }
}

fn target_spec_dir() -> std::path::PathBuf {
    let dir = cache_dir().join("target-specs");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[derive(Parser, Debug)]
struct Install {
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
}

impl Install {
    fn spirv_cli(&self) -> Spirv {
        Spirv {
            dep: self.spirv_builder.clone(),
            channel: self.rust_toolchain.clone(),
        }
    }

    fn write_source_files(&self) {
        const DUMMY_DEPENDENCY_LINE: &str = "spirv-builder = { path = \"../spirv-builder-dummy\" }";

        let cli = self.spirv_cli();
        let checkout = cli.cached_checkout_path();
        std::fs::create_dir_all(checkout.join("src")).unwrap();
        for (filename, contents) in SPIRV_BUILDER_FILES.iter() {
            log::debug!("writing {filename}");

            let lines: Vec<_> = contents
                .lines()
                .map(|line| {
                    if line == DUMMY_DEPENDENCY_LINE {
                        format!("spirv-builder = {}", &cli.dep)
                    } else {
                        line.replace("${CHANNEL}", &cli.channel).into()
                    }
                })
                .collect();

            let path = checkout.join(filename);
            let mut file = std::fs::File::create(&path).unwrap();
            file.write_all(lines.join("\n").as_bytes()).unwrap();
        }
    }

    fn write_target_spec_files(&self) {
        for (filename, contents) in TARGET_SPECS.iter() {
            let path = target_spec_dir().join(filename);
            if !path.is_file() || self.force_spirv_cli_rebuild {
                let mut file = std::fs::File::create(&path).unwrap();
                file.write_all(contents.as_bytes()).unwrap();
            }
        }
    }

    // Install the binary pair and return the paths, (dylib, cli).
    fn run(&self) -> (std::path::PathBuf, std::path::PathBuf) {
        // Ensure the cache dir exists
        let cache_dir = cache_dir();
        std::fs::create_dir_all(&cache_dir).unwrap_or_else(|e| {
            log::error!(
                "could not create cache directory '{}': {e}",
                cache_dir.display()
            );
            panic!("could not create cache dir");
        });

        let spirv_version = self.spirv_cli();
        spirv_version.ensure_version_channel_compatibility();
        spirv_version.ensure_toolchain_and_components_exist();

        let checkout = spirv_version.cached_checkout_path();
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
            log::info!(
                "cargo-gpu artifacts are already installed in '{}'",
                checkout.display()
            );
        }

        if dest_dylib_path.is_file() && dest_cli_path.is_file() && !self.force_spirv_cli_rebuild {
            log::info!("...and so we are aborting the install step.");
        } else {
            log::debug!(
                "writing spirv-builder-cli source files into '{}'",
                checkout.display()
            );
            self.write_source_files();
            self.write_target_spec_files();

            log::debug!("building artifacts");
            let output = std::process::Command::new("cargo")
                .current_dir(&checkout)
                .arg(format!("+{}", spirv_version.channel))
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

#[derive(Parser, Debug)]
struct Build {
    #[clap(flatten)]
    install: Install,

    /// Directory containing the shader crate to compile.
    #[clap(long, default_value = "./")]
    shader_crate: std::path::PathBuf,

    /// Shader target.
    #[clap(long, default_value = "spirv-unknown-vulkan1.2")]
    shader_target: String,

    /// Set cargo default-features.
    #[clap(long)]
    no_default_features: bool,

    /// Set cargo features.
    #[clap(long)]
    features: Vec<String>,

    /// Path to the output directory for the compiled shaders.
    #[clap(long, short, default_value = "./")]
    output_dir: std::path::PathBuf,
}

impl Build {
    fn run(&mut self) {
        let (dylib_path, spirv_builder_cli_path) = self.install.run();

        // Ensure the shader output dir exists
        log::debug!("ensuring output-dir '{}' exists", self.output_dir.display());
        std::fs::create_dir_all(&self.output_dir).unwrap();
        self.output_dir = self.output_dir.canonicalize().unwrap();

        // Ensure the shader crate exists
        self.shader_crate = self.shader_crate.canonicalize().unwrap();
        assert!(
            self.shader_crate.exists(),
            "shader crate '{}' does not exist. (Current dir is '{}')",
            self.shader_crate.display(),
            std::env::current_dir().unwrap().display()
        );

        let spirv_builder_args = cargo_gpu::spirv_builder_cli::Args {
            dylib_path,
            shader_crate: self.shader_crate.clone(),
            shader_target: self.shader_target.clone(),
            path_to_target_spec: target_spec_dir().join(format!("{}.json", self.shader_target)),
            no_default_features: self.no_default_features,
            features: self.features.clone(),
            output_dir: self.output_dir.clone(),
        };

        // UNWRAP: safe because we know this always serializes
        let arg = serde_json::to_string_pretty(&spirv_builder_args).unwrap();
        log::info!("using spirv-builder-cli arg: {arg}");

        // Call spirv-builder-cli to compile the shaders.
        let output = std::process::Command::new(spirv_builder_cli_path)
            .arg(arg)
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .output()
            .unwrap();
        assert!(output.status.success(), "build failed");

        let spirv_manifest = self.output_dir.join("spirv-manifest.json");
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
                    use relative_path::PathExt;
                    let path = self.output_dir.join(filepath.file_name().unwrap());
                    std::fs::copy(&filepath, &path).unwrap();
                    let path_relative_to_shader_crate =
                        path.relative_to(&self.shader_crate).unwrap().to_path("");
                    Linkage::new(entry, path_relative_to_shader_crate)
                },
            )
            .collect();

        // Write the shader manifest json file
        let manifest_path = self.output_dir.join("manifest.json");
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

        if spirv_manifest.is_file() {
            log::debug!(
                "removing spirv-manifest.json file '{}'",
                spirv_manifest.display()
            );
            std::fs::remove_file(spirv_manifest).unwrap();
        }
    }
}

#[derive(Parser)]
struct Toml {
    /// Path to a workspace or package Cargo.toml file.
    ///
    /// Must include a [[workspace | package].metadata.rust-gpu.build] section where
    /// arguments to `cargo gpu build` are listed.
    ///
    /// Path arguments like `output-dir` and `shader-manifest` must be relative to
    /// the location of the Cargo.toml file.
    ///
    /// Example:
    ///
    /// ```toml
    ///     [package.metadata.rust-gpu.build.spirv-builder]
    ///     git = "https://github.com/Rust-GPU/rust-gpu.git"  
    ///     rev = "0da80f8"
    ///
    ///     [package.metadata.rust-gpu.build]
    ///     output-dir = "shaders"
    ///     shader-manifest = "shaders/manifest.json"
    /// ```
    ///
    /// Calling `cargo gpu toml {path/to/Cargo.toml}` with a Cargo.toml that
    /// contains the example above would compile the crate and place the compiled
    /// `.spv` files and manifest in a directory "shaders".
    #[clap(default_value = "./Cargo.toml", verbatim_doc_comment)]
    path: std::path::PathBuf,
}

impl Toml {
    fn run(&self) {
        // Find the path to the toml file to use
        let path = if self.path.is_file() && self.path.ends_with(".toml") {
            self.path.clone()
        } else {
            let path = self.path.join("Cargo.toml");
            if path.is_file() {
                path
            } else {
                log::error!("toml file '{}' is not a file", self.path.display());
                panic!("toml file '{}' is not a file", self.path.display());
            }
        };

        log::info!("using toml file '{}'", path.display());

        // Determine if this is a workspace's Cargo.toml or a crate's Cargo.toml
        let contents = std::fs::read_to_string(&path).unwrap();
        let toml: toml::Table = toml::from_str(&contents).unwrap();

        fn get_metadata_rustgpu_table<'a>(
            toml: &'a toml::Table,
            toml_type: &'static str,
        ) -> Option<&'a toml::Table> {
            let workspace = toml.get(toml_type)?.as_table()?;
            let metadata = workspace.get("metadata")?.as_table()?;
            metadata.get("rust-gpu")?.as_table()
        }

        let (toml_type, table) = if toml.contains_key("workspace") {
            let table = get_metadata_rustgpu_table(&toml, "workspace")
                .unwrap_or_else(|| {
                    panic!(
                        "toml file '{}' is missing a [workspace.metadata.rust-gpu] table",
                        path.display()
                    );
                })
                .clone();
            ("workspace", table)
        } else if toml.contains_key("package") {
            let mut table = get_metadata_rustgpu_table(&toml, "package")
                .unwrap_or_else(|| {
                    panic!(
                        "toml file '{}' is missing a [package.metadata.rust-gpu] table",
                        path.display()
                    );
                })
                .clone();
            // Ensure the package name is included as the shader-crate parameter
            if !table.contains_key("shader-crate") {
                table.insert(
                    "shader-crate".to_string(),
                    format!("{}", path.parent().unwrap().display()).into(),
                );
            }
            ("package", table)
        } else {
            panic!("toml file '{}' must describe a workspace containing [workspace.metadata.rust-gpu.build] or a describe a crate with [package.metadata.rust-gpu.build]", path.display());
        };
        log::info!(
            "building with [{toml_type}.metadata.rust-gpu.build] section of the toml file at '{}'",
            path.display()
        );
        log::debug!("table: {table:#?}");

        let mut parameters = table
            .get("build")
            .unwrap_or_else(|| panic!("toml is missing the 'build' table"))
            .as_table()
            .unwrap_or_else(|| {
                panic!("toml file's '{toml_type}.metadata.rust-gpu.build' property is not a table")
            })
            .into_iter()
            .flat_map(|(k, v)| match v {
                toml::Value::String(s) => [format!("--{k}"), s.clone()],
                _ => {
                    let mut value = String::new();
                    let ser = toml::ser::ValueSerializer::new(&mut value);
                    serde::Serialize::serialize(v, ser).unwrap();
                    [format!("--{k}"), value]
                }
            })
            .collect::<Vec<_>>();
        parameters.insert(0, "cargo-gpu".to_string());
        parameters.insert(1, "build".to_string());

        let working_directory = path.parent().unwrap();
        log::info!(
            "issuing cargo commands from the working directory '{}'",
            working_directory.display()
        );
        std::env::set_current_dir(working_directory).unwrap();

        log::debug!("build parameters: {parameters:#?}");
        if let Cli {
            command: Command::Build(mut build),
        } = Cli::parse_from(parameters)
        {
            log::debug!("build: {build:?}");
            build.run();
        } else {
            log::error!("parameters found in [{toml_type}.metadata.rust-gpu.build] were not parameters to `cargo gpu build`");
            panic!("could not determin build command");
        }
    }
}

#[derive(Subcommand)]
enum Command {
    /// Install rust-gpu compiler artifacts.
    Install(Install),

    /// Compile a shader crate to SPIR-V.
    Build(Build),

    /// Compile a shader crate according to the `cargo gpu build` parameters
    /// found in the given toml file.
    Toml(Toml),
}

#[derive(Parser)]
#[clap(author, version, about, subcommand_required = true)]
struct Cli {
    /// The command to run.
    #[clap(subcommand)]
    command: Command,
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

fn main() {
    env_logger::builder().init();

    let args = std::env::args()
        .filter(|p| {
            // Calling cargo-gpu as the cargo subcommand "cargo gpu" passes "gpu"
            // as the first parameter, which we want to ignore.
            p != "gpu"
        })
        .collect::<Vec<_>>();
    log::trace!("args: {args:?}");
    let cli = Cli::parse_from(args);

    match cli.command {
        Command::Install(install) => {
            let _ = install.run();
        }
        Command::Build(mut build) => build.run(),
        Command::Toml(toml) => toml.run(),
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn builder_from_params() {
        let shader_crate = std::path::PathBuf::from("../shader-crate-template");
        let output_dir = std::path::PathBuf::from("../shader-crate-template/shaders");
        let args = [
            "target/debug/cargo-gpu",
            "build",
            "--shader-crate",
            &format!("{}", shader_crate.display()),
            "--output-dir",
            &format!("{}", output_dir.display()),
        ];
        if let Cli {
            command: Command::Build(build),
        } = Cli::parse_from(args)
        {
            assert_eq!(shader_crate, build.shader_crate);
            assert_eq!(output_dir, build.output_dir);
        } else {
            panic!("was not a build command");
        }
    }
}
