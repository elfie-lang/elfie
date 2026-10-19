// @lfy def/cli/compile.lfy:compile
//! Running the compiler, the verifier, and a command of the target: the hosted session each
//! run is given, the environment it is handed, the lines it writes as they arrive, the
//! locations a command's output names, and the process group it is stopped with.

use super::*;

/// What a finished command left: its code, -1 when it ended by a signal or could not be
/// started, everything it wrote, held whole, and whatever it submitted through the agent
/// server before its session was closed.
// @lfy def/cli/compile.lfy:compile#compile:compile:aa66eb73d63beed141bc36accf7ad7dd3920fb56c5500f6e4295a50ad7ba7b37
pub(crate) struct Exit {
    pub(crate) code: i32,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    /// What the run recorded through the agent server — the reviews it submitted, the
    /// outcome it submitted, and the notes it gave the units it touched — as closing its
    /// session gave it back.
    // @lfy def/cli/compile.lfy:compile#compile:compile:b4fcd8255dc77a6812ba729d68dac80c3c551fa5f203c2d68df2bab37b05ee57
    pub(crate) record: SessionRecord,
    /// The agent session id the run was given as ELFIE_SESSION, so that a resuming run
    /// after it can go on from the same one.
    // @lfy def/cli/compile.lfy:compile#compile:compile:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
    // @lfy def/cli/compile.lfy:compile#compile:compile:7f343c1256669da811796e2ffa76c4b0def02ea5161f3b27756047490353fd43
    pub(crate) session: String,
}

/// Every process group a compile has running: the compiler, the verifier, or a command of
/// the target, so that stopping the compile stops them and everything they started.
// @lfy def/cli/compile.lfy:compile#compile:compile:da791c98fbedcf3fb13fe11ba3a9b7a6af2de1cacab56f35de534f60c8208f32
#[derive(Default)]
pub(crate) struct Running {
    /// The leaders of the groups still running, by the number each was given.
    // @lfy def/cli/compile.lfy:compile#compile:compile:da791c98fbedcf3fb13fe11ba3a9b7a6af2de1cacab56f35de534f60c8208f32
    groups: Mutex<BTreeMap<u64, u32>>,
    /// The number the next group to start takes.
    // @lfy def/cli/compile.lfy:compile#compile:compile:da791c98fbedcf3fb13fe11ba3a9b7a6af2de1cacab56f35de534f60c8208f32
    next: Mutex<u64>,
    /// Whether the compile stopped, so that nothing starts after it did.
    // @lfy def/cli/compile.lfy:compile#compile:compile:51b22c76c1410a187c0f287178a9ecb28048b54a983754ad873fa703eb7809a3
    stopped: std::sync::atomic::AtomicBool,
}

impl Running {
    /// One group remembered while it runs, forgotten when the handle is dropped; one that
    /// starts once the compile stopped is stopped at once.
    // @lfy def/cli/compile.lfy:compile#compile:compile:da791c98fbedcf3fb13fe11ba3a9b7a6af2de1cacab56f35de534f60c8208f32
    pub(crate) fn started(&self, leader: u32) -> Group<'_> {
        let at = {
            let mut next = self.next.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let at = *next;
            *next += 1;
            at
        };
        self.groups.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(at, leader);
        // @lfy def/cli/compile.lfy:compile#compile:compile:da791c98fbedcf3fb13fe11ba3a9b7a6af2de1cacab56f35de534f60c8208f32
        if self.is_stopped() {
            stop_group(leader);
        }
        Group { running: self, at }
    }

    /// Whether the compile stopped.
    // @lfy def/cli/compile.lfy:compile#compile:compile:51b22c76c1410a187c0f287178a9ecb28048b54a983754ad873fa703eb7809a3
    pub(crate) fn is_stopped(&self) -> bool {
        self.stopped.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Every group still running stopped, with every process of each, and nothing started
    /// after that.
    // @lfy def/cli/compile.lfy:compile#compile:compile:da791c98fbedcf3fb13fe11ba3a9b7a6af2de1cacab56f35de534f60c8208f32
    pub(crate) fn stop(&self) {
        self.stopped.store(true, std::sync::atomic::Ordering::SeqCst);
        let leaders: Vec<u32> = {
            let groups = self.groups.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            groups.values().copied().collect()
        };
        for leader in leaders {
            // @lfy def/cli/compile.lfy:compile#compile:compile:da791c98fbedcf3fb13fe11ba3a9b7a6af2de1cacab56f35de534f60c8208f32
            stop_group(leader);
        }
    }
}

/// One process group of a compile while it runs.
// @lfy def/cli/compile.lfy:compile#compile:compile:da791c98fbedcf3fb13fe11ba3a9b7a6af2de1cacab56f35de534f60c8208f32
pub(crate) struct Group<'r> {
    running: &'r Running,
    at: u64,
}

impl Drop for Group<'_> {
    /// A run that has ended is no longer a group to stop.
    // @lfy def/cli/compile.lfy:compile#compile:compile:da791c98fbedcf3fb13fe11ba3a9b7a6af2de1cacab56f35de534f60c8208f32
    fn drop(&mut self) {
        self.running
            .groups
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.at);
    }
}

/// The agent session a resuming run goes on from: for the compiler and for the verifier
/// apart, the session of that one's run on the unit before it, in the unit's directory.
///
/// Every resuming run of a chain carries the same id, because a resuming run passes on its
/// own ELFIE_RESUME when it had one and its ELFIE_SESSION otherwise. A unit that runs again
/// after it was accepted, touched by another unit or planned by the global review, begins a
/// new chain, since this is made afresh each time the unit starts.
// @lfy def/cli/compile.lfy:compile#compile:compile:7f343c1256669da811796e2ffa76c4b0def02ea5161f3b27756047490353fd43
// @lfy def/cli/compile.lfy:compile#compile:compile:9e092cdf07f0cd7a1468317b2e75bb10ee5513015372fa964ca1fdff2d436dba
#[derive(Default)]
pub(crate) struct Resuming {
    /// The session the compiler's runs on the unit go on from.
    // @lfy def/cli/compile.lfy:compile#compile:compile:7f343c1256669da811796e2ffa76c4b0def02ea5161f3b27756047490353fd43
    compiler: Option<String>,
    /// The session the verifier's runs on the unit go on from.
    // @lfy def/cli/compile.lfy:compile#compile:compile:7f343c1256669da811796e2ffa76c4b0def02ea5161f3b27756047490353fd43
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
    // @lfy def/cli/compile.lfy:compile#compile:compile:7f343c1256669da811796e2ffa76c4b0def02ea5161f3b27756047490353fd43
    pub(crate) fn note(&mut self, role: Role, resumed: Option<&str>, session: &str) {
        let chain = match role {
            Role::Compiler => &mut self.compiler,
            Role::Verifier => &mut self.verifier,
        };
        // @lfy def/cli/compile.lfy:compile#compile:compile:7f343c1256669da811796e2ffa76c4b0def02ea5161f3b27756047490353fd43
        let id = resumed.or(if session.is_empty() { None } else { Some(session) });
        if let Some(id) = id {
            *chain = Some(id.to_string());
        }
    }

    /// The id a resuming run of one role goes on from; nothing when that role has not run on
    /// the unit here.
    // @lfy def/cli/compile.lfy:compile#compile:compile:7f343c1256669da811796e2ffa76c4b0def02ea5161f3b27756047490353fd43
    pub(crate) fn of(&self, role: Role) -> Option<&str> {
        match role {
            Role::Compiler => self.compiler.as_deref(),
            Role::Verifier => self.verifier.as_deref(),
        }
    }
}

/// The platform shell and the flag it takes a command line after: `sh -c` on Unix and
/// `cmd /C` on Windows.
// @lfy def/cli/compile.lfy:compile#compile:compile:3a5a6187176db8b0b5e61a117d0b28b3181489ed6ba22acc61e795ac4bbf9d31
// @lfy def/cli/compile.lfy:compile#compile:compile:2122ce59b6ca33af5e18c265f0334204d7ceb9807440dcaae456e418da206c9d
pub(crate) fn shell() -> (&'static str, &'static str) {
    match cfg!(windows) {
        true => ("cmd", "/C"),
        false => ("sh", "-c"),
    }
}

/// A run made the leader of a process group of its own, so that stopping it stops everything
/// it started: a process group on Unix, and on Windows a process started with the
/// CREATE_NEW_PROCESS_GROUP creation flag, which is 0x00000200.
// @lfy def/cli/compile.lfy:compile#compile:compile:da791c98fbedcf3fb13fe11ba3a9b7a6af2de1cacab56f35de534f60c8208f32
pub(crate) fn leads_group(spawning: &mut Process) {
    // @lfy def/cli/compile.lfy:compile#compile:compile:da791c98fbedcf3fb13fe11ba3a9b7a6af2de1cacab56f35de534f60c8208f32
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(spawning, 0);
    // @lfy def/cli/compile.lfy:compile#compile:compile:da791c98fbedcf3fb13fe11ba3a9b7a6af2de1cacab56f35de534f60c8208f32
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(spawning, 0x0000_0200);
}

/// Every process of a process group stopped, through the platform's own way of stopping one:
/// `kill` of the group on Unix, and `taskkill /T /F` of the leader on Windows.
// @lfy def/cli/compile.lfy:compile#compile:compile:da791c98fbedcf3fb13fe11ba3a9b7a6af2de1cacab56f35de534f60c8208f32
pub(crate) fn stop_group(leader: u32) {
    // @lfy def/cli/compile.lfy:compile#compile:compile:da791c98fbedcf3fb13fe11ba3a9b7a6af2de1cacab56f35de534f60c8208f32
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
/// unit, or by global for the global review, appended to the log, and kept whole. With --json
/// it is shown on standard error, so that standard output stays one JSON object per line.
/// Every line of what the command wrote reaches the log, standard error among them, so a run
/// reads after the fact as it read while it ran.
// @lfy def/cli/compile.lfy:compile#compile:compile:aa66eb73d63beed141bc36accf7ad7dd3920fb56c5500f6e4295a50ad7ba7b37
// @lfy def/cli/compile.lfy:compile#compile:compile:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
pub(crate) fn watch<R: io::Read + Send + 'static>(
    pipe: R,
    label: &str,
    keep: bool,
    to_stdout: bool,
    on: bool,
    stream: Stream,
    logs: Vec<PathBuf>,
) -> JoinHandle<String> {
    // Two spaces, the unit or global, a space, a bar, and a space.
    // @lfy def/cli/compile.lfy:compile#compile:compile:2e0dbfd080f3b449aa5c3bcb1573c50971d381975307a1b40917dd84d98a4a42
    let plain = format!("  {label} \u{2502} ");
    // The prefix of a line that came from standard output is muted, and one that came from
    // standard error is warning.
    // @lfy def/cli/compile.lfy:compile#compile:compile:714e27f2331360d1b071e2a8bdf90f345382ea99e062330f8068d835170e9d81
    // @lfy def/cli/compile.lfy:compile#compile:compile:f9dd575e7bd97b0a74ebebee78b614a642f4aefa5b071efda9788975324bccbf
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
            // stream, so a line of the other stream never mixes into it.
            // @lfy def/cli/compile.lfy:compile#compile:compile:2e0dbfd080f3b449aa5c3bcb1573c50971d381975307a1b40917dd84d98a4a42
            // @lfy def/cli/compile.lfy:compile#compile:compile:aa66eb73d63beed141bc36accf7ad7dd3920fb56c5500f6e4295a50ad7ba7b37
            // A line waits for the turn to print, so it never comes between a progress line of
            // another unit and what follows it.
            // @lfy def/cli/compile.lfy:compile#compile:compile:a7d36c6314ecefbcbce7f5f0d72190c18e20d5d6358618dd7d1063060f1ae77f
            let _turn = turn();
            if to_stdout {
                println!("{prefix}{line}");
            } else {
                eprintln!("{prefix}{line}");
            }
            // The line the log holds is the line as printed with colors off.
            // @lfy def/cli/main.lfy:main#main:main:3082965661f2495d1e5e9d8a2177fc2ca759320db1b83c7e7bc0605b9be65f3f
            // @lfy def/cli/compile.lfy:compile#compile:compile:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
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
// @lfy def/cli/compile.lfy:compile#compile:compile:5724f5824450ab53811f66b437b4a0ea4d89fea52c17be8df51e351a55ae8fe9
// @lfy def/cli/compile.lfy:compile#compile:compile:528aaa4cb8488c953d03050aadaa0d8bc000afe6ba74f6af3daa9146ffb6c4c4
pub(crate) struct Server {
    /// The root, where every session's MCP config file is written.
    // @lfy def/cli/compile.lfy:compile#compile:compile:5b2c8910206c8e75afcd2cd352436a6895414fa9284039b11ef9f9b3c1fb88bb
    pub(crate) root: PathBuf,
    /// What hosting came to, once something needed it: the host, or why there is none.
    // @lfy def/cli/compile.lfy:compile#compile:compile:5724f5824450ab53811f66b437b4a0ea4d89fea52c17be8df51e351a55ae8fe9
    pub(crate) hosted: Mutex<Option<Result<Arc<Host>, String>>>,
}

impl Server {
    // @lfy def/cli/compile.lfy:compile
    pub(crate) fn new(root: PathBuf) -> Server {
        Server { root, hosted: Mutex::new(None) }
    }

    /// The server hosted for the length of this command, made the first time a run needs it
    /// and the same one after that.
    // @lfy def/cli/compile.lfy:compile#compile:compile:5724f5824450ab53811f66b437b4a0ea4d89fea52c17be8df51e351a55ae8fe9
    // @lfy def/cli/compile.lfy:compile#compile:compile:528aaa4cb8488c953d03050aadaa0d8bc000afe6ba74f6af3daa9146ffb6c4c4
    pub(crate) fn host(&self) -> Result<Arc<Host>, String> {
        let mut held = self.hosted.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // @lfy def/cli/compile.lfy:compile#compile:compile:5724f5824450ab53811f66b437b4a0ea4d89fea52c17be8df51e351a55ae8fe9
        held.get_or_insert_with(|| elfie_mcp::host().map(Arc::new)).clone()
    }

    /// Whether hosting was tried and failed, so that nothing more is run once it has.
    // @lfy def/cli/compile.lfy:compile#compile:compile:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
    pub(crate) fn failed(&self) -> bool {
        let held = self.hosted.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        matches!(*held, Some(Err(_)))
    }

    /// Every session that is still open ended and the server stopped, once the last run has
    /// returned; nothing to stop when nothing was ever hosted.
    // @lfy def/cli/compile.lfy:compile#compile:compile:77f1f3a8ea3c2d7f433ee642b0418c00d1ce2b7dd1dca0106b299cb12959c12b
    pub(crate) fn stop(&self) {
        let mut held = self.hosted.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // @lfy def/cli/compile.lfy:compile#compile:compile:77f1f3a8ea3c2d7f433ee642b0418c00d1ce2b7dd1dca0106b299cb12959c12b
        if let Some(Ok(host)) = held.take() {
            (host.stop)();
        }
    }
}

/// The tools that run a target's command, in the order the agent server lists them, so that
/// a run is told which of them its session was offered.
// @lfy def/cli/compile.lfy:compile#compile:compile:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
pub(crate) const COMMAND_TOOLS: [&str; 4] = ["elfie_build", "elfie_test", "elfie_lint", "elfie_format"];

/// The command tools among the names a session is offered, in the order the server lists
/// them, joined by spaces; empty when it was offered none.
// @lfy def/cli/compile.lfy:compile#compile:compile:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
pub(crate) fn command_tools_of(offered: &[String]) -> String {
    // @lfy def/cli/compile.lfy:compile#compile:compile:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
    offered
        .iter()
        .filter(|name| COMMAND_TOOLS.contains(&name.as_str()))
        .map(String::as_str)
        .collect::<Vec<&str>>()
        .join(" ")
}

/// The word a role goes by in a file name and in the log.
// @lfy def/cli/compile.lfy:compile#compile:compile:5b2c8910206c8e75afcd2cd352436a6895414fa9284039b11ef9f9b3c1fb88bb
pub(crate) fn word_of(role: Role) -> &'static str {
    match role {
        Role::Compiler => "compiler",
        Role::Verifier => "verifier",
    }
}

/// The name of the variable that carries the agent session id of a run.
// @lfy def/cli/compile.lfy:compile#compile:compile:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
pub(crate) const SESSION: &str = "ELFIE_SESSION";

/// The name of the variable that carries the agent session id a resuming run goes on from.
// @lfy def/cli/compile.lfy:compile#compile:compile:7f343c1256669da811796e2ffa76c4b0def02ea5161f3b27756047490353fd43
pub(crate) const RESUME: &str = "ELFIE_RESUME";

/// The name of the variable that names the unit a run is for, or global.
// @lfy def/cli/compile.lfy:compile#compile:compile:7522539013e41173fe7dcc12bb506b34925d3a1de610ef78fe9a1db5ea960f10
pub(crate) const UNIT: &str = "ELFIE_UNIT";

/// An agent session id new for one run: a version 4 UUID made from the operating system's
/// secure random source, spelled as 36 lowercase characters in the 8-4-4-4-12 form.
// @lfy def/cli/compile.lfy:compile#compile:compile:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
pub(crate) fn session_id() -> Result<String, String> {
    // @lfy def/cli/compile.lfy:compile#compile:compile:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
    let mut bytes = [0u8; 16];
    // The source is the one the operating system offers, whichever platform this is.
    getrandom::fill(&mut bytes)
        .map_err(|error| format!("the operating system's secure random source cannot be read: {error}"))?;
    Ok(uuid_of(bytes))
}

/// Sixteen random bytes spelled as a version 4 UUID: the version in the high nibble of the
/// seventh byte and the variant in the two high bits of the ninth, then 36 lowercase
/// characters in the 8-4-4-4-12 form.
// @lfy def/cli/compile.lfy:compile#compile:compile:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
pub(crate) fn uuid_of(mut bytes: [u8; 16]) -> String {
    // @lfy def/cli/compile.lfy:compile#compile:compile:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    // @lfy def/cli/compile.lfy:compile#compile:compile:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// What running one agent takes: how its lines are shown and logged, the executable an agent
/// starts its server with, and the server every run works through.
// @lfy def/cli/compile.lfy:compile#compile:compile:3a5a6187176db8b0b5e61a117d0b28b3181489ed6ba22acc61e795ac4bbf9d31
#[derive(Clone)]
pub(crate) struct Agent {
    pub(crate) json: bool,
    pub(crate) style: Paint,
    pub(crate) logs: Vec<PathBuf>,
    /// The path of the running executable, so the agent server an agent starts is this same
    /// program.
    // @lfy def/cli/compile.lfy:compile#compile:compile:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
    pub(crate) elfie: PathBuf,
    /// The agent server every run works through.
    // @lfy def/cli/compile.lfy:compile#compile:compile:5724f5824450ab53811f66b437b4a0ea4d89fea52c17be8df51e351a55ae8fe9
    pub(crate) server: Arc<Server>,
}

impl Agent {
    // @lfy def/cli/compile.lfy:compile
    pub(crate) fn new(json: bool, style: Paint, logs: Vec<PathBuf>, root: &Path) -> Agent {
        // @lfy def/cli/compile.lfy:compile#compile:compile:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
        let elfie = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("elfie"));
        // @lfy def/cli/compile.lfy:compile#compile:compile:5724f5824450ab53811f66b437b4a0ea4d89fea52c17be8df51e351a55ae8fe9
        Agent { json, style, logs, elfie, server: Arc::new(Server::new(root.to_path_buf())) }
    }

    /// The MCP config file of one session, written under the root before the run starts:
    /// `elfie-requests/<name>.<role>.mcp.json`, with `<name>` the request name of the run's
    /// unit or global, holding one object naming the session's URL and the bearer token every
    /// request to it must carry.
    // @lfy def/cli/compile.lfy:compile#compile:compile:5b2c8910206c8e75afcd2cd352436a6895414fa9284039b11ef9f9b3c1fb88bb
    // @lfy def/cli/compile.lfy:compile#compile:compile:44a95955daee5d89e4435d08b9cc7de1e517c400cca66ecc9ac0f0aecd83045b
    pub(crate) fn write_config(&self, host: &Host, url: &str, label: &str, role: Role) -> PathBuf {
        let directory = requests_directory(&self.server.root);
        // @lfy def/cli/compile.lfy:compile#compile:compile:5b2c8910206c8e75afcd2cd352436a6895414fa9284039b11ef9f9b3c1fb88bb
        let path = directory.join(format!("{}.{}.mcp.json", request_name(label), word_of(role)));
        // One object with the key mcpServers, holding one object with the key elfie, holding
        // the type http, the session's URL, and the Authorization header.
        // @lfy def/cli/compile.lfy:compile#compile:compile:44a95955daee5d89e4435d08b9cc7de1e517c400cca66ecc9ac0f0aecd83045b
        let value = serde_json::json!({
            "mcpServers": {
                "elfie": {
                    "type": "http",
                    "url": url,
                    "headers": { "Authorization": format!("Bearer {}", host.token) },
                }
            }
        });
        // Json.stringify of it, in its compact layout.
        // @lfy def/cli/compile.lfy:compile#compile:compile:44a95955daee5d89e4435d08b9cc7de1e517c400cca66ecc9ac0f0aecd83045b
        let text = value.to_string();
        // @lfy def/cli/main.lfy:main#main:main:97749da3fde520bf9fb6a1d4a7f1782131dc4eea08223f3ea0036e11724b6e42
        if let Err(error) = fs::create_dir_all(&directory).and_then(|()| fs::write(&path, &text)) {
            let at = path.display().to_string();
            complain_logged(self.style.err, &self.logs, Some(&at), &format!("could not be written: {error}"));
        }
        path
    }

    /// Streams one agent, the compiler or the verifier, once for one unit or for the global
    /// review: the instructions as its input, the directory given as its directory, and
    /// ELFIE_ROOT (that directory), ELFIE_UNIT (the unit's stem, or global), ELFIE,
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
    /// These are the whole of what it adds: a run in a change set's copy reads the root's
    /// history through the `.git` file the copy holds, so nothing of git is set here and a
    /// repository one of its own commands makes elsewhere stays its own.
    // @lfy def/cli/compile.lfy:compile#compile:compile:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
    // @lfy def/cli/compile.lfy:compile#compile:compile:1801f488181faebe1a16be771aa07937a5104a6088bcedecad5d5ad5c4c0825b
    // @lfy def/cli/compile.lfy:compile#compile:compile:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
    // @lfy def/cli/compile.lfy:compile#compile:compile:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
    #[expect(clippy::too_many_arguments, reason = "one run of an agent takes all of this")]
    pub(crate) fn run(
        &self,
        command: &str,
        directory: &Path,
        label: &str,
        instructions: &str,
        binding: Binding,
        resume: Option<&str>,
        running: Option<&Running>,
    ) -> Exit {
        let role = binding.role;
        let what = word_of(role);
        // Once the compile stopped, nothing more is started.
        // @lfy def/cli/compile.lfy:compile#compile:compile:51b22c76c1410a187c0f287178a9ecb28048b54a983754ad873fa703eb7809a3
        if running.is_some_and(Running::is_stopped) {
            return Exit {
                code: -1,
                stdout: String::new(),
                stderr: "the compile stopped".to_string(),
                record: SessionRecord::default(),
                session: String::new(),
            };
        }
        // An agent session id new for the run, read from the secure random source; a source
        // that cannot be read is a run that could not be set up.
        // @lfy def/cli/compile.lfy:compile#compile:compile:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
        let session_id = match session_id() {
            Ok(id) => id,
            // @lfy def/cli/compile.lfy:compile#compile:compile:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
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
        // @lfy def/cli/compile.lfy:compile#compile:compile:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
        let host = match self.server.host() {
            Ok(host) => host,
            // @lfy def/cli/compile.lfy:compile#compile:compile:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
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
        // @lfy def/cli/compile.lfy:compile#compile:compile:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
        let session = (host.open)(binding);
        // @lfy def/cli/compile.lfy:compile#compile:compile:5b2c8910206c8e75afcd2cd352436a6895414fa9284039b11ef9f9b3c1fb88bb
        let config = self.write_config(&host, &session, label, role);
        // Which of the command tools this session was offered, in the order the server lists
        // them, so that a run knows what to call instead of knowing the tools.
        // @lfy def/cli/compile.lfy:compile#compile:compile:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
        let tools = command_tools_of(&(host.offered)(&session));
        // @lfy def/cli/compile.lfy:compile#compile:compile:3a5a6187176db8b0b5e61a117d0b28b3181489ed6ba22acc61e795ac4bbf9d31
        let (program, flag) = shell();
        let mut spawning = Process::new(program);
        spawning
            .arg(flag)
            .arg(command)
            .current_dir(directory)
            // ELFIE_ROOT is the directory it runs in, ELFIE the path of the running
            // executable, ELFIE_MCP_CONFIG the config file of this run's hosted session, and
            // ELFIE_COMMAND_TOOLS the command tools that session was offered; no git variable
            // is added beside these.
            // @lfy def/cli/compile.lfy:compile#compile:compile:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
            // @lfy def/cli/compile.lfy:compile#compile:compile:1801f488181faebe1a16be771aa07937a5104a6088bcedecad5d5ad5c4c0825b
            .env("ELFIE_ROOT", directory)
            .env("ELFIE", &self.elfie)
            .env("ELFIE_MCP_CONFIG", &config)
            .env("ELFIE_COMMAND_TOOLS", &tools)
            // ELFIE_UNIT is the unit's stem, or global for the global review.
            // @lfy def/cli/compile.lfy:compile#compile:compile:7522539013e41173fe7dcc12bb506b34925d3a1de610ef78fe9a1db5ea960f10
            // @lfy def/cli/compile.lfy:compile#compile:compile:73c389b755f64b116aefe36d13a3dbbe224b401808c21b5c6e58de07a4b1f1d6
            .env(UNIT, label)
            // The agent session id of this run.
            // @lfy def/cli/compile.lfy:compile#compile:compile:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
            .env(SESSION, &session_id)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // A resuming run goes on from the session of the run of the same one of them on the
        // unit before it; every other run has no ELFIE_RESUME at all, whatever the
        // environment the command was started in held.
        // @lfy def/cli/compile.lfy:compile#compile:compile:7f343c1256669da811796e2ffa76c4b0def02ea5161f3b27756047490353fd43
        // @lfy def/cli/compile.lfy:compile#compile:compile:9e092cdf07f0cd7a1468317b2e75bb10ee5513015372fa964ca1fdff2d436dba
        match resume {
            // @lfy def/cli/compile.lfy:compile#compile:compile:7f343c1256669da811796e2ffa76c4b0def02ea5161f3b27756047490353fd43
            Some(before) => spawning.env(RESUME, before),
            // @lfy def/cli/compile.lfy:compile#compile:compile:9e092cdf07f0cd7a1468317b2e75bb10ee5513015372fa964ca1fdff2d436dba
            None => spawning.env_remove(RESUME),
        };
        // The run leads a process group of its own, so that stopping it stops everything it
        // started.
        // @lfy def/cli/compile.lfy:compile#compile:compile:da791c98fbedcf3fb13fe11ba3a9b7a6af2de1cacab56f35de534f60c8208f32
        leads_group(&mut spawning);
        // The command could not be started: the code is -1 and the failure is the output.
        // @lfy def/cli/compile.lfy:compile#compile:compile:979d0574227f5474ed3c9c68fd3f62895696f4c2e92d830ba8016b19b0624a69
        let mut child = match spawning.spawn() {
            Ok(child) => child,
            Err(error) => {
                // @lfy def/cli/compile.lfy:compile#compile:compile:b4fcd8255dc77a6812ba729d68dac80c3c551fa5f203c2d68df2bab37b05ee57
                let record = (host.close)(&session);
                // @lfy def/cli/compile.lfy:compile#compile:compile:d5aa2bf16f2da120b4d37b7d66b8856800a80b580034d60d8ee7a8967d232f02
                remove_config(&config, &self.logs, self.style.err);
                return Exit {
                    code: -1,
                    stdout: String::new(),
                    stderr: error.to_string(),
                    record,
                    session: session_id,
                };
            }
        };
        // The group this run leads is remembered while it runs, so that stopping the compile
        // stops it and every process it started.
        // @lfy def/cli/compile.lfy:compile#compile:compile:da791c98fbedcf3fb13fe11ba3a9b7a6af2de1cacab56f35de534f60c8208f32
        let group = running.map(|running| running.started(child.id()));
        // Each line is shown as it arrives, prefixed by the unit; with --json it goes to
        // standard error, so standard output stays one JSON object per line.
        // @lfy def/cli/compile.lfy:compile#compile:compile:aa66eb73d63beed141bc36accf7ad7dd3920fb56c5500f6e4295a50ad7ba7b37
        let logs = self.logs.clone();
        let out = child
            .stdout
            .take()
            .map(|pipe| watch(pipe, label, true, !self.json, self.style.out, Stream::Output, logs.clone()));
        // Standard error reaches the log as standard output does, so nothing the command
        // wrote is lost to a run read after the fact.
        // @lfy def/cli/compile.lfy:compile#compile:compile:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
        let err = child
            .stderr
            .take()
            .map(|pipe| watch(pipe, label, true, false, self.style.err, Stream::Error, logs.clone()));
        if let Some(mut stdin) = child.stdin.take()
            && let Err(error) = stdin.write_all(instructions.as_bytes())
        {
            complain_logged(self.style.err, &logs, None, &format!("the {what} command did not read its input: {error}"));
        }
        let status = child.wait();
        // @lfy def/cli/compile.lfy:compile#compile:compile:da791c98fbedcf3fb13fe11ba3a9b7a6af2de1cacab56f35de534f60c8208f32
        drop(group);
        let stdout = out.map(|handle| handle.join().unwrap_or_default()).unwrap_or_default();
        let stderr = err.map(|handle| handle.join().unwrap_or_default()).unwrap_or_default();
        // The run has returned: its session is closed, and what it submitted through the
        // server comes back with its output, before anything reads its outcome. Its config
        // file goes with the session, whose token no longer opens anything.
        // @lfy def/cli/compile.lfy:compile#compile:compile:b4fcd8255dc77a6812ba729d68dac80c3c551fa5f203c2d68df2bab37b05ee57
        let record = (host.close)(&session);
        // @lfy def/cli/compile.lfy:compile#compile:compile:d5aa2bf16f2da120b4d37b7d66b8856800a80b580034d60d8ee7a8967d232f02
        remove_config(&config, &logs, self.style.err);
        let status = match status {
            Ok(status) => status,
            Err(error) => {
                return Exit { code: -1, stdout, stderr: error.to_string(), record, session: session_id };
            }
        };
        Exit { code: status.code().unwrap_or(-1), stdout, stderr, record, session: session_id }
    }
}

/// What every run of the compiler and the verifier for one unit shares while the unit is
/// worked: the agent, the process groups the compile has running, and the sessions a
/// resuming run goes on from.
// @lfy def/cli/compile.lfy:compile#compile:compile:7f343c1256669da811796e2ffa76c4b0def02ea5161f3b27756047490353fd43
// @lfy def/cli/compile.lfy:compile#compile:compile:da791c98fbedcf3fb13fe11ba3a9b7a6af2de1cacab56f35de534f60c8208f32
pub(crate) struct Runs {
    pub(crate) agent: Agent,
    /// The process groups the compile has running, so that a stopped compile stops them.
    // @lfy def/cli/compile.lfy:compile#compile:compile:da791c98fbedcf3fb13fe11ba3a9b7a6af2de1cacab56f35de534f60c8208f32
    pub(crate) running: Arc<Running>,
    /// The sessions a resuming run of each role goes on from.
    // @lfy def/cli/compile.lfy:compile#compile:compile:7f343c1256669da811796e2ffa76c4b0def02ea5161f3b27756047490353fd43
    resuming: Mutex<Resuming>,
}

impl Runs {
    // @lfy def/cli/compile.lfy:compile#compile:compile:7f343c1256669da811796e2ffa76c4b0def02ea5161f3b27756047490353fd43
    pub(crate) fn new(agent: Agent, running: Arc<Running>) -> Runs {
        Runs { agent, running, resuming: Mutex::new(Resuming::default()) }
    }

    /// One run of the compiler or the verifier for the unit: a resuming run carries the
    /// session of that role's run on the unit before it, and every run is remembered while it
    /// runs so that stopping the compile stops it.
    // @lfy def/cli/compile.lfy:compile#compile:compile:7f343c1256669da811796e2ffa76c4b0def02ea5161f3b27756047490353fd43
    // @lfy def/cli/compile.lfy:compile#compile:compile:9e092cdf07f0cd7a1468317b2e75bb10ee5513015372fa964ca1fdff2d436dba
    pub(crate) fn run(
        &self,
        command: &str,
        directory: &Path,
        label: &str,
        instructions: &str,
        binding: Binding,
        resuming: bool,
    ) -> Exit {
        let role = binding.role;
        // A resuming run goes on from the session of that role's run on the unit before it;
        // every other run carries none.
        // @lfy def/cli/compile.lfy:compile#compile:compile:7f343c1256669da811796e2ffa76c4b0def02ea5161f3b27756047490353fd43
        // @lfy def/cli/compile.lfy:compile#compile:compile:9e092cdf07f0cd7a1468317b2e75bb10ee5513015372fa964ca1fdff2d436dba
        let resume = if resuming {
            let held = self.resuming.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            held.of(role).map(str::to_string)
        } else {
            None
        };
        let exit = self.agent.run(
            command,
            directory,
            label,
            instructions,
            binding,
            resume.as_deref(),
            Some(&self.running),
        );
        // @lfy def/cli/compile.lfy:compile#compile:compile:7f343c1256669da811796e2ffa76c4b0def02ea5161f3b27756047490353fd43
        self.resuming
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .note(role, resume.as_deref(), &exit.session);
        exit
    }
}

/// The MCP config file of a run that has returned, removed: the token it holds no longer
/// opens anything, so nothing is left behind that reads as a way in.
// @lfy def/cli/compile.lfy:compile#compile:compile:d5aa2bf16f2da120b4d37b7d66b8856800a80b580034d60d8ee7a8967d232f02
pub(crate) fn remove_config(config: &Path, logs: &[PathBuf], on: bool) {
    // @lfy def/cli/compile.lfy:compile#compile:compile:d5aa2bf16f2da120b4d37b7d66b8856800a80b580034d60d8ee7a8967d232f02
    if let Err(error) = fs::remove_file(config)
        && error.kind() != io::ErrorKind::NotFound
    {
        let at = config.display().to_string();
        complain_logged(on, logs, Some(&at), &format!("could not be removed: {error}"));
    }
}

/// A message the CLI prints to standard error, appended to the log as it was printed with
/// colors off, so the log holds no line that was never printed.
// @lfy def/cli/main.lfy:main#main:main:3082965661f2495d1e5e9d8a2177fc2ca759320db1b83c7e7bc0605b9be65f3f
pub(crate) fn complain_logged(on: bool, logs: &[PathBuf], path: Option<&str>, message: &str) {
    complain(on, path, message);
    // @lfy def/cli/main.lfy:main#main:main:3082965661f2495d1e5e9d8a2177fc2ca759320db1b83c7e7bc0605b9be65f3f
    append_to(logs, &complaint(false, path, message));
}

/// How long a command of the target may run before it and every process of its group are
/// stopped, as the command tools stop one.
// @lfy def/cli/compile.lfy:compile#compile:compile:2122ce59b6ca33af5e18c265f0334204d7ceb9807440dcaae456e418da206c9d
pub(crate) const COMMAND_DEADLINE: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// How often a running command of the target is looked in on while its deadline runs.
// @lfy def/cli/compile.lfy:compile#compile:compile:2122ce59b6ca33af5e18c265f0334204d7ceb9807440dcaae456e418da206c9d
pub(crate) const LOOKED_IN_ON: std::time::Duration = std::time::Duration::from_millis(50);

/// The line a target runs for one operation, when it has one for it.
// @lfy def/cli/compile.lfy:compile#compile:compile:2693766a523314183e6282316ef0592ed35595e8a47c47ed91606d938be00563
pub(crate) fn line_for(target: &Target, operation: Operation) -> Option<&str> {
    // @lfy def/cli/compile.lfy:compile#compile:compile:2693766a523314183e6282316ef0592ed35595e8a47c47ed91606d938be00563
    target
        .commands
        .iter()
        .find(|command| command.operation == operation)
        .and_then(|command| command.line.as_deref())
}

/// The word an operation of a target goes by in a progress line and a problem.
// @lfy def/cli/compile.lfy:compile#compile:compile:20d39c165928d43ffa93da6970f91339f7c50e254aa218ea87fb80072d87f066
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

/// What one run of a command of the target came to: its exit code, -1 when it could not be
/// started or ran out of time, and every line it wrote, standard output then standard error.
// @lfy def/cli/compile.lfy:compile#compile:compile:2122ce59b6ca33af5e18c265f0334204d7ceb9807440dcaae456e418da206c9d
pub(crate) struct Ran {
    pub(crate) code: i32,
    pub(crate) lines: Vec<String>,
    /// Whether the command was started at all; one that could not be is a command that could
    /// not run, and its one line says why.
    // @lfy def/cli/compile.lfy:compile#compile:compile:979d0574227f5474ed3c9c68fd3f62895696f4c2e92d830ba8016b19b0624a69
    pub(crate) started: bool,
}

impl Ran {
    /// A command succeeds when its exit code is 0.
    // @lfy def/cli/compile.lfy:compile#compile:compile:2122ce59b6ca33af5e18c265f0334204d7ceb9807440dcaae456e418da206c9d
    pub(crate) fn succeeded(&self) -> bool {
        self.code == 0
    }
}

/// One run of a command of a target, as `elfie_build`, `elfie_test`, `elfie_lint`, or
/// `elfie_format` runs it given no filter and no scope: through the platform shell, with no
/// input, in the target's output directory under the directory it runs for, with RUN_SCRIPT
/// set to the target's script runner or the empty string and TESTS and SCOPE set to the empty
/// string. ALLOW_STUBS is 1 for a test command that allows stubs and the empty string
/// otherwise.
///
/// It and every process of its group are stopped thirty minutes after it started if it has
/// not ended. Every line it wrote is kept, and none reaches the log, which holds only what
/// was printed. Nothing when the target has no line for the operation.
// @lfy def/cli/compile.lfy:compile#compile:compile:2122ce59b6ca33af5e18c265f0334204d7ceb9807440dcaae456e418da206c9d
// @lfy def/cli/compile.lfy:compile#compile:compile:ba814337ca5699de23cfbe6d5488f298a354e81440cab6936bf45c6ccc9f2411
// @lfy def/cli/compile.lfy:compile#compile:compile:625e4b5fb16259dc0adc0244165b4419cca124f091680c494fefd6ce6246e67b
pub(crate) fn run_target_command(
    target: &Target,
    operation: Operation,
    directory: &Path,
    running: Option<&Running>,
    stubs: bool,
) -> Option<Ran> {
    // @lfy def/cli/compile.lfy:compile#compile:compile:2693766a523314183e6282316ef0592ed35595e8a47c47ed91606d938be00563
    let line = line_for(target, operation)?;
    // Once the compile stopped, nothing more is started, and what this would give is ignored.
    // @lfy def/cli/compile.lfy:compile#compile:compile:51b22c76c1410a187c0f287178a9ecb28048b54a983754ad873fa703eb7809a3
    if running.is_some_and(Running::is_stopped) {
        return Some(Ran { code: -1, lines: vec!["the compile stopped".to_string()], started: true });
    }
    // The command runs in the target's output directory under the directory it runs for.
    // @lfy def/cli/compile.lfy:compile#compile:compile:2122ce59b6ca33af5e18c265f0334204d7ceb9807440dcaae456e418da206c9d
    let within = directory.join(&target.output_directory);
    let (program, flag) = shell();
    let mut spawning = Process::new(program);
    // The test command with stubs allowed has ALLOW_STUBS set to 1, and every other run has
    // it set to the empty string.
    // @lfy def/cli/compile.lfy:compile#compile:compile:ba814337ca5699de23cfbe6d5488f298a354e81440cab6936bf45c6ccc9f2411
    // @lfy def/cli/compile.lfy:compile#compile:compile:625e4b5fb16259dc0adc0244165b4419cca124f091680c494fefd6ce6246e67b
    let allow = if stubs && operation == Operation::Test { "1" } else { "" };
    spawning
        .arg(flag)
        .arg(line)
        .current_dir(&within)
        // @lfy def/cli/compile.lfy:compile#compile:compile:2122ce59b6ca33af5e18c265f0334204d7ceb9807440dcaae456e418da206c9d
        .env("RUN_SCRIPT", target.script_runner.clone().unwrap_or_default())
        .env("TESTS", "")
        .env("SCOPE", "")
        // @lfy def/cli/compile.lfy:compile#compile:compile:ba814337ca5699de23cfbe6d5488f298a354e81440cab6936bf45c6ccc9f2411
        .env("ALLOW_STUBS", allow)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // The command leads a process group of its own, so everything it starts is stopped with
    // it when it runs out of time or the compile is stopped.
    // @lfy def/cli/compile.lfy:compile#compile:compile:2122ce59b6ca33af5e18c265f0334204d7ceb9807440dcaae456e418da206c9d
    leads_group(&mut spawning);
    let mut child = match spawning.spawn() {
        Ok(child) => child,
        // @lfy def/cli/compile.lfy:compile#compile:compile:2122ce59b6ca33af5e18c265f0334204d7ceb9807440dcaae456e418da206c9d
        Err(error) => {
            let line = format!("the {} command could not be run: {error}", word_of_operation(operation));
            return Some(Ran { code: -1, lines: vec![line], started: false });
        }
    };
    let leader = child.id();
    // @lfy def/cli/compile.lfy:compile#compile:compile:da791c98fbedcf3fb13fe11ba3a9b7a6af2de1cacab56f35de534f60c8208f32
    let group = running.map(|running| running.started(leader));
    let out = child.stdout.take().map(kept_lines);
    let err = child.stderr.take().map(kept_lines);
    let started = std::time::Instant::now();
    // @lfy def/cli/compile.lfy:compile#compile:compile:2122ce59b6ca33af5e18c265f0334204d7ceb9807440dcaae456e418da206c9d
    let code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code().unwrap_or(-1),
            Ok(None) => {}
            Err(_) => break -1,
        }
        // @lfy def/cli/compile.lfy:compile#compile:compile:2122ce59b6ca33af5e18c265f0334204d7ceb9807440dcaae456e418da206c9d
        if started.elapsed() >= COMMAND_DEADLINE {
            stop_group(leader);
            let _ = child.kill();
            let _ = child.wait();
            break -1;
        }
        std::thread::sleep(LOOKED_IN_ON);
    };
    // @lfy def/cli/compile.lfy:compile#compile:compile:da791c98fbedcf3fb13fe11ba3a9b7a6af2de1cacab56f35de534f60c8208f32
    drop(group);
    let mut lines = Vec::new();
    for handle in [out, err].into_iter().flatten() {
        lines.extend(handle.join().unwrap_or_default());
    }
    // @lfy def/cli/compile.lfy:compile#compile:compile:2122ce59b6ca33af5e18c265f0334204d7ceb9807440dcaae456e418da206c9d
    Some(Ran { code, lines, started: true })
}

/// One stream of a command of the target, every line of it kept, so that the locations it
/// names can be read.
// @lfy def/cli/compile.lfy:compile#compile:compile:2122ce59b6ca33af5e18c265f0334204d7ceb9807440dcaae456e418da206c9d
pub(crate) fn kept_lines<R: io::Read + Send + 'static>(pipe: R) -> JoinHandle<Vec<String>> {
    std::thread::spawn(move || io::BufReader::new(pipe).lines().map_while(Result::ok).collect())
}

/// The first text of a line that reads a path, a colon, and a line number, as the path and
/// the number; nothing when no text of it does.
///
/// A path here is a run of characters none of which is white space, a quote, a bracket, a
/// parenthesis, or a comma, holding a slash, a backslash, or a dot, so that a time of day or
/// a count is not read as one.
// @lfy def/cli/compile.lfy:compile#compile:compile:8ff734425428618669a4f137ee0c85c9936237068dab48c69ebdcab1c20cfc15
pub(crate) fn location_in(line: &str) -> Option<(String, usize)> {
    let stops = |c: char| c.is_whitespace() || "\"'`()[]{}<>,;".contains(c);
    let characters: Vec<(usize, char)> = line.char_indices().collect();
    for (at, &(offset, c)) in characters.iter().enumerate() {
        if c != ':' {
            continue;
        }
        // The digits after the colon.
        // @lfy def/cli/compile.lfy:compile#compile:compile:8ff734425428618669a4f137ee0c85c9936237068dab48c69ebdcab1c20cfc15
        let digits: String =
            characters[at + 1..].iter().map(|&(_, c)| c).take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            continue;
        }
        // The path before it, back to the first character that cannot be in one.
        // @lfy def/cli/compile.lfy:compile#compile:compile:8ff734425428618669a4f137ee0c85c9936237068dab48c69ebdcab1c20cfc15
        let start = line[..offset]
            .char_indices()
            .rev()
            .find(|&(_, c)| stops(c))
            .map_or(0, |(found, c)| found + c.len_utf8());
        let path = &line[start..offset];
        if path.is_empty() || !path.contains(['/', '\\', '.']) || path.chars().all(|c| c.is_ascii_digit() || c == '.') {
            continue;
        }
        let Ok(number) = digits.parse::<usize>() else { continue };
        return Some((path.to_string(), number));
    }
    None
}

/// Where a location a command's output names is, relative to the unit's directory with
/// forward slashes: the path read relative to the directory the command ran in when a file
/// is at that path there, and relative to the unit's directory otherwise.
// @lfy def/cli/compile.lfy:compile#compile:compile:8ff734425428618669a4f137ee0c85c9936237068dab48c69ebdcab1c20cfc15
pub(crate) fn located(path: &str, ran_in: &Path, directory: &Path) -> String {
    // @lfy def/cli/compile.lfy:compile#compile:compile:8ff734425428618669a4f137ee0c85c9936237068dab48c69ebdcab1c20cfc15
    let there = ran_in.join(path);
    let at = if there.is_file() { there } else { directory.join(path) };
    relative(directory, &at).trim_start_matches("./").to_string()
}

/// Every line of a command's output that names a location, with the location it names,
/// relative to the unit's directory.
// @lfy def/cli/compile.lfy:compile#compile:compile:8ff734425428618669a4f137ee0c85c9936237068dab48c69ebdcab1c20cfc15
pub(crate) fn locations_of<'l>(lines: &'l [String], ran_in: &Path, directory: &Path) -> Vec<(&'l str, String, usize)> {
    lines
        .iter()
        .filter_map(|line| {
            let (path, number) = location_in(line)?;
            Some((line.as_str(), located(&path, ran_in, directory), number))
        })
        .collect()
}

/// Whether a path relative to the root is under a directory relative to the root, the root
/// itself included.
// @lfy def/cli/compile.lfy:compile#compile:compile:20d39c165928d43ffa93da6970f91339f7c50e254aa218ea87fb80072d87f066
pub(crate) fn within(path: &str, directory: &str) -> bool {
    let directory = directory.trim_end_matches('/');
    directory.is_empty() || directory == "." || path == directory || path.starts_with(&format!("{directory}/"))
}
