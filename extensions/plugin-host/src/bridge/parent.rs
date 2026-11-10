//! The host's side of a plugin in a process of its own: [`Bridged`] for the table, and
//! [`BridgeAudio`] for the engine.
//!
//! Every control call waits for its answer. A child is hung only when its main thread has not
//! come back to its loop for [`Isolation::hung_after`], or [`Isolation::hung_loading_after`]
//! for a request that loads or saves. A crash, a hang, or an answer that is
//! no answer of ours makes the plugin crashed, and from then on every call answers at once with
//! nothing.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::process::{Child, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering, fence};
use std::time::{Duration, Instant};

use sound_core::{CHANNELS, PrepareConfig};

use super::protocol::{self, Answer, Broken, Message, Request};
use super::shared::{MAX_DROPPED, Mapping, RELEASE, RUN, STOP};
use super::{Channel, Hello, Identity, Isolation, PLAY_ARGUMENT};
use crate::backend::{Hand, LoadedPlugin, ParameterChange, PluginGui, Requests};
use crate::parameters::Parameter;
use crate::processor::{
    AutomatedPins, Block, Control, Laned, Played, PluginAudio, not_ours, pass_through,
};
use crate::scan::{ScanCommand, ScannedPlugin};
use crate::{Pin, PluginProblem};

/// How long a child has to start and connect, before any plugin code runs.
const START: Duration = Duration::from_secs(10);

/// How long a quitting child may take for its plugin's teardown before it is killed.
const QUIT: Duration = Duration::from_secs(10);

/// What the table gets of a plugin that loaded in a process of its own.
pub(crate) struct Opened {
    pub plugin: Box<dyn LoadedPlugin>,
    pub audio: Box<dyn PluginAudio>,
    pub laned: Laned,
    pub notes: Vec<PluginProblem>,
}

/// What both sides of a bridged plugin share.
struct Link {
    mapping: Arc<Mapping>,
    audio: UnixStream,
    /// The child is gone, hung or not to be trusted. Set by either side, read by both.
    failed: AtomicBool,
    block_timeout: Duration,
}

impl Link {
    fn fail(&self) {
        self.failed.store(true, Ordering::Release);
    }

    fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }

    /// One byte to the child's audio thread and its answer, within the block timeout. The
    /// socket calls are the call into the plugin: what the realtime sanitizer checks is our
    /// code around them.
    fn command(&self, byte: u8) -> bool {
        if self.failed() {
            return false;
        }
        fence(Ordering::Release);
        let answered = not_ours(|| {
            let mut socket = &self.audio;
            if socket.write_all(&[byte]).is_err() {
                return false;
            }
            let mut ready = libc::pollfd {
                fd: self.audio.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let milliseconds = self.block_timeout.as_millis().min(i32::MAX as u128) as i32;
            // SAFETY: one valid descriptor, for the length given.
            if unsafe { libc::poll(&mut ready, 1, milliseconds) } != 1 {
                return false;
            }
            let mut answer = [0];
            matches!(socket.read(&mut answer), Ok(1)) && answer[0] == byte
        });
        fence(Ordering::Acquire);
        if !answered {
            self.fail();
        }
        answered
    }
}

/// The engine's side: one block in shared memory per round trip.
pub(crate) struct BridgeAudio {
    link: Arc<Link>,
    /// Held while the engine has this side; the table tells by it that the engine gave it
    /// back.
    _held: Arc<()>,
    latency: u32,
}

impl PluginAudio for BridgeAudio {
    fn latency(&self) -> u32 {
        self.latency
    }

    fn begin(&mut self, pins: AutomatedPins) {
        self.link.mapping.shared().write_pins(pins, true);
    }

    fn pins(&mut self, pins: AutomatedPins) {
        self.link.mapping.shared().write_pins(pins, false);
    }

    fn play(&mut self, block: Block<'_>, controls: &mut [Control; 4]) -> Played {
        let Block {
            events,
            lanes,
            input,
            left,
            right,
        } = block;
        let frames = left.len().min(sound_core::MAX_BLOCK);
        if self.link.failed() {
            pass_through(input, left, right, frames);
            return Played {
                played: false,
                dropped: 0,
            };
        }
        let shared = self.link.mapping.shared();
        let left_over = shared.write_block(events, lanes);
        // SAFETY: the child reads these only between the byte below and its answer.
        let into = unsafe { &mut *shared.input.get() };
        for (channel, from) in into.iter_mut().zip(input) {
            channel[..frames].copy_from_slice(&from[..frames]);
        }
        shared.frames.store(frames as u32, Ordering::Relaxed);
        shared.write_controls(controls);
        if !self.link.command(RUN) {
            pass_through(input, left, right, frames);
            return Played {
                played: false,
                dropped: left_over,
            };
        }
        // SAFETY: the child answered, so it is done with them.
        let output = unsafe { &*shared.output.get() };
        let [from_left, from_right]: &[[f32; sound_core::MAX_BLOCK]; CHANNELS] = output;
        left[..frames].copy_from_slice(&from_left[..frames]);
        right[..frames].copy_from_slice(&from_right[..frames]);
        shared.read_controls(controls);
        let played = shared.played.load(Ordering::Relaxed) != 0;
        if !played {
            pass_through(input, left, right, frames);
        }
        let dropped = shared.dropped.load(Ordering::Relaxed).min(MAX_DROPPED);
        Played {
            played,
            dropped: dropped + left_over,
        }
    }

    fn stop(&mut self) {
        self.link.command(STOP);
    }
}

/// The table's side.
pub(crate) struct Bridged {
    link: Arc<Link>,
    control: UnixStream,
    child: Option<Child>,
    /// The values sent since the last request, the newest of each parameter, in order.
    sends: Vec<ParameterChange>,
    /// Held by the audio side the engine has, see [`BridgeAudio`].
    held: Arc<()>,
    /// The child's audio thread gave its audio side back since the engine gave this one back.
    audio_released: bool,
    /// The hand the last read of the pins brought along.
    hand: Option<Hand>,
    crashed: bool,
    hung_after: Duration,
    hung_loading_after: Duration,
    /// No call waits past this, once the host closes.
    deadline: Option<Instant>,
}

impl Bridged {
    /// One request and its answer. A plugin that crashed answers nothing.
    fn call(&mut self, request: Request) -> Option<Answer> {
        if self.crashed() {
            return None;
        }
        let hung_after = self.hung_after(&request);
        let message = Message {
            sends: std::mem::take(&mut self.sends),
            request,
        };
        if protocol::write_message(&mut self.control, &message).is_err() {
            self.crash();
            return None;
        }
        self.answer(hung_after)
    }

    fn hung_after(&self, request: &Request) -> Duration {
        if request.is_loading() {
            self.hung_loading_after
        } else {
            self.hung_after
        }
    }

    fn answer(&mut self, hung_after: Duration) -> Option<Answer> {
        let Self {
            link,
            control,
            child,
            deadline,
            ..
        } = self;
        let read = {
            let mut check = waiting(link, child, hung_after, *deadline);
            protocol::read_message(control, &mut check)
        };
        match read {
            Ok(answer) => Some(answer),
            Err(_) => {
                self.crash();
                None
            }
        }
    }

    fn bytes(&mut self) -> Option<Vec<u8>> {
        let Self {
            link,
            control,
            child,
            hung_loading_after,
            deadline,
            ..
        } = self;
        let read = {
            let mut check = waiting(link, child, *hung_loading_after, *deadline);
            protocol::read_bytes(control, &mut check)
        };
        match read {
            Ok(bytes) => Some(bytes),
            Err(_) => {
                self.crash();
                None
            }
        }
    }

    /// The plugin is crashed for good: its process is killed, so whatever it was doing ends.
    fn crash(&mut self) {
        self.crashed = true;
        self.link.fail();
        if let Some(child) = &mut self.child
            && let Err(error) = child.kill()
            && error.kind() != std::io::ErrorKind::InvalidInput
        {
            eprintln!("error: the plugin's process could not be ended: {error}");
        }
    }

    /// Whether the engine still has the audio side.
    fn engine_has_audio(&self) -> bool {
        Arc::strong_count(&self.held) > 1
    }

    /// Asks the child's audio thread for its audio side, once the engine gave this one back.
    fn release_audio(&mut self) -> bool {
        if !self.audio_released {
            self.audio_released = self.link.command(RELEASE);
        }
        self.audio_released
    }
}

/// What a wait for an answer checks each time the socket's timeout passes: the child is still
/// there, its audio side did not fail, its main thread still turns its loop, and the host is not
/// past closing.
fn waiting<'a>(
    link: &'a Link,
    child: &'a mut Option<Child>,
    hung_after: Duration,
    deadline: Option<Instant>,
) -> impl FnMut() -> Result<(), Broken> + 'a {
    let heartbeat = &link.mapping.shared().heartbeat;
    let mut last = (heartbeat.load(Ordering::Relaxed), Instant::now());
    move || {
        if link.failed() || deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(Broken::GaveUp);
        }
        if child
            .as_mut()
            .is_none_or(|child| !matches!(child.try_wait(), Ok(None)))
        {
            return Err(Broken::Gone);
        }
        let beat = heartbeat.load(Ordering::Relaxed);
        if beat != last.0 {
            last = (beat, Instant::now());
        } else if last.1.elapsed() >= hung_after {
            return Err(Broken::GaveUp);
        }
        Ok(())
    }
}

impl LoadedPlugin for Bridged {
    fn poll(&mut self) -> Requests {
        match self.call(Request::Poll) {
            Some(Answer::Polled(requests)) => requests,
            _ => Requests::default(),
        }
    }

    fn save_state(&mut self) -> Result<Vec<u8>, String> {
        match self.call(Request::SaveState) {
            Some(Answer::State(Ok(()))) => self
                .bytes()
                .ok_or_else(|| "the plugin stopped working".to_string()),
            Some(Answer::State(Err(message))) => Err(message),
            _ => Err("the plugin stopped working".to_string()),
        }
    }

    fn gui(&mut self) -> Option<&mut dyn PluginGui> {
        None
    }

    fn value(&mut self, id: u32) -> Option<f64> {
        match self.call(Request::Value(id)) {
            Some(Answer::Value(value)) => value,
            _ => None,
        }
    }

    fn text(&mut self, id: u32, value: f64) -> Option<String> {
        match self.call(Request::Text { id, value }) {
            Some(Answer::Text(text)) => text,
            _ => None,
        }
    }

    fn parameters(&mut self) -> Vec<Parameter> {
        match self.call(Request::Parameters) {
            Some(Answer::Parameters(parameters)) => parameters,
            _ => Vec::new(),
        }
    }

    fn send(&mut self, change: ParameterChange) {
        self.sends.retain(|sent| sent.id != change.id);
        self.sends.push(change);
    }

    fn show(&mut self, change: ParameterChange) -> bool {
        matches!(self.call(Request::Show(change)), Some(Answer::Shown(true)))
    }

    fn sent_values_played(&mut self) -> bool {
        self.read_pins(&[]).is_some()
    }

    fn read_pins(&mut self, ids: &[u32]) -> Option<Vec<(u32, f64)>> {
        match self.call(Request::ReadPins(ids.to_vec())) {
            Some(Answer::Pins { values, hand }) => {
                self.hand = Some(hand);
                values
            }
            _ => None,
        }
    }

    fn hand(&mut self) -> Hand {
        self.hand.take().unwrap_or(Hand::Unknown)
    }

    fn released(&mut self) -> bool {
        if self.crashed() {
            return true;
        }
        if self.engine_has_audio() || !self.release_audio() {
            return self.crashed();
        }
        matches!(self.call(Request::Released), Some(Answer::Released(true))) || self.crashed()
    }

    fn restart(
        &mut self,
        config: PrepareConfig,
        pins: &[ParameterChange],
        _laned: &Laned,
    ) -> Option<Result<Box<dyn PluginAudio>, PluginProblem>> {
        if self.engine_has_audio() || self.crashed() {
            return None;
        }
        if !self.release_audio() {
            return None;
        }
        let request = Request::Restart {
            sample_rate: config.sample_rate,
            offline: config.offline,
            pins: pins.to_vec(),
        };
        match self.call(request)? {
            Answer::Restarted(None) => None,
            Answer::Restarted(Some(Err(problem))) => Some(Err(problem)),
            Answer::Restarted(Some(Ok(latency))) => {
                self.audio_released = false;
                self.held = Arc::new(());
                Some(Ok(Box::new(BridgeAudio {
                    link: self.link.clone(),
                    _held: self.held.clone(),
                    latency,
                })))
            }
            _ => None,
        }
    }

    fn crashed(&self) -> bool {
        self.crashed || self.link.failed()
    }

    fn closing(&mut self, deadline: Instant) {
        self.deadline = Some(deadline);
    }
}

/// Letting go: a plugin that still works quits, which runs its own teardown. Its sockets stay
/// open until it has: end of file would end it in the middle of it. While the host plays on, a
/// reaper thread waits for it and kills it when it takes too long. When the host closes, the
/// wait is here, until the closing deadline: a thread would end with this process.
impl Drop for Bridged {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        if !self.crashed() {
            let message = Message {
                sends: Vec::new(),
                request: Request::Quit,
            };
            if protocol::write_message(&mut self.control, &message).is_err() {
                self.crash();
            }
        }
        let until = match self.crashed() {
            true => Instant::now(),
            false => self.deadline.unwrap_or_else(|| Instant::now() + QUIT),
        };
        if self.deadline.is_some() {
            reap(&mut child, until);
            return;
        }
        let control = self.control.try_clone();
        let reaped = std::thread::Builder::new()
            .name("plugin-reaper".into())
            .spawn(move || {
                reap(&mut child, until);
                drop(control);
            });
        if let Err(error) = reaped {
            eprintln!("error: nothing waits for a plugin's process: {error}");
        }
    }
}

/// Waits for `child` until `until`, then ends it.
fn reap(child: &mut Child, until: Instant) {
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(20)),
            _ => break,
        }
    }
    if let Err(error) = child.kill()
        && error.kind() != std::io::ErrorKind::InvalidInput
    {
        eprintln!("error: a plugin's process could not be ended: {error}");
    }
    if let Err(error) = child.wait() {
        eprintln!("error: a plugin's process was not waited for: {error}");
    }
}

/// A process and a name no other bridge of this process has.
fn unique() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// Starts a process for the plugin `found` and loads it there, as `clap::load` and
/// `vst3::load` load one here.
pub(crate) fn open(
    program: &ScanCommand,
    isolation: &Isolation,
    found: &ScannedPlugin,
    saved: Option<&[u8]>,
    config: PrepareConfig,
    pins: &BTreeMap<u32, Pin>,
) -> Result<Opened, PluginProblem> {
    let name = found.name.clone();
    let crashed = || PluginProblem::Crashed {
        plugin_id: found.id.clone(),
        name: name.clone(),
    };
    let updated = || PluginProblem::Updated { name: name.clone() };
    let fail = |message: String| PluginProblem::DidNotLoad {
        plugin_id: found.id.clone(),
        message,
    };
    let path = program.program();
    let identity =
        Identity::remembered(path).ok_or_else(|| fail("the program is not there".into()))?;
    if Identity::of(path) != Some(identity) {
        return Err(updated());
    }
    let unique = unique();
    let socket = std::env::temp_dir().join(format!("st-{unique}.sock"));
    let shm = format!("/st{unique}");
    let listener = UnixListener::bind(&socket).map_err(|error| fail(error.to_string()));
    let started = listener.and_then(|listener| {
        let mapping = Mapping::create(&shm).map_err(|error| fail(error.to_string()))?;
        let token = format!("{:016x}", random());
        let mut command = sound_core::process::background_command(path);
        command
            .arg(PLAY_ARGUMENT)
            .arg(&socket)
            .arg(&token)
            .arg(&shm)
            .arg(std::process::id().to_string())
            .arg(identity.text())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(match isolation.quiet {
                true => Stdio::null(),
                false => Stdio::inherit(),
            });
        for (name, value) in program.environment() {
            command.env(name, value);
        }
        // Spawning is quick; what the plugin does happens in the child.
        #[allow(clippy::disallowed_methods)]
        let mut child = command.spawn().map_err(|error| fail(error.to_string()))?;
        match connect(&listener, &mut child, &token) {
            Ok(Connected::Both { control, audio }) => Ok((child, control, audio, mapping)),
            Ok(Connected::OtherBuild) => {
                reap(&mut child, Instant::now());
                Err(updated())
            }
            Err(message) => {
                reap(&mut child, Instant::now());
                Err(crashed_with(found, &name, message))
            }
        }
    });
    remove_file(&socket);
    Mapping::remove(&shm);
    let (child, control, audio, mapping) = started?;
    if let Err(error) = control.set_read_timeout(Some(Duration::from_millis(100))) {
        return Err(fail(error.to_string()));
    }
    let mapping = Arc::new(mapping);
    let link = Arc::new(Link {
        mapping: mapping.clone(),
        audio,
        failed: AtomicBool::new(false),
        block_timeout: isolation.block_timeout,
    });
    let mut bridged = Bridged {
        link: link.clone(),
        control,
        child: Some(child),
        sends: Vec::new(),
        held: Arc::new(()),
        audio_released: false,
        hand: None,
        crashed: false,
        hung_after: isolation.hung_after,
        hung_loading_after: isolation.hung_loading_after,
        deadline: None,
    };
    let request = Request::Load {
        found: found.clone(),
        sample_rate: config.sample_rate,
        offline: config.offline,
        pins: pins.clone(),
        state: saved.is_some(),
    };
    let message = Message {
        sends: Vec::new(),
        request,
    };
    let sent = protocol::write_message(&mut bridged.control, &message).and_then(|()| match saved {
        Some(bytes) => protocol::write_bytes(&mut bridged.control, bytes),
        None => Ok(()),
    });
    if sent.is_err() {
        bridged.crash();
        return Err(crashed());
    }
    match bridged.answer(bridged.hung_loading_after) {
        Some(Answer::Loaded { latency, notes }) => {
            let audio = Box::new(BridgeAudio {
                link,
                _held: bridged.held.clone(),
                latency,
            });
            Ok(Opened {
                plugin: Box::new(bridged),
                audio,
                laned: mapping,
                notes,
            })
        }
        Some(Answer::DidNotLoad(problem)) => Err(problem),
        _ => {
            bridged.crash();
            Err(crashed())
        }
    }
}

fn crashed_with(found: &ScannedPlugin, name: &str, message: String) -> PluginProblem {
    eprintln!("error: the process of {name} did not start: {message}");
    PluginProblem::Crashed {
        plugin_id: found.id.clone(),
        name: name.to_string(),
    }
}

enum Connected {
    Both {
        control: UnixStream,
        audio: UnixStream,
    },
    OtherBuild,
}

/// Takes the child's two connections, each named by its first message with the token, within
/// [`START`]. A connection without the token is not the child's and is closed.
fn connect(listener: &UnixListener, child: &mut Child, token: &str) -> Result<Connected, String> {
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let until = Instant::now() + START;
    let (mut control, mut audio) = (None, None);
    while control.is_none() || audio.is_none() {
        if Instant::now() >= until {
            return Err("it did not connect in time".into());
        }
        if !matches!(child.try_wait(), Ok(None)) {
            return Err("it ended before it connected".into());
        }
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }
            Err(error) => return Err(error.to_string()),
        };
        stream
            .set_nonblocking(false)
            .and_then(|()| stream.set_read_timeout(Some(Duration::from_millis(100))))
            .map_err(|error| error.to_string())?;
        let mut check = || match Instant::now() < until {
            true => Ok(()),
            false => Err(Broken::GaveUp),
        };
        let Ok(hello) = protocol::read_message::<Hello>(&mut stream, &mut check) else {
            continue;
        };
        if hello.token != token {
            continue;
        }
        if !hello.same_build {
            return Ok(Connected::OtherBuild);
        }
        match hello.channel {
            Channel::Control => control = Some(stream),
            Channel::Audio => {
                stream
                    .set_read_timeout(None)
                    .map_err(|error| error.to_string())?;
                audio = Some(stream);
            }
        }
    }
    match (control, audio) {
        (Some(control), Some(audio)) => Ok(Connected::Both { control, audio }),
        _ => Err("it did not connect".into()),
    }
}

fn remove_file(path: &Path) {
    if let Err(error) = std::fs::remove_file(path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!("error: {}: {error}", path.display());
    }
}

/// A number a connection that is not the child's cannot guess.
fn random() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u32(std::process::id());
    hasher.finish()
}
