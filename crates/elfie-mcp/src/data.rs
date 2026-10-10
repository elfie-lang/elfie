//! Compiled from `def/mcp/data.lfy`: the data of the agent server.
//!
//! A [`Tool`] is one fn of `def/mcp/main.lfy` as the server lists it, with one
//! [`ToolArgument`] per parameter; a [`ToolResult`] is what a call returns; a [`Session`]
//! is one agent's connection, holding the program as of the last call and the stamp of
//! every file it was read from; a [`Binding`] is what one hosted session is bound to and a
//! [`Host`] the server that serves such sessions for the length of one compile or verify; a
//! [`GlobalReview`] is the last global review of a project, which says which units a review
//! found violated.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use elfie_core::generation::{self, Batch, Plan, Review, ReviewRequest, ReviewStatus, SourceMap};
use elfie_core::interpret::Program;
use elfie_core::workspace::Workspace;
use serde_json::Value;

/// One argument of a tool, as the server lists it.
// @lfy def/mcp/data.lfy:ToolArgument
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolArgument {
    /// As the parameter is named.
    pub name: String, // @lfy def/mcp/data.lfy:ToolArgument.name
    /// What it is for.
    pub description: String, // @lfy def/mcp/data.lfy:ToolArgument.description
    /// `string`, `number`, or `boolean`.
    pub ty: String, // @lfy def/mcp/data.lfy:ToolArgument.type
    /// Whether a call must give it.
    pub required: bool, // @lfy def/mcp/data.lfy:ToolArgument.required
}

impl ToolArgument {
    /// An argument of the type given, required unless `optional`.
    pub fn new(name: &str, description: &str, ty: &str, required: bool) -> ToolArgument {
        ToolArgument {
            name: name.to_string(),
            description: description.to_string(),
            ty: ty.to_string(),
            required,
        }
    }
}

/// One tool as the server lists it.
// @lfy def/mcp/data.lfy:Tool
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tool {
    /// What a call names it by: `elfie_` followed by the fn's identifier.
    pub name: String, // @lfy def/mcp/data.lfy:Tool.name
    /// What it does: the fn's definition.
    pub description: String, // @lfy def/mcp/data.lfy:Tool.description
    /// One per argument a call may give, in parameter order.
    pub arguments: Vec<ToolArgument>, // @lfy def/mcp/data.lfy:Tool.arguments
}

/// What a call returns.
// @lfy def/mcp/data.lfy:ToolResult
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    /// Plain text for the agent to read.
    pub text: String, // @lfy def/mcp/data.lfy:ToolResult.text
    /// Whether the call failed.
    pub is_error: bool, // @lfy def/mcp/data.lfy:ToolResult.isError
}

impl ToolResult {
    /// A result that succeeded with the text.
    pub fn text(text: impl Into<String>) -> ToolResult {
        ToolResult {
            text: text.into(),
            is_error: false,
        }
    }

    /// A result that failed with the text.
    pub fn error(text: impl Into<String>) -> ToolResult {
        ToolResult {
            text: text.into(),
            is_error: true,
        }
    }
}

impl From<Result<String, String>> for ToolResult {
    /// What a tool fn returns, as a result: `Ok` is the text, `Err` the failure.
    fn from(result: Result<String, String>) -> ToolResult {
        match result {
            Ok(text) => ToolResult::text(text),
            Err(text) => ToolResult::error(text),
        }
    }
}

/// The modification time and size of one file when it was last read.
// Decision: the definition types `Session.seen` as an object; its entries are kept as
// this pair so that a changed file is noticed by either its time or its size.
pub type Stamp = (Option<SystemTime>, u64);

/// One agent's connection.
// @lfy def/mcp/data.lfy:Session
#[derive(Debug)]
pub struct Session {
    /// The program as of the last call.
    pub workspace: Workspace, // @lfy def/mcp/data.lfy:Session.workspace
    /// The modification time and size of every file of the program when it was last
    /// read, by path relative to the workspace root. Empty in a hosted session, which
    /// never re-reads one.
    pub seen: BTreeMap<String, Stamp>, // @lfy def/mcp/data.lfy:Session.seen
    /// What a hosted session is bound to; `None` for a session over standard input and
    /// output.
    pub binding: Option<Binding>, // @lfy def/mcp/data.lfy:Session.binding
    /// Every review `elfie_report` recorded in the session, one per id, in the order
    /// recorded.
    // Decision: the `tool` trait reaches a fn with the session, not with a session it may
    // change, so the list a call appends to is kept behind a lock of its own; the calls of
    // one session are handled one at a time, so it is never contended.
    pub reviews: Mutex<Vec<Review>>, // @lfy def/mcp/data.lfy:Session.reviews
}

/// What the agent of a hosted session is run as.
// @lfy def/mcp/data.lfy:Role
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The compiler of one batch.
    Compiler, // @lfy def/mcp/data.lfy:Role.compiler
    /// The verifier of one batch, or of the global review.
    Verifier, // @lfy def/mcp/data.lfy:Role.verifier
}

impl Role {
    /// The value of the enum member.
    // @lfy def/mcp/data.lfy:Role
    pub fn value(self) -> &'static str {
        match self {
            Role::Compiler => "the compiler of one batch",
            Role::Verifier => "the verifier of one batch, or of the global review",
        }
    }
}

/// What one hosted session is bound to, fixed when the CLI opens it.
// Decision: a hosted session answers for one run of one agent and nothing else, so what it
// is bound to is fixed when the CLI opens it and no call can change it.
// @lfy def/mcp/data.lfy:Binding
#[derive(Debug, Clone)]
pub struct Binding {
    /// What its agent is run as.
    pub role: Role, // @lfy def/mcp/data.lfy:Binding.role
    /// The batch the run is for; `None` for the global review.
    pub batch: Option<Batch>, // @lfy def/mcp/data.lfy:Binding.batch
    /// The absolute path of the directory the run is in: the batch's copy when it runs in
    /// one, the root otherwise.
    // Decision: the definition types `directory` as a string; it is kept as the `PathBuf`
    // the session's workspace root is read as, so every path of the workspace joins to it
    // directly, as `Workspace::root` is kept.
    pub directory: PathBuf, // @lfy def/mcp/data.lfy:Binding.directory
    /// The plan the compile or verify made.
    pub plan: Plan, // @lfy def/mcp/data.lfy:Binding.plan
    /// The program the compile lowered and planned; its workspace is the one the session
    /// reads definitions from.
    pub program: Program, // @lfy def/mcp/data.lfy:Binding.program
    /// The review request given to a verifier run as its input; `None` for a compiler run.
    pub review: Option<ReviewRequest>, // @lfy def/mcp/data.lfy:Binding.review
    /// The source maps that review request was made with; empty for a compiler run.
    pub source_maps: Vec<SourceMap>, // @lfy def/mcp/data.lfy:Binding.sourceMaps
}

/// What [`Host::open`] holds: it opens one hosted session bound to a [`Binding`] and
/// returns the session's URL.
// @lfy def/mcp/data.lfy:Host.open
pub type Open = Box<dyn Fn(Binding) -> String + Send + Sync>;

/// What [`Host::close`] holds: it ends the session at a URL and returns
/// [`Session::reviews`] of it.
// @lfy def/mcp/data.lfy:Host.close
pub type Close = Box<dyn Fn(&str) -> Vec<Review> + Send + Sync>;

/// What [`Host::stop`] holds: it ends every session that is still open and stops listening.
// @lfy def/mcp/data.lfy:Host.stop
pub type Stop = Box<dyn Fn() + Send + Sync>;

/// An agent server served over HTTP on this machine for the length of one compile or
/// verify.
// @lfy def/mcp/data.lfy:Host
pub struct Host {
    /// Where it listens: `http://127.0.0.1`, a colon, the port, and a slash.
    pub url: String, // @lfy def/mcp/data.lfy:Host.url
    /// What every request to it must carry as its bearer token.
    pub token: String, // @lfy def/mcp/data.lfy:Host.token
    /// Opens one hosted session bound to a [`Binding`] and returns the session's URL.
    pub open: Open, // @lfy def/mcp/data.lfy:Host.open
    /// Ends the session at a URL and returns [`Session::reviews`] of it.
    pub close: Close, // @lfy def/mcp/data.lfy:Host.close
    /// Ends every session that is still open and stops listening.
    pub stop: Stop, // @lfy def/mcp/data.lfy:Host.stop
}

impl fmt::Debug for Host {
    /// The address and the token's length; a closure has nothing to show.
    // @lfy def/mcp/data.lfy:Host
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Host")
            .field("url", &self.url)
            .field("token", &self.token.len())
            .finish_non_exhaustive()
    }
}

/// Where the command line keeps its own files, under the root.
// @lfy def/mcp/data.lfy:GlobalReview#GlobalReview:GlobalReview:0a7247ddcb559874bf46416d7fad715bfcb5ca46d1b6f9d3d90827ba820dd8d8
pub const REQUESTS: &str = "elfie-requests";

/// The file under [`REQUESTS`] holding the last global review.
// @lfy def/mcp/data.lfy:GlobalReview#GlobalReview:GlobalReview:0a7247ddcb559874bf46416d7fad715bfcb5ca46d1b6f9d3d90827ba820dd8d8
pub const GLOBAL_REVIEWS: &str = "global.reviews.json";

/// The last global review of a project, as `elfie-requests/global.reviews.json` under its
/// root holds it.
// Decision: the CLI and the agent server both plan with the units the last global review
// found violated, so the file it is kept in and which units it found violated live here,
// once, beside the agent server's data: the CLI uses the agent server and never the other
// way round.
// @lfy def/mcp/data.lfy:GlobalReview
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GlobalReview {
    /// The SHA-256 of the ids of every global criterion and test it reviewed, as lowercase
    /// hex.
    pub requirements: String, // @lfy def/mcp/data.lfy:GlobalReview.requirements
    /// Each review, in report order, one per id.
    pub reviews: Vec<Review>, // @lfy def/mcp/data.lfy:GlobalReview.reviews
    /// Each review line that was not read as a review, then the line reading the report did
    /// not end when there was no end line.
    pub problems: Vec<String>, // @lfy def/mcp/data.lfy:GlobalReview.problems
    /// Each review line that was not read as a review though its status is violated, as
    /// written; each is in [`GlobalReview::problems`] too.
    pub unplaced: Vec<String>, // @lfy def/mcp/data.lfy:GlobalReview.unplaced
    /// Each id the review was asked about that no review in [`GlobalReview::reviews`] names,
    /// in the order it was asked.
    pub missing: Vec<String>, // @lfy def/mcp/data.lfy:GlobalReview.missing
}

/// The texts of a JSON array of strings, in order; none when the value is not an array.
fn texts_of(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

impl GlobalReview {
    /// The path the review is kept at, under a root.
    // @lfy def/mcp/data.lfy:GlobalReview#GlobalReview:GlobalReview:0a7247ddcb559874bf46416d7fad715bfcb5ca46d1b6f9d3d90827ba820dd8d8
    pub fn path(root: &Path) -> std::path::PathBuf {
        root.join(REQUESTS).join(GLOBAL_REVIEWS)
    }

    /// The review as one JSON object: the keys `requirements`, `reviews`, and `problems`,
    /// and the two further lists the data holds.
    // Decision: the criterion names the three keys a reader needs, and the object is this
    // review stringified, so `unplaced` and `missing` are written beside them rather than
    // dropped; a reader that knows only the three named keys still reads the object.
    // @lfy def/mcp/data.lfy:GlobalReview#GlobalReview:GlobalReview:0a7247ddcb559874bf46416d7fad715bfcb5ca46d1b6f9d3d90827ba820dd8d8
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "requirements": self.requirements,
            "reviews": self.reviews.iter().map(Review::to_json).collect::<Vec<Value>>(),
            "problems": self.problems,
            "unplaced": self.unplaced,
            "missing": self.missing,
        })
    }

    /// The review one JSON object holds; `None` when the value is not an object.
    // Decision: a file written before `unplaced` and `missing` were recorded holds neither,
    // and is read as a review with both empty rather than as no review at all.
    // @lfy def/mcp/data.lfy:GlobalReview#GlobalReview:GlobalReview:0a7247ddcb559874bf46416d7fad715bfcb5ca46d1b6f9d3d90827ba820dd8d8
    pub fn from_json(value: &Value) -> Option<GlobalReview> {
        let object = value.as_object()?;
        Some(GlobalReview {
            requirements: object.get("requirements").and_then(Value::as_str).unwrap_or_default().to_string(),
            reviews: object
                .get("reviews")
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(Review::from_json).collect())
                .unwrap_or_default(),
            problems: texts_of(object.get("problems")),
            unplaced: texts_of(object.get("unplaced")),
            missing: texts_of(object.get("missing")),
        })
    }

    /// The last global review of the project at a root; `None` when the file is missing or
    /// holds no review, so that no unit is among its violated units.
    // @lfy def/mcp/data.lfy:GlobalReview#GlobalReview:GlobalReview:d8da2a793916a0f1b55b56ff2e5c895b333113799087cb6f6a586324037f9ebe
    pub fn read(root: &Path) -> Option<GlobalReview> {
        let text = std::fs::read_to_string(GlobalReview::path(root)).ok()?;
        GlobalReview::from_json(&serde_json::from_str::<Value>(&text).ok()?)
    }

    /// The ids [`GlobalReview::requirements`] hashes: those of the program's global
    /// criteria, then those of its global tests, in that order.
    // @lfy def/mcp/data.lfy:GlobalReview#GlobalReview:GlobalReview:e3c16f728df79d47fdf209aba977dbc07df5c6ab2d8443e7cffe5c92dafc7f8a
    pub fn requirement_ids(program: &Program) -> Vec<String> {
        program
            .criteria
            .iter()
            .map(|criterion| criterion.id.clone())
            .chain(program.tests.iter().map(|test| test.id.clone()))
            .collect()
    }

    /// [`GlobalReview::requirements`] for a program: the SHA-256 of those ids, joined by
    /// line breaks, as lowercase hex.
    // @lfy def/mcp/data.lfy:GlobalReview#GlobalReview:GlobalReview:e3c16f728df79d47fdf209aba977dbc07df5c6ab2d8443e7cffe5c92dafc7f8a
    pub fn requirements_of(program: &Program) -> String {
        generation::source_hash(&GlobalReview::requirement_ids(program).join("\n"))
    }

    /// The ids of every review of this one whose status is violated.
    // @lfy def/mcp/data.lfy:GlobalReview#GlobalReview:GlobalReview:39f323338f7f688f8c2a6dd3daca8bded2ff388bf65d8327ef5dc209f13581ab
    pub fn violated_requirements(&self) -> Vec<String> {
        self.reviews
            .iter()
            .filter(|review| review.status == ReviewStatus::Violated)
            .map(|review| review.id.clone())
            .collect()
    }

    /// The violated units of this review: the stems of every unit of a plan whose outputs
    /// hold a marker answering for one of those ids, each once, in plan order.
    // @lfy def/mcp/data.lfy:GlobalReview#GlobalReview:GlobalReview:39f323338f7f688f8c2a6dd3daca8bded2ff388bf65d8327ef5dc209f13581ab
    pub fn violated_stems(&self, plan: &Plan) -> Vec<String> {
        let violated = self.violated_requirements();
        let mut stems: Vec<String> = Vec::new();
        for unit in &plan.units {
            // @lfy def/mcp/data.lfy:GlobalReview#GlobalReview:GlobalReview:39f323338f7f688f8c2a6dd3daca8bded2ff388bf65d8327ef5dc209f13581ab
            let answered = unit.outputs.iter().flat_map(|map| map.markers.iter()).any(|marker| {
                marker
                    .requirement
                    .as_ref()
                    .is_some_and(|id| violated.iter().any(|found| found == id))
            });
            if answered && !stems.contains(&unit.stem) {
                stems.push(unit.stem.clone());
            }
        }
        stems
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap as Map;

    use elfie_core::generation::{Marker, SourceMap, Unit};

    use super::*;

    /// One review of an id, with a status.
    fn review(id: &str, status: ReviewStatus) -> Review {
        Review {
            id: id.to_string(),
            file: "def/a.lfy".to_string(),
            line: 1,
            entity: "a".to_string(),
            status,
            evidence: "out/a.rs:1-2".to_string(),
            note: "why".to_string(),
        }
    }

    /// One unit with a stem whose one recorded output answers for the requirement ids
    /// given.
    fn unit(stem: &str, answered: &[&str]) -> Unit {
        Unit {
            target: 0,
            file: 0,
            entities: Vec::new(),
            lowered: 0,
            text: String::new(),
            stem: stem.to_string(),
            dependencies: Vec::new(),
            outputs: vec![SourceMap {
                target: "rust".to_string(),
                output: format!("out/{stem}.rs"),
                source: format!("def/{stem}.lfy"),
                hash: String::new(),
                requirements: String::new(),
                answered: Vec::new(),
                signature: String::new(),
                dependencies: Map::new(),
                generated: String::new(),
                markers: answered
                    .iter()
                    .map(|id| Marker {
                        output_line: 1,
                        file: format!("def/{stem}.lfy"),
                        entity: Some(stem.to_string()),
                        line: 1,
                        column: None,
                        requirement: Some((*id).to_string()),
                        end: 2,
                    })
                    .collect(),
            }],
            reason: None,
        }
    }

    // @lfy def/mcp/data.lfy:GlobalReview#GlobalReview:GlobalReview:0a7247ddcb559874bf46416d7fad715bfcb5ca46d1b6f9d3d90827ba820dd8d8
    #[test]
    fn a_global_review_is_one_object_with_the_keys_it_is_read_by() {
        let found = GlobalReview {
            requirements: "0".repeat(64),
            reviews: vec![review("global:def/a.lfy:aa", ReviewStatus::Violated)],
            problems: vec!["a line".to_string()],
            unplaced: vec!["a line".to_string()],
            missing: vec!["global:def/a.lfy:bb".to_string()],
        };
        let value = found.to_json();
        assert!(value.is_object());
        assert_eq!(value["requirements"], "0".repeat(64));
        assert_eq!(value["reviews"].as_array().map(Vec::len), Some(1));
        assert_eq!(value["problems"], serde_json::json!(["a line"]));
        assert_eq!(value["unplaced"], serde_json::json!(["a line"]));
        assert_eq!(value["missing"], serde_json::json!(["global:def/a.lfy:bb"]));
        // The object is this review stringified, so it is read back whole. A review's
        // place — its file, line, and entity — is not in the JSON a review report carries,
        // so it comes back empty and the id is what names the requirement.
        // @lfy def/mcp/data.lfy:GlobalReview#GlobalReview:GlobalReview:0a7247ddcb559874bf46416d7fad715bfcb5ca46d1b6f9d3d90827ba820dd8d8
        let read = GlobalReview::from_json(&value).unwrap();
        assert_eq!(read.requirements, found.requirements);
        assert_eq!(read.problems, found.problems);
        assert_eq!(read.unplaced, found.unplaced);
        assert_eq!(read.missing, found.missing);
        assert_eq!(read.reviews.len(), 1);
        assert_eq!(read.reviews[0].id, found.reviews[0].id);
        assert_eq!(read.reviews[0].status, found.reviews[0].status);
        assert_eq!(read.reviews[0].evidence, found.reviews[0].evidence);
        assert_eq!(read.reviews[0].note, found.reviews[0].note);
        // A file written before `unplaced` and `missing` were recorded is read with both
        // empty.
        let older = serde_json::json!({ "requirements": "ab", "reviews": [], "problems": [] });
        let read = GlobalReview::from_json(&older).unwrap();
        assert_eq!(read.requirements, "ab");
        assert!(read.unplaced.is_empty() && read.missing.is_empty());
        assert_eq!(GlobalReview::from_json(&serde_json::json!([])), None);
    }

    // @lfy def/mcp/data.lfy:GlobalReview#GlobalReview:GlobalReview:d8da2a793916a0f1b55b56ff2e5c895b333113799087cb6f6a586324037f9ebe
    #[test]
    fn a_missing_file_is_no_last_global_review_and_no_violated_unit() {
        let root = std::env::temp_dir().join(format!("elfie-mcp-data-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(GlobalReview::read(&root), None);
        // There is no last review, so nothing can name a unit as violated.
        // @lfy def/mcp/data.lfy:GlobalReview#GlobalReview:GlobalReview:d8da2a793916a0f1b55b56ff2e5c895b333113799087cb6f6a586324037f9ebe
        let plan = Plan {
            units: vec![unit("a", &["global:def/a.lfy:aa"])],
            ..Plan::default()
        };
        assert!(GlobalReview::default().violated_stems(&plan).is_empty());
    }

    // @lfy def/mcp/data.lfy:GlobalReview#GlobalReview:GlobalReview:39f323338f7f688f8c2a6dd3daca8bded2ff388bf65d8327ef5dc209f13581ab
    #[test]
    fn the_violated_units_are_the_stems_whose_markers_answer_for_a_violated_review() {
        let plan = Plan {
            units: vec![
                unit("a", &["global:def/a.lfy:aa"]),
                unit("b", &["global:def/a.lfy:bb"]),
                unit("c", &[]),
            ],
            ..Plan::default()
        };
        let found = GlobalReview {
            reviews: vec![
                review("global:def/a.lfy:aa", ReviewStatus::Violated),
                review("global:def/a.lfy:bb", ReviewStatus::Satisfied),
            ],
            ..GlobalReview::default()
        };
        assert_eq!(found.violated_requirements(), ["global:def/a.lfy:aa"]);
        // Only `a` answers for a violated review; `b` answers for a satisfied one and `c`
        // answers for none.
        // @lfy def/mcp/data.lfy:GlobalReview#GlobalReview:GlobalReview:39f323338f7f688f8c2a6dd3daca8bded2ff388bf65d8327ef5dc209f13581ab
        assert_eq!(found.violated_stems(&plan), ["a"]);
    }

    // @lfy def/mcp/data.lfy:GlobalReview#GlobalReview:GlobalReview:e3c16f728df79d47fdf209aba977dbc07df5c6ab2d8443e7cffe5c92dafc7f8a
    #[test]
    fn the_requirements_hash_the_ids_of_the_program_joined_by_line_breaks() {
        // The ids are hashed joined by line breaks, as a source map's requirements are.
        // @lfy def/mcp/data.lfy:GlobalReview#GlobalReview:GlobalReview:e3c16f728df79d47fdf209aba977dbc07df5c6ab2d8443e7cffe5c92dafc7f8a
        assert_eq!(
            generation::source_hash("global:def/a.lfy:aa\nglobal:def/a.lfy:bb"),
            generation::source_hash(&["global:def/a.lfy:aa", "global:def/a.lfy:bb"].join("\n"))
        );
        // A program with no global criterion and no global test has no ids to hash, so the
        // ids are read from the program and from nowhere else.
        // @lfy def/mcp/data.lfy:GlobalReview#GlobalReview:GlobalReview:e3c16f728df79d47fdf209aba977dbc07df5c6ab2d8443e7cffe5c92dafc7f8a
        let root = std::env::temp_dir().join(format!("elfie-mcp-data-program-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("def")).unwrap();
        std::fs::write(root.join("elfie.json"), r#"{ "lib": "lib" }"#).unwrap();
        let program = elfie_core::interpret::lower(elfie_core::workspace::load(&root));
        assert!(program.criteria.is_empty() && program.tests.is_empty());
        assert!(GlobalReview::requirement_ids(&program).is_empty());
        assert_eq!(GlobalReview::requirements_of(&program), generation::source_hash(""));
        let _ = std::fs::remove_dir_all(&root);
    }
}
