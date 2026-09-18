//! Compiled from `def/workspace/data.lfy`: the data of a loaded project.
//!
//! A [`Workspace`] is one project, loaded and bound. Its [`File`]s are the sources the
//! model was bound from, in bind order; its [`Package`]s and [`Target`]s come from
//! `elfie.json`; and its problems are every [`LoadProblem`] that arose before there was a
//! node to point at, followed by every [`Problem`] of the model.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use crate::model::{EntityId, Model, Problem};

/// Something the generated code needs from the target's own ecosystem.
// @lfy def/workspace/data.lfy:4
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeDependency {
    /// The name that ecosystem knows it by.
    pub identifier: String, // @lfy def/workspace/data.lfy:5
    /// The ecosystem that the dependency is in.
    pub ecosystem: String, // @lfy def/workspace/data.lfy:6
    /// The version the project asks for, spelled as that ecosystem spells it; `None` when
    /// it pins none.
    pub version: Option<String>, // @lfy def/workspace/data.lfy:7
}

/// Elfie source loaded from outside the project.
// @lfy def/workspace/data.lfy:10
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// The name a `Use` refers to it by.
    pub identifier: String, // @lfy def/workspace/data.lfy:11
    /// Its directory, relative to [`Workspace::root`].
    pub root: String, // @lfy def/workspace/data.lfy:12
    /// What code generated from it requires.
    pub native_dependencies: Vec<NativeDependency>, // @lfy def/workspace/data.lfy:13
}

/// One `.lfy` file in the program, ready to bind.
///
/// `File extends Source`: the source itself lives in [`Model::sources`] at
/// [`File::source`], which is also the file's position in [`Workspace::files`].
// @lfy def/workspace/data.lfy:16
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// The file's path relative to [`Workspace::root`], with forward slashes; equal to the
    /// `Source::path` it was bound as.
    pub path: String, // @lfy def/workspace/data.lfy:16
    /// The package it came from, as an index into [`Workspace::packages`]; `None` for the
    /// project's own source.
    pub package: Option<usize>, // @lfy def/workspace/data.lfy:17
    /// The `Source` this file extends, as an index into [`Model::sources`].
    pub source: usize, // @lfy def/workspace/data.lfy:16
}

/// One thing the project is compiled into.
// @lfy def/workspace/data.lfy:20
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The name the project calls it.
    pub identifier: String, // @lfy def/workspace/data.lfy:21
    /// The trait an entity carries to be built for this target.
    pub marker: EntityId, // @lfy def/workspace/data.lfy:22
    /// The package that declares it, and the guidance for building against it, as an
    /// index into [`Workspace::packages`].
    pub package: usize, // @lfy def/workspace/data.lfy:23
    /// Where its generated code is written, relative to [`Workspace::root`].
    pub output_directory: String, // @lfy def/workspace/data.lfy:24
}

/// Something that went wrong before there was a node to point at.
// @lfy def/workspace/data.lfy:27
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadProblem {
    /// What could not be read, relative to [`Workspace::root`]; `None` when the project
    /// itself is at fault.
    pub path: Option<String>, // @lfy def/workspace/data.lfy:28
    /// What and why.
    pub message: String, // @lfy def/workspace/data.lfy:29
}

impl fmt::Display for LoadProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.path {
            Some(path) => write!(f, "{path}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

/// One entry of [`Workspace::problems`]: `LoadProblem | Problem`.
// @lfy def/workspace/data.lfy:41
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceProblem {
    Load(LoadProblem),
    Bind(Problem),
}

impl WorkspaceProblem {
    pub fn as_load(&self) -> Option<&LoadProblem> {
        match self {
            WorkspaceProblem::Load(problem) => Some(problem),
            WorkspaceProblem::Bind(_) => None,
        }
    }

    pub fn as_bind(&self) -> Option<&Problem> {
        match self {
            WorkspaceProblem::Bind(problem) => Some(problem),
            WorkspaceProblem::Load(_) => None,
        }
    }
}

impl fmt::Display for WorkspaceProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkspaceProblem::Load(problem) => problem.fmt(f),
            WorkspaceProblem::Bind(problem) => problem.fmt(f),
        }
    }
}

/// A project, loaded and bound.
// @lfy def/workspace/data.lfy:32
#[derive(Debug, Clone, PartialEq)]
pub struct Workspace {
    /// The project's directory, as it was given to `load`.
    // Decision: the definition types `root` as a string; it is kept as the `PathBuf` that
    // `load` received so every file path in the workspace can be joined to it directly.
    pub root: PathBuf, // @lfy def/workspace/data.lfy:33
    /// The project's name.
    pub name: String, // @lfy def/workspace/data.lfy:34
    /// Where the project's own source lives, relative to [`Workspace::root`].
    pub source_directory: String, // @lfy def/workspace/data.lfy:35
    /// Where generated code is written by default, relative to [`Workspace::root`].
    pub output_directory: String, // @lfy def/workspace/data.lfy:36
    /// Every file in the program, in the order they were bound.
    pub files: Vec<File>, // @lfy def/workspace/data.lfy:37
    /// The program.
    pub model: Model, // @lfy def/workspace/data.lfy:38
    /// The packages `elfie.json` names as dependencies, in the order they were read.
    pub packages: Vec<Package>, // @lfy def/workspace/data.lfy:10
    /// What the project is compiled into.
    pub targets: Vec<Target>, // @lfy def/workspace/data.lfy:39
    /// What code generated from the project's own source requires, as its `elfie.json`
    /// names it.
    pub native_dependencies: Vec<NativeDependency>, // @lfy def/workspace/data.lfy:40
    /// Every [`LoadProblem`], then every [`Problem`] of [`Workspace::model`].
    pub problems: Vec<WorkspaceProblem>, // @lfy def/workspace/data.lfy:41
    /// The files `change` replaced, by path, holding the text each is read as instead of
    /// what is on disk. A replacement stands until the same path is given again.
    pub overlays: BTreeMap<String, String>, // @lfy def/workspace/main.lfy:108
}

impl Workspace {
    /// The file at a path, when it is in the program.
    pub fn file(&self, path: &str) -> Option<&File> {
        self.files.iter().find(|file| file.path == path)
    }

    /// Every load problem, in the order it arose.
    pub fn load_problems(&self) -> impl Iterator<Item = &LoadProblem> {
        self.problems.iter().filter_map(WorkspaceProblem::as_load)
    }

    /// Every problem of the model, in node order.
    pub fn bind_problems(&self) -> impl Iterator<Item = &Problem> {
        self.problems.iter().filter_map(WorkspaceProblem::as_bind)
    }
}
