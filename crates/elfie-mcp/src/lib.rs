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
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use elfie_core::generation::{
    self, Batch, Marker, Move, Output, Plan, Request, Review, ReviewStatus, SourceMap, Unit,
};
use elfie_core::interpret::{self, LoweredCriterion, LoweredNode, LoweredTest, Program};
use elfie_core::model::{self, Criterion, EntityId, FileId, Model, Problem};
use elfie_core::query::{self, Outline, Position, Range};
use elfie_core::workspace::{self, Workspace};
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

pub use data::{Binding, GlobalReview, Host, Role, Session, Stamp, Tool, ToolArgument, ToolResult};
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
        }
    }

    /// Every review `elfie_report` has recorded in the session, in the order recorded.
    // @lfy def/mcp/main.lfy:host#host:host:fb368c740c9b5b7193ed5862a6e77ae6f5f3721486407aa0d26222433824a606
    pub fn recorded(&self) -> Vec<Review> {
        self.reviews.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
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
        // @lfy def/mcp/main.lfy:output#output:tool:6ec90ebb27e8df39d72c6327a86558dba615e33de7e1a1320cf3f20be8be9b9b
        // @lfy def/mcp/main.lfy:output#output:tool:4eb131ab31bb445c54c0ce94c545024446ab993437862448bc07d78bd45c18d0
        // @lfy def/mcp/main.lfy:output#output:tool:c72de76b2235f6d72935b0bb9556fdcb35f808b7c57557076b5abef04e1ae663
        // @lfy def/mcp/main.lfy:output#output:tool:36c2a21196c163f26ef3000bea97d35e35acda46fa0c93daea9b3a8770617cc5
        Registered::new(
            "output",
            "Where generated code is, with its text: every region that came from one entity, that answers for one criterion or test, or that one output file holds",
            // @lfy def/mcp/main.lfy:output#output:tool:2e6b42c0a83704a168683bcc35f00029ab55fcf28d2f29b1c3de09b006c0a9da
            vec![
                // @lfy def/mcp/main.lfy:output#output:tool:b44f0f08cd4a5a17ed3563ee169631ec0e06701a1866f2bcbb070d4ea652c7d5
                // @lfy def/mcp/main.lfy:output#output:tool:536b816ae7d597b602e24a5aa8afccf72252f366fdbf9adb3e6ee1abaae424ac
                // @lfy def/mcp/main.lfy:output#output:tool:84805fed91235a6a28f19e8f6aa8a7e0de24b33cbd3ef8f1ae5312db98688ddf
                ToolArgument::new("name", "as elfie_find takes it, a requirement id, or the path of an output file relative to the root", "string", true),
                // @lfy def/mcp/main.lfy:output#output:tool:cccf2bf991122a41e5f8d1b615ff93f9a9b92d85f03790b2610579b50c0becde
                ToolArgument::new("target", "the identifier of one target; every target when left out", "string", false),
            ],
            // @lfy def/mcp/main.lfy:output#output:tool:c74c2a14496848786c2a9b0c1df1da9d6b9205300b34a2f42d670ce562aa0cf3
            // @lfy def/mcp/main.lfy:output#output:tool:81224c8b94d715808fe36a32d50ac22cb7b68f3aaf9efac14a04a2d3ba729de1
            // @lfy def/mcp/main.lfy:output#output:tool:30281c2c4ab3871ba9d3e8e1d4bd3865e4ffa3438c789f65733035c5008a4f9b
            // @lfy def/mcp/main.lfy:output#output:tool:3e79b4342df7d8ac276a343f02bf39ffd5636a96ef3ba4641642d00b806447e4
            // @lfy def/mcp/main.lfy:output#output:tool:76a973ce9145a4809d1d1de1a68875e33067542ee119844cf65ac55c2d7affca
            // @lfy def/mcp/main.lfy:output#output:tool:1e4baa6263f16aec686a93f72ebe9e48822140f1dbbcc30be00356c10db27e08
            // @lfy def/mcp/main.lfy:output#output:tool:1efeaccaafd14ce38d6aacf01412cd2561c720263e5fcc9555e78c00c0bc642c
            // @lfy def/mcp/main.lfy:output#output:tool:355f2e4745bc2e753294ba137c3f91cae87ebf6bc265f34a566e5fd7b744b62a
            |session, arguments| output(session, arguments.string("name")?, arguments.optional("target")),
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
    ]
}

/// The names of the tools a session is offered: every fn of this module that carries
/// `tool`, less the one its role is not offered.
///
/// A session over standard input and output and the hosted session of a compiler are
/// offered every tool but `report`, which submits the verdict on a criterion of a review
/// they are not running; the hosted session of a verifier is offered `report` and every
/// tool but `check`, which answers whether a unit's outputs would be accepted.
// @lfy def/mcp/main.lfy:serve#serve:serve:8c8a0026f64efd88fd17abc5e0b6117e69f41dc9f25bc9981c211bb34ef51eab
// @lfy def/mcp/main.lfy:host#host:host:1a4382c1323fe191d3c3a4a60c0c6c3439bdea0dd8de7786751e380c5b6d4fce
// @lfy def/mcp/main.lfy:host#host:host:6272bd8878a4f4d729d54c18349ca6dabcc3838ced948fac26c6c9a037658539
fn offered(tools: &[Registered], binding: Option<&Binding>) -> Vec<String> {
    let withheld = match binding.map(|binding| binding.role) {
        Some(Role::Verifier) => "elfie_check",
        Some(Role::Compiler) | None => "elfie_report",
    };
    tools
        .iter()
        .map(|registered| registered.tool.name.clone())
        .filter(|name| name != withheld)
        .collect()
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
        if argument.ty != "number" {
            continue;
        }
        if let Some(number) = spelled.get(&argument.name).filter(|value| value.is_number()).cloned() {
            spelled.insert(argument.name.clone(), Value::String(number.to_string()));
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
/// session has a lock of its own, so a call of one never waits for a call of another.
// @lfy def/mcp/main.lfy:serve#serve:serve:b40b7d13aaacdbb22af8b5a419d2c34ed6ffe19d0e12598b4aa56f9dee24fe22
// @lfy def/mcp/main.lfy:host#host:host:ccd2ef2fd65b6d18cbe3bd391b33964306338675eb2302322f14cf7991f56750
// @lfy def/mcp/main.lfy:host#host:host:56d39a9faeffbcf6f80174ba63daca88d9831f2156e8b7fd6d7f963361ef396f
struct Served {
    /// The session, behind a lock.
    session: Mutex<Session>,
    /// Every fn of this module that carries `tool`.
    tools: Vec<Registered>,
    /// The names of the tools the session is offered, as [`offered`] gives them.
    offered: Vec<String>,
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
        let offered = offered(&tools, None);
        Served {
            session: Mutex::new(Session::load(root)),
            tools,
            offered,
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
            .with_instructions(
                "The Elfie agent server. elfie_problems, elfie_find, elfie_entity, elfie_references, and elfie_outline read the program; elfie_grammar gives the language; elfie_units, elfie_request, elfie_check, elfie_review, and elfie_globalReview drive compilation; elfie_output, elfie_source, and elfie_changes read what the last acceptance recorded; elfie_report submits a verifier's verdicts. Nothing here writes a file.",
            )
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
    // @lfy def/mcp/main.lfy:host#host:host:0e5bf548b58d82d9c681d6c046cd70e0e7da32b9a4532e50e6f0be121f6379f9
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
        let offered = offered(&tools, Some(&binding));
        let session = Session {
            workspace,
            // A hosted session never re-reads a file of the program, so it stamps none.
            seen: BTreeMap::new(),
            binding: Some(binding),
            // @lfy def/mcp/main.lfy:host#host:host:91b44b23c29eaf7242b190da7fe5ab5f3642df5ec77212e680331af6b76e01b1
            reviews: Mutex::new(Vec::new()),
        };
        let served = Arc::new(Served {
            session: Mutex::new(session),
            tools,
            offered,
            reread: false,
        });
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(format!("/{SESSIONS}/{number}"), served);
        format!("{}{SESSIONS}/{number}", self.url)
    }

    /// The session at a URL ended, and what `elfie_report` recorded in it; a URL that is no
    /// open session's ends nothing and gives an empty list.
    // @lfy def/mcp/main.lfy:host#host:host:fb368c740c9b5b7193ed5862a6e77ae6f5f3721486407aa0d26222433824a606
    // @lfy def/mcp/main.lfy:host#host:host:caaadcd17cc50142ec83820303c8d57ce517397063641ba79b3f62cc25a03c40
    fn close(&self, url: &str) -> Vec<Review> {
        let Some(path) = url.strip_prefix(&self.url) else {
            return Vec::new();
        };
        let removed = self
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&format!("/{path}"));
        let Some(served) = removed else {
            return Vec::new();
        };
        served
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .recorded()
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
// @lfy def/mcp/main.lfy:host#host:host:0e5bf548b58d82d9c681d6c046cd70e0e7da32b9a4532e50e6f0be121f6379f9
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
            // @lfy def/mcp/main.lfy:host#host:host:0e5bf548b58d82d9c681d6c046cd70e0e7da32b9a4532e50e6f0be121f6379f9
            .with_legacy_session_mode(false)
            .with_json_response(true),
    );
    service.handle(request).await.map(axum::body::Body::new)
}

/// Serve agents over HTTP on this machine for the length of one compile or verify, one
/// hosted session per run of the compiler or the verifier.
///
/// Nothing here opens a file for writing, and no tool a hosted session serves does either;
/// nothing is written to standard output, since the protocol travels over HTTP and what the
/// host has to say it says by its answer.
// @lfy def/mcp/main.lfy:host#host:host:a8cb0027089ab0ec6fcb4de19907afba597173cea19d5f4ab215e07283d50569
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

/// [`generation::source_maps_of`] the session's workspace, with the maps the session's
/// review was made with standing in for the recorded maps of the same target and source.
///
/// A verifier judges the outputs its review was made against, so where the binding carries
/// a map for a target and a source, every map read for that pair is left out and the
/// binding's maps are read in their place; a map the binding carries that replaces none is
/// read after the rest. With no binding, or with none carried, what the last acceptance
/// recorded under the session's root is all that is read.
// @lfy def/mcp/main.lfy:output#output:output:429055d8371ef1095c71aff08d676ca7f8b8e25c1c2f54be17304040862780ab
// @lfy def/mcp/main.lfy:host#host:host:6f8405c8e6f3e4591069a7d71ce8930df3cd097e586d761bdaa8bb9a2df6ce2e
// @lfy def/mcp/main.lfy:host#host:host:232c0725ae5219330822ed9664cc87be38c4591303cbc3dead664044e09b75eb
fn source_maps_of(session: &Session, target: Option<&str>) -> Vec<SourceMap> {
    let read = generation::source_maps_of(&session.workspace, target);
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
// @lfy def/mcp/main.lfy:request#request:request:4dee59f1344d1563846d889e2ed940f684d08de14d3da6460c92f8859ad610de
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
    generation::request(program, plan, batch, &existing, &BTreeMap::new())
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
// @lfy def/mcp/main.lfy:request#request:request:4dee59f1344d1563846d889e2ed940f684d08de14d3da6460c92f8859ad610de
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
    Ok(request_for(&planned.program, plan, &plan.batches[index]).instructions) // @lfy def/mcp/main.lfy:request#request:request:4dee59f1344d1563846d889e2ed940f684d08de14d3da6460c92f8859ad610de
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
// @lfy def/mcp/main.lfy:output#output:output:6055cc354d99aaba9dae3cb3d09dc636c7ca78c9c6c7d9ab508cec4c6d857443
fn fence_of(path: &str) -> &str {
    Path::new(path).extension().and_then(|extension| extension.to_str()).unwrap_or("")
}

/// The lines of an output from `first` through `last`, counting from 1, as they are on
/// disk; the empty text when the file cannot be read.
// @lfy def/mcp/main.lfy:output#output:output:6055cc354d99aaba9dae3cb3d09dc636c7ca78c9c6c7d9ab508cec4c6d857443
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
/// ends on, a space, and what it came from, then its text in a fenced block.
// @lfy def/mcp/main.lfy:output#output:output:22ddcfeba7456aa91005e6aeab2fcd871e9c512ede2849961438a5b7a3cc5d7c
fn region_block(workspace: &Workspace, output: &str, marker: &Marker) -> String {
    let mut heading = format!("{}:{}-{}", output, marker.output_line, marker.end);
    if let Some(entity) = &marker.entity {
        heading.push_str(&format!(" {entity}"));
    }
    // The marker names a requirement, so the line ends with its id, after the entity.
    // @lfy def/mcp/main.lfy:output#output:output:f06f38040e8ba03b92a99e321da9419cc77651260ecc24f8c0087ce2ba8f1dcd
    if let Some(requirement) = &marker.requirement {
        heading.push_str(&format!(" {requirement}"));
    }
    // @lfy def/mcp/main.lfy:output#output:output:6055cc354d99aaba9dae3cb3d09dc636c7ca78c9c6c7d9ab508cec4c6d857443
    format!(
        "{heading}\n```{}\n{}\n```",
        fence_of(output),
        excerpt(workspace, output, marker.output_line, marker.end)
    )
}

/// Every region of every source map, each with the output that holds it, in source map then
/// output order, limited to one target when one is given.
// @lfy def/mcp/main.lfy:output#output:output:429055d8371ef1095c71aff08d676ca7f8b8e25c1c2f54be17304040862780ab
fn every_region<'m>(maps: &'m [SourceMap], target: Option<&str>) -> Vec<(&'m str, &'m Marker)> {
    maps.iter()
        .filter(|map| !target.is_some_and(|target| map.target != target))
        .flat_map(|map| map.markers.iter().map(|marker| (map.output.as_str(), marker)))
        .collect()
}

/// Where generated code is, with its text: every region that came from one entity, that
/// answers for one criterion or test, or that one output file holds.
// @lfy def/mcp/main.lfy:output#output:output:429055d8371ef1095c71aff08d676ca7f8b8e25c1c2f54be17304040862780ab
pub fn output(session: &Session, name: &str, target: Option<&str>) -> Result<String, String> {
    let workspace = &session.workspace;
    check_target(workspace, target)?; // @lfy def/mcp/main.lfy:output#output:output:e2e8756b78db77377231d8ef5ee9fd3e078703c801906a9b774ee59a2135a3ab
    let maps = source_maps_of(session, None); // @lfy def/mcp/main.lfy:output#output:output:429055d8371ef1095c71aff08d676ca7f8b8e25c1c2f54be17304040862780ab
    let every = every_region(&maps, target);
    // The name is the output of a source map, so the regions are every marker of it in
    // output order, and the answer reads as an outline of what generated the file.
    // @lfy def/mcp/main.lfy:output#output:output:011a8219d4a9a1eb6b18f7e331922d2aae1b4938d18adb692714488698f3c513
    let mut regions: Vec<(&str, &Marker)> =
        every.iter().copied().filter(|(output, _)| *output == name).collect();
    regions.sort_by_key(|(_, marker)| marker.output_line);
    // The name is one an entity owns, so the regions are the ones `regions_of` gives for it;
    // a marker is matched by the same target and the same marker it was found by, because
    // `regions_of` keeps no path.
    // @lfy def/mcp/main.lfy:output#output:output:a5cb2104c621c7fa795cd1fd5852076e2cbd1c5c3555abeef192eccac947387e
    if regions.is_empty() {
        let owned = generation::regions_of(workspace, &maps, name, target);
        regions = every.iter().copied().filter(|(_, marker)| owned.contains(marker)).collect();
    }
    // The name is the id of a criterion or test of the lowered program, local or global, so
    // its text comes first and the regions are the ones answering for it.
    // @lfy def/mcp/main.lfy:output#output:output:6a17670735bc53b7f9d4a073cb75d08d5ee4039cf0fd1e61549b0816fcbebd92
    let mut heading = None;
    if regions.is_empty() {
        let program = interpret::lower(workspace.clone());
        if let Some(text) = requirement_text(&program, name) {
            heading = Some(text);
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
    blocks.extend(regions.into_iter().map(|(output, marker)| region_block(workspace, output, marker)));
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
    /// order the definition declares the fns that carry `tool`: every one of them but
    /// `elfie_report`, which only a verifier's hosted session is offered.
    const TOOL_NAMES: [&str; 14] = [
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
        "elfie_source",
        "elfie_changes",
        "elfie_review",
        "elfie_globalReview",
    ];

    /// The name of every fn of this module that carries `tool`, in declaration order.
    const EVERY_TOOL: [&str; 15] = [
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
        "elfie_source",
        "elfie_changes",
        "elfie_review",
        "elfie_globalReview",
        "elfie_report",
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
        // @lfy def/mcp/main.lfy:serve#serve:serve:8c8a0026f64efd88fd17abc5e0b6117e69f41dc9f25bc9981c211bb34ef51eab
        assert_eq!(offered(&tools(), None), TOOL_NAMES);
        let fixture = Fixture::new();
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
        assert_eq!(listed.len(), 14);
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
        assert_eq!(listed[10].input_schema["properties"]["line"]["type"], "number");
        // @lfy def/mcp/main.lfy:source#source:tool:5bf72ec14e48137ed89a57ea542187e7e9c485c8a42ba64d90d18cc298f7a0fe
        assert_eq!(listed[10].input_schema["required"], serde_json::json!(["file", "line"]));
        // @lfy def/mcp/main.lfy:changes#changes:tool:ec3c7defff9f765cc593846440341f6c9ab6818d3c051ab9101385709365d3fb
        assert_eq!(listed[11].input_schema["properties"]["target"]["type"], "string");
        // @lfy def/mcp/main.lfy:changes#changes:tool:0c631194a5026a108cc79d09dc3877b6278e686a4a4b0853d4b441b65c8bb6fc
        assert_eq!(listed[11].input_schema["required"], serde_json::json!(["unit"]));
    }

    /// The whole server, driven over streams: a root whose `def` holds `a.lfy` reading
    /// `const y = z;` answers a `tools/list` naming the fourteen tools and a call of
    /// `elfie_problems` with the one binder error at `z`, and returns 0 once the stream it
    /// reads as standard input closes.
    // @lfy def/mcp/main.lfy:serve#serve:serve:d068530591797fc019dd47b6560b9736cd156d69a05cb80d610f4d291a55c876
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
        agent
            .send(serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .await;

        // @lfy def/mcp/main.lfy:serve#serve:serve:8c8a0026f64efd88fd17abc5e0b6117e69f41dc9f25bc9981c211bb34ef51eab
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

    // @lfy def/mcp/main.lfy:request#request:request:4dee59f1344d1563846d889e2ed940f684d08de14d3da6460c92f8859ad610de
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

    // @lfy def/mcp/main.lfy:output#output:output:a5cb2104c621c7fa795cd1fd5852076e2cbd1c5c3555abeef192eccac947387e
    #[test]
    fn output_gives_every_region_of_an_entity_with_its_text() {
        let fixture = accepted_project();
        let session = fixture.session();
        // Each region reads the path, the lines it spans, and the entity the marker names,
        // then the text of those lines in a fenced block.
        // @lfy def/mcp/main.lfy:output#output:output:22ddcfeba7456aa91005e6aeab2fcd871e9c512ede2849961438a5b7a3cc5d7c
        let text = output(&session, "B", None).unwrap();
        assert_eq!(text, "out/b.rs:1-2 B\n```rs\n// @lfy def/b.lfy:B\npub struct B;\n```");
        assert_eq!(output(&session, "B", Some("rust")).unwrap(), text);
        // The name is the output of a source map, so the regions are every marker of it.
        // @lfy def/mcp/main.lfy:output#output:output:011a8219d4a9a1eb6b18f7e331922d2aae1b4938d18adb692714488698f3c513
        assert_eq!(output(&session, "out/b.rs", None).unwrap(), text);
        // Nothing is generated for the name.
        // @lfy def/mcp/main.lfy:output#output:output:7e2589822812f683a65808151f3ae1177182b076f571c65ab59fa2d7e78162a8
        let none = output(&session, "A", None).unwrap();
        assert_eq!(none, "nothing is generated for A");
        // The target is not a known one.
        // @lfy def/mcp/main.lfy:output#output:output:e2e8756b78db77377231d8ef5ee9fd3e078703c801906a9b774ee59a2135a3ab
        let error = output(&session, "B", Some("go")).unwrap_err();
        assert!(error.contains("go is not a target") && error.contains("rust"), "{error}");
    }

    // @lfy def/mcp/main.lfy:output#output:output:6a17670735bc53b7f9d4a073cb75d08d5ee4039cf0fd1e61549b0816fcbebd92
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
        // @lfy def/mcp/main.lfy:output#output:output:6a17670735bc53b7f9d4a073cb75d08d5ee4039cf0fd1e61549b0816fcbebd92
        assert_eq!(
            output(&session, &local, None).unwrap(),
            format!(
                "It answers\n\nout/b.rs:1-4 b {local}\n```rs\n// @lfy def/b.lfy:b#{local}\npub fn b() -> String {{\n    String::new()\n}}\n```"
            )
        );
        // A global id is read from the lowered program the same way; no region answers for
        // this one, so its text is followed by nothing being generated for it.
        // @lfy def/mcp/main.lfy:output#output:output:6a17670735bc53b7f9d4a073cb75d08d5ee4039cf0fd1e61549b0816fcbebd92
        assert!(Review::is_global(&global), "{global}");
        let text = output(&session, &global, None).unwrap();
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
        Binding {
            role,
            batch: named,
            directory: fixture.root.clone(),
            plan: planned.plan.clone(),
            program: planned.program.clone(),
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
    // @lfy def/mcp/main.lfy:host#host:host:1a4382c1323fe191d3c3a4a60c0c6c3439bdea0dd8de7786751e380c5b6d4fce
    // @lfy def/mcp/main.lfy:host#host:host:6272bd8878a4f4d729d54c18349ca6dabcc3838ced948fac26c6c9a037658539
    // @lfy def/mcp/main.lfy:host#host:host:91b44b23c29eaf7242b190da7fe5ab5f3642df5ec77212e680331af6b76e01b1
    // @lfy def/mcp/main.lfy:host#host:host:fb368c740c9b5b7193ed5862a6e77ae6f5f3721486407aa0d26222433824a606
    // @lfy def/mcp/main.lfy:host#host:host:caaadcd17cc50142ec83820303c8d57ce517397063641ba79b3f62cc25a03c40
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
        // A compiler is offered every tool but `report`; a verifier every tool but `check`.
        // @lfy def/mcp/main.lfy:host#host:host:1a4382c1323fe191d3c3a4a60c0c6c3439bdea0dd8de7786751e380c5b6d4fce
        assert_eq!(compiling.offered, TOOL_NAMES);
        // @lfy def/mcp/main.lfy:host#host:host:6272bd8878a4f4d729d54c18349ca6dabcc3838ced948fac26c6c9a037658539
        assert!(verifying.offers("elfie_report"), "{:?}", verifying.offered);
        assert!(!verifying.offers("elfie_check"), "{:?}", verifying.offered);
        // `check` is not offered to it, so it is not listed either.
        // @lfy def/mcp/main.lfy:check#check:tool:d1b4319d34a3d00324ec0850bf1e0987ceb4112aab80f13d77b82554c27ca5f9
        assert!(!verifying.list().tools.iter().any(|tool| tool.name == "elfie_check"));
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

        // Closing a session gives back what it recorded and leaves it no longer open.
        // @lfy def/mcp/main.lfy:host#host:host:fb368c740c9b5b7193ed5862a6e77ae6f5f3721486407aa0d26222433824a606
        assert!(hosting.close(&first).is_empty());
        assert!(hosting.session("/sessions/1").is_none());
        // A URL that is no open session's gives an empty list.
        // @lfy def/mcp/main.lfy:host#host:host:caaadcd17cc50142ec83820303c8d57ce517397063641ba79b3f62cc25a03c40
        assert!(hosting.close(&first).is_empty());
        assert!(hosting.close("http://127.0.0.1:7/sessions/9").is_empty());
        assert!(hosting.close("http://elsewhere/sessions/2").is_empty());
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
    // @lfy def/mcp/main.lfy:host#host:host:0e5bf548b58d82d9c681d6c046cd70e0e7da32b9a4532e50e6f0be121f6379f9
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
        // @lfy def/mcp/main.lfy:host#host:host:0e5bf548b58d82d9c681d6c046cd70e0e7da32b9a4532e50e6f0be121f6379f9
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
        assert_eq!(served, TOOL_NAMES);
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
}
