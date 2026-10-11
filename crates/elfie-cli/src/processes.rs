// @lfy def/cli/main.lfy:main
//! Running the compiler, the verifier, and a command of the target: the hosted session each
//! run is given, the environment it is handed, the lines it writes as they arrive, and the
//! process group it is stopped with.

use super::*;
use crate::copies::Where;

/// What a finished command left: its code, -1 when it ended by a signal or could not be
/// started, everything it wrote, held whole, and whatever it submitted through the agent
/// server before its session was closed.
// @lfy def/cli/main.lfy:main#main:main:f507a20d5658ce7b158a66e802e963debf7d67da97fd22a6992b840100d837b2
pub(crate) struct Exit {
    pub(crate) code: i32,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    /// What the run recorded through the agent server — the reviews it submitted and the
    /// outcome it submitted — as closing its session gave it back.
    // @lfy def/cli/main.lfy:main#main:main:b4fcd8255dc77a6812ba729d68dac80c3c551fa5f203c2d68df2bab37b05ee57
    pub(crate) record: SessionRecord,
    /// The agent session id the run was given as ELFIE_SESSION, so that a resuming run
    /// after it can go on from the same one.
    // @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
    // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
    pub(crate) session: String,
}

/// Every process group a batch has running: the compiler, the verifier, or a command of the
/// target, so that stopping the batch stops them and everything they started.
// @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
// @lfy def/cli/main.lfy:main#main:main:7abeb271cb2f0100d93f668ddc16303e063bcd2d3df7de58cd6b6d23d0f9ea21
#[derive(Default)]
pub(crate) struct Running {
    /// The leaders of the groups still running, by the number each was given.
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    groups: Mutex<BTreeMap<u64, u32>>,
    /// The number the next group to start takes.
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    next: Mutex<u64>,
}

impl Running {
    /// One group remembered while it runs, forgotten when the handle is dropped.
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    pub(crate) fn started(&self, leader: u32) -> Group<'_> {
        let at = {
            let mut next = self.next.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let at = *next;
            *next += 1;
            at
        };
        self.groups.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(at, leader);
        Group { running: self, at }
    }

    /// Every group still running stopped, with every process of each.
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    // @lfy def/cli/main.lfy:main#main:main:7abeb271cb2f0100d93f668ddc16303e063bcd2d3df7de58cd6b6d23d0f9ea21
    pub(crate) fn stop(&self) {
        let leaders: Vec<u32> = {
            let groups = self.groups.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            groups.values().copied().collect()
        };
        for leader in leaders {
            // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
            stop_group(leader);
        }
    }
}

/// One process group of a batch while it runs.
// @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
pub(crate) struct Group<'r> {
    running: &'r Running,
    at: u64,
}

impl Drop for Group<'_> {
    /// A run that has ended is no longer a group to stop.
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    fn drop(&mut self) {
        self.running
            .groups
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.at);
    }
}

/// The agent session a resuming run goes on from: for the compiler and for the verifier
/// apart, the session of that one's first run on the batch in this directory.
///
/// Every resuming run of a chain carries the same id, because a resuming run passes on its
/// own ELFIE_RESUME when it had one and its ELFIE_SESSION otherwise. A batch that runs again
/// from a new copy begins a new chain, since this is made afresh for each directory.
// @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
// @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
#[derive(Default)]
pub(crate) struct Resuming {
    /// The session the compiler's runs on the batch here go on from.
    // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
    compiler: Option<String>,
    /// The session the verifier's runs on the batch here go on from.
    // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
    verifier: Option<String>,
}

impl Resuming {
    /// What a resuming run of one role goes on from after the run that just ended: that run's
    /// own `ELFIE_RESUME` when it had one, and its `ELFIE_SESSION` otherwise.
    ///
    /// `resumed` is what the run that ended carried, so a run that went on from a session
    /// passes that session on and a fresh run in the middle of a chain begins a new one: after
    /// a run that was killed and a fresh run after it, the next resuming run goes on from the
    /// fresh run rather than from the one before it.
    // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
    pub(crate) fn note(&mut self, role: Role, resumed: Option<&str>, session: &str) {
        let chain = match role {
            Role::Compiler => &mut self.compiler,
            Role::Verifier => &mut self.verifier,
        };
        // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
        let id = resumed.or(if session.is_empty() { None } else { Some(session) });
        // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
        if let Some(id) = id {
            *chain = Some(id.to_string());
        }
    }

    /// The id a resuming run of one role goes on from; nothing when that role has not run on
    /// the batch here.
    // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
    pub(crate) fn of(&self, role: Role) -> Option<&str> {
        match role {
            Role::Compiler => self.compiler.as_deref(),
            Role::Verifier => self.verifier.as_deref(),
        }
    }
}

/// The platform shell and the flag it takes a command line after: `sh -c` on Unix and
/// `cmd /C` on Windows.
// @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
// @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
pub(crate) fn shell() -> (&'static str, &'static str) {
    match cfg!(windows) {
        true => ("cmd", "/C"),
        false => ("sh", "-c"),
    }
}

/// A run made the leader of a process group of its own, so that stopping it stops everything
/// it started: a process group on Unix, and on Windows a process started with the
/// CREATE_NEW_PROCESS_GROUP creation flag, which is 0x00000200.
// @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
pub(crate) fn leads_group(spawning: &mut Process) {
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(spawning, 0);
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(spawning, 0x0000_0200);
}

/// Every process of a process group stopped, through the platform's own way of stopping one:
/// `kill` of the group on Unix, and `taskkill /T /F` of the leader on Windows.
// @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
pub(crate) fn stop_group(leader: u32) {
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    let mut stopping = match cfg!(windows) {
        true => {
            let mut stopping = Process::new("taskkill");
            stopping.arg("/T").arg("/F").arg("/PID").arg(leader.to_string());
            stopping
        }
        false => {
            let mut stopping = Process::new("sh");
            stopping.arg("-c").arg(format!("kill -KILL -- -{leader}"));
            stopping
        }
    };
    let _ = stopping.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
}

/// One stream of the compiler or the verifier, each line shown as it arrives prefixed by the
/// batch, appended to the log, and kept whole. With --json it is shown on standard error, so
/// that standard output stays one JSON object per line. Every line of what the command wrote
/// reaches the log, standard error among them, so a run reads after the fact as it read while
/// it ran.
// @lfy def/cli/main.lfy:main#main:main:f507a20d5658ce7b158a66e802e963debf7d67da97fd22a6992b840100d837b2
// @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
pub(crate) fn watch<R: io::Read + Send + 'static>(
    pipe: R,
    batch: &str,
    keep: bool,
    to_stdout: bool,
    on: bool,
    stream: Stream,
    logs: Vec<PathBuf>,
) -> JoinHandle<String> {
    // Two spaces, the batch, a space, a bar, and a space.
    // @lfy def/cli/main.lfy:main#main:main:f747b7755db17f8d8a69c66bccc1af569ec45c9542e442d92a729acb82938344
    let plain = format!("  {batch} \u{2502} ");
    // The prefix of a line that came from standard output is muted, and one that came from
    // standard error is warning.
    // @lfy def/cli/main.lfy:main#main:main:714e27f2331360d1b071e2a8bdf90f345382ea99e062330f8068d835170e9d81
    // @lfy def/cli/main.lfy:main#main:main:f9dd575e7bd97b0a74ebebee78b614a642f4aefa5b071efda9788975324bccbf
    let tone = match stream {
        Stream::Output => Tone::Muted,
        Stream::Error => Tone::Warning,
    };
    let prefix = paint(&plain, tone, on);
    std::thread::spawn(move || {
        let mut kept = String::new();
        for line in io::BufReader::new(pipe).lines().map_while(Result::ok) {
            // The line itself is shown exactly as the command wrote it, as it arrives. One
            // `println!` writes the prefix and the line together under the lock on the
            // stream, so a line of another batch never mixes into it.
            // @lfy def/cli/main.lfy:main#main:main:f747b7755db17f8d8a69c66bccc1af569ec45c9542e442d92a729acb82938344
            // @lfy def/cli/main.lfy:main#main:main:f507a20d5658ce7b158a66e802e963debf7d67da97fd22a6992b840100d837b2
            // @lfy def/cli/main.lfy:main#main:main:3a1f341e1f488489eebade96f9e410811db493057811f2c78e1ceb8d3a8ae77d
            if to_stdout {
                println!("{prefix}{line}");
            } else {
                eprintln!("{prefix}{line}");
            }
            // The line the log holds is the line as printed with colors off.
            // @lfy def/cli/main.lfy:main#main:main:3082965661f2495d1e5e9d8a2177fc2ca759320db1b83c7e7bc0605b9be65f3f
            // @lfy def/cli/main.lfy:main#main:main:c2a64cff68c468890c2b64bb46be8472891c54f12fce3e4a2d404d4b2e5f0969
            append_to(&logs, &format!("{plain}{line}"));
            if keep {
                kept.push_str(&line);
                kept.push('\n');
            }
        }
        kept
    })
}

/// The agent server every run of a compile or a verify works through, hosted on this machine
/// for the length of that command.
///
/// It is hosted when the first agent is about to run and never before, so a command that runs
/// neither the compiler nor the verifier hosts nothing; what hosting it came to is kept, so
/// that the host is made once and a failure to make it is the same failure for every run.
// @lfy def/cli/main.lfy:main#main:main:5724f5824450ab53811f66b437b4a0ea4d89fea52c17be8df51e351a55ae8fe9
// @lfy def/cli/main.lfy:main#main:main:528aaa4cb8488c953d03050aadaa0d8bc000afe6ba74f6af3daa9146ffb6c4c4
pub(crate) struct Server {
    /// The root, where every session's MCP config file is written.
    // @lfy def/cli/main.lfy:main#main:main:db24670f7a47cc1d0d13a51de65a69c1469315a12c8359b5ba6d6f0d620ded13
    pub(crate) root: PathBuf,
    /// What hosting came to, once something needed it: the host, or why there is none.
    // @lfy def/cli/main.lfy:main#main:main:5724f5824450ab53811f66b437b4a0ea4d89fea52c17be8df51e351a55ae8fe9
    pub(crate) hosted: Mutex<Option<Result<Arc<Host>, String>>>,
}

impl Server {
    // @lfy def/cli/main.lfy:main
    pub(crate) fn new(root: PathBuf) -> Server {
        Server { root, hosted: Mutex::new(None) }
    }

    /// The server hosted for the length of this command, made the first time a run needs it
    /// and the same one after that.
    // @lfy def/cli/main.lfy:main#main:main:5724f5824450ab53811f66b437b4a0ea4d89fea52c17be8df51e351a55ae8fe9
    // @lfy def/cli/main.lfy:main#main:main:528aaa4cb8488c953d03050aadaa0d8bc000afe6ba74f6af3daa9146ffb6c4c4
    pub(crate) fn host(&self) -> Result<Arc<Host>, String> {
        let mut held = self.hosted.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // @lfy def/cli/main.lfy:main#main:main:5724f5824450ab53811f66b437b4a0ea4d89fea52c17be8df51e351a55ae8fe9
        held.get_or_insert_with(|| elfie_mcp::host().map(Arc::new)).clone()
    }

    /// Every session that is still open ended and the server stopped, once the last run has
    /// returned; nothing to stop when nothing was ever hosted.
    // @lfy def/cli/main.lfy:main#main:main:77f1f3a8ea3c2d7f433ee642b0418c00d1ce2b7dd1dca0106b299cb12959c12b
    pub(crate) fn stop(&self) {
        let mut held = self.hosted.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // @lfy def/cli/main.lfy:main#main:main:77f1f3a8ea3c2d7f433ee642b0418c00d1ce2b7dd1dca0106b299cb12959c12b
        if let Some(Ok(host)) = held.take() {
            (host.stop)();
        }
    }
}

/// The tools that run a target's command, in the order the agent server lists them, so that
/// a run is told which of them its session was offered.
// @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
pub(crate) const COMMAND_TOOLS: [&str; 4] = ["elfie_build", "elfie_test", "elfie_lint", "elfie_format"];

/// The command tools among the names a session is offered, in the order the server lists
/// them, joined by spaces; empty when it was offered none.
// @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
pub(crate) fn command_tools_of(offered: &[String]) -> String {
    // @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
    offered
        .iter()
        .filter(|name| COMMAND_TOOLS.contains(&name.as_str()))
        .map(String::as_str)
        .collect::<Vec<&str>>()
        .join(" ")
}

/// The word a role goes by in a file name and in the log.
// @lfy def/cli/main.lfy:main#main:main:db24670f7a47cc1d0d13a51de65a69c1469315a12c8359b5ba6d6f0d620ded13
pub(crate) fn word_of(role: Role) -> &'static str {
    match role {
        Role::Compiler => "compiler",
        Role::Verifier => "verifier",
    }
}

/// The name of the variable that carries the agent session id of a run.
// @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
pub(crate) const SESSION: &str = "ELFIE_SESSION";

/// The name of the variable that carries the agent session id a resuming run goes on from.
// @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
pub(crate) const RESUME: &str = "ELFIE_RESUME";

/// An agent session id new for one run: a version 4 UUID made from the operating system's
/// secure random source, spelled as 36 lowercase characters in the 8-4-4-4-12 form.
// @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
pub(crate) fn session_id() -> Result<String, String> {
    // @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
    let mut bytes = [0u8; 16];
    // The source is the one the operating system offers, whichever platform this is.
    getrandom::fill(&mut bytes)
        .map_err(|error| format!("the operating system's secure random source cannot be read: {error}"))?;
    Ok(uuid_of(bytes))
}

/// Sixteen random bytes spelled as a version 4 UUID: the version in the high nibble of the
/// seventh byte and the variant in the two high bits of the ninth, then 36 lowercase
/// characters in the 8-4-4-4-12 form.
// @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
pub(crate) fn uuid_of(mut bytes: [u8; 16]) -> String {
    // @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    // @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// What running one agent takes, held apart from the rest of a compile so that a batch can
/// stream its compiler and its verifier while the other batches stream theirs: nothing here
/// changes once a compile has begun.
// @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
#[derive(Clone)]
pub(crate) struct Agent {
    pub(crate) json: bool,
    pub(crate) style: Paint,
    pub(crate) logs: Vec<PathBuf>,
    /// The path of the running executable, so the agent server an agent starts is this same
    /// program.
    // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
    pub(crate) elfie: PathBuf,
    /// The agent server every run works through, shared by every batch.
    // @lfy def/cli/main.lfy:main#main:main:5724f5824450ab53811f66b437b4a0ea4d89fea52c17be8df51e351a55ae8fe9
    pub(crate) server: Arc<Server>,
}

impl Agent {
    // @lfy def/cli/main.lfy:main
    pub(crate) fn new(json: bool, style: Paint, logs: Vec<PathBuf>, root: &Path) -> Agent {
        // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
        let elfie = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("elfie"));
        // @lfy def/cli/main.lfy:main#main:main:5724f5824450ab53811f66b437b4a0ea4d89fea52c17be8df51e351a55ae8fe9
        Agent { json, style, logs, elfie, server: Arc::new(Server::new(root.to_path_buf())) }
    }

    /// The MCP config file of one session, written under the root before the run starts:
    /// `elfie-requests/<batch>.<role>.mcp.json`, holding one object naming the session's URL
    /// and the bearer token every request to it must carry.
    // @lfy def/cli/main.lfy:main#main:main:db24670f7a47cc1d0d13a51de65a69c1469315a12c8359b5ba6d6f0d620ded13
    // @lfy def/cli/main.lfy:main#main:main:44a95955daee5d89e4435d08b9cc7de1e517c400cca66ecc9ac0f0aecd83045b
    pub(crate) fn write_config(&self, host: &Host, url: &str, label: &str, role: Role) -> PathBuf {
        let directory = requests_directory(&self.server.root);
        // @lfy def/cli/main.lfy:main#main:main:db24670f7a47cc1d0d13a51de65a69c1469315a12c8359b5ba6d6f0d620ded13
        let path = directory.join(format!("{}.{}.mcp.json", file_name_of(label), word_of(role)));
        // One object with the key mcpServers, holding one object with the key elfie, holding
        // the type http, the session's URL, and the Authorization header.
        // @lfy def/cli/main.lfy:main#main:main:44a95955daee5d89e4435d08b9cc7de1e517c400cca66ecc9ac0f0aecd83045b
        let value = serde_json::json!({
            "mcpServers": {
                "elfie": {
                    "type": "http",
                    "url": url,
                    "headers": { "Authorization": format!("Bearer {}", host.token) },
                }
            }
        });
        let text = format!("{}\n", serde_json::to_string_pretty(&value).unwrap_or_default());
        // @lfy def/cli/main.lfy:main#main:main:97749da3fde520bf9fb6a1d4a7f1782131dc4eea08223f3ea0036e11724b6e42
        if let Err(error) = fs::create_dir_all(&directory).and_then(|()| fs::write(&path, &text)) {
            append_to(&self.logs, &format!("{} could not be written: {error}", path.display()));
        }
        path
    }

    /// Streams one agent, the compiler or the verifier, once for one batch: the instructions
    /// as its input, the batch's directory as its directory, and ELFIE_ROOT (the batch's
    /// directory), ELFIE_BATCH, ELFIE_UNITS (the stems, space separated), ELFIE,
    /// ELFIE_MCP_CONFIG, and ELFIE_COMMAND_TOOLS as its environment. Its standard output is
    /// the report. The code is -1 when the command could not be started or the agent server
    /// could not be hosted.
    ///
    /// One hosted session is opened for the run just before it starts, bound to what the run
    /// is, and closed once it has returned, before anything reads its outcome; what it
    /// submitted through the server comes back with its output. Its config file is removed
    /// once the run has returned, since the token it holds no longer opens anything.
    ///
    /// ELFIE_SESSION is an agent session id new for the run, and ELFIE_RESUME the id a
    /// resuming run goes on from, set only for the runs [`Resuming`] knows of and removed
    /// from what was inherited otherwise.
    ///
    /// These are the whole of what it adds: a batch running in a copy of the root reads
    /// the root's history through the `.git` file the copy holds, so nothing of git is set
    /// here and a repository one of its own commands makes elsewhere stays its own.
    // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
    // @lfy def/cli/main.lfy:main#main:main:0a2bb4066f627b6e8b99aaad5534b75619cb71fa40c84cd5ddf152a449abd6a8
    // @lfy def/cli/main.lfy:main#main:main:dc6670a1659abe98c2ae0ed1502d175a57162b9aa395c843f4fdeaa3d5ce4664
    // @lfy def/cli/main.lfy:main#main:main:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
    // @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
    #[expect(clippy::too_many_arguments, reason = "one run of an agent takes all of this")]
    pub(crate) fn run(
        &self,
        command: &str,
        where_: &Where,
        label: &str,
        units: &str,
        instructions: &str,
        binding: Binding,
        resume: Option<&str>,
        running: Option<&Running>,
    ) -> Exit {
        let directory = where_.directory();
        let role = binding.role;
        let what = word_of(role);
        // An agent session id new for the run, read from the secure random source; a source
        // that cannot be read is a run that could not be set up.
        // @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
        let session_id = match session_id() {
            Ok(id) => id,
            // @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
            Err(error) => {
                return Exit {
                    code: -1,
                    stdout: String::new(),
                    stderr: error,
                    record: SessionRecord::default(),
                    session: String::new(),
                };
            }
        };
        // The agent server is hosted the first time a run needs it; nothing runs when it
        // cannot be hosted, and its failure is what the run came to.
        // @lfy def/cli/main.lfy:main#main:main:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
        let host = match self.server.host() {
            Ok(host) => host,
            // @lfy def/cli/main.lfy:main#main:main:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
            Err(error) => {
                return Exit {
                    code: -1,
                    stdout: String::new(),
                    stderr: format!("the agent server could not be hosted: {error}"),
                    record: SessionRecord::default(),
                    session: session_id,
                };
            }
        };
        // One hosted session for this run, bound to what it is, opened just before it starts.
        // @lfy def/cli/main.lfy:main#main:main:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
        let session = (host.open)(binding);
        // @lfy def/cli/main.lfy:main#main:main:db24670f7a47cc1d0d13a51de65a69c1469315a12c8359b5ba6d6f0d620ded13
        let config = self.write_config(&host, &session, label, role);
        // Which of the command tools this session was offered, in the order the server lists
        // them, so that a run knows what to call instead of knowing the tools.
        // @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
        let tools = command_tools_of(&(host.offered)(&session));
        // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
        let (program, flag) = shell();
        let mut spawning = Process::new(program);
        spawning
            .arg(flag)
            .arg(command)
            .current_dir(directory)
            // @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
            // @lfy def/cli/main.lfy:main#main:main:12899ead30416b53072d96627b1e63022facd0087666a2a48961725a3a6d7fb1
            .env("ELFIE_ROOT", directory)
            // ELFIE_BATCH is the identifier of the batch and ELFIE_UNITS its stems, separated
            // by spaces.
            // @lfy def/cli/main.lfy:main#main:main:d998aab5e2cced5f970810a3173862fcd78e571f2000c43dd4bdddaefa571c0b
            .env("ELFIE_BATCH", label)
            .env("ELFIE_UNITS", units)
            // ELFIE_ROOT is the directory it runs in, ELFIE the path of the running
            // executable, ELFIE_MCP_CONFIG the config file of this run's hosted session, and
            // ELFIE_COMMAND_TOOLS the command tools that session was offered; no git variable
            // is added beyond these six.
            // @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
            // @lfy def/cli/main.lfy:main#main:main:e17f6b936896c49f2d5ae4a8c46ccaf3f569ef4620c0e0e927b573171cbde588
            .env("ELFIE", &self.elfie)
            // @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
            .env("ELFIE_MCP_CONFIG", &config)
            // @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
            .env("ELFIE_COMMAND_TOOLS", &tools)
            // The agent session id of this run.
            // @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
            .env(SESSION, &session_id)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // A resuming run goes on from the session of the run of the same one of them on the
        // batch before it; every other run has no ELFIE_RESUME at all, whatever the
        // environment the command was started in held.
        // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
        // @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
        match resume {
            // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
            Some(before) => spawning.env(RESUME, before),
            // @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
            None => spawning.env_remove(RESUME),
        };
        // The run leads a process group of its own, so that stopping it stops everything it
        // started.
        // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
        leads_group(&mut spawning);
        // The command could not be started: the code is -1 and the failure is the output.
        // @lfy def/cli/main.lfy:main#main:main:979d0574227f5474ed3c9c68fd3f62895696f4c2e92d830ba8016b19b0624a69
        let mut child = match spawning.spawn() {
            Ok(child) => child,
            Err(error) => {
                // @lfy def/cli/main.lfy:main#main:main:b4fcd8255dc77a6812ba729d68dac80c3c551fa5f203c2d68df2bab37b05ee57
                let record = (host.close)(&session);
                // @lfy def/cli/main.lfy:main#main:main:d5aa2bf16f2da120b4d37b7d66b8856800a80b580034d60d8ee7a8967d232f02
                remove_config(&config, &self.logs);
                return Exit {
                    code: -1,
                    stdout: String::new(),
                    stderr: error.to_string(),
                    record,
                    session: session_id,
                };
            }
        };
        // The group this run leads is remembered while it runs, so that stopping the batch
        // stops it and every process it started.
        // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
        let group = running.map(|running| running.started(child.id()));
        // Each line is shown as it arrives, prefixed by the batch; with --json it goes to
        // standard error, so standard output stays one JSON object per line.
        // @lfy def/cli/main.lfy:main#main:main:f507a20d5658ce7b158a66e802e963debf7d67da97fd22a6992b840100d837b2
        let logs = self.logs.clone();
        let out = child
            .stdout
            .take()
            .map(|pipe| watch(pipe, label, true, !self.json, self.style.out, Stream::Output, logs.clone()));
        // Standard error reaches the log as standard output does, so nothing the command
        // wrote is lost to a run read after the fact.
        // @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
        let err = child
            .stderr
            .take()
            .map(|pipe| watch(pipe, label, true, false, self.style.err, Stream::Error, logs.clone()));
        if let Some(mut stdin) = child.stdin.take()
            && let Err(error) = stdin.write_all(instructions.as_bytes())
        {
            append_to(&logs, &format!("the {what} command did not read its input: {error}"));
        }
        let status = child.wait();
        // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
        drop(group);
        let stdout = out.map(|handle| handle.join().unwrap_or_default()).unwrap_or_default();
        let stderr = err.map(|handle| handle.join().unwrap_or_default()).unwrap_or_default();
        // The run has returned: its session is closed, and what it submitted through the
        // server comes back with its output, before anything reads its outcome. Its config
        // file goes with the session, whose token no longer opens anything.
        // @lfy def/cli/main.lfy:main#main:main:b4fcd8255dc77a6812ba729d68dac80c3c551fa5f203c2d68df2bab37b05ee57
        let record = (host.close)(&session);
        // @lfy def/cli/main.lfy:main#main:main:d5aa2bf16f2da120b4d37b7d66b8856800a80b580034d60d8ee7a8967d232f02
        remove_config(&config, &logs);
        let status = match status {
            Ok(status) => status,
            Err(error) => {
                return Exit { code: -1, stdout, stderr: error.to_string(), record, session: session_id };
            }
        };
        if !status.success() {
            append_to(&logs, &format!("the {what} command exited with {status}"));
        }
        Exit { code: status.code().unwrap_or(-1), stdout, stderr, record, session: session_id }
    }
}

/// What every run of the compiler and the verifier for one batch in one directory shares:
/// the agent, the process groups the batch has running, and the sessions a resuming run goes
/// on from.
// @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
// @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
pub(crate) struct Runs {
    pub(crate) agent: Agent,
    /// The process groups the batch has running, shared with the compile so that a stopped
    /// compile stops them.
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    pub(crate) running: Arc<Running>,
    /// The sessions a resuming run of each role goes on from, in this directory.
    // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
    resuming: Mutex<Resuming>,
}

impl Runs {
    // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
    pub(crate) fn new(agent: Agent, running: Arc<Running>) -> Runs {
        Runs { agent, running, resuming: Mutex::new(Resuming::default()) }
    }

    /// One run of the compiler or the verifier for the batch: a resuming run carries the
    /// session of that role's run on the batch before it here, and every run is remembered
    /// while it runs so that stopping the batch stops it.
    // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
    // @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
    #[expect(clippy::too_many_arguments, reason = "one run of an agent takes all of this")]
    pub(crate) fn run(
        &self,
        command: &str,
        where_: &Where,
        label: &str,
        units: &str,
        instructions: &str,
        binding: Binding,
        resuming: bool,
    ) -> Exit {
        let role = binding.role;
        // A resuming run goes on from the session of that role's first run on the batch here;
        // every other run carries none.
        // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
        // @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
        let resume = if resuming {
            let held = self.resuming.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            held.of(role).map(str::to_string)
        } else {
            None
        };
        let exit = self.agent.run(
            command,
            where_,
            label,
            units,
            instructions,
            binding,
            resume.as_deref(),
            Some(&self.running),
        );
        // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
        self.resuming
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .note(role, resume.as_deref(), &exit.session);
        exit
    }
}

/// The MCP config file of a run that has returned, removed: the token it holds no longer
/// opens anything, so nothing is left behind that reads as a way in.
// @lfy def/cli/main.lfy:main#main:main:d5aa2bf16f2da120b4d37b7d66b8856800a80b580034d60d8ee7a8967d232f02
pub(crate) fn remove_config(config: &Path, logs: &[PathBuf]) {
    // @lfy def/cli/main.lfy:main#main:main:d5aa2bf16f2da120b4d37b7d66b8856800a80b580034d60d8ee7a8967d232f02
    if let Err(error) = fs::remove_file(config)
        && error.kind() != io::ErrorKind::NotFound
    {
        append_to(logs, &format!("{} could not be removed: {error}", config.display()));
    }
}

/// How long a command of the target may run before it and every process of its group are
/// stopped, as the command tools stop one.
// @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
pub(crate) const COMMAND_DEADLINE: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// How often a running command of the target is looked in on while its deadline runs.
// @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
pub(crate) const LOOKED_IN_ON: std::time::Duration = std::time::Duration::from_millis(50);

/// The line a target runs for one operation, when it has one for it.
// @lfy def/cli/main.lfy:main#main:main:75192ed1a30affcf3c4f5a267b8bf361995b47809aca6cb8e4585e78bffd8551
pub(crate) fn line_for(target: &Target, operation: Operation) -> Option<&str> {
    // @lfy def/cli/main.lfy:main#main:main:75192ed1a30affcf3c4f5a267b8bf361995b47809aca6cb8e4585e78bffd8551
    target
        .commands
        .iter()
        .find(|command| command.operation == operation)
        .and_then(|command| command.line.as_deref())
}

/// The word an operation of a target goes by in a progress line.
// @lfy def/cli/main.lfy:main#main:main:9253f376492f06a33bd0110d5f3f29c04bffd6a3b5a3a922f56cb3f2d9a668e9
pub(crate) fn word_of_operation(operation: Operation) -> &'static str {
    match operation {
        Operation::Install => "install",
        Operation::Add => "add",
        Operation::Build => "build",
        Operation::Test => "test",
        Operation::Lint => "lint",
        Operation::Format => "format",
        Operation::Run => "run",
    }
}

/// One run of a command of a target on a batch, as `elfie_build` or `elfie_test` runs it
/// given no filter and no scope: through the platform shell, with no input, in the target's
/// output directory under the batch's directory, with RUN_SCRIPT set to the target's script
/// runner or the empty string and TESTS and SCOPE set to the empty string.
///
/// It and every process of its group are stopped thirty minutes after it started if it has
/// not ended, and it succeeds when its exit code is 0. Every line it wrote reaches the log.
// @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
pub(crate) fn run_target_command(
    target: &Target,
    operation: Operation,
    directory: &Path,
    logs: &[PathBuf],
    running: Option<&Running>,
) -> Option<i32> {
    // @lfy def/cli/main.lfy:main#main:main:75192ed1a30affcf3c4f5a267b8bf361995b47809aca6cb8e4585e78bffd8551
    let line = line_for(target, operation)?;
    // The command runs in the target's output directory under the batch's directory.
    // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
    let within = directory.join(&target.output_directory);
    // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
    let (program, flag) = shell();
    let mut spawning = Process::new(program);
    spawning
        .arg(flag)
        .arg(line)
        .current_dir(&within)
        // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
        .env("RUN_SCRIPT", target.script_runner.clone().unwrap_or_default())
        // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
        .env("TESTS", "")
        .env("SCOPE", "")
        // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // The command leads a process group of its own, so everything it starts is stopped with
    // it when it runs out of time or the batch is stopped.
    // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    leads_group(&mut spawning);
    let mut child = match spawning.spawn() {
        Ok(child) => child,
        // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
        Err(error) => {
            append_to(logs, &format!("the {} command could not be run: {error}", word_of_operation(operation)));
            return Some(-1);
        }
    };
    let leader = child.id();
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    let group = running.map(|running| running.started(leader));
    let label = format!("{} {}", target.identifier, word_of_operation(operation));
    let out = child.stdout.take().map(|pipe| logged(pipe, &label, logs.to_vec()));
    let err = child.stderr.take().map(|pipe| logged(pipe, &label, logs.to_vec()));
    let started = std::time::Instant::now();
    // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
    let code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code().unwrap_or(-1),
            Ok(None) => {}
            Err(_) => break -1,
        }
        // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
        if started.elapsed() >= COMMAND_DEADLINE {
            stop_group(leader);
            let _ = child.kill();
            let _ = child.wait();
            append_to(logs, &format!("{label} was stopped after 30 minutes"));
            break -1;
        }
        std::thread::sleep(LOOKED_IN_ON);
    };
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    drop(group);
    for handle in [out, err].into_iter().flatten() {
        let _ = handle.join();
    }
    // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
    Some(code)
}

/// One stream of a command of the target, every line of it appended to the log under the
/// name of the command, so that a run read after the fact holds what the command wrote.
// @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
pub(crate) fn logged<R: io::Read + Send + 'static>(pipe: R, label: &str, logs: Vec<PathBuf>) -> JoinHandle<()> {
    let prefix = format!("  {label} \u{2502} ");
    std::thread::spawn(move || {
        for line in io::BufReader::new(pipe).lines().map_while(Result::ok) {
            append_to(&logs, &format!("{prefix}{line}"));
        }
    })
}
