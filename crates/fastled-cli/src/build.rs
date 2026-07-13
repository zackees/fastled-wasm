//! Build orchestration for WASM compilation.
//!
//! The Rust CLI owns the build backend directly. Python remains a compatibility
//! API layer and is not part of the primary compile path.

use crate::cli::LinkMode;

pub use crate::wasm_build::{run_build, run_build_streaming, BuildMode, BuildRequest, BuildResult};

pub(crate) fn effective_link_mode(build_mode: BuildMode, requested: LinkMode) -> LinkMode {
    if build_mode == BuildMode::Release && requested == LinkMode::Dynamic {
        LinkMode::Static
    } else {
        requested
    }
}

pub(crate) fn dynamic_linking_release_fallback(build_mode: BuildMode, requested: LinkMode) -> bool {
    build_mode == BuildMode::Release && requested == LinkMode::Dynamic
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_dynamic_is_normalized_to_static() {
        assert_eq!(
            effective_link_mode(BuildMode::Release, LinkMode::Dynamic),
            LinkMode::Static
        );
        assert!(dynamic_linking_release_fallback(
            BuildMode::Release,
            LinkMode::Dynamic
        ));
    }

    #[test]
    fn dynamic_is_preserved_in_non_release_modes() {
        for mode in [BuildMode::Debug, BuildMode::Quick] {
            assert_eq!(
                effective_link_mode(mode, LinkMode::Dynamic),
                LinkMode::Dynamic
            );
            assert!(!dynamic_linking_release_fallback(mode, LinkMode::Dynamic));
        }
    }
}
