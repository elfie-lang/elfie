// @lfy def/mcp/main.lfy:build
//! The tools that run the target's own commands in this run's directory: its build, its
//! tests, its linters, and its format check.

use super::*;

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
pub(crate) const STOPPED: &str = "[elfie: stopped after 30 minutes]";

/// How many of the lines a command wrote are kept from the end.
// @lfy def/mcp/main.lfy:build#build:commandTool:b3cf05e26d7d3b687e7b0b4028358dba9a481d54a4bddde0f8e7ab00f9743fb7
// @lfy def/mcp/main.lfy:test#test:commandTool:b3cf05e26d7d3b687e7b0b4028358dba9a481d54a4bddde0f8e7ab00f9743fb7
// @lfy def/mcp/main.lfy:lint#lint:commandTool:b3cf05e26d7d3b687e7b0b4028358dba9a481d54a4bddde0f8e7ab00f9743fb7
// @lfy def/mcp/main.lfy:format#format:commandTool:b3cf05e26d7d3b687e7b0b4028358dba9a481d54a4bddde0f8e7ab00f9743fb7
pub(crate) const KEPT_LINES: usize = 200;

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
pub(crate) struct Turn {
    queue: Waiting,
    ticket: u64,
}

impl Turn {
    /// A place at the back of the queue of a directory, taken at once so that the calls for
    /// one directory run their commands in the order they arrived.
    // @lfy def/mcp/main.lfy:build#build:commandTool:55ee18e11aaf994de7de40b0727c7ff09d9b5230a39f4ff4070a8e0b0108478a
    pub(crate) fn take(directory: &Path) -> Turn {
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
    pub(crate) fn wait(&self) {
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

/// Every process of a process group stopped, through the platform's own way of stopping one:
/// `taskkill /T /F` on Windows, and a `kill` of the whole group elsewhere.
// @lfy def/mcp/main.lfy:build#build:commandTool:1ec8e462d2638913d1d6b8cd5b590f00751d033f0891e5ca220c79cd71a87001
fn stop_group(leader: u32) {
    let mut stopping = match cfg!(windows) {
        true => {
            let mut stopping = Command::new("taskkill");
            stopping.arg("/T").arg("/F").arg("/PID").arg(leader.to_string());
            stopping
        }
        false => {
            let mut stopping = Command::new("sh");
            stopping.arg("-c").arg(format!("kill -KILL -- -{leader}"));
            stopping
        }
    };
    let _ = stopping.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
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
    let (program, flag) = match cfg!(windows) {
        true => ("cmd", "/C"),
        false => ("sh", "-c"),
    };
    let mut spawning = Command::new(program);
    spawning
        .arg(flag)
        .arg(line)
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in environment {
        spawning.env(name, value);
    }
    // The command leads a process group of its own, so everything it starts is stopped with
    // it when it runs out of time: a process group on Unix, and on Windows a process started
    // with the CREATE_NEW_PROCESS_GROUP creation flag, which is 0x00000200.
    // @lfy def/mcp/main.lfy:build#build:commandTool:f83af6a046e8e4f79a777c2d3c9b677a67d05faab6429155fbda5b26e27e4159
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut spawning, 0);
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut spawning, 0x0000_0200);
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
pub(crate) fn kept_output(lines: &[String], stopped: bool) -> (String, usize) {
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

