//! Compiled from `def/workspace/data.lfy`: the data of a loaded project.
//!
//! A [`Workspace`] is one project, loaded and bound. Its [`File`]s are the sources the
//! model was bound from, in bind order; its [`Package`]s and [`Target`]s come from
//! `elfie.json`; and its problems are every [`LoadProblem`] that arose before there was a
//! node to point at, followed by every [`Problem`] of the model.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use crate::model::{EntityId, Model, Problem, Source};
use crate::parser::Tree;

/// Something the generated code needs from the target's own ecosystem.
// @lfy def/workspace/data.lfy:NativeDependency
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeDependency {
    /// The name that ecosystem knows it by.
    pub identifier: String, // @lfy def/workspace/data.lfy:NativeDependency.identifier
    /// The ecosystem that the dependency is in.
    pub ecosystem: String, // @lfy def/workspace/data.lfy:NativeDependency.ecosystem
    /// The version the project asks for, spelled as that ecosystem spells it; `None` when
    /// it pins none.
    pub version: Option<String>, // @lfy def/workspace/data.lfy:NativeDependency.version
}

/// Elfie source loaded from outside the project.
// @lfy def/workspace/data.lfy:Package
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// The name a `Use` refers to it by.
    pub identifier: String, // @lfy def/workspace/data.lfy:Package.identifier
    /// Its directory, relative to [`Workspace::root`].
    pub root: String, // @lfy def/workspace/data.lfy:Package.root
    /// What code generated from it requires.
    pub native_dependencies: Vec<NativeDependency>, // @lfy def/workspace/data.lfy:Package.nativeDependencies
}

/// One `.lfy` file in the program, ready to bind.
///
/// `File extends Source`: the source itself lives in [`Model::sources`] at
/// [`File::source`], which is also the file's position in [`Workspace::files`].
// @lfy def/workspace/data.lfy:File
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// The file's path relative to [`Workspace::root`], with forward slashes; equal to the
    /// `Source::path` it was bound as.
    pub path: String, // @lfy def/workspace/data.lfy:File
    /// The package it came from, as an index into [`Workspace::packages`]; `None` for the
    /// project's own source.
    pub package: Option<usize>, // @lfy def/workspace/data.lfy:File.package
    /// The `Source` this file extends, as an index into [`Model::sources`].
    pub source: usize, // @lfy def/workspace/data.lfy:File
}

/// One thing the project is compiled into.
// @lfy def/workspace/data.lfy:Target
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The name the project calls it.
    pub identifier: String, // @lfy def/workspace/data.lfy:Target.identifier
    /// The trait an entity carries to be built for this target.
    pub marker: EntityId, // @lfy def/workspace/data.lfy:Target.marker
    /// The package that declares it, and the guidance for building against it, as an
    /// index into [`Workspace::packages`].
    pub package: usize, // @lfy def/workspace/data.lfy:Target.package
    /// Where its generated code is written, relative to [`Workspace::root`].
    pub output_directory: String, // @lfy def/workspace/data.lfy:Target.outputDirectory
}

/// Something that went wrong before there was a node to point at.
// @lfy def/workspace/data.lfy:LoadProblem
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadProblem {
    /// What could not be read, relative to [`Workspace::root`]; `None` when the project
    /// itself is at fault.
    pub path: Option<String>, // @lfy def/workspace/data.lfy:LoadProblem.path
    /// What and why.
    pub message: String, // @lfy def/workspace/data.lfy:LoadProblem.message
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
// @lfy def/workspace/data.lfy:Workspace.problems
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
// @lfy def/workspace/data.lfy:Workspace
#[derive(Debug, Clone, PartialEq)]
pub struct Workspace {
    /// The project's directory, as it was given to `load`.
    // Decision: the definition types `root` as a string; it is kept as the `PathBuf` that
    // `load` received so every file path in the workspace can be joined to it directly.
    pub root: PathBuf, // @lfy def/workspace/data.lfy:Workspace.root
    /// The project's name.
    pub name: String, // @lfy def/workspace/data.lfy:Workspace.name
    /// Where the project's own source lives, relative to [`Workspace::root`].
    pub source_directory: String, // @lfy def/workspace/data.lfy:Workspace.sourceDirectory
    /// Where generated code is written by default, relative to [`Workspace::root`].
    pub output_directory: String, // @lfy def/workspace/data.lfy:Workspace.outputDirectory
    /// Every file in the program, in the order they were bound.
    pub files: Vec<File>, // @lfy def/workspace/data.lfy:Workspace.files
    /// The program.
    pub model: Model, // @lfy def/workspace/data.lfy:Workspace.model
    /// The packages `elfie.json` names as dependencies, in the order they were read.
    pub packages: Vec<Package>, // @lfy def/workspace/data.lfy:Package
    /// What the project is compiled into.
    pub targets: Vec<Target>, // @lfy def/workspace/data.lfy:Workspace.targets
    /// What code generated from the project's own source requires, as its `elfie.json`
    /// names it.
    pub native_dependencies: Vec<NativeDependency>, // @lfy def/workspace/data.lfy:Workspace.nativeDependencies
    /// Every [`LoadProblem`], then every [`Problem`] of [`Workspace::model`].
    pub problems: Vec<WorkspaceProblem>, // @lfy def/workspace/data.lfy:Workspace.problems
    /// The files `change` replaced, by path, holding the text each is read as instead of
    /// what is on disk. A replacement stands until the same path is given again.
    pub overlays: BTreeMap<String, String>, // @lfy def/workspace/main.lfy:change
}

impl Workspace {
    /// The file at a path, when it is in the program.
    pub fn file(&self, path: &str) -> Option<&File> {
        self.files.iter().find(|file| file.path == path)
    }

    /// The `Source` a file extends: `File extends Source`, so a file's `path`, `tree`,
    /// and `uses` are the source's.
    // @lfy def/workspace/data.lfy:File
    pub fn source(&self, file: &File) -> &Source {
        &self.model.sources[file.source]
    }

    /// `File.tree`: the parse of the file.
    // @lfy def/workspace/main.lfy:load
    pub fn tree(&self, file: &File) -> &Tree {
        &self.source(file).tree
    }

    /// `File.uses`: the path each `Use` in the tree refers to, in the order the uses
    /// appear; `None` where it refers to nothing.
    // @lfy def/workspace/main.lfy:load
    pub fn uses(&self, file: &File) -> &[Option<String>] {
        &self.source(file).uses
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
