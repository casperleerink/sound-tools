//! The plugin's own process: it loads one plugin with the backend of its format and serves the
//! host's requests, its audio side on a real-time thread of its own.
//!
//! Three threads. A reader takes every request off the control socket and ends the process as
//! soon as the host is gone, whatever the plugin is doing. The main thread serves the requests,
//! as the host's own main thread serves a plugin in its process. The audio thread waits on the
//! audio socket and plays each block the host hands over in the shared memory.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{Ordering, fence};
use std::sync::mpsc;
use std::time::Duration;

use sound_core::{CHANNELS, PrepareConfig};

use super::protocol::{self, Answer, Broken, Message, Request};
use super::shared::{MAX_DROPPED, Mapping, RELEASE, RUN, STOP};
use super::{Hello, Identity, PLAY_ARGUMENT};
use crate::PluginFormat;
use crate::backend::{LoadedPlugin, Opening};
use crate::parameters::{by_id, playable};
use crate::processor::{Block, Laned, Played, Player, PluginAudio};

/// How often the main thread shows it is alive, also when nothing is asked of it.
const HEARTBEAT: Duration = Duration::from_millis(100);

/// How often the reader looks whether the host is still there, when nothing reaches it.
const PARENT_CHECK: Duration = Duration::from_secs(1);

/// Serves one plugin for the host that started this process with [`PLAY_ARGUMENT`] and these
/// `arguments` after it. Returns only when it could not start; otherwise the process ends here.
pub fn serve(arguments: &[OsString]) -> Result<(), String> {
    let [socket, token, shm, parent, identity] = arguments else {
        return Err(format!(
            "usage: {PLAY_ARGUMENT} <socket> <token> <memory> <parent> <identity>"
        ));
    };
    let text = |value: &OsString| value.to_string_lossy().into_owned();
    let parent: u32 = text(parent)
        .parse()
        .map_err(|_| "the parent is no process id".to_string())?;
    let socket = PathBuf::from(socket);
    let token = text(token);
    let same_build = Identity::of_this_program().is_some_and(|own| own.text() == text(identity));
    let connect = |channel: &str| {
        let mut stream = UnixStream::connect(&socket).map_err(|error| error.to_string())?;
        let hello = Hello {
            token: token.clone(),
            channel: channel.to_string(),
            same_build,
        };
        protocol::write_message(&mut stream, &hello).map_err(|error| error.to_string())?;
        Ok::<_, String>(stream)
    };
    let control = connect("control")?;
    if !same_build {
        return Err("this program is another build than the host that started it".into());
    }
    let audio = connect("audio")?;
    let mapping = Arc::new(Mapping::open(&text(shm)).map_err(|error| error.to_string())?);
    mapping.shared().heartbeat.fetch_add(1, Ordering::Relaxed);

    let requests = start_reader(control.try_clone().map_err(|e| e.to_string())?, parent)?;
    let (install, installed) = rtrb::RingBuffer::<Box<dyn PluginAudio>>::new(2);
    let (given_back, returned) = rtrb::RingBuffer::<Box<dyn PluginAudio>>::new(4);
    let audio_thread = {
        let mapping = mapping.clone();
        let audio = audio.try_clone().map_err(|e| e.to_string())?;
        std::thread::Builder::new()
            .name("plugin-audio".into())
            .spawn(move || play(audio, mapping, installed, given_back))
            .map_err(|error| error.to_string())?
    };
    let mut server = Server {
        control,
        audio,
        mapping,
        plugin: None,
        install,
        returned,
        audio_thread: Some(audio_thread),
    };
    loop {
        server
            .mapping
            .shared()
            .heartbeat
            .fetch_add(1, Ordering::Relaxed);
        match requests.recv_timeout(HEARTBEAT) {
            Ok((message, bytes)) => server.serve(message, bytes),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => end_now(),
        }
    }
}

/// Ends the process without its plugin's teardown: the host is gone, nobody waits for it.
fn end_now() -> ! {
    // SAFETY: ends the process; nothing after it runs.
    unsafe { libc::_exit(0) }
}

type Received = (Message, Option<Vec<u8>>);

/// The thread that reads the requests. It ends the process at end of file, and when its parent
/// is no longer the host that started it, which covers a socket end some other process
/// inherited.
fn start_reader(mut control: UnixStream, parent: u32) -> Result<mpsc::Receiver<Received>, String> {
    control
        .set_read_timeout(Some(PARENT_CHECK))
        .map_err(|error| error.to_string())?;
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("plugin-requests".into())
        .spawn(move || {
            let mut check = || match std::os::unix::process::parent_id() == parent {
                true => Ok(()),
                false => Err(Broken::Gone),
            };
            loop {
                let Ok(message) = protocol::read_message::<Message>(&mut control, &mut check)
                else {
                    end_now();
                };
                let bytes = match &message.request {
                    Request::Load { state: true, .. } => {
                        match protocol::read_bytes(&mut control, &mut check) {
                            Ok(bytes) => Some(bytes),
                            Err(_) => end_now(),
                        }
                    }
                    _ => None,
                };
                if sender.send((message, bytes)).is_err() {
                    end_now();
                }
            }
        })
        .map_err(|error| error.to_string())?;
    Ok(receiver)
}

struct Server {
    control: UnixStream,
    audio: UnixStream,
    mapping: Arc<Mapping>,
    plugin: Option<Box<dyn LoadedPlugin>>,
    install: rtrb::Producer<Box<dyn PluginAudio>>,
    returned: rtrb::Consumer<Box<dyn PluginAudio>>,
    audio_thread: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    fn laned(&self) -> Laned {
        self.mapping.clone()
    }

    fn serve(&mut self, message: Message, bytes: Option<Vec<u8>>) {
        if let Some(plugin) = &mut self.plugin {
            for change in message.sends {
                plugin.send(change);
            }
        }
        let answer = match message.request {
            Request::Load {
                found,
                sample_rate,
                offline,
                pins,
                ..
            } => {
                let config = PrepareConfig {
                    sample_rate,
                    offline,
                };
                let loaded = match found.format {
                    PluginFormat::Clap => {
                        crate::clap::load(&found, bytes.as_deref(), config, &pins)
                    }
                    PluginFormat::Vst3 => {
                        crate::vst3::load(&found, bytes.as_deref(), config, &pins)
                    }
                };
                match loaded {
                    Ok(Opening {
                        plugin,
                        started,
                        notes,
                    }) => {
                        let mut plugin = plugin;
                        // The record wins over the state just loaded, from the first block on,
                        // as `Plugins::load` sends the pins in this process: what the host sends
                        // now would reach the plugin only with its next request.
                        if !pins.is_empty() {
                            let parameters = by_id(plugin.parameters());
                            for change in playable(&parameters, &pins) {
                                plugin.send(change);
                            }
                        }
                        self.plugin = Some(plugin);
                        let audio = Box::new(Player::new(started, self.laned()));
                        let latency = audio.latency();
                        self.give_to_audio(audio, sample_rate);
                        Answer::Loaded { latency, notes }
                    }
                    Err(problem) => Answer::DidNotLoad(problem),
                }
            }
            request => {
                let Some(plugin) = self.plugin.as_mut() else {
                    return;
                };
                match request {
                    Request::Load { .. } => return,
                    Request::Poll => Answer::Polled(plugin.poll()),
                    Request::ReadPins(ids) => Answer::Pins {
                        values: plugin.read_pins(&ids),
                        hand: plugin.hand(),
                    },
                    Request::Value(id) => Answer::Value(plugin.value(id)),
                    Request::Text { id, value } => Answer::Text(plugin.text(id, value)),
                    Request::Parameters => Answer::Parameters(plugin.parameters()),
                    Request::Show(change) => Answer::Shown(plugin.show(change)),
                    Request::SaveState => {
                        let saved = plugin.save_state().and_then(|bytes| {
                            match bytes.len() <= protocol::MAX_STATE {
                                true => Ok(bytes),
                                false => Err("the plugin's state is too large to keep".into()),
                            }
                        });
                        let answer =
                            Answer::State(saved.as_ref().map(|_| ()).map_err(Clone::clone));
                        if protocol::write_message(&mut self.control, &answer).is_err() {
                            end_now();
                        }
                        if let Ok(bytes) = saved
                            && protocol::write_bytes(&mut self.control, &bytes).is_err()
                        {
                            end_now();
                        }
                        return;
                    }
                    Request::Restart {
                        sample_rate,
                        offline,
                        pins,
                    } => {
                        self.drop_returned();
                        let config = PrepareConfig {
                            sample_rate,
                            offline,
                        };
                        let laned = self.laned();
                        let Some(plugin) = self.plugin.as_mut() else {
                            return;
                        };
                        let restarted = match plugin.restart(config, &pins, &laned) {
                            None => None,
                            Some(Err(problem)) => Some(Err(problem)),
                            Some(Ok(audio)) => {
                                // Ahead of the first block of the new audio side, as the host
                                // sends them in this process.
                                for change in &pins {
                                    plugin.send(*change);
                                }
                                let latency = audio.latency();
                                self.give_to_audio(audio, sample_rate);
                                Some(Ok(latency))
                            }
                        };
                        Answer::Restarted(restarted)
                    }
                    Request::Released => {
                        self.drop_returned();
                        let released = self.plugin.as_mut().is_some_and(|plugin| plugin.released());
                        Answer::Released(released)
                    }
                    Request::Quit => self.quit(),
                }
            }
        };
        if protocol::write_message(&mut self.control, &answer).is_err() {
            end_now();
        }
    }

    fn give_to_audio(&mut self, audio: Box<dyn PluginAudio>, sample_rate: u32) {
        self.mapping
            .shared()
            .sample_rate
            .store(sample_rate, Ordering::Relaxed);
        if self.install.push(audio).is_err() {
            // The host hands a new audio side over only once the last one came back, so the
            // ring always has room.
            eprintln!("error: the plugin's audio side had nowhere to go");
        }
    }

    /// Lets go of the audio sides the audio thread gave back.
    fn drop_returned(&mut self) {
        while self.returned.pop().is_ok() {}
    }

    /// The quit order: no more audio, the audio side stopped on its thread and dropped, then
    /// the plugin, which runs its own teardown. Then the process ends.
    fn quit(&mut self) -> ! {
        // Wakes the audio thread out of its read, and every later block of the host sees the
        // end at once and passes its input through.
        // A host that let go of the plugin closed its end already, which is no error.
        if let Err(error) = self.audio.shutdown(std::net::Shutdown::Both)
            && error.kind() != std::io::ErrorKind::NotConnected
        {
            eprintln!("error: the plugin's audio did not stop: {error}");
        }
        if let Some(thread) = self.audio_thread.take()
            && thread.join().is_err()
        {
            eprintln!("error: the plugin's audio thread panicked");
        }
        self.drop_returned();
        if let Some(plugin) = &mut self.plugin
            && !plugin.released()
        {
            eprintln!("error: the plugin did not let go of its audio side");
        }
        self.plugin = None;
        if protocol::write_message(&mut self.control, &Answer::Quit).is_err() {
            end_now();
        }
        end_now();
    }
}

/// The audio thread: one byte from the host, one block or command, one byte back.
fn play(
    mut socket: UnixStream,
    mapping: Arc<Mapping>,
    mut installed: rtrb::Consumer<Box<dyn PluginAudio>>,
    mut given_back: rtrb::Producer<Box<dyn PluginAudio>>,
) {
    let mut audio: Option<Box<dyn PluginAudio>> = None;
    let mut promoted_at = 0;
    let mut give_back = |audio: &mut Option<Box<dyn PluginAudio>>| {
        if let Some(mut leaving) = audio.take() {
            leaving.stop();
            // Room for every side the main thread ever hands over before it takes them back.
            if given_back.push(leaving).is_err() {
                eprintln!("error: the plugin's audio side could not be given back");
            }
        }
    };
    let mut command = [0];
    loop {
        if !matches!(socket.read(&mut command), Ok(1)) {
            give_back(&mut audio);
            return;
        }
        fence(Ordering::Acquire);
        if let Ok(new) = installed.pop() {
            give_back(&mut audio);
            audio = Some(new);
            let rate = mapping.shared().sample_rate.load(Ordering::Relaxed);
            if rate != promoted_at {
                promoted_at = rate;
                promote(rate);
            }
        }
        match command[0] {
            RUN => run(&mapping, audio.as_deref_mut()),
            STOP => {
                if let Some(audio) = &mut audio {
                    audio.stop();
                }
            }
            RELEASE => give_back(&mut audio),
            _ => {}
        }
        fence(Ordering::Release);
        if socket.write_all(&command).is_err() {
            give_back(&mut audio);
            return;
        }
    }
}

/// Real-time priority for this thread, with a period of one sub-block. Without it the plugin
/// plays at normal priority, which is no reason to stop.
fn promote(sample_rate: u32) {
    if let Err(error) = audio_thread_priority::promote_current_thread_to_real_time(
        sound_core::MAX_BLOCK as u32,
        sample_rate.max(1),
    ) {
        eprintln!("the plugin's audio thread plays at normal priority: {error}");
    }
}

/// One block, from the shared memory and back.
#[rtsan_standalone::nonblocking]
fn run(mapping: &Mapping, audio: Option<&mut (dyn PluginAudio + 'static)>) {
    // SAFETY: flushes denormals for the closure only and restores the mode after it, as
    // `Engine::process_block` does, so a plugin computes here what it would in the host.
    unsafe { no_denormals::no_denormals(|| play_block(mapping, audio)) }
}

fn play_block(mapping: &Mapping, audio: Option<&mut (dyn PluginAudio + 'static)>) {
    {
        let shared = mapping.shared();
        let Some(audio) = audio else {
            shared.played.store(0, Ordering::Relaxed);
            return;
        };
        // SAFETY: between the host's byte and this side's answer, after the fence above.
        let ((begin, pins), (events, lanes)) = unsafe { (shared.take_pins(), shared.block()) };
        if let Some(pins) = pins {
            match begin {
                true => audio.begin(pins),
                false => audio.pins(pins),
            }
        }
        let frames = (shared.frames.load(Ordering::Relaxed) as usize).min(sound_core::MAX_BLOCK);
        let mut controls = crate::processor::Control::REST;
        shared.read_controls(&mut controls);
        // SAFETY: as above; the host does not touch them until the answer.
        let (input, output) = unsafe { (&*shared.input.get(), &mut *shared.output.get()) };
        let input: [&[f32]; CHANNELS] = [&input[0][..frames], &input[1][..frames]];
        let [left, right] = output;
        let block = Block {
            events,
            lanes,
            input,
            left: &mut left[..frames],
            right: &mut right[..frames],
        };
        let Played { played, dropped } = audio.play(block, &mut controls);
        shared.write_controls(&controls);
        shared.played.store(u32::from(played), Ordering::Relaxed);
        shared
            .dropped
            .store(dropped.min(MAX_DROPPED), Ordering::Relaxed);
    }
}
