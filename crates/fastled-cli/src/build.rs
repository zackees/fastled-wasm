//! Build orchestration for WASM compilation.
//!
//! The Rust CLI owns the build backend directly. Python remains a compatibility
//! API layer and is not part of the primary compile path.

pub use crate::wasm_build::{run_build, BuildMode, BuildRequest, BuildResult};
