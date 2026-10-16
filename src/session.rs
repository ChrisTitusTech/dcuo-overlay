//! One session reader shared by all tabs, with a separate bounded API worker.
use crate::{
    api::{self, CharacterReference},
    discovery::{NameSearch, Progress},
    memory, native, process,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct ProcessChoice {
    pub pid: i32,
    pub start_ticks: u64,
}

#[derive(Clone, Debug)]
pub struct BuildInfo {
    pub pid: i32,
    pub start_ticks: u64,
    pub sha256: String,
    pub executable_bytes: u64,
    pub machine: u16,
    pub header_access: bool,
    pub map_source: Option<std::path::PathBuf>,
}

#[derive(Clone, Debug)]
pub struct View {
    pub candidates: Vec<ProcessChoice>,
    pub selected_pid: Option<i32>,
    pub connected: bool,
    pub busy: bool,
    pub paused: bool,
    pub status: String,
    pub build: Option<BuildInfo>,
    pub scan: Option<Progress>,
    pub api: Option<CharacterReference>,
    pub api_status: String,
    pub api_fetched: Option<Instant>,
    pub native: Option<native::Sample>,
    pub native_status: String,
    /// Retained character knowledge, independent of short-lived physics samples.
    pub feats: Option<Arc<crate::feat_memory::Snapshot>>,
    pub location_generation: u64,
}

impl Default for View {
    fn default() -> Self {
        Self {
            candidates: Vec::new(),
            selected_pid: None,
            connected: false,
            busy: false,
            paused: false,
            status: "Select a game session to inspect".into(),
            build: None,
            scan: None,
            api: None,
            api_status: "No reference requested".into(),
            api_fetched: None,
            native: None,
            feats: None,
            native_status: "Experimental physics reader not connected".into(),
            location_generation: 0,
        }
    }
}

// Merge only proven observations. A transient missing sample cannot remove rows.
fn retain_feats(
    saved: &mut Option<Arc<crate::feat_memory::Snapshot>>,
    incoming: &Arc<crate::feat_memory::Snapshot>,
) {
    let Some(old) = saved
        .as_ref()
        .filter(|old| old.entity == incoming.entity && old.entity_id == incoming.entity_id)
    else {
        *saved = Some(incoming.clone());
        return;
    };
    if old.observed_at >= incoming.observed_at {
        return;
    }
    let mut merged = (**incoming).clone();
    let mut entries: std::collections::BTreeMap<_, _> = old
        .entries
        .iter()
        .map(|entry| (entry.id, entry.clone()))
        .collect();
    for entry in &incoming.entries {
        let mut entry = entry.clone();
        if let Some(previous) = entries.get(&entry.id) {
            entry.completed |= previous.completed;
        }
        entries.insert(entry.id, entry);
    }
    if entries.len() > 8192 {
        return;
    }
    merged.entries = entries.into_values().collect();
    *saved = Some(Arc::new(merged));
}

enum Operation {
    Refresh,
    Connect(i32, Option<u64>),
    Clear,
    ResetSearch,
    Resume,
    Recover(BuildInfo),
    Lookup {
        base: String,
        name: String,
        world: String,
        request: u64,
    },
    Scan(String),
}
struct Command {
    generation: u64,
    cancel: Arc<AtomicBool>,
    operation: Operation,
}
enum Update {
    Choices(Result<Vec<ProcessChoice>, String>),
    Connected(Result<BuildInfo, String>),
    Recovered(Result<BuildInfo, RecoveryFailure>),
    Reference(u64, Result<api::LookupResult, String>),
    Scan(Result<Progress, String>),
    Done,
    Lost,
    Native(Result<native::Sample, String>),
}
struct Event {
    generation: u64,
    update: Update,
}

#[derive(Debug)]
enum RecoveryFailure {
    Retry(String),
    Blocked(String),
}
struct Recovery {
    expected: BuildInfo,
    next_attempt: Instant,
    delay: Duration,
    in_flight: bool,
}
impl Recovery {
    fn new(expected: BuildInfo, now: Instant) -> Self {
        Self {
            expected,
            next_attempt: now + Duration::from_secs(2),
            delay: Duration::from_secs(2),
            in_flight: false,
        }
    }
    fn retry(&mut self, now: Instant) {
        self.delay = (self.delay * 2).min(Duration::from_secs(10));
        self.next_attempt = now + self.delay;
        self.in_flight = false;
    }
}

pub struct Controller {
    view: View,
    sender: SyncSender<Command>,
    api_sender: SyncSender<Command>,
    receiver: Receiver<Event>,
    generation: Arc<AtomicU64>,
    stopping: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    api_worker: Option<JoinHandle<()>>,
    pending: usize,
    api_request: u64,
    api_pending: bool,
    last_native_owner: Option<native::Sample>,
    feats_enabled: Arc<AtomicBool>,
    ground_spawns_enabled: Arc<AtomicBool>,
    ground_spawns_enabled_since: Option<Instant>,
    auto_reconnect: bool,
    recovery: Option<Recovery>,
}

impl Default for Controller {
    fn default() -> Self {
        Self::new()
    }
}

impl Controller {
    pub fn new() -> Self {
        let (sender, commands) = mpsc::sync_channel(8);
        let (api_sender, api_commands) = mpsc::sync_channel(2);
        // Eight queued reader commands and two API requests, with one active
        // operation per worker. Events and autonomous samples remain bounded.
        let (events, receiver) = mpsc::sync_channel(16);
        let generation = Arc::new(AtomicU64::new(0));
        let stopping = Arc::new(AtomicBool::new(false));
        let worker_generation = generation.clone();
        let worker_stop = stopping.clone();
        let ground_spawns_enabled = Arc::new(AtomicBool::new(false));
        let worker_ground_spawns = ground_spawns_enabled.clone();
        let feats_enabled = Arc::new(AtomicBool::new(false));
        let worker_feats = feats_enabled.clone();
        let api_events = events.clone();
        let api_generation = generation.clone();
        let api_stop = stopping.clone();
        let api_worker =
            thread::spawn(move || api_loop(api_commands, api_events, api_generation, api_stop));
        let worker = thread::spawn(move || {
            worker_loop(
                commands,
                events,
                worker_generation,
                worker_stop,
                worker_feats,
                worker_ground_spawns,
            )
        });
        let mut result = Self {
            view: View::default(),
            sender,
            api_sender,
            receiver,
            generation,
            stopping,
            cancel: Arc::new(AtomicBool::new(false)),
            worker: Some(worker),
            api_worker: Some(api_worker),
            pending: 0,
            api_request: 0,
            api_pending: false,
            last_native_owner: None,
            feats_enabled,
            ground_spawns_enabled,
            ground_spawns_enabled_since: None,
            auto_reconnect: false,
            recovery: None,
        };
        result.refresh_sessions();
        result
    }
    pub fn view(&self) -> &View {
        &self.view
    }
    /// Feat scans run only while their view is visible.
    pub fn set_feats_enabled(&self, enabled: bool) {
        self.feats_enabled.store(enabled, Ordering::Release);
    }
    /// Optional world traversal never resets the player or map session.
    pub fn set_ground_spawns_enabled(&mut self, enabled: bool) {
        if self.ground_spawns_enabled.load(Ordering::Acquire) == enabled {
            return;
        }
        self.ground_spawns_enabled_since = enabled.then(Instant::now);
        self.ground_spawns_enabled.store(enabled, Ordering::Release);
        if let Some(sample) = self.view.native.as_mut() {
            sample.ground_spawns.clear();
            sample.ground_spawn_sweep = None;
            sample.ground_spawns_status = if enabled {
                "Waiting for fresh ground-spawn sample"
            } else {
                "Ground-spawn reading disabled"
            }
            .into();
        }
    }
    pub fn is_recovering(&self) -> bool {
        self.recovery.is_some()
    }
    fn epoch(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
    fn invalidate(&mut self) {
        if self.api_pending {
            self.view.api_status = "Reference request cancelled; look up again when ready".into();
            self.api_pending = false;
        }
        self.cancel.store(true, Ordering::Release);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.pending = 0;
        self.view.busy = false;
        self.view.native = None;
        self.last_native_owner = None;
        self.view.feats = None;
        self.view.location_generation = self.view.location_generation.wrapping_add(1);
    }
    fn queue(&mut self, operation: Operation) -> bool {
        let command = Command {
            generation: self.epoch(),
            cancel: self.cancel.clone(),
            operation,
        };
        let sender = if matches!(command.operation, Operation::Lookup { .. }) {
            &self.api_sender
        } else {
            &self.sender
        };
        match sender.try_send(command) {
            Ok(()) => {
                self.pending += 1;
                self.view.busy = true;
                true
            }
            Err(_) => {
                self.view.status =
                    "Worker queue is busy; retry when the current operation finishes".into();
                false
            }
        }
    }
    /// Opt in to recovering only a previously connected, unexpectedly lost session.
    /// This never selects a session at startup or after manual disconnect.
    pub fn set_auto_reconnect(&mut self, enabled: bool) {
        self.auto_reconnect = enabled;
        if !enabled && self.recovery.is_some() {
            self.disconnect();
            self.view.status = "Automatic reconnect disabled; select a session to connect".into();
        }
    }
    fn recover_if_due(&mut self, now: Instant) {
        let expected = self
            .recovery
            .as_ref()
            .filter(|recovery| {
                self.auto_reconnect
                    && !self.view.paused
                    && !recovery.in_flight
                    && now >= recovery.next_attempt
            })
            .map(|recovery| recovery.expected.clone());
        if let Some(expected) = expected {
            if self.queue(Operation::Recover(expected)) {
                if let Some(recovery) = self.recovery.as_mut() {
                    recovery.in_flight = true;
                }
                self.view.status =
                    "Checking for one fresh session with the previous executable build...".into();
            } else if let Some(recovery) = self.recovery.as_mut() {
                recovery.retry(now);
            }
        }
    }
    pub fn refresh_sessions(&mut self) {
        self.queue(Operation::Refresh);
    }
    pub fn connect(&mut self, pid: i32) {
        self.begin_connect(pid, None);
    }
    pub fn connect_selection(&mut self, pid: i32, start_ticks: u64) {
        self.begin_connect(pid, Some(start_ticks));
    }
    fn begin_connect(&mut self, pid: i32, expected_start: Option<u64>) {
        self.recovery = None;
        self.invalidate();
        let candidates = std::mem::take(&mut self.view.candidates);
        self.view = View {
            candidates,
            selected_pid: Some(pid),
            location_generation: self.view.location_generation,
            status: "Checking process, executable and read access...".into(),
            ..View::default()
        };
        self.queue(Operation::Connect(pid, expected_start));
        self.refresh_sessions();
    }
    pub fn disconnect(&mut self) {
        self.recovery = None;
        self.invalidate();
        let candidates = std::mem::take(&mut self.view.candidates);
        self.view = View {
            candidates,
            status: "Disconnected; previous session data cleared".into(),
            location_generation: self.view.location_generation,
            ..View::default()
        };
        self.queue(Operation::Clear);
    }
    pub fn pause(&mut self) {
        if !self.view.connected {
            return;
        }
        self.invalidate();
        self.view.paused = true;
        self.view.native_status = "Experimental physics reads paused; values cleared".into();
        self.view.scan = None;
        self.view.status = "Memory experiments paused; previous occurrences discarded".into();
        self.queue(Operation::ResetSearch);
    }
    pub fn resume(&mut self) {
        if !self.view.connected {
            return;
        }
        self.invalidate();
        self.view.paused = false;
        self.view.status = "Session attached; experimental physics reads resuming".into();
        self.view.native_status = "Waiting for fresh experimental physics sample".into();
        self.queue(Operation::Resume);
    }
    pub fn lookup_reference(&mut self, base: &str, name: &str, world: &str) {
        self.api_request = self.api_request.wrapping_add(1);
        if !self.view.api.as_ref().is_some_and(|old| {
            old.name.eq_ignore_ascii_case(name.trim()) && old.world_id == world.trim()
        }) {
            self.view.api = None;
            self.view.api_fetched = None;
        }
        self.view.api_status = "Fetching exact API reference...".into();
        self.api_pending = self.queue(Operation::Lookup {
            base: base.into(),
            name: name.into(),
            world: world.into(),
            request: self.api_request,
        });
        if !self.api_pending {
            self.view.api_status = "Reference request was not queued; retry when ready".into();
        }
    }
    pub fn scan_name(&mut self, name: &str) {
        if !self.view.connected || self.view.paused || self.view.busy {
            return;
        }
        self.view.status = "Reading one bounded name-discovery batch...".into();
        self.queue(Operation::Scan(name.into()));
    }
    pub fn poll(&mut self) {
        while let Ok(event) = self.receiver.try_recv() {
            if event.generation != self.epoch() {
                continue;
            }
            if !matches!(event.update, Update::Lost | Update::Native(_)) {
                self.pending = self.pending.saturating_sub(1);
            }
            match event.update {
                Update::Choices(Ok(choices)) => self.view.candidates = choices,
                Update::Choices(Err(error)) => self.view.status = error,
                Update::Connected(Ok(build)) => {
                    self.view.connected = true;
                    self.view.build = Some(build);
                    self.view.status =
                        "Read access confirmed; live character profile unsupported".into();
                }
                Update::Connected(Err(error)) => {
                    self.view.connected = false;
                    self.view.build = None;
                    self.view.status = format!("Cannot attach: {error}");
                }
                Update::Recovered(result) => {
                    if self.auto_reconnect && self.recovery.is_some() {
                        match result {
                            Ok(build)
                                if self.recovery.as_ref().is_some_and(|recovery| {
                                    same_build(&recovery.expected, &build)
                                }) =>
                            {
                                self.view.connected = true;
                                self.view.selected_pid = Some(build.pid);
                                // Replace the exited session before the UI consumes recovery.
                                // This also handles PID reuse with a new process start time.
                                self.view.candidates = vec![ProcessChoice {
                                    pid: build.pid,
                                    start_ticks: build.start_ticks,
                                }];
                                self.view.build = Some(build);
                                self.view.status = "Reconnected to one fresh session with the previous executable build".into();
                                self.view.native_status =
                                    "Waiting for fresh experimental physics sample".into();
                                self.recovery = None;
                            }
                            Ok(_) | Err(RecoveryFailure::Blocked(_)) => {
                                self.view.status = match result {
                                    Err(RecoveryFailure::Blocked(error)) => error,
                                    _ => "Game build changed; select a session explicitly".into(),
                                };
                                self.recovery = None;
                            }
                            Err(RecoveryFailure::Retry(error)) => {
                                if let Some(recovery) = self.recovery.as_mut() {
                                    recovery.retry(Instant::now());
                                    self.view.status = format!(
                                        "{error}; retrying in {} seconds",
                                        recovery.delay.as_secs()
                                    );
                                }
                            }
                        }
                    }
                }
                Update::Reference(request, result) if request == self.api_request => {
                    self.api_pending = false;
                    match result {
                        Ok(result) => {
                            self.view.api = Some(result.reference);
                            self.view.api_fetched = Some(result.received_at);
                            self.view.api_status = match result.source {
                                api::LookupSource::Network => "API reference received; upstream freshness unknown; not memory verified",
                                api::LookupSource::Cache => "Cached API reference; original receipt time retained; not memory verified",
                            }.into();
                        }
                        Err(error) => self.view.api_status = error,
                    }
                }
                Update::Scan(Ok(progress)) => {
                    self.view.status = progress.status.clone();
                    self.view.scan = Some(progress);
                }
                Update::Scan(Err(error)) => {
                    self.view.scan = None;
                    self.view.status = format!("Name experiment stopped: {error}");
                }
                Update::Native(result) if self.view.connected && !self.view.paused => {
                    match result {
                        Ok(mut sample)
                            if sample.observed_at.elapsed() <= Duration::from_secs(1) =>
                        {
                            // Queued samples from before the latest enable cannot
                            // restore optional markers after an off/on transition.
                            if !self.ground_spawns_enabled.load(Ordering::Acquire)
                                || self
                                    .ground_spawns_enabled_since
                                    .is_none_or(|since| sample.observed_at < since)
                            {
                                sample.ground_spawns.clear();
                                sample.ground_spawn_sweep = None;
                                sample.ground_spawns_status =
                                    if self.ground_spawns_enabled_since.is_some() {
                                        "Waiting for fresh ground-spawn sample"
                                    } else {
                                        "Ground-spawn reading disabled"
                                    }
                                    .into();
                            }
                            if self
                                .last_native_owner
                                .as_ref()
                                .is_some_and(|old| !old.same_owner(&sample))
                            {
                                self.view.location_generation =
                                    self.view.location_generation.wrapping_add(1);
                                self.view.scan = None;
                            }
                            if self
                                .last_native_owner
                                .as_ref()
                                .is_some_and(|old| !old.same_character_owner(&sample))
                                || sample.feats_binding.is_some_and(|binding| {
                                    self.view
                                        .feats
                                        .as_ref()
                                        .is_some_and(|old| (old.entity, old.entity_id) != binding)
                                })
                            {
                                self.view.feats = None;
                            }
                            if let Some(feats) = &sample.feats {
                                retain_feats(&mut self.view.feats, feats);
                            }
                            self.last_native_owner = Some(sample.clone());
                            self.view.native = Some(sample);
                            self.view.native_status = "Experimental controlled-pawn physics; character identity and map context unproven".into();
                        }
                        Ok(_) => {
                            self.view.native = None;
                            self.view.native_status =
                                "Experimental physics sample expired; waiting for fresh data"
                                    .into();
                        }
                        Err(error) => {
                            if self.view.native.take().is_some() {
                                self.view.location_generation =
                                    self.view.location_generation.wrapping_add(1);
                            }
                            self.view.native_status =
                                format!("Experimental physics unavailable: {error}");
                        }
                    }
                }
                Update::Native(_) => {}
                Update::Lost => {
                    let expected = if self.auto_reconnect && !self.view.paused {
                        self.view.build.clone()
                    } else {
                        None
                    };
                    self.disconnect();
                    self.recovery = expected.map(|build| Recovery::new(build, Instant::now()));
                    self.view.status = if self.recovery.is_some() {
                        "Game session lost; previous data cleared; checking for the same build in 2 seconds".into()
                    } else {
                        "Game process or executable mapping changed; all session data cleared"
                            .into()
                    };
                }
                Update::Reference(_, _) | Update::Done => {}
            }
            self.view.busy = self.pending > 0;
        }
        if self
            .view
            .native
            .as_ref()
            .is_some_and(|sample| sample.observed_at.elapsed() > Duration::from_secs(1))
        {
            self.view.native = None;
            self.view.location_generation = self.view.location_generation.wrapping_add(1);
            self.view.native_status =
                "Experimental physics stale; waiting for the session reader".into();
        }
        self.recover_if_due(Instant::now());
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        self.stopping.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        // api::lookup has an eight-second global request/body timeout. No HTTP
        // worker outlives the application, even when an in-flight call is cancelled.
        if let Some(worker) = self.api_worker.take() {
            let _ = worker.join();
        }
    }
}

fn same_build(expected: &BuildInfo, current: &BuildInfo) -> bool {
    expected.sha256 == current.sha256
        && expected.executable_bytes == current.executable_bytes
        && expected.machine == current.machine
        && current.header_access
}
fn unique_recovery_candidate(
    candidates: &[process::Candidate],
) -> Result<&process::Candidate, RecoveryFailure> {
    match candidates {
        [] => Err(RecoveryFailure::Retry(
            "Waiting for the game to restart".into(),
        )),
        [candidate] => Ok(candidate),
        _ => Err(RecoveryFailure::Blocked(
            "Several game sessions are available; select one explicitly".into(),
        )),
    }
}
fn inspect_recovery(
    expected: &BuildInfo,
) -> Result<(process::Candidate, BuildInfo), RecoveryFailure> {
    let candidates = process::discover()
        .map_err(|_| RecoveryFailure::Retry("Session discovery unavailable".into()))?;
    let selected = unique_recovery_candidate(&candidates)?;
    if selected.identity.pid == expected.pid
        && selected.identity.start_ticks == expected.start_ticks
    {
        return Err(RecoveryFailure::Retry(
            "Waiting for a fresh game process; previous session is still present".into(),
        ));
    }
    let (candidate, build) = inspect(selected.identity.pid, Some(selected.identity.start_ticks))
        .map_err(RecoveryFailure::Retry)?;
    if !same_build(expected, &build) {
        return Err(RecoveryFailure::Blocked(
            "Game build changed; select a session explicitly".into(),
        ));
    }
    Ok((candidate, build))
}

fn unchanged(expected: &process::Candidate) -> bool {
    process::candidate(expected.identity.pid).is_ok_and(|current| {
        current.identity == expected.identity && current.mapping == expected.mapping
    })
}
fn check_selection(
    identity: &process::Identity,
    expected_start: Option<u64>,
) -> Result<(), String> {
    if expected_start.is_some_and(|expected| expected != identity.start_ticks) {
        return Err("Selected process was replaced; refresh and explicitly select it again".into());
    }
    Ok(())
}
fn map_source_path(executable: &std::path::Path) -> Option<std::path::PathBuf> {
    if !executable.is_absolute()
        || executable
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return None;
    }
    let matches = |path: &std::path::Path, name: &str| {
        path.file_name()
            .and_then(|v| v.to_str())
            .is_some_and(|v| v.eq_ignore_ascii_case(name))
    };
    let arch = executable.parent()?;
    let binaries = arch.parent()?;
    let unreal = binaries.parent()?;
    if !matches(executable, "DCGAME.EXE")
        || !matches(arch, "WIN64")
        || !matches(binaries, "BINARIES")
        || !matches(unreal, "UNREAL3")
    {
        return None;
    }
    Some(unreal.join("DCGAME/COOKEDPC"))
}

fn inspect(
    pid: i32,
    expected_start: Option<u64>,
) -> Result<(process::Candidate, BuildInfo), String> {
    let candidate = process::candidate(pid).map_err(|e| e.to_string())?;
    check_selection(&candidate.identity, expected_start)?;
    let metadata = process::executable_metadata(&candidate).map_err(|e| e.to_string())?;
    let header = memory::read_exact(&candidate.identity, candidate.mapping.start, 64)
        .map_err(|e| e.to_string())?;
    if !metadata.matches_dos_header(&header) || !unchanged(&candidate) {
        return Err("Mapped executable changed or header did not match".into());
    }
    let start_ticks = candidate.identity.start_ticks;
    let map_source = candidate
        .mapping
        .path
        .as_deref()
        .and_then(map_source_path)
        .filter(|path| path.join("UI/MAPS").is_dir());
    Ok((
        candidate,
        BuildInfo {
            pid,
            start_ticks,
            sha256: metadata.sha256,
            executable_bytes: metadata.size,
            machine: metadata.machine,
            header_access: true,
            map_source,
        },
    ))
}
fn deliver(
    events: &SyncSender<Event>,
    mut event: Event,
    generation: &AtomicU64,
    stopping: &AtomicBool,
) {
    loop {
        if stopping.load(Ordering::Acquire)
            || event.generation != generation.load(Ordering::Acquire)
        {
            return;
        }
        match events.try_send(event) {
            Ok(()) | Err(mpsc::TrySendError::Disconnected(_)) => return,
            Err(mpsc::TrySendError::Full(returned)) => {
                event = returned;
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

fn api_loop(
    commands: Receiver<Command>,
    events: SyncSender<Event>,
    generation: Arc<AtomicU64>,
    stopping: Arc<AtomicBool>,
) {
    let mut client = api::ReferenceClient::new();
    while !stopping.load(Ordering::Acquire) {
        let command = match commands.recv_timeout(Duration::from_millis(50)) {
            Ok(command) => command,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        let active = || {
            !command.cancel.load(Ordering::Acquire)
                && command.generation == generation.load(Ordering::Acquire)
                && !stopping.load(Ordering::Acquire)
        };
        if !active() {
            continue;
        }
        let Operation::Lookup {
            base,
            name,
            world,
            request,
        } = command.operation
        else {
            continue;
        };
        let result = client
            .lookup(&base, &name, &world, &command.cancel)
            .map_err(|error| error.to_string());
        if active() {
            deliver(
                &events,
                Event {
                    generation: command.generation,
                    update: Update::Reference(request, result),
                },
                &generation,
                &stopping,
            );
        }
    }
}

fn worker_loop(
    commands: Receiver<Command>,
    events: SyncSender<Event>,
    generation: Arc<AtomicU64>,
    stopping: Arc<AtomicBool>,
    feats_enabled: Arc<AtomicBool>,
    ground_spawns_enabled: Arc<AtomicBool>,
) {
    let mut attached: Option<process::Candidate> = None;
    let mut attached_generation = 0;
    let mut search: Option<(String, NameSearch)> = None;
    let mut last_check = Instant::now();
    let mut reader: Option<native::Reader> = None;
    let mut paused = false;
    let mut last_sample = Instant::now();
    let mut last_owner: Option<native::Sample> = None;
    let mut native_attach_error: Option<String> = None;
    while !stopping.load(Ordering::Acquire) {
        let command = match commands.recv_timeout(Duration::from_millis(50)) {
            Ok(command) => Some(command),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        if let Some(command) = command {
            let active = || {
                !command.cancel.load(Ordering::Acquire)
                    && generation.load(Ordering::Acquire) == command.generation
                    && !stopping.load(Ordering::Acquire)
            };
            if !active() {
                continue;
            }
            let update = match command.operation {
                Operation::Refresh => Update::Choices(
                    process::discover()
                        .map(|choices| {
                            choices
                                .into_iter()
                                .map(|c| ProcessChoice {
                                    pid: c.identity.pid,
                                    start_ticks: c.identity.start_ticks,
                                })
                                .collect()
                        })
                        .map_err(|e| format!("Process discovery failed: {e}")),
                ),
                Operation::Connect(pid, expected_start) => {
                    attached = None;
                    search = None;
                    reader = None;
                    paused = false;
                    last_owner = None;
                    match inspect(pid, expected_start) {
                        Ok((candidate, info)) if active() => {
                            let native_candidate = process::Candidate {
                                identity: candidate.identity.clone(),
                                mapping: candidate.mapping.clone(),
                            };
                            match native::Reader::attach(native_candidate) {
                                Ok(mut attached_reader) => {
                                    attached_reader.set_cancel(command.cancel.clone());
                                    reader = Some(attached_reader);
                                    native_attach_error = None;
                                }
                                Err(error) => native_attach_error = Some(error),
                            }
                            attached = Some(candidate);
                            attached_generation = command.generation;
                            Update::Connected(Ok(info))
                        }
                        Ok(_) => continue,
                        Err(error) => Update::Connected(Err(error)),
                    }
                }
                Operation::Recover(expected) => {
                    attached = None;
                    reader = None;
                    search = None;
                    last_owner = None;
                    paused = false;
                    match inspect_recovery(&expected) {
                        Ok((candidate, info)) if active() => {
                            let native_candidate = process::Candidate {
                                identity: candidate.identity.clone(),
                                mapping: candidate.mapping.clone(),
                            };
                            match native::Reader::attach(native_candidate) {
                                Ok(mut attached_reader) => {
                                    attached_reader.set_cancel(command.cancel.clone());
                                    reader = Some(attached_reader);
                                    native_attach_error = None;
                                }
                                Err(error) => native_attach_error = Some(error),
                            }
                            attached = Some(candidate);
                            attached_generation = command.generation;
                            Update::Recovered(Ok(info))
                        }
                        Ok(_) => continue,
                        Err(error) => Update::Recovered(Err(error)),
                    }
                }
                Operation::Clear => {
                    reader = None;
                    paused = true;
                    last_owner = None;
                    native_attach_error = None;
                    attached = None;
                    search = None;
                    Update::Done
                }
                Operation::ResetSearch => {
                    paused = true;
                    last_owner = None;
                    search = None;
                    attached_generation = command.generation;
                    if attached.is_some() {
                        Update::Done
                    } else {
                        Update::Lost
                    }
                }
                Operation::Resume => {
                    paused = false;
                    attached_generation = command.generation;
                    if let Some(reader) = reader.as_mut() {
                        reader.set_cancel(command.cancel.clone());
                    } else if let Some(candidate) = attached.as_ref().filter(|c| unchanged(c)) {
                        let candidate = process::Candidate {
                            identity: candidate.identity.clone(),
                            mapping: candidate.mapping.clone(),
                        };
                        match native::Reader::attach(candidate) {
                            Ok(mut new_reader) => {
                                new_reader.set_cancel(command.cancel.clone());
                                reader = Some(new_reader);
                                native_attach_error = None;
                            }
                            Err(error) => native_attach_error = Some(error),
                        }
                    }
                    if attached.is_some() {
                        Update::Done
                    } else {
                        Update::Lost
                    }
                }
                // Lookup commands are routed directly to the API worker.
                Operation::Lookup { .. } => Update::Done,
                Operation::Scan(name) => {
                    if let Some(candidate) = attached.as_ref().filter(|c| unchanged(c)) {
                        let result = (|| {
                            if search.as_ref().is_none_or(|(previous, scan)| {
                                previous != &name || scan.progress().done
                            }) {
                                search = Some((
                                    name.clone(),
                                    NameSearch::new(candidate.identity.pid, &name)?,
                                ));
                            }
                            search.as_mut().unwrap().1.step(&command.cancel)
                        })();
                        if result.is_err() {
                            search = None;
                        }
                        Update::Scan(result)
                    } else {
                        Update::Lost
                    }
                }
            };
            if active() {
                if attached.as_ref().is_some_and(|c| !unchanged(c)) {
                    attached = None;
                    search = None;
                    reader = None;
                    deliver(
                        &events,
                        Event {
                            generation: command.generation,
                            update: Update::Lost,
                        },
                        &generation,
                        &stopping,
                    );
                } else {
                    deliver(
                        &events,
                        Event {
                            generation: command.generation,
                            update,
                        },
                        &generation,
                        &stopping,
                    );
                }
            }
        }
        if attached_generation == generation.load(Ordering::Acquire)
            && !paused
            && attached.is_some()
            && last_sample.elapsed() >= Duration::from_millis(250)
        {
            last_sample = Instant::now();
            let result = if let Some(reader) = reader.as_mut() {
                reader.set_feats_enabled(feats_enabled.load(Ordering::Acquire));
                reader.set_ground_spawns_enabled(ground_spawns_enabled.load(Ordering::Acquire));
                reader.sample_with_progress(|sample| {
                    deliver(
                        &events,
                        Event {
                            generation: attached_generation,
                            update: Update::Native(sample),
                        },
                        &generation,
                        &stopping,
                    );
                })
            } else {
                Err(native_attach_error
                    .clone()
                    .unwrap_or_else(|| "Experimental reader unavailable".into()))
            };
            if let Ok(sample) = &result {
                if last_owner
                    .as_ref()
                    .is_some_and(|old| !old.same_owner(sample))
                {
                    search = None;
                }
                last_owner = Some(sample.clone());
            }
            deliver(
                &events,
                Event {
                    generation: attached_generation,
                    update: Update::Native(result),
                },
                &generation,
                &stopping,
            );
        }
        if attached_generation == generation.load(Ordering::Acquire)
            && last_check.elapsed() >= Duration::from_secs(1)
        {
            last_check = Instant::now();
            if attached.as_ref().is_some_and(|c| !unchanged(c)) {
                attached = None;
                search = None;
                reader = None;
                deliver(
                    &events,
                    Event {
                        generation: attached_generation,
                        update: Update::Lost,
                    },
                    &generation,
                    &stopping,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn map_source_comes_only_from_the_expected_game_install_layout() {
        use std::path::Path;
        assert_eq!(
            map_source_path(Path::new(
                "/games/DC Universe Online/UNREAL3/BINARIES/WIN64/DCGAME.EXE"
            )),
            Some(Path::new("/games/DC Universe Online/UNREAL3/DCGAME/COOKEDPC").into())
        );
        for path in [
            "relative/DCGAME.EXE",
            "/tmp/DCGAME.EXE",
            "/games/UNREAL3/BINARIES/WIN32/DCGAME.EXE",
            "/games/../UNREAL3/BINARIES/WIN64/DCGAME.EXE",
        ] {
            assert!(map_source_path(Path::new(path)).is_none());
        }
    }
    fn synthetic_build() -> BuildInfo {
        BuildInfo {
            pid: 42,
            start_ticks: 900,
            sha256: "synthetic-build".into(),
            executable_bytes: 64,
            machine: 0x8664,
            header_access: true,
            map_source: None,
        }
    }
    fn recovery_controller() -> (Controller, SyncSender<Event>, Receiver<Command>) {
        let (mut controller, events) = offline_controller();
        let (sender, commands) = mpsc::sync_channel(8);
        controller.sender = sender;
        controller.pending = 0;
        controller.view.connected = true;
        controller.view.build = Some(synthetic_build());
        controller.set_auto_reconnect(true);
        (controller, events, commands)
    }
    #[test]
    fn opt_in_does_not_attach_at_first_launch() {
        let (mut controller, _) = offline_controller();
        let (sender, commands) = mpsc::sync_channel(8);
        controller.sender = sender;
        controller.set_auto_reconnect(true);
        controller.recover_if_due(Instant::now() + Duration::from_secs(20));
        assert!(controller.recovery.is_none());
        assert!(commands.try_recv().is_err());
        assert!(!controller.view.connected);
    }
    #[test]
    fn loss_schedules_one_recovery_and_manual_disconnect_cancels_it() {
        let (mut controller, events, commands) = recovery_controller();
        controller.view.native = Some(native::Sample::synthetic(1));
        events
            .send(Event {
                generation: 1,
                update: Update::Lost,
            })
            .unwrap();
        controller.poll();
        assert!(controller.view.native.is_none());
        assert!(controller.view.api.is_none());
        assert!(!controller.view.connected);
        let due = controller.recovery.as_ref().unwrap().next_attempt;
        controller.recover_if_due(due - Duration::from_millis(1));
        assert!(matches!(
            commands.try_recv().unwrap().operation,
            Operation::Clear
        ));
        assert!(commands.try_recv().is_err());
        controller.recover_if_due(due);
        controller.recover_if_due(due);
        assert!(matches!(
            commands.try_recv().unwrap().operation,
            Operation::Recover(_)
        ));
        assert!(commands.try_recv().is_err());
        let cancelled_epoch = controller.epoch();
        controller.disconnect();
        controller.set_auto_reconnect(true);
        controller.recover_if_due(due + Duration::from_secs(20));
        events
            .send(Event {
                generation: cancelled_epoch,
                update: Update::Recovered(Ok(synthetic_build())),
            })
            .unwrap();
        controller.poll();
        assert!(controller.recovery.is_none());
        assert!(!controller.view.connected);
        assert!(controller.view.build.is_none());
        assert!(
            commands
                .try_iter()
                .all(|command| matches!(command.operation, Operation::Clear))
        );
    }
    #[test]
    fn paused_or_not_opted_in_session_loss_never_recovers() {
        for paused in [false, true] {
            let (mut controller, events, commands) = recovery_controller();
            controller.view.paused = paused;
            controller.set_auto_reconnect(paused);
            events
                .send(Event {
                    generation: 1,
                    update: Update::Lost,
                })
                .unwrap();
            controller.poll();
            controller.recover_if_due(Instant::now() + Duration::from_secs(20));
            assert!(controller.recovery.is_none());
            assert!(
                commands
                    .try_iter()
                    .all(|command| matches!(command.operation, Operation::Clear))
            );
        }
    }
    #[test]
    fn recovery_replaces_stale_selector_identity_including_reused_pid() {
        for pid_delta in [0, 1] {
            let (mut controller, events, _) = recovery_controller();
            let expected = synthetic_build();
            controller.view.candidates = vec![ProcessChoice {
                pid: expected.pid,
                start_ticks: expected.start_ticks,
            }];
            controller.recovery = Some(Recovery::new(expected.clone(), Instant::now()));
            let mut recovered = expected;
            recovered.pid += pid_delta;
            recovered.start_ticks += 1;
            events
                .send(Event {
                    generation: controller.epoch(),
                    update: Update::Recovered(Ok(recovered.clone())),
                })
                .unwrap();
            controller.poll();
            assert!(controller.view.connected);
            assert_eq!(controller.view.selected_pid, Some(recovered.pid));
            assert_eq!(controller.view.candidates.len(), 1);
            assert_eq!(controller.view.candidates[0].pid, recovered.pid);
            assert_eq!(
                controller.view.candidates[0].start_ticks,
                recovered.start_ticks
            );
        }
    }

    #[test]
    fn recovery_requires_unique_candidate_and_exact_build() {
        let candidate = |pid| process::Candidate {
            identity: process::Identity {
                pid,
                start_ticks: 901,
            },
            mapping: process::parse_maps("10000-11000 r--p 0 08:01 42 /synthetic/DCGAME.EXE")
                .unwrap()
                .remove(0),
        };
        assert!(matches!(
            unique_recovery_candidate(&[]),
            Err(RecoveryFailure::Retry(_))
        ));
        let candidates = [candidate(42)];
        assert_eq!(
            unique_recovery_candidate(&candidates).unwrap().identity.pid,
            42
        );
        assert!(matches!(
            unique_recovery_candidate(&[candidate(42), candidate(43)]),
            Err(RecoveryFailure::Blocked(_))
        ));
        let expected = synthetic_build();
        let mut fresh = expected.clone();
        fresh.start_ticks += 1;
        fresh.pid += 1;
        assert!(same_build(&expected, &fresh));
        for field in 0..4 {
            let mut changed = fresh.clone();
            match field {
                0 => changed.sha256.push('x'),
                1 => changed.executable_bytes += 1,
                2 => changed.machine = 0x14c,
                _ => changed.header_access = false,
            }
            assert!(!same_build(&expected, &changed));
        }
    }
    #[test]
    fn recovery_backoff_is_bounded_and_build_changes_block_recovery() {
        let now = Instant::now();
        let mut retry = Recovery::new(synthetic_build(), now);
        assert_eq!(retry.next_attempt, now + Duration::from_secs(2));
        for delay in [4, 8, 10, 10] {
            retry.retry(now);
            assert_eq!(retry.delay, Duration::from_secs(delay));
            assert_eq!(retry.next_attempt, now + Duration::from_secs(delay));
        }
        let (mut controller, events, _) = recovery_controller();
        controller.recovery = Some(Recovery::new(synthetic_build(), now));
        controller.view.connected = false;
        let mut changed = synthetic_build();
        changed.sha256.push('x');
        events
            .send(Event {
                generation: 1,
                update: Update::Recovered(Ok(changed)),
            })
            .unwrap();
        controller.poll();
        assert!(controller.recovery.is_none());
        assert!(!controller.view.connected);
        assert!(controller.view.status.contains("build changed"));
    }
    #[test]
    fn turning_recovery_off_invalidates_in_flight_results() {
        let (mut controller, events, _) = recovery_controller();
        controller.recovery = Some(Recovery::new(synthetic_build(), Instant::now()));
        let old_epoch = controller.epoch();
        controller.set_auto_reconnect(false);
        events
            .send(Event {
                generation: old_epoch,
                update: Update::Recovered(Ok(synthetic_build())),
            })
            .unwrap();
        controller.poll();
        assert!(controller.recovery.is_none());
        assert!(!controller.view.connected);
    }
    #[test]
    fn delayed_http_lookup_does_not_block_session_refresh() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let (accepted, started) = mpsc::sync_channel(1);
        let (release, response_gate) = mpsc::sync_channel(1);
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => panic!("Delayed fixture server failed: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut bytes = [0; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = stream.read(&mut bytes).unwrap();
                assert!(count > 0 && request.len() < 8192);
                request.extend_from_slice(&bytes[..count]);
            }
            accepted.send(()).unwrap();
            response_gate.recv_timeout(Duration::from_secs(3)).unwrap();
            let body = br#"{"character_id":"123","name":"Synthetic","world_id":"2"}"#;
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(headers.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
        });
        let mut controller = Controller::new();
        let deadline = Instant::now() + Duration::from_secs(2);
        while controller.pending > 0 && Instant::now() < deadline {
            controller.poll();
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(controller.pending, 0);
        controller.lookup_reference(&base, "Synthetic", "2");
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(controller.pending, 1);
        controller.refresh_sessions();
        assert_eq!(controller.pending, 2);
        let deadline = Instant::now() + Duration::from_secs(1);
        while controller.pending > 1 && Instant::now() < deadline {
            controller.poll();
            thread::sleep(Duration::from_millis(5));
        }
        // The HTTP response is still held by the server. Only the local reader
        // can have completed the additional operation and decremented pending.
        assert_eq!(controller.pending, 1);
        assert!(controller.api_pending && controller.view.api.is_none());
        release.send(()).unwrap();
        server.join().unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while controller.pending > 0 && Instant::now() < deadline {
            controller.poll();
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(controller.pending, 0);
        assert!(!controller.api_pending);
        assert_eq!(controller.view.api.as_ref().unwrap().id, "123");
        // Controller drop joins both bounded workers.
    }
    #[test]
    fn full_event_queue_does_not_block_shutdown() {
        let (events, _receiver) = mpsc::sync_channel(1);
        events
            .send(Event {
                generation: 1,
                update: Update::Done,
            })
            .unwrap();
        let generation = Arc::new(AtomicU64::new(1));
        let stopping = Arc::new(AtomicBool::new(false));
        let stop_worker = stopping.clone();
        let worker = thread::spawn(move || {
            deliver(
                &events,
                Event {
                    generation: 1,
                    update: Update::Done,
                },
                &generation,
                &stop_worker,
            )
        });
        thread::sleep(Duration::from_millis(20));
        stopping.store(true, Ordering::Release);
        worker.join().unwrap();
    }
    fn offline_controller() -> (Controller, SyncSender<Event>) {
        let (sender, _commands) = mpsc::sync_channel(8);
        let (events, receiver) = mpsc::sync_channel(16);
        (
            Controller {
                view: View::default(),
                api_sender: sender.clone(),
                sender,
                receiver,
                generation: Arc::new(AtomicU64::new(1)),
                stopping: Arc::new(AtomicBool::new(false)),
                cancel: Arc::new(AtomicBool::new(false)),
                worker: None,
                api_worker: None,
                pending: 1,
                api_request: 2,
                api_pending: false,
                last_native_owner: None,
                feats_enabled: Arc::new(AtomicBool::new(false)),
                ground_spawns_enabled: Arc::new(AtomicBool::new(false)),
                ground_spawns_enabled_since: None,
                auto_reconnect: false,
                recovery: None,
            },
            events,
        )
    }
    fn feat_snapshot(id: u32, completed: bool) -> Arc<crate::feat_memory::Snapshot> {
        Arc::new(crate::feat_memory::Snapshot {
            entity: 0x10000,
            entity_id: 77,
            observed_at: Instant::now(),
            entries: vec![crate::feat_memory::Entry {
                id,
                title: format!("Synthetic {id}"),
                description: "Goal".into(),
                points: 10,
                completed,
                eligible: true,
            }],
        })
    }
    #[test]
    fn character_knowledge_survives_physics_gaps_but_not_owner_or_session_changes() {
        let (mut controller, events) = offline_controller();
        controller.view.connected = true;
        let mut first = native::Sample::synthetic(1);
        first.feats = Some(feat_snapshot(1, true));
        events
            .send(Event {
                generation: 1,
                update: Update::Native(Ok(first)),
            })
            .unwrap();
        controller.poll();
        let saved = controller.view.feats.clone().unwrap();
        for result in [
            Err("Transient scan failure".into()),
            Ok(native::Sample::synthetic(1)),
            Ok(native::Sample::synthetic(1).with_changed_streaming()),
        ] {
            events
                .send(Event {
                    generation: 1,
                    update: Update::Native(result),
                })
                .unwrap();
            controller.poll();
            assert!(Arc::ptr_eq(&saved, controller.view.feats.as_ref().unwrap()));
        }
        controller.view.native.as_mut().unwrap().observed_at =
            Instant::now() - Duration::from_secs(2);
        controller.poll();
        assert!(controller.view.native.is_none());
        assert!(controller.view.feats.is_some());
        events
            .send(Event {
                generation: 1,
                update: Update::Native(Ok(native::Sample::synthetic(2))),
            })
            .unwrap();
        controller.poll();
        assert!(controller.view.feats.is_none());
        controller.view.feats = Some(saved);
        controller.disconnect();
        assert!(controller.view.feats.is_none());
    }
    #[test]
    fn native_entity_change_clears_retained_feats_before_replacement_scan() {
        let (mut controller, events) = offline_controller();
        controller.view.connected = true;
        controller.last_native_owner = Some(native::Sample::synthetic(1));
        controller.view.feats = Some(feat_snapshot(1, true));
        let mut changed = native::Sample::synthetic(1);
        changed.feats_binding = Some((0x10000, 88));
        events
            .send(Event {
                generation: 1,
                update: Update::Native(Ok(changed)),
            })
            .unwrap();
        controller.poll();
        assert!(controller.view.feats.is_none());
    }
    #[test]
    fn new_findings_merge_without_removing_rows_or_completed_facts() {
        let mut saved = Some(feat_snapshot(1, true));
        retain_feats(&mut saved, &feat_snapshot(2, false));
        retain_feats(&mut saved, &feat_snapshot(1, false));
        let entries = &saved.as_ref().unwrap().entries;
        assert_eq!(entries.len(), 2);
        assert!(
            entries
                .iter()
                .find(|entry| entry.id == 1)
                .unwrap()
                .completed
        );
        let mut changed = (*feat_snapshot(3, false)).clone();
        changed.entity_id = 88;
        retain_feats(&mut saved, &Arc::new(changed));
        assert_eq!(saved.as_ref().unwrap().entries.len(), 1);
        assert_eq!(saved.unwrap().entries[0].id, 3);
    }
    #[test]
    fn fresh_progress_during_feat_scan_preserves_location_generation() {
        let (mut controller, events) = offline_controller();
        controller.view.connected = true;
        controller.view.location_generation = 9;
        let mut previous = native::Sample::synthetic(1);
        previous.observed_at = Instant::now() - Duration::from_secs(2);
        controller.last_native_owner = Some(previous.clone());
        controller.view.native = Some(previous);
        for x in [2.0, 3.0, 4.0] {
            let mut fresh = native::Sample::synthetic(1);
            fresh.position[0] = x;
            events
                .send(Event {
                    generation: 1,
                    update: Update::Native(Ok(fresh)),
                })
                .unwrap();
            controller.poll();
            assert_eq!(controller.view.location_generation, 9);
            assert_eq!(controller.view.native.as_ref().unwrap().position[0], x);
        }
    }
    #[test]
    fn ground_spawn_toggle_rejects_queued_markers_without_resetting_player() {
        let (mut controller, events) = offline_controller();
        controller.view.connected = true;
        controller.view.native = Some(native::Sample::synthetic(1));
        controller.view.location_generation = 7;
        let marker_sample = || {
            let mut sample = native::Sample::synthetic(1);
            sample.ground_spawns.push(native::Collectible {
                position: [4.0, 5.0, 6.0],
                kind: native::GroundSpawnKind::Blue,
            });
            sample.ground_spawn_sweep = Some(
                native::GroundSpawnSweep {
                    completed_at: sample.observed_at,
                    spawns: sample.ground_spawns.clone(),
                }
                .into(),
            );
            sample.ground_spawns_status = "Synthetic ground spawn".into();
            sample
        };
        let before_enable = marker_sample();
        controller.set_ground_spawns_enabled(true);
        events
            .send(Event {
                generation: 1,
                update: Update::Native(Ok(before_enable)),
            })
            .unwrap();
        controller.poll();
        assert!(
            controller
                .view
                .native
                .as_ref()
                .unwrap()
                .ground_spawn_sweep
                .is_none()
        );
        assert!(
            controller
                .view
                .native
                .as_ref()
                .unwrap()
                .ground_spawns
                .is_empty()
        );
        let fresh = marker_sample();
        events
            .send(Event {
                generation: 1,
                update: Update::Native(Ok(fresh.clone())),
            })
            .unwrap();
        controller.poll();
        assert_eq!(
            controller.view.native.as_ref().unwrap().ground_spawns.len(),
            1
        );
        controller.set_ground_spawns_enabled(false);
        assert!(
            controller
                .view
                .native
                .as_ref()
                .unwrap()
                .ground_spawns
                .is_empty()
        );
        events
            .send(Event {
                generation: 1,
                update: Update::Native(Ok(fresh.clone())),
            })
            .unwrap();
        controller.poll();
        assert!(
            controller
                .view
                .native
                .as_ref()
                .unwrap()
                .ground_spawn_sweep
                .is_none()
        );
        assert!(
            controller
                .view
                .native
                .as_ref()
                .unwrap()
                .ground_spawns
                .is_empty()
        );
        controller.set_ground_spawns_enabled(true);
        events
            .send(Event {
                generation: 1,
                update: Update::Native(Ok(fresh)),
            })
            .unwrap();
        controller.poll();
        assert!(
            controller
                .view
                .native
                .as_ref()
                .unwrap()
                .ground_spawn_sweep
                .is_none()
        );
        assert!(
            controller
                .view
                .native
                .as_ref()
                .unwrap()
                .ground_spawns
                .is_empty()
        );
        assert!(
            controller
                .view
                .native
                .as_ref()
                .unwrap()
                .ground_spawns_status
                .contains("Waiting")
        );
        events
            .send(Event {
                generation: 1,
                update: Update::Native(Ok(marker_sample())),
            })
            .unwrap();
        controller.poll();
        assert_eq!(
            controller.view.native.as_ref().unwrap().ground_spawns.len(),
            1
        );
        assert_eq!(
            controller.view.native.as_ref().unwrap().position,
            [1.0, 2.0, 3.0]
        );
        assert_eq!(controller.view.location_generation, 7);
        assert_eq!(controller.epoch(), 1);
        assert_eq!(controller.pending, 1);
        assert!(controller.view.connected && !controller.view.paused);
    }
    #[test]
    fn native_updates_preserve_command_pending_and_reject_late_generation() {
        let (mut controller, events) = offline_controller();
        controller.view.connected = true;
        controller.view.busy = true;
        events
            .send(Event {
                generation: 0,
                update: Update::Native(Ok(native::Sample::synthetic(1))),
            })
            .unwrap();
        controller.poll();
        assert!(controller.view.native.is_none());
        assert_eq!(controller.pending, 1);
        events
            .send(Event {
                generation: 1,
                update: Update::Native(Ok(native::Sample::synthetic(1))),
            })
            .unwrap();
        controller.poll();
        assert!(controller.view.native.is_some());
        assert_eq!(controller.pending, 1);
        assert!(controller.view.busy);
    }
    #[test]
    fn unavailable_native_profile_preserves_independent_name_progress() {
        let (mut controller, events) = offline_controller();
        controller.view.connected = true;
        controller.view.scan = Some(Progress {
            bytes_scanned: 4096,
            total_bytes: 8192,
            utf8_hits: 0,
            utf16_hits: 0,
            candidate_count: 0,
            done: false,
            status: "Searching".into(),
            observed_name: None,
        });
        for _ in 0..3 {
            events
                .send(Event {
                    generation: 1,
                    update: Update::Native(Err("Unsupported executable".into())),
                })
                .unwrap();
            controller.poll();
            assert_eq!(controller.view.scan.as_ref().unwrap().bytes_scanned, 4096);
        }
        controller.disconnect();
        assert!(controller.view.scan.is_none());
    }
    #[test]
    fn stale_and_failed_samples_remove_values() {
        let (mut controller, events) = offline_controller();
        controller.view.connected = true;
        let mut stale = native::Sample::synthetic(1);
        stale.observed_at = Instant::now() - Duration::from_secs(2);
        events
            .send(Event {
                generation: 1,
                update: Update::Native(Ok(stale)),
            })
            .unwrap();
        controller.poll();
        assert!(controller.view.native.is_none());
        controller.view.native = Some(native::Sample::synthetic(1));
        events
            .send(Event {
                generation: 1,
                update: Update::Native(Err("Loading".into())),
            })
            .unwrap();
        controller.poll();
        assert!(controller.view.native.is_none());
        assert_eq!(controller.view.location_generation, 1);
        assert!(controller.view.native_status.contains("Loading"));
        assert_eq!(controller.pending, 1);
    }
    #[test]
    fn pawn_owner_change_invalidates_location_context() {
        let (mut controller, events) = offline_controller();
        controller.view.connected = true;
        for owner in [1, 2] {
            events
                .send(Event {
                    generation: 1,
                    update: Update::Native(Ok(native::Sample::synthetic(owner))),
                })
                .unwrap();
            controller.poll();
        }
        assert_eq!(controller.view.location_generation, 1);
        assert!(controller.view.native.is_some());
        assert_eq!(controller.pending, 1);
    }
    #[test]
    fn pause_clears_native_and_rejects_queued_samples() {
        let (mut controller, events) = offline_controller();
        controller.view.connected = true;
        controller.view.native = Some(native::Sample::synthetic(1));
        controller.pause();
        assert!(controller.view.native.is_none());
        events
            .send(Event {
                generation: 1,
                update: Update::Native(Ok(native::Sample::synthetic(1))),
            })
            .unwrap();
        events
            .send(Event {
                generation: 2,
                update: Update::Native(Ok(native::Sample::synthetic(1))),
            })
            .unwrap();
        controller.poll();
        assert!(controller.view.native.is_none());
        assert!(controller.view.paused);
    }
    #[test]
    fn old_session_results_cannot_populate_new_session() {
        let (mut controller, events) = offline_controller();
        events
            .send(Event {
                generation: 0,
                update: Update::Connected(Ok(BuildInfo {
                    pid: 42,
                    start_ticks: 900,
                    sha256: "synthetic".into(),
                    executable_bytes: 64,
                    machine: 0x8664,
                    header_access: true,
                    map_source: None,
                })),
            })
            .unwrap();
        controller.poll();
        assert!(!controller.view.connected);
        assert!(controller.view.build.is_none());
    }
    #[test]
    fn superseded_reference_error_is_ignored() {
        let (mut controller, events) = offline_controller();
        events
            .send(Event {
                generation: 1,
                update: Update::Reference(1, Err("old failure".into())),
            })
            .unwrap();
        controller.poll();
        assert_eq!(controller.view.api_status, "No reference requested");
    }
    #[test]
    fn pause_cancels_pending_reference_without_leaving_loading_state() {
        let (mut controller, events) = offline_controller();
        controller.view.connected = true;
        controller.view.api_status = "Fetching exact API reference...".into();
        controller.api_pending = true;
        controller.pause();
        events
            .send(Event {
                generation: 1,
                update: Update::Reference(2, Err("old result".into())),
            })
            .unwrap();
        controller.poll();
        assert!(controller.view.api_status.contains("cancelled"));
        assert!(!controller.api_pending);
        assert!(controller.view.api.is_none());
    }
    #[test]
    fn selected_process_start_time_must_still_match() {
        let identity = process::Identity {
            pid: 42,
            start_ticks: 900,
        };
        assert!(check_selection(&identity, Some(800)).is_err());
        assert!(check_selection(&identity, Some(900)).is_ok());
        assert!(check_selection(&identity, None).is_ok());
    }
    #[test]
    fn disconnect_clears_session_results_and_cancels_work() {
        let (mut controller, _) = offline_controller();
        controller.view.connected = true;
        controller.view.selected_pid = Some(42);
        let cancel = controller.cancel.clone();
        controller.disconnect();
        assert!(cancel.load(Ordering::Acquire));
        assert!(!controller.view.connected);
        assert!(controller.view.selected_pid.is_none());
        assert!(controller.view.scan.is_none());
        assert!(controller.view.api.is_none());
    }
}
