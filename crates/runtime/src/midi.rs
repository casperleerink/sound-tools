use midir::{MidiInput, MidiInputConnection, MidiInputPort};
use sound_core::{Error, Result};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender, TrySendError},
};

pub(super) const CAPACITY: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Event {
    Note { key: u8, velocity: u8, on: bool },
    AllNotesOff,
}

fn parse(bytes: &[u8]) -> Option<Event> {
    let &[status, key, velocity] = bytes else {
        return None;
    };
    if key > 127 || velocity > 127 {
        return None;
    }
    match status & 0xf0 {
        0x80 | 0x90 => Some(Event::Note {
            key,
            velocity,
            on: status & 0xf0 == 0x90 && velocity != 0,
        }),
        0xb0 if key == 120 || key == 123 => Some(Event::AllNotesOff),
        _ => None,
    }
}

pub(super) struct Sender {
    sender: SyncSender<Event>,
    overflow: Arc<AtomicBool>,
}

impl Sender {
    pub(super) fn receive(&mut self, bytes: &[u8]) {
        if self.overflow.load(Ordering::Acquire) {
            return;
        }
        if let Some(event) = parse(bytes)
            && let Err(TrySendError::Full(_)) = self.sender.try_send(event)
        {
            self.overflow.store(true, Ordering::Release);
        }
    }
}

pub(super) struct Queue {
    receiver: Receiver<Event>,
    overflow: Arc<AtomicBool>,
}

impl Queue {
    pub(super) fn new() -> (Sender, Self) {
        let (sender, receiver) = mpsc::sync_channel(CAPACITY);
        let overflow = Arc::new(AtomicBool::new(false));
        (
            Sender {
                sender,
                overflow: Arc::clone(&overflow),
            },
            Self { receiver, overflow },
        )
    }

    pub(super) fn drain(&self, mut receive: impl FnMut(Event)) {
        for _ in 0..CAPACITY {
            if self.overflow.load(Ordering::Acquire) {
                break;
            }
            let Ok(event) = self.receiver.try_recv() else {
                break;
            };
            receive(event);
        }
        if self.overflow.load(Ordering::Acquire) {
            for _ in 0..CAPACITY {
                if self.receiver.try_recv().is_err() {
                    break;
                }
            }
            receive(Event::AllNotesOff);
            self.overflow.store(false, Ordering::Release);
        }
    }
}

#[derive(Default)]
pub(super) struct Input {
    ports: Vec<(MidiInputPort, String)>,
    connection: Option<MidiInputConnection<Sender>>,
    port: Option<MidiInputPort>,
    pub(super) queue: Option<Queue>,
    pub(super) track: Option<String>,
}

impl Input {
    pub(super) fn refresh(&mut self) -> Result<Vec<String>> {
        self.ports.clear();
        let input = MidiInput::new("sound-tools-discovery")
            .map_err(|error| Error(format!("MIDI input unavailable: {error}")))?;
        self.ports = input
            .ports()
            .into_iter()
            .map(|port| {
                let name = input
                    .port_name(&port)
                    .map_err(|error| Error(format!("Cannot read MIDI port: {error}")))?;
                Ok((port, name))
            })
            .collect::<Result<_>>()?;
        Ok(self.ports.iter().map(|(_, name)| name.clone()).collect())
    }

    pub(super) fn port_missing(&self) -> bool {
        self.port
            .as_ref()
            .is_some_and(|connected| !self.ports.iter().any(|(port, _)| port == connected))
    }

    pub(super) fn connect(&mut self, index: usize, track: String) -> Result<String> {
        let (port, name) = self.ports.get(index).ok_or_else(|| {
            Error("MIDI port is unavailable. Refresh MIDI ports and retry.".into())
        })?;
        let input = MidiInput::new("sound-tools")
            .map_err(|error| Error(format!("MIDI input unavailable: {error}")))?;
        let (sender, queue) = Queue::new();
        let connection = input
            .connect(
                port,
                "sound-tools-input",
                |_, bytes, sender| sender.receive(bytes),
                sender,
            )
            .map_err(|error| Error(format!("Cannot connect MIDI input: {error}")))?;
        self.connection = Some(connection);
        self.queue = Some(queue);
        self.track = Some(track);
        self.port = Some(port.clone());
        Ok(name.clone())
    }

    pub(super) fn disconnect(&mut self) {
        self.connection.take();
        self.queue = None;
        self.track = None;
        self.port = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_notes_on_every_channel_and_zero_velocity_as_off() {
        for channel in 0..16 {
            for (status, velocity, on) in [(0x90, 100, true), (0x90, 0, false), (0x80, 64, false)] {
                assert_eq!(
                    parse(&[status | channel, 60, velocity]),
                    Some(Event::Note {
                        key: 60,
                        velocity,
                        on
                    })
                );
            }
            for controller in [120, 123] {
                assert_eq!(
                    parse(&[0xb0 | channel, controller, 0]),
                    Some(Event::AllNotesOff)
                );
            }
        }
    }

    #[test]
    fn ignores_unsupported_and_malformed_messages() {
        for bytes in [
            &[][..],
            &[0x90],
            &[0x90, 60],
            &[0x90, 60, 100, 0],
            &[0x90, 128, 100],
            &[0x90, 60, 128],
            &[0x70, 60, 100],
            &[0xb0, 64, 127],
            &[0xa0, 60, 100],
            &[0xc0, 1],
            &[0xd0, 1],
            &[0xe0, 0, 64],
            &[0xf8],
            &[0xf0, 1, 0xf7],
        ] {
            assert_eq!(parse(bytes), None, "{bytes:?}");
        }
    }

    #[test]
    fn overflow_discards_backlog_resets_voices_and_accepts_new_notes() {
        let (mut sender, queue) = Queue::new();
        for _ in 0..CAPACITY {
            sender.receive(&[0x90, 60, 100]);
        }
        sender.receive(&[0x80, 60, 0]);
        sender.receive(&[0x90, 61, 100]);
        let mut events = Vec::new();
        queue.drain(|event| events.push(event));
        assert_eq!(events, [Event::AllNotesOff]);
        sender.receive(&[0x90, 62, 100]);
        queue.drain(|event| events.push(event));
        assert_eq!(
            events[1],
            Event::Note {
                key: 62,
                velocity: 100,
                on: true
            }
        );
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn overflow_during_drain_ends_with_reset() {
        let (mut sender, queue) = Queue::new();
        sender.receive(&[0x90, 60, 100]);
        let mut events = Vec::new();
        queue.drain(|event| {
            events.push(event);
            if matches!(event, Event::Note { .. }) {
                for _ in 0..=CAPACITY {
                    sender.receive(&[0x90, 61, 100]);
                }
            }
        });
        assert_eq!(events.last(), Some(&Event::AllNotesOff));
        queue.drain(|_| panic!("stale event after recovery"));
    }
}
