use std::path::{Path, PathBuf};

use crate::cli::Cli;
use crate::project;
use crate::selection::select_sketch_directory;

pub(crate) fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

pub(crate) fn canonical_display_path(path: &Path) -> String {
    path.canonicalize()
        .map(|resolved| {
            let text = display_path(&resolved);
            #[cfg(windows)]
            {
                text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
            }
            #[cfg(not(windows))]
            {
                text
            }
        })
        .unwrap_or_else(|_| display_path(path))
}

pub(crate) fn detect_local_fastled_path() -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    project::find_fastled_repo_upwards(&cwd, 10).map(|path| canonical_display_path(&path))
}

pub(crate) fn resolve_compile_directory(cli: &Cli) -> Result<Option<String>, String> {
    let cwd = std::env::current_dir()
        .map_err(|err| format!("could not determine current directory: {err}"))?;

    let Some(directory) = &cli.directory else {
        if project::looks_like_sketch_directory(&cwd, false) {
            return Ok(Some(display_path(&cwd)));
        }
        let cwd_is_fastled = project::is_fastled_repo(&cwd);
        return select_sketch_directory(
            project::find_sketches(&cwd),
            cwd_is_fastled,
            cli.no_interactive,
        );
    };

    let provided_path = PathBuf::from(directory);
    if provided_path.is_file() {
        let parent = provided_path.parent().map(PathBuf::from);
        if let Some(parent) = parent {
            if project::looks_like_sketch_directory(&parent, false) {
                return Ok(Some(canonical_display_path(&parent)));
            }
        }
        return Ok(Some(directory.clone()));
    }

    if provided_path.exists() {
        return Ok(Some(canonical_display_path(&provided_path)));
    }

    project::find_sketch_by_partial_name(directory, &cwd)
        .map(|matched| display_path(&matched))
        .map(Some)
        .map_err(|err| err.to_string())
}
