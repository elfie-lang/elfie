// @lfy def/interpret/main.lfy:expand
//! The builtins the interpreter performs itself, as the target's own standard library
//! performs them.

use super::*;

impl Interpreter<'_> {
    /// Performs a builtin fn itself, as the target's own standard library performs it:
    /// compile-time code may call one, and nothing of its body is ever run. Runtime code
    /// keeps the call, so folding a runtime expression never reads a file, runs a command,
    /// or ends the process.
    // @lfy def/interpret/main.lfy:expand#expand:expand:d8317dcbe9cf6885feca31d997bb0425e00cd18a2059d87291a8d6105dc664ef
    pub(super) fn perform_builtin(
        &mut self,
        call: NodeRef,
        entity: EntityId,
        values: &[Interim],
        env: &mut Env,
    ) -> Interim {
        if !self.runs_functions && phase_of(self.model, call) != Phase::Compile {
            return self.only_at_runtime();
        }
        let Some((module, name)) = self.native_name(entity) else {
            return self.only_at_runtime();
        };
        let performed = match module.as_str() {
            "file" => perform_file(&name, values),
            "json" => perform_json(&name, values),
            "time" => perform_time(&name, values),
            "process" => self.perform_process(call, &name, values, env),
            "trait" if name == "apply" => self.perform_apply(call, values, env),
            "trait" if name == "layer" => self.perform_layer(call, values),
            _ => None,
        };
        // A builtin the interpreter has no native for reads as what only runtime answers.
        // @lfy def/interpret/main.lfy:expand
        performed.unwrap_or_else(|| self.only_at_runtime())
    }

    /// The library module and name of a builtin fn: `("file", "read")` for the `read` of
    /// `elfie/system/file`. A fn of the program is none of them, however it is named.
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn native_name(&self, entity: EntityId) -> Option<(String, String)> {
        let node = self.model.entities[entity].node?;
        let source = &self.model.sources[node.file];
        if source.origin == Origin::Program {
            return None;
        }
        let module = source
            .path
            .rsplit(['/', '\\'])
            .next()?
            .strip_suffix(".lfy")?
            .to_string();
        Some((module, self.model.entities[entity].identifier.clone()?))
    }

    /// The builtins of `elfie/system/process`: a command runs through the platform shell.
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn perform_process(
        &mut self,
        call: NodeRef,
        name: &str,
        values: &[Interim],
        env: &mut Env,
    ) -> Option<Interim> {
        match name {
            "run" | "stream" => {
                let exited = run_command(
                    &text_argument(values, 0)?,
                    text_argument(values, 1),
                    text_argument(values, 2),
                    values.get(3),
                );
                // Each line of the output reaches `onLine` before the call returns.
                // @lfy def/interpret/main.lfy:expand
                if name == "stream"
                    && let Some(on_line) = values.get(4)
                    && !matches!(on_line, Interim::Undefined | Interim::Null)
                {
                    let on_line = on_line.clone();
                    for (text, is_error) in
                        [(exited.stdout.clone(), false), (exited.stderr.clone(), true)]
                    {
                        for line in text.lines() {
                            let arguments =
                                vec![Interim::String(line.to_string()), Interim::Bool(is_error)];
                            self.call_value(call, &on_line, arguments, env);
                        }
                    }
                }
                Some(Interim::Object(vec![
                    ("code".to_string(), Interim::Number(f64::from(exited.code))),
                    ("stdout".to_string(), Interim::String(exited.stdout)),
                    ("stderr".to_string(), Interim::String(exited.stderr)),
                ]))
            }
            "environment" => Some(
                std::env::var(text_argument(values, 0)?).map_or(Interim::Undefined, Interim::String),
            ),
            "currentDirectory" => Some(
                std::env::current_dir()
                    .map(|directory| directory.to_string_lossy().replace('\\', "/"))
                    .map_or(Interim::Undefined, Interim::String),
            ),
            // The process ends at once, its output flushed, and nothing after the call runs.
            // @lfy def/interpret/main.lfy:expand
            "exit" => {
                let code = match values.first() {
                    Some(Interim::Number(code)) => *code as i32,
                    _ => 0,
                };
                let _ = std::io::Write::flush(&mut std::io::stdout());
                let _ = std::io::Write::flush(&mut std::io::stderr());
                std::process::exit(code)
            }
            "isTerminal" => {
                use std::io::IsTerminal;
                Some(Interim::Bool(match values.first() {
                    Some(Interim::Bool(true)) => std::io::stderr().is_terminal(),
                    _ => std::io::stdout().is_terminal(),
                }))
            }
            _ => None,
        }
    }

    /// `apply` called as a fn rather than as a member of a trait: the trait is applied to
    /// what the second argument names, with the rest as [`Applied::arguments`].
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn perform_apply(
        &mut self,
        call: NodeRef,
        values: &[Interim],
        env: &mut Env,
    ) -> Option<Interim> {
        let Some(&Interim::Entity(trait_id)) = values.first() else {
            return None;
        };
        if !self.model.entities[trait_id].is_trait() {
            return None;
        }
        if env.dry {
            return Some(Interim::Entity(trait_id));
        }
        let nodes = self.model.arguments(call);
        let target = *nodes.get(1)?;
        let rest = nodes.get(2..).unwrap_or_default().to_vec();
        let receivers = self.apply_receivers(target, env);
        let arguments = self.eval_arguments(&rest, env);
        for receiver in receivers {
            self.apply_trait(
                receiver,
                trait_id,
                rest.clone(),
                arguments.clone(),
                AppliedSource::Apply(call),
                false,
            );
        }
        Some(Interim::Entity(trait_id))
    }

    /// `layer` called as a fn: it yields the object holding the trait as its subject and
    /// the other arguments, evaluated in order, as its arguments. Nothing is applied by the
    /// call itself.
    // @lfy def/interpret/main.lfy:expand#expand:expand:199274135094063b0af3eb69ed4b59c3d739b76aca14b459ae4a3f182852b47d
    pub(super) fn perform_layer(&mut self, call: NodeRef, values: &[Interim]) -> Option<Interim> {
        let Some(&Interim::Entity(subject)) = values.first() else {
            return None;
        };
        if !self.model.entities[subject].is_trait() {
            return None;
        }
        Some(self.layer_value(call, subject, values[1..].to_vec()))
    }
}

/// The text an argument holds, when it holds text.
// @lfy def/interpret/main.lfy:expand
fn text_argument(values: &[Interim], at: usize) -> Option<String> {
    match values.get(at) {
        Some(Interim::String(text)) => Some(text.clone()),
        _ => None,
    }
}

/// The builtins of `elfie/system/file`, performed as `std::fs` performs them.
// @lfy def/interpret/main.lfy:expand
fn perform_file(name: &str, values: &[Interim]) -> Option<Interim> {
    let path = text_argument(values, 0)?;
    let at = std::path::Path::new(&path);
    Some(match name {
        "read" => match std::fs::read_to_string(at) {
            Ok(text) => Interim::String(text),
            Err(_) => Interim::Undefined,
        },
        "write" | "append" => {
            let text = text_argument(values, 1).unwrap_or_default();
            Interim::Bool(write_file(at, &text, name == "append"))
        }
        "exists" => Interim::Bool(at.exists()),
        "list" => {
            let recursive = matches!(values.get(1), Some(Interim::Bool(true)));
            Interim::List(
                list_files(at, recursive)
                    .into_iter()
                    .map(Interim::String)
                    .collect(),
            )
        }
        "createDirectory" => {
            let _ = std::fs::create_dir_all(at);
            Interim::Bool(at.is_dir())
        }
        "stat" => match std::fs::metadata(at) {
            Ok(about) => {
                let size = if about.is_dir() { 0 } else { about.len() };
                Interim::Object(vec![
                    ("size".to_string(), Interim::Number(size as f64)),
                    (
                        "modified".to_string(),
                        Interim::Number(seconds_of(about.modified().ok())),
                    ),
                    ("isDirectory".to_string(), Interim::Bool(about.is_dir())),
                ])
            }
            Err(_) => Interim::Undefined,
        },
        // Only a file is ever removed, so a directory is left alone.
        // @lfy def/interpret/main.lfy:expand
        "remove" => Interim::Bool(at.is_file() && std::fs::remove_file(at).is_ok()),
        _ => return None,
    })
}

/// Writes text as the whole content of a file, or after what it holds, creating every
/// missing directory above it; whether it was written.
// @lfy def/interpret/main.lfy:expand
fn write_file(path: &std::path::Path, text: &str, append: bool) -> bool {
    if let Some(directory) = path.parent()
        && !directory.as_os_str().is_empty()
        && std::fs::create_dir_all(directory).is_err()
    {
        return false;
    }
    if !append {
        return std::fs::write(path, text).is_ok();
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| std::io::Write::write_all(&mut file, text.as_bytes()))
        .is_ok()
}

/// The files in a directory, as paths relative to it with `/` between the segments, in
/// ascending text order; directories are not listed.
// @lfy def/interpret/main.lfy:expand
fn list_files(directory: &std::path::Path, recursive: bool) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let Ok(entries) = std::fs::read_dir(directory) else {
        return found;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else { continue };
        if !kind.is_dir() {
            found.push(name);
        } else if recursive {
            for under in list_files(&entry.path(), true) {
                found.push(format!("{name}/{under}"));
            }
        }
    }
    found.sort();
    found
}

/// A moment as seconds since the Unix epoch, to the millisecond.
// @lfy def/interpret/main.lfy:expand
fn seconds_of(time: Option<std::time::SystemTime>) -> f64 {
    time.and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0.0, |since| since.as_millis() as f64 / 1000.0)
}

/// The builtins of `elfie/system/time`: an instant is seconds since the Unix epoch, to the
/// millisecond, which is what `std::time::SystemTime` holds.
// @lfy def/interpret/main.lfy:expand
fn perform_time(name: &str, values: &[Interim]) -> Option<Interim> {
    match name {
        "now" => Some(Interim::Number(seconds_of(Some(
            std::time::SystemTime::now(),
        )))),
        "parse" => Some(
            text_argument(values, 0)
                .and_then(|text| instant_of(&text))
                .map_or(Interim::Undefined, Interim::Number),
        ),
        _ => None,
    }
}

/// The instant an RFC 3339 timestamp names, as seconds since the Unix epoch with its
/// fraction kept to the millisecond; nothing when the text is not one.
// @lfy def/interpret/main.lfy:expand
fn instant_of(text: &str) -> Option<f64> {
    let text = text.trim();
    let seconds = crate::generation::parse_rfc3339(text)? as f64;
    let fraction = text
        .get(19..)
        .and_then(|rest| rest.strip_prefix('.'))
        .map_or(0.0, |rest| {
            let mut digits: String = rest
                .chars()
                .take_while(char::is_ascii_digit)
                .take(3)
                .collect();
            while digits.len() < 3 {
                digits.push('0');
            }
            digits.parse::<f64>().unwrap_or(0.0) / 1000.0
        });
    Some(seconds + fraction)
}

/// The builtins of `elfie/system/json`: what JSON text carries, and the text carrying a
/// value, its keys in the order they were written.
// @lfy def/interpret/main.lfy:expand
fn perform_json(name: &str, values: &[Interim]) -> Option<Interim> {
    match name {
        "parse" => Some(
            text_argument(values, 0)
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .map_or(Interim::Undefined, |held| json_value(&held)),
        ),
        "stringify" => {
            let pretty = matches!(values.get(1), Some(Interim::Bool(true)));
            let text = json_text(values.first().unwrap_or(&Interim::Null), pretty, 0);
            Some(Interim::String(match pretty {
                true => format!("{text}\n"),
                false => text,
            }))
        }
        _ => None,
    }
}

/// What a JSON value holds.
// @lfy def/interpret/main.lfy:expand
fn json_value(value: &serde_json::Value) -> Interim {
    match value {
        serde_json::Value::Null => Interim::Null,
        serde_json::Value::Bool(held) => Interim::Bool(*held),
        serde_json::Value::Number(held) => Interim::Number(held.as_f64().unwrap_or_default()),
        serde_json::Value::String(held) => Interim::String(held.clone()),
        serde_json::Value::Array(items) => Interim::List(items.iter().map(json_value).collect()),
        serde_json::Value::Object(entries) => Interim::Object(
            entries
                .iter()
                .map(|(key, held)| (key.clone(), json_value(held)))
                .collect(),
        ),
    }
}

/// A value as JSON text: on one line with no spaces outside strings, or one entry or item
/// per line indented by two spaces per level, with an empty object or list on one line.
// @lfy def/interpret/main.lfy:expand
fn json_text(value: &Interim, pretty: bool, depth: usize) -> String {
    let (open, close, after) = match pretty {
        true => (
            format!("\n{}", "  ".repeat(depth + 1)),
            format!("\n{}", "  ".repeat(depth)),
            " ",
        ),
        false => (String::new(), String::new(), ""),
    };
    let between = format!(",{open}");
    match value {
        Interim::Bool(held) => held.to_string(),
        Interim::Number(held) => json_number(*held),
        Interim::String(text) => json_string(text),
        Interim::List(items) if !items.is_empty() => {
            let written: Vec<String> = items
                .iter()
                .map(|item| json_text(item, pretty, depth + 1))
                .collect();
            format!("[{open}{}{close}]", written.join(&between))
        }
        Interim::List(_) => "[]".to_string(),
        Interim::Object(pairs) if !pairs.is_empty() => {
            let written: Vec<String> = pairs
                .iter()
                .map(|(key, held)| {
                    let held = json_text(held, pretty, depth + 1);
                    format!("{}:{after}{held}", json_string(key))
                })
                .collect();
            format!("{{{open}{}{close}}}", written.join(&between))
        }
        Interim::Object(_) => "{}".to_string(),
        _ => "null".to_string(),
    }
}

/// A number as JSON spells it: a whole number without a fraction, and nothing JSON cannot
/// carry as null.
// @lfy def/interpret/main.lfy:expand
fn json_number(number: f64) -> String {
    if !number.is_finite() {
        return "null".to_string();
    }
    if number.fract() == 0.0 && number.abs() < 1e15 {
        return format!("{number:.0}");
    }
    number.to_string()
}

/// Text quoted with the escapes JSON requires and no others.
// @lfy def/interpret/main.lfy:expand
fn json_string(text: &str) -> String {
    let mut quoted = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            '\u{8}' => quoted.push_str("\\b"),
            '\u{c}' => quoted.push_str("\\f"),
            held if held < ' ' => quoted.push_str(&format!("\\u{:04x}", held as u32)),
            held => quoted.push(held),
        }
    }
    quoted.push('"');
    quoted
}

/// What a finished command left: its code, and everything it wrote.
// @lfy def/interpret/main.lfy:expand
struct Exited {
    code: i32,
    stdout: String,
    stderr: String,
}

/// Runs a command through the platform shell and waits for it, capturing its output whole.
// @lfy def/interpret/main.lfy:expand
fn run_command(
    command: &str,
    input: Option<String>,
    directory: Option<String>,
    environment: Option<&Interim>,
) -> Exited {
    let (program, flag) = match cfg!(windows) {
        true => ("cmd", "/C"),
        false => ("sh", "-c"),
    };
    let mut started = std::process::Command::new(program);
    started.arg(flag).arg(command);
    if let Some(directory) = directory {
        started.current_dir(directory);
    }
    // Each entry is set on top of the inherited environment; nothing is removed from it.
    // @lfy def/interpret/main.lfy:expand
    if let Some(Interim::Object(pairs)) = environment {
        for (name, value) in pairs {
            started.env(name, plain_text(value));
        }
    }
    started
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    // The shell could not be started, or the directory is not there.
    // @lfy def/interpret/main.lfy:expand
    let failed = |message: String| Exited {
        code: -1,
        stdout: String::new(),
        stderr: message,
    };
    let mut child = match started.spawn() {
        Ok(child) => child,
        Err(failure) => return failed(failure.to_string()),
    };
    if let Some(mut pipe) = child.stdin.take()
        && let Some(input) = input
    {
        let _ = std::io::Write::write_all(&mut pipe, input.as_bytes());
    }
    match child.wait_with_output() {
        Ok(output) => Exited {
            code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        },
        Err(failure) => failed(failure.to_string()),
    }
}

/// A value as the text an environment variable holds.
// @lfy def/interpret/main.lfy:expand
fn plain_text(value: &Interim) -> String {
    match value {
        Interim::String(text) => text.clone(),
        Interim::Number(number) => json_number(*number),
        Interim::Bool(held) => held.to_string(),
        _ => String::new(),
    }
}
