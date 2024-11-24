//! This program builds rust-gpu shader crates and writes generated spv files
//! into the main source repo.
use spirv_builder::{CompileResult, MetadataPrintout, ModuleResult, SpirvBuilder};

use cargo_gpu::spirv_builder_cli::{Args, ShaderModule};

const RUSTC_NIGHTLY_CHANNEL: &str = "${CHANNEL}";

fn set_rustup_toolchain() {
    log::trace!(
        "setting RUSTUP_TOOLCHAIN = '{}'",
        RUSTC_NIGHTLY_CHANNEL.trim_matches('"')
    );
    std::env::set_var("RUSTUP_TOOLCHAIN", RUSTC_NIGHTLY_CHANNEL.trim_matches('"'));
}

fn main() {
    env_logger::builder().init();

    set_rustup_toolchain();

    let args = std::env::args().collect::<Vec<_>>();
    log::debug!(
        "running spirv-builder-cli from '{}'",
        std::env::current_dir().unwrap().display()
    );
    let args = serde_json::from_str(&args[1]).unwrap();
    log::debug!("compiling with args: {args:#?}");
    let Args {
        dylib_path,
        shader_crate,
        shader_target,
        path_to_target_spec,
        no_default_features,
        features,
        output_dir,
    } = args;

    let CompileResult {
        entry_points,
        module,
    } = {
        let mut builder = SpirvBuilder::new(shader_crate, &shader_target)
            .rustc_codegen_spirv_location(dylib_path)
            .target_spec(path_to_target_spec)
            .print_metadata(MetadataPrintout::None)
            .multimodule(true);

        if no_default_features {
            log::info!("setting cargo --no-default-features");
            builder = builder.shader_crate_default_features(false);
        }
        if !features.is_empty() {
            log::info!("setting --features {features:?}");
            builder = builder.shader_crate_features(features);
        }

        builder.build().unwrap()
    };

    let dir = output_dir;
    let mut shaders = vec![];
    match module {
        ModuleResult::MultiModule(modules) => {
            assert!(!modules.is_empty(), "No shader modules to compile");
            for (entry, filepath) in modules.into_iter() {
                shaders.push(ShaderModule::new(entry, filepath));
            }
        }
        ModuleResult::SingleModule(filepath) => {
            for entry in entry_points {
                shaders.push(ShaderModule::new(entry, filepath.clone()));
            }
        }
    }

    use std::io::Write;
    let mut file = std::fs::File::create(dir.join("spirv-manifest.json")).unwrap();
    file.write_all(&serde_json::to_vec(&shaders).unwrap())
        .unwrap();
}
