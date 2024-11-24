//! Dummy module just to get everything to compile,
//! will be replaced with code from actual `spirv-builder` when running `cargo-gpu`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct CompileResult {
    pub entry_points: Vec<String>,
    pub module: ModuleResult,
}

pub enum MetadataPrintout {
    None,
    DependencyOnly,
    Full,
}

pub enum ModuleResult {
    SingleModule(PathBuf),
    MultiModule(BTreeMap<String, PathBuf>),
}

pub struct SpirvBuilder;

impl SpirvBuilder {
    pub fn new(_path_to_crate: impl AsRef<Path>, _target: impl Into<String>) -> Self {
        Self
    }

    pub fn build(self) -> Result<CompileResult, SpirvBuilderError> {
        Err(SpirvBuilderError)
    }

    /// Whether to print build.rs cargo metadata (e.g. cargo:rustc-env=var=val). Defaults to [`MetadataPrintout::Full`].
    pub fn print_metadata(self, _v: MetadataPrintout) -> Self {
        self
    }

    pub fn multimodule(self, _v: bool) -> Self {
        self
    }

    pub fn rustc_codegen_spirv_location(self, _path_to_dylib: impl AsRef<std::path::Path>) -> Self {
        self
    }

    pub fn shader_crate_default_features(self, _default_features: bool) -> Self {
        self
    }

    pub fn shader_crate_features(self, _features: impl IntoIterator<Item = String>) -> Self {
        self
    }

    pub fn target_spec(self, _p: impl AsRef<Path>) -> Self {
        self
    }
}

#[derive(Debug)]
pub struct SpirvBuilderError;
