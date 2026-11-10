//! Process-group spawning and reader threads for one harness run.
//! Spec: `docs/design/crates/vowp-adapters.md` §3 and `docs/design/adapters.md` §1.
//!
//! A [`RunHandle`] owns one child process in its own process group. A stdout reader feeds
//! the [`StreamParser`], a stderr reader keeps the last 4 KiB, and a waiter reaps the child,
//! joins both readers and then calls [`StreamParser::finish`]. Signals go to the whole group
//! through `/bin/kill`; there is no `unsafe` code.

use std::collections::{BTreeMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, ErrorKind, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use vowp_protocol::{
    Actor, AdapterError, AdapterRun, BuildMode, EventBody, HarnessEvent, Note, RunId, RunSummary, Timestamp,
    UsageSource,
};

use crate::stream::StreamParser;

/// A stdout line longer than this is truncated before parsing.
const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

/// Number of stderr bytes kept for classification.
const STDERR_TAIL_BYTES: usize = 4 * 1024;

/// How long [`RunHandle::stop`] waits after SIGTERM before sending SIGKILL.
const STOP_GRACE: Duration = Duration::from_secs(3);

/// How long [`RunHandle::interrupt`] waits after SIGINT before sending SIGKILL.
const INTERRUPT_GRACE: Duration = Duration::from_secs(5);

/// How to launch one child.
#[derive(Debug, Clone)]
pub struct SpawnSpec {
    /// Full argv; `argv[0]` is the resolved absolute path.
    pub argv: Vec<String>,
    /// Working directory for the child.
    pub cwd: PathBuf,
    /// Already-filtered environment (base variables plus the caller's `env_pass`).
    pub env: Vec<(String, String)>,
    /// Bytes written to stdin then closed; `None` attaches `/dev/null`.
    pub stdin: Option<Vec<u8>>,
    /// Raw stdout lines are appended here, flushed per line, before parsing.
    pub tee_stdout: Option<PathBuf>,
    /// Raw stderr lines are appended to this path with `.err` appended.
    pub tee_stderr: Option<PathBuf>,
    /// Overall wall-clock cap; the run is killed and reported as `Killed` afterwards.
    pub timeout: Option<Duration>,
}

/// The measurement an adapter leaves beside its raw run log.
///
/// Core reads the sidecar on the next sampler tick and writes the `run_usage` row. The file is
/// named `<run-id>.usage.json` next to `<run-id>.jsonl`; it is an implementation detail of the
/// measurement path, not a public wire format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcUsage {
    /// Run id carried by the raw log path.
    pub run_id: String,
    /// Start of the adapter process.
    pub started_at: Timestamp,
    /// End of the adapter process.
    pub ended_at: Timestamp,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
    /// Peak resident memory in bytes.
    pub peak_rss_bytes: u64,
    /// Sampled anonymous memory at the peak.
    pub anon_peak_bytes: Option<u64>,
    /// Sampled file-backed pages at the peak.
    pub file_peak_bytes: Option<u64>,
    /// Sampled kernel memory at the peak.
    pub kernel_peak_bytes: Option<u64>,
    /// CPU time in microseconds.
    pub cpu_usec: u64,
    /// Mean CPU cores used.
    pub mean_cpu_cores: f64,
    /// Maximum CPU cores observed.
    pub max_cpu_cores: f64,
    /// Build-mode inference made by the adapter, when it could tell.
    pub build_mode: BuildMode,
    /// Measurement source (`proc_tree` for the adapter fallback).
    pub source: UsageSource,
    /// Whether the kernel OOM killer ended the run.
    pub oom_killed: bool,
    /// Process exit code, when the process exited normally.
    pub exit_code: Option<i32>,
}

/// Returns `<run-id>.usage.json` for a raw run log path.
fn usage_sidecar_path(raw: &Path) -> Option<PathBuf> {
    let stem = raw.file_stem()?.to_str()?;
    RunId::parse(stem).ok()?;
    Some(raw.with_extension("usage.json"))
}

/// Samples a process tree for the lifetime of one adapter run.
struct ProcMeter {
    root_pid: u32,
    started_at: Timestamp,
    started: Instant,
    last_cpu: BTreeMap<u32, u64>,
    peak_rss: u64,
    anon: u64,
    file: u64,
    cpu_ticks: u64,
    max_cores: f64,
    last_sample: Instant,
}

impl ProcMeter {
    /// Creates a meter for `root_pid`.
    fn new(root_pid: u32) -> Self {
        Self {
            root_pid,
            started_at: Timestamp::now(),
            started: Instant::now(),
            last_cpu: BTreeMap::new(),
            peak_rss: 0,
            anon: 0,
            file: 0,
            cpu_ticks: 0,
            max_cores: 0.0,
            last_sample: Instant::now(),
        }
    }

    /// Takes one sample of the process tree.
    #[cfg(target_os = "linux")]
    fn sample(&mut self) {
        let mut rss = 0_u64;
        let mut anon = 0_u64;
        let mut file = 0_u64;
        let mut cpu_by_pid = BTreeMap::new();
        let mut seen = HashSet::new();
        let mut queue = vec![self.root_pid];
        while let Some(pid) = queue.pop() {
            if !seen.insert(pid) || pid == 0 {
                continue;
            }
            if let Ok(children) = read_children(pid) {
                queue.extend(children);
            }
            let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) else {
                continue;
            };
            let mut live_rss = 0_u64;
            let mut live_anon = 0_u64;
            let mut live_file = 0_u64;
            for line in status.lines() {
                let mut parts = line.split_whitespace();
                let Some(key) = parts.next() else { continue };
                let value = parts.next().and_then(|value| value.parse::<u64>().ok()).unwrap_or(0);
                match key {
                    "VmRSS:" => live_rss = value.saturating_mul(1024),
                    "RssAnon:" => live_anon = value.saturating_mul(1024),
                    "RssFile:" => live_file = value.saturating_mul(1024),
                    _ => {}
                }
            }
            rss = rss.saturating_add(live_rss);
            anon = anon.saturating_add(live_anon);
            file = file.saturating_add(live_file);
            if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat"))
                && let Some(close) = stat.rfind(')')
            {
                let rest = &stat[close + 1..];
                let mut fields = rest.split_whitespace();
                let utime = fields.nth(11).and_then(|value| value.parse::<u64>().ok());
                let stime = fields.next().and_then(|value| value.parse::<u64>().ok());
                if let (Some(utime), Some(stime)) = (utime, stime) {
                    cpu_by_pid.insert(pid, utime.saturating_add(stime));
                }
            }
        }
        let now = Instant::now();
        let mut delta_ticks = 0_u64;
        for (pid, ticks) in &cpu_by_pid {
            let previous = self.last_cpu.get(pid).copied().unwrap_or(0);
            if *ticks >= previous {
                delta_ticks = delta_ticks.saturating_add(*ticks - previous);
            }
        }
        self.last_cpu = cpu_by_pid;
        self.cpu_ticks = self.cpu_ticks.saturating_add(delta_ticks);
        let elapsed = now.saturating_duration_since(self.last_sample).as_secs_f64();
        if elapsed > 0.0 {
            self.max_cores = self.max_cores.max(delta_ticks as f64 / 100.0 / elapsed);
        }
        self.last_sample = now;
        self.peak_rss = self.peak_rss.max(rss);
        if rss >= self.peak_rss {
            self.anon = anon;
            self.file = file;
        }
    }

    /// Takes one sample; non-Linux hosts have no `/proc` tree.
    #[cfg(not(target_os = "linux"))]
    fn sample(&mut self) {}

    /// Builds the sidecar record after the child exits.
    fn finish(self, exit_code: Option<i32>) -> Option<ProcUsage> {
        if self.peak_rss == 0 {
            return None;
        }
        let ended_at = Timestamp::now();
        let elapsed = self.started.elapsed().as_secs_f64();
        let mean = if elapsed > 0.0 {
            self.cpu_ticks as f64 / 100.0 / elapsed
        } else {
            0.0
        };
        Some(ProcUsage {
            run_id: String::new(),
            started_at: self.started_at,
            ended_at,
            duration_ms: u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX),
            peak_rss_bytes: self.peak_rss,
            anon_peak_bytes: (self.anon > 0).then_some(self.anon),
            file_peak_bytes: (self.file > 0).then_some(self.file),
            kernel_peak_bytes: None,
            cpu_usec: self.cpu_ticks.saturating_mul(10_000),
            mean_cpu_cores: mean,
            max_cpu_cores: self.max_cores.max(mean),
            build_mode: BuildMode::Unknown,
            source: UsageSource::ProcTree,
            oom_killed: false,
            exit_code,
        })
    }
}

/// Reads the direct children of `pid` on Linux.
#[cfg(target_os = "linux")]
fn read_children(pid: u32) -> io::Result<Vec<u32>> {
    let dir = format!("/proc/{pid}/task");
    let mut children = Vec::new();
    for task in std::fs::read_dir(dir)? {
        let task = task?;
        let path = task.path().join("children");
        let text = std::fs::read_to_string(path)?;
        children.extend(text.split_whitespace().filter_map(|value| value.parse::<u32>().ok()));
    }
    Ok(children)
}

/// Writes the sidecar atomically beside the raw log.
fn write_usage_sidecar(path: &Path, mut usage: ProcUsage, run_id: &str) {
    usage.run_id = run_id.to_owned();
    let Ok(text) = serde_json::to_vec(&usage) else {
        return;
    };
    let Some(parent) = path.parent() else { return };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let tmp = path.with_extension(format!("usage.tmp.{}", std::process::id()));
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(tmp, path);
    }
}

/// The signal sent to a process group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// SIGINT, the polite interrupt.
    Int,
    /// SIGTERM, the normal stop.
    Term,
    /// SIGKILL, the escalated stop.
    Kill,
}

impl Signal {
    fn flag(self) -> &'static str {
        match self {
            Self::Int => "-INT",
            Self::Term => "-TERM",
            Self::Kill => "-KILL",
        }
    }
}

/// A running child with reader threads.
///
/// Implements [`AdapterRun`] generically over a boxed [`StreamParser`]. The event channel
/// closes after the terminal events that [`StreamParser::finish`] returns.
pub struct RunHandle {
    pid: u32,
    rx: Receiver<HarnessEvent>,
    waiter: Option<JoinHandle<Result<(), AdapterError>>>,
    shared: Arc<Shared>,
    stopped: bool,
    grace: Duration,
    interrupt_grace: Duration,
}

struct Shared {
    killed: AtomicBool,
    done: AtomicBool,
    summary: Mutex<Option<RunSummary>>,
}

impl Shared {
    fn new() -> Self {
        Self {
            killed: AtomicBool::new(false),
            done: AtomicBool::new(false),
            summary: Mutex::new(None),
        }
    }
}

impl RunHandle {
    /// Spawns `spec` and starts its reader and waiter threads.
    ///
    /// Returns as soon as the child exists; the caller consumes events through
    /// [`AdapterRun::events`] and calls [`AdapterRun::wait`] when it is done.
    pub fn spawn(spec: SpawnSpec, parser: Box<dyn StreamParser>) -> Result<RunHandle, AdapterError> {
        Self::spawn_inner(spec, parser, None)
    }

    /// Spawns `spec` with an explicit stop/interrupt grace period.
    ///
    /// This is the injectable timing seam used by the process tests; production callers
    /// use [`RunHandle::spawn`].
    #[doc(hidden)]
    pub fn spawn_with_grace(
        spec: SpawnSpec,
        parser: Box<dyn StreamParser>,
        grace: Duration,
    ) -> Result<RunHandle, AdapterError> {
        Self::spawn_inner(spec, parser, Some(grace))
    }

    fn spawn_inner(
        spec: SpawnSpec,
        parser: Box<dyn StreamParser>,
        grace_override: Option<Duration>,
    ) -> Result<RunHandle, AdapterError> {
        let Some(binary) = spec.argv.first().cloned() else {
            return Err(AdapterError::Unavailable("empty argv".to_owned()));
        };

        let mut command = Command::new(&binary);
        command
            .args(&spec.argv[1..])
            .current_dir(&spec.cwd)
            .env_clear()
            .envs(spec.env.iter().map(|(name, value)| (name, value)))
            .stdin(if spec.stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);

        let mut child = command
            .spawn()
            .map_err(|source| AdapterError::Spawn { binary, source })?;
        let pid = child.id();

        let Some(stdout) = child.stdout.take() else {
            let _ = signal_group(pid, Signal::Kill);
            return Err(AdapterError::Protocol("child stdout was not piped".to_owned()));
        };
        let Some(stderr) = child.stderr.take() else {
            let _ = signal_group(pid, Signal::Kill);
            return Err(AdapterError::Protocol("child stderr was not piped".to_owned()));
        };

        if let (Some(data), Some(mut stdin)) = (spec.stdin, child.stdin.take()) {
            // Never block spawn on a large prompt: the child may not read it immediately.
            let _ = thread::Builder::new()
                .name(format!("vowp-adapters-stdin-{pid}"))
                .spawn(move || {
                    let _ = stdin.write_all(&data);
                });
        }

        let stdout_tee = open_tee(spec.tee_stdout.as_deref());
        let stderr_tee_path = spec.tee_stderr.as_deref().map(append_err_suffix);
        let stderr_tee = open_tee(stderr_tee_path.as_deref());
        let usage_path = spec.tee_stdout.as_deref().and_then(usage_sidecar_path);
        let usage_run_id = spec
            .tee_stdout
            .as_deref()
            .and_then(|path| path.file_stem())
            .and_then(|stem| stem.to_str())
            .map(str::to_owned);

        let parser = Arc::new(Mutex::new(Some(parser)));
        let shared = Arc::new(Shared::new());
        let (tx, rx) = mpsc::channel();

        let stdout_thread = spawn_stdout_reader(stdout, parser.clone(), tx.clone(), stdout_tee);
        let stderr_thread = spawn_stderr_reader(stderr, parser.clone(), stderr_tee);

        let (cancel_tx, cancel_rx) = mpsc::channel::<()>();
        let grace = grace_override.unwrap_or(STOP_GRACE);
        let interrupt_grace = grace_override.unwrap_or(INTERRUPT_GRACE);

        if let Some(timeout) = spec.timeout {
            spawn_timeout_watchdog(pid, timeout, grace, shared.clone(), cancel_rx);
        } else {
            drop(cancel_rx);
        }

        let waiter = spawn_waiter(
            child,
            parser,
            stdout_thread,
            stderr_thread,
            tx,
            shared.clone(),
            cancel_tx,
            usage_path,
            usage_run_id,
            ProcMeter::new(pid),
        );

        Ok(RunHandle {
            pid,
            rx,
            waiter: Some(waiter),
            shared,
            stopped: false,
            grace,
            interrupt_grace,
        })
    }

    /// Returns true once the waiter thread has finished.
    fn finished(&self) -> bool {
        self.waiter.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Polls until the waiter finishes or `grace` elapses.
    fn wait_for_grace(&self, grace: Duration) {
        let deadline = Instant::now() + grace;
        while !self.finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl AdapterRun for RunHandle {
    fn pid(&self) -> Option<u32> {
        Some(self.pid)
    }

    fn events(&self) -> &Receiver<HarnessEvent> {
        &self.rx
    }

    fn send(&mut self, _text: &str) -> Result<(), AdapterError> {
        Err(AdapterError::Unsupported("live_input"))
    }

    fn interrupt(&mut self) -> Result<(), AdapterError> {
        if self.stopped {
            return self.wait().map(|_| ());
        }
        self.shared.killed.store(true, Ordering::SeqCst);
        signal_group(self.pid, Signal::Int)?;
        self.wait_for_grace(self.interrupt_grace);
        if !self.finished() {
            // The polite signal did not land: perform the normal stop escalation.
            return self.stop();
        }
        self.stopped = true;
        self.wait().map(|_| ())
    }

    fn stop(&mut self) -> Result<(), AdapterError> {
        if self.stopped {
            return self.wait().map(|_| ());
        }
        self.stopped = true;
        self.shared.killed.store(true, Ordering::SeqCst);
        signal_group(self.pid, Signal::Term)?;
        self.wait_for_grace(self.grace);
        if !self.finished() {
            signal_group(self.pid, Signal::Kill)?;
        }
        self.wait().map(|_| ())
    }

    fn wait(&mut self) -> Result<RunSummary, AdapterError> {
        if let Some(waiter) = self.waiter.take() {
            match waiter.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => return Err(error),
                Err(_) => return Err(AdapterError::Protocol("run thread panicked".to_owned())),
            }
        }
        let summary = self
            .shared
            .summary
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        summary.ok_or_else(|| AdapterError::Protocol("run finished without a summary".to_owned()))
    }
}

/// Spawns the stdout reader thread.
fn spawn_stdout_reader(
    stdout: std::process::ChildStdout,
    parser: Arc<Mutex<Option<Box<dyn StreamParser>>>>,
    tx: Sender<HarnessEvent>,
    mut tee: Option<File>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut raw = Vec::new();
        loop {
            raw.clear();
            match reader.read_until(b'\n', &mut raw) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }

            if let Some(file) = tee.as_mut() {
                let _ = file.write_all(&raw);
                let _ = file.flush();
            }

            let truncated = raw.len() > MAX_LINE_BYTES;
            let bytes = if truncated { &raw[..MAX_LINE_BYTES] } else { &raw[..] };
            let mut line = String::from_utf8_lossy(bytes).into_owned();
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }

            let events = {
                let mut guard = parser.lock().unwrap_or_else(PoisonError::into_inner);
                guard.as_mut().map_or_else(Vec::new, |parser| parser.feed_line(&line))
            };
            for event in events {
                let _ = tx.send(event);
            }

            if truncated {
                let _ = tx.send(HarnessEvent {
                    ts: None,
                    actor: Actor::System,
                    body: EventBody::Note(Note {
                        text: format!("stdout line truncated to {MAX_LINE_BYTES} bytes"),
                    }),
                    raw: None,
                });
            }
        }
    })
}

/// Spawns the stderr reader thread and returns its 4 KiB tail through the join handle.
fn spawn_stderr_reader(
    stderr: std::process::ChildStderr,
    parser: Arc<Mutex<Option<Box<dyn StreamParser>>>>,
    mut tee: Option<File>,
) -> JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut raw = Vec::new();
        let mut tail = Vec::new();
        loop {
            raw.clear();
            match reader.read_until(b'\n', &mut raw) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }

            if let Some(file) = tee.as_mut() {
                let _ = file.write_all(&raw);
                let _ = file.flush();
            }

            let mut line = String::from_utf8_lossy(&raw).into_owned();
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }
            {
                let mut guard = parser.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(parser) = guard.as_mut() {
                    parser.feed_stderr(&line);
                }
            }

            tail.extend_from_slice(line.as_bytes());
            tail.push(b'\n');
            if tail.len() > STDERR_TAIL_BYTES {
                let excess = tail.len() - STDERR_TAIL_BYTES;
                tail.drain(..excess);
            }
        }
        tail
    })
}

/// Spawns the waiter that reaps the child, joins readers and calls `finish`.
#[allow(clippy::too_many_arguments)] // the waiter owns every half of one run; grouping them adds no clarity
fn spawn_waiter(
    mut child: std::process::Child,
    parser: Arc<Mutex<Option<Box<dyn StreamParser>>>>,
    stdout_thread: JoinHandle<()>,
    stderr_thread: JoinHandle<Vec<u8>>,
    tx: Sender<HarnessEvent>,
    shared: Arc<Shared>,
    cancel_tx: Sender<()>,
    usage_path: Option<PathBuf>,
    usage_run_id: Option<String>,
    mut meter: ProcMeter,
) -> JoinHandle<Result<(), AdapterError>> {
    thread::spawn(move || {
        meter.sample();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => {
                    meter.sample();
                    thread::sleep(Duration::from_millis(50));
                }
                Err(_) => break None,
            }
        };
        let exit = status.as_ref().and_then(std::process::ExitStatus::code);
        let stdout_result = stdout_thread.join();
        let stderr_tail = stderr_thread.join().unwrap_or_default();
        let killed = shared.killed.load(Ordering::SeqCst);

        if stdout_result.is_err() {
            shared.done.store(true, Ordering::SeqCst);
            drop(cancel_tx);
            return Err(AdapterError::Protocol("stdout reader thread panicked".to_owned()));
        }

        let parser = parser
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .ok_or_else(|| AdapterError::Protocol("parser already consumed".to_owned()))?;
        let stderr_text = String::from_utf8_lossy(&stderr_tail).into_owned();
        let (events, summary) = parser.finish(exit, killed, &stderr_text);
        for event in events {
            let _ = tx.send(event);
        }
        if let (Some(path), Some(run_id)) = (usage_path, usage_run_id)
            && let Some(usage) = meter.finish(exit)
        {
            write_usage_sidecar(&path, usage, &run_id);
        }
        *shared.summary.lock().unwrap_or_else(PoisonError::into_inner) = Some(summary);
        shared.done.store(true, Ordering::SeqCst);
        drop(cancel_tx);
        Ok(())
    })
}

/// Kills the group after `timeout`, escalating after the grace period.
fn spawn_timeout_watchdog(pid: u32, timeout: Duration, grace: Duration, shared: Arc<Shared>, cancel_rx: Receiver<()>) {
    let _ = thread::Builder::new()
        .name(format!("vowp-adapters-timeout-{pid}"))
        .spawn(move || match cancel_rx.recv_timeout(timeout) {
            Err(RecvTimeoutError::Timeout) => {
                shared.killed.store(true, Ordering::SeqCst);
                let _ = signal_group(pid, Signal::Term);
                let deadline = Instant::now() + grace;
                while !shared.done.load(Ordering::SeqCst) && Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(10));
                }
                if !shared.done.load(Ordering::SeqCst) {
                    let _ = signal_group(pid, Signal::Kill);
                }
            }
            Err(RecvTimeoutError::Disconnected) | Ok(()) => {}
        });
}

/// Appends `.err` to a tee path.
fn append_err_suffix(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".err");
    PathBuf::from(name)
}

/// Opens a tee file for appending, creating it when absent.
fn open_tee(path: Option<&Path>) -> Option<File> {
    let path = path?;
    OpenOptions::new().create(true).append(true).open(path).ok()
}

/// Returns the base environment given to children.
///
/// Only the variables listed in the spec that exist in this process are copied; `TERM` is
/// always `dumb`. `extra` is appended verbatim and therefore wins on duplicates.
pub fn child_env(extra: &[(String, String)]) -> Vec<(String, String)> {
    const BASE: [&str; 11] = [
        "PATH",
        "HOME",
        "USER",
        "LANG",
        "LC_ALL",
        "TERM",
        "TMPDIR",
        "XDG_RUNTIME_DIR",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
    ];

    let mut env = Vec::new();
    for name in BASE {
        if name == "TERM" {
            env.push(("TERM".to_owned(), "dumb".to_owned()));
        } else if let Ok(value) = std::env::var(name) {
            env.push((name.to_owned(), value));
        }
    }
    env.extend_from_slice(extra);
    env
}

/// Finds `name_or_path` on `PATH`, or accepts an existing executable path.
///
/// A value containing a path separator is used as a path directly; a bare name is looked
/// up in each `PATH` directory. Directories, missing files and files without any execute
/// bit are rejected. No `which` crate is involved.
pub fn resolve_binary(name_or_path: &str) -> Option<PathBuf> {
    let path = Path::new(name_or_path);
    if path.components().count() > 1 {
        return is_executable(path).then(|| path.to_path_buf());
    }

    let path_var = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path_var) {
        let candidate = directory.join(name_or_path);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// Returns true when `path` is a regular file with at least one execute bit.
fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
}

/// Runs a command to completion, killing its process group after `timeout`.
///
/// Returns `(exit code, stdout, stderr)`; the exit code is `None` when the timeout killed
/// the command. Used by adapter probes.
pub fn run_capture(argv: &[String], timeout: Duration) -> Result<(Option<i32>, String, String), AdapterError> {
    let Some(binary) = argv.first().cloned() else {
        return Err(AdapterError::Unavailable("empty argv".to_owned()));
    };

    let mut child = Command::new(&binary)
        .args(&argv[1..])
        .env_clear()
        .envs(child_env(&[]))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|source| AdapterError::Spawn { binary, source })?;
    let pid = child.id();

    let stdout_thread = child.stdout.take().map(|mut pipe| {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            bytes
        })
    });
    let stderr_thread = child.stderr.take().map(|mut pipe| {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            bytes
        })
    });

    let deadline = Instant::now() + timeout;
    let mut exit = None;
    let mut timed_out = false;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                exit = status.code();
                break;
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    timed_out = true;
                    signal_group(pid, Signal::Kill)?;
                    let _ = child.wait();
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(AdapterError::Io(error)),
        }
    }

    if timed_out {
        exit = None;
    }
    let stdout = stdout_thread.and_then(|handle| handle.join().ok()).unwrap_or_default();
    let stderr = stderr_thread.and_then(|handle| handle.join().ok()).unwrap_or_default();
    Ok((
        exit,
        String::from_utf8_lossy(&stdout).into_owned(),
        String::from_utf8_lossy(&stderr).into_owned(),
    ))
}

/// Returns true when `pid` names a process this program may safely signal.
///
/// On platforms whose `pid_t` is signed 32-bit, `kill` parses large `u32` values as negative
/// ids. In particular, `u32::MAX` becomes `-1`, which addresses every process the caller may
/// signal. VOWP therefore refuses pid/gid 0, pid 1 and every value above [`i32::MAX`].
pub fn valid_pid(pid: u32) -> bool {
    pid > 1 && pid <= i32::MAX as u32
}

/// Returns whether a process with `pid` exists.
///
/// Invalid ids (0, 1 and values above [`i32::MAX`]) always count as not alive. Linux reads
/// `/proc/<pid>`; other platforms use `kill -0`, which never signals the target.
pub fn process_alive(pid: u32) -> bool {
    if !valid_pid(pid) {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }
}

/// Sends `sig` to process group `pgid`.
///
/// Refuses invalid pids (see [`valid_pid`]) and this process's own group, because a negative
/// pid there would signal unrelated processes. A group that no longer exists is treated as
/// success.
pub fn signal_group(pgid: u32, sig: Signal) -> io::Result<()> {
    if !valid_pid(pgid) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "refusing to signal an invalid process group",
        ));
    }
    if own_process_group() == Some(pgid) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "refusing to signal this process's own group",
        ));
    }

    let output = Command::new("kill")
        .arg(sig.flag())
        .arg("--")
        .arg(format!("-{pgid}"))
        .output()?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("No such process") {
        return Ok(());
    }
    Err(io::Error::other(format!("kill failed: {}", stderr.trim())))
}

/// Returns this process's group id, from `/proc/self/stat` with a `ps` fallback.
fn own_process_group() -> Option<u32> {
    #[cfg(target_os = "linux")]
    if let Some(pgid) = own_process_group_from_proc() {
        return Some(pgid);
    }
    own_process_group_from_ps()
}

/// Reads this process's group id from `/proc/self/stat`.
#[cfg(target_os = "linux")]
fn own_process_group_from_proc() -> Option<u32> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    let close = stat.rfind(')')?;
    let rest = stat.get(close + 1..)?;
    let mut fields = rest.split_whitespace();
    let _state = fields.next()?;
    let _ppid = fields.next()?;
    fields.next()?.parse().ok()
}

/// Reads this process's group id with `ps`, which works on Linux and macOS.
fn own_process_group_from_ps() -> Option<u32> {
    let pid = std::process::id().to_string();
    let output = Command::new("ps").args(["-o", "pgid=", "-p", &pid]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_env_always_sets_dumb_terminal() {
        let env = child_env(&[("VOWP_TEST_VALUE".to_owned(), "1".to_owned())]);
        assert!(env.contains(&("TERM".to_owned(), "dumb".to_owned())));
        assert!(env.contains(&("VOWP_TEST_VALUE".to_owned(), "1".to_owned())));
    }

    #[test]
    fn resolve_binary_rejects_missing_paths() {
        assert!(resolve_binary("/definitely/not/a/vowp-binary").is_none());
    }

    #[test]
    fn valid_pid_rejects_ids_that_wrap_to_negative_pid_t() {
        for pid in [0, 1, u32::MAX, i32::MAX as u32 + 1] {
            assert!(!valid_pid(pid), "{pid} must not be signalled");
        }
        assert!(valid_pid(2));
        assert!(valid_pid(i32::MAX as u32));
    }

    #[test]
    fn process_alive_rejects_invalid_pids_and_finds_this_process() {
        for pid in [0, 1, u32::MAX, i32::MAX as u32 + 1] {
            assert!(!process_alive(pid), "{pid} must count as not alive");
        }
        assert!(process_alive(std::process::id()));
    }

    #[test]
    fn ps_fallback_reports_this_process_group() {
        let pgid = own_process_group_from_ps().expect("ps reports the process group");
        assert!(valid_pid(pgid), "process group {pgid}");
        assert_eq!(own_process_group(), Some(pgid));
    }
}
