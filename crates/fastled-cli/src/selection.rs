use std::io::Write;
use std::path::{Path, PathBuf};

use crate::project;
use crate::DEFAULT_EXAMPLE;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptChoice {
    Selected(String),
    Narrowed(Vec<String>),
    Retry,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SketchSelection {
    Selected(String),
    Prompt(Vec<String>),
    None,
}

fn option_basename(option: &str) -> &str {
    Path::new(option)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(option)
}

fn fuzzy_match_options(input: &str, options: &[String]) -> Vec<String> {
    let basenames: Vec<String> = options
        .iter()
        .map(|option| option_basename(option).to_string())
        .collect();
    let basename_refs: Vec<&str> = basenames.iter().map(String::as_str).collect();
    let fuzzy_matches = project::best_sketch_match(input, &basename_refs);
    if fuzzy_matches.is_empty() {
        return Vec::new();
    }

    let mut results = Vec::new();
    for (index, basename) in basenames.iter().enumerate() {
        if fuzzy_matches.iter().any(|candidate| candidate == basename) {
            results.push(options[index].clone());
        }
    }
    results
}

pub fn resolve_prompt_choice(
    input: &str,
    options: &[String],
    default_index: usize,
) -> PromptChoice {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return PromptChoice::Selected(options[default_index].clone());
    }

    if let Ok(index) = trimmed.parse::<usize>() {
        if (1..=options.len()).contains(&index) {
            return PromptChoice::Selected(options[index - 1].clone());
        }
    }

    if let Some(exact) = options
        .iter()
        .find(|option| option.eq_ignore_ascii_case(trimmed))
    {
        return PromptChoice::Selected(exact.clone());
    }

    let input_lower = trimmed.to_lowercase();
    let partial_matches: Vec<String> = options
        .iter()
        .filter(|option| option.to_lowercase().contains(&input_lower))
        .cloned()
        .collect();
    match partial_matches.len() {
        1 => return PromptChoice::Selected(partial_matches[0].clone()),
        n if n > 1 => return PromptChoice::Narrowed(partial_matches),
        _ => {}
    }

    let fuzzy_matches = fuzzy_match_options(trimmed, options);
    match fuzzy_matches.len() {
        1 => PromptChoice::Selected(fuzzy_matches[0].clone()),
        n if n > 1 => PromptChoice::Narrowed(fuzzy_matches),
        _ => PromptChoice::Retry,
    }
}

pub fn prepare_sketch_selection(
    mut sketch_directories: Vec<PathBuf>,
    cwd_is_fastled: bool,
    is_followup: bool,
) -> SketchSelection {
    if cwd_is_fastled {
        sketch_directories.retain(|path| {
            let text = path.to_string_lossy().replace('\\', "/");
            !matches!(text.as_str(), "src" | "dev" | "tests")
        });
    }

    match sketch_directories.len() {
        0 => SketchSelection::None,
        1 => SketchSelection::Selected(crate::paths_util::display_path(&sketch_directories[0])),
        _ if !is_followup && sketch_directories.len() > 4 => SketchSelection::None,
        _ => SketchSelection::Prompt(
            sketch_directories
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
        ),
    }
}

pub(crate) fn prompt_for_choice(
    options: &[String],
    prompt: &str,
    default_index: usize,
) -> Result<String, String> {
    if options.is_empty() {
        return Err("no options available".to_string());
    }
    if options.len() == 1 {
        return Ok(options[0].clone());
    }

    let mut current_options = options.to_vec();
    let mut current_prompt = prompt.to_string();
    let mut current_default = default_index.min(current_options.len() - 1);

    loop {
        println!("\n{current_prompt}");
        for (index, option) in current_options.iter().enumerate() {
            if index == current_default {
                println!("  [{}]: [{}]", index + 1, option);
            } else {
                println!("  [{}]: {}", index + 1, option);
            }
        }

        let default_option = &current_options[current_default];
        print!("\nEnter number or name (default: [{default_option}]): ");
        std::io::stdout()
            .flush()
            .map_err(|err| format!("failed to flush prompt: {err}"))?;

        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .map_err(|err| format!("failed to read selection: {err}"))?;

        match resolve_prompt_choice(&input, &current_options, current_default) {
            PromptChoice::Selected(choice) => return Ok(choice),
            PromptChoice::Narrowed(matches) => {
                let query = input.trim();
                let is_partial = current_options
                    .iter()
                    .filter(|option| option.to_lowercase().contains(&query.to_lowercase()))
                    .count()
                    > 1;
                current_prompt = if is_partial {
                    format!("Multiple partial matches for '{query}':")
                } else {
                    format!("Multiple fuzzy matches for '{query}':")
                };
                current_options = matches;
                current_default = 0;
            }
            PromptChoice::Retry => {
                println!("No match found for '{}'. Please try again.", input.trim());
            }
        }
    }
}

pub(crate) fn prompt_for_example(repo_root: &Path) -> Result<String, String> {
    let examples = project::collect_examples(&repo_root.join("examples"));
    if examples.is_empty() {
        return Err(format!(
            "no examples found in FastLED repo {}",
            repo_root.display()
        ));
    }
    let default_index = examples
        .iter()
        .position(|example| example.eq_ignore_ascii_case(DEFAULT_EXAMPLE))
        .unwrap_or(0);
    prompt_for_choice(&examples, "Available examples:", default_index)
}

pub(crate) fn select_sketch_directory(
    mut sketch_directories: Vec<PathBuf>,
    cwd_is_fastled: bool,
    no_interactive: bool,
) -> Result<Option<String>, String> {
    if cwd_is_fastled {
        sketch_directories.retain(|path| {
            let text = path.to_string_lossy().replace('\\', "/");
            !matches!(text.as_str(), "src" | "dev" | "tests")
        });
    }

    match sketch_directories.len() {
        0 => Ok(None),
        1 => Ok(Some(crate::paths_util::display_path(
            &sketch_directories[0],
        ))),
        _ if no_interactive => Err(
            "multiple sketch directories found; specify one explicitly when using --no-interactive"
                .to_string(),
        ),
        _ => {
            let options: Vec<String> = sketch_directories
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect();
            prompt_for_choice(&options, "Multiple Directories found, choose one:", 0).map(Some)
        }
    }
}
