//! Compiled from `def/mcp/main.lfy`: the Elfie agent server.
//!
//! One tool per fn of the definition that carries `tool`, spoken over the Model Context
//! Protocol on standard input and output, or over HTTP on this machine for the length of
//! one compile or verify. The server never writes a file: during compilation the agent
//! writes the outputs itself and asks the server to check them, so one path exists for
//! writing and the CLI owns it.

pub mod data;
pub mod traits; // @lfy def/mcp/traits.lfy:tool

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use elfie_core::generation::{
    self, Batch, Marker, Move, Output, Plan, Request, Requirement, Review, ReviewStatus, Rewording,
    SourceMap, Submission, SubmittedOutcome, Unit,
};
use elfie_core::interpret::{self, LoweredCriterion, LoweredNode, LoweredTest, Program};
use elfie_core::model::{self, Criterion, EntityId, FileId, Model, NodeRef, Operation, Problem, SymbolKind};
use elfie_core::query::{self, Outline, Position, Range};
use elfie_core::workspace::{self, Target, Workspace};
use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ErrorData, Implementation,
    JsonObject, ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerConfig,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ServerHandler, ServiceExt};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite};

pub use data::{
    Binding, CommandRun, GlobalReview, Host, Role, Session, SessionRecord, Stamp, Tool,
    ToolArgument, ToolResult,
};
pub use traits::{Arguments, Registered};

/// The project layout file; a change to it loads the workspace again.
const MANIFEST: &str = "elfie.json";

/// The exit code when the server itself cannot run, as the CLI reads it.
const FAILURE: i32 = 3;

// ---- the session ------------------------------------------------------------------

/// The stamp of one file on disk, or `None` when there is no file there.
///
/// A file is noticed by either its modification time or its size, so an edit that keeps
/// both is not seen by the next call.
// @lfy def/mcp/main.lfy:serve#serve:serve:a0e0712c211c914b7d1655d1a85d5aa373db8b9d94524624d54554e2501f845d
fn stamp(path: &Path) -> Option<Stamp> {
    let metadata = fs::metadata(path).ok()?;
    Some((metadata.modified().ok(), metadata.len()))
}

/// A path under the root, relative to it, with forward slashes.
fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Every `.lfy` file under a directory.
fn walk_sources(directory: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_sources(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "lfy") {
            out.push(path);
        }
    }
}

/// Every file of the program: the files bound, every `.lfy` file under the source
/// directory (so that a new file is noticed), and the manifest.
fn watched(workspace: &Workspace) -> BTreeSet<String> {
    let mut paths: BTreeSet<String> = workspace.files.iter().map(|file| file.path.clone()).collect();
    let mut found = Vec::new();
    walk_sources(&workspace.root.join(&workspace.source_directory), &mut found);
    for path in found {
        paths.insert(relative(&workspace.root, &path));
    }
    paths.insert(MANIFEST.to_string());
    paths
}

/// The stamp of every file of the program that exists on disk, by path.
fn stamps(workspace: &Workspace) -> BTreeMap<String, Stamp> {
    watched(workspace)
        .into_iter()
        .filter_map(|path| stamp(&workspace.root.join(&path)).map(|stamp| (path, stamp)))
        .collect()
}

impl Session {
    /// A session whose workspace is the project at `root`, loaded, with every file of it
    /// stamped as read. It is bound to nothing, so it reads the files on disk and holds no
    /// review.
    // @lfy def/mcp/main.lfy:serve#serve:serve:0d7ad5672274c815b6ad256268ff29d8425eb7ed8114bb7c9ea52bd41bedde9b:2
    // @lfy def/mcp/main.lfy:serve#serve:serve:8579f2d17bf0b508fda59fe780ac73d54d3271fe8f9994ccd60bd5f66d026331
    pub fn load(root: &Path) -> Session {
        let workspace = workspace::load(root);
        let seen = stamps(&workspace);
        Session {
            workspace,
            seen,
            binding: None,
            reviews: Mutex::new(Vec::new()),
            submission: Mutex::new(None),
        }
    }

    /// Every review `elfie_report` has recorded in the session, in the order recorded.
    pub fn recorded(&self) -> Vec<Review> {
        self.reviews.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
    }

    /// What the session recorded: the reviews `elfie_report` kept, in the order recorded,
    /// and the outcome `elfie_submit` last kept.
    // @lfy def/mcp/main.lfy:host#host:host:64c0df55947a7e7a600bb0796c4715f740afcfda41cdd6f86b4cb408c4705713
    pub fn record(&self) -> SessionRecord {
        SessionRecord {
            reviews: self.recorded(),
            submission: self
                .submission
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone(),
        }
    }

    /// What runs first when a call arrives: every file of the program whose modification
    /// time or size differs from what was seen is re-read through `workspace::change`
    /// with no text, and a file that vanished the same way.
    ///
    /// When `elfie.json` has changed since the last call, the workspace is loaded again
    /// instead, before the tool runs.
    // @lfy def/mcp/main.lfy:serve#serve:serve:995535b9c7a11554f96d88793004bc092de31525b26cd0ca15ce449e17d8e386
    pub fn refresh(&mut self) {
        let now = stamps(&self.workspace);
        // elfie.json has changed since the last call.
        // @lfy def/mcp/main.lfy:serve#serve:serve:4c5aeb9423c7e9206e75798da9c197b91b439325adda5776e09a2061d144988a
        if now.get(MANIFEST) != self.seen.get(MANIFEST) {
            self.workspace = workspace::load(&self.workspace.root);
            self.seen = stamps(&self.workspace);
            return;
        }
        // A `.lfy` file under the source directory that is not in the program and that no
        // earlier call saw has no stamp among those seen, so it is among the paths that
        // differ; a file whose modification time or size differs is too, and so is one that
        // has vanished, which has no stamp now.
        // @lfy def/mcp/main.lfy:serve#serve:serve:1893a7a1afe4f16bc76a47596614b6645e060072995b9387c9386905037fe959
        let changed: Vec<String> = now
            .keys()
            .chain(self.seen.keys())
            .filter(|path| now.get(*path) != self.seen.get(*path))
            .cloned()
            .collect::<BTreeSet<String>>()
            .into_iter()
            .collect();
        // Each is read through the loader with no text, which reads it from disk when it is
        // there and drops it from the program when it is not.
        // @lfy def/mcp/main.lfy:serve#serve:serve:9a771a2886ce7deadeb6a39ae4524d1bded9ac7c87fc49266ce3da78c7e4fbcc
        // @lfy def/mcp/main.lfy:serve#serve:serve:01ce18f671ec478741555e6ff345d61120f118da6209c46d18cd01b3cbff3e6a
        for path in &changed {
            self.workspace = workspace::change(&self.workspace, path, None);
        }
        if !changed.is_empty() {
            self.seen = stamps(&self.workspace);
        }
    }
}

// ---- the server -------------------------------------------------------------------

/// The fns of this module that carry `tool`, in the order the definition declares them; no
/// resource and no prompt is listed beside them.
///
/// One [`Registered`] per fn carries the name the server lists it by, the fn's definition
/// as its description, and one [`ToolArgument`] per parameter that is not the session. A
/// call reaches the fn through [`call_tool`], which checks the arguments against those
/// parameters before the fn is called, answers an argument that is missing, unknown, or of
/// the wrong type with an error naming it and without calling the fn, and turns what the fn
/// returns into the call's result — a failure into an error carrying it, so the server goes
/// on.
///
/// Which of them a session is offered is [`offered`]; a session lists and calls only those.
pub fn tools() -> Vec<Registered> {
    vec![
        // @lfy def/mcp/main.lfy:problems#problems:tool:47dafd874509841945f72721454f0bde82f7cb760967d4104944c32a0b08e4f6
        // @lfy def/mcp/main.lfy:problems#problems:tool:8697079e758527441df828916c31420593b865e8089a7dc5367a58f9ea2131d8
        // @lfy def/mcp/main.lfy:problems#problems:tool:e3326a06fa53e9623634da620a9bac4aa2a1d0c7e0ea9f0d1143f0f4443b8ba5
        // @lfy def/mcp/main.lfy:problems#problems:tool:97369ca270f9c5503c620a5a45c2e5bda573d9d44900f052d6aa2f6fab3ccd0c
        Registered::new(
            "problems",
            "The problems of the program, or of one file: what the compiler, an editor, or a check would report",
            // @lfy def/mcp/main.lfy:problems#problems:tool:54f7dd8fa92710b384c60ba1163c17852fbfb0a80ca36261f57990307bf95f7e
            vec![
                // @lfy def/mcp/main.lfy:problems#problems:tool:18f70fcb2a52141d025ba8a3d8956133691c72b34fdf58d3db5db6df54e1f6fc
                // @lfy def/mcp/main.lfy:problems#problems:tool:0d47cd7405248873ce91f19150020622e2bbc3156320aa272d9325d8ab7ad9b4
                // @lfy def/mcp/main.lfy:problems#problems:tool:db1bf2892de252566cd7219c7c054c56115cc9b26b82cd247bd66388521af771
                // @lfy def/mcp/main.lfy:problems#problems:tool:87fb1f093e523c06d8bd452dd3daa9a60adcb2fa4c4dd899726161be0c185d3c
                ToolArgument::new("file", "a path relative to the root; every file when left out", "string", false),
            ],
            // @lfy def/mcp/main.lfy:problems#problems:tool:88901ed2e2feae85726424eef060489ae98b454113d1f36783aa11b341a5d86e
            // @lfy def/mcp/main.lfy:problems#problems:tool:7ea2a0f37c0856454065e9eeb1658f25ad2acfead105b67722cc2dc039c5021e
            // @lfy def/mcp/main.lfy:problems#problems:tool:6d5668f30152be9816a0ad67fc9c30c0863004d526f68f20d20a072f51191521
            // @lfy def/mcp/main.lfy:problems#problems:tool:d9b6b1ce215d966d4d49ba31d319199d9539fe1614bed20bc86032b0e2fc28c0
            // @lfy def/mcp/main.lfy:problems#problems:tool:123e5ad677760165c91b58ed682062ff6497cb37eb7c02f17e80bcf7f81ef7ad
            // @lfy def/mcp/main.lfy:problems#problems:tool:0365ce41c179a2bdfc7bb6bc7e6f5a2521574397ecc547a36369152171a66b3b
            // @lfy def/mcp/main.lfy:problems#problems:tool:8cd074b3973e9c4a3bb9a83a787e6370caea26c94222c18023fca79732f472ec
            // @lfy def/mcp/main.lfy:problems#problems:tool:a9b7494a79d88bd889cce42866936bb912530e77684203cac316c431a18f60f0
            |session, arguments| problems(session, arguments.optional("file")),
        ),
        // @lfy def/mcp/main.lfy:find#find:tool:e132ab083f2616c0a9f886e961c7096e1e994b7fd593134d0682c3c14c68548e
        // @lfy def/mcp/main.lfy:find#find:tool:64a7427c36430ec8c9e5b9699af3a44d627b57d784c3448c5797f67490c2afbb
        // @lfy def/mcp/main.lfy:find#find:tool:57389a2d52ac601927c3841a7dbd13f10ca56c231d22e66e3ddf12b8b5954166
        // @lfy def/mcp/main.lfy:find#find:tool:ea2d1bca6af575f658c0f695835faedb4e77f280b10551bf06dcf01d1ae9fbb5
        Registered::new(
            "find",
            "Where things with a name are declared",
            // @lfy def/mcp/main.lfy:find#find:tool:7ecb9cc6ffb66b1e366569590d4060dfdd696066f9463240d0dee9c53d39002c
            vec![
                // @lfy def/mcp/main.lfy:find#find:tool:835a1a009cd5f49f70c7b88b2d718244df6c4a39ed9f73d4925fe13b1231e88b
                // @lfy def/mcp/main.lfy:find#find:tool:c9afb88c885fac20c29b4b48da700972a1882420b759f8d79b25d77630a0e168
                // @lfy def/mcp/main.lfy:find#find:tool:26960caba8b776d48816a31b7cd3d9d20254e141db7595c95945236688b3ff13
                // @lfy def/mcp/main.lfy:find#find:tool:01a20a244cc7cf7bd12c17e49fbfebc0c04502486559072c3783e51b53a22130
                ToolArgument::new(
                    "name",
                    "an identifier, or an owner's identifier, a dot, and a member's name; a file path and a colon before it limits the search to that file",
                    "string",
                    true,
                ),
            ],
            // @lfy def/mcp/main.lfy:find#find:tool:4d9398185457aa7fab84df6408fca264cc40ac9c40f927ce96e5873ee5a87493
            // @lfy def/mcp/main.lfy:find#find:tool:0da0cb212aed7b675bf8a12558f696ff9fc2926351e843f06741a1d48dcf2e23
            // @lfy def/mcp/main.lfy:find#find:tool:f8dce49696575cb028eb74d284baa5dbba8b29514ddff662ee8900b968dd66f1
            // @lfy def/mcp/main.lfy:find#find:tool:9e6ccee8bf3cdcda1e9f14fb07ef0083f6b49f3e910bddf0534c5a120eb4f11a
            // @lfy def/mcp/main.lfy:find#find:tool:14b1732157522ebf4b24b6038fcddb65b5b23a3689651521b1bf763ca8fb0879
            // @lfy def/mcp/main.lfy:find#find:tool:26dc105fcfc8d9cc4ed181d1ae1172c0d0e88942453e1513fd22d3d6c8b3b095
            // @lfy def/mcp/main.lfy:find#find:tool:ee0ffd83b80c1fa9a6f6446550d0ede648668263c911d25b7cccde3b1c251207
            // @lfy def/mcp/main.lfy:find#find:tool:b969b67f01ae22cf235972b295dcf1c53ad993facbcd3ecfca3694f891b7749d
            |session, arguments| find(session, arguments.string("name")?),
        ),
        // @lfy def/mcp/main.lfy:entity#entity:tool:b14133579f6b9759ae1979e937ff1aaa58a141db5d2437d71d839b46e724231a
        // @lfy def/mcp/main.lfy:entity#entity:tool:e1c69a6fbf06d17784db05e85f146afdc2ed92a34fb43987c806cc9de90c4229
        // @lfy def/mcp/main.lfy:entity#entity:tool:175628e227d6a58160595e49d7da66a0ba3720c6ea864fabcaf7685eb6d6a104
        // @lfy def/mcp/main.lfy:entity#entity:tool:6fb1b9cd738fb7625982664ced1d50b2a1996ba46a7b736de1f0ca4c2e2ff289
        Registered::new(
            "entity",
            "Everything known about the things with a name: definition, type, documentation, traits, members, criteria, tests, references, and source",
            // @lfy def/mcp/main.lfy:entity#entity:tool:29720a02e6e0244d640f8a4c253e92ede320148b52f75a1b5b7eacfe11c2e754
            vec![
                // @lfy def/mcp/main.lfy:entity#entity:tool:ce7d3834e778c9a33a677519c7f05ec96ecf849dcd62a3aa02fd77889652deff
                // @lfy def/mcp/main.lfy:entity#entity:tool:1688a7f2f37257b8e98d373d0e0c42181ec7505a1280455fe7f5f3a63b07184d
                // @lfy def/mcp/main.lfy:entity#entity:tool:dc7ae5ee6d4efa4661b6e14a134daf3b4186a340a242f2c99b8354aa10976b0a
                // @lfy def/mcp/main.lfy:entity#entity:tool:da01a1719cc145a83e84788e0ccacdf684edb378e0cb800425b88629e2ef17e6
                ToolArgument::new("name", "as elfie_find takes it", "string", true),
            ],
            // @lfy def/mcp/main.lfy:entity#entity:tool:3d70810395770f46f4b0341b19b3b5746a6829d42a024bdba5165bc6d4a732bb
            // @lfy def/mcp/main.lfy:entity#entity:tool:b90d61e8f37841867d442fc530d5ddcf92714d7d1d80b844e8c85cf8597e59b5
            // @lfy def/mcp/main.lfy:entity#entity:tool:413c4c7682326a1441760af5984fa5ff4cacad84ceef75ed3d5f1df46112b944
            // @lfy def/mcp/main.lfy:entity#entity:tool:79374b37a4c2b0e653d83228d8c5563ef6eb7a434e367323698fc7c8d6b4ddfe
            // @lfy def/mcp/main.lfy:entity#entity:tool:fb4d5af52d123ee4f2664e89990ada1b015f962994f33159a15b431af33b7090
            // @lfy def/mcp/main.lfy:entity#entity:tool:95f57cc552a1a5cb5ecce19b598d13f87a05c5bdfaaa6938a13310f2508831b6
            // @lfy def/mcp/main.lfy:entity#entity:tool:5829a685241aba8ca4ca4bf3f62ea2b3055e121c981de79669a9d7c3d83770cc
            // @lfy def/mcp/main.lfy:entity#entity:tool:a7c0ee0d3baa0c575fa939c2dc2d147543f450d6e97fe4e0cf89d5de4453291d
            |session, arguments| entity(session, arguments.string("name")?),
        ),
        // @lfy def/mcp/main.lfy:references#references:tool:91b0b8e47d08c4733fa86ad5af06ea3965620c5c00d40d07c8cc7fb921a0a660
        // @lfy def/mcp/main.lfy:references#references:tool:cb492bde6d5a369786317bbcb74ee8376654febd8e1f52e4b7974b010a3f24a7
        // @lfy def/mcp/main.lfy:references#references:tool:2b8a93892addcbfcba7721d58c72423118422837b4989f9c4c085c7482cddaa2
        // @lfy def/mcp/main.lfy:references#references:tool:cbfdf53dbe0d6ebe62a812187c349b933d14488e7b4da9df8e358161ef6a89af
        Registered::new(
            "references",
            "Every place things with a name are used",
            // @lfy def/mcp/main.lfy:references#references:tool:2646b783aeb40a7eebe94f279731de3bd187157a5b6de43e5d2ed7295da143b6
            vec![
                // @lfy def/mcp/main.lfy:references#references:tool:8a676febf5a8ed15bfe4330e5952233928dcf8ca2ce05ea181240c551b835b00
                // @lfy def/mcp/main.lfy:references#references:tool:8c38aea1e82e269279d66e3527838a3ad463fe9f2f5a260edd02048c4e72d341
                // @lfy def/mcp/main.lfy:references#references:tool:5040afcd04817be85669201eb87a8b9d789a85ad095fd2c10fd0fff86814e310
                // @lfy def/mcp/main.lfy:references#references:tool:00801e0b270e823290817c793008d5f0397ef2e68f54a8165f9d83991546a1a8
                ToolArgument::new("name", "as elfie_find takes it", "string", true),
            ],
            // @lfy def/mcp/main.lfy:references#references:tool:578cd15582e96bfc66f2880b2a48c591dbce2deb398f425f9a4d3063183a9028
            // @lfy def/mcp/main.lfy:references#references:tool:5913c8338e4fe8a298fa992c72acace50365461a6be39b24f5e283c0b4247d4b
            // @lfy def/mcp/main.lfy:references#references:tool:44eba5979daecaf73a71db45d29b3241c24d26df19be8c87c04f0776c54fe9e5
            // @lfy def/mcp/main.lfy:references#references:tool:907da8e53ec21ffd2ba78ae2828dc026cfe99c3aa51c848998571262d0c7139d
            // @lfy def/mcp/main.lfy:references#references:tool:aad077310fe2dbc43c8b73a03ab9c9f16da6988cbc12422aa252f25c40a90b31
            // @lfy def/mcp/main.lfy:references#references:tool:dd81133cca58c8ba8fc6b9dc72579d5233719d65c7212530254058936f847c45
            // @lfy def/mcp/main.lfy:references#references:tool:b4222e73bcbb5478afea50f42b71e34c9d17c5065af4fa14863c508322e95c80
            // @lfy def/mcp/main.lfy:references#references:tool:9ae2c8dcc1248d086202d8cd6bc7e4ce571330e106c86a53547eca02254d5430
            |session, arguments| references(session, arguments.string("name")?),
        ),
        // @lfy def/mcp/main.lfy:outline#outline:tool:eb4909f50a27918f8d4090b839b8e84945dfe78d2a8435b6f7292f31da9873f7
        // @lfy def/mcp/main.lfy:outline#outline:tool:b6b8b54cef8bc5d0b4bf310052c258e50aa82540e2c975253cd5bcd908208bd2
        // @lfy def/mcp/main.lfy:outline#outline:tool:69f17896d41fb427aacfe72b29e39086032b94f68fc0d1024abacfaaba1e9765
        // @lfy def/mcp/main.lfy:outline#outline:tool:e3acad11964324b9d465702630424bd224590317efb9fed52df411bce0c0655f
        Registered::new(
            "outline",
            "The declarations of one file, nested as written",
            // @lfy def/mcp/main.lfy:outline#outline:tool:5e24e9b2b20a966af8ef5248c6ed51d7dc711cb99d83e5129cfd35f8c4063037
            vec![
                // @lfy def/mcp/main.lfy:outline#outline:tool:b5be734a5a6b719e237407065190ce1f921c2ce13d33c7d8f5d7a4db8ad6f5fa
                // @lfy def/mcp/main.lfy:outline#outline:tool:c5ef388a3c005c460bf2f607ced79a87ed09da338da69eef72bb3ef4e56fd31b
                // @lfy def/mcp/main.lfy:outline#outline:tool:b6f44f85d33e69eb9cd8fef87a44c3f84d930895c2a8fdbba60a2336ae625a4b
                // @lfy def/mcp/main.lfy:outline#outline:tool:c981cc2af2eafbd212193c4edd589306b5fc1b12afbf2df7eb4cc1ef1dfd1306
                ToolArgument::new("file", "a path relative to the root", "string", true),
            ],
            // @lfy def/mcp/main.lfy:outline#outline:tool:56807c149fa490c87e28f903d14a7c0f45d7bd9ef10df81481e83b233f241b30
            // @lfy def/mcp/main.lfy:outline#outline:tool:9912a2923d9900857a75b4e16c297f8763189871aec10ad9b59a4c3da8441630
            // @lfy def/mcp/main.lfy:outline#outline:tool:a2fc4d8d3c79341b031d9456a187c8df308d0051f1a4aff47ab6111fdba562d5
            // @lfy def/mcp/main.lfy:outline#outline:tool:f5e2e8d44cd795aa69322fef5a2f21e8c943fd631ec83fb04f50665b2a0d412f
            // @lfy def/mcp/main.lfy:outline#outline:tool:6e95ab5fd88f41d7145f86ed344f8156d3464326e598b0ac637ac3ddbc5d369f
            // @lfy def/mcp/main.lfy:outline#outline:tool:150963511f1c73e51401709017f75ec80e37e411880f968dadf301ed995494cd
            // @lfy def/mcp/main.lfy:outline#outline:tool:6114e21db6cb9375ba623e9ff0b359857133d98565b50e37b08ee737a3e0ac41
            // @lfy def/mcp/main.lfy:outline#outline:tool:532b2ad88427069a8c24f3a734fe21c29f9d0b4bcf57c523a2436c4d0228712f
            |session, arguments| outline(session, arguments.string("file")?),
        ),
        // @lfy def/mcp/main.lfy:grammar#grammar:tool:c3c16f96ee3cf86971b4ccd7fd1a9c9450ab8f70516c3d2c4eaedbb816b2a2f0
        // @lfy def/mcp/main.lfy:grammar#grammar:tool:867847f2e6eb9134fd409375bb2a074218ed073c91533dbef7939b3e214305b6
        // @lfy def/mcp/main.lfy:grammar#grammar:tool:c5653402025c8545919d8b3c15e8bce1041d5302593e40a588969af7a8e79961
        // @lfy def/mcp/main.lfy:grammar#grammar:tool:269dc484865babf40b3e114fadf80bd330d3279d8933541dc768d44ac91dc1f1
        Registered::new(
            "grammar",
            "The grammar of Elfie: every terminal and every rule as EBNF, for an agent writing or reading Elfie",
            // Its one parameter is the session, so it holds no argument, and nothing further
            // is asked of an argument's name, type, description, or whether it is needed.
            // @lfy def/mcp/main.lfy:grammar#grammar:tool:7c4b82dcd83184afd88477db1f841b99e09a48579fbd8a607ecacab9384b07a8
            // @lfy def/mcp/main.lfy:grammar#grammar:tool:f850ab1c3e6cc60934d83c21bf8c38985073e480f783ed7b1e3938becac79470
            // @lfy def/mcp/main.lfy:grammar#grammar:tool:f19df1b9d30afb44626fbac447fc8174ccf96090af4d4c55d7cd15962678ec16
            // @lfy def/mcp/main.lfy:grammar#grammar:tool:1bee4ee804f7710cdf19b8f85355a3d25ef669978aedd26b77fd12b4384bea46
            // @lfy def/mcp/main.lfy:grammar#grammar:tool:6fc2ab76fe5f882a5659db4de547d76cfb7a2d744ead1bd45cf7df62cd222a7e
            vec![],
            // @lfy def/mcp/main.lfy:grammar#grammar:tool:f115d229e0e441100f57379c4e9320b7a35a8a4df3397e7b94b5dc042fad25a1
            // @lfy def/mcp/main.lfy:grammar#grammar:tool:445bc95fc02be39bbfa3ac79b8d2d20865a5992e51582c1b2fd9f0701fd2a742
            // @lfy def/mcp/main.lfy:grammar#grammar:tool:6f8d7a1462a99bf3fa72427d3a73b0a614fdc10fc808163f370626ed4a92c4b3
            // @lfy def/mcp/main.lfy:grammar#grammar:tool:bd5a9029211a8e2f3d3c41e446aba7cfaa1aa0a444f42df76c2d0255eb780df8
            // @lfy def/mcp/main.lfy:grammar#grammar:tool:b32976cb22016686217d233fb9df1603beb2140132b48dd9c7812b79c2173d4d
            // @lfy def/mcp/main.lfy:grammar#grammar:tool:4dc3ad8c479a0c30222640c742e8a2d038c354b7c49002f7e7418207a552f774
            // @lfy def/mcp/main.lfy:grammar#grammar:tool:1e0a1ad9253f2d958a5b86d8cc0eafcecedb8d8cf8244049d2761ea7ee21559e
            // @lfy def/mcp/main.lfy:grammar#grammar:tool:b7323dc3bdb5a4319278e80e814e49181832a99d94e88ba1d9c7a9a3a6d8b0e8
            |session, _| grammar(session),
        ),
        // @lfy def/mcp/main.lfy:units#units:tool:2ac75ac99fe508b7a8c4a2d27aca12fd9d96cccf9950c3f94cce20f1da99655b
        // @lfy def/mcp/main.lfy:units#units:tool:11b09db1a7752a1e56b444520139ac0f584b54714e72d5b267322c54277ecfb4
        // @lfy def/mcp/main.lfy:units#units:tool:9d5a244e3de0666f68b9af569acc4864151e7fd8a5d48f7cfcc6bcabad9e2670
        // @lfy def/mcp/main.lfy:units#units:tool:cfeac83cd614fdfc6ff23cb436a54783c7d0e1652aa3199bc3686cf6826f60e0
        Registered::new(
            "units",
            "The units the compiler would produce, in order, why each needs generating, and how they are batched",
            // @lfy def/mcp/main.lfy:units#units:tool:9e4e0920c3a77f7c24ca79866264ca5ea4507712f18a471af4347d4632455852
            vec![
                // @lfy def/mcp/main.lfy:units#units:tool:4f77f81c4f6b8f1b069afb12ae98bb421ef8b1aeafd78e6382ba0c3bc37144a9
                // @lfy def/mcp/main.lfy:units#units:tool:f825dc9832555a4eb351f2e0e489d07be3fcc6600371fdc9bee6bdd83b3b661b
                // @lfy def/mcp/main.lfy:units#units:tool:ca98b07c5016189c3ac214fd187b84a997e536edc1ba4d14c833d955e423de52
                // @lfy def/mcp/main.lfy:units#units:tool:7049caa73147226f8d26eb3c3b60ba88b5cf9217fc22308e57f0cdd5b4a92743
                ToolArgument::new("target", "the identifier of one target; every target when left out", "string", false),
            ],
            // @lfy def/mcp/main.lfy:units#units:tool:c0392f44087e7b29669f0f8d6b5d294cb0195675616839b4f6d80e791125f939
            // @lfy def/mcp/main.lfy:units#units:tool:f26dfeee8e2b533ee1e3670612cb4079764032f285d8b25f9249a136fcbc513d
            // @lfy def/mcp/main.lfy:units#units:tool:a71b6d7d974a07b20d3d3a0a6bed025139d2e10a00a2b42542d18d13ac16d6d7
            // @lfy def/mcp/main.lfy:units#units:tool:59e3afe0a4f39564b82a347fa5494defa7421ae95a945c19049ed8e307b1b0e8
            // @lfy def/mcp/main.lfy:units#units:tool:6fe94ed319295931073848d26163e8ac3c9efe7cf750da516cb1583b09e4c940
            // @lfy def/mcp/main.lfy:units#units:tool:a2e1c4ab8495bfed9ff4f22428462d6c61f0c1df2bb61f7b05ccacd5e3106f75
            // @lfy def/mcp/main.lfy:units#units:tool:81aae6abacf07fa5edf743b82fc37586ea135ec9ec9af2639c6e6662a0c9469f
            // @lfy def/mcp/main.lfy:units#units:tool:1ee7df6703e37c15ef56982f86d76fae7deacf742479bd28a9b3bde7ee66a1b2
            |session, arguments| units(session, arguments.optional("target")),
        ),
        // @lfy def/mcp/main.lfy:request#request:tool:beadebf1d4caa2b6158bcb4afe387ffbf1fdaa65b6ebb342c852e52d52b2b658
        // @lfy def/mcp/main.lfy:request#request:tool:3d666a1c238113290ac9bc99eba6f8b431a32dfbb44f42d7b63e635a6a487ba0
        // @lfy def/mcp/main.lfy:request#request:tool:30eeb76061a9ac9849a230ebdd0d367531f04ba72967baee676deb1bfa0b711a
        // @lfy def/mcp/main.lfy:request#request:tool:0a9e15bec9a45024fd3d99828e92e41ae9b90f757fd2e0b9727195f40f5086ec
        Registered::new(
            "request",
            "The full request for one batch: what to produce, where, every criterion to satisfy, and how to report",
            // @lfy def/mcp/main.lfy:request#request:tool:79078aaecc3eb0b5da33d600e95bbc7dcde986688c474910707530f5666a030c
            vec![
                // @lfy def/mcp/main.lfy:request#request:tool:a87c03b34486d18a1c17ceb833c722c4dd3842a6a421881c70f322337e0e0481
                // @lfy def/mcp/main.lfy:request#request:tool:5f1390725bc367714b87749e253f03091c3893e803feaaa995bfd7aaefa84e7a
                // @lfy def/mcp/main.lfy:request#request:tool:3960bc68d750e3fc1407e6c8d636ec1b5b2b1066b7a1e809a5ffeec14a2b3011
                ToolArgument::new("batch", "the identifier of a batch, as elfie_units lists it, or the stem of one of its units", "string", true),
                // @lfy def/mcp/main.lfy:request#request:tool:f30b7bfcc6ed637477e5d9c6dde218d0f7b7274a7eb40565013244416b0fa956
                ToolArgument::new("target", "the identifier of the batch's target; the only target when left out", "string", false),
            ],
            // @lfy def/mcp/main.lfy:request#request:tool:d360786cef01769989e63abe1e561a739a3abcc849ac2c13a4aec96f220addff
            // @lfy def/mcp/main.lfy:request#request:tool:e5bdf55db701cf680b09a00655b7f1d5a6a54182089d06b9ae4d6c4a439e676d
            // @lfy def/mcp/main.lfy:request#request:tool:b7c905f3a462069d30d33e3a3e61cd57f34c3039bfa131ae26f1d852fdd2070c
            // @lfy def/mcp/main.lfy:request#request:tool:6937dbc72b50f20142642a70d702a97291567a2c235b538ce49149e7311ef28c
            // @lfy def/mcp/main.lfy:request#request:tool:9fb364f7e6ba15fc4a1310334b46e7caa824e8536933e4d30e126523ca55344d
            // @lfy def/mcp/main.lfy:request#request:tool:f1c8ff30fb778df56f920993cac9f434ca7d05174e08a4529c76ef50f0f34122
            // @lfy def/mcp/main.lfy:request#request:tool:806f418c79eeea1378264a7f704dd7542b5fafffc84e9d62cd6694870e3f424c
            // @lfy def/mcp/main.lfy:request#request:tool:b0e0cfd7d7f475f1bd8b173c972e686a43cabef5a4a9cc18b77eab7a994d2b5b
            |session, arguments| request(session, arguments.string("batch")?, arguments.optional("target")),
        ),
        // @lfy def/mcp/main.lfy:task#task:tool:b4176adea23f8e6c58f4d041e7a85447286076984bd05464e92528b60fda6d8b
        // @lfy def/mcp/main.lfy:task#task:tool:b4a5feee0df4f25397442a14ddc5789cddaecd15441de63010655c2768703e25
        // @lfy def/mcp/main.lfy:task#task:tool:46ad78939d73914a576d0dce0904cae992474f9eeb184c717a5a0c324091d4d5
        // @lfy def/mcp/main.lfy:task#task:tool:3f09d52a0b71d1b830863b88454d30b98bb821b36e428efed32d7e6169482a3c
        Registered::new(
            "task",
            "What this compiler run must do: for each unit of its batch, the outputs and the reason, every criterion and test to answer for with its id, place, and text, each marked new, reworded, kept, or unknown, and those removed since the outputs were accepted",
            // @lfy def/mcp/main.lfy:task#task:tool:6ddac2bcba95be32011edabc6d0abbc63da57cd4a0182a8c86f882a8482706e7
            vec![
                // @lfy def/mcp/main.lfy:task#task:tool:e91db1a6069241eefc991fb1723f76dac6a194f33db9dbeaa21f8163f69cbdaf
                // @lfy def/mcp/main.lfy:task#task:tool:b2e2fadd9a1cd573a4b55e2e77fbd573ec3a9f6184ccd88beca5e743b871e9cd
                // @lfy def/mcp/main.lfy:task#task:tool:efcc6222428846670162553eed20dfde5c5603101a6a03e991a434832f04894b
                // @lfy def/mcp/main.lfy:task#task:tool:b361de400635b42112cf8a02205ed19f3875a7f7f9835b646817814b39f0233d
                ToolArgument::new(
                    "unit",
                    "the stem of one unit of the batch, as elfie_units lists it; every unit of the batch when left out",
                    "string",
                    false,
                ),
            ],
            // @lfy def/mcp/main.lfy:task#task:tool:6906aa388a4da929b046d4c96d4a070b0963d06bccb58ae5b990c00788627817
            // @lfy def/mcp/main.lfy:task#task:tool:b5c70f578c9857ac3ed4a51529ca0521467c10eab0b4ab4cf28cc3781d492359
            // @lfy def/mcp/main.lfy:task#task:tool:fd081eb82b1124fbd1ca009325b4978d8b342b69049fb32f4cacb808670051a7
            // @lfy def/mcp/main.lfy:task#task:tool:550a53793d91d79739538739bb531ddc98c3305e26aab4b81984562bde2f013f
            // @lfy def/mcp/main.lfy:task#task:tool:07e54abc770f1a3ac7809eca72a5406d78899640f48cc32a1a8f4f8aeb365374
            // @lfy def/mcp/main.lfy:task#task:tool:58f60da4f6fb7b5f0f9d9ded72cb43dc5df3fb5021ebd4dfcc459110960d3243
            // @lfy def/mcp/main.lfy:task#task:tool:4edb5fa4433e757f9e3a630b34a68b8a9e99be901d69ac1d91725b52b9682e49
            // @lfy def/mcp/main.lfy:task#task:tool:3824695bea06c924cb4eba5253dac7d204a002a30490259b15481ca71f9c4933
            |session, arguments| task(session, arguments.optional("unit")),
        ),
        // @lfy def/mcp/main.lfy:dependency#dependency:tool:4fd4dbdb36a4d1d4e8a9589471a29d8337372b850160162b8a14f940ad74bbc6
        // @lfy def/mcp/main.lfy:dependency#dependency:tool:c19bc9c85c771a6a121e931a3d90f5e925e08c8d87bdaf800db6922b0e2876ff
        // @lfy def/mcp/main.lfy:dependency#dependency:tool:7b7fe0f695f34ca950fbd8f4bc3aa16eb90bc78d78ed5a87812fe95b03c71eb5
        // @lfy def/mcp/main.lfy:dependency#dependency:tool:62ccf07aff23acf72a7e091f19b6d110da5bb4dc6f969dcadd130d72ab9ccf3f
        Registered::new(
            "dependency",
            "What one entity of a dependency of this compiler run's batch offers its dependents: the dependency's unit and outputs, and the entity's kind, definition, type, parameters, output, and members",
            // @lfy def/mcp/main.lfy:dependency#dependency:tool:01c082cb2c776a72049a0c164c556b408ada68b6c8d3e961aed6be20476ba114
            vec![
                // @lfy def/mcp/main.lfy:dependency#dependency:tool:feb08658fc3d60e206035d80439de4021e443fe53d2226e6e8ad43298da9f5b7
                // @lfy def/mcp/main.lfy:dependency#dependency:tool:4894a15426436158e1d439a6ad82b49784af28fc4c896d683eb4bcfe455480a0
                // @lfy def/mcp/main.lfy:dependency#dependency:tool:2b186e60bc3e791246ddc810258418700586e003d4998497bd580b40f6624761
                // @lfy def/mcp/main.lfy:dependency#dependency:tool:81c9fb533af066d41280087dd8b620bc5dd294bb4ca35adb668ec0c9d36c003a
                ToolArgument::new(
                    "name",
                    "the identifier of an entity of a dependency outside the batch, as the request's interfaces part names it, or an owner's identifier, a dot, and a member's name; a dependency's file path and a colon before it limits the search to that dependency",
                    "string",
                    true,
                ),
            ],
            // @lfy def/mcp/main.lfy:dependency#dependency:tool:9dac3037592a32302560384e3c8a39ca962001113ac59a8acf2c5c4780f71e38
            // @lfy def/mcp/main.lfy:dependency#dependency:tool:375badf5392deddf1689b695d0c62a631d22d6b39064e37883f362fdd24b073d
            // @lfy def/mcp/main.lfy:dependency#dependency:tool:1a71bf120077fba410061a19731e2eff14fe02eae36f6c4d869ab1b6d22d664d
            // @lfy def/mcp/main.lfy:dependency#dependency:tool:07aec05fe6ccc4299e78e4c5d9be92049d849a42d2bc4b3ce3e7f2215c356db7
            // @lfy def/mcp/main.lfy:dependency#dependency:tool:cbb88fee807215cb82267d268f14bfd999720fea12dbb9b3088ea6a7734c1def
            // @lfy def/mcp/main.lfy:dependency#dependency:tool:649a317a7da320bf4282cbbf655cddce41a07fadfa4f9b2fb6a7c2d00814b14b
            // @lfy def/mcp/main.lfy:dependency#dependency:tool:730e9d6821325999ab31904691e3e1a7013318bf229e3b12c01b8c8b519dce62
            // @lfy def/mcp/main.lfy:dependency#dependency:tool:597317caf65eb0dd18b63c0849c00451c0667a3796442dbf65f7c3e79567420d
            |session, arguments| dependency(session, arguments.string("name")?),
        ),
        // @lfy def/mcp/main.lfy:check#check:tool:6ab2c1cff2807e117f8a6061395f3550cc0741100a599c11fe1972c39b35dacb
        // @lfy def/mcp/main.lfy:check#check:tool:d1b4319d34a3d00324ec0850bf1e0987ceb4112aab80f13d77b82554c27ca5f9
        // @lfy def/mcp/main.lfy:check#check:tool:5efbbf0ff43ad79f41d3ac7bf5367d058f6765db795a0f1f4662585a08da45fa
        // @lfy def/mcp/main.lfy:check#check:tool:9043fbf64929cb4a1d5e4a012d96fe8e84677ea1faa807b230616a4170b41143
        Registered::new(
            "check",
            "Whether the outputs on disk satisfy the request for one unit, and what is wrong when they do not",
            // @lfy def/mcp/main.lfy:check#check:tool:e7f686636ea7a27002b3e7592c7dacea4aaf4782ad7c636ed061a34eafb74fd2
            vec![
                // @lfy def/mcp/main.lfy:check#check:tool:20606b167e31f14c6a9694e62792af4c2d5ba24799dc24be9d5fb029dcc551c1
                // @lfy def/mcp/main.lfy:check#check:tool:c8ac27693e52973e511d6eedac2afb8cf65dcab8e12698a4c0fb63cb79630cc8
                // @lfy def/mcp/main.lfy:check#check:tool:66481f10c2ad4b9468770c8845da8aadc1620c975cb26ebba898b9cc49091033
                ToolArgument::new("unit", "the stem of a unit, as elfie_units lists it", "string", true),
                // @lfy def/mcp/main.lfy:check#check:tool:d240d07696d43d22eb9adbfb087c059bc742e7a9cb1d1fee32ee4e86cb1b141f
                ToolArgument::new("target", "the identifier of the unit's target; the only target when left out", "string", false),
            ],
            // @lfy def/mcp/main.lfy:check#check:tool:775a7d5308dcc9bdbf4efc9c0771345d75c0042b2dcb90727173a98c7ad8c576
            // @lfy def/mcp/main.lfy:check#check:tool:171e2f1ce748190f5ccbf577a5f91fc2fd8683a6b877bb654a16a48965e01265
            // @lfy def/mcp/main.lfy:check#check:tool:15fd0657dd9bfbe4565283f531743c14447968c8361b95ba4e874d2a8309c888
            // @lfy def/mcp/main.lfy:check#check:tool:551327521f11195dad0acd8b202685f4710f5c12578520e07845c6e3f9be0f9b
            // @lfy def/mcp/main.lfy:check#check:tool:e42f51f8d640acef564760981510ea1aa80bae4c7719847ff7ec3001be3cdc31
            // @lfy def/mcp/main.lfy:check#check:tool:684b58b87a0a19dc2fdbf39f7e588c713ffb806ffa8ed9df048eb64354a7be4b
            // @lfy def/mcp/main.lfy:check#check:tool:2feecd005d1f5b032cc009fa850348ac4256213d1b2ce6bd9bfff1963661d055
            // @lfy def/mcp/main.lfy:check#check:tool:d01144c9d33cf8a9ac5454fa795448019dc9005930ef3c1f9e38f1bb5e2ef5da
            |session, arguments| check(session, arguments.string("unit")?, arguments.optional("target")),
        ),
        // @lfy def/mcp/main.lfy:mark#mark:tool:0f8e6f40a5d6de867cde8b1be5b3fc853845b786664671c550f9dad09eb01911
        // @lfy def/mcp/main.lfy:mark#mark:tool:d0022315029fb169381cd2230a938407632d7d0dd0131fbc83073feec1ae22af
        // @lfy def/mcp/main.lfy:mark#mark:tool:0b9567706ca9a8b66a0757d097e5fd9c7e6df936098d4aeb7f7e6834a6635d71
        // @lfy def/mcp/main.lfy:mark#mark:tool:5295f5acf77612da1ff8627c746c87ccff5a0dbb1770c05ed869d1ed28d6c7a7
        Registered::new(
            "mark",
            "Write a marker line into one of this compiler run's outputs, or rewrite to a new id every marker of its outputs that names an old one, so that no id is typed by hand",
            // @lfy def/mcp/main.lfy:mark#mark:tool:44369bf678415e07ca9c08b82b57c5fe7969e2e9850e765a8f32faeab4b784c4
            vec![
                // @lfy def/mcp/main.lfy:mark#mark:tool:644ced5c6aeca4f5ba2d0a64656faf7dc4224aa51d7dcfc2c46b2d9a241fb040
                // @lfy def/mcp/main.lfy:mark#mark:tool:84d31d74a162783af16e3e2c1bbb0339be256b810408f3869f52a6d8f243ec5e
                // @lfy def/mcp/main.lfy:mark#mark:tool:80130108b96245bc7a9d383a2cfec314326969ff4054bab4ed3dc8f54da23386
                // @lfy def/mcp/main.lfy:mark#mark:tool:745f570e06eb5111681f3c579ae61468329b1f2ae73381dfb2d99e3ba743c32f
                ToolArgument::new(
                    "id",
                    "the id a marker names afterwards: for a rename, the new id; for an insertion, the id of the criterion or test the new marker answers for, left out for a marker naming only its entity",
                    "string",
                    false,
                ),
                ToolArgument::new("replaces", "for a rename, the id every marker to rewrite names now", "string", false),
                ToolArgument::new(
                    "file",
                    "for an insertion, the path of the output file to insert the marker into, relative to the root",
                    "string",
                    false,
                ),
                ToolArgument::new(
                    "line",
                    "for an insertion, the line of that file the marker is inserted before, counting from 1; one past its last line appends it",
                    "number",
                    false,
                ),
                ToolArgument::new(
                    "entity",
                    "for an insertion, the entity the marker names: an identifier, or an owner's identifier, a dot, and a member's name; a definition file's path and a colon before it limits it to that file",
                    "string",
                    false,
                ),
            ],
            // @lfy def/mcp/main.lfy:mark#mark:tool:d98367d940b3e529d2e08602bdac8f47fd16cec96553f895f7018870629df196
            // @lfy def/mcp/main.lfy:mark#mark:tool:7ac635ccb304b659fae9f602760e20e353f0dd9965c63cf87f39c14388f43a82
            // @lfy def/mcp/main.lfy:mark#mark:tool:27d6627a321b91c18b762a15bd13e0a3acdf4998ac63354bd71ea16d6583b690
            // @lfy def/mcp/main.lfy:mark#mark:tool:385acec2857321e10454e5a802b16a10ef28dc7faa7ad1d0368c0be3d5f107df
            // @lfy def/mcp/main.lfy:mark#mark:tool:dd84a3f71e4f4f6435a8276ad2cf10761a7252093a95113c1fed9f428efc69fb
            // @lfy def/mcp/main.lfy:mark#mark:tool:c460de7b205e88646a18f16cc7fa76d5a2725c3f7b52b73abbc413c79548c188
            // @lfy def/mcp/main.lfy:mark#mark:tool:c9cac0d319222b17058eb203cf66e762b5dfcc60d8542ba572c63268cf5d241f
            // @lfy def/mcp/main.lfy:mark#mark:tool:7230fe00beab23c3f13fc1407ab1d636b4e55a8a1725832980d751e19d6a1f39
            |session, arguments| {
                mark(
                    session,
                    arguments.optional("id"),
                    arguments.optional("replaces"),
                    arguments.optional("file"),
                    arguments.optional("line").map(|line| counted(line, "line")).transpose()?,
                    arguments.optional("entity"),
                )
            },
        ),
        // @lfy def/mcp/main.lfy:submit#submit:tool:46e6a49028a494b7d6bb872cd0cc63d6d03e25fffce40f625b0facd57b49a725
        // @lfy def/mcp/main.lfy:submit#submit:tool:8f4adcbdfc16d749d9b83d55424fdb414dca3efc886559174e902fd2a4912a79
        // @lfy def/mcp/main.lfy:submit#submit:tool:46c97b8c41d5f41394e9c6efbaf1f3bdc24c40082759615fb1b06732fb2d02c7
        // @lfy def/mcp/main.lfy:submit#submit:tool:59836951e15ef484cb381d25b49b247ff2da58ae11631e91ff190839871ce24d
        Registered::new(
            "submit",
            "Say how this compiler run ended: done when every output is written, blocked when it cannot proceed, or clarify when only a person can decide",
            // @lfy def/mcp/main.lfy:submit#submit:tool:27e3a76662e3a4a2d2275aa703d2b835bd68c51282753e21b9337c744d37e5d6
            vec![
                // @lfy def/mcp/main.lfy:submit#submit:tool:87847e0f685515fe21c083343037e22e14155b3956e0220e0c6c17c145fd682d
                // @lfy def/mcp/main.lfy:submit#submit:tool:f5d375e7f92630c7c75c6af5a62d636df1c895f2abf6280b5fc27486624dd3d6
                // @lfy def/mcp/main.lfy:submit#submit:tool:5e8f39a0f84b057b8516d3aba402f08b7a867533e314d8ede4294d9169055ec1
                // @lfy def/mcp/main.lfy:submit#submit:tool:dd8cc3696294c48c92600acd3e1cc71e6e50459a5f971d53856d37d8d18509ec
                ToolArgument::new("outcome", "done, blocked, or clarify", "string", true),
                ToolArgument::new(
                    "message",
                    "for blocked, the reason, quoting the criterion; for clarify, the one question; for done, a note that is no part of the outcome, or empty",
                    "string",
                    true,
                ),
            ],
            // @lfy def/mcp/main.lfy:submit#submit:tool:e1cdea82bc8c49ba5d42b1a183d864e50dcdcb79c278c71097f6e1c6635ee7ad
            // @lfy def/mcp/main.lfy:submit#submit:tool:61ef1fd9f675009231990424e1bb2d2bcbed26bd90a15c89262acd18f8d3aa64
            // @lfy def/mcp/main.lfy:submit#submit:tool:b04c384cd1c006ae6ca5c0a481d033184e71884090bfd8a530d95e2124ffbc5a
            // @lfy def/mcp/main.lfy:submit#submit:tool:f87ad8b66f74c9462b6bb314454e9746aceb34d3fc56ede64867e6767e61b95b
            // @lfy def/mcp/main.lfy:submit#submit:tool:69f43f991763121fd742f21cdb9b3ca44378a49aa50d5a029da01b958d61fe75
            // @lfy def/mcp/main.lfy:submit#submit:tool:4c16156f137c18f569c4b5529c1eda802ccf1aab677a95ef553a87ea8267602c
            // @lfy def/mcp/main.lfy:submit#submit:tool:f883f2fc8ea86956d67e88fcad8d1e9d60d6e28fd202faf0d872b9c7d78170b8
            // @lfy def/mcp/main.lfy:submit#submit:tool:8c456bf550d67a36f52300668226f9b16da8299ec30096adc20c36603993cf04
            |session, arguments| submit(session, arguments.string("outcome")?, arguments.string("message")?),
        ),
        // @lfy def/mcp/main.lfy:output#output:tool:6ec90ebb27e8df39d72c6327a86558dba615e33de7e1a1320cf3f20be8be9b9b
        // @lfy def/mcp/main.lfy:output#output:tool:4eb131ab31bb445c54c0ce94c545024446ab993437862448bc07d78bd45c18d0
        // @lfy def/mcp/main.lfy:output#output:tool:c72de76b2235f6d72935b0bb9556fdcb35f808b7c57557076b5abef04e1ae663
        // @lfy def/mcp/main.lfy:output#output:tool:d72cb21d0e903b84ed1802adf513079764243da9561991d7767a50a7eb2e2cbd
        Registered::new(
            "output",
            "Where generated code is, with its text or without it: every region that came from one entity, that answers for one criterion or test, or that one output file holds",
            // @lfy def/mcp/main.lfy:output#output:tool:2e6b42c0a83704a168683bcc35f00029ab55fcf28d2f29b1c3de09b006c0a9da
            vec![
                // @lfy def/mcp/main.lfy:output#output:tool:b44f0f08cd4a5a17ed3563ee169631ec0e06701a1866f2bcbb070d4ea652c7d5
                // @lfy def/mcp/main.lfy:output#output:tool:536b816ae7d597b602e24a5aa8afccf72252f366fdbf9adb3e6ee1abaae424ac
                // @lfy def/mcp/main.lfy:output#output:tool:84805fed91235a6a28f19e8f6aa8a7e0de24b33cbd3ef8f1ae5312db98688ddf
                ToolArgument::new("name", "as elfie_find takes it, a requirement id, or the path of an output file relative to the root", "string", true),
                // @lfy def/mcp/main.lfy:output#output:tool:cccf2bf991122a41e5f8d1b615ff93f9a9b92d85f03790b2610579b50c0becde
                ToolArgument::new("target", "the identifier of one target; every target when left out", "string", false),
                // `text` has a default, so it is not listed as required.
                // @lfy def/mcp/main.lfy:output#output:tool:cccf2bf991122a41e5f8d1b615ff93f9a9b92d85f03790b2610579b50c0becde
                ToolArgument::new(
                    "text",
                    "whether each region's line is followed by its code; false gives only where the regions are, an outline of an output file",
                    "boolean",
                    false,
                ),
            ],
            // @lfy def/mcp/main.lfy:output#output:tool:c74c2a14496848786c2a9b0c1df1da9d6b9205300b34a2f42d670ce562aa0cf3
            // @lfy def/mcp/main.lfy:output#output:tool:81224c8b94d715808fe36a32d50ac22cb7b68f3aaf9efac14a04a2d3ba729de1
            // @lfy def/mcp/main.lfy:output#output:tool:30281c2c4ab3871ba9d3e8e1d4bd3865e4ffa3438c789f65733035c5008a4f9b
            // @lfy def/mcp/main.lfy:output#output:tool:3e79b4342df7d8ac276a343f02bf39ffd5636a96ef3ba4641642d00b806447e4
            // @lfy def/mcp/main.lfy:output#output:tool:76a973ce9145a4809d1d1de1a68875e33067542ee119844cf65ac55c2d7affca
            // @lfy def/mcp/main.lfy:output#output:tool:1e4baa6263f16aec686a93f72ebe9e48822140f1dbbcc30be00356c10db27e08
            // @lfy def/mcp/main.lfy:output#output:tool:1efeaccaafd14ce38d6aacf01412cd2561c720263e5fcc9555e78c00c0bc642c
            // @lfy def/mcp/main.lfy:output#output:tool:355f2e4745bc2e753294ba137c3f91cae87ebf6bc265f34a566e5fd7b744b62a
            |session, arguments| {
                output(
                    session,
                    arguments.string("name")?,
                    arguments.optional("target"),
                    truth(arguments.optional("text"), true),
                )
            },
        ),
        // @lfy def/mcp/main.lfy:search#search:tool:38ffd83bb0a3a18ad44b57a02e041c06b560ae946cbab843e22f39a99e9aac66
        // @lfy def/mcp/main.lfy:search#search:tool:d2c7b2635329f74a09d591b5a08a10bf91aec95b06cf165df0d237df2bfe4442
        // @lfy def/mcp/main.lfy:search#search:tool:49b02b5a6852ab0082ddca33e30f1e4557cf5670633599b4da7d94b54afc74a0
        // @lfy def/mcp/main.lfy:search#search:tool:dffd65949512cdddb7735f1aeed21e452cf605acabf06b395717014263c1c4e8
        Registered::new(
            "search",
            "The lines of generated code that match a regular expression, each with where it is and the entity, and the criterion or test, of the region it is in",
            // @lfy def/mcp/main.lfy:search#search:tool:84b35a27f4c75349de61c3a20a30bb45eead886577edc895af4e407810c893a3
            vec![
                // @lfy def/mcp/main.lfy:search#search:tool:8cabf755d8b4ed9c400ddc760b3676b7d288d84ef6a4b4d3588840ee0fb85e46
                // @lfy def/mcp/main.lfy:search#search:tool:502a513b0a0681b5d9af190aa237ed651ee77b387a6b91c00943b77203be2a26
                // @lfy def/mcp/main.lfy:search#search:tool:1e8cb253167c19b0e2e9148d8b4dd61e332b68d5b13ccf6aa0523572759ef678
                ToolArgument::new(
                    "pattern",
                    "a regular expression, matched against each line of the outputs without its line break",
                    "string",
                    true,
                ),
                // @lfy def/mcp/main.lfy:search#search:tool:32aab10a54d3615f163e0fbc4cd259bb8fdce07cc79bde7fddfbe0fa16fba954
                ToolArgument::new(
                    "file",
                    "the path, relative to the root, of one output file or of a directory under an output directory, to search only under; every output when left out",
                    "string",
                    false,
                ),
            ],
            // @lfy def/mcp/main.lfy:search#search:tool:8a867d96708fad948dfe52a7145345a0d001ac770d16f11460714158c328ec29
            // @lfy def/mcp/main.lfy:search#search:tool:4880440147d84d90bbd981904421e3f06c8c23cac72f4ca0f650d96ae8f43cbb
            // @lfy def/mcp/main.lfy:search#search:tool:49b79ac88e68ac0c8f20e55a2f107e9adcfe236d66979aabf0ee57746aa50a0a
            // @lfy def/mcp/main.lfy:search#search:tool:6181491e41c7534b36495a0ec649e67956a35e780b4963454e225fc0b554754e
            // @lfy def/mcp/main.lfy:search#search:tool:c171f43a45ba4151b6a0bdb64449c032c1bf9926719152f7a9c0142c5de27d5e
            // @lfy def/mcp/main.lfy:search#search:tool:83e2124f3a0907f079adde2fc91d328be754c3ab340536e6154b1c98198a10a9
            // @lfy def/mcp/main.lfy:search#search:tool:215037b5f46848fb934ed18ac570f9ed009eab54e0614959a956d5c99abd3f9b
            // @lfy def/mcp/main.lfy:search#search:tool:d86ec47169d8d453d526a2ae4c472edebcf52bce2a253c8461f5b2a683122d42
            |session, arguments| search(session, arguments.string("pattern")?, arguments.optional("file")),
        ),
        // @lfy def/mcp/main.lfy:source#source:tool:177439079fd384c431d5f00043095ba02ce9d5d960147cc9393b04b553d4026a
        // @lfy def/mcp/main.lfy:source#source:tool:0e837a65b26e2ac3b8f7d64f5dcd769d6785bb879e63c4bd5761e9543fa11814
        // @lfy def/mcp/main.lfy:source#source:tool:5e06f9d1bab537b3f12cc6f40e5b43d875169e6dde2af548117c563a52564a91
        // @lfy def/mcp/main.lfy:source#source:tool:a69258bb42344c0bd286241ad3495c363708649d40587973d2446d8dd7a67ba8
        Registered::new(
            "source",
            "Where one line of generated code came from: the definition file, the entity, and its line",
            // @lfy def/mcp/main.lfy:source#source:tool:86bcade62e065481b0e71151933c5e24311e5ae5c768cf621808ea4c2804928b
            vec![
                // @lfy def/mcp/main.lfy:source#source:tool:c6487b689eaab2ccd0e199ee1b320e9a2665cee02bdd6cb984e65e94e9b22b46
                // @lfy def/mcp/main.lfy:source#source:tool:3e6dbae45a3eab388dff4594bb23659b4840e880ec944f2c8b52d151427f8e42
                // @lfy def/mcp/main.lfy:source#source:tool:5bf72ec14e48137ed89a57ea542187e7e9c485c8a42ba64d90d18cc298f7a0fe
                ToolArgument::new("file", "the path of an output file, relative to the root", "string", true),
                // A line is a number, and is listed as one. No parameter of `source` is
                // optional or has a default, so no argument of it is left out of `required`.
                // @lfy def/mcp/main.lfy:source#source:tool:582533c08bce4d6d3774ac52fd022fbdcbd0c60b58c1b8626ecf62dc622eeab5
                ToolArgument::new("line", "a line of that file, counting from 1", "number", true),
            ],
            // @lfy def/mcp/main.lfy:source#source:tool:e0b14b4706f8dae10a85f002eb93be09aa1f890a3140dca03046fdc0ed52915a
            // @lfy def/mcp/main.lfy:source#source:tool:c88f2812efd1f28c24567ebe4b778a45c64bbbdecc46e8aa143b46ec4fa686e3
            // @lfy def/mcp/main.lfy:source#source:tool:b9fc9d843cff425a9166223d1c90501b53db07c6fa1d662bf901e4c44542d92e
            // @lfy def/mcp/main.lfy:source#source:tool:09d86a7ebefb49c93cedccbd396bfe51c1fd158af3e4aea4f45843eebd4da3a2
            // @lfy def/mcp/main.lfy:source#source:tool:6d9dd51635b7204772d74da138f616b326aac0af4536ec450e8f26c7c9650de0
            // @lfy def/mcp/main.lfy:source#source:tool:f64e48f070058897b4b82d2d2649c81b9155f184931f4b71a81c2bb9163b850a
            // @lfy def/mcp/main.lfy:source#source:tool:36b64abf8443d64fac524ad81fcaa9c9e1ece561cdf7b0a6ff1e66dba001fac3
            // @lfy def/mcp/main.lfy:source#source:tool:933096756b3b232e53313f802792772462a7923e6772c081a67cd7f54440b7e4
            |session, arguments| source(session, arguments.string("file")?, counted(arguments.string("line")?, "line")?),
        ),
        // @lfy def/mcp/main.lfy:changes#changes:tool:ec6bca8a3cbc36434de614909a53653a3b0f25e84dfad4d7c7318de2bb35879e
        // @lfy def/mcp/main.lfy:changes#changes:tool:0f166abc4aab53e3c0ebb7ea52e8dc8852f3d4d8d10729c84048c79f3b95c964
        // @lfy def/mcp/main.lfy:changes#changes:tool:23f664f16c7c7c3856d8730e9420e96c25250a86fc013c64a2dc86e9ba9690fb
        // @lfy def/mcp/main.lfy:changes#changes:tool:bb175b58bc93d07549300b484ff2382a881c6cf49c12e9210f23f57c0c852709
        Registered::new(
            "changes",
            "What differs for each entity of a unit since its outputs were last accepted",
            // @lfy def/mcp/main.lfy:changes#changes:tool:ec3c7defff9f765cc593846440341f6c9ab6818d3c051ab9101385709365d3fb
            vec![
                // @lfy def/mcp/main.lfy:changes#changes:tool:490033cd4b258e1604cf1c128b2ae6ab1127d571246c9bbddfa64de843ff9ce7
                // @lfy def/mcp/main.lfy:changes#changes:tool:07440d03005075a867105049b30aaf727dfbcb8271af1b1538caca58cf4903ec
                // @lfy def/mcp/main.lfy:changes#changes:tool:0c631194a5026a108cc79d09dc3877b6278e686a4a4b0853d4b441b65c8bb6fc
                ToolArgument::new("unit", "the stem of a unit, as elfie_units lists it", "string", true),
                // @lfy def/mcp/main.lfy:changes#changes:tool:7669d2bede3f5a5167af6c96cad8a2292f2328bbbffa4e4e9fbb74a18d346d49
                ToolArgument::new(
                    "target",
                    "the identifier of the unit's target; the first target in Workspace.targets order that has a unit with that stem when left out",
                    "string",
                    false,
                ),
            ],
            // @lfy def/mcp/main.lfy:changes#changes:tool:07f057e8b5e7087629b58f0a25b19295add4159a054a4a1dbb02181687082f5c
            // @lfy def/mcp/main.lfy:changes#changes:tool:1085cc6532a1e368462f1f090f302b718569139cd8f264ab5261a97307262bb0
            // @lfy def/mcp/main.lfy:changes#changes:tool:5226f13af1af781bf79cce55820c206559e6603718d66ea566a8a88c040a2247
            // @lfy def/mcp/main.lfy:changes#changes:tool:f5c4e810ed5d871ec0a3742590856586a4bbb180829fa0a37f5bec6655f6667d
            // @lfy def/mcp/main.lfy:changes#changes:tool:aa51c0384474065c01079886c2fa98ccd1c60e25ba6a48a34cb22571fba8088f
            // @lfy def/mcp/main.lfy:changes#changes:tool:1b481949890f42d1353e622ca00080011c52df05703303c8a4a0073f55d96380
            // @lfy def/mcp/main.lfy:changes#changes:tool:7956a05a34c47e5177fb4661eb64d4384e77763432be57f4b4db3f2d15aa972b
            // @lfy def/mcp/main.lfy:changes#changes:tool:0dc4b671044e5b169084445b22daec236489f564fb85985cca66cc6a88b680d5
            |session, arguments| changes(session, arguments.string("unit")?, arguments.optional("target")),
        ),
        // @lfy def/mcp/main.lfy:review#review:tool:cae93d1e0915b7e3244718911a45cd0e8a145cb441856eec83c09a11547ff0a6
        // @lfy def/mcp/main.lfy:review#review:tool:70891d9a91ae04731a72b80d2f6c86b173925fc777f95ee77561af6d5b77ef37
        // @lfy def/mcp/main.lfy:review#review:tool:90f43a9d7a341c4448c88bdb65846b1958c620269b4928c1820b8bdd36a04645
        // @lfy def/mcp/main.lfy:review#review:tool:851e5fb699313c73590128b97abe97d419edd52546bce09e0341b62db2d5f897
        Registered::new(
            "review",
            "The full review request for one batch: every criterion and test with where the regions generated for its entities are, and how to report",
            // @lfy def/mcp/main.lfy:review#review:tool:4db83e86e12ebe1d2b0275a0875d451913e2e33a2985e19be1239eeb55afcf55
            vec![
                // @lfy def/mcp/main.lfy:review#review:tool:176e2dae9d3fa2eeb0d9e1f4cb3f69e21eddf6a4ee54eeec769831da57d33106
                // @lfy def/mcp/main.lfy:review#review:tool:f12b54937440941b301f9813502fcf02c0f5c15985fa86f3c18d3be8971e926c
                // @lfy def/mcp/main.lfy:review#review:tool:26448cfe30979765e8825f454475ab554f07d764985e474795c7f379bcb790b8
                ToolArgument::new("batch", "the identifier of a batch, as elfie_units lists it, or the stem of one of its units", "string", true),
                // @lfy def/mcp/main.lfy:review#review:tool:c9eb9c029bfc76ba91183426864af318ca0ba9c40d66aa878456a1365ec79882
                ToolArgument::new("target", "the identifier of the batch's target; the only target when left out", "string", false),
            ],
            // @lfy def/mcp/main.lfy:review#review:tool:a214aeda7f2dcdb8d57836782e9f7950b99a0cbffd7ded1e4d02512863333bce
            // @lfy def/mcp/main.lfy:review#review:tool:c5cc7af933cc365db492e83cc2677c5381c9cc7293ef97f115d12d762c1b2584
            // @lfy def/mcp/main.lfy:review#review:tool:8c428dc0edd273685caaabe72eb81a080104324829ee2f95f5e3ccab5c86a3ca
            // @lfy def/mcp/main.lfy:review#review:tool:6136567dbb52c10db4b93ba491d3db66273bcb1eb7671f8831bb2f8567157ce6
            // @lfy def/mcp/main.lfy:review#review:tool:1f10b2e2381645fd0db250b709faee6d4fcb7777f16fffff10731a3c197f94cc
            // @lfy def/mcp/main.lfy:review#review:tool:3d16d076268d4bb0d06e54067ea264107a204761e51fedb45c43d4da6266e867
            // @lfy def/mcp/main.lfy:review#review:tool:783352704aa244702e71b187abc170d3a55eb7ee3b01dddbdffd6b21219355e8
            // @lfy def/mcp/main.lfy:review#review:tool:5c26dbd79645c2274940ae02bf61ebc5954a4c194e657a89b203f1976fb73dce
            |session, arguments| review(session, arguments.string("batch")?, arguments.optional("target")),
        ),
        // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:63522dbcfabc1cddee57ee188e7c75c7a4c6576f807ce8a0a4dd4ffe69250ca7
        // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:d895cc0bf43e7d708d5eba5fe5af8ca74d3565a1d0ebba4ffd5d274cb025ebeb
        // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:14933b1ce190a0da8af31a4b31f6a8dc1a38277ec20a83476f9cb01e93f24e8e
        // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:3f4ec62a56477a71ffce1d8e9448a9c31cbe5891bc4426c362c26086454332b3
        Registered::new(
            "globalReview",
            "The review request for every global criterion and test, once for the whole program, with where the regions that answer for each are",
            // Its one parameter is the session, so it holds no argument, and nothing further
            // is asked of an argument's name, type, description, or whether it is needed.
            // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:82e3fa7ae3f9252bf237de6847f6e4f376d8359dbf5534ae09480fd3423e9c90
            // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:37bd7f504566a0de5dd33fd249659e845dfa71e53baf0404c88d78abdb413040
            // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:dfd01992e60e996e02f3abdace15928aa26c171d40598e6ef831122cca7dc13e
            // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:e6002fa85fc2e5cb7599cd691c03f7c44cd15a3a91606fb73ee48193ac48e0d4
            // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:5f7ebced7900d0c3e6313eb42c7094e11d5a40a106f18a1451685cf793ba8c45
            vec![],
            // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:49180ae21ec1d01742b6b16217a91564016033aded3011a92220096a63031b98
            // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:b965850b0512ad933fd27191c3cf2fb9d268a9f8c7f28a2fa66402f7f6302d20
            // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:f2d3f884428ba9b23752fbb54426c519c209d4c0804df1b4c254f81834cbe0a9
            // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:116e7e050e8da1a1cfcfd7ac5bbea519c9e62b5349a6a40b2b8f69de71ace555
            // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:ddb77eff3445623b782407a0b541cab4cd213bfe13fafedbde6e2c9f1c58926e
            // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:1bd58ca877dd1efffc6fcfb3da9e0bc8a89f596f6bc8293a4de3c8e6ebe8a903
            // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:dd97712ea04dd944092a306fe48514d200aca70d8859b91716e78311d351b34d
            // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:a465a324a0a88351aa9aceb3ac8748f741a2f91ba57cccc57bc75ea168bbac9b
            |session, _| global_review(session),
        ),
        // @lfy def/mcp/main.lfy:report#report:tool:6c734414d43397d25076d1deee1bd29dc4629bfe66542bddca3ce1cdce3d96fb
        // @lfy def/mcp/main.lfy:report#report:tool:ff191fef77edc21f5b1b6379da11a026007625ceb2ea84877365d76b43d3a88f
        // @lfy def/mcp/main.lfy:report#report:tool:75d6b6bdeb8d8a74966583e1c58c03775a0f243350447c165a263304032f268a
        // @lfy def/mcp/main.lfy:report#report:tool:a1742728abf4993bf168d78ca9c2423c2bc5bf5a43161ca1f56dfa0a9098fc27
        Registered::new(
            "report",
            "Submit the verdict on one criterion or test of the review this session is for",
            // @lfy def/mcp/main.lfy:report#report:tool:e2565b57b91b75898a51429de938829bbc20515ebf54db2921b4ff1bf9a54879
            vec![
                // @lfy def/mcp/main.lfy:report#report:tool:69094a572908b566940e33fea2e59850769a5b07a88194d58c582bd3fe9e78dc
                // @lfy def/mcp/main.lfy:report#report:tool:189e78800b2582a76073a4234cfc05713af072e7c7dedd6248b2388cf0286d40
                // @lfy def/mcp/main.lfy:report#report:tool:54a6b370411e4c4595c18f8704defe58865d880d81e0cf482dc2d9f5a4559834
                ToolArgument::new(
                    "id",
                    "the id of the criterion or test, as the review request gives it; may be left out when file and line name it",
                    "string",
                    false,
                ),
                ToolArgument::new(
                    "file",
                    "the first half of the place the review request gives the criterion or test, such as def/a.lfy",
                    "string",
                    false,
                ),
                ToolArgument::new("line", "the second half of that place", "number", false),
                // @lfy def/mcp/main.lfy:report#report:tool:cb1276d6f803e4db72b776992eaa443eb5a2a3262545b7da8843f1c39d27ac7c
                ToolArgument::new("status", "satisfied, violated, or unverifiable", "string", true),
                ToolArgument::new(
                    "evidence",
                    "the output path and line range judged, such as crates/elfie-core/src/x.rs:120-134; empty when none",
                    "string",
                    true,
                ),
                ToolArgument::new("note", "why, in one line; for a violated one, what the code does instead", "string", true),
            ],
            // @lfy def/mcp/main.lfy:report#report:tool:ba274d3143e08dca992403814eb84d371a75a9ccf10bdc3886b7f4231fee61da
            // @lfy def/mcp/main.lfy:report#report:tool:6dc5f130f1e89693dbd6fa0880cdf2ac8d2707f1a139f3fb6ddb612870f43651
            // @lfy def/mcp/main.lfy:report#report:tool:e007736a21684f3a220132392343eddcb6da76ee00624a48a375f7ebc96a48e8
            // @lfy def/mcp/main.lfy:report#report:tool:47ce9c03cbce9a8af5a39f9f8a921dda6ab3b03d6962aa825201f5a68c964549
            // @lfy def/mcp/main.lfy:report#report:tool:dcade66a6165424800631bce9e97b04fd1a999eedb40dd6d92fc4caeb2fcbc1a
            // @lfy def/mcp/main.lfy:report#report:tool:24cf190dc5ccc762c260fd7f9a21fc7a10f1379914fe96ad237b9e95a70fcaef
            // @lfy def/mcp/main.lfy:report#report:tool:32260c0058131ab628d7a49257a394dc8b025e50525f206439eae3edd222e245
            // @lfy def/mcp/main.lfy:report#report:tool:82ad2f8ed7450e51414f398a901f29cfad00fbff59cc1159c376d09c2bfefc65
            |session, arguments| {
                report(
                    session,
                    arguments.optional("id"),
                    arguments.optional("file"),
                    arguments.optional("line").map(|line| counted(line, "line")).transpose()?,
                    arguments.string("status")?,
                    arguments.string("evidence")?,
                    arguments.string("note")?,
                )
            },
        ),
        // @lfy def/mcp/main.lfy:build#build:tool:437d060ed5ca74732574dd1afab19a5a166957ef20fcdb97bf4d590041c48336
        // @lfy def/mcp/main.lfy:build#build:tool:88906b89a3fff75afd55acabcbfcfcad2e07d893ba2ff2c1a9fbe35abfcb1f8d
        // @lfy def/mcp/main.lfy:build#build:tool:e0309114048649453e6790ef4c6deeabd6ec9481b22aa5d6159c671535d25832
        // @lfy def/mcp/main.lfy:build#build:tool:d0bfe30fbbef86bd5724c6c59435c754542f0b44ae99df590217de4b3e02fd28
        Registered::new(
            "build",
            "Run the target's build command in this run's directory, narrowed to a scope when one is given, and say whether it passed, with its exit code and the end of its output",
            // @lfy def/mcp/main.lfy:build#build:tool:a89edaee38beabc1ef0df886f2a21cc45ce985962b64e3309e470a2a02e9b576
            vec![
                // @lfy def/mcp/main.lfy:build#build:tool:a3290c07f19e4214355190476d5ab8cc6a9bba3ec08cfedee6e90f738f4dcfc0
                // @lfy def/mcp/main.lfy:build#build:tool:cc821fff828717e6310df1103a08b0267f91fc98bbc71bc89165e1224840cb87
                // @lfy def/mcp/main.lfy:build#build:tool:501c436862b393c2162f170908d65e859adf42a2afe8973015becc465cc36df3
                // @lfy def/mcp/main.lfy:build#build:tool:d7aadcec979a675c909a3493228563ee20b979feaf86ec325edc207f6683b4f5
                ToolArgument::new("scope", SCOPE_ARGUMENT, "string", false),
                ToolArgument::new("target", TARGET_ARGUMENT, "string", false),
            ],
            // @lfy def/mcp/main.lfy:build#build:tool:dc3440b1ed927411929bf777ccdee3fa195a29b2e360b86349621d1cd28f0899
            // @lfy def/mcp/main.lfy:build#build:tool:61838c6a72b2030685de2c121a5cedbd7fcae01c5f420817b479871235807b2c
            // @lfy def/mcp/main.lfy:build#build:tool:e12dfbde34a596f9d4adba2bbf56c5c9e8bb66a94ba4d495da554a8e9f24e9e6
            // @lfy def/mcp/main.lfy:build#build:tool:9c0329a8cedbc80328b8f48979d3e131ae4e06f573e61d535e026d3cc3016667
            // @lfy def/mcp/main.lfy:build#build:tool:f717af6b96f7f183061064eaf6c5100974aa1a5044351a8b1d1bd849fdff0814
            // @lfy def/mcp/main.lfy:build#build:tool:647bbeea74883056cd3e033a601636e7830b8834fb1c6959e6d67fc25b2e9a2d
            // @lfy def/mcp/main.lfy:build#build:tool:3f6b3e002975d4e0f6c61fa73845c14f4bb28aaab5eeca8bb72b042120eab977
            // @lfy def/mcp/main.lfy:build#build:tool:b896e4ddc78794b2515435559e036b43da3c57899559a9e9fcfab6b19fd23377
            |session, arguments| build(session, arguments.optional("scope"), arguments.optional("target")),
        ),
        // @lfy def/mcp/main.lfy:test#test:tool:5386b37f95ca729944462379291817c9470858369f6cfb9387b834f8767c2b51
        // @lfy def/mcp/main.lfy:test#test:tool:3095733c477eec37202566f8813793d01f848b595701cdd835ffc630ba78f768
        // @lfy def/mcp/main.lfy:test#test:tool:a038afc81a5b6f64a780e0144c3ac1be7ce831938025338a3c18193929918aba
        // @lfy def/mcp/main.lfy:test#test:tool:b781017493ee33fccd6be22cef4cb4b2338a130da8607db5c8a6899222f40fd1
        Registered::new(
            "test",
            "Run the target's test command in this run's directory, narrowed by a filter and a scope when they are given, and say whether it passed, with its exit code and the end of its output",
            // @lfy def/mcp/main.lfy:test#test:tool:4930fab7d3d2c47253afb07544bc3ab8fcd4c5deb1e42ce3f9f8b2dcc6175c30
            vec![
                // @lfy def/mcp/main.lfy:test#test:tool:cbeb0da9f28bfbd6a6d084bb140fca7c043af3a7fee4c0c506d08562fcb13392
                // @lfy def/mcp/main.lfy:test#test:tool:91cfa553c86897863866df4f70a520da5ab64eb9facaea1c00b2c552ee52e2da
                // @lfy def/mcp/main.lfy:test#test:tool:76c25a388f0b4e4e85ec4a0eb794a6b564a41f19f1f17a0f984c9409e38f1193
                // @lfy def/mcp/main.lfy:test#test:tool:430d6d90db0096b765c313099b0425e48c0069b5c3a1f92aa82676371e14f7f3
                ToolArgument::new(
                    "filter",
                    "the words that narrow which tests run, given to the test command as TESTS; every test runs when left out",
                    "string",
                    false,
                ),
                ToolArgument::new("scope", SCOPE_ARGUMENT, "string", false),
                ToolArgument::new("target", TARGET_ARGUMENT, "string", false),
            ],
            // @lfy def/mcp/main.lfy:test#test:tool:c3ee2fa9249263228e75266d415288016fb367dd3e42ea7111abb95b2753f842
            // @lfy def/mcp/main.lfy:test#test:tool:21a65a75a3ed0d3332a816d5fc81762a4aaafa8216d19d4536b1c82ba9fb2ac9
            // @lfy def/mcp/main.lfy:test#test:tool:3073281f62f23eaf3e61e93368285720562153141f52e5c0ef0ed408a1fe7983
            // @lfy def/mcp/main.lfy:test#test:tool:7d2fef009ba6dc46b09fa7aceab547155137968b1e8ed074aad0e0c693ab4950
            // @lfy def/mcp/main.lfy:test#test:tool:805100fc5d20dfd5b05cb25d4737aec89a63dd10d2c9088ca35088065481d956
            // @lfy def/mcp/main.lfy:test#test:tool:8e29089986847bb9e3dd6c6eb1b1d87ac2091a5acb94f321648a76498b553f68
            // @lfy def/mcp/main.lfy:test#test:tool:237ea91ba542b9d07f5394b8c65fc2360ee924a487193374c9ed0fd5796a2d10
            // @lfy def/mcp/main.lfy:test#test:tool:5c20d0aa955cefbb611028a3bb9e2be9b761f1036511d6ba89ffe31082eb4015
            |session, arguments| {
                test(
                    session,
                    arguments.optional("filter"),
                    arguments.optional("scope"),
                    arguments.optional("target"),
                )
            },
        ),
        // @lfy def/mcp/main.lfy:lint#lint:tool:6e14d79b29847057cee32aad56ac37159ec40c848c8cb9346ffbcca0cf4ce022
        // @lfy def/mcp/main.lfy:lint#lint:tool:f0044ba556ab8e00066360dce81c2a584d6ad207e30d0233e25d4294fde9e8a5
        // @lfy def/mcp/main.lfy:lint#lint:tool:9f1b5c7200bd49c99da2c72dd74e0589a75980b35e4b797ecc4b538e18b380a1
        // @lfy def/mcp/main.lfy:lint#lint:tool:fada2d3544a4caf7440370d2fdff08fd8d46235a4307ed27758f2aa426047db5
        Registered::new(
            "lint",
            "Run the target's lint command in this run's directory, narrowed to a scope when one is given, and say whether it passed, with its exit code and the end of its output",
            // @lfy def/mcp/main.lfy:lint#lint:tool:f23edb098a47aff31e4aaab809a03e3db1a479cda385597dafd4ffca5fc96a43
            vec![
                // @lfy def/mcp/main.lfy:lint#lint:tool:9293b2f58b28f6819e289105b3ffdfeddf57cb0b003b969596d58a7cf3cedeab
                // @lfy def/mcp/main.lfy:lint#lint:tool:8415b8b106d669fec27102fd04507c3d814b919d8bcf79f8e7d2650f5fb4faee
                // @lfy def/mcp/main.lfy:lint#lint:tool:8716f81a78f82cfecc907c464ca05907aaaf0ae13cf0d25431b6c89f1e71e1d9
                // @lfy def/mcp/main.lfy:lint#lint:tool:81afa3f50b86d1ed54b7bad2244850fb1e841e53d326e18b8f9188e1037bc426
                ToolArgument::new("scope", SCOPE_ARGUMENT, "string", false),
                ToolArgument::new("target", TARGET_ARGUMENT, "string", false),
            ],
            // @lfy def/mcp/main.lfy:lint#lint:tool:67dfb4c019b54e4680477b0d722fdb00b27b2f4a851b62346c48fa4720da1f48
            // @lfy def/mcp/main.lfy:lint#lint:tool:b0159114c3f88d628e5b62b17ca6b8953b1359d225b4142388fce204476c3aec
            // @lfy def/mcp/main.lfy:lint#lint:tool:d6bc657b557dd76141f7e3de7f7503ec53bf669098e3548458d88f8faea06f71
            // @lfy def/mcp/main.lfy:lint#lint:tool:cdc7eba40cf4a3184b3cb1b0636dbb899a1baf5373f13039407ca6374e147178
            // @lfy def/mcp/main.lfy:lint#lint:tool:aec85fdd371eb5db1dbc0cf90cdf13875cdc4061a7d1e2a567a6ed0e8575762d
            // @lfy def/mcp/main.lfy:lint#lint:tool:3a17eeac12b98c995afdd2992499aa8cfd276689d7504545bff7f9090266ff44
            // @lfy def/mcp/main.lfy:lint#lint:tool:37270271962317584d5e2d9e760a441d5cca8d15e1b084389ac22bbe5fd906cf
            // @lfy def/mcp/main.lfy:lint#lint:tool:5f09fb0d33ecba7c22d7aee2684106b4d835e32c750478ccf6f5f781d36f716a
            |session, arguments| lint(session, arguments.optional("scope"), arguments.optional("target")),
        ),
        // @lfy def/mcp/main.lfy:format#format:tool:a84a6f1ea99ee3c31c458952672bfa71b4b076970b0ea7e5f97fc4ca1ad3476d
        // @lfy def/mcp/main.lfy:format#format:tool:fad2173ad6add7bf6e4a61983e9685caf2ffe4dfb3b1a4887c920667e347acee
        // @lfy def/mcp/main.lfy:format#format:tool:c9525e9b70499077881113750f0011474b3ed51c8f839564643d93637cedd60c
        // @lfy def/mcp/main.lfy:format#format:tool:b844517eed0302f020cc276c74d1c8e649070a57de01e61e1bc8194d3157f1a5
        Registered::new(
            "format",
            "Run the target's format check in this run's directory and say whether it passed, with its exit code and the end of its output",
            // @lfy def/mcp/main.lfy:format#format:tool:6b1377a3f7b741d23cf4ee8f6be9b6e69d161c9b715fd8f55d0247578017bb6c
            vec![
                // @lfy def/mcp/main.lfy:format#format:tool:6f7967f9d4f086abae04ce989ce1d49bea43882ec4bbb31e34aee7bf9417d481
                // @lfy def/mcp/main.lfy:format#format:tool:c7c05422e084fd79bb583a5aa90b70d947898019100fe9d2bd60b4fe23a540f2
                // @lfy def/mcp/main.lfy:format#format:tool:97215d50932f95a6114e77ea1b0963c2a0c4806cae35d0b58fa00664bb7a8647
                // @lfy def/mcp/main.lfy:format#format:tool:41a181e4de5b31903fd19b4fec451a4d3982908e194189ef3b45c691280dd06b
                ToolArgument::new("target", TARGET_ARGUMENT, "string", false),
            ],
            // @lfy def/mcp/main.lfy:format#format:tool:c59813097edc6cf5ab13b506118db9446fd32c0da7b14d873b35f77bc6c773ae
            // @lfy def/mcp/main.lfy:format#format:tool:ded962a70404db26d92fd2f1c148048bf282f6f74294da88a4ad6ff019d80e2f
            // @lfy def/mcp/main.lfy:format#format:tool:54223274bff6ab3fc913d294ebef77a618b91dd99298cb2ba3fb8f0b745a64b8
            // @lfy def/mcp/main.lfy:format#format:tool:8dd8b98f33f85088cb303af3a244f717653a87a913537f313cbc1a7d1975a5a4
            // @lfy def/mcp/main.lfy:format#format:tool:b2c97631e6993afc21bc88c7f3e05498172f0153aab0eae0bfaeb5e964e70fc4
            // @lfy def/mcp/main.lfy:format#format:tool:5f23b74ea1ea2db877ac4897e58e6eb22a1b830eda76d071948abb3c45aedf91
            // @lfy def/mcp/main.lfy:format#format:tool:71b1b0cad7054dac9422dcee77e95c91d38e3b49363a05a35ea51aceb419d7dd
            // @lfy def/mcp/main.lfy:format#format:tool:78b9d6daad5e2b3f5ccc2f0e0f4b7170c43a9c9ea683740b3770712eeddd69fd
            |session, arguments| format_check(session, arguments.optional("target")),
        ),
    ]
}

/// What the `scope` argument of a command tool says, as the fns that take one describe it.
// @lfy def/mcp/main.lfy:build#build:tool:cc821fff828717e6310df1103a08b0267f91fc98bbc71bc89165e1224840cb87
// @lfy def/mcp/main.lfy:test#test:tool:91cfa553c86897863866df4f70a520da5ab64eb9facaea1c00b2c552ee52e2da
// @lfy def/mcp/main.lfy:lint#lint:tool:8415b8b106d669fec27102fd04507c3d814b919d8bcf79f8e7d2650f5fb4faee
const SCOPE_ARGUMENT: &str = "the part of the outputs to run on, as the target's ecosystem names it, such as a package for cargo; given to the command as SCOPE; all of them when left out";

/// What the `target` argument of a command tool says, as the fns that take one describe it.
// @lfy def/mcp/main.lfy:build#build:tool:cc821fff828717e6310df1103a08b0267f91fc98bbc71bc89165e1224840cb87
// @lfy def/mcp/main.lfy:test#test:tool:91cfa553c86897863866df4f70a520da5ab64eb9facaea1c00b2c552ee52e2da
// @lfy def/mcp/main.lfy:lint#lint:tool:8415b8b106d669fec27102fd04507c3d814b919d8bcf79f8e7d2650f5fb4faee
// @lfy def/mcp/main.lfy:format#format:tool:c7c05422e084fd79bb583a5aa90b70d947898019100fe9d2bd60b4fe23a540f2
const TARGET_ARGUMENT: &str = "the identifier of the target; the target of the session's batch, or the only target, when left out";

/// A boolean argument read back from the text it was spelled as, with what its parameter
/// defaults to when a call leaves it out.
// @lfy def/mcp/main.lfy:output
fn truth(text: Option<&str>, default: bool) -> bool {
    text.map_or(default, |text| text == "true")
}

/// What a fn of this module carries besides `tool`: `compilerTool`, which acts on the batch
/// and the request of a compiler run, `commandTool`, which runs the target's command for one
/// operation, or neither.
// @lfy def/mcp/main.lfy:serve#serve:serve:6d46022fc7731233d153763aaa5a974d404b799d2e7f3d3535aaccd0a9895518
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Carried {
    /// It carries `tool` alone.
    None,
    /// It carries `compilerTool`.
    Compiler,
    /// It carries `commandTool` with this operation as its argument.
    Command(Operation),
}

/// What the fn a tool name belongs to carries besides `tool`.
// @lfy def/mcp/main.lfy:serve#serve:serve:6d46022fc7731233d153763aaa5a974d404b799d2e7f3d3535aaccd0a9895518
// @lfy def/mcp/main.lfy:host#host:host:5e071f6dac08ca55d36f2e5b605c6cec0298a9dfbc6b6128864a422ec69e53d3
// @lfy def/mcp/main.lfy:host#host:host:7afa5703849ae259b65ae3bd7d32057445477122e6db4ac372c99154d461ea95
fn carried(name: &str) -> Carried {
    match name {
        "elfie_task" | "elfie_dependency" | "elfie_mark" | "elfie_submit" => Carried::Compiler,
        "elfie_build" => Carried::Command(Operation::Build),
        "elfie_test" => Carried::Command(Operation::Test),
        "elfie_lint" => Carried::Command(Operation::Lint),
        "elfie_format" => Carried::Command(Operation::Format),
        _ => Carried::None,
    }
}

/// The targets of a session: the target of the units of the binding's batch when it has one,
/// and every target of the workspace when it has none.
// @lfy def/mcp/main.lfy:host#host:host:f5d226a776398720183cca8da8ecac0ec6273fcfb6503d5533239adec7e2322b
fn session_targets<'w>(workspace: &'w Workspace, binding: Option<&Binding>) -> Vec<&'w Target> {
    // @lfy def/mcp/main.lfy:host#host:host:f5d226a776398720183cca8da8ecac0ec6273fcfb6503d5533239adec7e2322b
    if let Some(binding) = binding
        && let Some(batch) = &binding.batch
    {
        let mut found: Vec<&Target> = Vec::new();
        for &unit in &batch.units {
            if let Some(unit) = binding.plan.units.get(unit)
                && let Some(target) = workspace.targets.get(unit.target)
                && !found.iter().any(|known| known.identifier == target.identifier)
            {
                found.push(target);
            }
        }
        return found;
    }
    // @lfy def/mcp/main.lfy:host#host:host:f5d226a776398720183cca8da8ecac0ec6273fcfb6503d5533239adec7e2322b
    workspace.targets.iter().collect()
}

/// The name of an operation's member, as `Operation` spells it: the word its value begins
/// with, before the colon that introduces what the operation is for.
// @lfy def/mcp/main.lfy:build#build:commandTool:394aef9de6082d12b6f9d58c8eaae738d8ab0f6aef3341b5fda12620fe6500a6
fn operation_name(operation: Operation) -> &'static str {
    operation.value().split(':').next().unwrap_or_default()
}

/// The command of a target for one operation whose line is set, when it has one.
// @lfy def/mcp/main.lfy:build#build:commandTool:654d31e36e3f112d06aa532374f30fda36190672b3a323580052a1c0f96e1401
// @lfy def/mcp/main.lfy:test#test:commandTool:82d963f45b5a888916a03e865844e4c1961de319774b2584f65489dee966cdb5
// @lfy def/mcp/main.lfy:lint#lint:commandTool:db94076681d45420de07709b25f27480d746751609aa1bb42a77c77d453001eb
// @lfy def/mcp/main.lfy:format#format:commandTool:a09c5e43d4c38d2e3196576c80db3cf1cb918134842760d0c7e7aa134b552a6b
fn line_for(target: &Target, operation: Operation) -> Option<&str> {
    target
        .commands
        .iter()
        .find(|command| command.operation == operation)
        .and_then(|command| command.line.as_deref())
}

/// The names of the tools a session is offered.
///
/// A session over standard input and output is offered every fn of this module that carries
/// `tool` alone but `report`, which submits the verdict on a criterion of a review it is not
/// running: it has no binding, so no run's directory to run a command in and no batch whose
/// request to read. A compiler's hosted session is offered those, `report` apart, together
/// with the fns that carry `compilerTool`; a verifier's is offered `report` and every fn
/// carrying `tool` alone but `check`, which answers whether a unit's outputs would be
/// accepted. Both are offered a fn carrying `commandTool` when a target of the session has a
/// command for its operation.
// @lfy def/mcp/main.lfy:serve#serve:serve:6d46022fc7731233d153763aaa5a974d404b799d2e7f3d3535aaccd0a9895518
// @lfy def/mcp/main.lfy:host#host:host:5e071f6dac08ca55d36f2e5b605c6cec0298a9dfbc6b6128864a422ec69e53d3
// @lfy def/mcp/main.lfy:host#host:host:7afa5703849ae259b65ae3bd7d32057445477122e6db4ac372c99154d461ea95
fn offered(tools: &[Registered], workspace: &Workspace, binding: Option<&Binding>) -> Vec<String> {
    let targets = session_targets(workspace, binding);
    let withheld = match binding.map(|binding| binding.role) {
        Some(Role::Verifier) => "elfie_check",
        Some(Role::Compiler) | None => "elfie_report",
    };
    tools
        .iter()
        .map(|registered| registered.tool.name.clone())
        .filter(|name| name != withheld)
        .filter(|name| match carried(name) {
            Carried::None => true,
            // @lfy def/mcp/main.lfy:serve#serve:serve:6d46022fc7731233d153763aaa5a974d404b799d2e7f3d3535aaccd0a9895518
            // @lfy def/mcp/main.lfy:host#host:host:7afa5703849ae259b65ae3bd7d32057445477122e6db4ac372c99154d461ea95
            Carried::Compiler => binding.is_some_and(|binding| binding.role == Role::Compiler),
            // @lfy def/mcp/main.lfy:host#host:host:5e071f6dac08ca55d36f2e5b605c6cec0298a9dfbc6b6128864a422ec69e53d3
            Carried::Command(operation) => {
                binding.is_some() && targets.iter().any(|target| line_for(target, operation).is_some())
            }
        })
        .collect()
}

/// Whether the tool of this name changes a file: `elfie_mark`, which writes a marker line
/// into one of a compiler run's outputs, and a command tool, which runs the target's own
/// command and so writes whatever that command always writes.
// @lfy def/mcp/main.lfy:host#host:host:eeef9359197b88bf6fc69027d5771f23a22ac670ed51c8455dd7ddc9ce90c2b6
fn changes_files(name: &str) -> bool {
    name == "elfie_mark" || matches!(carried(name), Carried::Command(_))
}

/// The instructions the initialize answer carries, named for the one session it is for: the
/// tools that session is offered and nothing else, which of them change files, and, when it
/// is bound to a run, which role it is bound to.
///
/// A session over standard input and output is offered no command tool and no compiler
/// tool, so none of its tools writes a file, and the line says so outright.
// @lfy def/mcp/main.lfy:serve#serve:serve:3cd568fc43a27e041f94444e0bd774ad591038e833bc93d99d8667e3342f190f
// @lfy def/mcp/main.lfy:host#host:host:eeef9359197b88bf6fc69027d5771f23a22ac670ed51c8455dd7ddc9ce90c2b6
fn instructions(offered: &[String], binding: Option<&Binding>) -> String {
    let mut text = format!("The Elfie agent server. Its tools in this session are {}.", offered.join(", "));
    let Some(binding) = binding else {
        // @lfy def/mcp/main.lfy:serve#serve:serve:3cd568fc43a27e041f94444e0bd774ad591038e833bc93d99d8667e3342f190f
        text.push_str(" None of them writes a file.");
        return text;
    };
    // @lfy def/mcp/main.lfy:host#host:host:eeef9359197b88bf6fc69027d5771f23a22ac670ed51c8455dd7ddc9ce90c2b6
    let writing: Vec<&str> = offered.iter().map(String::as_str).filter(|name| changes_files(name)).collect();
    let _ = write!(
        text,
        " This session is bound to a {} run.",
        match binding.role {
            Role::Compiler => "compiler",
            Role::Verifier => "verifier",
        }
    );
    // @lfy def/mcp/main.lfy:host#host:host:eeef9359197b88bf6fc69027d5771f23a22ac670ed51c8455dd7ddc9ce90c2b6
    if writing.is_empty() {
        text.push_str(" None of them changes a file.");
        return text;
    }
    let _ = write!(
        text,
        " Of them, {} change files: elfie_mark writes a marker line into one of this run's outputs, \
         and a command tool runs the target's own command, which writes what that command always \
         writes. Every other tool here changes nothing.",
        writing.join(", ")
    );
    text
}

/// One call of a fn carrying `tool`: the arguments are checked against the parameters as
/// the parameters are declared, then the fn is called with the session and them.
///
// Decision: `source` takes a line, which is a number, and the trait's `Arguments` gives a
// fn the text of an argument. A number argument is therefore checked as a number here,
// where the fn that carries the trait lives, and then handed on spelled as text through a
// tool that says text, so that the one call path of `traits` still makes the call, catches
// a failure, and answers with the result.
// @lfy def/mcp/main.lfy:source
fn call_tool(registered: &Registered, session: &Session, arguments: &JsonObject) -> ToolResult {
    if let Err(message) = traits::check_arguments(&registered.tool, arguments) {
        return ToolResult::error(message);
    }
    let mut spelled = arguments.clone();
    let mut tool = registered.tool.clone();
    for argument in &mut tool.arguments {
        if argument.ty != "number" && argument.ty != "boolean" {
            continue;
        }
        if let Some(value) = spelled
            .get(&argument.name)
            .filter(|value| value.is_number() || value.is_boolean())
            .cloned()
        {
            spelled.insert(argument.name.clone(), Value::String(value.to_string()));
        }
        argument.ty = "string".to_string();
    }
    traits::call(
        &Registered {
            tool,
            call: registered.call,
        },
        session,
        &spelled,
    )
}

/// A number argument that counts things — a line, an index, or a length — read back from
/// the text it was spelled as.
// @lfy def/mcp/main.lfy:source
fn counted(text: &str, name: &str) -> Result<usize, String> {
    let value: f64 = text
        .parse()
        .map_err(|_| format!("the argument {name} must be a number"))?;
    if value.fract() != 0.0 || value < 1.0 {
        return Err(format!("the argument {name} must be a whole number, counting from 1"));
    }
    Ok(value as usize)
}

/// One session served, with every tool and the names of those the session is offered.
///
/// The session is behind a lock, so a call holds it until it answers and the next waits:
/// the calls of one session are handled one at a time, in the order received. Each served
/// session has a lock of its own, so a call of one never waits for a call of another — save
/// for two calls of command tools for one directory, which share the queue that runs their
/// commands one after the other.
// @lfy def/mcp/main.lfy:serve#serve:serve:b40b7d13aaacdbb22af8b5a419d2c34ed6ffe19d0e12598b4aa56f9dee24fe22
// @lfy def/mcp/main.lfy:host#host:host:ccd2ef2fd65b6d18cbe3bd391b33964306338675eb2302322f14cf7991f56750
// @lfy def/mcp/main.lfy:host#host:host:b09cfad6442aec8ac4faf8e13385b2973136e4d0ac08ec20419f15750ea59366
struct Served {
    /// The session, behind a lock.
    session: Mutex<Session>,
    /// Every fn of this module that carries `tool`.
    tools: Vec<Registered>,
    /// The names of the tools the session is offered, as [`offered`] gives them.
    offered: Vec<String>,
    /// The instructions this session's initialize answer carries, as [`instructions`]
    /// spells them for the tools it is offered and the role it is bound to.
    // @lfy def/mcp/main.lfy:serve#serve:serve:3cd568fc43a27e041f94444e0bd774ad591038e833bc93d99d8667e3342f190f
    // @lfy def/mcp/main.lfy:host#host:host:eeef9359197b88bf6fc69027d5771f23a22ac670ed51c8455dd7ddc9ce90c2b6
    instructions: String,
    /// Whether every call re-reads the files of the program before the tool runs: a session
    /// over standard input and output does, a hosted one never does.
    reread: bool,
}

impl Served {
    // Decision: the definition loads the workspace on initialize; it is loaded when the
    // server is built, just before the initialize exchange, which the agent cannot tell
    // apart and which keeps the protocol library's handshake untouched.
    // @lfy def/mcp/main.lfy:serve#serve:serve:0d7ad5672274c815b6ad256268ff29d8425eb7ed8114bb7c9ea52bd41bedde9b:2
    fn over_stdio(root: &Path) -> Served {
        let tools = tools();
        let session = Session::load(root);
        let offered = offered(&tools, &session.workspace, None);
        // @lfy def/mcp/main.lfy:serve#serve:serve:3cd568fc43a27e041f94444e0bd774ad591038e833bc93d99d8667e3342f190f
        let instructions = instructions(&offered, None);
        Served {
            session: Mutex::new(session),
            tools,
            offered,
            instructions,
            // @lfy def/mcp/main.lfy:serve#serve:serve:995535b9c7a11554f96d88793004bc092de31525b26cd0ca15ce449e17d8e386
            reread: true,
        }
    }

    /// Whether a tool of this name is offered to the session.
    fn offers(&self, name: &str) -> bool {
        self.offered.iter().any(|offered| offered == name)
    }

    /// The tools the session is offered, as the protocol lists them. The result may be
    /// treated as fresh for no time at all and cached by this client only: what every tool
    /// answers depends on the files and the outputs as they are when the call arrives.
    // @lfy def/mcp/main.lfy:problems#problems:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:find#find:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:entity#entity:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:references#references:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:outline#outline:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:grammar#grammar:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:units#units:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:request#request:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:check#check:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:output#output:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:source#source:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:changes#changes:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:review#review:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:report#report:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:task#task:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:dependency#dependency:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:mark#mark:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:submit#submit:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:search#search:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:build#build:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:test#test:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:lint#lint:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:format#format:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    fn list(&self) -> ListToolsResult {
        let offered: Vec<Registered> = self
            .tools
            .iter()
            .filter(|registered| self.offers(&registered.tool.name))
            .cloned()
            .collect();
        ListToolsResult::with_all_items(traits::list(&offered))
            .with_ttl_ms(0)
            .with_cache_scope(CacheScope::Private)
    }

    /// One call: the session is re-read when it is one that re-reads, then the tool runs. A
    /// call naming a tool the session is not offered is answered with an error saying so,
    /// and the fn is not reached.
    // @lfy def/mcp/main.lfy:serve#serve:serve:b40b7d13aaacdbb22af8b5a419d2c34ed6ffe19d0e12598b4aa56f9dee24fe22
    fn call(&self, request: &CallToolRequestParams) -> Result<CallToolResult, ErrorData> {
        let name: &str = &request.name;
        let Some(registered) = traits::find(&self.tools, name) else {
            return Err(ErrorData::invalid_params(format!("no tool named {name}"), None));
        };
        let result = if self.offers(name) {
            let mut session = self.session.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if self.reread {
                // @lfy def/mcp/main.lfy:serve#serve:serve:995535b9c7a11554f96d88793004bc092de31525b26cd0ca15ce449e17d8e386
                session.refresh();
            }
            let empty = JsonObject::new();
            let arguments = request.arguments.as_ref().unwrap_or(&empty);
            call_tool(registered, &session, arguments)
        } else {
            ToolResult::error(format!("{name} is not offered to this session"))
        };
        let content = vec![ContentBlock::text(result.text)];
        Ok(if result.is_error { CallToolResult::error(content) } else { CallToolResult::success(content) })
    }
}

/// The protocol server: one served session, over standard input and output or over HTTP.
#[derive(Clone)]
struct Server {
    served: Arc<Served>,
}

impl Server {
    /// A server over standard input and output, for the project at `root`.
    // @lfy def/mcp/main.lfy:serve#serve:serve:0d7ad5672274c815b6ad256268ff29d8425eb7ed8114bb7c9ea52bd41bedde9b:2
    fn new(root: &Path) -> Server {
        Server {
            served: Arc::new(Served::over_stdio(root)),
        }
    }
}

impl ServerHandler for Server {
    /// Tools only: no resources and no prompts.
    // @lfy def/mcp/main.lfy:serve#serve:serve:4d105e14d91381963764973d69e9e54abacde103ac17990250a98dadf1ced8cb
    // @lfy def/mcp/main.lfy:host#host:host:4d105e14d91381963764973d69e9e54abacde103ac17990250a98dadf1ced8cb
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::V_2025_06_18) // @lfy def/mcp/main.lfy:Protocol
            .with_server_info(Implementation::new("elfie", env!("CARGO_PKG_VERSION")))
            // @lfy def/mcp/main.lfy:serve#serve:serve:3cd568fc43a27e041f94444e0bd774ad591038e833bc93d99d8667e3342f190f
            // @lfy def/mcp/main.lfy:host#host:host:eeef9359197b88bf6fc69027d5771f23a22ac670ed51c8455dd7ddc9ce90c2b6
            .with_instructions(self.served.instructions.clone())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(self.served.list())
    }

    // @lfy def/mcp/main.lfy:serve#serve:serve:7a38d3ecf5e074e43e215152a74b94923a889a14c60638b479fd67135299e336
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        self.served.call(&request).map(CallToolResponse::from)
    }
}

/// Serve agents over standard input and output until the connection closes.
///
/// The protocol library owns standard output and writes only protocol messages there;
/// nothing here prints to it. Returns 0 when standard input closes.
// @lfy def/mcp/main.lfy:serve#serve:serve:7a38d3ecf5e074e43e215152a74b94923a889a14c60638b479fd67135299e336
// @lfy def/mcp/main.lfy:serve#serve:serve:cdd8cbb19e8838a376ee63434bb80d8dd21a483810121a6bced851760822594c:2
// Nothing here opens a file for writing, and no tool it serves does either: the agent
// writes the outputs itself and asks the server to check them.
// @lfy def/mcp/main.lfy:serve#serve:serve:41ad46670c5145672ee63c9f4ba6f892fd59cb442ca93c795e977b22fd589cef
pub fn serve(root: Option<&Path>) -> i32 {
    // @lfy def/mcp/main.lfy:serve#serve:serve:5d8b7ced1bda14a6a824d8f764b914631ac3a471c462a38e1399cfe3eb88e760
    let root = match root {
        Some(root) => root.to_path_buf(),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        // Every line the server logs goes to standard error, which leaves standard output
        // to the protocol alone.
        // @lfy def/mcp/main.lfy:serve#serve:serve:2e8afa5f7b4b656715c118a89eb0aa4e31bf4d6d7c23f5f07c261d830eba22d2:2
        Err(error) => {
            eprintln!("elfie mcp: {error}");
            return FAILURE;
        }
    };
    let (input, output) = rmcp::transport::stdio();
    runtime.block_on(serve_on(input, output, root))
}

/// [`serve`] over any pair of streams, so a test can drive the whole server in memory.
// @lfy def/mcp/main.lfy:serve#serve:serve:7a38d3ecf5e074e43e215152a74b94923a889a14c60638b479fd67135299e336
// @lfy def/mcp/main.lfy:serve#serve:serve:cdd8cbb19e8838a376ee63434bb80d8dd21a483810121a6bced851760822594c:2
async fn serve_on<I, O>(input: I, output: O, root: PathBuf) -> i32
where
    I: AsyncRead + Send + Unpin + 'static,
    O: AsyncWrite + Send + Unpin + 'static,
{
    let server = Server::new(&root);
    // @lfy def/mcp/main.lfy:serve#serve:serve:7a38d3ecf5e074e43e215152a74b94923a889a14c60638b479fd67135299e336
    let running = match server.serve((input, output)).await {
        Ok(running) => running,
        // Decision: standard input closing before the agent initializes is the
        // connection closing, so it exits with 0 like any other close.
        // @lfy def/mcp/main.lfy:serve#serve:serve:e36ddce87201b751562fbe8a68504f70e09879c00d4b93be51b40d6176adae2e
        Err(rmcp::service::ServerInitializeError::ConnectionClosed(_)) => return 0,
        Err(error) => {
            eprintln!("elfie mcp: {error}");
            return FAILURE;
        }
    };
    // @lfy def/mcp/main.lfy:serve#serve:serve:e36ddce87201b751562fbe8a68504f70e09879c00d4b93be51b40d6176adae2e
    match running.waiting().await {
        Ok(_) => 0,
        Err(error) => {
            eprintln!("elfie mcp: {error}");
            FAILURE
        }
    }
}

// ---- the host -----------------------------------------------------------------------

// Decision: a compile hosts the agent server itself, so every agent it runs reads the plan
// the compile made, not one a server of its own re-made from files that may have changed
// since, and its verdicts reach the CLI through the server rather than through text it
// prints. The server still never writes a file: what `elfie_report` records stays in memory
// until the CLI closes the session.

/// The one address a host listens on: the loopback address, never another interface, so no
/// program on another machine can reach a tool.
// @lfy def/mcp/main.lfy:host#host:host:ea1cfd81c073494e7f7377472fc3070be780e4455e629f59e365d21d658269d4
const LOOPBACK: &str = "127.0.0.1";

/// What the path of a hosted session's URL begins with, under the host's own URL.
// @lfy def/mcp/main.lfy:host#host:host:465078bd14c4c2971c8ae047a59db580f64b30e5fa762af2e656c4accbede7ba
const SESSIONS: &str = "sessions";

/// The operating system's secure random source.
// Decision: `/dev/urandom` is the secure random source of the one platform the target
// lists, posix; it is a device the kernel answers from, not configuration, so reading it is
// not configuration read from a fixed absolute path.
// @lfy def/mcp/main.lfy:host#host:host:246a5aacda8ffe241aedd0b00fec70b3f76a7c6220141f54731dcb8097f58852
const RANDOM: &str = "/dev/urandom";

/// How many bytes a token holds: 32, which is 64 hex digits.
// @lfy def/mcp/main.lfy:host#host:host:246a5aacda8ffe241aedd0b00fec70b3f76a7c6220141f54731dcb8097f58852
const TOKEN_BYTES: usize = 32;

/// A token of its own for one host: 64 lowercase hex digits from the operating system's
/// secure random source, read afresh for each call of [`host`].
// @lfy def/mcp/main.lfy:host#host:host:246a5aacda8ffe241aedd0b00fec70b3f76a7c6220141f54731dcb8097f58852
fn fresh_token() -> Result<String, String> {
    let mut bytes = [0u8; TOKEN_BYTES];
    fs::File::open(RANDOM)
        .and_then(|mut file| std::io::Read::read_exact(&mut file, &mut bytes))
        .map_err(|error| format!("the secure random source {RANDOM} cannot be read: {error}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// What one host holds: where it listens, what it demands of a request, and the sessions
/// still open.
// @lfy def/mcp/main.lfy:host
struct Hosting {
    /// Where it listens, as [`Host::url`] spells it.
    url: String,
    /// What every request must carry as its bearer token.
    token: String,
    /// The sessions still open, by the path of their URL.
    sessions: Mutex<BTreeMap<String, Arc<Served>>>,
    /// How many sessions the host has opened, so that no two are given one number.
    // @lfy def/mcp/main.lfy:host#host:host:465078bd14c4c2971c8ae047a59db580f64b30e5fa762af2e656c4accbede7ba
    opened: AtomicUsize,
    /// Notified when the host is to stop listening.
    stopping: tokio::sync::Notify,
}

impl Hosting {
    /// The session open at a path, when one is.
    // @lfy def/mcp/main.lfy:host#host:host:c0519927a29b45c6d3ed0bf600d8dd929353a41c1678e7be62c950db70763d79
    fn session(&self, path: &str) -> Option<Arc<Served>> {
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(path)
            .cloned()
    }

    /// One hosted session bound to a binding, and its URL: the host's URL, `sessions`, a
    /// slash, and a number no earlier session of the host was given.
    ///
    /// Its workspace is the one the binding's program was lowered from, read at the
    /// binding's directory, so every output, source map, and map file a call reads is read
    /// under that directory; nothing of the program is read from disk again.
    // @lfy def/mcp/main.lfy:host#host:host:465078bd14c4c2971c8ae047a59db580f64b30e5fa762af2e656c4accbede7ba
    // @lfy def/mcp/main.lfy:host#host:host:238a67527a5f81bdc05abb6515eacc7b86b9ead269c56d3b3654d34d47184b50
    // @lfy def/mcp/main.lfy:host#host:host:b01f7a894faf3752cba01a91aaed7228df1bb1b93d87ea2c648f034427bcc3fd
    // @lfy def/mcp/main.lfy:host#host:host:bb7c6b5258b2c4026015a8f9915717da1ec50aa40dcc10a43e5b50bc9e39310e
    // @lfy def/mcp/main.lfy:host#host:host:232c0725ae5219330822ed9664cc87be38c4591303cbc3dead664044e09b75eb
    fn open(&self, binding: Binding) -> String {
        let number = self.opened.fetch_add(1, Ordering::Relaxed) + 1;
        let mut workspace = binding.program.workspace.clone();
        workspace.root = binding.directory.clone();
        let tools = tools();
        let offered = offered(&tools, &workspace, Some(&binding));
        // @lfy def/mcp/main.lfy:host#host:host:eeef9359197b88bf6fc69027d5771f23a22ac670ed51c8455dd7ddc9ce90c2b6
        let instructions = instructions(&offered, Some(&binding));
        let session = Session {
            workspace,
            // A hosted session never re-reads a file of the program, so it stamps none.
            seen: BTreeMap::new(),
            binding: Some(binding),
            // @lfy def/mcp/main.lfy:host#host:host:91b44b23c29eaf7242b190da7fe5ab5f3642df5ec77212e680331af6b76e01b1
            reviews: Mutex::new(Vec::new()),
            // @lfy def/mcp/main.lfy:host#host:host:5e5526603ec0ccf1f819bfc584b556e779b3a2a8f9d0f46fdc9b92ad91bb9dc5
            submission: Mutex::new(None),
        };
        let served = Arc::new(Served {
            session: Mutex::new(session),
            tools,
            offered,
            instructions,
            reread: false,
        });
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(format!("/{SESSIONS}/{number}"), served);
        format!("{}{SESSIONS}/{number}", self.url)
    }

    /// The path of a URL under this host's own, when the URL is one of its own.
    // @lfy def/mcp/main.lfy:host#host:host:1f6860e467f9f7b17993f762e5ba4521688af80851fff707da5bbb18f86cdbba
    fn path_of(&self, url: &str) -> Option<String> {
        url.strip_prefix(&self.url).map(|path| format!("/{path}"))
    }

    /// The names of the tools the session at a URL is offered, in the order the server lists
    /// them; a URL that is no open session's is offered none.
    // @lfy def/mcp/main.lfy:host#host:host:d3b3abd60412e83fb1835597e514740cc7d4648f94a73575cf7f076399063025
    // @lfy def/mcp/main.lfy:host#host:host:1f6860e467f9f7b17993f762e5ba4521688af80851fff707da5bbb18f86cdbba
    fn offered(&self, url: &str) -> Vec<String> {
        let Some(served) = self.path_of(url).and_then(|path| self.session(&path)) else {
            // @lfy def/mcp/main.lfy:host#host:host:1f6860e467f9f7b17993f762e5ba4521688af80851fff707da5bbb18f86cdbba
            return Vec::new();
        };
        // The tools are listed in the order the server lists them, which is the order the
        // definition declares the fns that carry `tool`.
        // @lfy def/mcp/main.lfy:host#host:host:d3b3abd60412e83fb1835597e514740cc7d4648f94a73575cf7f076399063025
        served
            .tools
            .iter()
            .map(|registered| registered.tool.name.clone())
            .filter(|name| served.offers(name))
            .collect()
    }

    /// The session at a URL ended, and what it recorded; a URL that is no open session's
    /// ends nothing and gives a record holding no review and no submission.
    // @lfy def/mcp/main.lfy:host#host:host:64c0df55947a7e7a600bb0796c4715f740afcfda41cdd6f86b4cb408c4705713
    // @lfy def/mcp/main.lfy:host#host:host:8eae0f57be7e371560c08488ce9a3c25a5eb3254d63aa6f4fcd5f317786e93b6
    fn close(&self, url: &str) -> SessionRecord {
        let Some(path) = self.path_of(url) else {
            // @lfy def/mcp/main.lfy:host#host:host:8eae0f57be7e371560c08488ce9a3c25a5eb3254d63aa6f4fcd5f317786e93b6
            return SessionRecord::default();
        };
        let removed = self
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&path);
        let Some(served) = removed else {
            // @lfy def/mcp/main.lfy:host#host:host:8eae0f57be7e371560c08488ce9a3c25a5eb3254d63aa6f4fcd5f317786e93b6
            return SessionRecord::default();
        };
        served
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .record()
    }
}

/// Every request a host answers: the bearer token first, then the session its path names,
/// then the protocol over the streamable HTTP transport of the ecosystem's library.
///
/// The transport is served statelessly, so a request acts on the hosted session its path
/// names whatever MCP session id it carries, and whether it carries one at all.
// @lfy def/mcp/main.lfy:host#host:host:d17c6dc3479336c65543a2c99389c29582860e9358d65bd4e36a3d9c355e7825
// @lfy def/mcp/main.lfy:host#host:host:b20b250482898ccb1326689317f112cb544e9941949596a5c823ce56f4d692b9
// @lfy def/mcp/main.lfy:host#host:host:f876706edb4133d6c7e2e9d4f43bf7b7cc00c92572911e227d0c789a95636e5d
// @lfy def/mcp/main.lfy:host#host:host:c0519927a29b45c6d3ed0bf600d8dd929353a41c1678e7be62c950db70763d79
async fn hosted(
    axum::extract::State(hosting): axum::extract::State<Arc<Hosting>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    // A request without the header `Authorization` reading `Bearer `, then the host's
    // token, is answered with 401 and reaches no session, so no tool runs for it.
    // @lfy def/mcp/main.lfy:host#host:host:b20b250482898ccb1326689317f112cb544e9941949596a5c823ce56f4d692b9
    let carried = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if carried != Some(hosting.token.as_str()) {
        return (axum::http::StatusCode::UNAUTHORIZED, "the bearer token of the host is missing or wrong\n").into_response();
    }
    // @lfy def/mcp/main.lfy:host#host:host:f876706edb4133d6c7e2e9d4f43bf7b7cc00c92572911e227d0c789a95636e5d
    let Some(served) = hosting.session(request.uri().path()) else {
        return (axum::http::StatusCode::NOT_FOUND, "no session is open at this URL\n").into_response();
    };
    // @lfy def/mcp/main.lfy:host#host:host:d17c6dc3479336c65543a2c99389c29582860e9358d65bd4e36a3d9c355e7825
    let service = StreamableHttpService::new(
        move || Ok(Server { served: served.clone() }),
        Arc::new(NeverSessionManager::default()),
        StreamableHttpServerConfig::default()
            // @lfy def/mcp/main.lfy:host#host:host:c0519927a29b45c6d3ed0bf600d8dd929353a41c1678e7be62c950db70763d79
            .with_legacy_session_mode(false)
            .with_json_response(true),
    );
    service.handle(request).await.map(axum::body::Body::new)
}

/// Serve agents over HTTP on this machine for the length of one compile or verify, one
/// hosted session per run of the compiler or the verifier.
///
/// Nothing here opens a file for writing, and the only tool a hosted session serves that
/// does is `elfie_mark`, which writes a marker line into one of the run's own outputs, and a
/// command tool, which writes whatever the target's own command writes. Nothing is written
/// to standard output, since the protocol travels over HTTP and what the host has to say it
/// says by its answer.
// @lfy def/mcp/main.lfy:host#host:host:01fa8e1706370fdc8f52d02ad3bb1a8e87f29d8d913cbe72a9195f08470fa011
// @lfy def/mcp/main.lfy:host#host:host:0587dce8e7ae5b18edf59adfa5cbec0f98b32b34d8182cf85359c17c142f1d8d
pub fn host() -> Result<Host, String> {
    // @lfy def/mcp/main.lfy:host#host:host:246a5aacda8ffe241aedd0b00fec70b3f76a7c6220141f54731dcb8097f58852
    let token = fresh_token()?;
    // The operating system picks the port, and only the loopback address is listened on.
    // @lfy def/mcp/main.lfy:host#host:host:ea1cfd81c073494e7f7377472fc3070be780e4455e629f59e365d21d658269d4
    // @lfy def/mcp/main.lfy:host#host:host:a1747a1f253c33baae12ae425d2c5c491a7c303766db5dce456ac31d206ff9e1
    let listener = std::net::TcpListener::bind((LOOPBACK, 0))
        .map_err(|error| format!("no port of {LOOPBACK} can be listened on: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("the port of {LOOPBACK} that is listened on cannot be read: {error}"))?
        .port();
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("the port {port} of {LOOPBACK} cannot be served: {error}"))?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("the host cannot be run: {error}"))?;
    let hosting = Arc::new(Hosting {
        // @lfy def/mcp/main.lfy:host#host:host:c0b5fd0495fb73ed8d21e6599998b9acc5a051a0dde254bb492f14f26417fade
        url: format!("http://{LOOPBACK}:{port}/"),
        token: token.clone(),
        sessions: Mutex::new(BTreeMap::new()),
        opened: AtomicUsize::new(0),
        stopping: tokio::sync::Notify::new(),
    });
    let router = axum::Router::new().fallback(hosted).with_state(hosting.clone());
    let serving = {
        let hosting = hosting.clone();
        runtime.spawn(async move {
            let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
                return;
            };
            // @lfy def/mcp/main.lfy:host#host:host:d17c6dc3479336c65543a2c99389c29582860e9358d65bd4e36a3d9c355e7825
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async move { hosting.stopping.notified().await })
                .await;
        })
    };
    let runtime = Arc::new(runtime);
    Ok(Host {
        url: hosting.url.clone(),
        token,
        open: Box::new({
            let hosting = hosting.clone();
            move |binding| hosting.open(binding)
        }),
        // @lfy def/mcp/main.lfy:host#host:host:d3b3abd60412e83fb1835597e514740cc7d4648f94a73575cf7f076399063025
        offered: Box::new({
            let hosting = hosting.clone();
            move |url: &str| hosting.offered(url)
        }),
        close: Box::new({
            let hosting = hosting.clone();
            move |url: &str| hosting.close(url)
        }),
        // Every session still open is ended, the server is told to shut down, and the task
        // that listens is waited for: the port is no longer listened on once it returns.
        // @lfy def/mcp/main.lfy:host#host:host:e07851ea0cedfcd8e3d913fa15d72b1c389e0d5f87edfc1f52b7841decccbd1e
        stop: Box::new({
            let serving = Mutex::new(Some(serving));
            move || {
                hosting
                    .sessions
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clear();
                hosting.stopping.notify_one();
                let waiting = serving.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
                if let Some(waiting) = waiting {
                    let _ = runtime.block_on(waiting);
                }
            }
        }),
    })
}

// ---- shared helpers ---------------------------------------------------------------

/// The kind of an entity, as its symbol names it.
fn kind_of(model: &Model, entity: EntityId) -> String {
    model.entities[entity]
        .symbol
        .map_or_else(|| "entity".to_string(), |symbol| model.symbols[symbol].kind.as_str().to_string())
}

/// The identifier of an entity, or `anonymous`.
fn identifier_of(model: &Model, entity: EntityId) -> String {
    model.entities[entity]
        .identifier
        .clone()
        .unwrap_or_else(|| "anonymous".to_string())
}

/// The range of the token that spells an entity's name, or of the first token of its
/// declaration; an empty range when it has neither.
fn identifier_range(model: &Model, entity: EntityId) -> Range {
    let record = &model.entities[entity];
    let Some(node) = record.node else { return Range::default() };
    let tokens = model.tokens(node.file);
    let named = record
        .symbol
        .and_then(|symbol| model.symbols[symbol].name_token)
        .and_then(|index| tokens.get(index));
    let token = named.or_else(|| model.first_token(node));
    match token {
        Some(token) => Range {
            file: token.file.to_string(),
            start: Position::new(token.line, token.column),
            end: Position::new(token.line, token.column + token.raw.chars().count()),
        },
        None => Range::default(),
    }
}

/// The file and line an entity is declared at, as `file:line`.
fn place_of(model: &Model, entity: EntityId) -> String {
    let range = identifier_range(model, entity);
    if range.file.is_empty() {
        "unplaced".to_string()
    } else {
        format!("{}:{}", range.file, range.start.line)
    }
}

/// The text of one line of a file, from its tokens.
fn line_text(model: &Model, file: FileId, line: usize) -> String {
    let text: String = model.tokens(file).iter().map(|token| token.raw.as_str()).collect();
    text.lines().nth(line.saturating_sub(1)).unwrap_or("").trim().to_string()
}

/// The text of a criterion: its situation when it has one, then its behavior and side
/// effects.
fn criterion_text(criterion: &Criterion) -> String {
    let mut text = String::new();
    if let Some(situation) = &criterion.situation {
        text.push_str(&format!("when {}: ", situation.join(" ")));
    }
    if let Some(behavior) = &criterion.behavior {
        text.push_str(&behavior.join("\n  "));
    }
    if let Some(side_effects) = &criterion.side_effects {
        if !text.is_empty() {
            text.push_str("; ");
        }
        text.push_str(&format!("side effects: {}", side_effects.join("\n  ")));
    }
    text
}

/// An error saying a file is not in the program and listing the files that are.
fn not_in_program(workspace: &Workspace, file: &str) -> String {
    let mut text = format!("{file} is not in the program; the files are:");
    for known in &workspace.files {
        text.push_str(&format!("\n  {}", known.path));
    }
    text
}

/// The text when nothing is declared with a name.
fn nothing_named(name: &str) -> String {
    format!("nothing is declared as {name}; elfie_outline lists the declarations of one file")
}

// ---- the tools --------------------------------------------------------------------

/// The problems of the program, or of one file: what the compiler, an editor, or a check
/// would report. One line per diagnostic reading the file, the line, the column, the
/// stage, the severity, and the message; the text says so when there are none.
// @lfy def/mcp/main.lfy:problems#problems:problems:c8c8b536f1269d0fb4bc9adb64fbce292df0715fa51d5d524a18f16685af7bf6
pub fn problems(session: &Session, file: Option<&str>) -> Result<String, String> {
    let workspace = &session.workspace;
    let diagnostics = query::diagnostics_of(workspace, file); // @lfy def/mcp/main.lfy:problems#problems:problems:c8c8b536f1269d0fb4bc9adb64fbce292df0715fa51d5d524a18f16685af7bf6
    if diagnostics.is_empty() {
        // Decision: the criterion answers this situation with text, not a failure, and it
        // names no exception for a file outside the program, so a name that matches
        // nothing is answered the same way; elfie_outline is the tool that lists the files.
        if let Some(file) = file {
            return Ok(format!("no problems in {file}")); // @lfy def/mcp/main.lfy:problems#problems:problems:4e726dce93d9d233aedec39235ecea647b64e1f851fe518f6992aea5add1da3c
        }
        return Ok("no problems".to_string()); // @lfy def/mcp/main.lfy:problems#problems:problems:4e726dce93d9d233aedec39235ecea647b64e1f851fe518f6992aea5add1da3c
    }
    let lines: Vec<String> = diagnostics
        .iter()
        .map(|diagnostic| {
            diagnostic_line(
                &diagnostic.range.file,
                diagnostic.range.start.line,
                diagnostic.range.start.column,
                &diagnostic.stage.to_string(),
                &diagnostic.severity.to_string(),
                &diagnostic.message,
            )
        })
        .collect();
    Ok(lines.join("\n"))
}

/// One diagnostic as `problems` spells it: the file, a colon, the line, a colon, the
/// column, a colon, a space, the stage, a space, the severity, a colon, a space, and the
/// message.
// @lfy def/mcp/main.lfy:problems#problems:problems:c8c8b536f1269d0fb4bc9adb64fbce292df0715fa51d5d524a18f16685af7bf6
fn diagnostic_line(file: &str, line: usize, column: usize, stage: &str, severity: &str, message: &str) -> String {
    format!("{file}:{line}:{column}: {stage} {severity}: {message}")
}

/// Where things with a name are declared: one line per entity reading the kind, the
/// identifier, the file, the line, and the definition when there is one.
// @lfy def/mcp/main.lfy:find#find:find:8d5e1239e348d7ebcaa7e007e6bfc71fed3f9aa4abd903c5cc7f3160b0416367
pub fn find(session: &Session, name: &str) -> Result<String, String> {
    let workspace = &session.workspace;
    let model = &workspace.model;
    let entities = query::find(workspace, name);
    if entities.is_empty() {
        // The text says so, and suggests elfie_outline.
        // @lfy def/mcp/main.lfy:find#find:find:33784dd4ac5c7832aad69dab34430cacc1eb4b82930eb20f500fcfe9c89be26c
        // @lfy def/mcp/main.lfy:find#find:find:1a0733810793cda3b844ef9c1beb121fa974d605748755292d61aa511c80a1e2
        return Ok(nothing_named(name));
    }
    // @lfy def/mcp/main.lfy:find#find:find:8d5e1239e348d7ebcaa7e007e6bfc71fed3f9aa4abd903c5cc7f3160b0416367
    let lines: Vec<String> = entities
        .iter()
        .map(|&entity| {
            let mut line = format!("{} {} {}", kind_of(model, entity), identifier_of(model, entity), place_of(model, entity));
            // The line ends with the definition, after the kind, the identifier, the file,
            // and the line.
            // @lfy def/mcp/main.lfy:find#find:find:eb271c552e5c82df2d2018db45719c602a20801b3e7e3e1f948bf7c9f0108b1d
            if let Some(definition) = &model.entities[entity].definition {
                line.push_str(&format!(": {}", model::strip_references(definition)));
            }
            line
        })
        .collect();
    Ok(lines.join("\n"))
}

/// Everything known about the things with a name: for each entity, the hover fields with a
/// heading each, its traits, members, tests, references, and source.
// @lfy def/mcp/main.lfy:entity#entity:entity:d43b0e556e705056317e1af4e3df190417f20a08435faad56d54fcafa63e524e
pub fn entity(session: &Session, name: &str) -> Result<String, String> {
    let workspace = &session.workspace;
    let model = &workspace.model;
    let entities = query::find(workspace, name);
    if entities.is_empty() {
        return Ok(nothing_named(name));
    }
    let mut sections = Vec::new();
    for &id in &entities {
        let record = &model.entities[id];
        let hover = query::hover_of(workspace, id);
        let mut out = String::new();
        // Each entity is headed by its file, so the caller can narrow the name with a
        // file path.
        // @lfy def/mcp/main.lfy:entity#entity:entity:0c69bca0860a743d038fb5c4f6c609c4ded6631ac9cf95f37cccba908854cbcd
        out.push_str(&format!("# {} {} ({})\n", hover.kind, hover.identifier, place_of(model, id)));
        // The fields of the hover, a heading each.
        // @lfy def/mcp/main.lfy:entity#entity:entity:d43b0e556e705056317e1af4e3df190417f20a08435faad56d54fcafa63e524e
        if let Some(definition) = &hover.definition {
            out.push_str(&format!("\n## Definition\n{definition}\n"));
        }
        if let Some(ty) = &hover.ty {
            out.push_str(&format!("\n## Type\n{ty}\n"));
        }
        if let Some(owner) = &hover.owner {
            out.push_str(&format!("\n## Owner\n{owner}\n"));
        }
        if let Some(documentation) = &hover.documentation {
            out.push_str(&format!("\n## Documentation\n{documentation}\n"));
        }
        if !hover.criteria.is_empty() {
            out.push_str("\n## Criteria\n");
            for criterion in &hover.criteria {
                let from = if criterion.contributor == id {
                    String::new()
                } else {
                    format!(" (from {})", identifier_of(model, criterion.contributor))
                };
                out.push_str(&format!("- {}{from}\n", criterion_text(criterion)));
            }
        }
        // The identifier of each trait, as the hover spells them: every one of
        // `Entity.traits` in application order, with `builtin` among them for a native
        // thing.
        // @lfy def/mcp/main.lfy:entity#entity:entity:d43b0e556e705056317e1af4e3df190417f20a08435faad56d54fcafa63e524e
        if !hover.traits.is_empty() {
            out.push_str(&format!("\n## Traits\n{}\n", hover.traits.join(", ")));
        }
        let members = model.members(id);
        if !members.is_empty() {
            out.push_str("\n## Members\n");
            for symbol in members {
                let symbol = &model.symbols[symbol];
                let member = &model.entities[symbol.entity];
                let mut line = format!("- {}", symbol.name);
                if let Some(definition) = &member.definition {
                    line.push_str(&format!(": {}", model::strip_references(definition)));
                }
                if let Some(ty) = &member.ty {
                    line.push_str(&format!(" ({})", model::type_text(model, ty)));
                }
                out.push_str(&line);
                out.push('\n');
            }
        }
        if !record.tests.is_empty() {
            out.push_str("\n## Tests\n");
            for test in &record.tests {
                out.push_str(&format!("- input: {}\n  expect: {}\n", test.input_text.trim(), test.expect_text.trim()));
            }
        }
        if let Some(symbol) = record.symbol {
            let usages = model::usages_of(model, symbol);
            out.push_str(&format!("\n## References ({})\n", usages.len()));
            for usage in usages {
                let (file, line, _) = usage_place(model, usage);
                out.push_str(&format!("- {file}:{line}\n"));
            }
        }
        out.push_str(&format!("\n## Source\n```elfie\n{}\n```", query::source_of(workspace, id).trim_end()));
        sections.push(out);
    }
    // More than one entity: each is separated by a rule.
    // @lfy def/mcp/main.lfy:entity#entity:entity:0c69bca0860a743d038fb5c4f6c609c4ded6631ac9cf95f37cccba908854cbcd
    Ok(sections.join("\n\n---\n\n"))
}

/// The file, line, and column of a usage: of the token that spells the name, or of the
/// first token of the using node.
fn usage_place(model: &Model, usage: usize) -> (String, usize, usize) {
    let usage = &model.usages[usage];
    let tokens = model.tokens(usage.node.file);
    let token = usage
        .token
        .and_then(|index| tokens.get(index))
        .or_else(|| model.first_token(usage.node));
    match token {
        Some(token) => (token.file.to_string(), token.line, token.column),
        None => (model.sources[usage.node.file].path.clone(), 0, 0),
    }
}

/// Every place things with a name are used: for each entity, one line per usage of its
/// symbol reading the file, the line, the column, and the text of that line.
// @lfy def/mcp/main.lfy:references#references:references:172a06e97d4c30625ada08fa53113f0fdd6c084862a2db5f64451f15c9853456
pub fn references(session: &Session, name: &str) -> Result<String, String> {
    let workspace = &session.workspace;
    let model = &workspace.model;
    let entities = query::find(workspace, name);
    if entities.is_empty() {
        // Decision: the definition says nothing about a name that matches no entity;
        // the answer of elfie_find is given so the caller learns the same thing.
        return Ok(nothing_named(name));
    }
    let mut lines = Vec::new();
    for &id in &entities {
        let Some(symbol) = model.entities[id].symbol else { continue };
        for usage in model::usages_of(model, symbol) {
            let (file, line, column) = usage_place(model, usage); // @lfy def/mcp/main.lfy:references#references:references:172a06e97d4c30625ada08fa53113f0fdd6c084862a2db5f64451f15c9853456
            let text = line_text(model, model.usages[usage].node.file, line);
            lines.push(format!("{file}:{line}:{column}: {text}"));
        }
    }
    if lines.is_empty() {
        // Decision: no usages is a plain answer, not an error.
        return Ok(format!("{name} is never used"));
    }
    Ok(lines.join("\n"))
}

/// One outline and its children as an indented list: two spaces per level, then the
/// kind, the name, and the line.
// @lfy def/mcp/main.lfy:outline#outline:outline:f4a001d0d002913edf84c20318c9fdac89e274849e6c553e36e790f938bb6027
fn render_outline(items: &[Outline], depth: usize, out: &mut Vec<String>) {
    for item in items {
        // Decision: the line is the identifier's, so a documented declaration is listed
        // where its name is, not where its documentation begins.
        out.push(format!("{}{} {} {}", "  ".repeat(depth), item.kind, item.name, item.selection_range.start.line));
        render_outline(&item.children, depth + 1, out);
    }
}

/// The declarations of one file, nested as written.
// @lfy def/mcp/main.lfy:outline#outline:outline:f4a001d0d002913edf84c20318c9fdac89e274849e6c553e36e790f938bb6027
pub fn outline(session: &Session, file: &str) -> Result<String, String> {
    let workspace = &session.workspace;
    if workspace.file(file).is_none() {
        return Err(not_in_program(workspace, file)); // @lfy def/mcp/main.lfy:outline#outline:outline:a024fe62db1e31cd5f20fef57d6d0cc38073b0724bb0ecf3e828e91bf7d495d3
    }
    let items = query::outline_of(workspace, file);
    if items.is_empty() {
        return Ok(format!("no declarations in {file}"));
    }
    let mut lines = Vec::new();
    render_outline(&items, 0, &mut lines); // @lfy def/mcp/main.lfy:outline#outline:outline:f4a001d0d002913edf84c20318c9fdac89e274849e6c553e36e790f938bb6027
    Ok(lines.join("\n"))
}

/// The grammar of Elfie: every terminal, a blank line, then every rule as EBNF.
// @lfy def/mcp/main.lfy:grammar#grammar:grammar:51bcd7292055364b6387726881c5443879b09b36eae8e1534b8d84a84effc72f
pub fn grammar(_session: &Session) -> Result<String, String> {
    Ok(format!(
        "{}\n\n{}",
        elfie_core::grammar::terminal_document(),
        elfie_core::grammar::grammar_document()
    )) // @lfy def/mcp/main.lfy:grammar#grammar:grammar:51bcd7292055364b6387726881c5443879b09b36eae8e1534b8d84a84effc72f
}

/// Every file under a directory, skipping build and version control directories.
fn walk_all(directory: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target" || name == ".git" || name == "node_modules") {
                continue;
            }
            walk_all(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// The files under the target's output directory that carry a marker for the unit's
/// file, read from disk.
// @lfy def/mcp/main.lfy:check#check:check:77f6c4a5084647dc21eb7114983b08f9ebbf27471071fee839c6113ba37cdfab
fn outputs_of(workspace: &Workspace, plan: &Plan, unit: usize) -> Vec<Output> {
    let unit = &plan.units[unit];
    let target = &workspace.targets[unit.target];
    let file = &workspace.files[unit.file].path;
    let mut paths = Vec::new();
    walk_all(&workspace.root.join(&target.output_directory), &mut paths);
    paths.sort();
    let mut found = Vec::new();
    for path in paths {
        let Ok(text) = fs::read_to_string(&path) else { continue };
        if generation::parse_markers(&text).iter().any(|marker| &marker.file == file) {
            found.push(Output {
                path: relative(&workspace.root, &path),
                text,
            });
        }
    }
    found
}

/// The identifiers of every target, joined.
fn known_targets(workspace: &Workspace) -> String {
    let names: Vec<&str> = workspace.targets.iter().map(|target| target.identifier.as_str()).collect();
    if names.is_empty() { "none".to_string() } else { names.join(", ") }
}

/// An error naming the known targets when the target given is not one.
// @lfy def/mcp/main.lfy:units#units:units:e2e8756b78db77377231d8ef5ee9fd3e078703c801906a9b774ee59a2135a3ab
fn check_target(workspace: &Workspace, target: Option<&str>) -> Result<(), String> {
    match target {
        Some(target) if !workspace.targets.iter().any(|known| known.identifier == target) => {
            Err(format!("{target} is not a target; the targets are: {}", known_targets(workspace)))
        }
        _ => Ok(()),
    }
}

/// A plan, the program it was planned from, and the source maps it was planned against:
/// what every tool that reads the compiler's work is given.
// Decision: `generation::plan` takes the workspace and gives back the program lowered from
// it beside the plan, and every later call reads that program rather than the workspace, so
// the three travel together.
// @lfy def/mcp/main.lfy:units#units:units:e35cb874630f014bcf475b9b10f74413ed36fe8b669e6a8194fd1ca57d8cf251
struct Planned {
    /// The workspace lowered, which every request and review reads.
    program: Program,
    /// The plan.
    plan: Plan,
    /// The source maps the plan was made against: what the last acceptance recorded.
    maps: Vec<SourceMap>,
}

impl Planned {
    /// The workspace the plan's units index into: the one the program was lowered from.
    fn workspace(&self) -> &Workspace {
        &self.program.workspace
    }
}

/// Every output file of one target, by path relative to the workspace root, in path order:
/// a file under the target's output directory whose extension is among the target's.
///
/// The root of a hosted session's workspace is its binding's directory, so these are the
/// files under that directory and the paths are relative to it.
// Decision: what a build leaves behind — a `target` directory, a `node_modules`, the
// repository's own `.git` — is walked past, as every other walk of an output directory walks
// past it: a file there was written by the target's own tools and came from no unit, so it is
// no output and nothing is ever generated for it.
// @lfy def/mcp/main.lfy:output#output:output:e3ada846896a5d797c74467acb25cc6ab9c9f3667f13eae813f8169d01373c8f
// @lfy def/mcp/main.lfy:mark#mark:mark:20e94fd8b37ced24d2351848435ef39376d3fc15eeb5d4e64ccffd5f95cb4053
// @lfy def/mcp/main.lfy:search#search:search:c17cae68f2b3424b9e51c005c2ca4bc5521e610abeb02c841daa9b067c1fa147
fn output_files(workspace: &Workspace, target: &Target) -> Vec<String> {
    let mut paths = Vec::new();
    walk_all(&workspace.root.join(&target.output_directory), &mut paths);
    paths.sort();
    paths
        .into_iter()
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| target.extensions.iter().any(|known| known == extension))
        })
        .map(|path| relative(&workspace.root, &path))
        .collect()
}

/// The source maps of the units of a compiler session's batch, read from the markers their
/// target's output files hold now rather than from what the last acceptance recorded: one
/// map per unit and output file that holds a marker naming the unit's source, whose markers
/// are the markers of that file naming that source, read as acceptance reads them.
///
/// A compiler has just written and edited those files, so it must see its regions where they
/// are now; the ends are derived over every marker of the file, as a source map derives
/// them, and only then are the markers of other sources left out, so a region still ends
/// where the next marker begins whoever wrote it.
///
/// Only the markers are read from the file. Everything else a map carries — what the unit's
/// text and requirements hashed to, which ids were answered, when it was accepted — is the
/// recorded map's, taken from `recorded`, the maps of those units that the live ones stand
/// in for, so the unit plans, changes, and requests as the compile planned it.
// @lfy def/mcp/main.lfy:host#host:host:e7b0595c23bc940969f71da55611064a2e5d53569ad18ad94b4b021935dc0a65
// @lfy def/mcp/main.lfy:host#host:host:3e562832ea4cdd3ed23d67fe79f9a9024f6c42e802d43f5e68dde2914e23c677
fn own_source_maps(session: &Session, target: Option<&str>, recorded: &[SourceMap]) -> Vec<SourceMap> {
    let Some(binding) = &session.binding else { return Vec::new() };
    let Some(batch) = &binding.batch else { return Vec::new() };
    let workspace = &session.workspace;
    let mut found: Vec<SourceMap> = Vec::new();
    for &index in &batch.units {
        let Some(unit) = binding.plan.units.get(index) else { continue };
        let Some(of_target) = workspace.targets.get(unit.target) else { continue };
        if target.is_some_and(|target| of_target.identifier != target) {
            continue;
        }
        let Some(file) = workspace.files.get(unit.file) else { continue };
        // @lfy def/mcp/main.lfy:host#host:host:e7b0595c23bc940969f71da55611064a2e5d53569ad18ad94b4b021935dc0a65
        for output in output_files(workspace, of_target) {
            let Ok(text) = fs::read_to_string(workspace.root.join(&output)) else { continue };
            let markers: Vec<Marker> = generation::parse_markers(&text)
                .into_iter()
                .filter(|marker| marker.file == file.path)
                .collect();
            if markers.is_empty() {
                continue;
            }
            // The recorded map of the same target, output, and source gives every member but
            // the markers; where none was recorded, there is nothing to carry over.
            // @lfy def/mcp/main.lfy:host#host:host:3e562832ea4cdd3ed23d67fe79f9a9024f6c42e802d43f5e68dde2914e23c677
            let mut map = recorded
                .iter()
                .find(|one| {
                    one.target == of_target.identifier && one.output == output && one.source == file.path
                })
                .cloned()
                // @lfy def/mcp/main.lfy:host#host:host:e7b0595c23bc940969f71da55611064a2e5d53569ad18ad94b4b021935dc0a65
                .unwrap_or_else(|| SourceMap {
                    target: of_target.identifier.clone(),
                    output: output.clone(),
                    source: file.path.clone(),
                    hash: String::new(),
                    requirements: String::new(),
                    answered: Vec::new(),
                    signature: String::new(),
                    text: String::new(),
                    dependencies: BTreeMap::new(),
                    generated: String::new(),
                    markers: Vec::new(),
                });
            // @lfy def/mcp/main.lfy:host#host:host:e7b0595c23bc940969f71da55611064a2e5d53569ad18ad94b4b021935dc0a65
            map.markers = markers;
            found.push(map);
        }
    }
    found
}

/// [`generation::source_maps_of`] the session's workspace, with the maps the session's
/// review was made with standing in for the recorded maps of the same target and source, and
/// with the maps of a compiler session's own units read from its outputs as they are now.
///
/// A verifier judges the outputs its review was made against, so where the binding carries
/// a map for a target and a source, every map read for that pair is left out and the
/// binding's maps are read in their place; a map the binding carries that replaces none is
/// read after the rest. A compiler reads its own units' regions from the files it has just
/// written, so every map recorded for one of its batch's sources is left out and
/// [`own_source_maps`] is read in their place. With no binding, what the last acceptance
/// recorded under the session's root is all that is read.
// @lfy def/mcp/main.lfy:output#output:output:18bbd05acd9d7ef32f170d4713899aaf2e4c19f93da25c7feae2938e50414fc1
// @lfy def/mcp/main.lfy:host#host:host:6f8405c8e6f3e4591069a7d71ce8930df3cd097e586d761bdaa8bb9a2df6ce2e
// @lfy def/mcp/main.lfy:host#host:host:1d7d3545dd3f2db19683b885c877989d8409d11cfdadaa4e2c381a4b9ef084ba
// @lfy def/mcp/main.lfy:host#host:host:232c0725ae5219330822ed9664cc87be38c4591303cbc3dead664044e09b75eb
fn source_maps_of(session: &Session, target: Option<&str>) -> Vec<SourceMap> {
    let mut read = generation::source_maps_of(&session.workspace, target);
    // A compiler session's own units are read from the markers their outputs hold now.
    // @lfy def/mcp/main.lfy:host#host:host:1d7d3545dd3f2db19683b885c877989d8409d11cfdadaa4e2c381a4b9ef084ba
    if let Some(binding) = &session.binding
        && binding.role == Role::Compiler
        && let Some(batch) = &binding.batch
    {
        let own: Vec<String> = batch
            .units
            .iter()
            .filter_map(|&index| binding.plan.units.get(index))
            .filter_map(|unit| session.workspace.files.get(unit.file))
            .map(|file| file.path.clone())
            .collect();
        // @lfy def/mcp/main.lfy:host#host:host:1d7d3545dd3f2db19683b885c877989d8409d11cfdadaa4e2c381a4b9ef084ba
        let left_out: Vec<SourceMap> = read.iter().filter(|map| own.contains(&map.source)).cloned().collect();
        read.retain(|map| !own.contains(&map.source));
        // @lfy def/mcp/main.lfy:host#host:host:3e562832ea4cdd3ed23d67fe79f9a9024f6c42e802d43f5e68dde2914e23c677
        read.extend(own_source_maps(session, target, &left_out));
    }
    let given: &[SourceMap] = match &session.binding {
        Some(binding) => &binding.source_maps,
        None => &[],
    };
    if given.is_empty() {
        return read;
    }
    let of_target = |map: &SourceMap| target.is_none_or(|target| map.target == target);
    let stands_for = |map: &SourceMap| given.iter().any(|one| one.target == map.target && one.source == map.source);
    let mut found: Vec<SourceMap> = Vec::new();
    let mut taken: Vec<(String, String)> = Vec::new();
    // @lfy def/mcp/main.lfy:host#host:host:6f8405c8e6f3e4591069a7d71ce8930df3cd097e586d761bdaa8bb9a2df6ce2e
    for map in read {
        if !stands_for(&map) {
            found.push(map);
            continue;
        }
        let pair = (map.target.clone(), map.source.clone());
        if taken.contains(&pair) {
            continue;
        }
        found.extend(
            given
                .iter()
                .filter(|one| one.target == pair.0 && one.source == pair.1 && of_target(one))
                .cloned(),
        );
        taken.push(pair);
    }
    // A map the binding carries that stood in for none is read after the rest.
    // @lfy def/mcp/main.lfy:host#host:host:6f8405c8e6f3e4591069a7d71ce8930df3cd097e586d761bdaa8bb9a2df6ce2e
    found.extend(
        given
            .iter()
            .filter(|one| of_target(one) && !taken.contains(&(one.target.clone(), one.source.clone())))
            .cloned(),
    );
    found
}

/// The plan of the session's workspace with [`source_maps_of`] it, and as its violated
/// units the violated units of the last [`GlobalReview`].
// Decision: a unit's outputs are read off the plan, and the violated units are an input of
// planning, so the plan is made once with no violated unit and again only when the last
// global review names one; a project with none, which is every project until a global
// review finds one, is planned once.
// @lfy def/mcp/main.lfy:units#units:units:e35cb874630f014bcf475b9b10f74413ed36fe8b669e6a8194fd1ca57d8cf251
fn plan_of(session: &Session) -> Planned {
    let workspace = &session.workspace;
    let maps = source_maps_of(session, None); // @lfy def/mcp/main.lfy:units#units:units:e35cb874630f014bcf475b9b10f74413ed36fe8b669e6a8194fd1ca57d8cf251
    let (program, plan) = generation::plan(workspace.clone(), &maps, &[], &[]);
    // @lfy def/mcp/main.lfy:units#units:units:e35cb874630f014bcf475b9b10f74413ed36fe8b669e6a8194fd1ca57d8cf251
    let stems = GlobalReview::read(&workspace.root).map(|last| last.violated_stems(&plan)).unwrap_or_default();
    if stems.is_empty() {
        return Planned { program, plan, maps };
    }
    // @lfy def/mcp/main.lfy:units#units:units:e35cb874630f014bcf475b9b10f74413ed36fe8b669e6a8194fd1ca57d8cf251
    let (program, plan) = generation::plan(workspace.clone(), &maps, &[], &stems);
    Planned { program, plan, maps }
}

/// Nothing when a session may be asked about a batch, and an error naming the one batch it
/// acts on — or the global review, when it is bound to no batch — when it may not.
// @lfy def/mcp/main.lfy:request#request:request:f7ef0e526d107bc01f393317930dc6691e83904a9ec731aef3e304884867cfe0
// @lfy def/mcp/main.lfy:review#review:review:0dcb77dad8ef4db5102c1bd7eeaec630f239dc244db5eee8132f8c57d77677ea
fn own_batch(session: &Session, identifier: &str) -> Result<(), String> {
    let Some(binding) = &session.binding else {
        return Ok(());
    };
    match &binding.batch {
        Some(batch) if batch.identifier == identifier => Ok(()),
        Some(batch) => Err(format!(
            "this session acts only on its own batch, {}; it was asked about {identifier}",
            batch.identifier
        )),
        None => Err(format!(
            "this session acts only on the global review; it was asked about the batch {identifier}"
        )),
    }
}

/// Nothing when a session may be asked about a unit with a stem, and an error naming the one
/// batch it acts on — or the global review — when it may not.
// @lfy def/mcp/main.lfy:check#check:check:9c073d6b09e272ea0d5a712167c00378477b35e103dad16445f0d53b88d7bc30
// @lfy def/mcp/main.lfy:changes#changes:changes:e97791dbc253b4037be4b5a6a54aa430027a02f99710070c689449608a3affe1
fn own_unit(session: &Session, stem: &str) -> Result<(), String> {
    let Some(binding) = &session.binding else {
        return Ok(());
    };
    match &binding.batch {
        Some(batch)
            if batch
                .units
                .iter()
                .filter_map(|&unit| binding.plan.units.get(unit))
                .any(|unit| unit.stem == stem) =>
        {
            Ok(())
        }
        Some(batch) => Err(format!(
            "this session acts only on its own batch, {}; it was asked about the unit {stem}",
            batch.identifier
        )),
        None => Err(format!(
            "this session acts only on the global review; it was asked about the unit {stem}"
        )),
    }
}

/// One problem of a plan as a line: as `problems` spells a diagnostic of stage generation
/// and severity error at its node.
// @lfy def/mcp/main.lfy:units#units:units:e3b6f3a30f32b79ed993570429ffa615792d56cc2481b2b41aa6f859567ec450
fn problem_line(model: &Model, problem: &Problem) -> String {
    let (file, line, column) = match model.first_token(problem.node) {
        Some(token) => (token.file.to_string(), token.line, token.column),
        None => (model.sources[problem.node.file].path.clone(), 0, 0),
    };
    diagnostic_line(&file, line, column, &problem.stage.to_string(), "error", &problem.message)
}

/// One move of a plan as a line: the word move, the target, the stem it came from, the word
/// into, the stem that compiles it, the identifiers moved in brackets, the word for, and
/// what caused it.
// @lfy def/mcp/main.lfy:units#units:units:04942eee0b664f2b18f16b21c1e245d6ec558e513af796471b999cea68d090c6
fn move_line(workspace: &Workspace, plan: &Plan, moved: &Move) -> String {
    let model = &workspace.model;
    let origin = &plan.units[moved.origin];
    let entities: Vec<String> = moved.entities.iter().map(|&entity| identifier_of(model, entity)).collect();
    format!(
        "move {} {} into {} [{}] for {}",
        workspace.targets[origin.target].identifier,
        origin.stem,
        plan.units[moved.into].stem,
        entities.join(", "),
        moved.cause
    )
}

/// One unit as a line: the target, the stem, the reason or up to date, and the stems of
/// its dependencies.
// @lfy def/mcp/main.lfy:units#units:units:c7fcb0ca869f9f4555904c7d48858c0314cebdf85dc76b32442da4fcb97e9021
fn unit_line(workspace: &Workspace, plan: &Plan, index: usize) -> String {
    let unit = &plan.units[index];
    let reason = unit.reason.map_or_else(|| "up to date".to_string(), |reason| reason.as_str().to_string());
    let dependencies: Vec<&str> = unit.dependencies.iter().map(|&dependency| plan.units[dependency].stem.as_str()).collect();
    format!("{} {} {reason} [{}]", workspace.targets[unit.target].identifier, unit.stem, dependencies.join(", "))
}

/// The identifier of the target a batch's units are of; a batch holds units of one target.
fn batch_target<'w>(workspace: &'w Workspace, plan: &Plan, batch: &Batch) -> &'w str {
    match batch.units.first() {
        Some(&unit) => workspace.targets[plan.units[unit].target].identifier.as_str(),
        None => "",
    }
}

/// One batch as a line: its identifier and the stems it holds.
// @lfy def/mcp/main.lfy:units#units:units:92120a7e31d7970c4faf3dd986b56767f95eeeb9e0470fd818c092f986d3e0d3
fn batch_line(plan: &Plan, batch: &Batch) -> String {
    let stems: Vec<&str> = batch.units.iter().map(|&unit| plan.units[unit].stem.as_str()).collect();
    format!("batch {} [{}]", batch.identifier, stems.join(", "))
}

/// The target of the unit a problem's node is in, when the node is in a unit's file; a
/// problem outside every unit's file is of no one target.
// Decision: a plan problem names a node, not a target, so the target narrows the problems
// by the file the node is in, which is the file of the unit the problem is about.
// @lfy def/mcp/main.lfy:units#units:units:05a79e1b79428d09f369fcfe7a098f9648d1dbfb976d11d8d457dd8945737272
fn problem_targets<'w>(workspace: &'w Workspace, plan: &Plan, problem: &Problem) -> Vec<&'w str> {
    let path = &workspace.model.sources[problem.node.file].path;
    plan.units
        .iter()
        .filter(|unit| &workspace.files[unit.file].path == path)
        .map(|unit| workspace.targets[unit.target].identifier.as_str())
        .collect()
}

/// The units the compiler would produce, in order, why each needs generating, and how
/// they are batched: the problems, then the moves, then the units, then the batches.
// @lfy def/mcp/main.lfy:units#units:units:3dfafdff61f6c1061f2fe64e135a6885c4554344d56b669fcd2f9dceddc9bb98
pub fn units(session: &Session, target: Option<&str>) -> Result<String, String> {
    check_target(&session.workspace, target)?;
    let planned = plan_of(session);
    let (workspace, plan) = (planned.workspace(), &planned.plan);
    let model = &workspace.model;
    // The problems come first, each as one line. A problem of a file no unit of the named
    // target holds is left out with that target's units.
    // @lfy def/mcp/main.lfy:units#units:units:e3b6f3a30f32b79ed993570429ffa615792d56cc2481b2b41aa6f859567ec450
    let mut lines: Vec<String> = plan
        .problems
        .iter()
        .filter(|problem| target.is_none_or(|target| problem_targets(workspace, plan, problem).contains(&target)))
        .map(|problem| problem_line(model, problem))
        .collect();
    // Then the moves, each as one line.
    // @lfy def/mcp/main.lfy:units#units:units:04942eee0b664f2b18f16b21c1e245d6ec558e513af796471b999cea68d090c6
    lines.extend(
        plan.moves
            .iter()
            .filter(|moved| {
                target.is_none_or(|target| workspace.targets[plan.units[moved.origin].target].identifier == target)
            })
            .map(|moved| move_line(workspace, plan, moved)),
    );
    // Then the units, each as one line.
    // @lfy def/mcp/main.lfy:units#units:units:05a79e1b79428d09f369fcfe7a098f9648d1dbfb976d11d8d457dd8945737272
    let units: Vec<String> = (0..plan.units.len())
        .filter(|&index| target.is_none_or(|target| workspace.targets[plan.units[index].target].identifier == target))
        .map(|index| unit_line(workspace, plan, index))
        .collect();
    if units.is_empty() {
        return Ok(match target {
            Some(target) => format!("no units for the target {target}"),
            None => "no units".to_string(),
        });
    }
    lines.extend(units);
    // Then one line per batch. Decision: a batch holds units of one target, so the
    // target narrows the batches the same way it narrows the units; a blank line and the
    // word batch keep the two lists apart, since a batch line has the shape of a unit's.
    // @lfy def/mcp/main.lfy:units#units:units:92120a7e31d7970c4faf3dd986b56767f95eeeb9e0470fd818c092f986d3e0d3
    let batches: Vec<String> = plan
        .batches
        .iter()
        .filter(|batch| target.is_none_or(|target| batch_target(workspace, plan, batch) == target))
        .map(|batch| batch_line(plan, batch))
        .collect();
    if !batches.is_empty() {
        lines.push(String::new());
        lines.extend(batches);
    }
    Ok(lines.join("\n"))
}

/// The unit with a stem, in the target given or the only target it is in; an error
/// saying which when no unit has that stem or the target is ambiguous.
// @lfy def/mcp/main.lfy:check
fn resolve_unit(workspace: &Workspace, plan: &Plan, stem: &str, target: Option<&str>) -> Result<usize, String> {
    check_target(workspace, target)?;
    let matches: Vec<usize> = (0..plan.units.len())
        .filter(|&index| {
            let unit = &plan.units[index];
            unit.stem == stem && target.is_none_or(|target| workspace.targets[unit.target].identifier == target)
        })
        .collect();
    match matches.as_slice() {
        [] => Err(match target {
            Some(target) => format!("no unit of the target {target} has the stem {stem}; elfie_units lists them"),
            None => format!("no unit has the stem {stem}; elfie_units lists them"),
        }),
        [index] => Ok(*index),
        many => {
            let targets: Vec<&str> = many.iter().map(|&index| workspace.targets[plan.units[index].target].identifier.as_str()).collect();
            Err(format!("the unit {stem} is in more than one target ({}); name one with target", targets.join(", ")))
        }
    }
}

/// The batch with an identifier, or the one holding a unit with a stem, in the target
/// given or the only target it is in; an error saying which when no batch has that
/// identifier or holds a unit with that stem, or the target is ambiguous.
///
/// A name that is the identifier of one batch and the stem of a unit of another means the
/// batch whose identifier it is: the identifiers are tried first and the stems only when
/// no batch is named by its identifier.
// @lfy def/mcp/main.lfy:request#request:request:a7e16dee761e4f416f1040c2c3f90c5e87e1352241e235f0eea4146033eccc99
fn resolve_batch(workspace: &Workspace, plan: &Plan, name: &str, target: Option<&str>) -> Result<usize, String> {
    check_target(workspace, target)?;
    let of_target = |batch: &Batch| target.is_none_or(|target| batch_target(workspace, plan, batch) == target);
    // @lfy def/mcp/main.lfy:request#request:request:a7e16dee761e4f416f1040c2c3f90c5e87e1352241e235f0eea4146033eccc99
    let mut matches: Vec<usize> = (0..plan.batches.len())
        .filter(|&index| plan.batches[index].identifier == name && of_target(&plan.batches[index]))
        .collect();
    if matches.is_empty() {
        matches = (0..plan.batches.len())
            .filter(|&index| {
                let batch = &plan.batches[index];
                batch.units.iter().any(|&unit| plan.units[unit].stem == name) && of_target(batch)
            })
            .collect();
    }
    match matches.as_slice() {
        // @lfy def/mcp/main.lfy:request#request:request:ca0a25d6a947aa41d3cbf9399a371f681224987d6109d85e16a0b7f8a85db14c
        [] => Err(match target {
            Some(target) => format!(
                "no batch of the target {target} has the identifier {name} or holds a unit with the stem {name}; elfie_units lists them"
            ),
            None => format!(
                "no batch has the identifier {name} or holds a unit with the stem {name}; elfie_units lists them"
            ),
        }),
        [index] => Ok(*index),
        // @lfy def/mcp/main.lfy:request#request:request:ca0a25d6a947aa41d3cbf9399a371f681224987d6109d85e16a0b7f8a85db14c
        many => {
            let targets: Vec<&str> = many
                .iter()
                .map(|&index| batch_target(workspace, plan, &plan.batches[index]))
                .collect();
            Err(format!("the batch {name} is in more than one target ({}); name one with target", targets.join(", ")))
        }
    }
}

/// The request for a batch, with the existing outputs of its units read from disk and no
/// previous source.
// @lfy def/mcp/main.lfy:request#request:request:92537a4ab2aac8027576852406e446fcb12c7b5b87c96095a386c6d7dac101ed
fn request_for(program: &Program, plan: &Plan, batch: &Batch) -> Request {
    let existing: Vec<Output> = batch
        .units
        .iter()
        .flat_map(|&unit| plan.units[unit].outputs.iter())
        .filter_map(|map| {
            fs::read_to_string(program.workspace.root.join(&map.output))
                .ok()
                .map(|text| Output { path: map.output.clone(), text })
        })
        .collect();
    // The request is made here rather than handed to the session, so it is no hosted
    // compiler's request and quotes the interfaces of its dependencies whole.
    // @lfy def/mcp/main.lfy:request#request:request:92537a4ab2aac8027576852406e446fcb12c7b5b87c96095a386c6d7dac101ed
    generation::request(program, plan, batch, &existing, &BTreeMap::new(), false)
}

/// The request of the batch that holds a unit.
// Decision: a unit that is up to date is in no batch, because only planned units are
// batched; it is requested as a batch of its own, which is the request its batch would
// be, so a check of an up-to-date unit answers rather than failing.
// @lfy def/mcp/main.lfy:check#check:check:77f6c4a5084647dc21eb7114983b08f9ebbf27471071fee839c6113ba37cdfab
fn request_of_unit(program: &Program, plan: &Plan, unit: usize) -> Request {
    match plan.batches.iter().find(|batch| batch.units.contains(&unit)) {
        Some(batch) => request_for(program, plan, batch),
        None => request_for(
            program,
            plan,
            &Batch {
                units: vec![unit],
                identifier: plan.units[unit].stem.clone(),
            },
        ),
    }
}

/// The full request for one batch: what to produce, where, every criterion to satisfy,
/// and how to report.
// @lfy def/mcp/main.lfy:request#request:request:92537a4ab2aac8027576852406e446fcb12c7b5b87c96095a386c6d7dac101ed
pub fn request(session: &Session, batch: &str, target: Option<&str>) -> Result<String, String> {
    let planned = plan_of(session);
    let plan = &planned.plan;
    // The plan has problems, so there is no request: an entity that reaches one not built
    // for its target, or a cycle no move resolved, leaves every unit of the plan doubtful.
    // This is read before the batch is resolved, because a plan with problems may hold no
    // batch for the name at all and the criterion names no exception.
    // @lfy def/mcp/main.lfy:request#request:request:2bea1608ac7ace6999af5d0abfb20a8166b6e5149871e97cdc2660b9709891e0
    if !plan.problems.is_empty() {
        let model = &planned.workspace().model;
        let mut text = "the plan has problems, so there is no request:".to_string();
        for problem in &plan.problems {
            text.push_str(&format!("\n  {}", problem_line(model, problem)));
        }
        return Err(text);
    }
    let index = resolve_batch(planned.workspace(), plan, batch, target)?; // @lfy def/mcp/main.lfy:request#request:request:ca0a25d6a947aa41d3cbf9399a371f681224987d6109d85e16a0b7f8a85db14c
    // A hosted session answers for one run of one agent, so it is given the request of its
    // own batch and of no other.
    // @lfy def/mcp/main.lfy:request#request:request:f7ef0e526d107bc01f393317930dc6691e83904a9ec731aef3e304884867cfe0
    own_batch(session, &plan.batches[index].identifier)?;
    // A compiler is given the request it was run with, so what it is asked to produce is what
    // the compile asked it, never a request made again from the definitions as they are now;
    // every other session has the request made for the batch.
    // @lfy def/mcp/main.lfy:request#request:request:af5eea7b402d3ed59ce0541ca71250ff92ea2e142a768f4b98af9e9bc4f348c9
    if let Some(binding) = &session.binding
        && binding.role == Role::Compiler
        && let Some(given) = &binding.request
    {
        return Ok(given.instructions.clone());
    }
    Ok(request_for(&planned.program, plan, &plan.batches[index]).instructions) // @lfy def/mcp/main.lfy:request#request:request:92537a4ab2aac8027576852406e446fcb12c7b5b87c96095a386c6d7dac101ed
}

/// Whether the outputs on disk satisfy the request for one unit, and what is wrong when
/// they do not. Nothing is written; the CLI records source maps.
// @lfy def/mcp/main.lfy:check#check:check:77f6c4a5084647dc21eb7114983b08f9ebbf27471071fee839c6113ba37cdfab
pub fn check(session: &Session, unit: &str, target: Option<&str>) -> Result<String, String> {
    let planned = plan_of(session);
    let (workspace, plan) = (planned.workspace(), &planned.plan);
    let index = resolve_unit(workspace, plan, unit, target)?;
    // A hosted session checks a unit of its own batch and nothing else, so nothing is
    // checked when it is asked about another.
    // @lfy def/mcp/main.lfy:check#check:check:9c073d6b09e272ea0d5a712167c00378477b35e103dad16445f0d53b88d7bc30
    own_unit(session, &plan.units[index].stem)?;
    let request = request_of_unit(&planned.program, plan, index);
    let outputs = outputs_of(workspace, plan, index); // @lfy def/mcp/main.lfy:check#check:check:77f6c4a5084647dc21eb7114983b08f9ebbf27471071fee839c6113ba37cdfab
    // What the last acceptance recorded for the target's other units, so that an output
    // another unit is to write is seen as that unit's rather than as this one's.
    // @lfy def/mcp/main.lfy:check#check:check:77f6c4a5084647dc21eb7114983b08f9ebbf27471071fee839c6113ba37cdfab
    let source = workspace.files[plan.units[index].file].path.clone();
    let identifier = workspace.targets[plan.units[index].target].identifier.clone();
    let recorded: Vec<SourceMap> = source_maps_of(session, Some(&identifier))
        .into_iter()
        .filter(|map| map.source != source)
        .collect();
    let verdict = generation::accept(&planned.program, plan, &request, index, &outputs, &recorded);
    if verdict.accepted {
        // @lfy def/mcp/main.lfy:check#check:check:7d045cb553e0d306e634e78ed9a299647790d4beae5075ece0962d22ff2d4ef8
        let mut text = format!(
            "accepted {} for {}: {} output{}",
            plan.units[index].stem,
            workspace.targets[plan.units[index].target].identifier,
            outputs.len(),
            if outputs.len() == 1 { "" } else { "s" }
        );
        for output in &outputs {
            text.push_str(&format!("\n  {}", output.path));
        }
        text.push_str("\nnothing was written; the CLI records the source maps");
        Ok(text)
    } else {
        // @lfy def/mcp/main.lfy:check#check:check:2a16ea176547468dbe87eb74de7c195dc58e16c26122fc69de5f2ff15dd469f1
        let mut text = format!("rejected {}:", plan.units[index].stem);
        for problem in &verdict.problems {
            text.push_str(&format!("\n  {problem}"));
        }
        Err(text)
    }
}

// ---- what the last acceptance recorded ---------------------------------------------

// Decision: the source maps these tools read are `generation::source_maps_of` the
// workspace, as elfie_units reads them, so what a tool answers is what the last acceptance
// recorded in elfie-compile/maps, never a guess from the text on disk.

/// The language a fenced block of an output is marked with: the output's extension, or
/// nothing when it has none.
// @lfy def/mcp/main.lfy:output#output:output:b5c9012118ce16c51f508f0dc7a730f69985ec6d3e5351b99d447dc69d898590
fn fence_of(path: &str) -> &str {
    Path::new(path).extension().and_then(|extension| extension.to_str()).unwrap_or("")
}

/// The lines of an output from `first` through `last`, counting from 1, as they are on
/// disk; the empty text when the file cannot be read.
// @lfy def/mcp/main.lfy:output#output:output:b5c9012118ce16c51f508f0dc7a730f69985ec6d3e5351b99d447dc69d898590
fn excerpt(workspace: &Workspace, output: &str, first: usize, last: usize) -> String {
    let Ok(text) = fs::read_to_string(workspace.root.join(output)) else {
        return String::new();
    };
    let lines: Vec<&str> = text.lines().collect();
    let from = first.saturating_sub(1).min(lines.len());
    let to = last.min(lines.len()).max(from);
    lines[from..to].join("\n")
}

/// One region of an output: its path, a colon, the line it begins on, a dash, the line it
/// ends on, a space, and what it came from, then, when `text` is true, its text in a fenced
/// block; with `text` false the line stands alone, so however long an output is its regions
/// read as a few lines.
// @lfy def/mcp/main.lfy:output#output:output:22ddcfeba7456aa91005e6aeab2fcd871e9c512ede2849961438a5b7a3cc5d7c
fn region_block(workspace: &Workspace, output: &str, marker: &Marker, text: bool) -> String {
    let mut heading = format!("{}:{}-{}", output, marker.output_line, marker.end);
    if let Some(entity) = &marker.entity {
        heading.push_str(&format!(" {entity}"));
    }
    // The marker names a requirement, so the line ends with its id, after the entity.
    // @lfy def/mcp/main.lfy:output#output:output:f06f38040e8ba03b92a99e321da9419cc77651260ecc24f8c0087ce2ba8f1dcd
    if let Some(requirement) = &marker.requirement {
        heading.push_str(&format!(" {requirement}"));
    }
    if !text {
        // @lfy def/mcp/main.lfy:output#output:output:a388e41750cae72a1a126c53b636547055414dad9404ae10cd679aed3861fe6a
        return heading;
    }
    // @lfy def/mcp/main.lfy:output#output:output:b5c9012118ce16c51f508f0dc7a730f69985ec6d3e5351b99d447dc69d898590
    format!(
        "{heading}\n```{}\n{}\n```",
        fence_of(output),
        excerpt(workspace, output, marker.output_line, marker.end)
    )
}

/// Every region of every source map, each with the output that holds it, in source map then
/// output order, limited to one target when one is given.
// @lfy def/mcp/main.lfy:output#output:output:18bbd05acd9d7ef32f170d4713899aaf2e4c19f93da25c7feae2938e50414fc1
fn every_region<'m>(maps: &'m [SourceMap], target: Option<&str>) -> Vec<(&'m str, &'m Marker)> {
    maps.iter()
        .filter(|map| !target.is_some_and(|target| map.target != target))
        .flat_map(|map| map.markers.iter().map(|marker| (map.output.as_str(), marker)))
        .collect()
}

/// Whether a path, relative to the workspace root, is an output file of a target of the
/// session: under the target's output directory, with an extension among the target's, and
/// there on disk. Limited to one target when one is named.
// @lfy def/mcp/main.lfy:output#output:output:e3ada846896a5d797c74467acb25cc6ab9c9f3667f13eae813f8169d01373c8f
fn is_output_file(workspace: &Workspace, path: &str, target: Option<&str>) -> bool {
    workspace
        .targets
        .iter()
        .filter(|of_target| target.is_none_or(|target| of_target.identifier == target))
        .any(|of_target| output_files(workspace, of_target).iter().any(|known| known == path))
}

/// Every marker one output file holds as it is read now, as acceptance reads them: the ends
/// derived over every marker of the file, then the markers naming a file that is not in the
/// program left out, since nothing was generated from a file the program does not hold.
// @lfy def/mcp/main.lfy:output#output:output:5f91a9d6a55f8212f9288be65dd547c69391009665dc988e76b8a5150ac20406
// @lfy def/mcp/main.lfy:output#output:output:84421a06b6eda0b985d11eb52870e3f12014a1594070085b9dc237f580115624
fn markers_now(workspace: &Workspace, path: &str) -> Vec<Marker> {
    let Ok(text) = fs::read_to_string(workspace.root.join(path)) else {
        return Vec::new();
    };
    generation::parse_markers(&text)
        .into_iter()
        // @lfy def/mcp/main.lfy:output#output:output:84421a06b6eda0b985d11eb52870e3f12014a1594070085b9dc237f580115624
        .filter(|marker| workspace.file(&marker.file).is_some())
        .collect()
}

/// Where generated code is, with its text or without it: every region that came from one
/// entity, that answers for one criterion or test, or that one output file holds.
// @lfy def/mcp/main.lfy:output#output:output:18bbd05acd9d7ef32f170d4713899aaf2e4c19f93da25c7feae2938e50414fc1
pub fn output(session: &Session, name: &str, target: Option<&str>, text: bool) -> Result<String, String> {
    let workspace = &session.workspace;
    check_target(workspace, target)?; // @lfy def/mcp/main.lfy:output#output:output:e2e8756b78db77377231d8ef5ee9fd3e078703c801906a9b774ee59a2135a3ab
    // The name is the path of an output file, so its regions are the markers it holds now,
    // read as acceptance reads them: a compiler that has just edited the file sees its
    // regions where they are rather than where the last acceptance recorded them.
    // @lfy def/mcp/main.lfy:output#output:output:5f91a9d6a55f8212f9288be65dd547c69391009665dc988e76b8a5150ac20406
    if is_output_file(workspace, name, target) {
        let held = markers_now(workspace, name);
        if held.is_empty() {
            // @lfy def/mcp/main.lfy:output#output:output:7e2589822812f683a65808151f3ae1177182b076f571c65ab59fa2d7e78162a8
            return Ok(format!("nothing is generated for {name}"));
        }
        // @lfy def/mcp/main.lfy:output#output:output:22ddcfeba7456aa91005e6aeab2fcd871e9c512ede2849961438a5b7a3cc5d7c
        let blocks: Vec<String> = held
            .iter()
            .map(|marker| region_block(workspace, name, marker, text))
            .collect();
        return Ok(blocks.join("\n\n"));
    }
    let maps = source_maps_of(session, None); // @lfy def/mcp/main.lfy:output#output:output:18bbd05acd9d7ef32f170d4713899aaf2e4c19f93da25c7feae2938e50414fc1
    let every = every_region(&maps, target);
    // The name is one an entity owns, so the regions are the ones `regions_of` gives for it;
    // a marker is matched by the same target and the same marker it was found by, because
    // `regions_of` keeps no path.
    // @lfy def/mcp/main.lfy:output#output:output:d930026963bc74a22a71de25fff3d4d8ac8a7b00ff6209a9e7d4afbbff550eb1
    let owned = generation::regions_of(workspace, &maps, name, target);
    let mut regions: Vec<(&str, &Marker)> =
        every.iter().copied().filter(|(_, marker)| owned.contains(marker)).collect();
    // The name is the id of a criterion or test of the lowered program, local or global, so
    // its text comes first and the regions are the ones answering for it.
    // @lfy def/mcp/main.lfy:output#output:output:8ac14ea384bf2a5e4cb895bab8105f34dfe6124df0ef5ae67edcf3fba77a408d
    let mut heading = None;
    if regions.is_empty() {
        let mut lowered = None;
        let program = match &session.binding {
            Some(binding) => &binding.program,
            None => lowered.insert(interpret::lower(workspace.clone())),
        };
        if let Some(spelled) = requirement_text(program, name) {
            heading = Some(spelled);
            regions = every
                .iter()
                .copied()
                .filter(|(_, marker)| marker.requirement.as_deref() == Some(name))
                .collect();
        }
    }
    let mut blocks: Vec<String> = heading.into_iter().collect();
    if regions.is_empty() {
        // @lfy def/mcp/main.lfy:output#output:output:7e2589822812f683a65808151f3ae1177182b076f571c65ab59fa2d7e78162a8
        blocks.push(format!("nothing is generated for {name}"));
    }
    // @lfy def/mcp/main.lfy:output#output:output:22ddcfeba7456aa91005e6aeab2fcd871e9c512ede2849961438a5b7a3cc5d7c
    blocks.extend(
        regions
            .into_iter()
            .map(|(output, marker)| region_block(workspace, output, marker, text)),
    );
    Ok(blocks.join("\n\n"))
}

/// One lowered criterion as one line: `When` and its situations, then its behaviors, then
/// `Side effects:` and its side effects, joined by `: `, as a request and a review spell it.
// @lfy def/mcp/main.lfy:source#source:source:bab27e2f0ed21f719d22e545c1c874a283b18a0ff5a8979b2a8bf9d324519b11
fn lowered_criterion_text(criterion: &LoweredCriterion) -> String {
    let mut parts = Vec::new();
    if let Some(situation) = &criterion.situation {
        parts.push(format!("When {}", situation.join(" ")));
    }
    if let Some(behavior) = &criterion.behavior {
        parts.push(behavior.join(" "));
    }
    if let Some(side_effects) = &criterion.side_effects {
        parts.push(format!("Side effects: {}", side_effects.join(" ")));
    }
    parts.join(": ")
}

/// One lowered test as one line: its input and its expectation, as spelled.
// @lfy def/mcp/main.lfy:source#source:source:bab27e2f0ed21f719d22e545c1c874a283b18a0ff5a8979b2a8bf9d324519b11
fn lowered_test_text(test: &LoweredTest) -> String {
    format!("Input `{}` gives `{}`", test.input_text.trim(), test.expect_text.trim())
}

/// Every criterion and test of a lowered node and the nodes under it, with those of the
/// criteria and tests given, as its id and the one line it is spelled as.
// @lfy def/mcp/main.lfy:source#source:source:bab27e2f0ed21f719d22e545c1c874a283b18a0ff5a8979b2a8bf9d324519b11
fn collect_requirements(
    criteria: &[LoweredCriterion],
    tests: &[LoweredTest],
    out: &mut Vec<(String, String)>,
) {
    out.extend(criteria.iter().map(|criterion| (criterion.id.clone(), lowered_criterion_text(criterion))));
    out.extend(tests.iter().map(|test| (test.id.clone(), lowered_test_text(test))));
}

/// Every criterion and test of the lowered program, each as its id and the one line it is
/// spelled as: the local ones of every file, then the global ones.
// @lfy def/mcp/main.lfy:source#source:source:bab27e2f0ed21f719d22e545c1c874a283b18a0ff5a8979b2a8bf9d324519b11
fn requirements_of(program: &Program) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    for lowered in &program.files {
        let mut nodes: Vec<&LoweredNode> = vec![&lowered.root];
        while let Some(node) = nodes.pop() {
            collect_requirements(&node.criteria, &node.tests, &mut found);
            nodes.extend(node.nodes());
        }
        collect_requirements(&lowered.criteria, &lowered.tests, &mut found);
    }
    // A global criterion or test is in the program once, under no file.
    // @lfy def/mcp/main.lfy:source#source:source:bab27e2f0ed21f719d22e545c1c874a283b18a0ff5a8979b2a8bf9d324519b11
    collect_requirements(&program.criteria, &program.tests, &mut found);
    found
}

/// The text of the criterion or test one requirement id names, local or global, from the
/// lowered program; `None` when the program holds none with that id.
// @lfy def/mcp/main.lfy:source#source:source:bab27e2f0ed21f719d22e545c1c874a283b18a0ff5a8979b2a8bf9d324519b11
fn requirement_text(program: &Program, id: &str) -> Option<String> {
    requirements_of(program)
        .into_iter()
        .find(|(spelled, _)| spelled == id)
        .map(|(_, text)| text)
}

/// Where one line of generated code came from: the definition file, the entity, and its
/// line.
// @lfy def/mcp/main.lfy:source#source:source:7095bd8049d91a01f9b2a650bf5e71d17bb706c9f4e6a470e004c2dd9881494e
pub fn source(session: &Session, file: &str, line: usize) -> Result<String, String> {
    let workspace = &session.workspace;
    for map in source_maps_of(session, None) {
        if map.output != file {
            continue;
        }
        // The marker read is the one whose region begins at or before the line and ends at
        // or after it.
        // @lfy def/mcp/main.lfy:source#source:source:52459b54250cb2edeb5d2b3079e663f17b6814b8d50602682f49ab2f55bf2285
        let Some(marker) = map.markers.iter().find(|marker| marker.covers(line)) else {
            continue;
        };
        // @lfy def/mcp/main.lfy:source#source:source:7095bd8049d91a01f9b2a650bf5e71d17bb706c9f4e6a470e004c2dd9881494e
        let mut text = format!("{}:{}", marker.file, marker.line);
        // @lfy def/mcp/main.lfy:source#source:source:b4c0ce9250c99f427b67858ad3e1b065b76f259ad3bf94c84085ee1551d51ab1
        if let Some(entity) = &marker.entity {
            text.push_str(&format!(" {entity}"));
        }
        // The requirement follows on the line after, and the text of that criterion or test
        // on the line after it; a criterion reads as a sentence, so it stands on its own
        // line rather than beside an id that is a name and a hash.
        // @lfy def/mcp/main.lfy:source#source:source:bab27e2f0ed21f719d22e545c1c874a283b18a0ff5a8979b2a8bf9d324519b11
        if let Some(requirement) = &marker.requirement {
            text.push('\n');
            text.push_str(requirement);
            let program = interpret::lower(workspace.clone());
            // A stale map may name an id the program no longer holds; the id still says
            // which requirement the region answered for.
            if let Some(spelled) = requirement_text(&program, requirement) {
                text.push('\n');
                text.push_str(&spelled);
            }
        }
        return Ok(text);
    }
    // @lfy def/mcp/main.lfy:source#source:source:3bd573d9905fc8bbb2ff9b436ef7c2affcef119a2a9cf64efa242292f4bf0299
    Ok(format!("no source map covers {file}:{line}"))
}

/// The unit with a stem, in the target given or, when none is given, of the first target in
/// `Workspace::targets` order that has a unit with that stem.
// @lfy def/mcp/main.lfy:changes#changes:changes:21683ec505359b2effe3c7d8bc5d8fbcc63e581f01406ec3948d214f74916b65
fn unit_of_stem(workspace: &Workspace, plan: &Plan, stem: &str, target: Option<&str>) -> Result<usize, String> {
    check_target(workspace, target)?; // @lfy def/mcp/main.lfy:changes#changes:changes:e2e8756b78db77377231d8ef5ee9fd3e078703c801906a9b774ee59a2135a3ab
    for index in 0..workspace.targets.len() {
        if target.is_some_and(|target| workspace.targets[index].identifier != target) {
            continue;
        }
        if let Some(found) = (0..plan.units.len())
            .find(|&unit| plan.units[unit].stem == stem && plan.units[unit].target == index)
        {
            return Ok(found);
        }
    }
    // @lfy def/mcp/main.lfy:changes#changes:changes:bbfd13869e5d8aa4ccdda753d25b4797e5140bfb3b5f287cf8f83310a3b51e7b
    Err(match target {
        Some(target) => format!("no unit of the target {target} has the stem {stem}; elfie_units lists them"),
        None => format!("no unit has the stem {stem}; elfie_units lists them"),
    })
}

/// One file read as something other than what is on disk, bound through the loader and
/// lowered: [`interpret::LoweredFile::text`] of it, which is what a source map's hash is
/// the hash of.
// @lfy def/mcp/main.lfy:changes#changes:changes:6fdf72883c3fb05ab2a58a8325708fe690fc56fcfe75d909abff5f56efdb18e1
fn lowered_text(workspace: &Workspace, path: &str, text: &str) -> Option<String> {
    let before = interpret::lower(workspace::change(workspace, path, Some(text)));
    let index = before.workspace.files.iter().position(|file| file.path == path)?;
    Some(before.files[index].text.clone())
}

/// The source at the last accepted generation, recovered as the command line recovers it:
/// through git when the root is a repository and the unit's source is in it, taking the
/// revision of the file whose hash is the one the source map recorded.
///
/// What is given back is that revision as git holds it, never lowered, because that is what
/// `generation::changes` takes; the revision is *found* by the hash of its lowered text,
/// since the map holds the hash of the lowered text rather than of the file's bytes, so a
/// revision differing only in a comment is the same revision.
// @lfy def/mcp/main.lfy:changes#changes:changes:6fdf72883c3fb05ab2a58a8325708fe690fc56fcfe75d909abff5f56efdb18e1
fn previous_source(workspace: &Workspace, unit: &Unit) -> Option<String> {
    let map = unit.outputs.first()?;
    let file = &workspace.files[unit.file].path;
    let log = Command::new("git")
        .args(["log", "--format=%H", "-n", "50", "--", file])
        .current_dir(&workspace.root)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !log.status.success() {
        return None;
    }
    for revision in String::from_utf8_lossy(&log.stdout).lines() {
        let show = Command::new("git")
            .args(["show", &format!("{revision}:{file}")])
            .current_dir(&workspace.root)
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !show.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&show.stdout).into_owned();
        // The revision is the one whose lowered text hashes to what the map recorded; what
        // is given back is the revision itself, as git holds it.
        // @lfy def/mcp/main.lfy:changes#changes:changes:6fdf72883c3fb05ab2a58a8325708fe690fc56fcfe75d909abff5f56efdb18e1
        let Some(lowered) = lowered_text(workspace, file, &text) else { continue };
        if generation::source_hash(&lowered) == map.hash {
            return Some(text);
        }
    }
    None
}

/// What differs for each entity of a unit since its outputs were last accepted.
// @lfy def/mcp/main.lfy:changes#changes:changes:21683ec505359b2effe3c7d8bc5d8fbcc63e581f01406ec3948d214f74916b65
pub fn changes(session: &Session, unit: &str, target: Option<&str>) -> Result<String, String> {
    let planned = plan_of(session);
    let workspace = planned.workspace();
    let index = unit_of_stem(workspace, &planned.plan, unit, target)?;
    // A hosted session answers for a unit of its own batch and no other.
    // @lfy def/mcp/main.lfy:changes#changes:changes:e97791dbc253b4037be4b5a6a54aa430027a02f99710070c689449608a3affe1
    own_unit(session, &planned.plan.units[index].stem)?;
    let of_stem = &planned.plan.units[index];
    if of_stem.outputs.is_empty() {
        return Ok(format!("no previous output for {unit}; nothing has been accepted for it yet")); // @lfy def/mcp/main.lfy:changes#changes:changes:413343511b79d06a29fa38852b4bf26d600fdb0e411965a13d3190ca9375546e
    }
    let previous = previous_source(workspace, of_stem); // @lfy def/mcp/main.lfy:changes#changes:changes:6fdf72883c3fb05ab2a58a8325708fe690fc56fcfe75d909abff5f56efdb18e1
    let mut text = String::new();
    if previous.is_none() {
        // @lfy def/mcp/main.lfy:changes#changes:changes:fc5534487756c5701f11c4c00be57babd85dc1275ba585942da27893626d07f4
        text.push_str("the source the outputs were generated from cannot be recovered; every entity is listed as added\n");
    }
    let found = generation::changes(&planned.program, of_stem, previous.as_deref()); // @lfy def/mcp/main.lfy:changes#changes:changes:21683ec505359b2effe3c7d8bc5d8fbcc63e581f01406ec3948d214f74916b65
    if found.is_empty() {
        return Ok(format!("no changes in {unit} since its outputs were accepted")); // @lfy def/mcp/main.lfy:changes#changes:changes:35a395b2402b76e8ae9d6c636a82478d28714ed6e6459fe96e70847ff73470a4
    }
    // @lfy def/mcp/main.lfy:changes#changes:changes:21683ec505359b2effe3c7d8bc5d8fbcc63e581f01406ec3948d214f74916b65
    let lines: Vec<String> = found
        .iter()
        .map(|change| format!("{} {} {}", change.entity, change.kind.as_str(), change.detail))
        .collect();
    text.push_str(&lines.join("\n"));
    Ok(text)
}

/// The full review request for one batch: every criterion and test with where the regions
/// generated for its entities are, and how to report.
///
/// The problems of the plan are not mentioned: a review reads the outputs that exist
/// against what was asked of them, which a plan that could not be made whole does not
/// change.
// @lfy def/mcp/main.lfy:review#review:review:5c61bb2a8fa4f5d02a3219d0c4a1f8d385dbad4a9caf2a52fa73d59c10014617
pub fn review(session: &Session, batch: &str, target: Option<&str>) -> Result<String, String> {
    let planned = plan_of(session);
    let plan = &planned.plan;
    // The batch whose identifier the name is is meant before any batch holding a unit with
    // it as a stem; an error says which when neither names one, or the target is ambiguous.
    // @lfy def/mcp/main.lfy:review#review:review:a7e16dee761e4f416f1040c2c3f90c5e87e1352241e235f0eea4146033eccc99
    // @lfy def/mcp/main.lfy:review#review:review:ca0a25d6a947aa41d3cbf9399a371f681224987d6109d85e16a0b7f8a85db14c
    let index = resolve_batch(planned.workspace(), plan, batch, target)?;
    // @lfy def/mcp/main.lfy:review#review:review:0dcb77dad8ef4db5102c1bd7eeaec630f239dc244db5eee8132f8c57d77677ea
    own_batch(session, &plan.batches[index].identifier)?;
    // A verifier is given the review request it was run with, so what it is asked about is
    // what the compile asked it, never a request made again from the outputs as they are
    // now; every other session has the request made for the batch.
    // @lfy def/mcp/main.lfy:review#review:review:da108e476bfaee29f25ace2bffd41fea2a2d7605e41824c4b25757bf8287f9b1
    if let Some(binding) = &session.binding
        && binding.role == Role::Verifier
        && let Some(given) = &binding.review
    {
        return Ok(given.instructions.clone());
    }
    // @lfy def/mcp/main.lfy:review#review:review:f58d35f9aea99f8ea42d7e215a8e84d42b2b414afa745217d99622b1a66e5c0d
    Ok(
        generation::review(
            &planned.program,
            plan,
            &plan.batches[index],
            &planned.maps,
            None,
            &[],
        )
        .instructions,
    )
}

/// The review request for every global criterion and test, once for the whole program, with
/// where the regions that answer for each are.
// @lfy def/mcp/main.lfy:globalReview#globalReview:globalReview:b93638e7046f17fc2cf321187664d92150028acae44164992beee7e46349fb1c
pub fn global_review(session: &Session) -> Result<String, String> {
    let planned = plan_of(session);
    // @lfy def/mcp/main.lfy:globalReview#globalReview:globalReview:f656c023948494da8d316780c15c2c1b96e1512caafb62d843dc6295be0d89be
    if planned.program.criteria.is_empty() && planned.program.tests.is_empty() {
        return Ok("no global criterion or test exists in the program".to_string());
    }
    // @lfy def/mcp/main.lfy:globalReview#globalReview:globalReview:b93638e7046f17fc2cf321187664d92150028acae44164992beee7e46349fb1c
    Ok(generation::global_review(&planned.program, &planned.maps).instructions)
}

// ---- the verdicts of a review -------------------------------------------------------

// Decision: a verdict is submitted as soon as it is judged and checked as it arrives, so a
// verdict with a wrong id is answered at once, while the verifier still knows what it
// meant, and none is lost to a final message that ends too soon or runs too long.

/// How many of the ids still missing a verdict are listed before the rest are only counted.
// @lfy def/mcp/main.lfy:report#report:report:33d92b578b9d5f560fea04be01b80b698d6cef193437113b87d6767239dba56f
const LISTED_MISSING: usize = 5;

/// The ids the review of a session asked about; empty when the session is bound to no
/// review.
// @lfy def/mcp/main.lfy:report#report:report:eecbccd9ec901434b61790fd663bb5081ca97c913c2328df04808e7f3755d06a
fn asked_of(session: &Session) -> &[String] {
    session
        .binding
        .as_ref()
        .and_then(|binding| binding.review.as_ref())
        .map_or(&[], |review| review.asked.as_slice())
}

/// Submit the verdict on one criterion or test of the review this session is for.
///
/// Nothing is written: the verdict is kept in the session until the CLI closes it.
// @lfy def/mcp/main.lfy:report#report:report:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
pub fn report(
    session: &Session,
    id: Option<&str>,
    file: Option<&str>,
    line: Option<usize>,
    status: &str,
    evidence: &str,
    note: &str,
) -> Result<String, String> {
    // The status is read first, so a status naming none of the three records nothing and is
    // answered with the three it could have named.
    // @lfy def/mcp/main.lfy:report#report:report:f95e4040e1b2d8658bf53cbe5eaf320a0628d57aafdf090791dea862afeb67fd
    if ReviewStatus::from_name(&status.to_ascii_lowercase()).is_none() {
        return Err(format!(
            "the status {status} is none of {}, {}, and {}",
            ReviewStatus::Satisfied,
            ReviewStatus::Violated,
            ReviewStatus::Unverifiable
        ));
    }
    // The criterion or test reported on is the one the review report reads a review line as
    // being for, so one line holding what was given is read exactly as a line of a final
    // message would be.
    // @lfy def/mcp/main.lfy:report#report:report:15a298dff4d1c9c629d5217e0802615370e072e57d444f4abd189e391365165b
    let submitted = serde_json::json!({
        "id": id.unwrap_or_default(),
        "file": file.unwrap_or_default(),
        "line": line.unwrap_or_default(),
        "status": status,
        "evidence": evidence,
        "note": note,
    })
    .to_string();
    let mut lowered = None;
    // @lfy def/mcp/main.lfy:report#report:report:15a298dff4d1c9c629d5217e0802615370e072e57d444f4abd189e391365165b
    let program = match &session.binding {
        Some(binding) => &binding.program,
        None => lowered.insert(interpret::lower(session.workspace.clone())),
    };
    let read = generation::review_of(&submitted, program, &[], &[]);
    // @lfy def/mcp/main.lfy:report#report:report:3d3b34634644a0a77e7ee4995575b04f369d10d6495cab7ed34e712d6c0df7d7
    let Some(review) = read.reviews.into_iter().next() else {
        let named = match (id.filter(|id| !id.is_empty()), file, line) {
            (Some(id), _, _) => format!("the id {id}"),
            (None, Some(file), Some(line)) => format!("the place {file}:{line}"),
            (None, Some(file), None) => format!("the file {file}"),
            (None, None, Some(line)) => format!("the line {line}"),
            (None, None, None) => "neither an id nor a place".to_string(),
        };
        return Err(format!("no criterion or test of this review matches {named}"));
    };
    // A verdict on a criterion this review did not ask about is no verdict of this review.
    // @lfy def/mcp/main.lfy:report#report:report:eecbccd9ec901434b61790fd663bb5081ca97c913c2328df04808e7f3755d06a
    let asked = asked_of(session);
    if !asked.iter().any(|one| one == &review.id) {
        return Err(format!("{} was not asked about in this review", review.id));
    }
    let mut recorded = session.reviews.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    // Of two verdicts on one criterion the later one stands, so the earlier is removed.
    // @lfy def/mcp/main.lfy:report#report:report:bfa37d307a244c3332ac692e347ce5f2a534f0f2a1f839c0cab30318da4f37cc
    recorded.retain(|kept| kept.id != review.id);
    // @lfy def/mcp/main.lfy:report#report:report:82b739ccf95aa1e1faa11800427ce14cadd3fdeef5e2c7904cc9999d70958a3a
    let place = format!("{}:{}", review.file, review.line);
    let named = review.id.clone();
    recorded.push(review);
    // @lfy def/mcp/main.lfy:report#report:report:cb0ab5dd13a5f32de066f7ec167f81ff4853716054b8ebe9eae3c72af49a0c4b
    let mut text = format!("recorded {named} at {place}");
    // @lfy def/mcp/main.lfy:report#report:report:33d92b578b9d5f560fea04be01b80b698d6cef193437113b87d6767239dba56f
    let missing: Vec<&str> = asked
        .iter()
        .filter(|id| !recorded.iter().any(|review| &review.id == *id))
        .map(String::as_str)
        .collect();
    if missing.is_empty() {
        // @lfy def/mcp/main.lfy:report#report:report:979112dd7ac1a472af0b4b0b3b534d9bf9f6464235b53bb1523bcb7fab70370c
        text.push_str(&format!(
            "\nevery criterion and test asked about has a verdict; end your final message with the one line {}",
            generation::REVIEWED
        ));
    } else {
        // @lfy def/mcp/main.lfy:report#report:report:33d92b578b9d5f560fea04be01b80b698d6cef193437113b87d6767239dba56f
        let listed: Vec<&str> = missing.iter().take(LISTED_MISSING).copied().collect();
        text.push_str(&format!("\n{} still missing: {}", missing.len(), listed.join(", ")));
    }
    Ok(text)
}

// ---- what a compiler run is bound to ------------------------------------------------

// Decision: a hosted compiler reads its obligations, its dependencies' interfaces, and its
// markers from the very request the compile handed it, so a compiler deep in a long run
// never reads definitions that may have changed since the compile planned.

/// What one compiler run is: the binding of its hosted session, and the request the compile
/// handed it.
// @lfy def/mcp/main.lfy:task#task:compilerTool:1e78463366ba8fbbfbec5fc142e181f67f7b2aead81f243e58972718f0a441a8
struct Run<'s> {
    /// What the hosted session is bound to.
    binding: &'s Binding,
    /// The request its compiler run was given.
    request: &'s Request,
}

/// The run the session is bound to, and an error naming the tool when the session is bound
/// to no compiler run. Nothing is read or written when there is none.
// @lfy def/mcp/main.lfy:task#task:compilerTool:09099b01d8e84ae85a89b2a12642ec68e8d2b277dfec75f117a5e093caf818b2
// @lfy def/mcp/main.lfy:dependency#dependency:compilerTool:94e97e4b9416897e8f7296ad1449f08bf7cb435fe9a76fc4125ba6de8559c49e
// @lfy def/mcp/main.lfy:mark#mark:compilerTool:56146f12eb0eb11e3ccbcf6b89b563f1173e5c188a183687178892cdd6aefb62
// @lfy def/mcp/main.lfy:submit#submit:compilerTool:ee05d8be309e4a7aab71c6ed469b76e49a958abace52e8b6407087bf66c21140
fn own_request<'s>(session: &'s Session, tool: &str) -> Result<Run<'s>, String> {
    match &session.binding {
        Some(binding) => match &binding.request {
            // The batch and the units acted on are the request's own, and the outputs are
            // read and written under the binding's directory, which is the root the hosted
            // session's workspace was read at.
            // @lfy def/mcp/main.lfy:task#task:compilerTool:1e78463366ba8fbbfbec5fc142e181f67f7b2aead81f243e58972718f0a441a8
            // @lfy def/mcp/main.lfy:task#task:compilerTool:a0f3f916620f9e289b39745855f3979f0a0db1a4ff2e30369ec791cd1d94087a
            // @lfy def/mcp/main.lfy:dependency#dependency:compilerTool:0044266dd6cf0877762ead31dc8ae4567556e4f98fae42343fc4f7d1616d887c
            // @lfy def/mcp/main.lfy:dependency#dependency:compilerTool:0c15908a4dcfda7bb04d0631866acd9f199cf990a0bd8a108b4e4d2744608e3a
            // @lfy def/mcp/main.lfy:mark#mark:compilerTool:a8577d54d63dba15d279a393a495d3cc11ecc51430e5ccdd9709b2c5dcb1d764
            // @lfy def/mcp/main.lfy:mark#mark:compilerTool:85e5595e052eeca50b56f9cebdba26de23ab6e384826458c5f80bff35a6a3579
            // @lfy def/mcp/main.lfy:submit#submit:compilerTool:60628d931d17c35840a5661d90ba57e6f4365679924ab922f3dc16beb99f6278
            // @lfy def/mcp/main.lfy:submit#submit:compilerTool:fe5e191c412762eabc28bb250d5cdf400a97c9315325f022cff7b603d32ee079
            Some(request) => Ok(Run { binding, request }),
            // @lfy def/mcp/main.lfy:dependency#dependency:compilerTool:94e97e4b9416897e8f7296ad1449f08bf7cb435fe9a76fc4125ba6de8559c49e
            None => Err(format!("{tool} acts only in a session bound to a compiler run")),
        },
        // @lfy def/mcp/main.lfy:mark#mark:compilerTool:56146f12eb0eb11e3ccbcf6b89b563f1173e5c188a183687178892cdd6aefb62
        // @lfy def/mcp/main.lfy:submit#submit:compilerTool:ee05d8be309e4a7aab71c6ed469b76e49a958abace52e8b6407087bf66c21140
        None => Err(format!("{tool} acts only in a session bound to a compiler run")),
    }
}

/// The units of a batch, as indices into the plan, leaving out any the plan does not hold.
// @lfy def/mcp/main.lfy:task#task:task:d056015a5c0120d12f5e1daaebd2c2f0c96947c0bbff29ed4ebdeb57af2bcbe2
fn batch_units(plan: &Plan, batch: &Batch) -> Vec<usize> {
    batch.units.iter().copied().filter(|&index| plan.units.get(index).is_some()).collect()
}

/// The stems of the units of a batch, joined.
// @lfy def/mcp/main.lfy:task#task:task:20cff63e998498e4042833f30de9a63a11338bcbcc0b15f2191ddcf2f7def4ab
fn batch_stems(plan: &Plan, batch: &Batch) -> String {
    let stems: Vec<&str> =
        batch_units(plan, batch).into_iter().map(|index| plan.units[index].stem.as_str()).collect();
    stems.join(", ")
}

/// A name with any leading definition file path and colon set aside: the path, when one was
/// spelled, and what remains.
// @lfy def/mcp/main.lfy:dependency#dependency:dependency:0e9dc230cab50f2d56b0c3bb4b3df7405447b216f7d728bbdb5c789f693619a0
// @lfy def/mcp/main.lfy:mark#mark:mark:ea0e414361181918c63cd2a060bf3bdae8d34bdc0cc992fa89cac7fc18138b2f
fn without_path(name: &str) -> (Option<&str>, &str) {
    match name.rsplit_once(':') {
        Some((path, rest)) if path.ends_with(".lfy") => (Some(path), rest),
        _ => (None, name),
    }
}

/// What remains of a name split into the identifier it begins with and the member name that
/// follows the first dot, when one does.
// @lfy def/mcp/main.lfy:dependency#dependency:dependency:0e9dc230cab50f2d56b0c3bb4b3df7405447b216f7d728bbdb5c789f693619a0
// @lfy def/mcp/main.lfy:mark#mark:mark:ea0e414361181918c63cd2a060bf3bdae8d34bdc0cc992fa89cac7fc18138b2f
fn identifier_and_member(rest: &str) -> (&str, Option<&str>) {
    match rest.split_once('.') {
        Some((identifier, member)) => (identifier, Some(member)),
        None => (rest, None),
    }
}

/// Every entity of one file a marker may name, with the name it is named by: each
/// declaration of the file scope and each entity of `extra` the file declares, then the
/// members each of them declares as `owner.member`. The owner's own line comes after its
/// members', as acceptance builds the same table.
// @lfy def/mcp/main.lfy:task#task:task:0ba2a96c6162e5b2088f0d0af27dcf6dcd7a99b009bf3be3772ffa13ffa864f4
// @lfy def/mcp/main.lfy:mark#mark:mark:1fe93b7a289817e552344c2be562a1942c5871b156c0bb103e88f3c5c41e4116
fn named_in_file(model: &Model, source: FileId, extra: &[EntityId]) -> Vec<(String, EntityId)> {
    let mut declarations: Vec<EntityId> = Vec::new();
    if let Some(&scope) = model.file_scopes.get(source) {
        for symbol in model.scopes[scope].declared() {
            let symbol = &model.symbols[symbol];
            let record = &model.entities[symbol.entity];
            if symbol.kind != SymbolKind::Module
                && record.file == Some(source)
                && record.node.is_some()
                && !declarations.contains(&symbol.entity)
            {
                declarations.push(symbol.entity);
            }
        }
    }
    for &entity in extra {
        let record = &model.entities[entity];
        if record.node.is_some() && record.file == Some(source) && !declarations.contains(&entity) {
            declarations.push(entity);
        }
    }
    let mut out: Vec<(String, EntityId)> = Vec::new();
    for entity in declarations {
        let Some(identifier) = model.entities[entity].identifier.clone() else { continue };
        for symbol in model.members(entity) {
            let member = model.symbols[symbol].entity;
            if let Some(name) = &model.entities[member].identifier
                && model.entities[member].node.is_some()
            {
                out.push((format!("{identifier}.{name}"), member));
            }
        }
        out.push((identifier, entity));
    }
    out
}

/// Every entity a unit's criteria and tests may be for, with the name it is named by: those
/// of the unit's own file, then those of each file declaring an entity moved into it.
// @lfy def/mcp/main.lfy:task#task:task:0ba2a96c6162e5b2088f0d0af27dcf6dcd7a99b009bf3be3772ffa13ffa864f4
fn named_in_unit(model: &Model, workspace: &Workspace, unit: &Unit) -> Vec<(String, EntityId)> {
    let mut sources: Vec<FileId> = vec![workspace.files[unit.file].source];
    for &entity in &unit.entities {
        if let Some(source) = model.entities[entity].file
            && !sources.contains(&source)
        {
            sources.push(source);
        }
    }
    sources.iter().flat_map(|&source| named_in_file(model, source, &unit.entities)).collect()
}

/// A criterion's or test's place, as a review request spells it: the file of its origin
/// relative to the workspace root, a colon, and the line its origin begins on.
// @lfy def/mcp/main.lfy:task#task:task:48b5a17b6bd8bf11626b7e3c8a77f30620a9bcf76c647c3d0cb15aaa2ac3e582
fn origin_place(model: &Model, origin: NodeRef) -> String {
    let line = model.first_token(origin).map_or(1, |token| token.line);
    format!("{}:{line}", model.sources[origin.file].path)
}

/// One criterion or test of a request, read for a line of `elfie_task`.
// @lfy def/mcp/main.lfy:task#task:task:bc0cfc213f87dda7b8e1106191bb902bdb5a2b08b2472742020be295a621309b
struct Asked {
    /// Its id.
    id: String,
    /// The entity it is for; `None` when it is global.
    entity: Option<EntityId>,
    /// Where it is, as a review request spells a place.
    place: String,
    /// The text a request and a review show a reader.
    text: String,
}

/// One criterion or test of a request as its id, the entity it is for, its place, and the
/// text a request and a review show a reader.
// @lfy def/mcp/main.lfy:task#task:task:bc0cfc213f87dda7b8e1106191bb902bdb5a2b08b2472742020be295a621309b
// @lfy def/mcp/main.lfy:task#task:task:8bf5a91fa0df3686fb02ff5457cd19a9f3c4d034aceb88db292e7f850ecd6638
fn requirement_parts(model: &Model, requirement: &Requirement) -> Asked {
    match requirement {
        Requirement::Criterion(criterion) => Asked {
            id: criterion.id.clone(),
            entity: criterion.entity,
            place: origin_place(model, criterion.origin),
            text: criterion.text(),
        },
        // @lfy def/mcp/main.lfy:task#task:task:8bf5a91fa0df3686fb02ff5457cd19a9f3c4d034aceb88db292e7f850ecd6638
        Requirement::Test(test) => Asked {
            id: test.id.clone(),
            entity: test.entity,
            place: origin_place(model, test.origin),
            text: lowered_test_text(test),
        },
    }
}

/// The rewording of one criterion or test of a unit, when its id rewords one the unit's
/// existing outputs were generated against.
// @lfy def/mcp/main.lfy:task#task:task:8aab948f1ff4b43702f1158f16a22c2a5c3be0d41ed51e5ff90073af5c03d692
fn rewording_of<'r>(request: &'r Request, path: &str, id: &str) -> Option<&'r Rewording> {
    request.reworded.get(path)?.iter().find(|rewording| rewording.id == id)
}

/// What a criterion or test of a unit is marked as: `reworded` when its id rewords one the
/// unit's existing outputs were generated against, `kept` when those outputs were generated
/// against it, `new` when they were generated against a known set that does not hold it or
/// there are no outputs yet, and `unknown` when what they answered is not recorded.
// @lfy def/mcp/main.lfy:task#task:task:00479e8dc9cc7003d7a8fc71ee72b37424f99ac82adaf9497bd8a64df5cd83a2
// @lfy def/mcp/main.lfy:task#task:task:aa3aedc17d31c8c2288f74ab8d995d37aabd29e988ed6623b3ab368c90880080
// @lfy def/mcp/main.lfy:task#task:task:3fd2e90fbb5affd82dc46bf4e87ba7bcb8ba2f4df7047de83fea316774ee4d01
fn mark_of(request: &Request, path: &str, has_outputs: bool, id: &str) -> &'static str {
    // A criterion that kept its place and changed its words took a new id, which the compile
    // has already moved its markers to, so it is marked apart from what is really new.
    // @lfy def/mcp/main.lfy:task#task:task:8aab948f1ff4b43702f1158f16a22c2a5c3be0d41ed51e5ff90073af5c03d692
    if rewording_of(request, path, id).is_some() {
        return "reworded";
    }
    match request.answered.get(path) {
        // @lfy def/mcp/main.lfy:task#task:task:00479e8dc9cc7003d7a8fc71ee72b37424f99ac82adaf9497bd8a64df5cd83a2
        Some(answered) if answered.iter().any(|known| known == id) => "kept",
        // @lfy def/mcp/main.lfy:task#task:task:aa3aedc17d31c8c2288f74ab8d995d37aabd29e988ed6623b3ab368c90880080
        Some(_) => "new",
        None if !has_outputs => "new",
        // @lfy def/mcp/main.lfy:task#task:task:3fd2e90fbb5affd82dc46bf4e87ba7bcb8ba2f4df7047de83fea316774ee4d01
        None => "unknown",
    }
}

/// What this compiler run must do, for each unit of its batch or for the one named.
// @lfy def/mcp/main.lfy:task#task:task:03c0e861c4bbd25a35c249a398a25887a28c2cef4fe08f743b831393be179644
pub fn task(session: &Session, unit: Option<&str>) -> Result<String, String> {
    let run = own_request(session, "elfie_task")?;
    let (binding, request) = (run.binding, run.request);
    // Everything below is read from the request and from the program the compile lowered;
    // no definition file and no output is read.
    // @lfy def/mcp/main.lfy:task#task:task:03c0e861c4bbd25a35c249a398a25887a28c2cef4fe08f743b831393be179644
    let plan = &binding.plan;
    let program = &binding.program;
    let workspace = &program.workspace;
    let model = &workspace.model;
    let batch = &request.batch;
    // @lfy def/mcp/main.lfy:task#task:task:d056015a5c0120d12f5e1daaebd2c2f0c96947c0bbff29ed4ebdeb57af2bcbe2
    let indexes: Vec<usize> = match unit {
        Some(stem) => {
            let found: Vec<usize> = batch_units(plan, batch)
                .into_iter()
                .filter(|&index| plan.units[index].stem == stem)
                .collect();
            if found.is_empty() {
                // @lfy def/mcp/main.lfy:task#task:task:20cff63e998498e4042833f30de9a63a11338bcbcc0b15f2191ddcf2f7def4ab
                return Err(format!(
                    "no unit of this batch has the stem {stem}; the units of {} are: {}",
                    batch.identifier,
                    batch_stems(plan, batch)
                ));
            }
            found
        }
        None => batch_units(plan, batch),
    };
    let mut lines: Vec<String> = Vec::new();
    let (mut fresh, mut rephrased, mut kept, mut unknown, mut removed) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    for index in indexes {
        let of_unit = &plan.units[index];
        let file = &workspace.files[of_unit.file];
        let target = &workspace.targets[of_unit.target];
        let reason = of_unit.reason.map_or_else(|| "up to date".to_string(), |reason| reason.as_str().to_string());
        // @lfy def/mcp/main.lfy:task#task:task:260c99a22d5afd2cb5bfe5e5341310aff3e4b5c20a62493303d03bc95d6f4767
        lines.push(format!("unit {} {} {reason}", of_unit.stem, file.path));
        if of_unit.outputs.is_empty() {
            // @lfy def/mcp/main.lfy:task#task:task:3701f0af0023dfc1eadd1dee395415c17f3523d19c39a1d6d8f30a8aff532e9e
            lines.push(format!(
                "no output yet; its outputs go under {} at the paths the guidance spells from the stem {}",
                target.output_directory, of_unit.stem
            ));
        } else {
            // @lfy def/mcp/main.lfy:task#task:task:93d986fe4163c82020f47668d2a385e3662ee162cec7a9332137b5df9c98b068
            let outputs: Vec<&str> = of_unit.outputs.iter().map(|map| map.output.as_str()).collect();
            lines.push(format!("outputs {}", outputs.join(", ")));
        }
        let named = named_in_unit(model, workspace, of_unit);
        let own = workspace.files[of_unit.file].source;
        let requirements: &[Requirement] =
            request.requirements.get(&file.path).map(Vec::as_slice).unwrap_or(&[]);
        let mut headed: Vec<EntityId> = Vec::new();
        let mut answered: Vec<EntityId> = Vec::new();
        // @lfy def/mcp/main.lfy:task#task:task:e6bdc2cd8bf73910039781fad6be14d73994cd9495ecfb29415132f0e0b7926d
        for requirement in requirements {
            let Asked { id, entity, place, text } = requirement_parts(model, requirement);
            if let Some(entity) = entity {
                if !answered.contains(&entity) {
                    answered.push(entity);
                }
                // @lfy def/mcp/main.lfy:task#task:task:0ba2a96c6162e5b2088f0d0af27dcf6dcd7a99b009bf3be3772ffa13ffa864f4
                if !headed.contains(&entity) {
                    headed.push(entity);
                    let is_own_file = model.file_entities.get(own) == Some(&entity);
                    let name = if is_own_file {
                        file.path.clone()
                    } else {
                        named
                            .iter()
                            .find(|&&(_, candidate)| candidate == entity)
                            .map(|(name, _)| name.clone())
                            .unwrap_or_else(|| identifier_of(model, entity))
                    };
                    // An entity moved into the unit is declared in another file, which the
                    // line names so that a marker for it spells that file.
                    // @lfy def/mcp/main.lfy:task#task:task:a151226a2f7853ca59f5d23754f07954956c81793b8a0b4e771b88a500617003
                    match model.entities[entity].file.filter(|&source| source != own) {
                        Some(source) => {
                            lines.push(format!("{name} declared in {}", model.sources[source].path));
                        }
                        None => lines.push(name),
                    }
                }
            }
            let marked = mark_of(request, &file.path, !of_unit.outputs.is_empty(), &id);
            match marked {
                "kept" => kept += 1,
                "unknown" => unknown += 1,
                "reworded" => rephrased += 1,
                _ => fresh += 1,
            }
            // @lfy def/mcp/main.lfy:task#task:task:bc0cfc213f87dda7b8e1106191bb902bdb5a2b08b2472742020be295a621309b
            lines.push(format!("{marked} {id} {place} — {text}"));
            // What a reworded one replaces, on the line after it.
            // @lfy def/mcp/main.lfy:task#task:task:8aab948f1ff4b43702f1158f16a22c2a5c3be0d41ed51e5ff90073af5c03d692
            if let Some(rewording) = rewording_of(request, &file.path, &id) {
                lines.push(format!("replaces {} — {}", rewording.replaces, rewording.text));
            }
        }
        // Every entity of the unit must carry a marker, so those with nothing to answer for
        // are named: no criterion and no test of the unit is for them.
        // @lfy def/mcp/main.lfy:task#task:task:f9fb73337a13788bc2c97891ea0b6725150d8f585b1ea3c8102578a43fdc42d6
        let without: Vec<String> = of_unit
            .entities
            .iter()
            .filter(|entity| !answered.contains(entity))
            .map(|&entity| identifier_of(model, entity))
            .collect();
        if without.is_empty() {
            // @lfy def/mcp/main.lfy:task#task:task:8cda0fab4d2e20c30f36514faa7a119d7713f6defa2830d8a2284e0161739f87
            lines.push("no entity of the unit lacks a criterion or test".to_string());
        } else {
            // @lfy def/mcp/main.lfy:task#task:task:f9fb73337a13788bc2c97891ea0b6725150d8f585b1ea3c8102578a43fdc42d6
            lines.push(format!(
                "no criterion or test is for {}; each still needs a marker",
                without.join(", ")
            ));
        }
        match request.removed.get(&file.path) {
            // @lfy def/mcp/main.lfy:task#task:task:0e1730d59363044ef74bcf4673aef8eae5aecf93a017aa90bc7f8d9d457f6afd
            Some(gone) if !gone.is_empty() => {
                lines.push("these were removed since its outputs were accepted".to_string());
                for one in gone {
                    removed += 1;
                    // @lfy def/mcp/main.lfy:task#task:task:0e1730d59363044ef74bcf4673aef8eae5aecf93a017aa90bc7f8d9d457f6afd
                    lines.push(format!("removed {} {} — {}", one.id, one.place, one.text));
                }
            }
            // @lfy def/mcp/main.lfy:task#task:task:c6544cb71fb79033882e38ad1d044989c8fa3a6dc97e274b3d4054fbb9b65bda
            None if !of_unit.outputs.is_empty() => {
                lines.push("which criteria and tests were removed is not known".to_string());
            }
            _ => {}
        }
    }
    // @lfy def/mcp/main.lfy:task#task:task:ca1abaa819942eb8c6c130e520f1c36ee16815c41b54d388cca6a9b8ad14a76c
    lines.push(format!(
        "{fresh} new, {rephrased} reworded, {kept} kept, {unknown} unknown, {removed} removed"
    ));
    Ok(lines.join("\n"))
}

/// The lines of one entity in the interface text of a unit: its own line, then the line of
/// each member or enum member it declares, or only the line of one member when one is named.
// @lfy def/mcp/main.lfy:dependency#dependency:dependency:5187f431c8e1a018407dfaf23c821f2cfc1f4a15370dc6acb8175a28e7fcd50c
// @lfy def/mcp/main.lfy:dependency#dependency:dependency:82fa16bd07081751c96e7033b5801a1698a74ccf133725c2d1e01ea9edd01680
fn interface_lines(text: &str, identifier: &str, member: Option<&str>) -> Vec<String> {
    let opening = format!("- `{identifier}` (");
    let mut found: Vec<String> = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line.starts_with(&opening) {
            inside = true;
            found.push(line.to_string());
            continue;
        }
        if !inside {
            continue;
        }
        let Some(rest) = line.strip_prefix("  ") else { break };
        // @lfy def/mcp/main.lfy:dependency#dependency:dependency:82fa16bd07081751c96e7033b5801a1698a74ccf133725c2d1e01ea9edd01680
        match member {
            Some(member) if !rest.starts_with(&format!("- `{member}` (")) => {}
            _ => found.push(line.to_string()),
        }
    }
    found
}

/// What one entity of a dependency of this compiler run's batch offers its dependents.
// @lfy def/mcp/main.lfy:dependency#dependency:dependency:88edd00efeadac6571b81f940a3b239335ff17aac9c416776584049ade3de935
pub fn dependency(session: &Session, name: &str) -> Result<String, String> {
    let run = own_request(session, "elfie_dependency")?;
    let (binding, request) = (run.binding, run.request);
    // The interfaces are the request's own, and the lines come from the program the compile
    // lowered; no definition file and no output is read.
    // @lfy def/mcp/main.lfy:dependency#dependency:dependency:88edd00efeadac6571b81f940a3b239335ff17aac9c416776584049ade3de935
    let plan = &binding.plan;
    let workspace = &binding.program.workspace;
    let model = &workspace.model;
    // @lfy def/mcp/main.lfy:dependency#dependency:dependency:0e9dc230cab50f2d56b0c3bb4b3df7405447b216f7d728bbdb5c789f693619a0
    let (in_file, rest) = without_path(name);
    let (identifier, member) = identifier_and_member(rest);
    let mut sections: Vec<String> = Vec::new();
    // @lfy def/mcp/main.lfy:dependency#dependency:dependency:48fa7258825a24866309481b9e6048acc49c37a1e74ed155a56a3210a884e862
    for interface in &request.interfaces {
        let Some(of_unit) = plan.units.get(interface.unit) else { continue };
        let file = &workspace.files[of_unit.file];
        // @lfy def/mcp/main.lfy:dependency#dependency:dependency:7b425db08ad081030e6137f0d6924c40b3b223d36d698551284b593335193519
        if in_file.is_some_and(|path| path != file.path) {
            continue;
        }
        // @lfy def/mcp/main.lfy:dependency#dependency:dependency:48fa7258825a24866309481b9e6048acc49c37a1e74ed155a56a3210a884e862
        if !interface
            .entities
            .iter()
            .any(|&entity| model.entities[entity].identifier.as_deref() == Some(identifier))
        {
            continue;
        }
        let text = generation::interface_of(workspace, of_unit);
        let found = interface_lines(&text, identifier, member);
        if found.is_empty() {
            continue;
        }
        // @lfy def/mcp/main.lfy:dependency#dependency:dependency:53420221788d685744cc63a541e15f79755a4b23efe12ec64289b0e9c90ad313
        let mut out = format!("{} {}\noutputs {}", of_unit.stem, file.path, interface.outputs.join(", "));
        // @lfy def/mcp/main.lfy:dependency#dependency:dependency:5187f431c8e1a018407dfaf23c821f2cfc1f4a15370dc6acb8175a28e7fcd50c
        for line in found {
            out.push('\n');
            out.push_str(&line);
        }
        sections.push(out);
    }
    if sections.is_empty() {
        // @lfy def/mcp/main.lfy:dependency#dependency:dependency:57ea76cedd14a64116b4e927fb4f9f13577e2df3fe6caa9fd1c92a2dd6d88d06
        return Err(format!(
            "no dependency outside the batch declares {name}; elfie_entity reads an entity of the batch itself, or of any file"
        ));
    }
    // @lfy def/mcp/main.lfy:dependency#dependency:dependency:52155c3bef2ef807556011be184c10507617e56c46ebfded60f2e934d1a72699
    Ok(sections.join("\n\n---\n\n"))
}

// ---- the markers of a compiler run --------------------------------------------------

// Decision: an id is 64 hex digits and more, which an agent copies wrong often enough to
// matter, so the server writes a marker's id from one it has checked, and moves every marker
// of a reworded criterion to its new id in one call.

/// The source paths a marker of one of this batch's regions may name: the file of each unit
/// of the batch, and the file of each entity moved into one.
// @lfy def/mcp/main.lfy:mark#mark:mark:aac83695fcc88a2c0a4688d77974f2d0a6805ebf6b2d752b2141ff71a8a745a2
fn own_sources(run: &Run<'_>) -> Vec<String> {
    let binding = run.binding;
    let workspace = &binding.program.workspace;
    let model = &workspace.model;
    let mut found: Vec<String> = Vec::new();
    for index in batch_units(&binding.plan, &run.request.batch) {
        let of_unit = &binding.plan.units[index];
        let mut paths = vec![workspace.files[of_unit.file].path.clone()];
        for &entity in &of_unit.entities {
            if let Some(source) = model.entities[entity].file {
                paths.push(model.sources[source].path.clone());
            }
        }
        for path in paths {
            if !found.contains(&path) {
                found.push(path);
            }
        }
    }
    found
}

/// The target of the batch of a compiler run, read off the session's workspace so that its
/// output directory is read under the binding's directory; `None` when the batch is empty.
// @lfy def/mcp/main.lfy:mark#mark:mark:20e94fd8b37ced24d2351848435ef39376d3fc15eeb5d4e64ccffd5f95cb4053
fn batch_own_target<'w>(workspace: &'w Workspace, run: &Run<'_>) -> Option<&'w Target> {
    let index = *run.request.batch.units.first()?;
    let of_unit = run.binding.plan.units.get(index)?;
    workspace.targets.get(of_unit.target)
}

/// One criterion or test of the batch's units, with the unit it belongs to, by id.
// @lfy def/mcp/main.lfy:mark#mark:mark:165048edf3b25a24ddb15a6916231fdc341c3ce643d5b2d74c220ae0c65ab193
fn requirement_unit(run: &Run<'_>, id: &str) -> Option<usize> {
    let binding = run.binding;
    let workspace = &binding.program.workspace;
    batch_units(&binding.plan, &run.request.batch).into_iter().find(|&index| {
        let path = &workspace.files[binding.plan.units[index].file].path;
        run.request
            .requirements
            .get(path)
            .is_some_and(|requirements| requirements.iter().any(|one| one.id() == id))
    })
}

/// Whether one id is that of a global criterion or test of the request.
// @lfy def/mcp/main.lfy:mark#mark:mark:dd3ef6b416a23f341639e29ff2f36569d059617f0c4d6c5404dea75abb9f0ef2
fn is_global_requirement(request: &Request, id: &str) -> bool {
    request.globals.iter().any(|one| one.id() == id)
}

/// One marker spelled as a marker line spells it after `@lfy` and a space.
// @lfy def/mcp/main.lfy:mark#mark:mark:5bb1f1250f7f259cd65c04fd3be30c251cc135967710c7ce195a78f31866f8fe
// @lfy def/mcp/main.lfy:search#search:search:fa10d4c06de0e21bae6488a493c889c66e5ea9d22f426fbcf0488df26d15e041
fn marker_text(marker: &Marker) -> String {
    match (&marker.entity, marker.column) {
        (Some(entity), _) => match &marker.requirement {
            Some(requirement) => format!("{}:{entity}#{requirement}", marker.file),
            None => format!("{}:{entity}", marker.file),
        },
        (None, Some(column)) => format!("{}:{}:{column}", marker.file, marker.line),
        (None, None) => format!("{}:{}", marker.file, marker.line),
    }
}

/// Every marker one of this batch's output files holds, read as acceptance reads them.
// @lfy def/mcp/main.lfy:mark#mark:mark:c3d247ab4b165bf17b1ba969469f280619af5c95535eec67c2881e691f8a9a83
fn markers_of(text: &str) -> Vec<Marker> {
    generation::parse_markers(text)
}

/// Every marker of one line of an output, as acceptance reads them.
// @lfy def/mcp/main.lfy:mark#mark:mark:c3d247ab4b165bf17b1ba969469f280619af5c95535eec67c2881e691f8a9a83
fn markers_on(markers: &[Marker], line: usize) -> Vec<&Marker> {
    markers.iter().filter(|marker| marker.output_line == line).collect()
}

/// Rewrite to a new id every marker of this run's outputs that names an old one.
///
/// Which markers those are is [`generation::rename`]'s to decide, and it decides it as
/// acceptance does: only the markers naming the file of the unit the new id belongs to, or
/// of a file declaring an entity moved into it, so another unit's markers are never touched.
/// The outputs it returns are the ones whose markers it rewrote, and only those are written.
// @lfy def/mcp/main.lfy:mark#mark:mark:4f60b16548a3a5d470c0ee9cd9a0c70970ffdedc84b94fa5e4f5efbde3465999
fn rename_markers(
    session: &Session,
    run: &Run<'_>,
    target: &Target,
    id: &str,
    replaces: &str,
) -> Result<String, String> {
    let workspace = &session.workspace;
    let binding = run.binding;
    // @lfy def/mcp/main.lfy:mark#mark:mark:165048edf3b25a24ddb15a6916231fdc341c3ce643d5b2d74c220ae0c65ab193
    let Some(index) = requirement_unit(run, id) else {
        return Err(format!(
            "{id} is no criterion or test of the units of the batch {}; elfie_task lists them",
            run.request.batch.identifier
        ));
    };
    // The output files of the batch's target as they are when the call arrives.
    // @lfy def/mcp/main.lfy:mark#mark:mark:9cbf7b9aff22e535179edfb2494ceb9e2aa081cb354077d9a0d66e38056c48d6
    let mut read: Vec<Output> = Vec::new();
    for output in output_files(workspace, target) {
        let path = workspace.root.join(&output);
        let Ok(text) = fs::read_to_string(&path) else { continue };
        read.push(Output { path: output, text });
    }
    let ids = BTreeMap::from([(replaces.to_string(), id.to_string())]);
    // @lfy def/mcp/main.lfy:mark#mark:mark:9cbf7b9aff22e535179edfb2494ceb9e2aa081cb354077d9a0d66e38056c48d6
    let renamed = generation::rename(workspace, &binding.plan, index, &read, &ids);
    let mut rewritten: Vec<String> = Vec::new();
    for output in &renamed {
        // @lfy def/mcp/main.lfy:mark#mark:mark:9cbf7b9aff22e535179edfb2494ceb9e2aa081cb354077d9a0d66e38056c48d6
        let path = workspace.root.join(&output.path);
        fs::write(&path, &output.text)
            .map_err(|error| format!("{} cannot be written: {error}", output.path))?;
        // The lines that differ from the file as it was read are the rewritten markers, since
        // `rename` keeps every other line of an output it returns.
        // @lfy def/mcp/main.lfy:mark#mark:mark:4f60b16548a3a5d470c0ee9cd9a0c70970ffdedc84b94fa5e4f5efbde3465999
        let before: Vec<&str> = read
            .iter()
            .find(|one| one.path == output.path)
            .map(|one| one.text.split_inclusive('\n').collect())
            .unwrap_or_default();
        let after = markers_of(&output.text);
        for (at, line) in output.text.split_inclusive('\n').enumerate() {
            if before.get(at).is_some_and(|was| *was == line) {
                continue;
            }
            // @lfy def/mcp/main.lfy:mark#mark:mark:5bb1f1250f7f259cd65c04fd3be30c251cc135967710c7ce195a78f31866f8fe
            for marker in markers_on(&after, at + 1) {
                rewritten.push(format!("{}:{} {}", output.path, at + 1, marker_text(marker)));
            }
        }
    }
    if rewritten.is_empty() {
        // @lfy def/mcp/main.lfy:mark#mark:mark:675bf462c93e343e85219da2087f1672279fc0bc713a21b5d0f5550e274a6a40
        return Err(format!(
            "no marker of the output files of the target {} naming {} names {replaces}",
            target.identifier,
            workspace.files[binding.plan.units[index].file].path
        ));
    }
    // @lfy def/mcp/main.lfy:mark#mark:mark:5bb1f1250f7f259cd65c04fd3be30c251cc135967710c7ce195a78f31866f8fe
    let mut text = format!("{} marker{} rewritten", rewritten.len(), if rewritten.len() == 1 { "" } else { "s" });
    for line in rewritten {
        text.push('\n');
        text.push_str(&line);
    }
    Ok(text)
}

/// The entity one name picks out of the batch's units, with the unit it belongs to and the
/// path of the file that declares it.
// @lfy def/mcp/main.lfy:mark#mark:mark:1fe93b7a289817e552344c2be562a1942c5871b156c0bb103e88f3c5c41e4116
fn entity_named(run: &Run<'_>, entity: &str) -> Result<(usize, String, String), String> {
    let (binding, request) = (run.binding, run.request);
    let workspace = &binding.program.workspace;
    let model = &workspace.model;
    // @lfy def/mcp/main.lfy:mark#mark:mark:ea0e414361181918c63cd2a060bf3bdae8d34bdc0cc992fa89cac7fc18138b2f
    let (in_file, rest) = without_path(entity);
    let (identifier, member) = identifier_and_member(rest);
    let mut found: Vec<(usize, String, String)> = Vec::new();
    for index in batch_units(&binding.plan, &request.batch) {
        for &candidate in &binding.plan.units[index].entities {
            if model.entities[candidate].identifier.as_deref() != Some(identifier) {
                continue;
            }
            let Some(source) = model.entities[candidate].file else { continue };
            let path = model.sources[source].path.clone();
            if in_file.is_some_and(|named| named != path) {
                continue;
            }
            // @lfy def/mcp/main.lfy:mark#mark:mark:1fe93b7a289817e552344c2be562a1942c5871b156c0bb103e88f3c5c41e4116
            if let Some(member) = member
                && !model
                    .members(candidate)
                    .into_iter()
                    .any(|symbol| model.symbols[symbol].name == member)
            {
                continue;
            }
            let name = match member {
                Some(member) => format!("{identifier}.{member}"),
                None => identifier.to_string(),
            };
            if !found.iter().any(|(_, _, spelled)| spelled == &path) {
                found.push((index, name, path));
            }
        }
    }
    match found.len() {
        // @lfy def/mcp/main.lfy:mark#mark:mark:64c1518801f24801feb6b601cbda298cc2a1a97c81889b15a61fe32d5a3ded45
        0 => Err(format!(
            "no unit of the batch {} declares {entity}; its units are: {}",
            request.batch.identifier,
            batch_stems(&binding.plan, &request.batch)
        )),
        1 => Ok(found.remove(0)),
        // @lfy def/mcp/main.lfy:mark#mark:mark:a38062a1fcda5efa808baa27f8d58d69ab0670d6640e713cece4075635abc240
        _ => {
            let files: Vec<&str> = found.iter().map(|(_, _, path)| path.as_str()).collect();
            Err(format!(
                "{entity} names entities of more than one file ({}); a file path and a colon before the name chooses one",
                files.join(", ")
            ))
        }
    }
}

/// Write one marker line into one of this run's outputs.
// @lfy def/mcp/main.lfy:mark#mark:mark:e959da5d59df47ac7933bb07c057234854cf20b78a1ae1296c4d2521057c5f32
fn insert_marker(
    session: &Session,
    run: &Run<'_>,
    target: &Target,
    id: Option<&str>,
    file: &str,
    line: usize,
    entity: &str,
) -> Result<String, String> {
    let workspace = &session.workspace;
    let binding = run.binding;
    // @lfy def/mcp/main.lfy:mark#mark:mark:62c0bc17d8c1d1e37e6821582cd6b59f05e0204a3f3edd6f1e33e9d2597fc2db
    if !output_files(workspace, target).iter().any(|known| known == file) {
        return Err(format!(
            "{file} is not an output file of the target {}; write the output first, then insert a marker into it",
            target.identifier
        ));
    }
    let path = workspace.root.join(file);
    let text = fs::read_to_string(&path).map_err(|error| format!("{file} cannot be read: {error}"))?;
    let count = text.lines().count();
    // @lfy def/mcp/main.lfy:mark#mark:mark:a86c8a8dd444a7e09d183e1c131c3e4e0e74fdc0a4a752698290645da729a4b0
    if line < 1 || line > count + 1 {
        return Err(format!("{file} has {count} lines, so line must be between 1 and {}", count + 1));
    }
    // The last marker before the line says whose region the line falls in; a region of
    // another unit is never written into.
    // @lfy def/mcp/main.lfy:mark#mark:mark:aac83695fcc88a2c0a4688d77974f2d0a6805ebf6b2d752b2141ff71a8a745a2
    let markers = markers_of(&text);
    let allowed = own_sources(run);
    if let Some(before) = markers.iter().rev().find(|marker| marker.output_line < line)
        && !allowed.contains(&before.file)
    {
        let owner = binding
            .plan
            .units
            .iter()
            .find(|of_unit| workspace.files[of_unit.file].path == before.file)
            .map(|of_unit| format!(" the unit {}", of_unit.stem))
            .unwrap_or_default();
        return Err(format!(
            "line {line} of {file} falls in a region of {}, which belongs to{}",
            before.file,
            if owner.is_empty() { " no unit of this batch".to_string() } else { owner }
        ));
    }
    let (index, name, declaring) = entity_named(run, entity)?;
    // @lfy def/mcp/main.lfy:mark#mark:mark:dd3ef6b416a23f341639e29ff2f36569d059617f0c4d6c5404dea75abb9f0ef2
    if let Some(id) = id {
        let of_unit = &binding.plan.units[index];
        let source = &binding.program.workspace.files[of_unit.file].path;
        let local = run
            .request
            .requirements
            .get(source)
            .is_some_and(|requirements| requirements.iter().any(|one| one.id() == id));
        if !local && !is_global_requirement(run.request, id) {
            return Err(format!(
                "{id} is no criterion or test of the unit {} and no global one; elfie_task lists them",
                of_unit.stem
            ));
        }
    }
    let mut pieces: Vec<String> = text.split_inclusive('\n').map(str::to_string).collect();
    // The line break is the one the file's first line ends with, and a line feed when it has
    // none.
    // @lfy def/mcp/main.lfy:mark#mark:mark:f81ece6f1d736c69189687f1b61d0a5d106ce51765776bbd8523caaecef41dea
    let ending = match pieces.first() {
        Some(first) if first.ends_with("\r\n") => "\r\n",
        _ => "\n",
    };
    // @lfy def/mcp/main.lfy:mark#mark:mark:e959da5d59df47ac7933bb07c057234854cf20b78a1ae1296c4d2521057c5f32
    let indent: String = match pieces.get(line.saturating_sub(1)).filter(|_| line <= count) {
        Some(at) => at.chars().take_while(|&character| character == ' ' || character == '\t').collect(),
        None => String::new(),
    };
    let inserted = match id {
        Some(id) => format!("{indent}{} @lfy {declaring}:{name}#{id}", target.marker_comment),
        None => format!("{indent}{} @lfy {declaring}:{name}", target.marker_comment),
    };
    if line > count {
        // @lfy def/mcp/main.lfy:mark#mark:mark:0d151705612442f586374d3ecc64db058c44120fe7a7355be4a84f2aafe6d259
        if let Some(last) = pieces.last_mut()
            && !last.ends_with('\n')
        {
            last.push_str(ending);
        }
        pieces.push(format!("{inserted}{ending}"));
    } else {
        // @lfy def/mcp/main.lfy:mark#mark:mark:f81ece6f1d736c69189687f1b61d0a5d106ce51765776bbd8523caaecef41dea
        pieces.insert(line - 1, format!("{inserted}{ending}"));
    }
    fs::write(&path, pieces.concat()).map_err(|error| format!("{file} cannot be written: {error}"))?;
    let at = line.min(count + 1);
    // @lfy def/mcp/main.lfy:mark#mark:mark:2e402e1e9ad7b4d3ccb7a990b41387cfa8be2150d5feaa6d160ee8adfd91704e
    Ok(format!("{file}:{at} {inserted}"))
}

/// Write a marker line into one of this compiler run's outputs, or rewrite to a new id every
/// marker of its outputs that names an old one, so that no id is typed by hand.
///
/// Nothing but an output file of the batch's target is written: no source map and no review
/// record, which the CLI writes, and no file outside that target's output directory.
// @lfy def/mcp/main.lfy:mark#mark:mark:a7dcf7eb424415c0fcc0ed5a98c1d56456a130be8cb8f194d3f21ec67c1606df
// @lfy def/mcp/main.lfy:mark#mark:mark:fcfecab8991108e6f27911bb4b6d77cfffcf93ddbb785581af7a1021cbe3b53c
pub fn mark(
    session: &Session,
    id: Option<&str>,
    replaces: Option<&str>,
    file: Option<&str>,
    line: Option<usize>,
    entity: Option<&str>,
) -> Result<String, String> {
    let run = own_request(session, "elfie_mark")?;
    let Some(target) = batch_own_target(&session.workspace, &run) else {
        return Err(format!("the batch {} holds no unit", run.request.batch.identifier));
    };
    // The arguments a call gives say what it wants: `replaces` asks for a rename, and `file`,
    // `line`, and `entity` ask for an insertion.
    // @lfy def/mcp/main.lfy:mark#mark:mark:de06da3d9e51338cb41d6032e36580ce6510b4297822e054fb6f234dc68b0594
    if let Some(replaces) = replaces {
        if id.is_none() || file.is_some() || line.is_some() || entity.is_some() {
            return Err(
                "a rename takes replaces and id and nothing else; leave out file, line, and entity".to_string()
            );
        }
        let id = id.unwrap_or_default();
        return rename_markers(session, &run, target, id, replaces);
    }
    // @lfy def/mcp/main.lfy:mark#mark:mark:2959e7495b60ed84e030a118eff9b9a1cb5d699815713975e3e7f53137050e14
    let (Some(file), Some(line), Some(entity)) = (file, line, entity) else {
        return Err("an insertion takes file, line, and entity, and optionally id".to_string());
    };
    insert_marker(session, &run, target, id, file, line, entity)
}

// ---- how a compiler run ended -------------------------------------------------------

// Decision: the compiler's outcome is submitted while it is still working, checked when it
// arrives, and kept in its session for the CLI, so how a run ended never depends on the last
// lines of a final message.

/// The line a compiler's final message ends with for each outcome.
// @lfy def/mcp/main.lfy:submit#submit:submit:f94daa1287cb9c2fd43bbdfe11f8cfe51bf9706696102840706f0445f387669a
const ENDS_WITH: &str = "ELFIE: DONE, ELFIE: BLOCKED: and the reason, or ELFIE: CLARIFY: and the question";

/// Say how this compiler run ended. Nothing is written: the outcome is kept in the session
/// until the CLI closes it.
// @lfy def/mcp/main.lfy:submit#submit:submit:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
pub fn submit(session: &Session, outcome: &str, message: &str) -> Result<String, String> {
    own_request(session, "elfie_submit")?;
    // @lfy def/mcp/main.lfy:submit#submit:submit:28e49a28fbbfa8f85d4d95ecccb6f05ae599e3a6f128b655d73257666a31412e
    let Some(kind) = SubmittedOutcome::from_name(&outcome.to_ascii_lowercase()) else {
        return Err(format!(
            "the outcome {outcome} is none of {}, {}, and {}",
            SubmittedOutcome::Done.as_str(),
            SubmittedOutcome::Blocked.as_str(),
            SubmittedOutcome::Clarify.as_str()
        ));
    };
    // @lfy def/mcp/main.lfy:submit#submit:submit:b99c292c6a953720125d01484c7cc9fc03b9727847086a54737c3363884273cb
    if matches!(kind, SubmittedOutcome::Blocked | SubmittedOutcome::Clarify) && message.trim().is_empty() {
        return Err(format!(
            "{} needs its reason and {} its question",
            SubmittedOutcome::Blocked.as_str(),
            SubmittedOutcome::Clarify.as_str()
        ));
    }
    // @lfy def/mcp/main.lfy:submit#submit:submit:839e3605860269cb5d2a142eae47825edd5f8e18cc86e1a3b9c4f70278331d8d
    let mut recorded = session.submission.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    *recorded = Some(Submission {
        outcome: kind,
        message: message.to_string(),
    });
    // @lfy def/mcp/main.lfy:submit#submit:submit:bedeb2634345b2348c1509d268083160d2e9755e210b5107f4fbc972522749c1
    // @lfy def/mcp/main.lfy:submit#submit:submit:f94daa1287cb9c2fd43bbdfe11f8cfe51bf9706696102840706f0445f387669a
    Ok(format!(
        "recorded the outcome {}; a later call of elfie_submit replaces it\nend your final message with the one line for the outcome: {ENDS_WITH}",
        kind.as_str()
    ))
}

// ---- searching the outputs ----------------------------------------------------------

// Decision: the regular expressions are those of the Rust regex crate, the library the
// compiler itself is written with, so every search ends in time linear in the text.

/// How many matching lines a search gives before the rest are only counted.
// @lfy def/mcp/main.lfy:search#search:search:a1cf2a0988555448b5857efc090387808f6292fa53e678aeff920abc5253a1b5
const SEARCHED_LINES: usize = 200;

/// How many characters of one line are given before the rest are left out.
// @lfy def/mcp/main.lfy:search#search:search:68207e69227f6ee9bb5e6b5a46a769482b26d625a8589e28cdcef599d5b1756f
const KEPT_WIDTH: usize = 500;

/// What marks a line that was cut short.
// @lfy def/mcp/main.lfy:search#search:search:68207e69227f6ee9bb5e6b5a46a769482b26d625a8589e28cdcef599d5b1756f
const CUT: char = '…';

/// A line kept at most `width` Unicode characters, followed by `…` when it was cut short.
// @lfy def/mcp/main.lfy:search#search:search:68207e69227f6ee9bb5e6b5a46a769482b26d625a8589e28cdcef599d5b1756f
fn shortened(line: &str, width: usize) -> String {
    let mut characters = line.chars();
    let kept: String = characters.by_ref().take(width).collect();
    if characters.next().is_some() {
        format!("{kept}{CUT}")
    } else {
        kept
    }
}

/// The files a search reads: every output file of every target it searches, each once, in
/// the order of their paths.
// @lfy def/mcp/main.lfy:search#search:search:ad139fbdabb5d3b024a7e78d751dcf2f3b3868f73d92f6746f13ae3b0356182e
fn searched_files(session: &Session) -> Vec<String> {
    let workspace = &session.workspace;
    // @lfy def/mcp/main.lfy:search#search:search:ad139fbdabb5d3b024a7e78d751dcf2f3b3868f73d92f6746f13ae3b0356182e
    let targets = session_targets(workspace, session.binding.as_ref());
    let mut found: Vec<String> = targets
        .iter()
        .flat_map(|target| output_files(workspace, target))
        .collect();
    found.sort();
    found.dedup();
    found
}

/// The lines of generated code that match a regular expression.
// @lfy def/mcp/main.lfy:search#search:search:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
pub fn search(session: &Session, pattern: &str, file: Option<&str>) -> Result<String, String> {
    let workspace = &session.workspace;
    let mut files = searched_files(session);
    // @lfy def/mcp/main.lfy:search#search:search:6fee06b6e4e8b823d41f3b760f75a448a7c9357ea1409123aaf2b2b62685f6ab
    if let Some(under) = file {
        let kept: Vec<String> = files
            .iter()
            .filter(|path| path.as_str() == under || path.starts_with(&format!("{under}/")))
            .cloned()
            .collect();
        if kept.is_empty() {
            // @lfy def/mcp/main.lfy:search#search:search:53b64d09870e3da2a2a74debb3f5058f60fda381188b78182eb74ba5aa2269f1
            return Err(format!("{under} is not under an output directory of this session's targets"));
        }
        files = kept;
    }
    // @lfy def/mcp/main.lfy:search#search:search:b52b7d681a1ab14d16e4367a42f8fe72e5a2c4577ef0e113b5f4e7a1e458ab91
    let expression = regex::Regex::new(pattern)
        // @lfy def/mcp/main.lfy:search#search:search:3eb91d30c58a23ec68143fc70f48b65844f317e8c7607db0b879df80b296c435
        .map_err(|error| format!("{pattern} is not a regular expression: {error}"))?;
    let searched = files.len();
    let mut found: Vec<String> = Vec::new();
    // The files come in the order of their paths, compared by Unicode code point, and each
    // file's lines in line order.
    // @lfy def/mcp/main.lfy:search#search:search:7841eec414d572a807f6ede13000e8a87220e884b36378adb2feef0da752b8ce
    for path in files {
        // @lfy def/mcp/main.lfy:search#search:search:54828c880e975ee7dc7220ac4d01db7f73b0a478d11a232ff914941398b8ac18
        let Ok(text) = fs::read_to_string(workspace.root.join(&path)) else { continue };
        let markers = markers_now(workspace, &path);
        for (index, line) in text.lines().enumerate() {
            // @lfy def/mcp/main.lfy:search#search:search:713960fcdedc766c95bed050ea8ded3ab14967c01ec4d36ecb5698a162d92fdd
            if !expression.is_match(line) {
                continue;
            }
            let number = index + 1;
            // @lfy def/mcp/main.lfy:search#search:search:fa10d4c06de0e21bae6488a493c889c66e5ea9d22f426fbcf0488df26d15e041
            let region = markers
                .iter()
                .rev()
                .find(|marker| marker.output_line <= number)
                .map(marker_text)
                // @lfy def/mcp/main.lfy:search#search:search:b08361d90ea78257dde207e7d32417d0b3458ac1edf1f2c1061ddfb9fcfaa2cd
                .unwrap_or_else(|| "-".to_string());
            // @lfy def/mcp/main.lfy:search#search:search:8112c25cc568699c5b153fd7ada26062fb8732b7fcaefef35a49e6389cc0a8ff
            found.push(format!("{path}:{number} [{region}]: {}", shortened(line, KEPT_WIDTH)));
        }
    }
    if found.is_empty() {
        // @lfy def/mcp/main.lfy:search#search:search:5fcbe5fc50d6c7ffeea79876a5cf547599a86c0eb99cb0bac1ed79367eee2402
        return Ok(format!("no line of the {searched} files searched matches {pattern}"));
    }
    // @lfy def/mcp/main.lfy:search#search:search:a1cf2a0988555448b5857efc090387808f6292fa53e678aeff920abc5253a1b5
    if found.len() > SEARCHED_LINES {
        let left = found.len() - SEARCHED_LINES;
        found.truncate(SEARCHED_LINES);
        found.push(format!(
            "{left} more matching lines are left out; name a file or narrow the pattern"
        ));
    }
    Ok(found.join("\n"))
}

// ---- the target's own commands ------------------------------------------------------

// Decision: an agent builds, tests, and lints through these tools rather than a shell, so it
// runs only what the target's commands say, in its own run's directory.

/// How long a command may run before it and every process of its group are stopped.
// @lfy def/mcp/main.lfy:build#build:commandTool:1ec8e462d2638913d1d6b8cd5b590f00751d033f0891e5ca220c79cd71a87001
// @lfy def/mcp/main.lfy:test#test:commandTool:1ec8e462d2638913d1d6b8cd5b590f00751d033f0891e5ca220c79cd71a87001
// @lfy def/mcp/main.lfy:lint#lint:commandTool:1ec8e462d2638913d1d6b8cd5b590f00751d033f0891e5ca220c79cd71a87001
// @lfy def/mcp/main.lfy:format#format:commandTool:1ec8e462d2638913d1d6b8cd5b590f00751d033f0891e5ca220c79cd71a87001
const COMMAND_DEADLINE: Duration = Duration::from_secs(30 * 60);

/// What is said of a command that ran out of time.
// @lfy def/mcp/main.lfy:build#build:commandTool:1ec8e462d2638913d1d6b8cd5b590f00751d033f0891e5ca220c79cd71a87001
const STOPPED: &str = "[elfie: stopped after 30 minutes]";

/// How many of the lines a command wrote are kept from the end.
// @lfy def/mcp/main.lfy:build#build:commandTool:b3cf05e26d7d3b687e7b0b4028358dba9a481d54a4bddde0f8e7ab00f9743fb7
// @lfy def/mcp/main.lfy:test#test:commandTool:b3cf05e26d7d3b687e7b0b4028358dba9a481d54a4bddde0f8e7ab00f9743fb7
// @lfy def/mcp/main.lfy:lint#lint:commandTool:b3cf05e26d7d3b687e7b0b4028358dba9a481d54a4bddde0f8e7ab00f9743fb7
// @lfy def/mcp/main.lfy:format#format:commandTool:b3cf05e26d7d3b687e7b0b4028358dba9a481d54a4bddde0f8e7ab00f9743fb7
const KEPT_LINES: usize = 200;

/// How many earlier lines that read as a failure are kept.
// @lfy def/mcp/main.lfy:build#build:commandTool:63be0a2d4357c8c544999776293adaded56a3f4a94b5c8ad25682f08f6ac70a6
// @lfy def/mcp/main.lfy:test#test:commandTool:63be0a2d4357c8c544999776293adaded56a3f4a94b5c8ad25682f08f6ac70a6
// @lfy def/mcp/main.lfy:lint#lint:commandTool:63be0a2d4357c8c544999776293adaded56a3f4a94b5c8ad25682f08f6ac70a6
// @lfy def/mcp/main.lfy:format#format:commandTool:63be0a2d4357c8c544999776293adaded56a3f4a94b5c8ad25682f08f6ac70a6
const KEPT_EARLIER: usize = 100;

/// How often a running command is looked in on while its deadline runs.
// @lfy def/mcp/main.lfy:build#build:commandTool:1ec8e462d2638913d1d6b8cd5b590f00751d033f0891e5ca220c79cd71a87001
const LOOKED_IN_ON: Duration = Duration::from_millis(50);

/// The queue the command tool calls for one directory wait in.
// @lfy def/mcp/main.lfy:build#build:commandTool:55ee18e11aaf994de7de40b0727c7ff09d9b5230a39f4ff4070a8e0b0108478a
struct Queue {
    /// The ticket the next call to arrive takes.
    next: u64,
    /// The ticket whose call may run its command now.
    serving: u64,
}

/// One directory's queue, with what the calls waiting in it wait on.
// @lfy def/mcp/main.lfy:build#build:commandTool:55ee18e11aaf994de7de40b0727c7ff09d9b5230a39f4ff4070a8e0b0108478a
type Waiting = Arc<(Mutex<Queue>, Condvar)>;

/// One queue per directory, made when the first command tool call for it arrives, so calls
/// for different directories never wait for one another.
// @lfy def/mcp/main.lfy:build#build:commandTool:55ee18e11aaf994de7de40b0727c7ff09d9b5230a39f4ff4070a8e0b0108478a
// @lfy def/mcp/main.lfy:host#host:host:b09cfad6442aec8ac4faf8e13385b2973136e4d0ac08ec20419f15750ea59366
static QUEUES: Mutex<BTreeMap<PathBuf, Waiting>> = Mutex::new(BTreeMap::new());

/// One call's place in the queue of a directory: taken when the call is about to run its
/// command, and given up once the command has ended, so the next waiting call runs its own.
// @lfy def/mcp/main.lfy:build#build:commandTool:55ee18e11aaf994de7de40b0727c7ff09d9b5230a39f4ff4070a8e0b0108478a
// @lfy def/mcp/main.lfy:test#test:commandTool:60b75255bc0d143aa66f828431f0a721c6c1c952e97439ee899c364c01652dda
// @lfy def/mcp/main.lfy:lint#lint:commandTool:bf3ae27fedd0e1cd8c26651f0067a25d484714febcf2dc6f9c6e38a17ed832a8
// @lfy def/mcp/main.lfy:format#format:commandTool:a92186f15098490adff4dd2576c7c07d375f1146705adda9596d1e934baa0f2e
struct Turn {
    queue: Waiting,
    ticket: u64,
}

impl Turn {
    /// A place at the back of the queue of a directory, taken at once so that the calls for
    /// one directory run their commands in the order they arrived.
    // @lfy def/mcp/main.lfy:build#build:commandTool:55ee18e11aaf994de7de40b0727c7ff09d9b5230a39f4ff4070a8e0b0108478a
    fn take(directory: &Path) -> Turn {
        let queue = {
            let mut queues = QUEUES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            queues
                .entry(directory.to_path_buf())
                .or_insert_with(|| Arc::new((Mutex::new(Queue { next: 0, serving: 0 }), Condvar::new())))
                .clone()
        };
        let ticket = {
            let (lock, _) = &*queue;
            let mut state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let ticket = state.next;
            state.next += 1;
            ticket
        };
        Turn { queue, ticket }
    }

    /// Waits until every call that took a place earlier has given its up.
    // @lfy def/mcp/main.lfy:build#build:commandTool:55ee18e11aaf994de7de40b0727c7ff09d9b5230a39f4ff4070a8e0b0108478a
    fn wait(&self) {
        let (lock, waiting) = &*self.queue;
        let mut state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        while state.serving != self.ticket {
            state = waiting.wait(state).unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }
}

impl Drop for Turn {
    /// The place is given up however the call ended, so a command that was stopped lets the
    /// next call waiting for the same directory run its own.
    // @lfy def/mcp/main.lfy:build#build:commandTool:1ec8e462d2638913d1d6b8cd5b590f00751d033f0891e5ca220c79cd71a87001
    fn drop(&mut self) {
        let (lock, waiting) = &*self.queue;
        let mut state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.serving <= self.ticket {
            state.serving = self.ticket + 1;
        }
        waiting.notify_all();
    }
}

/// Every line of one stream, appended to the lines the run collected as it arrives, so that
/// the lines of both streams are kept in the order they arrived.
// @lfy def/mcp/main.lfy:build#build:commandTool:b298059f6d35f48a0005d4d32a614a0694c0bc864f1d8f81d9bdb0569150f36d
// @lfy def/mcp/main.lfy:test#test:commandTool:b298059f6d35f48a0005d4d32a614a0694c0bc864f1d8f81d9bdb0569150f36d
// @lfy def/mcp/main.lfy:lint#lint:commandTool:b298059f6d35f48a0005d4d32a614a0694c0bc864f1d8f81d9bdb0569150f36d
// @lfy def/mcp/main.lfy:format#format:commandTool:b298059f6d35f48a0005d4d32a614a0694c0bc864f1d8f81d9bdb0569150f36d
fn collect_lines<R: std::io::Read + Send + 'static>(
    pipe: R,
    lines: Arc<Mutex<Vec<String>>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let reader = std::io::BufReader::new(pipe);
        for line in std::io::BufRead::lines(reader) {
            let Ok(line) = line else { break };
            lines.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(line);
        }
    })
}

/// Every process of a process group stopped, through the platform's own `kill`.
// @lfy def/mcp/main.lfy:build#build:commandTool:1ec8e462d2638913d1d6b8cd5b590f00751d033f0891e5ca220c79cd71a87001
fn stop_group(leader: u32) {
    let _ = Command::new("sh")
        .arg("-c")
        .arg(format!("kill -KILL -- -{leader}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// One run of a command line through the platform shell, as `Process.stream` runs one: with
/// no input, as the leader of a process group of its own, in a directory and with variables
/// added to the inherited environment. Gives its exit code, every line it wrote in the order
/// the lines arrived, and whether it was stopped for running out of time.
// @lfy def/mcp/main.lfy:build#build:commandTool:f83af6a046e8e4f79a777c2d3c9b677a67d05faab6429155fbda5b26e27e4159
// @lfy def/mcp/main.lfy:test#test:commandTool:346fac5fbebd390dcb152e0aa4764ba33a753ebf59454184387238b8bc53658e
// @lfy def/mcp/main.lfy:lint#lint:commandTool:bf3147cfd99d2a9b4e23f7bc279599b377502118fe79c0f3e4b61046b9b04474
// @lfy def/mcp/main.lfy:format#format:commandTool:233a6d05023c94898e39605cf19939d406164ae5ee43f059c1a31f44b94fa768
fn run_line(line: &str, directory: &Path, environment: &[(&str, String)]) -> (i32, Vec<String>, bool) {
    let mut spawning = Command::new("sh");
    spawning
        .arg("-c")
        .arg(line)
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in environment {
        spawning.env(name, value);
    }
    // The command leads a process group of its own, so everything it starts is stopped with
    // it when it runs out of time.
    // @lfy def/mcp/main.lfy:build#build:commandTool:f83af6a046e8e4f79a777c2d3c9b677a67d05faab6429155fbda5b26e27e4159
    std::os::unix::process::CommandExt::process_group(&mut spawning, 0);
    let mut child = match spawning.spawn() {
        Ok(child) => child,
        // The command could not be started, so its code is -1 and what went wrong is all it
        // wrote.
        // @lfy def/mcp/main.lfy:build#build:commandTool:af9a234fc63d3a56cd02fc43d481612e9c05d96d41642a52c35b8aa861c9ebd9
        Err(error) => return (-1, vec![error.to_string()], false),
    };
    let leader = child.id();
    let lines: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let out = child.stdout.take().map(|pipe| collect_lines(pipe, lines.clone()));
    let err = child.stderr.take().map(|pipe| collect_lines(pipe, lines.clone()));
    let started = Instant::now();
    let mut stopped = false;
    // The call returns only once the command has ended.
    // @lfy def/mcp/main.lfy:build#build:commandTool:e670d0aa01ae232b92df464a22a88088c64eb6c357dd204db95e9e5c6addeee1
    // @lfy def/mcp/main.lfy:test#test:commandTool:b62ccd3754950987963891430f09246c1dd9a2c833b841dad32ba94659a95928
    // @lfy def/mcp/main.lfy:lint#lint:commandTool:4186bc93dba70085787c21fda7d5bf8d944d5ac95e0b1018257303823b5a4991
    // @lfy def/mcp/main.lfy:format#format:commandTool:5e3829548d3f5d7d338494c802ee554c15f3cc452204dee325e25a77bdaedfce
    let code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code().unwrap_or(-1),
            Ok(None) => {}
            Err(_) => break -1,
        }
        // @lfy def/mcp/main.lfy:build#build:commandTool:1ec8e462d2638913d1d6b8cd5b590f00751d033f0891e5ca220c79cd71a87001
        if started.elapsed() >= COMMAND_DEADLINE {
            stopped = true;
            stop_group(leader);
            let _ = child.kill();
            let _ = child.wait();
            break -1;
        }
        std::thread::sleep(LOOKED_IN_ON);
    };
    if let Some(out) = out {
        let _ = out.join();
    }
    if let Some(err) = err {
        let _ = err.join();
    }
    let written = lines.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
    (code, written, stopped)
}

/// Whether a line a command wrote reads as a failure: it names an error or a warning,
/// whatever its case.
// @lfy def/mcp/main.lfy:build#build:commandTool:63be0a2d4357c8c544999776293adaded56a3f4a94b5c8ad25682f08f6ac70a6
fn reads_as_failure(line: &str) -> bool {
    let lowered = line.to_lowercase();
    lowered.contains("error") || lowered.contains("warning")
}

/// What a command wrote, kept short, and how many of its lines that leaves out.
///
/// Every line is ended by a newline and kept at most 500 characters; a line elfie adds is
/// never shortened.
// @lfy def/mcp/main.lfy:build#build:commandTool:061a911a3cd22a9aa176a791d4d9f3a03405f6c4d5a5bad8f44fd652e56f6477
// @lfy def/mcp/main.lfy:build#build:commandTool:f6efadbb442d048cef8053d19a026c18b7dbffde5a2b0015a6047743150dc9f3
// @lfy def/mcp/main.lfy:build#build:commandTool:0078fc9e2f6e0d5f31f5e4ca2228f246ffd00ecd0927004d85db9ceed18ed01b
// @lfy def/mcp/main.lfy:build#build:commandTool:a9b744b3a270971d0bd9ce3b4335190df2715b2f07ae02df1b2548deca9fe114
// @lfy def/mcp/main.lfy:test#test:commandTool:061a911a3cd22a9aa176a791d4d9f3a03405f6c4d5a5bad8f44fd652e56f6477
// @lfy def/mcp/main.lfy:test#test:commandTool:f6efadbb442d048cef8053d19a026c18b7dbffde5a2b0015a6047743150dc9f3
// @lfy def/mcp/main.lfy:test#test:commandTool:0078fc9e2f6e0d5f31f5e4ca2228f246ffd00ecd0927004d85db9ceed18ed01b
// @lfy def/mcp/main.lfy:test#test:commandTool:a9b744b3a270971d0bd9ce3b4335190df2715b2f07ae02df1b2548deca9fe114
// @lfy def/mcp/main.lfy:lint#lint:commandTool:061a911a3cd22a9aa176a791d4d9f3a03405f6c4d5a5bad8f44fd652e56f6477
// @lfy def/mcp/main.lfy:lint#lint:commandTool:f6efadbb442d048cef8053d19a026c18b7dbffde5a2b0015a6047743150dc9f3
// @lfy def/mcp/main.lfy:lint#lint:commandTool:0078fc9e2f6e0d5f31f5e4ca2228f246ffd00ecd0927004d85db9ceed18ed01b
// @lfy def/mcp/main.lfy:lint#lint:commandTool:a9b744b3a270971d0bd9ce3b4335190df2715b2f07ae02df1b2548deca9fe114
// @lfy def/mcp/main.lfy:format#format:commandTool:061a911a3cd22a9aa176a791d4d9f3a03405f6c4d5a5bad8f44fd652e56f6477
// @lfy def/mcp/main.lfy:format#format:commandTool:f6efadbb442d048cef8053d19a026c18b7dbffde5a2b0015a6047743150dc9f3
// @lfy def/mcp/main.lfy:format#format:commandTool:0078fc9e2f6e0d5f31f5e4ca2228f246ffd00ecd0927004d85db9ceed18ed01b
// @lfy def/mcp/main.lfy:format#format:commandTool:a9b744b3a270971d0bd9ce3b4335190df2715b2f07ae02df1b2548deca9fe114
fn kept_output(lines: &[String], stopped: bool) -> (String, usize) {
    let mut out = String::new();
    let mut omitted = 0;
    if lines.len() <= KEPT_LINES {
        // @lfy def/mcp/main.lfy:build#build:commandTool:b3cf05e26d7d3b687e7b0b4028358dba9a481d54a4bddde0f8e7ab00f9743fb7
        for line in lines {
            out.push_str(&shortened(line, KEPT_WIDTH));
            out.push('\n');
        }
    } else {
        let split = lines.len() - KEPT_LINES;
        let earlier = &lines[..split];
        // The earlier lines kept are the first hundred that read as a failure, or that come
        // directly after one.
        // @lfy def/mcp/main.lfy:build#build:commandTool:63be0a2d4357c8c544999776293adaded56a3f4a94b5c8ad25682f08f6ac70a6
        let mut keep: Vec<&String> = Vec::new();
        for (index, line) in earlier.iter().enumerate() {
            let after = index > 0 && reads_as_failure(&earlier[index - 1]);
            if reads_as_failure(line) || after {
                keep.push(line);
            }
            if keep.len() == KEPT_EARLIER {
                break;
            }
        }
        // @lfy def/mcp/main.lfy:build#build:commandTool:7a7e7f217024616471823763ae368e16d259097c0caecb49b0e9e3b404db0ca5
        // @lfy def/mcp/main.lfy:test#test:commandTool:7a7e7f217024616471823763ae368e16d259097c0caecb49b0e9e3b404db0ca5
        // @lfy def/mcp/main.lfy:lint#lint:commandTool:7a7e7f217024616471823763ae368e16d259097c0caecb49b0e9e3b404db0ca5
        // @lfy def/mcp/main.lfy:format#format:commandTool:7a7e7f217024616471823763ae368e16d259097c0caecb49b0e9e3b404db0ca5
        omitted = earlier.len() - keep.len();
        // @lfy def/mcp/main.lfy:build#build:commandTool:5cb971efd556449109eeb23f4d811571b6874a2274b251355982e35590d77441
        // @lfy def/mcp/main.lfy:test#test:commandTool:5cb971efd556449109eeb23f4d811571b6874a2274b251355982e35590d77441
        // @lfy def/mcp/main.lfy:lint#lint:commandTool:5cb971efd556449109eeb23f4d811571b6874a2274b251355982e35590d77441
        // @lfy def/mcp/main.lfy:format#format:commandTool:5cb971efd556449109eeb23f4d811571b6874a2274b251355982e35590d77441
        for line in keep {
            out.push_str(&shortened(line, KEPT_WIDTH));
            out.push('\n');
        }
        out.push_str(&format!("[elfie: {omitted} lines omitted]\n"));
        for line in &lines[split..] {
            out.push_str(&shortened(line, KEPT_WIDTH));
            out.push('\n');
        }
    }
    // @lfy def/mcp/main.lfy:build#build:commandTool:1ec8e462d2638913d1d6b8cd5b590f00751d033f0891e5ca220c79cd71a87001
    if stopped {
        out.push_str(STOPPED);
        out.push('\n');
    }
    (out, omitted)
}

/// The target one command tool call runs for: the one it names, the target of the session's
/// batch, or the only target of the workspace.
// @lfy def/mcp/main.lfy:build#build:commandTool:8ffd4f1c8964d044d3a2ad994cc592a42773205e5ab265fbba1c238e40c7390c
// @lfy def/mcp/main.lfy:build#build:commandTool:c21f972796c2663341ed914a6967afaa5dc6c0c14d8be6175383fa74deb6c3ed
// @lfy def/mcp/main.lfy:build#build:commandTool:e3805c0183b778c19e58e0e6579a16be881fa3c9595b5ff020f4ac5a70215cba
// @lfy def/mcp/main.lfy:test#test:commandTool:97d5a1312ee06fbe77300ae7c3ff0cf0af8ad0c311a6c611aa68fd8dd2197c28
// @lfy def/mcp/main.lfy:test#test:commandTool:aa5521ff3e38064799700843c6061daeefbbdc2d8b5e6b48a6f0a8faaafa2715
// @lfy def/mcp/main.lfy:test#test:commandTool:6aec0f3e99d25e6c4389be9d948c0c0c38fe5bced0fe17f7717b9ba2cef6285e
// @lfy def/mcp/main.lfy:lint#lint:commandTool:ffbdf33de2469869254a1bf05492bbc865db0dfbfbcf43c1e671c9887c5570d7
// @lfy def/mcp/main.lfy:lint#lint:commandTool:e91ad0846a3c0c777006bcdf1f40ae6a179e1a658044b4c17b6f6d3a8f077f14
// @lfy def/mcp/main.lfy:lint#lint:commandTool:d18e32d682bdbd5b0e1b52c44502c73935fb119538674429ce081b3443fb3ecd
// @lfy def/mcp/main.lfy:format#format:commandTool:9bf67e413eeeba868ece3fee8e14cb7eb5b7c31cf135e37f9a546cc5ea19bdfb
// @lfy def/mcp/main.lfy:format#format:commandTool:f5203bdc3e4001878911f4a29dc48c15802584bafd3fca37fd7a1c1c0939fef8
// @lfy def/mcp/main.lfy:format#format:commandTool:5a47bf0b2df9f3577fd4df46ba42f66a69be217b3e4551b35db11b5d765ea5af
fn command_target<'w>(
    workspace: &'w Workspace,
    binding: Option<&Binding>,
    target: Option<&str>,
) -> Result<&'w Target, String> {
    let own: Option<&Target> = binding
        .filter(|binding| binding.batch.is_some())
        .map(|binding| session_targets(workspace, Some(binding)))
        .and_then(|targets| targets.first().copied());
    match target {
        // @lfy def/mcp/main.lfy:build#build:commandTool:8ffd4f1c8964d044d3a2ad994cc592a42773205e5ab265fbba1c238e40c7390c
        Some(named) => {
            let Some(found) = workspace.targets.iter().find(|known| known.identifier == named) else {
                // @lfy def/mcp/main.lfy:build#build:commandTool:1f6aa9de92f8ca8b182ac8744b6bb86bfe4ef0e4fbe7aa6089dd3115896b773a
                // @lfy def/mcp/main.lfy:test#test:commandTool:93463f33b03a97ed01735064bd638bc594f1c1c7def4bf02737f4e8b5d05aa0e
                // @lfy def/mcp/main.lfy:lint#lint:commandTool:659639135a023a322a6102b6c467bff02e095114c72ecb2e452b5298f43d7e1e
                // @lfy def/mcp/main.lfy:format#format:commandTool:14c995f5b52b0b846e864af1eca37984adfdb800a11e8f871accd853273aec8a
                return Err(format!("{named} is not a target; the targets are: {}", known_targets(workspace)));
            };
            // @lfy def/mcp/main.lfy:build#build:commandTool:08ba5d263a364d377e0723c19fb633e9a30b76f64808daade4b50853607b05b3
            // @lfy def/mcp/main.lfy:test#test:commandTool:0c0559ca22d532a752da696a356628c24841fed67d840d879c3978ca6963c083
            // @lfy def/mcp/main.lfy:lint#lint:commandTool:eaea8d053be8130144b47484848cf310cfcfdfcf7b1ddd7c157703d7b81c6594
            // @lfy def/mcp/main.lfy:format#format:commandTool:c5ba8e321898943a2ea8ff001ad5e8982379e4db519f1bf779a9fec569c6d5fc
            if let Some(own) = own
                && own.identifier != found.identifier
            {
                return Err(format!(
                    "this session acts only on its own batch, whose target is {}; it was asked about {named}",
                    own.identifier
                ));
            }
            Ok(found)
        }
        // @lfy def/mcp/main.lfy:build#build:commandTool:c21f972796c2663341ed914a6967afaa5dc6c0c14d8be6175383fa74deb6c3ed
        None => match own {
            Some(own) => Ok(own),
            // @lfy def/mcp/main.lfy:build#build:commandTool:e3805c0183b778c19e58e0e6579a16be881fa3c9595b5ff020f4ac5a70215cba
            None => match workspace.targets.as_slice() {
                [one] => Ok(one),
                // @lfy def/mcp/main.lfy:build#build:commandTool:810411b30bd83781afdd188ef533b950e9275e621df179dd6067d0ac813fd5bc
                // @lfy def/mcp/main.lfy:test#test:commandTool:60a49be5aa1173056f510de843ef341a91124c5a2cf6551b89842dbea64d54b7
                // @lfy def/mcp/main.lfy:lint#lint:commandTool:de2031ae9fd1eaef06991febdb3b3084983787f7ccebc9878c487a056a3244eb
                // @lfy def/mcp/main.lfy:format#format:commandTool:62c1b90ed20ccf37dccd4788e9e1f8d5b7990bce260fb9e7cc7f33b62040959f
                _ => Err(format!("name the target; the targets are: {}", known_targets(workspace))),
            },
        },
    }
}

/// One run of a target's command for one operation, in this run's directory, with the
/// variables the tool adds to the environment, as the JSON of one [`CommandRun`].
// @lfy def/mcp/main.lfy:build#build:commandTool:394aef9de6082d12b6f9d58c8eaae738d8ab0f6aef3341b5fda12620fe6500a6
// @lfy def/mcp/main.lfy:test#test:commandTool:394aef9de6082d12b6f9d58c8eaae738d8ab0f6aef3341b5fda12620fe6500a6
// @lfy def/mcp/main.lfy:lint#lint:commandTool:394aef9de6082d12b6f9d58c8eaae738d8ab0f6aef3341b5fda12620fe6500a6
// @lfy def/mcp/main.lfy:format#format:commandTool:394aef9de6082d12b6f9d58c8eaae738d8ab0f6aef3341b5fda12620fe6500a6
fn command_tool(
    session: &Session,
    operation: Operation,
    target: Option<&str>,
    added: &[(&str, String)],
) -> Result<String, String> {
    let workspace = &session.workspace;
    let chosen = command_target(workspace, session.binding.as_ref(), target)?;
    // @lfy def/mcp/main.lfy:build#build:commandTool:654d31e36e3f112d06aa532374f30fda36190672b3a323580052a1c0f96e1401
    // @lfy def/mcp/main.lfy:test#test:commandTool:82d963f45b5a888916a03e865844e4c1961de319774b2584f65489dee966cdb5
    // @lfy def/mcp/main.lfy:lint#lint:commandTool:db94076681d45420de07709b25f27480d746751609aa1bb42a77c77d453001eb
    // @lfy def/mcp/main.lfy:format#format:commandTool:a09c5e43d4c38d2e3196576c80db3cf1cb918134842760d0c7e7aa134b552a6b
    let Some(line) = line_for(chosen, operation) else {
        return Err(format!(
            "the target {} has no command for the operation {}",
            chosen.identifier,
            operation.value()
        ));
    };
    // The working directory is the target's output directory under the run's directory,
    // which is the root the session's workspace was read at.
    // @lfy def/mcp/main.lfy:build#build:commandTool:9b8ea5c13487cc9695be55b65dee63a54d131b4325fed5195ed378368f8479de
    // @lfy def/mcp/main.lfy:test#test:commandTool:e58bd6defa0f46ad0a8d81c94e1ae84b4e2855bec746a6994866e0ff8c9d9c7e
    // @lfy def/mcp/main.lfy:lint#lint:commandTool:c12543e1f7804b80d93c2e3a72ad1c0f31f5a3476af5b1020f542a059f503a62
    // @lfy def/mcp/main.lfy:format#format:commandTool:240c4259c10da63d0e1f07bafcce450205c9ba0a7081bd677453fbeb360365af
    let run_in = match &session.binding {
        Some(binding) => binding.directory.clone(),
        None => workspace.root.clone(),
    };
    let directory = run_in.join(&chosen.output_directory);
    // @lfy def/mcp/main.lfy:build#build:commandTool:12878393e582a75489c9dea1e06ef0237a6e04ef7784802355edb6ae22e9815b
    // @lfy def/mcp/main.lfy:test#test:commandTool:21a498e9a25b4035a903bda858c68a84657b202a3d2652acc933aebb87553545
    // @lfy def/mcp/main.lfy:lint#lint:commandTool:291e397fdd96e2c05d56276ccd9eaa59d0626f2e906450925651ade003581d56
    // @lfy def/mcp/main.lfy:format#format:commandTool:79dfc6d649cbbac7251754b75682227355922d17ddda51111f462c582c2dff29
    let mut environment: Vec<(&str, String)> =
        vec![("RUN_SCRIPT", chosen.script_runner.clone().unwrap_or_default())];
    environment.extend(added.iter().map(|(name, value)| (*name, value.clone())));
    // The calls for one directory run their commands one after the other, in the order they
    // arrived; a call for another directory waits for none of them.
    // @lfy def/mcp/main.lfy:build#build:commandTool:55ee18e11aaf994de7de40b0727c7ff09d9b5230a39f4ff4070a8e0b0108478a
    // @lfy def/mcp/main.lfy:test#test:commandTool:60b75255bc0d143aa66f828431f0a721c6c1c952e97439ee899c364c01652dda
    // @lfy def/mcp/main.lfy:lint#lint:commandTool:bf3ae27fedd0e1cd8c26651f0067a25d484714febcf2dc6f9c6e38a17ed832a8
    // @lfy def/mcp/main.lfy:format#format:commandTool:a92186f15098490adff4dd2576c7c07d375f1146705adda9596d1e934baa0f2e
    let turn = Turn::take(&run_in);
    turn.wait();
    let (code, written, stopped) = run_line(line, &directory, &environment);
    drop(turn);
    let (output, omitted) = kept_output(&written, stopped);
    // @lfy def/mcp/main.lfy:build#build:commandTool:af9a234fc63d3a56cd02fc43d481612e9c05d96d41642a52c35b8aa861c9ebd9
    // @lfy def/mcp/main.lfy:test#test:commandTool:af9a234fc63d3a56cd02fc43d481612e9c05d96d41642a52c35b8aa861c9ebd9
    // @lfy def/mcp/main.lfy:lint#lint:commandTool:af9a234fc63d3a56cd02fc43d481612e9c05d96d41642a52c35b8aa861c9ebd9
    // @lfy def/mcp/main.lfy:format#format:commandTool:af9a234fc63d3a56cd02fc43d481612e9c05d96d41642a52c35b8aa861c9ebd9
    let run = CommandRun {
        target: chosen.identifier.clone(),
        operation: operation_name(operation).to_string(),
        line: line.to_string(),
        succeeded: code == 0,
        exit_code: code,
        output,
        omitted,
    };
    // The result is never an error, whatever the command's exit code: the run is what the
    // call answers with, and a failing command is a run that failed.
    // @lfy def/mcp/main.lfy:build#build:commandTool:2885c8e60a2e0374353fff71832add7b2dedc797a8f0f0bd2dacfc23d1f4d9fe
    // @lfy def/mcp/main.lfy:test#test:commandTool:2885c8e60a2e0374353fff71832add7b2dedc797a8f0f0bd2dacfc23d1f4d9fe
    // @lfy def/mcp/main.lfy:lint#lint:commandTool:2885c8e60a2e0374353fff71832add7b2dedc797a8f0f0bd2dacfc23d1f4d9fe
    // @lfy def/mcp/main.lfy:format#format:commandTool:2885c8e60a2e0374353fff71832add7b2dedc797a8f0f0bd2dacfc23d1f4d9fe
    Ok(serde_json::to_string(&run.to_json()).unwrap_or_default())
}

/// Run the target's build command in this run's directory, narrowed to a scope when one is
/// given.
// @lfy def/mcp/main.lfy:build#build:build:caecccd1cd176ce8fc2634b1001e015b48012d66788d863fb994735ca793fab5
pub fn build(session: &Session, scope: Option<&str>, target: Option<&str>) -> Result<String, String> {
    // @lfy def/mcp/main.lfy:build#build:build:caecccd1cd176ce8fc2634b1001e015b48012d66788d863fb994735ca793fab5
    command_tool(
        session,
        Operation::Build,
        target,
        &[("SCOPE", scope.unwrap_or_default().to_string())],
    )
}

/// Run the target's test command in this run's directory, narrowed by a filter and a scope
/// when they are given.
// @lfy def/mcp/main.lfy:test#test:test:c8bf810d04779ce26830e5c43f290fd753346658b3b40ff992ab1cdedef32174
// @lfy def/mcp/main.lfy:test#test:test:c7fb9e74d7bb66ee2c63c58681b14d00dc8236468fec85e845bd8aed14276ae6
pub fn test(
    session: &Session,
    filter: Option<&str>,
    scope: Option<&str>,
    target: Option<&str>,
) -> Result<String, String> {
    // @lfy def/mcp/main.lfy:test#test:test:c8bf810d04779ce26830e5c43f290fd753346658b3b40ff992ab1cdedef32174
    // @lfy def/mcp/main.lfy:test#test:test:c7fb9e74d7bb66ee2c63c58681b14d00dc8236468fec85e845bd8aed14276ae6
    // @lfy def/mcp/main.lfy:test#test:test:caecccd1cd176ce8fc2634b1001e015b48012d66788d863fb994735ca793fab5
    command_tool(
        session,
        Operation::Test,
        target,
        &[
            ("TESTS", filter.unwrap_or_default().to_string()),
            ("SCOPE", scope.unwrap_or_default().to_string()),
        ],
    )
}

/// Run the target's lint command in this run's directory, narrowed to a scope when one is
/// given.
// @lfy def/mcp/main.lfy:lint#lint:lint:caecccd1cd176ce8fc2634b1001e015b48012d66788d863fb994735ca793fab5
pub fn lint(session: &Session, scope: Option<&str>, target: Option<&str>) -> Result<String, String> {
    // @lfy def/mcp/main.lfy:lint#lint:lint:caecccd1cd176ce8fc2634b1001e015b48012d66788d863fb994735ca793fab5
    command_tool(
        session,
        Operation::Lint,
        target,
        &[("SCOPE", scope.unwrap_or_default().to_string())],
    )
}

/// Run the target's format check in this run's directory.
// @lfy def/mcp/main.lfy:format
pub fn format_check(session: &Session, target: Option<&str>) -> Result<String, String> {
    command_tool(session, Operation::Format, target, &[])
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use elfie_core::generation::{Review, ReviewStatus};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};

    use super::*;

    /// The directory of the library a fixture carries, relative to its root.
    const LIBRARY_ROOT: &str = "lib";
    /// The main file of the library a fixture carries: the prelude, which brings the
    /// vocabulary a target is declared with into the scope of every file of the project.
    const LIBRARY: &str = "use \"./prelude/builtin\";\nuse \"./prelude/entity\";\n\
                           use \"./prelude/trait\";\nuse \"./criteria/main\";\n\
                           use \"./target/main\";\n";
    /// The trait a builtin carries, which the library's own declarations need.
    const LIBRARY_BUILTIN: &str = "trait builtin: `Performed by the compiler` { }\n";
    /// The context layer every entity is seen through: the lists a body adds to live
    /// there, and `apply` and `layer` are members of a trait's kind rather than names
    /// nothing declares.
    const LIBRARY_ENTITY: &str = "\
d Entity: `A declared thing seen through its context layer` {
  $knowledge: `What the compiler is given to read for it` = object[];
  $commands: `Shell commands for its operations` = object[];
  $targets: `The targets it is built for` = object[];
}
d Trait extends Entity: `A trait seen through its context layer` {
  $apply: `Applies the trait to a target and returns the trait` = (target: Entity) => Trait;
  $layer: `The trait as a layer of a target, with arguments for its parameters` = (...arguments: string[]) => object;
}
";
    /// `layer`, which chooses a trait as a layer with arguments.
    const LIBRARY_TRAIT: &str = "use \"./builtin\";\n\
                                 fn layer(subject: trait, ...arguments: string[]) is builtin: \
                                 `A trait chosen as a layer of a target` => object;\n";
    /// The enums a knowledge item and a command name.
    const LIBRARY_CRITERIA: &str = "\
enum KnowledgeKind: `What a piece of knowledge is` {
  reference = `documentation to read`,
  example = `working code to imitate`,
  definition = `Elfie source that defines it`,
  tool = `a tool of the agent server`,
}
enum Operation: `What a command is for` {
  install = `install`,
  add = `add`,
  build = `build`,
  test = `test`,
  lint = `lint`,
  format = `format`,
  run = `run`,
}
";
    /// `Target`, the roles its slots take, and the function that gives a record written
    /// in braces its type, as the package `elfie` declares them.
    const LIBRARY_TARGET: &str = "\
use \"../prelude/builtin\";
trait target: `What every layer of a target is` {
  $markerComment: `How a line comment begins` = string;
  $provides: `Capabilities this layer gives its target` = string[];
  $requires: `Capabilities some layer of its target must provide` = string[];
  $outputExtensions: `Extensions besides the language's own` = string[];
  .provides = [];
  .requires = [];
  .outputExtensions = [];
}
trait targetLanguage extends target: `What its code is written in` {
  $fileExtension: `The extension of its files` = string;
}
trait targetRuntime extends target: `What runs its code` { }
trait targetPlatform extends target: `Where it runs` { }
trait targetEcosystem extends target: `Where its dependencies come from` {
  $ecosystem: `What that ecosystem is called` = string;
  $scriptRunner: `What runs a script its manifest names` = string;
}
trait targetFramework extends target: `What its code is built on` { }
trait targetInterface extends target: `What it offers` { }
trait targetLayout extends target: `Where its files go` { }
d Target is builtin: `One artifact the project is built into` {
  $output: `Where everything it writes goes` = string;
  $language: `What its code is written in` = trait;
  $runtime: `What runs its code` = trait | undefined;
  $platforms: `Where it runs` = trait[] | undefined;
  $ecosystem: `Where its dependencies come from` = trait | undefined;
  $frameworks: `What its code is built on` = trait[] | undefined;
  $interfaces: `What it offers` = trait[] | undefined;
  $layout: `Where its files go` = trait;
  $layers: `Anything else the compiler is told` = trait[] | undefined;
  $dependencies: `What its generated code needs` = object[] | undefined;
}
function targetOf(target: Target): `Gives a record written in braces its type` -> Target {
  return target;
}
";
    /// The language, the layout, and the ecosystem the fixture's target is built from.
    const ROLES: &str = "\
trait lang extends targetLanguage: `Rust` {
  .fileExtension = \"rs\";
  .markerComment = \"//\";
}
trait flat extends targetLayout: `One file per unit` { }
trait cargo extends targetEcosystem: `Crates from crates.io` {
  .ecosystem = \"cargo\";
}
";
    /// The `ace const` holding the target `rust`, whose output directory is `out`, added
    /// to `global` so that every entity of every project file is built for it.
    const TARGETS: &str = "\
use \"elfie/target\";
use \"./roles\";

ace const rust = targetOf({
  output = \"out\",
  language = lang,
  layout = flat,
  ecosystem = cargo,
});
global@targets.add(rust);
";
    /// The manifest of a fixture: it names the library the fixture carries.
    const LIBRARY_MANIFEST: &str = r#"{ "lib": "lib" }"#;

    /// A project directory under the system's temporary directory, removed when dropped.
    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Fixture {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!("elfie-mcp-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("def")).unwrap();
            let fixture = Fixture { root };
            // Every fixture carries a library of its own and names it in its manifest, so
            // that a test never reads the copy of the library the compiler was built
            // with, whose files would be files of the program like any other.
            fixture.write(MANIFEST, LIBRARY_MANIFEST).library();
            fixture
        }

        /// The library of the package `elfie`, written under [`LIBRARY_ROOT`]: the
        /// prelude, the kind data a body's lists live on, and the target vocabulary a
        /// project's `ace const` declares its targets with.
        fn library(&self) -> &Fixture {
            self.write(&format!("{LIBRARY_ROOT}/main.lfy"), LIBRARY)
                .write(&format!("{LIBRARY_ROOT}/prelude/builtin.lfy"), LIBRARY_BUILTIN)
                .write(&format!("{LIBRARY_ROOT}/prelude/entity.lfy"), LIBRARY_ENTITY)
                .write(&format!("{LIBRARY_ROOT}/prelude/trait.lfy"), LIBRARY_TRAIT)
                .write(&format!("{LIBRARY_ROOT}/criteria/main.lfy"), LIBRARY_CRITERIA)
                .write(&format!("{LIBRARY_ROOT}/target/main.lfy"), LIBRARY_TARGET)
        }

        /// The same project with one target `rust` whose output directory is `out`,
        /// declared as an `ace const` of the project and added to `global`, so that every
        /// entity of every project file is built for it.
        fn with_rust_target(&self) -> &Fixture {
            self.write("def/roles.lfy", ROLES).write("def/targets.lfy", TARGETS)
        }

        fn write(&self, path: &str, text: &str) -> &Fixture {
            let disk = self.root.join(path);
            fs::create_dir_all(disk.parent().unwrap()).unwrap();
            fs::write(disk, text).unwrap();
            self
        }

        fn remove(&self, path: &str) -> &Fixture {
            fs::remove_file(self.root.join(path)).unwrap();
            self
        }

        fn session(&self) -> Session {
            Session::load(&self.root)
        }

        /// Git run in the fixture's root, so that a test can make the root a repository and
        /// commit the source a unit's outputs are then accepted against.
        // @lfy def/mcp/main.lfy:changes#changes:changes:6fdf72883c3fb05ab2a58a8325708fe690fc56fcfe75d909abff5f56efdb18e1
        fn git(&self, arguments: &[&str]) -> &Fixture {
            let status = Command::new("git")
                .args(arguments)
                .current_dir(&self.root)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect("git");
            assert!(status.success(), "git {arguments:?}");
            self
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// A call through the dispatch table, as the protocol would make it.
    fn call(session: &Session, name: &str, arguments: serde_json::Value) -> ToolResult {
        let tools = tools();
        let registered = traits::find(&tools, name).unwrap_or_else(|| panic!("no tool named {name}"));
        call_tool(registered, session, arguments.as_object().unwrap())
    }

    /// The name of every tool a session over standard input and output is offered, in the
    /// order the definition declares the fns that carry `tool`: every one carrying `tool`
    /// alone but `elfie_report`, which only a verifier's hosted session is offered, and
    /// none carrying `compilerTool` or `commandTool`, which need a binding.
    const TOOL_NAMES: [&str; 15] = [
        "elfie_problems",
        "elfie_find",
        "elfie_entity",
        "elfie_references",
        "elfie_outline",
        "elfie_grammar",
        "elfie_units",
        "elfie_request",
        "elfie_check",
        "elfie_output",
        "elfie_search",
        "elfie_source",
        "elfie_changes",
        "elfie_review",
        "elfie_globalReview",
    ];

    /// The name of every tool a compiler's hosted session is offered when its target has no
    /// command: every fn carrying `tool` and not `commandTool` but `report`.
    const COMPILER_TOOLS: [&str; 19] = [
        "elfie_problems",
        "elfie_find",
        "elfie_entity",
        "elfie_references",
        "elfie_outline",
        "elfie_grammar",
        "elfie_units",
        "elfie_request",
        "elfie_task",
        "elfie_dependency",
        "elfie_check",
        "elfie_mark",
        "elfie_submit",
        "elfie_output",
        "elfie_search",
        "elfie_source",
        "elfie_changes",
        "elfie_review",
        "elfie_globalReview",
    ];

    /// The name of every tool a verifier's hosted session is offered when its target has no
    /// command: every fn carrying `tool` alone but `check`, `report` among them.
    const VERIFIER_TOOLS: [&str; 15] = [
        "elfie_problems",
        "elfie_find",
        "elfie_entity",
        "elfie_references",
        "elfie_outline",
        "elfie_grammar",
        "elfie_units",
        "elfie_request",
        "elfie_output",
        "elfie_search",
        "elfie_source",
        "elfie_changes",
        "elfie_review",
        "elfie_globalReview",
        "elfie_report",
    ];

    /// The name of every fn of this module that carries `tool`, in declaration order.
    const EVERY_TOOL: [&str; 24] = [
        "elfie_problems",
        "elfie_find",
        "elfie_entity",
        "elfie_references",
        "elfie_outline",
        "elfie_grammar",
        "elfie_units",
        "elfie_request",
        "elfie_task",
        "elfie_dependency",
        "elfie_check",
        "elfie_mark",
        "elfie_submit",
        "elfie_output",
        "elfie_search",
        "elfie_source",
        "elfie_changes",
        "elfie_review",
        "elfie_globalReview",
        "elfie_report",
        "elfie_build",
        "elfie_test",
        "elfie_lint",
        "elfie_format",
    ];

    /// An agent connected to a server over in-memory streams, speaking the protocol's
    /// newline-delimited JSON-RPC: the whole server runs, and closing the input it reads
    /// is standard input closing.
    struct Agent {
        /// The stream the server reads as its standard input; dropped to close it.
        input: Option<DuplexStream>,
        output: BufReader<DuplexStream>,
        served: tokio::task::JoinHandle<i32>,
        next_id: i64,
    }

    impl Agent {
        fn connect(root: &Path) -> Agent {
            let (input, server_input) = tokio::io::duplex(1 << 20);
            let (server_output, output) = tokio::io::duplex(1 << 20);
            let served = tokio::spawn(serve_on(server_input, server_output, root.to_path_buf()));
            Agent {
                input: Some(input),
                output: BufReader::new(output),
                served,
                next_id: 0,
            }
        }

        async fn send(&mut self, message: Value) {
            let line = format!("{message}\n");
            let input = self.input.as_mut().expect("an open input");
            input.write_all(line.as_bytes()).await.unwrap();
            input.flush().await.unwrap();
        }

        /// One line the server wrote to the stream it takes for standard output.
        async fn read(&mut self) -> Value {
            let mut line = String::new();
            let read = self.output.read_line(&mut line).await.unwrap();
            assert_ne!(read, 0, "the server wrote nothing");
            serde_json::from_str(&line).unwrap_or_else(|error| panic!("a protocol message, not {line:?}: {error}"))
        }

        /// A request and the response carrying its id.
        async fn request(&mut self, method: &str, params: Value) -> Value {
            self.next_id += 1;
            let id = self.next_id;
            self.send(serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
                .await;
            loop {
                let message = self.read().await;
                if message.get("id") == Some(&serde_json::json!(id)) {
                    return message;
                }
            }
        }

        /// Standard input closes; what the server returns.
        async fn close(mut self) -> i32 {
            drop(self.input.take());
            self.served.await.unwrap()
        }
    }

    #[test]
    fn the_tools_are_listed_with_their_arguments() {
        let names: Vec<String> = tools().into_iter().map(|registered| registered.tool.name).collect();
        assert_eq!(names, EVERY_TOOL);
        // The tools offered to a session over standard input and output are every fn of
        // this module that carries `tool` but `report`.
        // @lfy def/mcp/main.lfy:serve#serve:serve:6d46022fc7731233d153763aaa5a974d404b799d2e7f3d3535aaccd0a9895518
        let fixture = Fixture::new();
        assert_eq!(offered(&tools(), &fixture.session().workspace, None), TOOL_NAMES);
        let served = Served::over_stdio(&fixture.root);
        // Such a session is bound to nothing.
        // @lfy def/mcp/main.lfy:serve#serve:serve:8579f2d17bf0b508fda59fe780ac73d54d3271fe8f9994ccd60bd5f66d026331
        assert!(served.session.lock().unwrap().binding.is_none());
        let result = served.list();
        // The result may be treated as fresh for no time and cached by this client only.
        // @lfy def/mcp/main.lfy:problems#problems:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
        assert_eq!(result.ttl_ms, Some(0));
        assert_eq!(result.cache_scope, Some(CacheScope::Private));
        let listed = result.tools;
        assert_eq!(listed.len(), TOOL_NAMES.len());
        // `report` is not offered, so it is not listed.
        // @lfy def/mcp/main.lfy:report#report:tool:ff191fef77edc21f5b1b6379da11a026007625ceb2ea84877365d76b43d3a88f
        assert!(!listed.iter().any(|tool| tool.name == "elfie_report"), "{listed:?}");
        // Every other tool is listed as one tool, named and described by its fn.
        // @lfy def/mcp/main.lfy:problems#problems:tool:47dafd874509841945f72721454f0bde82f7cb760967d4104944c32a0b08e4f6
        assert_eq!(listed[0].name, "elfie_problems");
        // The one parameter of `problems` that is not the session is optional, so its
        // argument is not required; the one of `find` is neither optional nor defaulted.
        // @lfy def/mcp/main.lfy:problems#problems:tool:87fb1f093e523c06d8bd452dd3daa9a60adcb2fa4c4dd899726161be0c185d3c
        assert_eq!(listed[0].input_schema["required"], serde_json::json!([]));
        // @lfy def/mcp/main.lfy:find#find:tool:26960caba8b776d48816a31b7cd3d9d20254e141db7595c95945236688b3ff13
        assert_eq!(listed[1].input_schema["required"], serde_json::json!(["name"]));
        // The description of an argument is the description of its parameter.
        // @lfy def/mcp/main.lfy:problems#problems:tool:0d47cd7405248873ce91f19150020622e2bbc3156320aa272d9325d8ab7ad9b4
        assert_eq!(
            listed[0].input_schema["properties"]["file"]["description"],
            "a path relative to the root; every file when left out"
        );
        // The name of each tool is `elfie_` and the fn's identifier, and its description the
        // fn's definition.
        // @lfy def/mcp/main.lfy:problems#problems:tool:e3326a06fa53e9623634da620a9bac4aa2a1d0c7e0ea9f0d1143f0f4443b8ba5
        // @lfy def/mcp/main.lfy:problems#problems:tool:97369ca270f9c5503c620a5a45c2e5bda573d9d44900f052d6aa2f6fab3ccd0c
        assert_eq!(
            listed[0].description.as_deref(),
            Some("The problems of the program, or of one file: what the compiler, an editor, or a check would report")
        );
        // A tool whose fn takes nothing but the session holds no argument.
        // @lfy def/mcp/main.lfy:grammar#grammar:tool:7c4b82dcd83184afd88477db1f841b99e09a48579fbd8a607ecacab9384b07a8
        assert_eq!(listed[5].input_schema["properties"], serde_json::json!({}));
        // A line is a number, and is listed as one; `changes` now takes a target too.
        // @lfy def/mcp/main.lfy:source#source:tool:c6487b689eaab2ccd0e199ee1b320e9a2665cee02bdd6cb984e65e94e9b22b46
        assert_eq!(listed[11].input_schema["properties"]["line"]["type"], "number");
        // @lfy def/mcp/main.lfy:source#source:tool:5bf72ec14e48137ed89a57ea542187e7e9c485c8a42ba64d90d18cc298f7a0fe
        assert_eq!(listed[11].input_schema["required"], serde_json::json!(["file", "line"]));
        // @lfy def/mcp/main.lfy:changes#changes:tool:ec3c7defff9f765cc593846440341f6c9ab6818d3c051ab9101385709365d3fb
        assert_eq!(listed[12].input_schema["properties"]["target"]["type"], "string");
        // @lfy def/mcp/main.lfy:changes#changes:tool:0c631194a5026a108cc79d09dc3877b6278e686a4a4b0853d4b441b65c8bb6fc
        assert_eq!(listed[12].input_schema["required"], serde_json::json!(["unit"]));
        // `text` is a boolean with a default, so it is listed as one and is not required.
        // @lfy def/mcp/main.lfy:output#output:tool:b44f0f08cd4a5a17ed3563ee169631ec0e06701a1866f2bcbb070d4ea652c7d5
        // @lfy def/mcp/main.lfy:output#output:tool:cccf2bf991122a41e5f8d1b615ff93f9a9b92d85f03790b2610579b50c0becde
        assert_eq!(listed[9].input_schema["properties"]["text"]["type"], "boolean");
        assert_eq!(listed[9].input_schema["required"], serde_json::json!(["name"]));
        // @lfy def/mcp/main.lfy:output#output:tool:d72cb21d0e903b84ed1802adf513079764243da9561991d7767a50a7eb2e2cbd
        assert_eq!(
            listed[9].description.as_deref(),
            Some(
                "Where generated code is, with its text or without it: every region that came from one entity, that answers for one criterion or test, or that one output file holds"
            )
        );
        // `search` carries `tool` alone, so a session over standard input and output is
        // offered it; the fns carrying `compilerTool` and `commandTool` are not listed,
        // since such a session has no binding.
        // @lfy def/mcp/main.lfy:search#search:tool:38ffd83bb0a3a18ad44b57a02e041c06b560ae946cbab843e22f39a99e9aac66
        // @lfy def/mcp/main.lfy:search#search:tool:49b02b5a6852ab0082ddca33e30f1e4557cf5670633599b4da7d94b54afc74a0
        assert_eq!(listed[10].name, "elfie_search");
        // @lfy def/mcp/main.lfy:search#search:tool:84b35a27f4c75349de61c3a20a30bb45eead886577edc895af4e407810c893a3
        // @lfy def/mcp/main.lfy:search#search:tool:1e8cb253167c19b0e2e9148d8b4dd61e332b68d5b13ccf6aa0523572759ef678
        // @lfy def/mcp/main.lfy:search#search:tool:32aab10a54d3615f163e0fbc4cd259bb8fdce07cc79bde7fddfbe0fa16fba954
        assert_eq!(listed[10].input_schema["required"], serde_json::json!(["pattern"]));
        // @lfy def/mcp/main.lfy:task#task:tool:b4a5feee0df4f25397442a14ddc5789cddaecd15441de63010655c2768703e25
        // @lfy def/mcp/main.lfy:dependency#dependency:tool:c19bc9c85c771a6a121e931a3d90f5e925e08c8d87bdaf800db6922b0e2876ff
        // @lfy def/mcp/main.lfy:mark#mark:tool:d0022315029fb169381cd2230a938407632d7d0dd0131fbc83073feec1ae22af
        // @lfy def/mcp/main.lfy:submit#submit:tool:8f4adcbdfc16d749d9b83d55424fdb414dca3efc886559174e902fd2a4912a79
        // @lfy def/mcp/main.lfy:build#build:tool:88906b89a3fff75afd55acabcbfcfcad2e07d893ba2ff2c1a9fbe35abfcb1f8d
        // @lfy def/mcp/main.lfy:test#test:tool:3095733c477eec37202566f8813793d01f848b595701cdd835ffc630ba78f768
        // @lfy def/mcp/main.lfy:lint#lint:tool:f0044ba556ab8e00066360dce81c2a584d6ad207e30d0233e25d4294fde9e8a5
        // @lfy def/mcp/main.lfy:format#format:tool:fad2173ad6add7bf6e4a61983e9685caf2ffe4dfb3b1a4887c920667e347acee
        for withheld in ["elfie_task", "elfie_dependency", "elfie_mark", "elfie_submit", "elfie_build", "elfie_test", "elfie_lint", "elfie_format"] {
            assert!(!listed.iter().any(|tool| tool.name == withheld), "{withheld}");
        }
    }

    /// Every fn carrying `tool` is listed with the name, description, arguments, and
    /// required arguments its parameters give it, so each of them is one tool.
    // @lfy def/mcp/main.lfy:task#task:tool:b4176adea23f8e6c58f4d041e7a85447286076984bd05464e92528b60fda6d8b
    // @lfy def/mcp/main.lfy:task#task:tool:46ad78939d73914a576d0dce0904cae992474f9eeb184c717a5a0c324091d4d5
    // @lfy def/mcp/main.lfy:task#task:tool:3f09d52a0b71d1b830863b88454d30b98bb821b36e428efed32d7e6169482a3c
    // @lfy def/mcp/main.lfy:task#task:tool:6ddac2bcba95be32011edabc6d0abbc63da57cd4a0182a8c86f882a8482706e7
    // @lfy def/mcp/main.lfy:task#task:tool:e91db1a6069241eefc991fb1723f76dac6a194f33db9dbeaa21f8163f69cbdaf
    // @lfy def/mcp/main.lfy:task#task:tool:b2e2fadd9a1cd573a4b55e2e77fbd573ec3a9f6184ccd88beca5e743b871e9cd
    // @lfy def/mcp/main.lfy:task#task:tool:b361de400635b42112cf8a02205ed19f3875a7f7f9835b646817814b39f0233d
    // @lfy def/mcp/main.lfy:dependency#dependency:tool:4fd4dbdb36a4d1d4e8a9589471a29d8337372b850160162b8a14f940ad74bbc6
    // @lfy def/mcp/main.lfy:dependency#dependency:tool:7b7fe0f695f34ca950fbd8f4bc3aa16eb90bc78d78ed5a87812fe95b03c71eb5
    // @lfy def/mcp/main.lfy:dependency#dependency:tool:62ccf07aff23acf72a7e091f19b6d110da5bb4dc6f969dcadd130d72ab9ccf3f
    // @lfy def/mcp/main.lfy:dependency#dependency:tool:01c082cb2c776a72049a0c164c556b408ada68b6c8d3e961aed6be20476ba114
    // @lfy def/mcp/main.lfy:dependency#dependency:tool:feb08658fc3d60e206035d80439de4021e443fe53d2226e6e8ad43298da9f5b7
    // @lfy def/mcp/main.lfy:dependency#dependency:tool:4894a15426436158e1d439a6ad82b49784af28fc4c896d683eb4bcfe455480a0
    // @lfy def/mcp/main.lfy:dependency#dependency:tool:2b186e60bc3e791246ddc810258418700586e003d4998497bd580b40f6624761
    // @lfy def/mcp/main.lfy:mark#mark:tool:0f8e6f40a5d6de867cde8b1be5b3fc853845b786664671c550f9dad09eb01911
    // @lfy def/mcp/main.lfy:mark#mark:tool:0b9567706ca9a8b66a0757d097e5fd9c7e6df936098d4aeb7f7e6834a6635d71
    // @lfy def/mcp/main.lfy:mark#mark:tool:5295f5acf77612da1ff8627c746c87ccff5a0dbb1770c05ed869d1ed28d6c7a7
    // @lfy def/mcp/main.lfy:mark#mark:tool:44369bf678415e07ca9c08b82b57c5fe7969e2e9850e765a8f32faeab4b784c4
    // @lfy def/mcp/main.lfy:mark#mark:tool:644ced5c6aeca4f5ba2d0a64656faf7dc4224aa51d7dcfc2c46b2d9a241fb040
    // @lfy def/mcp/main.lfy:mark#mark:tool:84d31d74a162783af16e3e2c1bbb0339be256b810408f3869f52a6d8f243ec5e
    // @lfy def/mcp/main.lfy:mark#mark:tool:745f570e06eb5111681f3c579ae61468329b1f2ae73381dfb2d99e3ba743c32f
    // @lfy def/mcp/main.lfy:submit#submit:tool:46e6a49028a494b7d6bb872cd0cc63d6d03e25fffce40f625b0facd57b49a725
    // @lfy def/mcp/main.lfy:submit#submit:tool:46c97b8c41d5f41394e9c6efbaf1f3bdc24c40082759615fb1b06732fb2d02c7
    // @lfy def/mcp/main.lfy:submit#submit:tool:59836951e15ef484cb381d25b49b247ff2da58ae11631e91ff190839871ce24d
    // @lfy def/mcp/main.lfy:submit#submit:tool:27e3a76662e3a4a2d2275aa703d2b835bd68c51282753e21b9337c744d37e5d6
    // @lfy def/mcp/main.lfy:submit#submit:tool:87847e0f685515fe21c083343037e22e14155b3956e0220e0c6c17c145fd682d
    // @lfy def/mcp/main.lfy:submit#submit:tool:f5d375e7f92630c7c75c6af5a62d636df1c895f2abf6280b5fc27486624dd3d6
    // @lfy def/mcp/main.lfy:submit#submit:tool:5e8f39a0f84b057b8516d3aba402f08b7a867533e314d8ede4294d9169055ec1
    // @lfy def/mcp/main.lfy:search#search:tool:dffd65949512cdddb7735f1aeed21e452cf605acabf06b395717014263c1c4e8
    // @lfy def/mcp/main.lfy:search#search:tool:8cabf755d8b4ed9c400ddc760b3676b7d288d84ef6a4b4d3588840ee0fb85e46
    // @lfy def/mcp/main.lfy:search#search:tool:502a513b0a0681b5d9af190aa237ed651ee77b387a6b91c00943b77203be2a26
    // @lfy def/mcp/main.lfy:build#build:tool:437d060ed5ca74732574dd1afab19a5a166957ef20fcdb97bf4d590041c48336
    // @lfy def/mcp/main.lfy:build#build:tool:e0309114048649453e6790ef4c6deeabd6ec9481b22aa5d6159c671535d25832
    // @lfy def/mcp/main.lfy:build#build:tool:d0bfe30fbbef86bd5724c6c59435c754542f0b44ae99df590217de4b3e02fd28
    // @lfy def/mcp/main.lfy:build#build:tool:a89edaee38beabc1ef0df886f2a21cc45ce985962b64e3309e470a2a02e9b576
    // @lfy def/mcp/main.lfy:build#build:tool:a3290c07f19e4214355190476d5ab8cc6a9bba3ec08cfedee6e90f738f4dcfc0
    // @lfy def/mcp/main.lfy:build#build:tool:cc821fff828717e6310df1103a08b0267f91fc98bbc71bc89165e1224840cb87
    // @lfy def/mcp/main.lfy:build#build:tool:d7aadcec979a675c909a3493228563ee20b979feaf86ec325edc207f6683b4f5
    // @lfy def/mcp/main.lfy:test#test:tool:5386b37f95ca729944462379291817c9470858369f6cfb9387b834f8767c2b51
    // @lfy def/mcp/main.lfy:test#test:tool:a038afc81a5b6f64a780e0144c3ac1be7ce831938025338a3c18193929918aba
    // @lfy def/mcp/main.lfy:test#test:tool:b781017493ee33fccd6be22cef4cb4b2338a130da8607db5c8a6899222f40fd1
    // @lfy def/mcp/main.lfy:test#test:tool:4930fab7d3d2c47253afb07544bc3ab8fcd4c5deb1e42ce3f9f8b2dcc6175c30
    // @lfy def/mcp/main.lfy:test#test:tool:cbeb0da9f28bfbd6a6d084bb140fca7c043af3a7fee4c0c506d08562fcb13392
    // @lfy def/mcp/main.lfy:test#test:tool:91cfa553c86897863866df4f70a520da5ab64eb9facaea1c00b2c552ee52e2da
    // @lfy def/mcp/main.lfy:test#test:tool:430d6d90db0096b765c313099b0425e48c0069b5c3a1f92aa82676371e14f7f3
    // @lfy def/mcp/main.lfy:lint#lint:tool:6e14d79b29847057cee32aad56ac37159ec40c848c8cb9346ffbcca0cf4ce022
    // @lfy def/mcp/main.lfy:lint#lint:tool:9f1b5c7200bd49c99da2c72dd74e0589a75980b35e4b797ecc4b538e18b380a1
    // @lfy def/mcp/main.lfy:lint#lint:tool:fada2d3544a4caf7440370d2fdff08fd8d46235a4307ed27758f2aa426047db5
    // @lfy def/mcp/main.lfy:lint#lint:tool:f23edb098a47aff31e4aaab809a03e3db1a479cda385597dafd4ffca5fc96a43
    // @lfy def/mcp/main.lfy:lint#lint:tool:9293b2f58b28f6819e289105b3ffdfeddf57cb0b003b969596d58a7cf3cedeab
    // @lfy def/mcp/main.lfy:lint#lint:tool:8415b8b106d669fec27102fd04507c3d814b919d8bcf79f8e7d2650f5fb4faee
    // @lfy def/mcp/main.lfy:lint#lint:tool:81afa3f50b86d1ed54b7bad2244850fb1e841e53d326e18b8f9188e1037bc426
    // @lfy def/mcp/main.lfy:format#format:tool:a84a6f1ea99ee3c31c458952672bfa71b4b076970b0ea7e5f97fc4ca1ad3476d
    // @lfy def/mcp/main.lfy:format#format:tool:c9525e9b70499077881113750f0011474b3ed51c8f839564643d93637cedd60c
    // @lfy def/mcp/main.lfy:format#format:tool:b844517eed0302f020cc276c74d1c8e649070a57de01e61e1bc8194d3157f1a5
    // @lfy def/mcp/main.lfy:format#format:tool:6b1377a3f7b741d23cf4ee8f6be9b6e69d161c9b715fd8f55d0247578017bb6c
    // @lfy def/mcp/main.lfy:format#format:tool:6f7967f9d4f086abae04ce989ce1d49bea43882ec4bbb31e34aee7bf9417d481
    // @lfy def/mcp/main.lfy:format#format:tool:c7c05422e084fd79bb583a5aa90b70d947898019100fe9d2bd60b4fe23a540f2
    // @lfy def/mcp/main.lfy:format#format:tool:41a181e4de5b31903fd19b4fec451a4d3982908e194189ef3b45c691280dd06b
    #[test]
    fn every_tool_is_listed_as_its_fn_declares_it() {
        let listed = traits::list(&tools());
        let of = |name: &str| {
            listed
                .iter()
                .find(|tool| tool.name == name)
                .unwrap_or_else(|| panic!("{name} is listed"))
                .clone()
        };
        let task = of("elfie_task");
        assert!(task.description.as_deref().is_some_and(|text| text.starts_with("What this compiler run must do")));
        // The one parameter of `task` that is not the session is optional, so its argument
        // is not required.
        assert_eq!(task.input_schema["required"], serde_json::json!([]));
        assert_eq!(task.input_schema["properties"]["unit"]["type"], "string");
        let dependency = of("elfie_dependency");
        assert_eq!(dependency.input_schema["required"], serde_json::json!(["name"]));
        assert!(
            dependency
                .description
                .as_deref()
                .is_some_and(|text| text.starts_with("What one entity of a dependency"))
        );
        // Every parameter of `mark` is optional, so none of its arguments is required, and
        // `line` is a number.
        let mark = of("elfie_mark");
        assert_eq!(mark.input_schema["required"], serde_json::json!([]));
        assert_eq!(mark.input_schema["properties"]["line"]["type"], "number");
        assert_eq!(mark.input_schema["properties"]["entity"]["type"], "string");
        // Both parameters of `submit` are needed, so both arguments are required.
        let submit = of("elfie_submit");
        assert_eq!(submit.input_schema["required"], serde_json::json!(["outcome", "message"]));
        assert_eq!(submit.input_schema["properties"]["outcome"]["description"], "done, blocked, or clarify");
        let search = of("elfie_search");
        assert_eq!(search.input_schema["required"], serde_json::json!(["pattern"]));
        for (name, required, arguments) in [
            ("elfie_build", serde_json::json!([]), vec!["scope", "target"]),
            ("elfie_test", serde_json::json!([]), vec!["filter", "scope", "target"]),
            ("elfie_lint", serde_json::json!([]), vec!["scope", "target"]),
            ("elfie_format", serde_json::json!([]), vec!["target"]),
        ] {
            let tool = of(name);
            assert_eq!(tool.input_schema["required"], required, "{name}");
            for argument in arguments {
                assert_eq!(tool.input_schema["properties"][argument]["type"], "string", "{name} {argument}");
            }
            assert!(tool.description.as_deref().is_some_and(|text| text.starts_with("Run the target's")), "{name}");
        }
    }

    /// The whole server, driven over streams: a root whose `def` holds `a.lfy` reading
    /// `const y = z;` answers a `tools/list` naming the fourteen tools and a call of
    /// `elfie_problems` with the one binder error at `z`, and returns 0 once the stream it
    /// reads as standard input closes.
    // @lfy def/mcp/main.lfy:serve#serve:serve:594496b8884ab7d56e00dedca9ef986ab491c41d7a8017be615d11a2edfb0c89
    #[tokio::test]
    async fn serving_lists_the_tools_answers_a_call_and_returns_zero_when_input_closes() {
        let fixture = Fixture::new();
        fixture.write("def/a.lfy", "const y = z;\n");
        let session = fixture.session();
        let text = problems(&session, None).unwrap();
        assert_eq!(text.lines().count(), 1, "{text}");
        assert!(text.starts_with("def/a.lfy:1:10: binder error: "), "{text}");
        let result = call(&session, "elfie_problems", serde_json::json!({}));
        assert!(!result.is_error);
        assert_eq!(result.text, text);
        let result = call(&session, "elfie_problems", serde_json::json!({ "file": "def/a.lfy" }));
        assert_eq!(result.text, text);

        // @lfy def/mcp/main.lfy:serve#serve:serve:0d7ad5672274c815b6ad256268ff29d8425eb7ed8114bb7c9ea52bd41bedde9b:2
        let mut agent = Agent::connect(&fixture.root);
        let initialized = agent
            .request(
                "initialize",
                serde_json::json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": { "name": "elfie-mcp test", "version": "0" },
                }),
            )
            .await;
        // @lfy def/mcp/main.lfy:Protocol
        assert_eq!(initialized["result"]["protocolVersion"], "2025-06-18", "{initialized}");
        // Tools only: the capabilities carry no resources and no prompts.
        // @lfy def/mcp/main.lfy:serve#serve:serve:4d105e14d91381963764973d69e9e54abacde103ac17990250a98dadf1ced8cb
        assert!(initialized["result"]["capabilities"]["tools"].is_object(), "{initialized}");
        assert!(initialized["result"]["capabilities"]["resources"].is_null(), "{initialized}");
        assert!(initialized["result"]["capabilities"]["prompts"].is_null(), "{initialized}");
        // The instructions name only the tools offered over standard input and output, and
        // say outright that none of them writes a file.
        // @lfy def/mcp/main.lfy:serve#serve:serve:3cd568fc43a27e041f94444e0bd774ad591038e833bc93d99d8667e3342f190f
        let said = initialized["result"]["instructions"]
            .as_str()
            .unwrap_or_else(|| panic!("instructions, not {initialized}"));
        for name in TOOL_NAMES {
            assert!(said.contains(name), "{said}");
        }
        for withheld in ["elfie_report", "elfie_task", "elfie_dependency", "elfie_mark", "elfie_submit", "elfie_build"] {
            assert!(!said.contains(withheld), "{said}");
        }
        assert!(said.contains("None of them writes a file."), "{said}");
        agent
            .send(serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .await;

        // @lfy def/mcp/main.lfy:serve#serve:serve:6d46022fc7731233d153763aaa5a974d404b799d2e7f3d3535aaccd0a9895518
        let listed = agent.request("tools/list", serde_json::json!({})).await;
        let served: Vec<&str> = listed["result"]["tools"]
            .as_array()
            .unwrap_or_else(|| panic!("a list of tools, not {listed}"))
            .iter()
            .map(|tool| tool["name"].as_str().expect("a tool name"))
            .collect();
        assert_eq!(served, TOOL_NAMES);

        // @lfy def/mcp/main.lfy:problems#problems:tool:d9b6b1ce215d966d4d49ba31d319199d9539fe1614bed20bc86032b0e2fc28c0
        let called = agent
            .request("tools/call", serde_json::json!({ "name": "elfie_problems", "arguments": {} }))
            .await;
        assert_ne!(called["result"]["isError"], serde_json::json!(true), "{called}");
        assert_eq!(called["result"]["content"][0]["text"], text, "{called}");

        // @lfy def/mcp/main.lfy:serve#serve:serve:e36ddce87201b751562fbe8a68504f70e09879c00d4b93be51b40d6176adae2e
        assert_eq!(agent.close().await, 0);
    }

    // @lfy def/mcp/main.lfy:problems#problems:problems:4e726dce93d9d233aedec39235ecea647b64e1f851fe518f6992aea5add1da3c
    #[test]
    fn problems_says_when_there_are_none() {
        let fixture = Fixture::new();
        fixture.write("def/a.lfy", "const y = 1;\n");
        let session = fixture.session();
        assert_eq!(problems(&session, None).unwrap(), "no problems");
        assert_eq!(problems(&session, Some("def/a.lfy")).unwrap(), "no problems in def/a.lfy");
        // A file the program does not hold has no problems either, and is answered with
        // text rather than a failure.
        // @lfy def/mcp/main.lfy:problems#problems:problems:4e726dce93d9d233aedec39235ecea647b64e1f851fe518f6992aea5add1da3c
        assert_eq!(problems(&session, Some("def/b.lfy")).unwrap(), "no problems in def/b.lfy");
    }

    // The session over standard input and output is offered both `find` and `outline`, so
    // an argument of either that is missing, unknown, or of the wrong type is an error
    // naming it and the fn is not called.
    // @lfy def/mcp/main.lfy:find#find:tool:14b1732157522ebf4b24b6038fcddb65b5b23a3689651521b1bf763ca8fb0879
    // @lfy def/mcp/main.lfy:find#find:tool:26dc105fcfc8d9cc4ed181d1ae1172c0d0e88942453e1513fd22d3d6c8b3b095
    #[test]
    fn a_bad_argument_is_an_error_naming_it() {
        let fixture = Fixture::new();
        fixture.write("def/a.lfy", "const y = 1;\n");
        let session = fixture.session();
        let result = call(&session, "elfie_find", serde_json::json!({}));
        assert!(result.is_error);
        assert!(result.text.contains("name"), "{}", result.text);
        let result = call(&session, "elfie_find", serde_json::json!({ "name": "y", "extra": true }));
        assert!(result.is_error);
        assert!(result.text.contains("extra"), "{}", result.text);
        // @lfy def/mcp/main.lfy:outline#outline:tool:6e95ab5fd88f41d7145f86ed344f8156d3464326e598b0ac637ac3ddbc5d369f
        // @lfy def/mcp/main.lfy:outline#outline:tool:150963511f1c73e51401709017f75ec80e37e411880f968dadf301ed995494cd
        let result = call(&session, "elfie_outline", serde_json::json!({ "file": 1 }));
        assert!(result.is_error);
        assert!(result.text.contains("file") && result.text.contains("string"), "{}", result.text);
        // The fn fails: an error carrying the failure, and the server goes on, which the
        // call after this one shows.
        // @lfy def/mcp/main.lfy:outline#outline:tool:6114e21db6cb9375ba623e9ff0b359857133d98565b50e37b08ee737a3e0ac41
        // @lfy def/mcp/main.lfy:outline#outline:tool:532b2ad88427069a8c24f3a734fe21c29f9d0b4bcf57c523a2436c4d0228712f
        let result = call(&session, "elfie_outline", serde_json::json!({ "file": "def/none.lfy" }));
        assert!(result.is_error);
        assert!(result.text.contains("def/a.lfy"), "{}", result.text);
        // The fn returns: the result it returns is the call's result, and the fn was reached
        // with the session and the arguments.
        // @lfy def/mcp/main.lfy:outline#outline:tool:56807c149fa490c87e28f903d14a7c0f45d7bd9ef10df81481e83b233f241b30
        // @lfy def/mcp/main.lfy:outline#outline:tool:f5e2e8d44cd795aa69322fef5a2f21e8c943fd631ec83fb04f50665b2a0d412f
        let result = call(&session, "elfie_outline", serde_json::json!({ "file": "def/a.lfy" }));
        assert!(!result.is_error, "{}", result.text);
        assert_eq!(result.text, outline(&session, "def/a.lfy").unwrap());
    }

    // @lfy def/mcp/main.lfy:serve#serve:serve:995535b9c7a11554f96d88793004bc092de31525b26cd0ca15ce449e17d8e386
    #[test]
    fn changed_vanished_and_new_files_are_reread_before_a_call() {
        let fixture = Fixture::new();
        fixture.write("def/a.lfy", "const y = z;\n");
        let mut session = fixture.session();
        assert!(problems(&session, None).unwrap().contains("binder error"));

        // A file whose modification time or size differs is re-read.
        // @lfy def/mcp/main.lfy:serve#serve:serve:1893a7a1afe4f16bc76a47596614b6645e060072995b9387c9386905037fe959
        fixture.write("def/a.lfy", "const z = 1;\nconst y = z;\n");
        session.refresh();
        assert_eq!(problems(&session, None).unwrap(), "no problems");

        // A `.lfy` file under the source directory that no earlier call saw is read in.
        // @lfy def/mcp/main.lfy:serve#serve:serve:9a771a2886ce7deadeb6a39ae4524d1bded9ac7c87fc49266ce3da78c7e4fbcc
        fixture.write("def/b.lfy", "const w = missing;\n");
        session.refresh();
        assert!(session.workspace.file("def/b.lfy").is_some());
        assert!(problems(&session, None).unwrap().contains("def/b.lfy:1:10"));

        // A file that has vanished is dropped the same way.
        // @lfy def/mcp/main.lfy:serve#serve:serve:01ce18f671ec478741555e6ff345d61120f118da6209c46d18cd01b3cbff3e6a
        fixture.remove("def/b.lfy");
        session.refresh();
        assert!(session.workspace.file("def/b.lfy").is_none());
        assert_eq!(problems(&session, None).unwrap(), "no problems");

        // An edit that keeps both the size and the modification time is not seen: the stamp
        // of the file is unchanged, so nothing is re-read.
        // @lfy def/mcp/main.lfy:serve#serve:serve:a0e0712c211c914b7d1655d1a85d5aa373db8b9d94524624d54554e2501f845d
        let path = fixture.root.join("def/a.lfy");
        let before = stamp(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap().replace("const z = 1;", "const z = 2;");
        fs::write(&path, &text).unwrap();
        fs::File::open(&path).unwrap().set_modified(before.0.unwrap()).unwrap();
        assert_eq!(stamp(&path), Some(before));
        session.refresh();
        assert_eq!(session.seen.get("def/a.lfy"), Some(&before));

        // @lfy def/mcp/main.lfy:serve#serve:serve:4c5aeb9423c7e9206e75798da9c197b91b439325adda5776e09a2061d144988a
        fixture.write("elfie.json", r#"{ "name": "renamed", "source": "src", "lib": "lib" }"#);
        fixture.write("src/c.lfy", "const c = 1;\n");
        session.refresh();
        assert_eq!(session.workspace.name, "renamed");
        assert!(session.workspace.file("src/c.lfy").is_some());
        assert!(session.workspace.file("def/a.lfy").is_none());
    }

    // @lfy def/mcp/main.lfy:find#find:find:8d5e1239e348d7ebcaa7e007e6bfc71fed3f9aa4abd903c5cc7f3160b0416367
    #[test]
    fn find_entity_and_references_describe_a_declaration() {
        let fixture = Fixture::new();
        fixture.write("def/a.lfy", "/// Doc\nd A: `An A` { $x: `An x` = string; }\nconst y = A;\n");
        let session = fixture.session();

        let found = find(&session, "A").unwrap();
        // The line ends with the definition. @lfy def/mcp/main.lfy:find#find:find:eb271c552e5c82df2d2018db45719c602a20801b3e7e3e1f948bf7c9f0108b1d
        assert_eq!(found, "data A def/a.lfy:2: An A");
        assert_eq!(find(&session, "def/a.lfy:A").unwrap(), found);
        let none = find(&session, "nothing").unwrap();
        // @lfy def/mcp/main.lfy:find#find:find:33784dd4ac5c7832aad69dab34430cacc1eb4b82930eb20f500fcfe9c89be26c
        assert!(none.contains("nothing is declared as nothing"), "{none}");
        assert!(none.contains("elfie_outline"), "{none}"); // @lfy def/mcp/main.lfy:find#find:find:1a0733810793cda3b844ef9c1beb121fa974d605748755292d61aa511c80a1e2

        // @lfy def/mcp/main.lfy:entity#entity:entity:d43b0e556e705056317e1af4e3df190417f20a08435faad56d54fcafa63e524e
        let described = entity(&session, "A").unwrap();
        assert!(described.starts_with("# data A (def/a.lfy:2)"), "{described}");
        assert!(described.contains("## Definition\nAn A"), "{described}");
        assert!(described.contains("## Documentation\nDoc"), "{described}");
        assert!(described.contains("## Members\n- x: An x (string)"), "{described}");
        assert!(described.contains("## References (1)\n- def/a.lfy:3"), "{described}");
        assert!(described.contains("## Source\n```elfie\n/// Doc\nd A: `An A` { $x: `An x` = string; }\n```"), "{described}");
        assert!(!described.contains("\n---\n"));

        // @lfy def/mcp/main.lfy:references#references:references:172a06e97d4c30625ada08fa53113f0fdd6c084862a2db5f64451f15c9853456
        let used = references(&session, "A").unwrap();
        assert_eq!(used, "def/a.lfy:3:10: const y = A;");
        assert_eq!(references(&session, "y").unwrap(), "y is never used");
    }

    // @lfy def/mcp/main.lfy:entity#entity:entity:d43b0e556e705056317e1af4e3df190417f20a08435faad56d54fcafa63e524e
    #[test]
    fn entity_lists_the_traits_it_carries() {
        let fixture = Fixture::new();
        fixture.write("def/a.lfy", "trait t: `A t` {}\nd A is t {}\n");
        let session = fixture.session();
        assert_eq!(problems(&session, None).unwrap(), "no problems");
        let described = entity(&session, "A").unwrap();
        assert!(described.contains("## Traits\nt\n"), "{described}");
    }

    // Every field of the hover has a heading, `Hover.owner` among them: a member found as
    // `A.x` is headed by the declaration that declares it.
    // @lfy def/mcp/main.lfy:entity#entity:entity:d43b0e556e705056317e1af4e3df190417f20a08435faad56d54fcafa63e524e
    #[test]
    fn entity_names_the_owner_of_a_member() {
        let fixture = Fixture::new();
        fixture.write("def/a.lfy", "d A { $x: `An x` = string; }\n");
        let session = fixture.session();
        let described = entity(&session, "A.x").unwrap();
        assert!(described.contains("## Owner\nA\n"), "{described}");
        // A declaration that nothing owns has no such heading.
        assert!(!entity(&session, "A").unwrap().contains("## Owner"));
    }

    // @lfy def/mcp/main.lfy:entity#entity:entity:0c69bca0860a743d038fb5c4f6c609c4ded6631ac9cf95f37cccba908854cbcd
    #[test]
    fn several_entities_are_separated_by_a_rule_and_headed_by_their_file() {
        let fixture = Fixture::new();
        fixture.write("def/a.lfy", "d A {}\n").write("def/b.lfy", "d A {}\n");
        let session = fixture.session();
        let described = entity(&session, "A").unwrap();
        assert!(described.contains("\n---\n"), "{described}");
        assert!(described.contains("(def/a.lfy:1)") && described.contains("(def/b.lfy:1)"), "{described}");
        let narrowed = entity(&session, "def/b.lfy:A").unwrap();
        assert!(!narrowed.contains("\n---\n"), "{narrowed}");
        assert!(narrowed.contains("(def/b.lfy:1)"), "{narrowed}");
    }

    // @lfy def/mcp/main.lfy:outline#outline:outline:f4a001d0d002913edf84c20318c9fdac89e274849e6c553e36e790f938bb6027
    #[test]
    fn outline_indents_declarations_or_lists_the_files() {
        let fixture = Fixture::new();
        fixture.write("def/a.lfy", "/// Doc\nd A { $x = string; }\nconst y = 1;\n");
        let session = fixture.session();
        assert_eq!(outline(&session, "def/a.lfy").unwrap(), "data A 2\n  member x 2\nvariable y 3");
        // @lfy def/mcp/main.lfy:outline#outline:outline:a024fe62db1e31cd5f20fef57d6d0cc38073b0724bb0ecf3e828e91bf7d495d3
        let error = outline(&session, "def/zzz.lfy").unwrap_err();
        assert!(error.contains("def/zzz.lfy is not in the program") && error.contains("def/a.lfy"), "{error}");
    }

    // @lfy def/mcp/main.lfy:grammar#grammar:grammar:51bcd7292055364b6387726881c5443879b09b36eae8e1534b8d84a84effc72f
    #[test]
    fn grammar_holds_the_terminals_and_the_rules() {
        let fixture = Fixture::new();
        let session = fixture.session();
        let text = grammar(&session).unwrap();
        assert!(text.contains("SourceFile"), "{text}");
        assert!(text.contains("\n\n"), "{text}");
        assert_eq!(
            text,
            format!("{}\n\n{}", elfie_core::grammar::terminal_document(), elfie_core::grammar::grammar_document())
        );
    }

    fn compiled_project() -> Fixture {
        let fixture = Fixture::new();
        fixture
            .with_rust_target()
            .write("def/a.lfy", "use \"./b\";\nd A: `An A` { $b = B; }\n")
            .write("def/b.lfy", "d B {}\n");
        fixture
    }

    // The text lists the problems, then the moves, then the units, then the batches; this
    // project has neither a problem nor a move, so it begins at the units.
    // @lfy def/mcp/main.lfy:units#units:units:3dfafdff61f6c1061f2fe64e135a6885c4554344d56b669fcd2f9dceddc9bb98
    // @lfy def/mcp/main.lfy:units#units:units:c7fcb0ca869f9f4555904c7d48858c0314cebdf85dc76b32442da4fcb97e9021
    // @lfy def/mcp/main.lfy:units#units:units:92120a7e31d7970c4faf3dd986b56767f95eeeb9e0470fd818c092f986d3e0d3
    #[test]
    fn units_lists_every_unit_and_batch_or_one_targets() {
        let fixture = compiled_project();
        let session = fixture.session();
        assert_eq!(problems(&session, None).unwrap(), "no problems");
        let listed = "rust b fresh []\nrust a fresh [b]\n\nbatch b+1 [b, a]";
        assert_eq!(units(&session, None).unwrap(), listed);
        // @lfy def/mcp/main.lfy:units#units:units:05a79e1b79428d09f369fcfe7a098f9648d1dbfb976d11d8d457dd8945737272
        assert_eq!(units(&session, Some("rust")).unwrap(), listed);
        // @lfy def/mcp/main.lfy:units#units:units:e2e8756b78db77377231d8ef5ee9fd3e078703c801906a9b774ee59a2135a3ab
        let error = units(&session, Some("go")).unwrap_err();
        assert!(error.contains("go is not a target") && error.contains("rust"), "{error}");
    }

    // @lfy def/mcp/main.lfy:request#request:request:92537a4ab2aac8027576852406e446fcb12c7b5b87c96095a386c6d7dac101ed
    #[test]
    fn request_gives_the_instructions_of_one_batch() {
        let fixture = compiled_project();
        let session = fixture.session();
        // A batch is named by its identifier or by the stem of one of its units.
        let instructions = request(&session, "b+1", None).unwrap();
        assert!(instructions.contains("An A"), "{instructions}");
        assert!(instructions.contains("def/a.lfy"), "{instructions}");
        assert!(instructions.contains("def/b.lfy"), "{instructions}");
        assert_eq!(request(&session, "a", None).unwrap(), instructions);
        assert_eq!(request(&session, "a", Some("rust")).unwrap(), instructions);
        // @lfy def/mcp/main.lfy:request#request:request:ca0a25d6a947aa41d3cbf9399a371f681224987d6109d85e16a0b7f8a85db14c
        let error = request(&session, "c", None).unwrap_err();
        assert!(error.contains("no batch has the identifier c"), "{error}");
        let error = request(&session, "a", Some("go")).unwrap_err();
        assert!(error.contains("go is not a target"), "{error}");
    }

    // @lfy def/mcp/main.lfy:check#check:check:77f6c4a5084647dc21eb7114983b08f9ebbf27471071fee839c6113ba37cdfab
    #[test]
    fn check_reads_the_outputs_from_disk_and_writes_nothing() {
        let fixture = compiled_project();
        let session = fixture.session();
        // Rejected: an error listing the problems.
        // @lfy def/mcp/main.lfy:check#check:check:2a16ea176547468dbe87eb74de7c195dc58e16c26122fc69de5f2ff15dd469f1
        let error = check(&session, "b", None).unwrap_err();
        assert!(error.starts_with("rejected b:"), "{error}");
        fixture.write("out/b.rs", "// @lfy def/b.lfy:B\npub struct B;\n");
        // Accepted: the text says so and lists the outputs.
        // @lfy def/mcp/main.lfy:check#check:check:7d045cb553e0d306e634e78ed9a299647790d4beae5075ece0962d22ff2d4ef8
        let text = check(&session, "b", None).unwrap();
        assert!(text.starts_with("accepted b for rust: 1 output\n  out/b.rs"), "{text}");
        assert!(!fixture.root.join("out/source-map.json").exists());
        let error = check(&session, "a", None).unwrap_err();
        assert!(error.contains("rejected a:"), "{error}");
    }

    /// The unit with a stem accepted against the outputs on disk, with its source maps
    /// recorded in the target's output directory the way the command line records them.
    fn record_unit(fixture: &Fixture, stem: &str) {
        let session = fixture.session();
        let planned = plan_of(&session);
        let (workspace, plan) = (planned.workspace(), &planned.plan);
        let index = resolve_unit(workspace, plan, stem, None).unwrap();
        let request = request_of_unit(&planned.program, plan, index);
        let outputs = outputs_of(workspace, plan, index);
        let verdict = generation::accept(&planned.program, plan, &request, index, &outputs, &[]);
        assert!(verdict.accepted, "{:?}", verdict.problems);
        generation::write_source_maps(&fixture.root.join("out/source-map.json"), &verdict.source_maps).unwrap();
    }

    /// A project whose unit `b` has an accepted output, with the source maps recorded in
    /// the target's output directory the way the command line records them.
    fn accepted_project() -> Fixture {
        let fixture = compiled_project();
        fixture.write("out/b.rs", "// @lfy def/b.lfy:B\npub struct B;\n");
        record_unit(&fixture, "b");
        fixture
    }

    // @lfy def/mcp/main.lfy:output#output:output:d930026963bc74a22a71de25fff3d4d8ac8a7b00ff6209a9e7d4afbbff550eb1
    #[test]
    fn output_gives_every_region_of_an_entity_with_its_text() {
        let fixture = accepted_project();
        let session = fixture.session();
        // Each region reads the path, the lines it spans, and the entity the marker names,
        // then the text of those lines in a fenced block.
        // @lfy def/mcp/main.lfy:output#output:output:22ddcfeba7456aa91005e6aeab2fcd871e9c512ede2849961438a5b7a3cc5d7c
        let text = output(&session, "B", None, true).unwrap();
        assert_eq!(text, "out/b.rs:1-2 B\n```rs\n// @lfy def/b.lfy:B\npub struct B;\n```");
        assert_eq!(output(&session, "B", Some("rust"), true).unwrap(), text);
        // The name is the path of an output file, so the regions are every marker it holds.
        // @lfy def/mcp/main.lfy:output#output:output:e3ada846896a5d797c74467acb25cc6ab9c9f3667f13eae813f8169d01373c8f
        // @lfy def/mcp/main.lfy:output#output:output:5f91a9d6a55f8212f9288be65dd547c69391009665dc988e76b8a5150ac20406
        assert_eq!(output(&session, "out/b.rs", None, true).unwrap(), text);
        // With `text` false the line of each region stands alone, so however long an output
        // is its regions read as a few lines.
        // @lfy def/mcp/main.lfy:output#output:output:a388e41750cae72a1a126c53b636547055414dad9404ae10cd679aed3861fe6a
        assert_eq!(output(&session, "out/b.rs", None, false).unwrap(), "out/b.rs:1-2 B");
        assert_eq!(output(&session, "B", None, false).unwrap(), "out/b.rs:1-2 B");
        // A call gives `text` as the boolean it is listed as, and it reaches the fn; left
        // out, the parameter's default stands and each region carries its code.
        // @lfy def/mcp/main.lfy:output#output:tool:c74c2a14496848786c2a9b0c1df1da9d6b9205300b34a2f42d670ce562aa0cf3
        let called = call(&session, "elfie_output", serde_json::json!({ "name": "B", "text": false }));
        assert!(!called.is_error, "{}", called.text);
        assert_eq!(called.text, "out/b.rs:1-2 B");
        let called = call(&session, "elfie_output", serde_json::json!({ "name": "B" }));
        assert_eq!(called.text, text);
        // @lfy def/mcp/main.lfy:output#output:tool:76a973ce9145a4809d1d1de1a68875e33067542ee119844cf65ac55c2d7affca
        let called = call(&session, "elfie_output", serde_json::json!({ "name": "B", "text": "no" }));
        assert!(called.is_error && called.text.contains("text"), "{}", called.text);
        // Nothing is generated for the name.
        // @lfy def/mcp/main.lfy:output#output:output:7e2589822812f683a65808151f3ae1177182b076f571c65ab59fa2d7e78162a8
        let none = output(&session, "A", None, true).unwrap();
        assert_eq!(none, "nothing is generated for A");
        // The target is not a known one.
        // @lfy def/mcp/main.lfy:output#output:output:e2e8756b78db77377231d8ef5ee9fd3e078703c801906a9b774ee59a2135a3ab
        let error = output(&session, "B", Some("go"), true).unwrap_err();
        assert!(error.contains("go is not a target") && error.contains("rust"), "{error}");
    }

    /// An output file named by its path is read with the markers it holds when the call
    /// arrives, not with the ones the last acceptance recorded: `src/a.rs` is nine lines
    /// long and holds a marker for `A` on line 1 and one for `b` on line 6, while the
    /// recorded map, made before an edit, still puts the second region on line 4.
    // @lfy def/mcp/main.lfy:output#output:output:b8c57fc09f7567f3d3edbaa76e9e713c59924bbb396a55209e302c62655135ba
    #[test]
    fn an_output_file_is_read_with_the_markers_it_holds_now() {
        let fixture = Fixture::new();
        fixture.with_rust_target().write(
            "def/b.lfy",
            "d B {}\nfn b(): `A b` => string { @acceptanceCriteria.add({ behavior = `It answers` }); }\n",
        );
        let local = requirement_id(&fixture, "It answers");
        // The file as the last acceptance saw it: the second region begins on line 4.
        fixture.write(
            "out/b.rs",
            &format!(
                "// @lfy def/b.lfy:B\npub struct B;\n\n// @lfy def/b.lfy:b#{local}\npub fn b() -> String {{\n    String::new()\n}}\n"
            ),
        );
        record_unit(&fixture, "b");
        // The file as it is now: two lines were added before the second marker, so it sits
        // on line 6 and the file is nine lines long.
        fixture.write(
            "out/b.rs",
            &format!(
                "// @lfy def/b.lfy:B\npub struct B;\n\n/// A note.\n/// Another.\n// @lfy def/b.lfy:b#{local}\npub fn b() -> String {{\n    String::new()\n}}\n"
            ),
        );
        let session = fixture.session();
        // @lfy def/mcp/main.lfy:output#output:output:b8c57fc09f7567f3d3edbaa76e9e713c59924bbb396a55209e302c62655135ba
        assert_eq!(
            output(&session, "out/b.rs", None, false).unwrap(),
            format!("out/b.rs:1-5 B\n\nout/b.rs:6-9 b {local}")
        );
        // A marker naming a file that is not in the program gives no region.
        // @lfy def/mcp/main.lfy:output#output:output:84421a06b6eda0b985d11eb52870e3f12014a1594070085b9dc237f580115624
        fixture.write("out/c.rs", "// @lfy def/gone.lfy:C\npub struct C;\n");
        let session = fixture.session();
        assert_eq!(
            output(&session, "out/c.rs", None, false).unwrap(),
            "nothing is generated for out/c.rs"
        );
    }

    // @lfy def/mcp/main.lfy:output#output:output:8ac14ea384bf2a5e4cb895bab8105f34dfe6124df0ef5ae67edcf3fba77a408d
    #[test]
    fn output_gives_the_regions_that_answer_for_a_requirement() {
        let fixture = project_with_requirements();
        let local = requirement_id(&fixture, "It answers");
        let global = requirement_id(&fixture, "Nothing is written outside the output directory");
        fixture.write(
            "out/b.rs",
            &format!("// @lfy def/b.lfy:b#{local}\npub fn b() -> String {{\n    String::new()\n}}\n"),
        );
        record_unit(&fixture, "b");
        let session = fixture.session();
        // The text of the criterion comes first, then the region, whose line names the
        // requirement after the entity.
        // @lfy def/mcp/main.lfy:output#output:output:8ac14ea384bf2a5e4cb895bab8105f34dfe6124df0ef5ae67edcf3fba77a408d
        assert_eq!(
            output(&session, &local, None, true).unwrap(),
            format!(
                "It answers\n\nout/b.rs:1-4 b {local}\n```rs\n// @lfy def/b.lfy:b#{local}\npub fn b() -> String {{\n    String::new()\n}}\n```"
            )
        );
        // A global id is read from the lowered program the same way; no region answers for
        // this one, so its text is followed by nothing being generated for it.
        // @lfy def/mcp/main.lfy:output#output:output:8ac14ea384bf2a5e4cb895bab8105f34dfe6124df0ef5ae67edcf3fba77a408d
        assert!(Review::is_global(&global), "{global}");
        let text = output(&session, &global, None, true).unwrap();
        assert_eq!(
            text,
            format!("Nothing is written outside the output directory\n\nnothing is generated for {global}")
        );
    }

    // The marker read is the one covering the line, and the first line of the text reads
    // its file and its line.
    // @lfy def/mcp/main.lfy:source#source:source:52459b54250cb2edeb5d2b3079e663f17b6814b8d50602682f49ab2f55bf2285
    // @lfy def/mcp/main.lfy:source#source:source:7095bd8049d91a01f9b2a650bf5e71d17bb706c9f4e6a470e004c2dd9881494e
    #[test]
    fn source_names_the_definition_a_line_came_from() {
        let fixture = accepted_project();
        let session = fixture.session();
        // The line ends with the entity the marker names.
        // @lfy def/mcp/main.lfy:source#source:source:b4c0ce9250c99f427b67858ad3e1b065b76f259ad3bf94c84085ee1551d51ab1
        assert_eq!(source(&session, "out/b.rs", 1).unwrap(), "def/b.lfy:1 B");
        assert_eq!(source(&session, "out/b.rs", 2).unwrap(), "def/b.lfy:1 B");
        // No source map has the file as its output.
        // @lfy def/mcp/main.lfy:source#source:source:3bd573d9905fc8bbb2ff9b436ef7c2affcef119a2a9cf64efa242292f4bf0299
        let text = source(&session, "out/a.rs", 1).unwrap();
        assert_eq!(text, "no source map covers out/a.rs:1");
        // A line past the region of every marker.
        // @lfy def/mcp/main.lfy:source#source:source:3bd573d9905fc8bbb2ff9b436ef7c2affcef119a2a9cf64efa242292f4bf0299
        assert_eq!(source(&session, "out/b.rs", 9).unwrap(), "no source map covers out/b.rs:9");
        // A number argument reaches the fn as a number, and what the fn returns is the
        // call's result.
        // @lfy def/mcp/main.lfy:source#source:tool:e0b14b4706f8dae10a85f002eb93be09aa1f890a3140dca03046fdc0ed52915a
        // @lfy def/mcp/main.lfy:source#source:tool:09d86a7ebefb49c93cedccbd396bfe51c1fd158af3e4aea4f45843eebd4da3a2
        let result = call(&session, "elfie_source", serde_json::json!({ "file": "out/b.rs", "line": 2 }));
        assert!(!result.is_error, "{}", result.text);
        assert_eq!(result.text, "def/b.lfy:1 B");
        // An argument of the wrong type is an error naming it, and the fn is not called.
        // @lfy def/mcp/main.lfy:source#source:tool:6d9dd51635b7204772d74da138f616b326aac0af4536ec450e8f26c7c9650de0
        // @lfy def/mcp/main.lfy:source#source:tool:f64e48f070058897b4b82d2d2649c81b9155f184931f4b71a81c2bb9163b850a
        let result = call(&session, "elfie_source", serde_json::json!({ "file": "out/b.rs", "line": "2" }));
        assert!(result.is_error);
        assert!(result.text.contains("line") && result.text.contains("number"), "{}", result.text);
    }

    /// The id of the one criterion or test of a project spelled as `text`.
    fn requirement_id(fixture: &Fixture, text: &str) -> String {
        let program = interpret::lower(fixture.session().workspace);
        requirements_of(&program)
            .into_iter()
            .find(|(_, spelled)| spelled == text)
            .map(|(id, _)| id)
            .unwrap_or_else(|| panic!("the program holds a requirement reading {text}"))
    }

    /// A project with one fn carrying one local criterion and one global criterion.
    fn project_with_requirements() -> Fixture {
        let fixture = Fixture::new();
        fixture
            .with_rust_target()
            .write(
                "def/b.lfy",
                "fn b(): `A b` => string { @acceptanceCriteria.add({ behavior = `It answers` }); }\nglobal@acceptanceCriteria.add({ behavior = `Nothing is written outside the output directory` });\n",
            );
        fixture
    }

    // @lfy def/mcp/main.lfy:source#source:source:bab27e2f0ed21f719d22e545c1c874a283b18a0ff5a8979b2a8bf9d324519b11
    #[test]
    fn source_names_the_requirement_a_region_answers_for() {
        let fixture = project_with_requirements();
        let local = requirement_id(&fixture, "It answers");
        fixture.write(
            "out/b.rs",
            &format!("// @lfy def/b.lfy:b#{local}\npub fn b() -> String {{\n    String::new()\n}}\n"),
        );
        let session = fixture.session();
        assert_eq!(problems(&session, None).unwrap(), "no problems");
        record_unit(&fixture, "b");
        // The marker names a requirement, so its id follows on the line after the one
        // reading the file and the line, and then the text of that criterion.
        // @lfy def/mcp/main.lfy:source#source:source:bab27e2f0ed21f719d22e545c1c874a283b18a0ff5a8979b2a8bf9d324519b11
        let session = fixture.session();
        assert_eq!(
            source(&session, "out/b.rs", 2).unwrap(),
            format!("def/b.lfy:1 b\n{local}\nIt answers")
        );
    }

    /// A region may answer for a global criterion as well as a local one; its text is read
    /// from the lowered program the same way.
    // @lfy def/mcp/main.lfy:source#source:source:bab27e2f0ed21f719d22e545c1c874a283b18a0ff5a8979b2a8bf9d324519b11
    #[test]
    fn source_reads_a_global_requirement_from_the_lowered_program() {
        let fixture = project_with_requirements();
        let global = requirement_id(&fixture, "Nothing is written outside the output directory");
        assert!(Review::is_global(&global), "{global}");
        fixture.write(
            "out/b.rs",
            &format!("// @lfy def/b.lfy:b#{global}\npub fn b() -> String {{\n    String::new()\n}}\n"),
        );
        record_unit(&fixture, "b");
        let session = fixture.session();
        // @lfy def/mcp/main.lfy:source#source:source:bab27e2f0ed21f719d22e545c1c874a283b18a0ff5a8979b2a8bf9d324519b11
        assert_eq!(
            source(&session, "out/b.rs", 1).unwrap(),
            format!("def/b.lfy:1 b\n{global}\nNothing is written outside the output directory")
        );
    }

    // @lfy def/mcp/main.lfy:changes#changes:changes:21683ec505359b2effe3c7d8bc5d8fbcc63e581f01406ec3948d214f74916b65
    #[test]
    fn changes_reports_what_differs_since_the_outputs_were_accepted() {
        let fixture = accepted_project();
        let session = fixture.session();
        // The unit has outputs and the previous text cannot be recovered: the text says so
        // first, then every entity as added.
        // @lfy def/mcp/main.lfy:changes#changes:changes:fc5534487756c5701f11c4c00be57babd85dc1275ba585942da27893626d07f4
        let text = changes(&session, "b", None).unwrap();
        assert!(text.starts_with("the source the outputs were generated from cannot be recovered"), "{text}");
        assert!(text.contains("\nB added "), "{text}");
        // The target may be named, and names the same unit.
        // @lfy def/mcp/main.lfy:changes#changes:changes:21683ec505359b2effe3c7d8bc5d8fbcc63e581f01406ec3948d214f74916b65
        assert_eq!(changes(&session, "b", Some("rust")).unwrap(), text);
        // The outputs of a unit are empty.
        // @lfy def/mcp/main.lfy:changes#changes:changes:413343511b79d06a29fa38852b4bf26d600fdb0e411965a13d3190ca9375546e
        let text = changes(&session, "a", None).unwrap();
        assert!(text.starts_with("no previous output for a"), "{text}");
        // No unit has that stem.
        // @lfy def/mcp/main.lfy:changes#changes:changes:bbfd13869e5d8aa4ccdda753d25b4797e5140bfb3b5f287cf8f83310a3b51e7b
        let error = changes(&session, "zzz", None).unwrap_err();
        assert!(error.contains("no unit has the stem zzz"), "{error}");
        let error = changes(&session, "zzz", Some("rust")).unwrap_err();
        assert!(error.contains("no unit of the target rust has the stem zzz"), "{error}");
        // The target is set and is not a known target.
        // @lfy def/mcp/main.lfy:changes#changes:changes:e2e8756b78db77377231d8ef5ee9fd3e078703c801906a9b774ee59a2135a3ab
        let error = changes(&session, "b", Some("go")).unwrap_err();
        assert!(error.contains("go is not a target") && error.contains("rust"), "{error}");
    }

    /// The root is a repository holding the source the outputs were accepted against, so
    /// that source is the previous text: the revision is found by the hash of its lowered
    /// text, which is what the source map records, and the entities added since are the
    /// only changes.
    // @lfy def/mcp/main.lfy:changes#changes:changes:6fdf72883c3fb05ab2a58a8325708fe690fc56fcfe75d909abff5f56efdb18e1
    #[test]
    fn changes_recovers_the_previous_source_from_git() {
        let fixture = accepted_project();
        fixture
            .git(&["init", "--quiet"])
            .git(&["add", "-A"])
            .git(&[
                "-c",
                "user.email=p@example.com",
                "-c",
                "user.name=p",
                "commit",
                "--quiet",
                "-m",
                "the accepted source",
            ]);
        // The revision is found although the file on disk now carries a comment the
        // committed one did not, because it is matched by the hash of its lowered text and
        // a comment is not lowered.
        // @lfy def/mcp/main.lfy:changes#changes:changes:6fdf72883c3fb05ab2a58a8325708fe690fc56fcfe75d909abff5f56efdb18e1
        fixture.write("def/b.lfy", "// a comment\nd B {}\nd C {}\n");
        let session = fixture.session();
        // What is passed on is that revision as git holds it, never lowered: `git show` of
        // the revision the map's hash names.
        // @lfy def/mcp/main.lfy:changes#changes:changes:6fdf72883c3fb05ab2a58a8325708fe690fc56fcfe75d909abff5f56efdb18e1
        let planned = plan_of(&session);
        let index = unit_of_stem(planned.workspace(), &planned.plan, "b", None).unwrap();
        let recovered = previous_source(planned.workspace(), &planned.plan.units[index]).unwrap();
        assert_eq!(recovered, "d B {}\n");
        // @lfy def/mcp/main.lfy:changes#changes:changes:6fdf72883c3fb05ab2a58a8325708fe690fc56fcfe75d909abff5f56efdb18e1
        let text = changes(&session, "b", None).unwrap();
        assert!(!text.contains("cannot be recovered"), "{text}");
        // `B` was declared in the recovered source, so only `C` is an addition.
        // @lfy def/mcp/main.lfy:changes#changes:changes:6fdf72883c3fb05ab2a58a8325708fe690fc56fcfe75d909abff5f56efdb18e1
        assert!(!text.contains("B added"), "{text}");
        assert!(text.lines().any(|line| line.starts_with("C added ")), "{text}");
    }

    // @lfy def/mcp/main.lfy:changes#changes:changes:35a395b2402b76e8ae9d6c636a82478d28714ed6e6459fe96e70847ff73470a4
    #[test]
    fn changes_says_so_when_nothing_differs() {
        let fixture = accepted_project();
        let session = fixture.session();
        let planned = plan_of(&session);
        let index = unit_of_stem(planned.workspace(), &planned.plan, "b", None).unwrap();
        // The file as it is now is the file the outputs were generated from, so nothing
        // differs.
        // @lfy def/mcp/main.lfy:changes#changes:changes:35a395b2402b76e8ae9d6c636a82478d28714ed6e6459fe96e70847ff73470a4
        let text = fs::read_to_string(fixture.root.join("def/b.lfy")).unwrap();
        assert!(generation::changes(&planned.program, &planned.plan.units[index], Some(&text)).is_empty());
    }

    // @lfy def/mcp/main.lfy:review#review:review:f58d35f9aea99f8ea42d7e215a8e84d42b2b414afa745217d99622b1a66e5c0d
    #[test]
    fn review_gives_the_review_request_of_one_batch() {
        let fixture = project_with_requirements();
        let session = fixture.session();
        // The request is made with no review record, so every criterion of the batch is
        // asked about however an earlier review found it.
        // @lfy def/mcp/main.lfy:review#review:review:f58d35f9aea99f8ea42d7e215a8e84d42b2b414afa745217d99622b1a66e5c0d
        let instructions = review(&session, "b", None).unwrap();
        assert!(instructions.contains("Reviewing the batch"), "{instructions}");
        assert!(instructions.contains("It answers"), "{instructions}");
        assert_eq!(review(&session, "b", Some("rust")).unwrap(), instructions);
        // No batch has that identifier or holds a unit with that stem, or the target is
        // ambiguous.
        // @lfy def/mcp/main.lfy:review#review:review:ca0a25d6a947aa41d3cbf9399a371f681224987d6109d85e16a0b7f8a85db14c
        let error = review(&session, "c", None).unwrap_err();
        assert!(error.contains("no batch has the identifier c"), "{error}");
        let error = review(&session, "b", Some("go")).unwrap_err();
        assert!(error.contains("go is not a target"), "{error}");
        // A batch whose units carry no criterion and no test needs no review; the batch is
        // still named by its identifier or by the stem of one of its units.
        let bare = compiled_project();
        let session = bare.session();
        let instructions = review(&session, "b+1", None).unwrap();
        assert!(instructions.contains("Reviewing the batch"), "{instructions}");
        assert_eq!(review(&session, "a", None).unwrap(), instructions);
    }

    /// A binding for a role, bound to the batch of a fixture with an identifier or, with
    /// none named, to the global review. A verifier's binding carries the review request
    /// that verifier would have been run with.
    fn binding_of(fixture: &Fixture, role: Role, batch: Option<&str>) -> Binding {
        let session = fixture.session();
        let planned = plan_of(&session);
        let named = batch.map(|name| {
            planned
                .plan
                .batches
                .iter()
                .find(|one| one.identifier == name)
                .cloned()
                .unwrap_or_else(|| panic!("a batch named {name}"))
        });
        let review = match (role, &named) {
            (Role::Verifier, Some(batch)) => Some(generation::review(
                &planned.program,
                &planned.plan,
                batch,
                &planned.maps,
                None,
                &[],
            )),
            _ => None,
        };
        // A compiler run is given the request the compile made for its batch, as the CLI
        // gives it one.
        // @lfy def/mcp/main.lfy:request#request:request:af5eea7b402d3ed59ce0541ca71250ff92ea2e142a768f4b98af9e9bc4f348c9
        let request = match (role, &named) {
            (Role::Compiler, Some(batch)) => Some(generation::request(
                &planned.program,
                &planned.plan,
                batch,
                &[],
                &BTreeMap::new(),
                true,
            )),
            _ => None,
        };
        Binding {
            role,
            batch: named,
            directory: fixture.root.clone(),
            plan: planned.plan.clone(),
            program: planned.program.clone(),
            request,
            review,
            source_maps: Vec::new(),
        }
    }

    /// A session bound to a binding, as a host opens one.
    fn bound(fixture: &Fixture, binding: Binding) -> Session {
        let mut workspace = binding.program.workspace.clone();
        workspace.root = fixture.root.clone();
        Session {
            workspace,
            seen: BTreeMap::new(),
            binding: Some(binding),
            reviews: Mutex::new(Vec::new()),
            submission: Mutex::new(None),
        }
    }

    /// A host's own state, with a URL of its own and no session open.
    fn hosting_of(url: &str) -> Hosting {
        Hosting {
            url: url.to_string(),
            token: "a".repeat(TOKEN_BYTES * 2),
            sessions: Mutex::new(BTreeMap::new()),
            opened: AtomicUsize::new(0),
            stopping: tokio::sync::Notify::new(),
        }
    }

    /// A call of a tool by name, as the protocol carries it.
    fn calling(name: &str, arguments: Value) -> CallToolRequestParams {
        serde_json::from_value(serde_json::json!({ "name": name, "arguments": arguments })).expect("a call")
    }

    /// The text of what a call answered, and whether it is an error.
    fn answered(result: &CallToolResult) -> (bool, String) {
        let value = serde_json::to_value(result).expect("a result");
        (
            value["isError"] == serde_json::json!(true),
            value["content"][0]["text"].as_str().unwrap_or_default().to_string(),
        )
    }

    /// Each session a host opens is given a number no earlier one had, is bound to what it
    /// was opened with, reads its workspace at the binding's directory without re-reading a
    /// file of the program, and is offered the tools of its role; closing it gives back what
    /// `elfie_report` recorded in it, and a URL that is no open session's gives nothing.
    // @lfy def/mcp/main.lfy:host#host:host:465078bd14c4c2971c8ae047a59db580f64b30e5fa762af2e656c4accbede7ba
    // @lfy def/mcp/main.lfy:host#host:host:238a67527a5f81bdc05abb6515eacc7b86b9ead269c56d3b3654d34d47184b50
    // @lfy def/mcp/main.lfy:host#host:host:b01f7a894faf3752cba01a91aaed7228df1bb1b93d87ea2c648f034427bcc3fd
    // @lfy def/mcp/main.lfy:host#host:host:bb7c6b5258b2c4026015a8f9915717da1ec50aa40dcc10a43e5b50bc9e39310e
    // @lfy def/mcp/main.lfy:host#host:host:5e071f6dac08ca55d36f2e5b605c6cec0298a9dfbc6b6128864a422ec69e53d3
    // @lfy def/mcp/main.lfy:host#host:host:7afa5703849ae259b65ae3bd7d32057445477122e6db4ac372c99154d461ea95
    // @lfy def/mcp/main.lfy:host#host:host:91b44b23c29eaf7242b190da7fe5ab5f3642df5ec77212e680331af6b76e01b1
    // @lfy def/mcp/main.lfy:host#host:host:64c0df55947a7e7a600bb0796c4715f740afcfda41cdd6f86b4cb408c4705713
    // @lfy def/mcp/main.lfy:host#host:host:8eae0f57be7e371560c08488ce9a3c25a5eb3254d63aa6f4fcd5f317786e93b6
    #[test]
    fn a_hosted_session_is_bound_numbered_and_closed_with_what_it_recorded() {
        let fixture = compiled_project();
        let hosting = hosting_of("http://127.0.0.1:7/");
        // @lfy def/mcp/main.lfy:host#host:host:465078bd14c4c2971c8ae047a59db580f64b30e5fa762af2e656c4accbede7ba
        let first = hosting.open(binding_of(&fixture, Role::Compiler, Some("b+1")));
        assert_eq!(first, "http://127.0.0.1:7/sessions/1");
        let second = hosting.open(binding_of(&fixture, Role::Verifier, Some("b+1")));
        assert_eq!(second, "http://127.0.0.1:7/sessions/2");

        let compiling = hosting.session("/sessions/1").expect("the first session");
        let verifying = hosting.session("/sessions/2").expect("the second session");
        // A compiler is offered every tool carrying `tool` and not `commandTool` but
        // `report`, the fns carrying `compilerTool` among them; a verifier is offered every
        // tool carrying `tool` alone but `check`. The fixture's target has no command, so
        // neither is offered a command tool.
        // @lfy def/mcp/main.lfy:host#host:host:5e071f6dac08ca55d36f2e5b605c6cec0298a9dfbc6b6128864a422ec69e53d3
        assert_eq!(compiling.offered, COMPILER_TOOLS);
        // @lfy def/mcp/main.lfy:host#host:host:7afa5703849ae259b65ae3bd7d32057445477122e6db4ac372c99154d461ea95
        assert_eq!(verifying.offered, VERIFIER_TOOLS);
        assert!(verifying.offers("elfie_report"), "{:?}", verifying.offered);
        assert!(!verifying.offers("elfie_check"), "{:?}", verifying.offered);
        // `Host.offered` gives the names of the tools the session at a URL is offered, in
        // the order the server lists them; a URL that is no open session's gives none.
        // @lfy def/mcp/main.lfy:host#host:host:d3b3abd60412e83fb1835597e514740cc7d4648f94a73575cf7f076399063025
        assert_eq!(hosting.offered(&first), COMPILER_TOOLS);
        assert_eq!(hosting.offered(&second), VERIFIER_TOOLS);
        // @lfy def/mcp/main.lfy:host#host:host:1f6860e467f9f7b17993f762e5ba4521688af80851fff707da5bbb18f86cdbba
        assert!(hosting.offered("http://127.0.0.1:7/sessions/9").is_empty());
        assert!(hosting.offered("http://elsewhere/sessions/1").is_empty());
        // `check` is not offered to it, so it is not listed either.
        // @lfy def/mcp/main.lfy:check#check:tool:d1b4319d34a3d00324ec0850bf1e0987ceb4112aab80f13d77b82554c27ca5f9
        assert!(!verifying.list().tools.iter().any(|tool| tool.name == "elfie_check"));
        // Each session's instructions name only the tools that session is offered, say which
        // of them change files, and say which role it is bound to. The fixture's target has
        // no command, so a verifier is offered nothing that changes a file.
        // @lfy def/mcp/main.lfy:host#host:host:eeef9359197b88bf6fc69027d5771f23a22ac670ed51c8455dd7ddc9ce90c2b6
        for (served, offered, withheld, role) in [
            (&compiling, COMPILER_TOOLS.as_slice(), "elfie_report", "compiler"),
            (&verifying, VERIFIER_TOOLS.as_slice(), "elfie_check", "verifier"),
        ] {
            let said = &served.instructions;
            for name in offered {
                assert!(said.contains(name), "{said}");
            }
            assert!(!said.contains(withheld), "{said}");
            assert!(said.contains(&format!("bound to a {role} run")), "{said}");
        }
        // @lfy def/mcp/main.lfy:host#host:host:eeef9359197b88bf6fc69027d5771f23a22ac670ed51c8455dd7ddc9ce90c2b6
        assert!(compiling.instructions.contains("elfie_mark writes a marker line"), "{}", compiling.instructions);
        assert!(verifying.instructions.contains("None of them changes a file."), "{}", verifying.instructions);
        {
            let session = compiling.session.lock().unwrap();
            let binding = session.binding.as_ref().expect("a binding");
            // @lfy def/mcp/main.lfy:host#host:host:238a67527a5f81bdc05abb6515eacc7b86b9ead269c56d3b3654d34d47184b50
            assert_eq!(binding.role, Role::Compiler);
            assert_eq!(binding.batch.as_ref().map(|batch| batch.identifier.as_str()), Some("b+1"));
            // @lfy def/mcp/main.lfy:host#host:host:b01f7a894faf3752cba01a91aaed7228df1bb1b93d87ea2c648f034427bcc3fd
            assert_eq!(session.workspace.root, fixture.root);
            assert_eq!(session.workspace.files.len(), binding.program.workspace.files.len());
            // No file of the program is stamped, so no call re-reads one.
            // @lfy def/mcp/main.lfy:host#host:host:bb7c6b5258b2c4026015a8f9915717da1ec50aa40dcc10a43e5b50bc9e39310e
            assert!(session.seen.is_empty());
            // @lfy def/mcp/main.lfy:host#host:host:91b44b23c29eaf7242b190da7fe5ab5f3642df5ec77212e680331af6b76e01b1
            assert!(session.reviews.lock().unwrap().is_empty());
            // @lfy def/mcp/main.lfy:host#host:host:5e5526603ec0ccf1f819bfc584b556e779b3a2a8f9d0f46fdc9b92ad91bb9dc5
            assert!(session.submission.lock().unwrap().is_none());
        }
        // A call naming a tool the session is not offered is an error saying so, and the fn
        // is not reached: nothing is checked and no plan is made.
        // @lfy def/mcp/main.lfy:check#check:tool:171e2f1ce748190f5ccbf577a5f91fc2fd8683a6b877bb654a16a48965e01265
        // @lfy def/mcp/main.lfy:check#check:tool:15fd0657dd9bfbe4565283f531743c14447968c8361b95ba4e874d2a8309c888
        let refused = verifying
            .call(&calling("elfie_check", serde_json::json!({ "unit": "b" })))
            .expect("a result");
        let (failed, text) = answered(&refused);
        assert!(failed, "{text}");
        assert_eq!(text, "elfie_check is not offered to this session");
        // A tool the session is offered is reached as ever.
        // @lfy def/mcp/main.lfy:units#units:tool:c0392f44087e7b29669f0f8d6b5d294cb0195675616839b4f6d80e791125f939
        let listed = compiling.call(&calling("elfie_units", serde_json::json!({}))).expect("a result");
        let (failed, text) = answered(&listed);
        assert!(!failed, "{text}");
        assert!(text.contains("rust b"), "{text}");

        // A compiler's session records what `elfie_submit` submitted, and closing it gives
        // that back with its reviews and leaves it no longer open.
        // @lfy def/mcp/main.lfy:host#host:host:64c0df55947a7e7a600bb0796c4715f740afcfda41cdd6f86b4cb408c4705713
        let submitted = compiling
            .call(&calling(
                "elfie_submit",
                serde_json::json!({ "outcome": "done", "message": "all four written" }),
            ))
            .expect("a result");
        let (failed, text) = answered(&submitted);
        assert!(!failed, "{text}");
        let record = hosting.close(&first);
        assert!(record.reviews.is_empty());
        assert_eq!(
            record.submission.as_ref().map(|one| (one.outcome, one.message.as_str())),
            Some((SubmittedOutcome::Done, "all four written"))
        );
        assert!(hosting.session("/sessions/1").is_none());
        // A URL that is no open session's gives a record holding nothing.
        // @lfy def/mcp/main.lfy:host#host:host:8eae0f57be7e371560c08488ce9a3c25a5eb3254d63aa6f4fcd5f317786e93b6
        for url in [first.as_str(), "http://127.0.0.1:7/sessions/9", "http://elsewhere/sessions/2"] {
            let record = hosting.close(url);
            assert!(record.reviews.is_empty() && record.submission.is_none(), "{url}");
        }
        assert!(hosting.session("/sessions/2").is_some());
    }

    /// One HTTP request to a URL on the loopback address, with a bearer token when one is
    /// given: the status and the body.
    fn http(url: &str, token: Option<&str>, body: &str) -> (u16, String) {
        use std::io::{Read, Write};
        let rest = url.strip_prefix("http://").expect("an http url");
        let (authority, path) = match rest.find('/') {
            Some(at) => (&rest[..at], &rest[at..]),
            None => (rest, "/"),
        };
        let mut stream = std::net::TcpStream::connect(authority).expect("the host listens");
        let mut request = format!(
            "POST {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\
             Accept: application/json, text/event-stream\r\nContent-Type: application/json\r\n"
        );
        if let Some(token) = token {
            request.push_str(&format!("Authorization: Bearer {token}\r\n"));
        }
        request.push_str(&format!("Content-Length: {}\r\n\r\n{body}", body.len()));
        stream.write_all(request.as_bytes()).unwrap();
        stream.flush().unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).unwrap();
        let raw = String::from_utf8_lossy(&raw).into_owned();
        let status = raw
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap_or_else(|| panic!("an HTTP answer, not {raw:?}"));
        let body = raw.split_once("\r\n\r\n").map(|(_, body)| body.to_string()).unwrap_or_default();
        (status, body)
    }

    /// The whole host, driven over HTTP: it listens on the loopback address at a port the
    /// operating system picks, behind a token of its own; a request without the token is
    /// unauthorized, one at a path no session is open at is not found, and one at a
    /// session's URL speaks the protocol whatever MCP session id it carries. The port is no
    /// longer listened on once `stop` returns.
    // @lfy def/mcp/main.lfy:host#host:host:d17c6dc3479336c65543a2c99389c29582860e9358d65bd4e36a3d9c355e7825
    // @lfy def/mcp/main.lfy:host#host:host:ea1cfd81c073494e7f7377472fc3070be780e4455e629f59e365d21d658269d4
    // @lfy def/mcp/main.lfy:host#host:host:c0b5fd0495fb73ed8d21e6599998b9acc5a051a0dde254bb492f14f26417fade
    // @lfy def/mcp/main.lfy:host#host:host:246a5aacda8ffe241aedd0b00fec70b3f76a7c6220141f54731dcb8097f58852
    // @lfy def/mcp/main.lfy:host#host:host:b20b250482898ccb1326689317f112cb544e9941949596a5c823ce56f4d692b9
    // @lfy def/mcp/main.lfy:host#host:host:f876706edb4133d6c7e2e9d4f43bf7b7cc00c92572911e227d0c789a95636e5d
    // @lfy def/mcp/main.lfy:host#host:host:c0519927a29b45c6d3ed0bf600d8dd929353a41c1678e7be62c950db70763d79
    // @lfy def/mcp/main.lfy:host#host:host:e07851ea0cedfcd8e3d913fa15d72b1c389e0d5f87edfc1f52b7841decccbd1e
    #[test]
    fn the_host_speaks_the_protocol_over_http_behind_its_token() {
        /// What a call of `tools/list` reads.
        const LISTING: &str = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#;
        let fixture = compiled_project();
        let host = host().expect("a host");
        // The loopback address, a colon, the port the operating system picked, and a slash.
        // @lfy def/mcp/main.lfy:host#host:host:ea1cfd81c073494e7f7377472fc3070be780e4455e629f59e365d21d658269d4
        // @lfy def/mcp/main.lfy:host#host:host:c0b5fd0495fb73ed8d21e6599998b9acc5a051a0dde254bb492f14f26417fade
        let address = host.url.strip_prefix("http://").and_then(|rest| rest.strip_suffix('/')).unwrap_or_default();
        let (name, port) = address.split_once(':').unwrap_or_else(|| panic!("{}", host.url));
        assert_eq!(name, LOOPBACK);
        assert_ne!(port.parse::<u16>().unwrap_or(0), 0, "{}", host.url);
        // 64 lowercase hex digits, new for each call of `host`.
        // @lfy def/mcp/main.lfy:host#host:host:246a5aacda8ffe241aedd0b00fec70b3f76a7c6220141f54731dcb8097f58852
        assert_eq!(host.token.len(), TOKEN_BYTES * 2);
        assert!(
            host.token.chars().all(|digit| digit.is_ascii_hexdigit() && !digit.is_ascii_uppercase()),
            "{}",
            host.token
        );
        let other = crate::host().expect("a second host");
        assert_ne!(other.token, host.token);
        assert_ne!(other.url, host.url);
        (other.stop)();

        let url = (host.open)(binding_of(&fixture, Role::Compiler, Some("b+1")));
        // No token: 401, and nothing of the protocol is answered, so no tool ran.
        // @lfy def/mcp/main.lfy:host#host:host:b20b250482898ccb1326689317f112cb544e9941949596a5c823ce56f4d692b9
        let (status, body) = http(&url, None, LISTING);
        assert_eq!(status, 401, "{body}");
        assert!(!body.contains("jsonrpc"), "{body}");
        // The token, at a path no session is open at: 404, and no tool ran for it either.
        // @lfy def/mcp/main.lfy:host#host:host:f876706edb4133d6c7e2e9d4f43bf7b7cc00c92572911e227d0c789a95636e5d
        let (status, body) = http(&format!("{}sessions/9", host.url), Some(&host.token), LISTING);
        assert_eq!(status, 404, "{body}");
        assert!(!body.contains("jsonrpc"), "{body}");

        // The session's URL, under no MCP session id at all: the protocol is spoken, and
        // the listing is the session's.
        // @lfy def/mcp/main.lfy:host#host:host:d17c6dc3479336c65543a2c99389c29582860e9358d65bd4e36a3d9c355e7825
        // @lfy def/mcp/main.lfy:host#host:host:c0519927a29b45c6d3ed0bf600d8dd929353a41c1678e7be62c950db70763d79
        let (status, body) = http(&url, Some(&host.token), LISTING);
        assert_eq!(status, 200, "{body}");
        let listed: Value =
            serde_json::from_str(&body).unwrap_or_else(|error| panic!("a protocol answer, not {body:?}: {error}"));
        let served: Vec<&str> = listed["result"]["tools"]
            .as_array()
            .unwrap_or_else(|| panic!("a list of tools, not {listed}"))
            .iter()
            .map(|tool| tool["name"].as_str().expect("a tool name"))
            .collect();
        assert_eq!(served, COMPILER_TOOLS);
        // The same session answers at its URL whatever the request carries, so `Host.offered`
        // names the very tools it listed.
        // @lfy def/mcp/main.lfy:host#host:host:d3b3abd60412e83fb1835597e514740cc7d4648f94a73575cf7f076399063025
        assert_eq!((host.offered)(&url), COMPILER_TOOLS);
        // @lfy def/mcp/main.lfy:units#units:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
        assert_eq!(listed["result"]["ttlMs"], serde_json::json!(0), "{listed}");
        assert_eq!(listed["result"]["cacheScope"], "private", "{listed}");

        // A call of a tool the session is offered answers with what the fn returns.
        // @lfy def/mcp/main.lfy:units#units:tool:59e3afe0a4f39564b82a347fa5494defa7421ae95a945c19049ed8e307b1b0e8
        let called = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"elfie_units","arguments":{}}}"#;
        let (status, body) = http(&url, Some(&host.token), called);
        assert_eq!(status, 200, "{body}");
        let called: Value = serde_json::from_str(&body).unwrap_or_else(|error| panic!("{body:?}: {error}"));
        let text = called["result"]["content"][0]["text"].as_str().unwrap_or_default();
        assert!(text.contains("rust b"), "{called}");

        // Stop: every session is ended and the port is no longer listened on.
        // @lfy def/mcp/main.lfy:host#host:host:e07851ea0cedfcd8e3d913fa15d72b1c389e0d5f87edfc1f52b7841decccbd1e
        let address = address.to_string();
        (host.stop)();
        assert!(std::net::TcpStream::connect(&address).is_err(), "{address}");
    }

    /// The source maps a verifier's review was made with stand in for what the last
    /// acceptance recorded, so the verifier reads the outputs it is judging.
    // @lfy def/mcp/main.lfy:host#host:host:6f8405c8e6f3e4591069a7d71ce8930df3cd097e586d761bdaa8bb9a2df6ce2e
    #[test]
    fn the_source_maps_of_a_review_stand_in_for_the_recorded_ones() {
        let fixture = accepted_project();
        let plain = fixture.session();
        let recorded = source_maps_of(&plain, None);
        let found = recorded
            .iter()
            .find(|map| map.output == "out/b.rs")
            .unwrap_or_else(|| panic!("a recorded map, not {recorded:?}"))
            .clone();
        // A map of the same target and the same source as a recorded one stands in for it.
        // @lfy def/mcp/main.lfy:host#host:host:6f8405c8e6f3e4591069a7d71ce8930df3cd097e586d761bdaa8bb9a2df6ce2e
        let mut given = found.clone();
        given.output = "out/elsewhere.rs".to_string();
        let mut binding = binding_of(&fixture, Role::Verifier, None);
        binding.source_maps = vec![given];
        let session = bound(&fixture, binding);
        let read = source_maps_of(&session, None);
        assert!(read.iter().any(|map| map.output == "out/elsewhere.rs"), "{read:?}");
        assert!(!read.iter().any(|map| map.output == "out/b.rs"), "{read:?}");
        assert_eq!(read.len(), recorded.len());
        // With no map carried, what the last acceptance recorded is all that is read.
        // @lfy def/mcp/main.lfy:host#host:host:232c0725ae5219330822ed9664cc87be38c4591303cbc3dead664044e09b75eb
        let session = bound(&fixture, binding_of(&fixture, Role::Verifier, None));
        assert_eq!(source_maps_of(&session, None), recorded);
    }

    /// In a compiler's session the map of one of its own units is read from the markers its
    /// output holds now, while every other member of it is the recorded map's, so the unit
    /// still plans, changes, and requests as the compile planned it.
    // @lfy def/mcp/main.lfy:host#host:host:3e562832ea4cdd3ed23d67fe79f9a9024f6c42e802d43f5e68dde2914e23c677
    #[test]
    fn a_compilers_own_source_map_keeps_what_the_recorded_one_holds() {
        let fixture = Fixture::new();
        fixture.with_rust_target().write(
            "def/b.lfy",
            "fn b(): `A b` => string { @acceptanceCriteria.add({ behavior = `It answers` }); }\n",
        );
        let local = requirement_id(&fixture, "It answers");
        let accepted =
            format!("// @lfy def/b.lfy:b#{local}\npub fn b() -> String {{\n    String::new()\n}}\n");
        fixture.write("out/b.rs", &accepted);
        record_unit(&fixture, "b");
        let of_output = |maps: Vec<SourceMap>| {
            maps.into_iter()
                .find(|map| map.output == "out/b.rs")
                .unwrap_or_else(|| panic!("a map of out/b.rs"))
        };
        let recorded = of_output(source_maps_of(&fixture.session(), None));
        assert_eq!(recorded.markers.first().map(|marker| marker.output_line), Some(1));
        assert!(!recorded.hash.is_empty() && recorded.answered == vec![local.clone()], "{recorded:?}");
        // The output as it is now: a line was added before the marker, so its region begins
        // one line later than the recorded map says.
        fixture.write("out/b.rs", &format!("// A note.\n{accepted}"));
        // The unit is up to date, so the plan batches nothing; a compiler run of it is bound
        // to a batch of its own.
        let planned = plan_of(&fixture.session());
        let of_b = (0..planned.plan.units.len())
            .find(|&unit| planned.plan.units[unit].stem == "b")
            .expect("the unit b");
        let session = bound(
            &fixture,
            Binding {
                role: Role::Compiler,
                batch: Some(Batch { units: vec![of_b], identifier: "b".to_string() }),
                directory: fixture.root.clone(),
                plan: planned.plan.clone(),
                program: planned.program.clone(),
                request: None,
                review: None,
                source_maps: Vec::new(),
            },
        );
        let live = of_output(source_maps_of(&session, None));
        // @lfy def/mcp/main.lfy:host#host:host:3e562832ea4cdd3ed23d67fe79f9a9024f6c42e802d43f5e68dde2914e23c677
        assert_eq!(live.markers.first().map(|marker| marker.output_line), Some(2));
        assert_ne!(live.markers, recorded.markers);
        // Nothing else differs: with the recorded markers put back, the two maps are one.
        // @lfy def/mcp/main.lfy:host#host:host:3e562832ea4cdd3ed23d67fe79f9a9024f6c42e802d43f5e68dde2914e23c677
        let carried = SourceMap { markers: recorded.markers.clone(), ..live };
        assert_eq!(carried, recorded);
    }

    /// A hosted session acts on its own batch and on no other: a request, a check, a
    /// change, or a review for another is an error naming the batch it acts on, or the
    /// global review when it is bound to none.
    // @lfy def/mcp/main.lfy:request#request:request:f7ef0e526d107bc01f393317930dc6691e83904a9ec731aef3e304884867cfe0
    // @lfy def/mcp/main.lfy:check#check:check:9c073d6b09e272ea0d5a712167c00378477b35e103dad16445f0d53b88d7bc30
    // @lfy def/mcp/main.lfy:changes#changes:changes:e97791dbc253b4037be4b5a6a54aa430027a02f99710070c689449608a3affe1
    // @lfy def/mcp/main.lfy:review#review:review:0dcb77dad8ef4db5102c1bd7eeaec630f239dc244db5eee8132f8c57d77677ea
    #[test]
    fn a_hosted_session_acts_only_on_its_own_batch() {
        let fixture = compiled_project();
        let plain = fixture.session();
        let planned = plan_of(&plain);
        // The fixture batches both units as `b+1`; a binding to a batch of its own holding
        // only `a` leaves the unit `b` and the batch `b+1` outside it.
        let of_a = (0..planned.plan.units.len())
            .find(|&unit| planned.plan.units[unit].stem == "a")
            .expect("the unit a");
        let mut binding = binding_of(&fixture, Role::Compiler, Some("b+1"));
        binding.batch = Some(Batch {
            units: vec![of_a],
            identifier: "a".to_string(),
        });
        let session = bound(&fixture, binding);
        // @lfy def/mcp/main.lfy:request#request:request:f7ef0e526d107bc01f393317930dc6691e83904a9ec731aef3e304884867cfe0
        let error = request(&session, "b+1", None).unwrap_err();
        assert!(error.contains("acts only on its own batch, a"), "{error}");
        // @lfy def/mcp/main.lfy:check#check:check:9c073d6b09e272ea0d5a712167c00378477b35e103dad16445f0d53b88d7bc30
        let error = check(&session, "b", None).unwrap_err();
        assert!(error.contains("acts only on its own batch, a"), "{error}");
        // @lfy def/mcp/main.lfy:changes#changes:changes:e97791dbc253b4037be4b5a6a54aa430027a02f99710070c689449608a3affe1
        let error = changes(&session, "b", None).unwrap_err();
        assert!(error.contains("acts only on its own batch, a"), "{error}");
        // @lfy def/mcp/main.lfy:review#review:review:0dcb77dad8ef4db5102c1bd7eeaec630f239dc244db5eee8132f8c57d77677ea
        let error = review(&session, "b+1", None).unwrap_err();
        assert!(error.contains("acts only on its own batch, a"), "{error}");
        // Its own unit is answered for as ever: the guard lets it through and the verdict
        // of `accept` is what comes back.
        // @lfy def/mcp/main.lfy:check#check:check:2a16ea176547468dbe87eb74de7c195dc58e16c26122fc69de5f2ff15dd469f1
        let error = check(&session, "a", None).unwrap_err();
        assert!(error.starts_with("rejected a:"), "{error}");

        // A session bound to the global review acts on no batch at all.
        // @lfy def/mcp/main.lfy:review#review:review:0dcb77dad8ef4db5102c1bd7eeaec630f239dc244db5eee8132f8c57d77677ea
        let session = bound(&fixture, binding_of(&fixture, Role::Verifier, None));
        let error = review(&session, "b+1", None).unwrap_err();
        assert!(error.contains("acts only on the global review"), "{error}");
        let error = request(&session, "b+1", None).unwrap_err();
        assert!(error.contains("acts only on the global review"), "{error}");
        let error = changes(&session, "b", None).unwrap_err();
        assert!(error.contains("acts only on the global review"), "{error}");
    }

    /// A verifier is given the review request it was run with, never one made again from
    /// the outputs as they are now; a compiler of the same batch has the request made for
    /// the batch.
    // @lfy def/mcp/main.lfy:review#review:review:da108e476bfaee29f25ace2bffd41fea2a2d7605e41824c4b25757bf8287f9b1
    #[test]
    fn a_verifier_is_given_the_review_request_it_was_run_with() {
        /// What the review request the verifier was run with reads.
        const GIVEN: &str = "# Reviewing the batch `b`\n\nthe request this verifier was run with\n";
        let fixture = project_with_requirements();
        let mut binding = binding_of(&fixture, Role::Verifier, Some("b"));
        let mut given = binding.review.clone().expect("a verifier's review");
        given.instructions = GIVEN.to_string();
        binding.review = Some(given);
        let session = bound(&fixture, binding);
        // @lfy def/mcp/main.lfy:review#review:review:da108e476bfaee29f25ace2bffd41fea2a2d7605e41824c4b25757bf8287f9b1
        assert_eq!(review(&session, "b", None).unwrap(), GIVEN);
        // A compiler's session has the request made for the batch instead.
        // @lfy def/mcp/main.lfy:review#review:review:f58d35f9aea99f8ea42d7e215a8e84d42b2b414afa745217d99622b1a66e5c0d
        let session = bound(&fixture, binding_of(&fixture, Role::Compiler, Some("b")));
        let instructions = review(&session, "b", None).unwrap();
        assert_ne!(instructions, GIVEN);
        assert!(instructions.contains("It answers"), "{instructions}");
    }

    /// A project whose one fn carries two local criteria, each beginning on a line of its
    /// own so that each is named by its place as well as by its id, and one global
    /// criterion, which a review of the batch never asks about.
    fn project_with_two_criteria() -> Fixture {
        let fixture = Fixture::new();
        fixture.with_rust_target().write(
            "def/a.lfy",
            "fn a(): `An a` => string {\n  \
             @acceptanceCriteria.add({ behavior = `It answers` });\n  \
             @acceptanceCriteria.add({ behavior = `It answers in one line` });\n}\n\
             global@acceptanceCriteria.add({ behavior = `Nothing is written outside the output directory` });\n",
        );
        fixture
    }

    /// The file and the line of the criterion or test an id names, as a review request
    /// gives the place.
    fn place_of_requirement(session: &Session, id: &str) -> (String, usize) {
        let line = serde_json::json!({ "id": id, "status": "satisfied", "evidence": "", "note": "" }).to_string();
        let program = &session.binding.as_ref().expect("a binding").program;
        let found = generation::review_of(&line, program, &[], &[])
            .reviews
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("a criterion named {id}"));
        (found.file, found.line)
    }

    /// A verdict reported by its place is recorded for the criterion that place names, with
    /// its status, evidence, and note; the text says so and counts what is still missing. A
    /// status naming none of the three, an id naming no criterion, and an id the review did
    /// not ask about each record nothing.
    // @lfy def/mcp/main.lfy:report#report:report:34ee53fe216df9928c40072f557778f5a2188225c7e5884099686da22499f026
    // @lfy def/mcp/main.lfy:report#report:report:00107440d32bd506e9119bd632f7552b025f2dfc4f0e24ebaf0fd98a73fb2141
    // @lfy def/mcp/main.lfy:report#report:report:1899e3f7c1c027232563ba6f23f541c4c864d9ac2021cf429ec4439cce298785
    #[test]
    fn report_records_one_verdict_and_counts_what_is_still_missing() {
        let fixture = project_with_two_criteria();
        let session = bound(&fixture, binding_of(&fixture, Role::Verifier, Some("a")));
        let asked = asked_of(&session).to_vec();
        assert_eq!(asked.len(), 2, "{asked:?}");
        let first = requirement_id(&fixture, "It answers");
        let second = requirement_id(&fixture, "It answers in one line");
        assert!(asked.contains(&first) && asked.contains(&second), "{asked:?}");

        // Reported by its place rather than by its id: the criterion that place names is
        // recorded, with the status, the evidence, and the note given.
        // @lfy def/mcp/main.lfy:report#report:report:15a298dff4d1c9c629d5217e0802615370e072e57d444f4abd189e391365165b
        // @lfy def/mcp/main.lfy:report#report:report:82b739ccf95aa1e1faa11800427ce14cadd3fdeef5e2c7904cc9999d70958a3a
        let (file, line) = place_of_requirement(&session, &second);
        let text = report(
            &session,
            None,
            Some(&file),
            Some(line),
            "violated",
            "out/a.rs:12-18",
            "returns undefined instead of an empty list",
        )
        .unwrap();
        // @lfy def/mcp/main.lfy:report#report:report:cb0ab5dd13a5f32de066f7ec167f81ff4853716054b8ebe9eae3c72af49a0c4b
        assert_eq!(text.lines().next(), Some(format!("recorded {second} at {file}:{line}").as_str()));
        // @lfy def/mcp/main.lfy:report#report:report:33d92b578b9d5f560fea04be01b80b698d6cef193437113b87d6767239dba56f
        assert!(text.contains(&format!("1 still missing: {first}")), "{text}");
        let recorded = session.recorded();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].id, second);
        assert_eq!(recorded[0].status, ReviewStatus::Violated);
        assert_eq!(recorded[0].evidence, "out/a.rs:12-18");
        assert_eq!(recorded[0].note, "returns undefined instead of an empty list");
        // Nothing is written: the verdict is kept in the session alone.
        // @lfy def/mcp/main.lfy:report#report:report:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
        assert!(!fixture.root.join("elfie-requests").exists());

        // An id that names no criterion or test of the review records nothing.
        // @lfy def/mcp/main.lfy:report#report:report:3d3b34634644a0a77e7ee4995575b04f369d10d6495cab7ed34e712d6c0df7d7
        let error = report(&session, Some("A:A:77aa"), None, None, "satisfied", "", "ok").unwrap_err();
        assert!(error.contains("A:A:77aa") && error.contains("no criterion or test"), "{error}");
        assert_eq!(session.recorded().len(), 1);
        // A status naming none of the three records nothing and names the three.
        // @lfy def/mcp/main.lfy:report#report:report:f95e4040e1b2d8658bf53cbe5eaf320a0628d57aafdf090791dea862afeb67fd
        let error = report(&session, Some(&first), None, None, "maybe", "", "unsure").unwrap_err();
        assert!(error.contains("satisfied") && error.contains("violated") && error.contains("unverifiable"), "{error}");
        assert_eq!(session.recorded().len(), 1);
        // A criterion of the program the review did not ask about records nothing either.
        // @lfy def/mcp/main.lfy:report#report:report:eecbccd9ec901434b61790fd663bb5081ca97c913c2328df04808e7f3755d06a
        let global = requirement_id(&fixture, "Nothing is written outside the output directory");
        let error = report(&session, Some(&global), None, None, "satisfied", "", "ok").unwrap_err();
        assert!(error.contains("was not asked about in this review"), "{error}");
        assert_eq!(session.recorded().len(), 1);

        // Every id asked about has a verdict: the text says so and how the final message
        // ends.
        // @lfy def/mcp/main.lfy:report#report:report:979112dd7ac1a472af0b4b0b3b534d9bf9f6464235b53bb1523bcb7fab70370c
        let text = report(&session, Some(&first), None, None, "satisfied", "", "it answers").unwrap();
        assert!(text.contains("every criterion and test asked about has a verdict"), "{text}");
        assert!(text.contains(generation::REVIEWED), "{text}");
        assert_eq!(session.recorded().len(), 2);
        // A second verdict on one criterion stands, and the earlier one is removed.
        // @lfy def/mcp/main.lfy:report#report:report:bfa37d307a244c3332ac692e347ce5f2a534f0f2a1f839c0cab30318da4f37cc
        let text = report(&session, Some(&first), None, None, "UNVERIFIABLE", "", "no region answers").unwrap();
        assert!(text.starts_with(&format!("recorded {first} ")), "{text}");
        let recorded = session.recorded();
        assert_eq!(recorded.len(), 2);
        assert_eq!(recorded[1].id, first);
        assert_eq!(recorded[1].status, ReviewStatus::Unverifiable);
    }

    /// A name that is the identifier of one batch and the stem of a unit of another means
    /// the batch whose identifier it is. The plan of the fixture batches `b` and `a` as
    /// `b+1`; a second batch whose identifier is `b` makes the name `b` mean both, and the
    /// one whose identifier it is wins.
    // @lfy def/mcp/main.lfy:request#request:request:a7e16dee761e4f416f1040c2c3f90c5e87e1352241e235f0eea4146033eccc99
    #[test]
    fn a_batch_identifier_is_meant_before_a_unit_stem() {
        let fixture = compiled_project();
        let session = fixture.session();
        let planned = plan_of(&session);
        let workspace = planned.workspace();
        let mut plan = planned.plan.clone();
        let of_b = (0..plan.units.len()).find(|&unit| plan.units[unit].stem == "b").unwrap();
        let of_a = (0..plan.units.len()).find(|&unit| plan.units[unit].stem == "a").unwrap();
        // `b+1` holds the unit whose stem is `b`; the batch added here has `b` as its
        // identifier and does not hold that unit.
        assert_eq!(plan.batches[0].identifier, "b+1");
        assert!(plan.batches[0].units.contains(&of_b));
        plan.batches.push(Batch {
            units: vec![of_a],
            identifier: "b".to_string(),
        });
        // @lfy def/mcp/main.lfy:request#request:request:a7e16dee761e4f416f1040c2c3f90c5e87e1352241e235f0eea4146033eccc99
        let index = resolve_batch(workspace, &plan, "b", None).unwrap();
        assert_eq!(plan.batches[index].identifier, "b");
        assert_eq!(plan.batches[index].units, [of_a]);
        // A stem that is no batch's identifier still names the batch holding that unit.
        // @lfy def/mcp/main.lfy:review#review:review:a7e16dee761e4f416f1040c2c3f90c5e87e1352241e235f0eea4146033eccc99
        let index = resolve_batch(workspace, &planned.plan, "a", None).unwrap();
        assert_eq!(planned.plan.batches[index].identifier, "b+1");
    }

    /// A project for a target that one entity is built for and the entity it reaches is
    /// not: the plan has a problem, so there is no request, while a review request is given
    /// as for any plan and does not mention it.
    // @lfy def/mcp/main.lfy:request#request:request:2bea1608ac7ace6999af5d0abfb20a8166b6e5149871e97cdc2660b9709891e0
    #[test]
    fn a_plan_with_problems_has_no_request_but_still_has_a_review() {
        let fixture = Fixture::new();
        fixture
            .with_rust_target()
            .write("def/a.lfy", "use \"./b\";\nd A: `An A` { $b = B; }\n")
            .write("def/b.lfy", "d B: `A B` { }\n")
            // A second target that only `A` is built for, so `A` reaches `B`, which is not
            // built for it.
            .write(
                "def/go.lfy",
                "use \"elfie/target\";\nuse \"./roles\";\nuse \"./a\";\n\n\
                 ace const go = targetOf({\n  output = \"go\",\n  language = lang,\n  \
                 layout = flat,\n  ecosystem = cargo,\n});\nA@targets.add(go);\n",
            );
        let session = fixture.session();
        let planned = plan_of(&session);
        // A plan with no problem would make this test prove nothing, so it is asserted.
        assert!(!planned.plan.problems.is_empty(), "{}", units(&session, None).unwrap());
        // Each problem is one line, as `problems` spells a diagnostic of stage generation
        // and severity error at its node.
        // @lfy def/mcp/main.lfy:units#units:units:e3b6f3a30f32b79ed993570429ffa615792d56cc2481b2b41aa6f859567ec450
        let listed = units(&session, None).unwrap();
        let first = listed.lines().next().unwrap();
        assert!(first.contains(": generation error: "), "{listed}");
        assert!(first.starts_with("def/"), "{listed}");
        // The problems come before the units.
        // @lfy def/mcp/main.lfy:units#units:units:3dfafdff61f6c1061f2fe64e135a6885c4554344d56b669fcd2f9dceddc9bb98
        let problem_at = listed.lines().position(|line| line.contains(": generation error: ")).unwrap();
        let unit_at = listed.lines().position(|line| line.starts_with("rust ")).unwrap();
        assert!(problem_at < unit_at, "{listed}");
        // There is no request.
        // @lfy def/mcp/main.lfy:request#request:request:2bea1608ac7ace6999af5d0abfb20a8166b6e5149871e97cdc2660b9709891e0
        let error = request(&session, "a", Some("rust")).unwrap_err();
        assert!(error.starts_with("the plan has problems, so there is no request:"), "{error}");
        assert!(error.contains(": generation error: "), "{error}");
        // The review request is given as for any plan, and does not mention the problems.
        // @lfy def/mcp/main.lfy:review#review:review:5c61bb2a8fa4f5d02a3219d0c4a1f8d385dbad4a9caf2a52fa73d59c10014617
        let batch = planned.plan.batches.first().map(|batch| batch.identifier.clone()).unwrap_or_else(|| panic!("{listed}"));
        let instructions = review(&session, &batch, None).unwrap();
        assert!(instructions.contains("Reviewing the batch"), "{instructions}");
        assert!(!instructions.contains("generation error"), "{instructions}");
    }

    /// Each move is one line reading the word move, the target, the stem it came from, the
    /// word into, the stem that compiles it, the identifiers moved in brackets, the word
    /// for, and the cause.
    // Decision: the planner of this fixture resolves a mutual use without moving anything,
    // so the line is read off a move built here rather than off a project contrived to make
    // one; the line is what the criterion is about.
    // @lfy def/mcp/main.lfy:units#units:units:04942eee0b664f2b18f16b21c1e245d6ec558e513af796471b999cea68d090c6
    #[test]
    fn a_move_is_one_line_naming_what_moved_and_why() {
        let fixture = compiled_project();
        let session = fixture.session();
        let planned = plan_of(&session);
        let workspace = planned.workspace();
        let plan = &planned.plan;
        let of_b = (0..plan.units.len()).find(|&unit| plan.units[unit].stem == "b").unwrap();
        let of_a = (0..plan.units.len()).find(|&unit| plan.units[unit].stem == "a").unwrap();
        let moved_entity = query::find(workspace, "B")[0];
        let moved = Move {
            entities: vec![moved_entity],
            origin: of_b,
            into: of_a,
            cause: "A.b".to_string(),
        };
        // @lfy def/mcp/main.lfy:units#units:units:04942eee0b664f2b18f16b21c1e245d6ec558e513af796471b999cea68d090c6
        assert_eq!(move_line(workspace, plan, &moved), "move rust b into a [B] for A.b");
    }

    /// A unit whose output answers for a violated review of the last [`GlobalReview`] is
    /// planned with the reason `violated`; without that review the plan is made with no
    /// violated unit at all.
    // @lfy def/mcp/main.lfy:units#units:units:e35cb874630f014bcf475b9b10f74413ed36fe8b669e6a8194fd1ca57d8cf251
    #[test]
    fn units_plans_a_violated_unit_again() {
        let fixture = project_with_requirements();
        let global = requirement_id(&fixture, "Nothing is written outside the output directory");
        fixture.write(
            "out/b.rs",
            &format!("// @lfy def/b.lfy:b#{global}\npub fn b() -> String {{\n    String::new()\n}}\n"),
        );
        record_unit(&fixture, "b");

        // There is no last global review, so the plan is made with no violated unit and the
        // recorded unit is up to date.
        // @lfy def/mcp/main.lfy:units#units:units:e35cb874630f014bcf475b9b10f74413ed36fe8b669e6a8194fd1ca57d8cf251
        let session = fixture.session();
        assert_eq!(units(&session, None).unwrap(), "rust b up to date []");

        // A review that is satisfied names no violated unit either. The file is written as
        // the command line writes it: a global review stringified.
        // @lfy def/mcp/main.lfy:units#units:units:e35cb874630f014bcf475b9b10f74413ed36fe8b669e6a8194fd1ca57d8cf251
        let review_file = |status: &str| {
            let last = GlobalReview {
                requirements: "0".repeat(64),
                reviews: vec![Review {
                    id: global.clone(),
                    file: String::new(),
                    line: 0,
                    entity: String::new(),
                    status: ReviewStatus::from_name(status).unwrap(),
                    evidence: "out/b.rs:1-4".to_string(),
                    note: "it is under out".to_string(),
                }],
                problems: Vec::new(),
                unplaced: Vec::new(),
                missing: Vec::new(),
            };
            format!("{}\n", last.to_json())
        };
        fixture.write("elfie-requests/global.reviews.json", &review_file("satisfied"));
        let session = fixture.session();
        assert_eq!(units(&session, None).unwrap(), "rust b up to date []");

        // The review is violated and a marker of the unit's output answers for its id, so
        // the unit is planned again.
        // @lfy def/mcp/main.lfy:units#units:units:e35cb874630f014bcf475b9b10f74413ed36fe8b669e6a8194fd1ca57d8cf251
        fixture.write("elfie-requests/global.reviews.json", &review_file("violated"));
        let session = fixture.session();
        assert_eq!(units(&session, None).unwrap(), "rust b violated []\n\nbatch b [b]");
    }

    // @lfy def/mcp/main.lfy:globalReview#globalReview:globalReview:b93638e7046f17fc2cf321187664d92150028acae44164992beee7e46349fb1c
    #[test]
    fn global_review_gives_the_review_request_of_every_global_requirement() {
        let fixture = project_with_requirements();
        let session = fixture.session();
        assert_eq!(problems(&session, None).unwrap(), "no problems");
        let instructions = global_review(&session).unwrap();
        assert!(instructions.contains("global review"), "{instructions}");
        assert!(instructions.contains("Nothing is written outside the output directory"), "{instructions}");

        // No global criterion or test exists.
        // @lfy def/mcp/main.lfy:globalReview#globalReview:globalReview:f656c023948494da8d316780c15c2c1b96e1512caafb62d843dc6295be0d89be
        let bare = compiled_project();
        let session = bare.session();
        assert_eq!(global_review(&session).unwrap(), "no global criterion or test exists in the program");
    }

    // ---- what a compiler run is bound to --------------------------------------------

    /// A session bound to a compiler run of the only batch of a fixture, with the source
    /// each unit's outputs were generated from where one is given, so that the request it is
    /// handed knows which criteria and tests were removed.
    // @lfy def/mcp/main.lfy:task#task:compilerTool:1e78463366ba8fbbfbec5fc142e181f67f7b2aead81f243e58972718f0a441a8
    fn compiling(fixture: &Fixture, previous: &[(&str, &str)]) -> Session {
        let session = fixture.session();
        let planned = plan_of(&session);
        let batch = planned.plan.batches.first().cloned().expect("one batch");
        let was: BTreeMap<String, String> = previous
            .iter()
            .map(|(path, text)| ((*path).to_string(), (*text).to_string()))
            .collect();
        let existing: Vec<Output> = batch
            .units
            .iter()
            .flat_map(|&unit| planned.plan.units[unit].outputs.iter())
            .filter_map(|map| {
                fs::read_to_string(fixture.root.join(&map.output))
                    .ok()
                    .map(|text| Output { path: map.output.clone(), text })
            })
            .collect();
        let request = generation::request(&planned.program, &planned.plan, &batch, &existing, &was, true);
        bound(
            fixture,
            Binding {
                role: Role::Compiler,
                batch: Some(batch),
                directory: fixture.root.clone(),
                plan: planned.plan.clone(),
                program: planned.program.clone(),
                request: Some(request),
                review: None,
                source_maps: Vec::new(),
            },
        )
    }

    /// The definition of a unit whose fn `b` answers one criterion, with a second criterion
    /// named by `other`.
    fn two_criteria(other: &str) -> String {
        format!(
            "fn b(): `A b` => string {{\n  @acceptanceCriteria.add({{ behavior = `It answers` }}).add({{ behavior = `{other}` }});\n}}\n"
        )
    }

    /// The source of unit `b` as its outputs were accepted: `b` carries two criteria and
    /// `c` carries one.
    const WAS: &str = "fn b(): `A b` => string {\n  @acceptanceCriteria.add({ behavior = `It answers` }).add({ behavior = `It never fails` });\n}\nfn c(): `A c` => string {\n  @acceptanceCriteria.add({ behavior = `It was slow` });\n}\n";

    /// The same source now: `b` has dropped its second criterion, and `c` has reworded its
    /// own and gained another. One criterion is kept, one removed, one reworded, one new.
    const NOW: &str = "fn b(): `A b` => string {\n  @acceptanceCriteria.add({ behavior = `It answers` });\n}\nfn c(): `A c` => string {\n  @acceptanceCriteria.add({ behavior = `It is quick` }).add({ behavior = `It also returns` });\n}\n";

    /// A project whose unit `b` has an accepted output, one criterion of which has since been
    /// dropped and one reworded, and the source the output was generated from.
    fn project_with_a_removed_criterion() -> (Fixture, String) {
        let fixture = Fixture::new();
        fixture.with_rust_target().write("def/b.lfy", WAS);
        let answers = requirement_id(&fixture, "It answers");
        let never = requirement_id(&fixture, "It never fails");
        let slow = requirement_id(&fixture, "It was slow");
        fixture.write(
            "out/b.rs",
            &format!(
                "// @lfy def/b.lfy:b#{answers}\n// @lfy def/b.lfy:b#{never}\npub fn b() -> String {{\n    String::new()\n}}\n// @lfy def/b.lfy:c#{slow}\npub fn c() -> String {{\n    String::new()\n}}\n"
            ),
        );
        record_unit(&fixture, "b");
        fixture.write("def/b.lfy", NOW);
        (fixture, WAS.to_string())
    }

    /// `elfie_task` gives each unit of the batch with its outputs and its reason, then every
    /// criterion and test to answer for with its mark, id, place, and text — a reworded one
    /// followed by the id and the text it replaces — then what was removed, then the counts;
    /// a stem of no unit of the batch is an error naming its units.
    // @lfy def/mcp/main.lfy:task#task:task:f5712f43d66f568f15101eeb1ba81a9fc8e2c6c7d9a372433f7bdfc21c443d5f
    // @lfy def/mcp/main.lfy:task#task:task:0ad7d52b1e05307d441d73562c31fb6ed56b23934d94cccfa43bcada539529d3
    #[test]
    fn task_gives_the_obligations_of_the_units_of_its_batch() {
        let (fixture, was) = project_with_a_removed_criterion();
        let kept = requirement_id(&fixture, "It answers");
        let fresh = requirement_id(&fixture, "It also returns");
        let rephrased = requirement_id(&fixture, "It is quick");
        let replaced = {
            let older = Fixture::new();
            older.with_rust_target().write("def/b.lfy", &was);
            requirement_id(&older, "It was slow")
        };
        let session = compiling(&fixture, &[("def/b.lfy", &was)]);
        let text = task(&session, None).unwrap();
        // The unit begins with its stem, its file, and its reason, then its outputs.
        // @lfy def/mcp/main.lfy:task#task:task:260c99a22d5afd2cb5bfe5e5341310aff3e4b5c20a62493303d03bc95d6f4767
        // @lfy def/mcp/main.lfy:task#task:task:93d986fe4163c82020f47668d2a385e3662ee162cec7a9332137b5df9c98b068
        assert!(text.contains("unit b def/b.lfy requirements"), "{text}");
        assert!(text.contains("\noutputs out/b.rs\n"), "{text}");
        // Then a line naming the entity, then one line per criterion in request order, each
        // with its mark, id, place, and text.
        // @lfy def/mcp/main.lfy:task#task:task:0ba2a96c6162e5b2088f0d0af27dcf6dcd7a99b009bf3be3772ffa13ffa864f4
        // @lfy def/mcp/main.lfy:task#task:task:e6bdc2cd8bf73910039781fad6be14d73994cd9495ecfb29415132f0e0b7926d
        // @lfy def/mcp/main.lfy:task#task:task:bc0cfc213f87dda7b8e1106191bb902bdb5a2b08b2472742020be295a621309b
        // @lfy def/mcp/main.lfy:task#task:task:48b5a17b6bd8bf11626b7e3c8a77f30620a9bcf76c647c3d0cb15aaa2ac3e582
        // @lfy def/mcp/main.lfy:task#task:task:8bf5a91fa0df3686fb02ff5457cd19a9f3c4d034aceb88db292e7f850ecd6638
        assert!(text.contains("\nb\n") && text.contains("\nc\n"), "{text}");
        // @lfy def/mcp/main.lfy:task#task:task:00479e8dc9cc7003d7a8fc71ee72b37424f99ac82adaf9497bd8a64df5cd83a2
        assert!(text.contains(&format!("kept {kept} def/b.lfy:2 — It answers")), "{text}");
        // @lfy def/mcp/main.lfy:task#task:task:aa3aedc17d31c8c2288f74ab8d995d37aabd29e988ed6623b3ab368c90880080
        assert!(text.contains(&format!("new {fresh} def/b.lfy:5 — It also returns")), "{text}");
        // A reworded one is marked apart from a new one, and the line after it gives the id
        // and the text it replaces.
        // @lfy def/mcp/main.lfy:task#task:task:8aab948f1ff4b43702f1158f16a22c2a5c3be0d41ed51e5ff90073af5c03d692
        assert!(
            text.contains(&format!(
                "reworded {rephrased} def/b.lfy:5 — It is quick\nreplaces {replaced} — It was slow\n"
            )),
            "{text}"
        );
        // Every entity of the unit has a criterion, so nothing is said to lack one.
        // @lfy def/mcp/main.lfy:task#task:task:8cda0fab4d2e20c30f36514faa7a119d7713f6defa2830d8a2284e0161739f87
        assert!(text.contains("no entity of the unit lacks a criterion or test"), "{text}");
        // Then what was removed since the outputs were accepted, one line each.
        // @lfy def/mcp/main.lfy:task#task:task:0e1730d59363044ef74bcf4673aef8eae5aecf93a017aa90bc7f8d9d457f6afd
        assert!(text.contains("these were removed since its outputs were accepted"), "{text}");
        assert!(text.contains("\nremoved ") && text.contains("It never fails"), "{text}");
        // The last line counts the marks.
        // @lfy def/mcp/main.lfy:task#task:task:ca1abaa819942eb8c6c130e520f1c36ee16815c41b54d388cca6a9b8ad14a76c
        assert_eq!(text.lines().next_back(), Some("1 new, 1 reworded, 1 kept, 0 unknown, 1 removed"));
        // Only the unit named is given.
        // @lfy def/mcp/main.lfy:task#task:task:d056015a5c0120d12f5e1daaebd2c2f0c96947c0bbff29ed4ebdeb57af2bcbe2
        assert_eq!(task(&session, Some("b")).unwrap(), text);
        // A stem of no unit of the batch is an error naming the units of the batch.
        // @lfy def/mcp/main.lfy:task#task:task:20cff63e998498e4042833f30de9a63a11338bcbcc0b15f2191ddcf2f7def4ab
        let error = task(&session, Some("a")).unwrap_err();
        assert!(error.contains("no unit of this batch has the stem a") && error.contains("are: b"), "{error}");
        // A session bound to no compiler run is answered with an error, and nothing is read.
        // @lfy def/mcp/main.lfy:task#task:compilerTool:09099b01d8e84ae85a89b2a12642ec68e8d2b277dfec75f117a5e093caf818b2
        let loose = fixture.session();
        assert_eq!(
            task(&loose, None).unwrap_err(),
            "elfie_task acts only in a session bound to a compiler run"
        );
    }

    /// A unit with no output yet says so and where its outputs go, and names every entity
    /// that has no criterion or test of its own.
    // @lfy def/mcp/main.lfy:task#task:task:3701f0af0023dfc1eadd1dee395415c17f3523d19c39a1d6d8f30a8aff532e9e
    // @lfy def/mcp/main.lfy:task#task:task:f9fb73337a13788bc2c97891ea0b6725150d8f585b1ea3c8102578a43fdc42d6
    #[test]
    fn task_says_where_a_fresh_units_outputs_go_and_what_lacks_a_criterion() {
        let fixture = compiled_project();
        let session = compiling(&fixture, &[]);
        let text = task(&session, None).unwrap();
        // @lfy def/mcp/main.lfy:task#task:task:3701f0af0023dfc1eadd1dee395415c17f3523d19c39a1d6d8f30a8aff532e9e
        assert!(text.contains("no output yet; its outputs go under out"), "{text}");
        // `B` has no criterion and no test, so it is named as still needing a marker.
        // @lfy def/mcp/main.lfy:task#task:task:f9fb73337a13788bc2c97891ea0b6725150d8f585b1ea3c8102578a43fdc42d6
        assert!(text.contains("no criterion or test is for B; each still needs a marker"), "{text}");
        // With no output there is nothing to have answered, so every mark is new and nothing
        // is said to have been removed.
        // @lfy def/mcp/main.lfy:task#task:task:c6544cb71fb79033882e38ad1d044989c8fa3a6dc97e274b3d4054fbb9b65bda
        assert!(!text.contains("is not known"), "{text}");
        // @lfy def/mcp/main.lfy:task#task:task:ca1abaa819942eb8c6c130e520f1c36ee16815c41b54d388cca6a9b8ad14a76c
        assert_eq!(text.lines().next_back(), Some("0 new, 0 reworded, 0 kept, 0 unknown, 0 removed"));
    }

    /// `elfie_dependency` gives one entity of a dependency outside the batch: the unit and
    /// its outputs, then the entity's own interface line and its members'.
    // @lfy def/mcp/main.lfy:dependency#dependency:dependency:59b75ec9e00f60a47689d0771606d6eb6e14ab13f45334c91b35dfb8f1d2f4b8
    // @lfy def/mcp/main.lfy:dependency#dependency:dependency:588c65261ef39abe974c127325bbce4ef580f262ff0112f967c93e292fb47dc0
    #[test]
    fn dependency_gives_one_entity_of_a_dependency_of_the_batch() {
        let fixture = Fixture::new();
        fixture
            .with_rust_target()
            .write("def/a.lfy", "use \"./b\";\nd A: `An A` { $b = B; }\n")
            .write("def/b.lfy", "d B: `a thing` { $n: `How many` = number; }\n")
            .write("out/b.rs", "// @lfy def/b.lfy:B\npub struct B;\n");
        // `b` is accepted already, so only `a` is planned and `b` is a dependency of its
        // batch from outside it.
        record_unit(&fixture, "b");
        let session = fixture.session();
        let planned = plan_of(&session);
        let batch = planned
            .plan
            .batches
            .iter()
            .find(|batch| batch.units.iter().any(|&unit| planned.plan.units[unit].stem == "a"))
            .cloned()
            .expect("the batch of a");
        let request = generation::request(&planned.program, &planned.plan, &batch, &[], &BTreeMap::new(), true);
        let session = bound(
            &fixture,
            Binding {
                role: Role::Compiler,
                batch: Some(batch),
                directory: fixture.root.clone(),
                plan: planned.plan.clone(),
                program: planned.program.clone(),
                request: Some(request),
                review: None,
                source_maps: Vec::new(),
            },
        );
        // @lfy def/mcp/main.lfy:dependency#dependency:dependency:59b75ec9e00f60a47689d0771606d6eb6e14ab13f45334c91b35dfb8f1d2f4b8
        let text = dependency(&session, "B").unwrap();
        // @lfy def/mcp/main.lfy:dependency#dependency:dependency:53420221788d685744cc63a541e15f79755a4b23efe12ec64289b0e9c90ad313
        assert!(text.starts_with("b def/b.lfy\noutputs "), "{text}");
        // @lfy def/mcp/main.lfy:dependency#dependency:dependency:5187f431c8e1a018407dfaf23c821f2cfc1f4a15370dc6acb8175a28e7fcd50c
        assert!(text.contains("- `B` (data"), "{text}");
        assert!(text.contains("a thing"), "{text}");
        assert!(text.contains("  - `n` (member)"), "{text}");
        assert!(text.contains("number"), "{text}");
        // A member named after a dot gives only that member's line, and a file path and a
        // colon limits the search to that dependency.
        // @lfy def/mcp/main.lfy:dependency#dependency:dependency:82fa16bd07081751c96e7033b5801a1698a74ccf133725c2d1e01ea9edd01680
        let member = dependency(&session, "B.n").unwrap();
        assert!(member.contains("  - `n` (member)"), "{member}");
        // @lfy def/mcp/main.lfy:dependency#dependency:dependency:7b425db08ad081030e6137f0d6924c40b3b223d36d698551284b593335193519
        assert_eq!(dependency(&session, "def/b.lfy:B").unwrap(), text);
        assert!(dependency(&session, "def/a.lfy:B").is_err());
        // No dependency outside the batch declares the name.
        // @lfy def/mcp/main.lfy:dependency#dependency:dependency:588c65261ef39abe974c127325bbce4ef580f262ff0112f967c93e292fb47dc0
        // @lfy def/mcp/main.lfy:dependency#dependency:dependency:57ea76cedd14a64116b4e927fb4f9f13577e2df3fe6caa9fd1c92a2dd6d88d06
        let error = dependency(&session, "Missing").unwrap_err();
        assert!(
            error.contains("no dependency outside the batch declares Missing") && error.contains("elfie_entity"),
            "{error}"
        );
        // A session bound to no compiler run is answered with an error.
        // @lfy def/mcp/main.lfy:dependency#dependency:compilerTool:94e97e4b9416897e8f7296ad1449f08bf7cb435fe9a76fc4125ba6de8559c49e
        let loose = fixture.session();
        assert_eq!(
            dependency(&loose, "B").unwrap_err(),
            "elfie_dependency acts only in a session bound to a compiler run"
        );
    }

    /// `elfie_mark` rewrites every marker of the run's outputs that names an old id, leaves
    /// everything else of the file alone, and answers an id of no criterion of its units
    /// with an error that writes nothing.
    // @lfy def/mcp/main.lfy:mark#mark:mark:51af2d7f5c09267a7462bdb9c79e3acb6aa356508fd67fec076310690dbdf73e
    // @lfy def/mcp/main.lfy:mark#mark:mark:9fec123ea2c4a8fd481352dfaebe77ee76ea10475a33ec754f839b2fb2b20f13
    #[test]
    fn mark_renames_every_marker_of_an_old_id() {
        let (fixture, was) = project_with_a_removed_criterion();
        let fresh = requirement_id(&fixture, "It is quick");
        let gone = {
            let older = Fixture::new();
            older.with_rust_target().write("def/b.lfy", &was);
            requirement_id(&older, "It never fails")
        };
        // The output holds the old id twice, on lines 2 and 5.
        fixture.write(
            "out/b.rs",
            &format!(
                "// @lfy def/b.lfy:B\n// @lfy def/b.lfy:b#{gone}\npub fn b() -> String {{\n    String::new()\n    // @lfy def/b.lfy:b#{gone}\n}}\n"
            ),
        );
        let session = compiling(&fixture, &[("def/b.lfy", &was)]);
        // @lfy def/mcp/main.lfy:mark#mark:mark:4f60b16548a3a5d470c0ee9cd9a0c70970ffdedc84b94fa5e4f5efbde3465999
        // @lfy def/mcp/main.lfy:mark#mark:mark:5bb1f1250f7f259cd65c04fd3be30c251cc135967710c7ce195a78f31866f8fe
        let text = mark(&session, Some(&fresh), Some(&gone), None, None, None).unwrap();
        assert!(text.starts_with("2 markers rewritten"), "{text}");
        assert!(text.contains(&format!("out/b.rs:2 def/b.lfy:b#{fresh}")), "{text}");
        assert!(text.contains(&format!("out/b.rs:5 def/b.lfy:b#{fresh}")), "{text}");
        // The output `rename` returned was written at its path, and it is the file `rename`
        // gives: only the id changed.
        // @lfy def/mcp/main.lfy:mark#mark:mark:9cbf7b9aff22e535179edfb2494ceb9e2aa081cb354077d9a0d66e38056c48d6
        let written = fs::read_to_string(fixture.root.join("out/b.rs")).unwrap();
        assert_eq!(
            written,
            format!(
                "// @lfy def/b.lfy:B\n// @lfy def/b.lfy:b#{fresh}\npub fn b() -> String {{\n    String::new()\n    // @lfy def/b.lfy:b#{fresh}\n}}\n"
            )
        );
        // No marker names the old id any more, so a second rename is an error.
        // @lfy def/mcp/main.lfy:mark#mark:mark:675bf462c93e343e85219da2087f1672279fc0bc713a21b5d0f5550e274a6a40
        let error = mark(&session, Some(&fresh), Some(&gone), None, None, None).unwrap_err();
        assert!(error.contains("no marker") && error.contains(&gone), "{error}");
        // An id of no criterion or test of the batch's units writes nothing.
        // @lfy def/mcp/main.lfy:mark#mark:mark:9fec123ea2c4a8fd481352dfaebe77ee76ea10475a33ec754f839b2fb2b20f13
        // @lfy def/mcp/main.lfy:mark#mark:mark:165048edf3b25a24ddb15a6916231fdc341c3ce643d5b2d74c220ae0c65ab193
        let error = mark(&session, Some("b:b:0000"), Some(&fresh), None, None, None).unwrap_err();
        assert!(error.contains("b:b:0000") && error.contains("no criterion or test"), "{error}");
        assert_eq!(fs::read_to_string(fixture.root.join("out/b.rs")).unwrap(), written);
        // A rename takes `replaces` and `id` and nothing else, and an insertion takes
        // `file`, `line`, and `entity`.
        // @lfy def/mcp/main.lfy:mark#mark:mark:de06da3d9e51338cb41d6032e36580ce6510b4297822e054fb6f234dc68b0594
        let error = mark(&session, Some(&fresh), Some(&gone), Some("out/b.rs"), None, None).unwrap_err();
        assert!(error.contains("a rename takes replaces and id"), "{error}");
        let error = mark(&session, None, Some(&gone), None, None, None).unwrap_err();
        assert!(error.contains("a rename takes replaces and id"), "{error}");
        // @lfy def/mcp/main.lfy:mark#mark:mark:2959e7495b60ed84e030a118eff9b9a1cb5d699815713975e3e7f53137050e14
        let error = mark(&session, None, None, Some("out/b.rs"), Some(1), None).unwrap_err();
        assert!(error.contains("an insertion takes file, line, and entity"), "{error}");
        assert_eq!(fs::read_to_string(fixture.root.join("out/b.rs")).unwrap(), written);
        // A session bound to no compiler run is answered with an error.
        // @lfy def/mcp/main.lfy:mark#mark:compilerTool:56146f12eb0eb11e3ccbcf6b89b563f1173e5c188a183687178892cdd6aefb62
        let loose = fixture.session();
        assert_eq!(
            mark(&loose, Some(&fresh), Some(&gone), None, None, None).unwrap_err(),
            "elfie_mark acts only in a session bound to a compiler run"
        );
    }

    /// `elfie_mark` inserts one marker line before the line named, at that line's
    /// indentation, and leaves every other character of the file alone.
    // @lfy def/mcp/main.lfy:mark#mark:mark:4841b8b767e45c4dd46dbfae2894efb5ea42b96bd547ca1c057553b20948b95e
    #[test]
    fn mark_inserts_one_marker_line_into_an_output() {
        let fixture = Fixture::new();
        fixture.with_rust_target().write("def/b.lfy", &two_criteria("It is quick"));
        let first = requirement_id(&fixture, "It answers");
        fixture.write(
            "out/b.rs",
            "// @lfy def/b.lfy:b\npub fn b() -> String {\n    let helper = 1;\n    String::new()\n}\n",
        );
        let session = compiling(&fixture, &[]);
        // The inserted line takes the indentation of the line it comes before, the target's
        // marker comment, `@lfy`, the declaring file, the entity, and the id.
        // @lfy def/mcp/main.lfy:mark#mark:mark:e959da5d59df47ac7933bb07c057234854cf20b78a1ae1296c4d2521057c5f32
        // @lfy def/mcp/main.lfy:mark#mark:mark:2e402e1e9ad7b4d3ccb7a990b41387cfa8be2150d5feaa6d160ee8adfd91704e
        let text = mark(&session, Some(&first), None, Some("out/b.rs"), Some(3), Some("b")).unwrap();
        assert_eq!(text, format!("out/b.rs:3     // @lfy def/b.lfy:b#{first}"));
        // The line that was third is now fourth, and nothing else of the file changed.
        // @lfy def/mcp/main.lfy:mark#mark:mark:f81ece6f1d736c69189687f1b61d0a5d106ce51765776bbd8523caaecef41dea
        let written = fs::read_to_string(fixture.root.join("out/b.rs")).unwrap();
        assert_eq!(
            written,
            format!(
                "// @lfy def/b.lfy:b\npub fn b() -> String {{\n    // @lfy def/b.lfy:b#{first}\n    let helper = 1;\n    String::new()\n}}\n"
            )
        );
        // One past the last line appends it, with no indentation and no id when none is
        // given.
        // @lfy def/mcp/main.lfy:mark#mark:mark:0d151705612442f586374d3ecc64db058c44120fe7a7355be4a84f2aafe6d259
        let appended = mark(&session, None, None, Some("out/b.rs"), Some(7), Some("b")).unwrap();
        assert_eq!(appended, "out/b.rs:7 // @lfy def/b.lfy:b");
        assert!(
            fs::read_to_string(fixture.root.join("out/b.rs")).unwrap().ends_with("}\n// @lfy def/b.lfy:b\n"),
            "{written}"
        );
        // A line outside the file, a file that is no output of the target, an entity no unit
        // of the batch declares, and an id of neither the unit nor the globals are each an
        // error that writes nothing.
        let before = fs::read_to_string(fixture.root.join("out/b.rs")).unwrap();
        // @lfy def/mcp/main.lfy:mark#mark:mark:a86c8a8dd444a7e09d183e1c131c3e4e0e74fdc0a4a752698290645da729a4b0
        let error = mark(&session, None, None, Some("out/b.rs"), Some(99), Some("b")).unwrap_err();
        assert!(error.contains("has 7 lines"), "{error}");
        // @lfy def/mcp/main.lfy:mark#mark:mark:62c0bc17d8c1d1e37e6821582cd6b59f05e0204a3f3edd6f1e33e9d2597fc2db
        let error = mark(&session, None, None, Some("out/z.rs"), Some(1), Some("b")).unwrap_err();
        assert!(error.contains("is not an output file"), "{error}");
        // @lfy def/mcp/main.lfy:mark#mark:mark:64c1518801f24801feb6b601cbda298cc2a1a97c81889b15a61fe32d5a3ded45
        let error = mark(&session, None, None, Some("out/b.rs"), Some(1), Some("Z")).unwrap_err();
        assert!(error.contains("no unit of the batch") && error.contains("b"), "{error}");
        // @lfy def/mcp/main.lfy:mark#mark:mark:dd3ef6b416a23f341639e29ff2f36569d059617f0c4d6c5404dea75abb9f0ef2
        let error = mark(&session, Some("b:b:0000"), None, Some("out/b.rs"), Some(1), Some("b")).unwrap_err();
        assert!(error.contains("b:b:0000") && error.contains("no global one"), "{error}");
        // @lfy def/mcp/main.lfy:mark#mark:mark:a7dcf7eb424415c0fcc0ed5a98c1d56456a130be8cb8f194d3f21ec67c1606df
        // @lfy def/mcp/main.lfy:mark#mark:mark:fcfecab8991108e6f27911bb4b6d77cfffcf93ddbb785581af7a1021cbe3b53c
        assert_eq!(fs::read_to_string(fixture.root.join("out/b.rs")).unwrap(), before);
        assert!(!fixture.root.join("elfie-compile").exists());
    }

    /// A line that falls in a region of a file no unit of the batch owns is refused.
    // @lfy def/mcp/main.lfy:mark#mark:mark:aac83695fcc88a2c0a4688d77974f2d0a6805ebf6b2d752b2141ff71a8a745a2
    #[test]
    fn mark_refuses_a_line_in_another_units_region() {
        let fixture = Fixture::new();
        // One output holds both units' regions; the batch of `a` owns only the first, and
        // `b` is accepted already so that `a` is planned on its own.
        fixture
            .with_rust_target()
            .write("def/a.lfy", "d A: `An A` { $n = number; }\n")
            .write("def/b.lfy", "d B: `A B` { $n = number; }\n")
            .write(
                "out/shared.rs",
                "// @lfy def/a.lfy:A\npub struct A;\n// @lfy def/b.lfy:B\npub struct B;\n",
            );
        record_unit(&fixture, "b");
        let session = fixture.session();
        let planned = plan_of(&session);
        let batch = planned
            .plan
            .batches
            .iter()
            .find(|batch| batch.units.iter().any(|&unit| planned.plan.units[unit].stem == "a"))
            .cloned()
            .expect("the batch of a");
        let request = generation::request(&planned.program, &planned.plan, &batch, &[], &BTreeMap::new(), true);
        let session = bound(
            &fixture,
            Binding {
                role: Role::Compiler,
                batch: Some(batch),
                directory: fixture.root.clone(),
                plan: planned.plan.clone(),
                program: planned.program.clone(),
                request: Some(request),
                review: None,
                source_maps: Vec::new(),
            },
        );
        let before = fs::read_to_string(fixture.root.join("out/shared.rs")).unwrap();
        // @lfy def/mcp/main.lfy:mark#mark:mark:aac83695fcc88a2c0a4688d77974f2d0a6805ebf6b2d752b2141ff71a8a745a2
        let error = mark(&session, None, None, Some("out/shared.rs"), Some(4), Some("A")).unwrap_err();
        assert!(error.contains("def/b.lfy") && error.contains("region"), "{error}");
        assert_eq!(fs::read_to_string(fixture.root.join("out/shared.rs")).unwrap(), before);
        // A line in the batch's own region is written.
        let text = mark(&session, None, None, Some("out/shared.rs"), Some(2), Some("A")).unwrap();
        assert_eq!(text, "out/shared.rs:2 // @lfy def/a.lfy:A");
    }

    /// `elfie_submit` records the outcome of the run, replaces an earlier one, and refuses a
    /// name that is none of the three and a blocked or clarify with nothing to say.
    // @lfy def/mcp/main.lfy:submit#submit:submit:681caf63bf979b14af1394ae6323a1e8bba1467185a28cf0201a947a73c9585d
    // @lfy def/mcp/main.lfy:submit#submit:submit:249b3b6f0035e686ddcd8050b765f8ae1695b4409763982164e8d8a663448e75
    // @lfy def/mcp/main.lfy:submit#submit:submit:24de485f562965f35b787f105da217baea3e41ef0dc7984011702fcc5274050b
    #[test]
    fn submit_records_how_the_run_ended() {
        let fixture = compiled_project();
        let session = compiling(&fixture, &[]);
        assert!(session.submission.lock().unwrap().is_none());
        // The outcome is read without regard to case, and what it says is kept.
        // @lfy def/mcp/main.lfy:submit#submit:submit:839e3605860269cb5d2a142eae47825edd5f8e18cc86e1a3b9c4f70278331d8d
        // @lfy def/mcp/main.lfy:submit#submit:submit:bedeb2634345b2348c1509d268083160d2e9755e210b5107f4fbc972522749c1
        // @lfy def/mcp/main.lfy:submit#submit:submit:f94daa1287cb9c2fd43bbdfe11f8cfe51bf9706696102840706f0445f387669a
        let text = submit(&session, "Blocked", "f:f:9c0d contradicts f:f:1f2e on empty lists").unwrap();
        assert!(text.contains("recorded the outcome blocked"), "{text}");
        assert!(text.contains("a later call of elfie_submit replaces it"), "{text}");
        assert!(text.contains("ELFIE: BLOCKED:"), "{text}");
        assert_eq!(
            session.submission.lock().unwrap().as_ref().map(|one| (one.outcome, one.message.clone())),
            Some((SubmittedOutcome::Blocked, "f:f:9c0d contradicts f:f:1f2e on empty lists".to_string()))
        );
        // Clarify needs its question, and nothing is recorded, so the submission still
        // stands.
        // @lfy def/mcp/main.lfy:submit#submit:submit:b99c292c6a953720125d01484c7cc9fc03b9727847086a54737c3363884273cb
        let error = submit(&session, "clarify", "").unwrap_err();
        assert!(error.contains("clarify its question"), "{error}");
        assert_eq!(
            session.submission.lock().unwrap().as_ref().map(|one| one.outcome),
            Some(SubmittedOutcome::Blocked)
        );
        // An outcome that is none of the three is an error naming the three.
        // @lfy def/mcp/main.lfy:submit#submit:submit:28e49a28fbbfa8f85d4d95ecccb6f05ae599e3a6f128b655d73257666a31412e
        let error = submit(&session, "finished", "").unwrap_err();
        assert!(
            error.contains("done") && error.contains("blocked") && error.contains("clarify"),
            "{error}"
        );
        // A later submission replaces the earlier one.
        // @lfy def/mcp/main.lfy:submit#submit:submit:839e3605860269cb5d2a142eae47825edd5f8e18cc86e1a3b9c4f70278331d8d
        submit(&session, "done", "").unwrap();
        assert_eq!(
            session.submission.lock().unwrap().as_ref().map(|one| one.outcome),
            Some(SubmittedOutcome::Done)
        );
        // Nothing is written, and a session bound to no compiler run is an error.
        // @lfy def/mcp/main.lfy:submit#submit:submit:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
        assert!(!fixture.root.join("out").exists());
        // @lfy def/mcp/main.lfy:submit#submit:compilerTool:ee05d8be309e4a7aab71c6ed469b76e49a958abace52e8b6407087bf66c21140
        let loose = fixture.session();
        assert_eq!(
            submit(&loose, "done", "").unwrap_err(),
            "elfie_submit acts only in a session bound to a compiler run"
        );
    }

    /// `elfie_search` gives each matching line of the outputs with where it is and the
    /// region it falls in, and refuses a pattern that is no regular expression.
    // @lfy def/mcp/main.lfy:search#search:search:9461946e8b14d460dcf00c7c38a9f31b59d9ef0204bb5dd72c7a8efdf6c5b31a
    // @lfy def/mcp/main.lfy:search#search:search:8f67741d05f229bb9234a3e249edc0f82804c4d25ba0235e349aeab582a65ff9
    #[test]
    fn search_gives_the_lines_of_the_outputs_that_match() {
        let fixture = Fixture::new();
        fixture.with_rust_target().write("def/b.lfy", &two_criteria("It is quick"));
        let first = requirement_id(&fixture, "It answers");
        fixture.write(
            "out/b.rs",
            &format!(
                "// @lfy def/b.lfy:B\npub struct B;\n\n\n\n// @lfy def/b.lfy:b#{first}\npub fn b() -> String {{\nfn helper() {{\n}}\n}}\n"
            ),
        );
        fixture.write("out/c.rs", "pub struct C;\n");
        let session = compiling(&fixture, &[]);
        // The line reads its path, its line, the region between brackets, and its text.
        // @lfy def/mcp/main.lfy:search#search:search:8112c25cc568699c5b153fd7ada26062fb8732b7fcaefef35a49e6389cc0a8ff
        // @lfy def/mcp/main.lfy:search#search:search:fa10d4c06de0e21bae6488a493c889c66e5ea9d22f426fbcf0488df26d15e041
        assert_eq!(
            search(&session, "fn helper[(]", None).unwrap(),
            format!("out/b.rs:8 [def/b.lfy:b#{first}]: fn helper() {{")
        );
        // A line before every marker of its file falls in no region, so its region is a
        // hyphen; the files are given in the order of their paths, then in line order.
        // @lfy def/mcp/main.lfy:search#search:search:b08361d90ea78257dde207e7d32417d0b3458ac1edf1f2c1061ddfb9fcfaa2cd
        // @lfy def/mcp/main.lfy:search#search:search:7841eec414d572a807f6ede13000e8a87220e884b36378adb2feef0da752b8ce
        assert_eq!(
            search(&session, "pub struct", None).unwrap(),
            "out/b.rs:2 [def/b.lfy:B]: pub struct B;\nout/c.rs:1 [-]: pub struct C;"
        );
        // Only the file named, or what is under it, is searched.
        // @lfy def/mcp/main.lfy:search#search:search:6fee06b6e4e8b823d41f3b760f75a448a7c9357ea1409123aaf2b2b62685f6ab
        assert_eq!(
            search(&session, "pub struct", Some("out/c.rs")).unwrap(),
            "out/c.rs:1 [-]: pub struct C;"
        );
        assert_eq!(search(&session, "pub struct", Some("out")).unwrap().lines().count(), 2);
        // @lfy def/mcp/main.lfy:search#search:search:53b64d09870e3da2a2a74debb3f5058f60fda381188b78182eb74ba5aa2269f1
        let error = search(&session, "pub struct", Some("def")).unwrap_err();
        assert!(error.contains("is not under an output directory"), "{error}");
        // No line matches, so the text says so with the count of the files searched.
        // @lfy def/mcp/main.lfy:search#search:search:5fcbe5fc50d6c7ffeea79876a5cf547599a86c0eb99cb0bac1ed79367eee2402
        let none = search(&session, "nowhere", None).unwrap();
        assert!(none.contains("no line of the 2 files searched matches"), "{none}");
        // A pattern that is no regular expression of the Rust regex crate is an error
        // carrying why it is not, and nothing is searched.
        // @lfy def/mcp/main.lfy:search#search:search:8f67741d05f229bb9234a3e249edc0f82804c4d25ba0235e349aeab582a65ff9
        // @lfy def/mcp/main.lfy:search#search:search:b52b7d681a1ab14d16e4367a42f8fe72e5a2c4577ef0e113b5f4e7a1e458ab91
        // @lfy def/mcp/main.lfy:search#search:search:3eb91d30c58a23ec68143fc70f48b65844f317e8c7607db0b879df80b296c435
        let error = search(&session, "fn (", None).unwrap_err();
        assert!(error.contains("is not a regular expression"), "{error}");
    }

    /// A matching line longer than five hundred characters is cut short, and more than two
    /// hundred matching lines are counted rather than all given.
    // @lfy def/mcp/main.lfy:search#search:search:68207e69227f6ee9bb5e6b5a46a769482b26d625a8589e28cdcef599d5b1756f
    // @lfy def/mcp/main.lfy:search#search:search:a1cf2a0988555448b5857efc090387808f6292fa53e678aeff920abc5253a1b5
    #[test]
    fn search_cuts_a_long_line_short_and_counts_what_it_leaves_out() {
        let fixture = Fixture::new();
        fixture.with_rust_target().write("def/b.lfy", "d B {}\n");
        let long = "x".repeat(KEPT_WIDTH + 10);
        let mut text = format!("// @lfy def/b.lfy:B\nlet wide = \"{long}\";\n");
        for index in 0..SEARCHED_LINES + 7 {
            text.push_str(&format!("let many{index} = 1;\n"));
        }
        fixture.write("out/b.rs", &text);
        let session = fixture.session();
        // @lfy def/mcp/main.lfy:search#search:search:68207e69227f6ee9bb5e6b5a46a769482b26d625a8589e28cdcef599d5b1756f
        let found = search(&session, "wide", None).unwrap();
        assert!(found.ends_with(CUT), "{found}");
        assert_eq!(found.lines().next().unwrap_or_default().chars().count(), "out/b.rs:2 [def/b.lfy:B]: ".chars().count() + KEPT_WIDTH + 1);
        // @lfy def/mcp/main.lfy:search#search:search:a1cf2a0988555448b5857efc090387808f6292fa53e678aeff920abc5253a1b5
        let many = search(&session, "let many", None).unwrap();
        let lines: Vec<&str> = many.lines().collect();
        assert_eq!(lines.len(), SEARCHED_LINES + 1);
        assert!(lines[SEARCHED_LINES].contains("7 more matching lines are left out"), "{many}");
        assert!(lines[SEARCHED_LINES].contains("narrow the pattern"), "{many}");
    }

    // ---- the target's own commands ---------------------------------------------------

    /// The roles of a target carrying one command line per operation named.
    // @lfy def/mcp/main.lfy:build#build:commandTool:f83af6a046e8e4f79a777c2d3c9b677a67d05faab6429155fbda5b26e27e4159
    fn roles_with(commands: &str) -> String {
        format!(
            "trait lang extends targetLanguage: `Rust` {{\n  .fileExtension = \"rs\";\n  .markerComment = \"//\";\n}}\n\
             trait flat extends targetLayout: `One file per unit` {{ }}\n\
             trait cargo extends targetEcosystem: `Crates from crates.io` {{\n  .ecosystem = \"cargo\";\n  @commands{commands};\n}}\n"
        )
    }

    /// A project whose one target `rust` writes under `out` and carries the commands given.
    /// The output directory is there, since a command runs in it.
    fn project_with_commands(commands: &str) -> Fixture {
        let fixture = Fixture::new();
        fixture
            .with_rust_target()
            .write("def/roles.lfy", &roles_with(commands))
            .write("def/b.lfy", "d B {}\n")
            .write("out/b.rs", "// @lfy def/b.lfy:B\npub struct B;\n");
        fixture
    }

    /// The run one command tool answered with, read back from its JSON.
    fn ran(text: &str) -> Value {
        serde_json::from_str(text).unwrap_or_else(|error| panic!("the JSON of a run, not {text:?}: {error}"))
    }

    /// `elfie_build` runs the target's build command in the run's directory and answers with
    /// the JSON of one run: its target, its operation, its line, whether it exited with 0,
    /// its code, what it wrote, and how many lines that leaves out.
    // @lfy def/mcp/main.lfy:build#build:build:a40f0109a27a2913a7b107821c016cbb3504324aca8cc70407cadc1922e6a6c1
    #[test]
    fn build_runs_the_targets_build_command_and_says_how_it_ended() {
        let fixture = project_with_commands(
            "\n    .add({ operation = Operation.build, line = \"echo built; exit 3\" })",
        );
        let session = compiling(&fixture, &[]);
        // @lfy def/mcp/main.lfy:build#build:commandTool:394aef9de6082d12b6f9d58c8eaae738d8ab0f6aef3341b5fda12620fe6500a6
        let run = ran(&build(&session, None, None).unwrap());
        assert_eq!(run["target"], "rust");
        assert_eq!(run["operation"], "build");
        assert_eq!(run["line"], "echo built; exit 3");
        // @lfy def/mcp/main.lfy:build#build:commandTool:af9a234fc63d3a56cd02fc43d481612e9c05d96d41642a52c35b8aa861c9ebd9
        assert_eq!(run["succeeded"], serde_json::json!(false));
        assert_eq!(run["exitCode"], serde_json::json!(3));
        // @lfy def/mcp/main.lfy:build#build:commandTool:b3cf05e26d7d3b687e7b0b4028358dba9a481d54a4bddde0f8e7ab00f9743fb7
        // @lfy def/mcp/main.lfy:build#build:commandTool:061a911a3cd22a9aa176a791d4d9f3a03405f6c4d5a5bad8f44fd652e56f6477
        assert_eq!(run["output"], "built\n");
        assert_eq!(run["omitted"], serde_json::json!(0));
        // A failing command is no error of the call: the run is the answer.
        // @lfy def/mcp/main.lfy:build#build:commandTool:2885c8e60a2e0374353fff71832add7b2dedc797a8f0f0bd2dacfc23d1f4d9fe
        let result = call(&session, "elfie_build", serde_json::json!({}));
        assert!(!result.is_error, "{}", result.text);
        // The target has no command for the other operations, so they are errors naming the
        // target and the operation, and nothing runs.
        // @lfy def/mcp/main.lfy:lint#lint:commandTool:db94076681d45420de07709b25f27480d746751609aa1bb42a77c77d453001eb
        let error = lint(&session, None, None).unwrap_err();
        assert!(error.contains("rust") && error.contains("lint"), "{error}");
        // @lfy def/mcp/main.lfy:format#format:commandTool:a09c5e43d4c38d2e3196576c80db3cf1cb918134842760d0c7e7aa134b552a6b
        let error = format_check(&session, None).unwrap_err();
        assert!(error.contains("format"), "{error}");
        // A target that is not one of the workspace's is an error naming the known targets.
        // @lfy def/mcp/main.lfy:build#build:commandTool:1f6aa9de92f8ca8b182ac8744b6bb86bfe4ef0e4fbe7aa6089dd3115896b773a
        let error = build(&session, None, Some("go")).unwrap_err();
        assert!(error.contains("go is not a target") && error.contains("rust"), "{error}");
        // The session's batch has one target, so naming it is allowed and naming another is
        // not; this workspace has only `rust`, so the error above covers the second.
        // @lfy def/mcp/main.lfy:build#build:commandTool:8ffd4f1c8964d044d3a2ad994cc592a42773205e5ab265fbba1c238e40c7390c
        // @lfy def/mcp/main.lfy:build#build:commandTool:c21f972796c2663341ed914a6967afaa5dc6c0c14d8be6175383fa74deb6c3ed
        let named = ran(&build(&session, None, Some("rust")).unwrap());
        assert_eq!(named["target"], "rust");
        // With no binding at all the only target of the workspace is the one.
        // @lfy def/mcp/main.lfy:build#build:commandTool:e3805c0183b778c19e58e0e6579a16be881fa3c9595b5ff020f4ac5a70215cba
        let loose = fixture.session();
        let only = ran(&build(&loose, None, None).unwrap());
        assert_eq!(only["target"], "rust");
    }

    /// `elfie_test` gives the test command the words that narrow which tests run, as `TESTS`,
    /// and the empty text when it is left out; `SCOPE` is set the same way.
    // @lfy def/mcp/main.lfy:test#test:test:3291847d6fc7e59e082b4c2e6004a110ccc8fa7a156885612c7dff255f1ce056
    // @lfy def/mcp/main.lfy:test#test:test:63cf7e8acbb800f9c7db948b8a480bd276884fb67ebc15e8da98f73210501a8d
    #[test]
    fn test_narrows_which_tests_run_by_a_filter_and_a_scope() {
        let fixture = project_with_commands(
            "\n    .add({ operation = Operation.test, line = 'echo \"[$TESTS]\"' })\n    .add({ operation = Operation.lint, line = 'echo \"[$SCOPE]\"' })",
        );
        let session = compiling(&fixture, &[]);
        // @lfy def/mcp/main.lfy:test#test:test:c8bf810d04779ce26830e5c43f290fd753346658b3b40ff992ab1cdedef32174
        let run = ran(&test(&session, Some("parse lex"), None, None).unwrap());
        assert_eq!(run["succeeded"], serde_json::json!(true));
        assert_eq!(run["output"], "[parse lex]\n");
        // @lfy def/mcp/main.lfy:test#test:test:c7fb9e74d7bb66ee2c63c58681b14d00dc8236468fec85e845bd8aed14276ae6
        let run = ran(&test(&session, None, None, None).unwrap());
        assert_eq!(run["output"], "[]\n");
        // `SCOPE` is the scope, or the empty text when it is left out.
        // @lfy def/mcp/main.lfy:lint#lint:lint:caecccd1cd176ce8fc2634b1001e015b48012d66788d863fb994735ca793fab5
        // @lfy def/mcp/main.lfy:test#test:test:caecccd1cd176ce8fc2634b1001e015b48012d66788d863fb994735ca793fab5
        // @lfy def/mcp/main.lfy:build#build:build:caecccd1cd176ce8fc2634b1001e015b48012d66788d863fb994735ca793fab5
        let run = ran(&lint(&session, Some("elfie-core"), None).unwrap());
        assert_eq!(run["output"], "[elfie-core]\n");
        let run = ran(&lint(&session, None, None).unwrap());
        assert_eq!(run["output"], "[]\n");
    }

    /// A command runs in the target's output directory under the run's directory, with
    /// `RUN_SCRIPT` set to the target's script runner, and the calls for one directory run
    /// their commands one after the other.
    // @lfy def/mcp/main.lfy:build#build:commandTool:9b8ea5c13487cc9695be55b65dee63a54d131b4325fed5195ed378368f8479de
    // @lfy def/mcp/main.lfy:build#build:commandTool:12878393e582a75489c9dea1e06ef0237a6e04ef7784802355edb6ae22e9815b
    #[test]
    fn a_command_runs_in_the_output_directory_with_the_script_runner_set() {
        let fixture = project_with_commands(
            "\n    .add({ operation = Operation.build, line = 'pwd; echo \"[$RUN_SCRIPT]\"' })",
        );
        let session = compiling(&fixture, &[]);
        // @lfy def/mcp/main.lfy:build#build:commandTool:9b8ea5c13487cc9695be55b65dee63a54d131b4325fed5195ed378368f8479de
        let run = ran(&build(&session, None, None).unwrap());
        let output = run["output"].as_str().unwrap_or_default();
        let shown = std::fs::canonicalize(fixture.root.join("out")).unwrap();
        assert!(output.contains(&shown.to_string_lossy().to_string()), "{output}");
        // The target's ecosystem sets no script runner, so `RUN_SCRIPT` is the empty text.
        // @lfy def/mcp/main.lfy:build#build:commandTool:12878393e582a75489c9dea1e06ef0237a6e04ef7784802355edb6ae22e9815b
        assert!(output.contains("[]"), "{output}");
    }

    /// What a command wrote is kept short: at most two hundred lines from the end, the first
    /// hundred earlier lines that read as a failure or follow one, a line counting what is
    /// left out, and no line longer than five hundred characters.
    // @lfy def/mcp/main.lfy:build#build:commandTool:5cb971efd556449109eeb23f4d811571b6874a2274b251355982e35590d77441
    // @lfy def/mcp/main.lfy:build#build:commandTool:63be0a2d4357c8c544999776293adaded56a3f4a94b5c8ad25682f08f6ac70a6
    // @lfy def/mcp/main.lfy:build#build:commandTool:7a7e7f217024616471823763ae368e16d259097c0caecb49b0e9e3b404db0ca5
    // @lfy def/mcp/main.lfy:build#build:commandTool:f6efadbb442d048cef8053d19a026c18b7dbffde5a2b0015a6047743150dc9f3
    // @lfy def/mcp/main.lfy:build#build:commandTool:0078fc9e2f6e0d5f31f5e4ca2228f246ffd00ecd0927004d85db9ceed18ed01b
    // @lfy def/mcp/main.lfy:build#build:commandTool:a9b744b3a270971d0bd9ce3b4335190df2715b2f07ae02df1b2548deca9fe114
    #[test]
    fn what_a_command_wrote_is_kept_short() {
        // At most two hundred lines: every line is kept and nothing is omitted.
        // @lfy def/mcp/main.lfy:build#build:commandTool:b3cf05e26d7d3b687e7b0b4028358dba9a481d54a4bddde0f8e7ab00f9743fb7
        let few: Vec<String> = (0..KEPT_LINES).map(|index| format!("line {index}")).collect();
        let (output, omitted) = kept_output(&few, false);
        assert_eq!(omitted, 0);
        assert_eq!(output.lines().count(), KEPT_LINES);
        // @lfy def/mcp/main.lfy:build#build:commandTool:061a911a3cd22a9aa176a791d4d9f3a03405f6c4d5a5bad8f44fd652e56f6477
        assert!(output.ends_with('\n'), "{output}");
        // More than two hundred: the kept earlier lines, then what was left out, then the
        // last two hundred.
        let mut many: Vec<String> = Vec::new();
        many.push("an Error happened".to_string());
        many.push("and this followed it".to_string());
        for index in 0..KEPT_LINES + 20 {
            many.push(format!("quiet {index}"));
        }
        let (output, omitted) = kept_output(&many, false);
        let lines: Vec<&str> = output.lines().collect();
        // @lfy def/mcp/main.lfy:build#build:commandTool:63be0a2d4357c8c544999776293adaded56a3f4a94b5c8ad25682f08f6ac70a6
        assert_eq!(lines[0], "an Error happened");
        assert_eq!(lines[1], "and this followed it");
        // @lfy def/mcp/main.lfy:build#build:commandTool:5cb971efd556449109eeb23f4d811571b6874a2274b251355982e35590d77441
        // @lfy def/mcp/main.lfy:build#build:commandTool:7a7e7f217024616471823763ae368e16d259097c0caecb49b0e9e3b404db0ca5
        assert_eq!(omitted, many.len() - KEPT_LINES - 2);
        assert_eq!(lines[2], format!("[elfie: {omitted} lines omitted]"));
        assert_eq!(lines.len(), 2 + 1 + KEPT_LINES);
        // @lfy def/mcp/main.lfy:build#build:commandTool:a9b744b3a270971d0bd9ce3b4335190df2715b2f07ae02df1b2548deca9fe114
        assert_eq!(lines[lines.len() - 1], &format!("quiet {}", KEPT_LINES + 19));
        // A line the command wrote and the output keeps is cut short at five hundred
        // characters; a line elfie adds is never shortened.
        // @lfy def/mcp/main.lfy:build#build:commandTool:f6efadbb442d048cef8053d19a026c18b7dbffde5a2b0015a6047743150dc9f3
        let wide = vec![format!("warning: {}", "x".repeat(KEPT_WIDTH))];
        let (output, _) = kept_output(&wide, true);
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines[0].chars().count(), KEPT_WIDTH + 1);
        assert!(lines[0].ends_with(CUT), "{output}");
        // A command that was stopped says so after the lines kept as for any run.
        // @lfy def/mcp/main.lfy:build#build:commandTool:1ec8e462d2638913d1d6b8cd5b590f00751d033f0891e5ca220c79cd71a87001
        // @lfy def/mcp/main.lfy:build#build:commandTool:0078fc9e2f6e0d5f31f5e4ca2228f246ffd00ecd0927004d85db9ceed18ed01b
        assert_eq!(lines[1], STOPPED);
    }

    /// The calls of command tools for one directory run their commands one after the other,
    /// in the order they arrived; two calls for different directories wait for neither.
    // @lfy def/mcp/main.lfy:build#build:commandTool:55ee18e11aaf994de7de40b0727c7ff09d9b5230a39f4ff4070a8e0b0108478a
    // @lfy def/mcp/main.lfy:host#host:host:b09cfad6442aec8ac4faf8e13385b2973136e4d0ac08ec20419f15750ea59366
    #[test]
    fn the_calls_for_one_directory_run_their_commands_in_turn() {
        let one = Path::new("/elfie/one");
        let other = Path::new("/elfie/other");
        let first = Turn::take(one);
        let second = Turn::take(one);
        let elsewhere = Turn::take(other);
        // The first in the queue runs at once, and so does one for another directory.
        // @lfy def/mcp/main.lfy:host#host:host:b09cfad6442aec8ac4faf8e13385b2973136e4d0ac08ec20419f15750ea59366
        first.wait();
        elsewhere.wait();
        // The second waits for the first to end, and runs as soon as it has.
        // @lfy def/mcp/main.lfy:build#build:commandTool:55ee18e11aaf994de7de40b0727c7ff09d9b5230a39f4ff4070a8e0b0108478a
        let waiting = std::thread::spawn(move || {
            second.wait();
            true
        });
        std::thread::sleep(Duration::from_millis(20));
        assert!(!waiting.is_finished());
        drop(first);
        assert!(waiting.join().unwrap_or(false));
        drop(elsewhere);
    }
}
