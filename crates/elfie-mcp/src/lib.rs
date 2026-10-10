//! Compiled from `def/mcp/main.lfy`: the Elfie agent server.
//!
//! One tool per fn of the definition that carries `tool`, spoken over the Model Context
//! Protocol on standard input and output. The server never writes a file: during
//! compilation the agent writes the outputs itself and asks the server to check them, so
//! one path exists for writing and the CLI owns it.

pub mod data;
pub mod traits; // @lfy def/mcp/traits.lfy:tool

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use elfie_core::generation::{self, Batch, Marker, Move, Output, Plan, Request, SourceMap, Unit};
use elfie_core::interpret::{self, LoweredCriterion, LoweredNode, LoweredTest, Program};
use elfie_core::model::{self, Criterion, EntityId, FileId, Model, Problem};
use elfie_core::query::{self, Outline, Position, Range};
use elfie_core::workspace::{self, Workspace};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ErrorData, Implementation, JsonObject,
    ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerConfig,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ServerHandler, ServiceExt};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite};

pub use data::{GlobalReview, Session, Stamp, Tool, ToolArgument, ToolResult};
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
    /// stamped as read.
    // @lfy def/mcp/main.lfy:serve#serve:serve:0d7ad5672274c815b6ad256268ff29d8425eb7ed8114bb7c9ea52bd41bedde9b:2
    pub fn load(root: &Path) -> Session {
        let workspace = workspace::load(root);
        let seen = stamps(&workspace);
        Session { workspace, seen }
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

/// The fns of this module that carry `tool`, in order, as the server lists them; no
/// resource and no prompt is listed beside them.
///
/// One [`Registered`] per fn carries the name the server lists it by, the fn's definition
/// as its description, and one [`ToolArgument`] per parameter that is not the session. A
/// call reaches the fn through [`call_tool`], which checks the arguments against those
/// parameters before the fn is called, answers an argument that is missing, unknown, or of
/// the wrong type with an error naming it and without calling the fn, and turns what the fn
/// returns into the call's result — a failure into an error carrying it, so the server goes
/// on.
// @lfy def/mcp/main.lfy:serve#serve:serve:e990effaf8e2dc344e48fb8a9fd67f96fe167fdecb4798c66784cab9b740766f
pub fn tools() -> Vec<Registered> {
    vec![
        // @lfy def/mcp/main.lfy:problems#problems:tool:209c4d7a2c69c47c18dc88b5e0384e2286139253e7bb0830ef8eeb207288eedf
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
            // @lfy def/mcp/main.lfy:problems#problems:tool:5e8f11c4474e2631c9ca664d232e8a38cb91d578d2ba02838aedafeb35050273
            // @lfy def/mcp/main.lfy:problems#problems:tool:d9b6b1ce215d966d4d49ba31d319199d9539fe1614bed20bc86032b0e2fc28c0
            // @lfy def/mcp/main.lfy:problems#problems:tool:bd217fa94e62eeed18e974ee633e125704d038aca620974a2488d096e108a590
            // @lfy def/mcp/main.lfy:problems#problems:tool:0365ce41c179a2bdfc7bb6bc7e6f5a2521574397ecc547a36369152171a66b3b
            // @lfy def/mcp/main.lfy:problems#problems:tool:8cd074b3973e9c4a3bb9a83a787e6370caea26c94222c18023fca79732f472ec
            // @lfy def/mcp/main.lfy:problems#problems:tool:a9b7494a79d88bd889cce42866936bb912530e77684203cac316c431a18f60f0
            |session, arguments| problems(session, arguments.optional("file")),
        ),
        // @lfy def/mcp/main.lfy:find#find:tool:676e7ecb17469c7ccc52c93dfa20240b7f60c68ffe5c225c334ce649260d3c82
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
            // @lfy def/mcp/main.lfy:find#find:tool:2f7784cc106602f975a8b522dfc056213500ed5cb79c837dde61e2c66aba3cc4
            // @lfy def/mcp/main.lfy:find#find:tool:9e6ccee8bf3cdcda1e9f14fb07ef0083f6b49f3e910bddf0534c5a120eb4f11a
            // @lfy def/mcp/main.lfy:find#find:tool:aed5897553c2e7ef22eaed9cdd2647d8cb7e1cf737e5006171dbe83b45233423
            // @lfy def/mcp/main.lfy:find#find:tool:26dc105fcfc8d9cc4ed181d1ae1172c0d0e88942453e1513fd22d3d6c8b3b095
            // @lfy def/mcp/main.lfy:find#find:tool:ee0ffd83b80c1fa9a6f6446550d0ede648668263c911d25b7cccde3b1c251207
            // @lfy def/mcp/main.lfy:find#find:tool:b969b67f01ae22cf235972b295dcf1c53ad993facbcd3ecfca3694f891b7749d
            |session, arguments| find(session, arguments.string("name")?),
        ),
        // @lfy def/mcp/main.lfy:entity#entity:tool:ddfe1dd35655350f03e7e5d17a49bf24ca5a74c140b6fa6634abee87255e84b1
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
            // @lfy def/mcp/main.lfy:entity#entity:tool:61bdac6fff6ab46c364dff94daa7e10d5c0d9a87f07673893bf796dbeff281a9
            // @lfy def/mcp/main.lfy:entity#entity:tool:79374b37a4c2b0e653d83228d8c5563ef6eb7a434e367323698fc7c8d6b4ddfe
            // @lfy def/mcp/main.lfy:entity#entity:tool:8732ebbd6469aef64d4210d2efa8e402b049c58ffd15e2917da0e2336fc4857e
            // @lfy def/mcp/main.lfy:entity#entity:tool:95f57cc552a1a5cb5ecce19b598d13f87a05c5bdfaaa6938a13310f2508831b6
            // @lfy def/mcp/main.lfy:entity#entity:tool:5829a685241aba8ca4ca4bf3f62ea2b3055e121c981de79669a9d7c3d83770cc
            // @lfy def/mcp/main.lfy:entity#entity:tool:a7c0ee0d3baa0c575fa939c2dc2d147543f450d6e97fe4e0cf89d5de4453291d
            |session, arguments| entity(session, arguments.string("name")?),
        ),
        // @lfy def/mcp/main.lfy:references#references:tool:5ebb0f45b314d193ac46c6e29257e1ab90664528e27df0504dea7fd4fa6e1a76
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
            // @lfy def/mcp/main.lfy:references#references:tool:62c6ea22831e62a7c1de4729a50b811d0a9924577bb61e484e912a1bb384ff47
            // @lfy def/mcp/main.lfy:references#references:tool:907da8e53ec21ffd2ba78ae2828dc026cfe99c3aa51c848998571262d0c7139d
            // @lfy def/mcp/main.lfy:references#references:tool:827893ae33cccbdccbb68193bc2659750928be808be56cdea6f6a3151bbe5920
            // @lfy def/mcp/main.lfy:references#references:tool:dd81133cca58c8ba8fc6b9dc72579d5233719d65c7212530254058936f847c45
            // @lfy def/mcp/main.lfy:references#references:tool:b4222e73bcbb5478afea50f42b71e34c9d17c5065af4fa14863c508322e95c80
            // @lfy def/mcp/main.lfy:references#references:tool:9ae2c8dcc1248d086202d8cd6bc7e4ce571330e106c86a53547eca02254d5430
            |session, arguments| references(session, arguments.string("name")?),
        ),
        // @lfy def/mcp/main.lfy:outline#outline:tool:df196ef41110173840cf5a55e8013acbaa7b8a4add66538a8d282417d6c6e91d
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
            // @lfy def/mcp/main.lfy:outline#outline:tool:e3e01391792508f856e25b42fb467e3af59ab0a2b5c55fed17f4980d92cfbd48
            // @lfy def/mcp/main.lfy:outline#outline:tool:f5e2e8d44cd795aa69322fef5a2f21e8c943fd631ec83fb04f50665b2a0d412f
            // @lfy def/mcp/main.lfy:outline#outline:tool:f45b9f5b80b0346a421a136e9a46d49daa083f4f889de7b0bbc94dba055b6376
            // @lfy def/mcp/main.lfy:outline#outline:tool:150963511f1c73e51401709017f75ec80e37e411880f968dadf301ed995494cd
            // @lfy def/mcp/main.lfy:outline#outline:tool:6114e21db6cb9375ba623e9ff0b359857133d98565b50e37b08ee737a3e0ac41
            // @lfy def/mcp/main.lfy:outline#outline:tool:532b2ad88427069a8c24f3a734fe21c29f9d0b4bcf57c523a2436c4d0228712f
            |session, arguments| outline(session, arguments.string("file")?),
        ),
        // @lfy def/mcp/main.lfy:grammar#grammar:tool:a1f3e7a6765ee6ebf710f01f93c94f4ff49e3834d81690e3ac0bd642932d98e5
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
            // @lfy def/mcp/main.lfy:grammar#grammar:tool:92e8668a21cecdde03eb20f1761cc2b4c96186551cd1896700ac586114b8641c
            // @lfy def/mcp/main.lfy:grammar#grammar:tool:bd5a9029211a8e2f3d3c41e446aba7cfaa1aa0a444f42df76c2d0255eb780df8
            // @lfy def/mcp/main.lfy:grammar#grammar:tool:5d01ad45f02d8f308978596b7cd1353c7c18b777428681b148a06bc1bdbfdd11
            // @lfy def/mcp/main.lfy:grammar#grammar:tool:4dc3ad8c479a0c30222640c742e8a2d038c354b7c49002f7e7418207a552f774
            // @lfy def/mcp/main.lfy:grammar#grammar:tool:1e0a1ad9253f2d958a5b86d8cc0eafcecedb8d8cf8244049d2761ea7ee21559e
            // @lfy def/mcp/main.lfy:grammar#grammar:tool:b7323dc3bdb5a4319278e80e814e49181832a99d94e88ba1d9c7a9a3a6d8b0e8
            |session, _| grammar(session),
        ),
        // @lfy def/mcp/main.lfy:units#units:tool:0aadf6dfa5e2e716dfd93a00d582dd6edd8f135da4e0026d898d1c9fbc0d2e5a
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
            // @lfy def/mcp/main.lfy:units#units:tool:f3ee60056fa5a656320793ea902ea7e5fdc0170e0f893584e6566fef2e0f74b3
            // @lfy def/mcp/main.lfy:units#units:tool:59e3afe0a4f39564b82a347fa5494defa7421ae95a945c19049ed8e307b1b0e8
            // @lfy def/mcp/main.lfy:units#units:tool:79659d60c809fbde8c5fd50f34e2d760f9ffae44bbd59b062481edd82fd42707
            // @lfy def/mcp/main.lfy:units#units:tool:a2e1c4ab8495bfed9ff4f22428462d6c61f0c1df2bb61f7b05ccacd5e3106f75
            // @lfy def/mcp/main.lfy:units#units:tool:81aae6abacf07fa5edf743b82fc37586ea135ec9ec9af2639c6e6662a0c9469f
            // @lfy def/mcp/main.lfy:units#units:tool:1ee7df6703e37c15ef56982f86d76fae7deacf742479bd28a9b3bde7ee66a1b2
            |session, arguments| units(session, arguments.optional("target")),
        ),
        // @lfy def/mcp/main.lfy:request#request:tool:b932a5387d73bc3b696af9ef6d890f649c2723a340779d696295b900d9fca321
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
            // @lfy def/mcp/main.lfy:request#request:tool:80bdad95bb8ed6450029bfec40af6b1575410be626ccd04f333dc62080d4e520
            // @lfy def/mcp/main.lfy:request#request:tool:6937dbc72b50f20142642a70d702a97291567a2c235b538ce49149e7311ef28c
            // @lfy def/mcp/main.lfy:request#request:tool:447785de268c3deb1569dbb3fc8eced0eb6b8754c5e7981995fe4c8d9c825d0c
            // @lfy def/mcp/main.lfy:request#request:tool:f1c8ff30fb778df56f920993cac9f434ca7d05174e08a4529c76ef50f0f34122
            // @lfy def/mcp/main.lfy:request#request:tool:806f418c79eeea1378264a7f704dd7542b5fafffc84e9d62cd6694870e3f424c
            // @lfy def/mcp/main.lfy:request#request:tool:b0e0cfd7d7f475f1bd8b173c972e686a43cabef5a4a9cc18b77eab7a994d2b5b
            |session, arguments| request(session, arguments.string("batch")?, arguments.optional("target")),
        ),
        // @lfy def/mcp/main.lfy:check#check:tool:d464f1453df5082e7edfdfbb7ed06ef5a75b5f41bf3d2cc1b1048f9b3891d745
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
            // @lfy def/mcp/main.lfy:check#check:tool:e4f4119453d4496182ba6b44764ff5d46999a348b556701ccfde1813cbb2f9c4
            // @lfy def/mcp/main.lfy:check#check:tool:551327521f11195dad0acd8b202685f4710f5c12578520e07845c6e3f9be0f9b
            // @lfy def/mcp/main.lfy:check#check:tool:3ebfec3582c2a4e6fb2b34dfd2e671754ab02a2ea3c59d857f9b920105f36bdf
            // @lfy def/mcp/main.lfy:check#check:tool:684b58b87a0a19dc2fdbf39f7e588c713ffb806ffa8ed9df048eb64354a7be4b
            // @lfy def/mcp/main.lfy:check#check:tool:2feecd005d1f5b032cc009fa850348ac4256213d1b2ce6bd9bfff1963661d055
            // @lfy def/mcp/main.lfy:check#check:tool:d01144c9d33cf8a9ac5454fa795448019dc9005930ef3c1f9e38f1bb5e2ef5da
            |session, arguments| check(session, arguments.string("unit")?, arguments.optional("target")),
        ),
        // @lfy def/mcp/main.lfy:output#output:tool:b0a617871e7a62075491124586cb3eb21fdf5b7eab26a6430d3f98231e0c4005
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
            // @lfy def/mcp/main.lfy:output#output:tool:1dd29e1542998f1ed18085546fcb9983add31779f4c443a70621a4c1468d4221
            // @lfy def/mcp/main.lfy:output#output:tool:3e79b4342df7d8ac276a343f02bf39ffd5636a96ef3ba4641642d00b806447e4
            // @lfy def/mcp/main.lfy:output#output:tool:07ca33ffd8e831873cb51292b3dbabd26eaffa50db2e76be6c27f702bcfa6a70
            // @lfy def/mcp/main.lfy:output#output:tool:1e4baa6263f16aec686a93f72ebe9e48822140f1dbbcc30be00356c10db27e08
            // @lfy def/mcp/main.lfy:output#output:tool:1efeaccaafd14ce38d6aacf01412cd2561c720263e5fcc9555e78c00c0bc642c
            // @lfy def/mcp/main.lfy:output#output:tool:355f2e4745bc2e753294ba137c3f91cae87ebf6bc265f34a566e5fd7b744b62a
            |session, arguments| output(session, arguments.string("name")?, arguments.optional("target")),
        ),
        // @lfy def/mcp/main.lfy:source#source:tool:84ab7d2fe8c5f162a2139b53780a62300c681cd2479bcb90131b0a724adde63c
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
            // @lfy def/mcp/main.lfy:source#source:tool:c89227ca5647c824538c9238aa374fe4c12052dabe906afddeed82f9e5733873
            // @lfy def/mcp/main.lfy:source#source:tool:09d86a7ebefb49c93cedccbd396bfe51c1fd158af3e4aea4f45843eebd4da3a2
            // @lfy def/mcp/main.lfy:source#source:tool:a161c47fda21f78a8bc4509efaf86c25a6d94898701da0c744c98302f2bfa14c
            // @lfy def/mcp/main.lfy:source#source:tool:f64e48f070058897b4b82d2d2649c81b9155f184931f4b71a81c2bb9163b850a
            // @lfy def/mcp/main.lfy:source#source:tool:36b64abf8443d64fac524ad81fcaa9c9e1ece561cdf7b0a6ff1e66dba001fac3
            // @lfy def/mcp/main.lfy:source#source:tool:933096756b3b232e53313f802792772462a7923e6772c081a67cd7f54440b7e4
            |session, arguments| source(session, arguments.string("file")?, counted(arguments.string("line")?, "line")?),
        ),
        // @lfy def/mcp/main.lfy:changes#changes:tool:f9367e7588952c7d86d18bd7560aa34707b3020c4e1eea5e6222c369e96d0175
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
            // @lfy def/mcp/main.lfy:changes#changes:tool:c47a5f505aac19bc1562dbe51150e5f12616d97f38ae91c3e9acd5315ae93742
            // @lfy def/mcp/main.lfy:changes#changes:tool:f5c4e810ed5d871ec0a3742590856586a4bbb180829fa0a37f5bec6655f6667d
            // @lfy def/mcp/main.lfy:changes#changes:tool:199fd81ae10f17deba5d8ef76103f4218e8a4e4e472c1f28d08a165be748eb28
            // @lfy def/mcp/main.lfy:changes#changes:tool:1b481949890f42d1353e622ca00080011c52df05703303c8a4a0073f55d96380
            // @lfy def/mcp/main.lfy:changes#changes:tool:7956a05a34c47e5177fb4661eb64d4384e77763432be57f4b4db3f2d15aa972b
            // @lfy def/mcp/main.lfy:changes#changes:tool:0dc4b671044e5b169084445b22daec236489f564fb85985cca66cc6a88b680d5
            |session, arguments| changes(session, arguments.string("unit")?, arguments.optional("target")),
        ),
        // @lfy def/mcp/main.lfy:review#review:tool:ab3762ae28ef238ec7713c28c7c06cbe690bf3ce286015cb4170d940c8b61c43
        // @lfy def/mcp/main.lfy:review#review:tool:90f43a9d7a341c4448c88bdb65846b1958c620269b4928c1820b8bdd36a04645
        // @lfy def/mcp/main.lfy:review#review:tool:2abf93ea0875a15a836506dce49781a69fda366ebd1d649a632fcd18cd90aaba
        Registered::new(
            "review",
            "The full review request for one batch: every criterion and test with the regions generated for its entities, and how to report",
            // @lfy def/mcp/main.lfy:review#review:tool:4db83e86e12ebe1d2b0275a0875d451913e2e33a2985e19be1239eeb55afcf55
            vec![
                // @lfy def/mcp/main.lfy:review#review:tool:176e2dae9d3fa2eeb0d9e1f4cb3f69e21eddf6a4ee54eeec769831da57d33106
                // @lfy def/mcp/main.lfy:review#review:tool:f12b54937440941b301f9813502fcf02c0f5c15985fa86f3c18d3be8971e926c
                // @lfy def/mcp/main.lfy:review#review:tool:26448cfe30979765e8825f454475ab554f07d764985e474795c7f379bcb790b8
                ToolArgument::new("batch", "the identifier of a batch, as elfie_units lists it, or the stem of one of its units", "string", true),
                // @lfy def/mcp/main.lfy:review#review:tool:c9eb9c029bfc76ba91183426864af318ca0ba9c40d66aa878456a1365ec79882
                ToolArgument::new("target", "the identifier of the batch's target; the only target when left out", "string", false),
            ],
            // @lfy def/mcp/main.lfy:review#review:tool:83a12b058db342a977b5e54b6056585a71ab4f1b28ab54c2415181e18605cdc0
            // @lfy def/mcp/main.lfy:review#review:tool:6136567dbb52c10db4b93ba491d3db66273bcb1eb7671f8831bb2f8567157ce6
            // @lfy def/mcp/main.lfy:review#review:tool:89fb67b957d123b52aefe043899a9d05d33fe40f038e700e9a3c37c4c250bdf1
            // @lfy def/mcp/main.lfy:review#review:tool:3d16d076268d4bb0d06e54067ea264107a204761e51fedb45c43d4da6266e867
            // @lfy def/mcp/main.lfy:review#review:tool:783352704aa244702e71b187abc170d3a55eb7ee3b01dddbdffd6b21219355e8
            // @lfy def/mcp/main.lfy:review#review:tool:5c26dbd79645c2274940ae02bf61ebc5954a4c194e657a89b203f1976fb73dce
            |session, arguments| review(session, arguments.string("batch")?, arguments.optional("target")),
        ),
        // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:115f9221b2166c3faef162ef9aae5d3236ffdf8145ae1a9a02254b22000ce0bb
        // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:14933b1ce190a0da8af31a4b31f6a8dc1a38277ec20a83476f9cb01e93f24e8e
        // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:afb254701ec23fe66c0d0f33c90501dc2218272c2e25c81d4aa6e8cb4386e1a9
        Registered::new(
            "globalReview",
            "The review request for every global criterion and test, once for the whole program, with the regions that answer for each",
            // Its one parameter is the session, so it holds no argument, and nothing further
            // is asked of an argument's name, type, description, or whether it is needed.
            // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:82e3fa7ae3f9252bf237de6847f6e4f376d8359dbf5534ae09480fd3423e9c90
            // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:37bd7f504566a0de5dd33fd249659e845dfa71e53baf0404c88d78abdb413040
            // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:dfd01992e60e996e02f3abdace15928aa26c171d40598e6ef831122cca7dc13e
            // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:e6002fa85fc2e5cb7599cd691c03f7c44cd15a3a91606fb73ee48193ac48e0d4
            // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:5f7ebced7900d0c3e6313eb42c7094e11d5a40a106f18a1451685cf793ba8c45
            vec![],
            // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:adf5c6d4bb588c1e6aa88fc77c9d434f6fedc44153e04df85c0ad504dba2ff6f
            // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:116e7e050e8da1a1cfcfd7ac5bbea519c9e62b5349a6a40b2b8f69de71ace555
            // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:0980f2712e949b1e8d0dde54b79cb7a6868b3740b8355baa6f0e63d4bec17d0a
            // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:1bd58ca877dd1efffc6fcfb3da9e0bc8a89f596f6bc8293a4de3c8e6ebe8a903
            // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:dd97712ea04dd944092a306fe48514d200aca70d8859b91716e78311d351b34d
            // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:a465a324a0a88351aa9aceb3ac8748f741a2f91ba57cccc57bc75ea168bbac9b
            |session, _| global_review(session),
        ),
    ]
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

/// The protocol server: one session behind a lock, and the tools.
struct Server {
    session: Mutex<Session>,
    tools: Vec<Registered>,
}

impl Server {
    // Decision: the definition loads the workspace on initialize; it is loaded when the
    // server is built, just before the initialize exchange, which the agent cannot tell
    // apart and which keeps the protocol library's handshake untouched.
    // @lfy def/mcp/main.lfy:serve#serve:serve:0d7ad5672274c815b6ad256268ff29d8425eb7ed8114bb7c9ea52bd41bedde9b:2
    fn new(root: &Path) -> Server {
        Server {
            session: Mutex::new(Session::load(root)),
            tools: tools(),
        }
    }

    /// One call: the session is refreshed, then the tool runs.
    ///
    /// The one session is behind a lock, so a call holds it until it answers and the next
    /// waits: calls are handled one at a time, in the order received.
    // @lfy def/mcp/main.lfy:serve#serve:serve:b40b7d13aaacdbb22af8b5a419d2c34ed6ffe19d0e12598b4aa56f9dee24fe22
    fn call(&self, request: &CallToolRequestParams) -> Result<CallToolResult, ErrorData> {
        let Some(registered) = traits::find(&self.tools, &request.name) else {
            return Err(ErrorData::invalid_params(format!("no tool named {}", request.name), None));
        };
        // @lfy def/mcp/main.lfy:serve#serve:serve:995535b9c7a11554f96d88793004bc092de31525b26cd0ca15ce449e17d8e386
        let mut session = self.session.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        session.refresh();
        let empty = JsonObject::new();
        let arguments = request.arguments.as_ref().unwrap_or(&empty);
        let result = call_tool(registered, &session, arguments);
        let content = vec![ContentBlock::text(result.text)];
        Ok(if result.is_error { CallToolResult::error(content) } else { CallToolResult::success(content) })
    }
}

impl ServerHandler for Server {
    /// Tools only: no resources and no prompts.
    // @lfy def/mcp/main.lfy:serve#serve:serve:4d105e14d91381963764973d69e9e54abacde103ac17990250a98dadf1ced8cb
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::V_2025_06_18) // @lfy def/mcp/main.lfy:Protocol
            .with_server_info(Implementation::new("elfie", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "The Elfie agent server. elfie_problems, elfie_find, elfie_entity, elfie_references, and elfie_outline read the program; elfie_grammar gives the language; elfie_units, elfie_request, elfie_check, elfie_review, and elfie_globalReview drive compilation; elfie_output, elfie_source, and elfie_changes read what the last acceptance recorded. Nothing here writes a file.",
            )
    }

    // @lfy def/mcp/main.lfy:serve#serve:serve:e990effaf8e2dc344e48fb8a9fd67f96fe167fdecb4798c66784cab9b740766f
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(traits::list(&self.tools)))
    }

    // @lfy def/mcp/main.lfy:serve#serve:serve:7a38d3ecf5e074e43e215152a74b94923a889a14c60638b479fd67135299e336
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        self.call(&request).map(CallToolResponse::from)
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

/// The plan of the workspace with [`generation::source_maps_of`] the workspace, and as its
/// violated units the violated units of the last [`GlobalReview`].
// Decision: a unit's outputs are read off the plan, and the violated units are an input of
// planning, so the plan is made once with no violated unit and again only when the last
// global review names one; a project with none, which is every project until a global
// review finds one, is planned once.
// @lfy def/mcp/main.lfy:units#units:units:e35cb874630f014bcf475b9b10f74413ed36fe8b669e6a8194fd1ca57d8cf251
fn plan_of(workspace: &Workspace) -> Planned {
    let maps = generation::source_maps_of(workspace, None); // @lfy def/mcp/main.lfy:units#units:units:e35cb874630f014bcf475b9b10f74413ed36fe8b669e6a8194fd1ca57d8cf251
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
    let planned = plan_of(&session.workspace);
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
    let planned = plan_of(&session.workspace);
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
    Ok(request_for(&planned.program, plan, &plan.batches[index]).instructions) // @lfy def/mcp/main.lfy:request#request:request:4dee59f1344d1563846d889e2ed940f684d08de14d3da6460c92f8859ad610de
}

/// Whether the outputs on disk satisfy the request for one unit, and what is wrong when
/// they do not. Nothing is written; the CLI records source maps.
// @lfy def/mcp/main.lfy:check#check:check:77f6c4a5084647dc21eb7114983b08f9ebbf27471071fee839c6113ba37cdfab
pub fn check(session: &Session, unit: &str, target: Option<&str>) -> Result<String, String> {
    let planned = plan_of(&session.workspace);
    let (workspace, plan) = (planned.workspace(), &planned.plan);
    let index = resolve_unit(workspace, plan, unit, target)?;
    let request = request_of_unit(&planned.program, plan, index);
    let outputs = outputs_of(workspace, plan, index); // @lfy def/mcp/main.lfy:check#check:check:77f6c4a5084647dc21eb7114983b08f9ebbf27471071fee839c6113ba37cdfab
    // What the last acceptance recorded for the target's other units, so that an output
    // another unit is to write is seen as that unit's rather than as this one's.
    // @lfy def/mcp/main.lfy:check#check:check:77f6c4a5084647dc21eb7114983b08f9ebbf27471071fee839c6113ba37cdfab
    let source = workspace.files[plan.units[index].file].path.clone();
    let identifier = workspace.targets[plan.units[index].target].identifier.clone();
    let recorded: Vec<SourceMap> = generation::source_maps_of(workspace, Some(&identifier))
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
    let maps = generation::source_maps_of(workspace, None); // @lfy def/mcp/main.lfy:output#output:output:429055d8371ef1095c71aff08d676ca7f8b8e25c1c2f54be17304040862780ab
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
    for map in generation::source_maps_of(workspace, None) {
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
    let planned = plan_of(&session.workspace);
    let workspace = planned.workspace();
    let index = unit_of_stem(workspace, &planned.plan, unit, target)?;
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

/// The full review request for one batch: every criterion and test with the regions
/// generated for its entities, and how to report.
///
/// The problems of the plan are not mentioned: a review reads the outputs that exist
/// against what was asked of them, which a plan that could not be made whole does not
/// change.
// @lfy def/mcp/main.lfy:review#review:review:5c61bb2a8fa4f5d02a3219d0c4a1f8d385dbad4a9caf2a52fa73d59c10014617
pub fn review(session: &Session, batch: &str, target: Option<&str>) -> Result<String, String> {
    let planned = plan_of(&session.workspace);
    let plan = &planned.plan;
    // The batch whose identifier the name is is meant before any batch holding a unit with
    // it as a stem; an error says which when neither names one, or the target is ambiguous.
    // @lfy def/mcp/main.lfy:review#review:review:a7e16dee761e4f416f1040c2c3f90c5e87e1352241e235f0eea4146033eccc99
    // @lfy def/mcp/main.lfy:review#review:review:ca0a25d6a947aa41d3cbf9399a371f681224987d6109d85e16a0b7f8a85db14c
    let index = resolve_batch(planned.workspace(), plan, batch, target)?;
    // @lfy def/mcp/main.lfy:review#review:review:730d473b155ea22d63d998272263ed31e6e8ced67d540ee5b52bd3531d434584
    Ok(
        generation::review(
            &planned.program,
            plan,
            &plan.batches[index],
            &planned.maps,
            None,
        )
        .instructions,
    )
}

/// The review request for every global criterion and test, once for the whole program, with
/// the regions that answer for each.
// @lfy def/mcp/main.lfy:globalReview#globalReview:globalReview:b93638e7046f17fc2cf321187664d92150028acae44164992beee7e46349fb1c
pub fn global_review(session: &Session) -> Result<String, String> {
    let planned = plan_of(&session.workspace);
    // @lfy def/mcp/main.lfy:globalReview#globalReview:globalReview:f656c023948494da8d316780c15c2c1b96e1512caafb62d843dc6295be0d89be
    if planned.program.criteria.is_empty() && planned.program.tests.is_empty() {
        return Ok("no global criterion or test exists in the program".to_string());
    }
    // @lfy def/mcp/main.lfy:globalReview#globalReview:globalReview:b93638e7046f17fc2cf321187664d92150028acae44164992beee7e46349fb1c
    Ok(generation::global_review(&planned.program, &planned.maps).instructions)
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

    /// The name of every tool the server lists, in the order the definition declares the
    /// fns that carry `tool`.
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

    // @lfy def/mcp/main.lfy:serve#serve:serve:e990effaf8e2dc344e48fb8a9fd67f96fe167fdecb4798c66784cab9b740766f
    #[test]
    fn the_tools_are_listed_with_their_arguments() {
        let names: Vec<String> = tools().into_iter().map(|registered| registered.tool.name).collect();
        assert_eq!(names, TOOL_NAMES);
        let listed = traits::list(&tools());
        assert_eq!(listed.len(), 14);
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

        // @lfy def/mcp/main.lfy:serve#serve:serve:e990effaf8e2dc344e48fb8a9fd67f96fe167fdecb4798c66784cab9b740766f
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

    // @lfy def/mcp/main.lfy:find#find:tool:aed5897553c2e7ef22eaed9cdd2647d8cb7e1cf737e5006171dbe83b45233423
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
        // @lfy def/mcp/main.lfy:outline#outline:tool:f45b9f5b80b0346a421a136e9a46d49daa083f4f889de7b0bbc94dba055b6376
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
        // @lfy def/mcp/main.lfy:outline#outline:tool:e3e01391792508f856e25b42fb467e3af59ab0a2b5c55fed17f4980d92cfbd48
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
        let planned = plan_of(&session.workspace);
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
        // @lfy def/mcp/main.lfy:source#source:tool:c89227ca5647c824538c9238aa374fe4c12052dabe906afddeed82f9e5733873
        // @lfy def/mcp/main.lfy:source#source:tool:09d86a7ebefb49c93cedccbd396bfe51c1fd158af3e4aea4f45843eebd4da3a2
        let result = call(&session, "elfie_source", serde_json::json!({ "file": "out/b.rs", "line": 2 }));
        assert!(!result.is_error, "{}", result.text);
        assert_eq!(result.text, "def/b.lfy:1 B");
        // An argument of the wrong type is an error naming it, and the fn is not called.
        // @lfy def/mcp/main.lfy:source#source:tool:a161c47fda21f78a8bc4509efaf86c25a6d94898701da0c744c98302f2bfa14c
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
        let planned = plan_of(&session.workspace);
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
        let planned = plan_of(&session.workspace);
        let index = unit_of_stem(planned.workspace(), &planned.plan, "b", None).unwrap();
        // The file as it is now is the file the outputs were generated from, so nothing
        // differs.
        // @lfy def/mcp/main.lfy:changes#changes:changes:35a395b2402b76e8ae9d6c636a82478d28714ed6e6459fe96e70847ff73470a4
        let text = fs::read_to_string(fixture.root.join("def/b.lfy")).unwrap();
        assert!(generation::changes(&planned.program, &planned.plan.units[index], Some(&text)).is_empty());
    }

    // @lfy def/mcp/main.lfy:review#review:review:730d473b155ea22d63d998272263ed31e6e8ced67d540ee5b52bd3531d434584
    #[test]
    fn review_gives_the_review_request_of_one_batch() {
        let fixture = compiled_project();
        let session = fixture.session();
        let instructions = review(&session, "b+1", None).unwrap();
        assert!(instructions.contains("Reviewing the batch"), "{instructions}");
        assert!(instructions.contains("def/a.lfy"), "{instructions}");
        assert_eq!(review(&session, "a", None).unwrap(), instructions);
        assert_eq!(review(&session, "a", Some("rust")).unwrap(), instructions);
        // No batch has that identifier or holds a unit with that stem, or the target is
        // ambiguous.
        // @lfy def/mcp/main.lfy:review#review:review:ca0a25d6a947aa41d3cbf9399a371f681224987d6109d85e16a0b7f8a85db14c
        let error = review(&session, "c", None).unwrap_err();
        assert!(error.contains("no batch has the identifier c"), "{error}");
        let error = review(&session, "a", Some("go")).unwrap_err();
        assert!(error.contains("go is not a target"), "{error}");
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
        let planned = plan_of(&session.workspace);
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
        let planned = plan_of(&session.workspace);
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
        let planned = plan_of(&session.workspace);
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
