//! Compiled from `def/workspace/main.lfy`: loading a project from disk and binding it.
//!
//! [`load`] reads the project layout from `elfie.json`, discovers every `.lfy` file under
//! the source directory and under every package root, follows every `use` to the file it
//! names, orders the files so each comes after the files it uses, and binds them once.
//! Loading never stops for a failure: every failure is a [`LoadProblem`] or a `Problem`
//! and the rest of the project still loads. The targets are read from the program itself,
//! after binding: every `ace const` of a project file holding a `Target` of the package
//! `elfie` gives one. [`change`] gives a workspace in which one file is read as other
//! text, by loading the same root again with that replacement in place.
//!
//! Loading is in the submodule `load`, the targets it reads from the bound program in
//! `targets`, and the replacement of one file in `change`. The paths all three speak in
//! are here, because all three speak in them.

pub mod data; // @lfy def/workspace/data.lfy:Workspace

pub use data::*;

mod change;
mod load;
mod targets;

#[cfg(test)]
mod change_tests;
#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod load_tests;

pub use change::change;
pub use load::load;

/// `Path::extension` of a source file.
const EXTENSION: &str = "lfy"; // @lfy def/workspace/main.lfy:load
/// The identifier of the package that holds the standard library: it is in the program
/// whether or not `elfie.json` names it.
const LIBRARY_PACKAGE: &str = "elfie"; // @lfy def/workspace/main.lfy:load

/// A directory as `elfie.json` spells it, without a trailing slash.
fn trim_directory(directory: &str) -> String {
    let trimmed = directory.trim_end_matches('/');
    if trimmed.is_empty() {
        ".".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Whether a directory is the root itself.
fn is_root(directory: &str) -> bool {
    directory.is_empty() || directory == "."
}

/// Whether a path is under a directory, both relative to the root.
fn under(path: &str, directory: &str) -> bool {
    is_root(directory)
        || path
            .strip_prefix(directory)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// A path under a directory, both relative to the root.
fn join(directory: &str, name: &str) -> String {
    if is_root(directory) {
        name.to_string()
    } else if name.is_empty() {
        directory.to_string()
    } else {
        format!("{directory}/{name}")
    }
}

/// The directory holding a file, relative to the root; empty for a file at the root.
fn directory_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(directory, _)| directory)
}

/// `Path::extension`: what follows the last dot of the last segment, without the dot;
/// `None` when the name has no dot or only a leading one.
// @lfy def/workspace/main.lfy:load
fn extension(path: &str) -> Option<&str> {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rfind('.') {
        Some(0) | None => None,
        Some(dot) => Some(&name[dot + 1..]),
    }
}

/// Whether a path names a file of the program: one whose extension is `lfy`.
// @lfy def/workspace/main.lfy:load
fn is_source(path: &str) -> bool {
    extension(path) == Some(EXTENSION)
}

/// `Path::normalize`: a path with every `.` dropped and every `..` applied to the segment
/// before it, never looking at the disk. An absolute path stays absolute, and `..` at the
/// root is dropped because the root has no parent.
// @lfy def/workspace/main.lfy:load
fn normalize(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => match segments.last() {
                Some(&last) if last != ".." => {
                    segments.pop();
                }
                _ if absolute => {}
                _ => segments.push(".."),
            },
            segment => segments.push(segment),
        }
    }
    let joined = segments.join("/");
    match (absolute, joined.is_empty()) {
        (true, _) => format!("/{joined}"),
        (false, true) => ".".to_string(),
        (false, false) => joined,
    }
}
