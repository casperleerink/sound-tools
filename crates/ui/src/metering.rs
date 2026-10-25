//! Feeding a meter from the audio thread: what a view keeps for each meter it shows.
//!
//! A processor records its [`Peaks`] every block. The view takes them once per poll of the
//! session, gives the [`Ballistics`] one reading and draws again only when what the meter shows
//! changed. So a meter at rest costs no frame, and one that falls after the sound draws until it
//! is at rest.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui::{App, Context, Global, Task};
use sound_core::Peaks;

use crate::components::meter::{Ballistics, Level};
use crate::session::POLL_INTERVAL;

/// The meter of one level: its ballistics and what it shows now.
#[derive(Debug, Default)]
pub struct Metering {
    ballistics: Ballistics,
    level: Level,
}

impl Metering {
    /// What the meter shows.
    pub fn level(&self) -> Level {
        self.level
    }

    /// One reading, one poll after the last: the peaks since then, or silence when there are
    /// none, as for an instance that is gone. Whether what the meter shows changed.
    pub fn read(&mut self, peaks: Option<&Peaks>) -> bool {
        self.read_amplitudes(peaks.map_or([0.0; 2], Peaks::take))
    }

    /// The same, for peaks that were taken already because something else shows them too.
    pub fn read_amplitudes(&mut self, amplitudes: [f32; 2]) -> bool {
        let seconds = POLL_INTERVAL.as_secs_f32();
        let level = self.ballistics.read(amplitudes.map(decibels), seconds);
        let changed = level != self.level;
        self.level = level;
        changed
    }

    /// The composer clicked the clip light.
    pub fn clear_clip(&mut self) {
        self.ballistics.clear_clip();
        self.level.clipped = false;
    }

    /// At rest, for a meter that shows another level now, such as the panel of another track.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// An amplitude in dBFS: 1 is 0 dB and 0 is `-inf`.
pub fn decibels(amplitude: f32) -> f32 {
    20.0 * amplitude.log10()
}

/// Runs `tick` on the view once per poll of the session, for as long as the view lives: the
/// clock of the meters of a view. Keep the task in the view; dropping it stops the ticks.
pub fn every_poll<V: 'static>(
    cx: &mut Context<V>,
    tick: impl Fn(&mut V, &mut Context<V>) + 'static,
) -> Task<()> {
    let view = cx.weak_entity();
    let kept = Rc::new(Cell::new(true));
    on_every_poll(cx, {
        let kept = kept.clone();
        move |cx| kept.get() && view.update(cx, |view, cx| tick(view, cx)).is_ok()
    });
    // Dropped with the task, which ends the ticks.
    let guard = Stop(kept);
    cx.spawn(async move |_, _| {
        let _guard = guard;
        std::future::pending::<()>().await;
    })
}

/// Runs `tick` once per poll for as long as it says to go on. All of them share one timer, so
/// the app wakes once per poll however many views tick.
pub fn on_every_poll(cx: &mut App, tick: impl FnMut(&mut App) -> bool + 'static) {
    let ticker = cx.default_global::<Ticker>();
    ticker.ticks.push(Box::new(tick));
    if std::mem::replace(&mut ticker.running, true) {
        return;
    }
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(POLL_INTERVAL).await;
            let going = cx.update(|cx| {
                let mut ticks = std::mem::take(&mut cx.global_mut::<Ticker>().ticks);
                ticks.retain_mut(|tick| tick(cx));
                let ticker = cx.global_mut::<Ticker>();
                // Those a tick added came after it took them.
                ticks.append(&mut ticker.ticks);
                ticker.ticks = ticks;
                ticker.running = !ticker.ticks.is_empty();
                ticker.running
            });
            if !going {
                break;
            }
        }
    })
    .detach();
}

#[derive(Default)]
struct Ticker {
    ticks: Vec<Box<dyn FnMut(&mut App) -> bool>>,
    running: bool,
}

impl Global for Ticker {}

struct Stop(Rc<Cell<bool>>);

impl Drop for Stop {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

/// Peaks that several views read, such as the device output, which the transport and a page of
/// a tool of the project both show. [`Peaks`] have one reader: each take empties them. Here
/// every reader gets the loudest since its own last read, however their reads interleave.
#[derive(Clone, Debug)]
pub struct SharedPeaks {
    peaks: Peaks,
    /// What each reader has not read yet, by its place; `None` for a reader that is gone.
    unread: Rc<RefCell<Vec<Option<[f32; 2]>>>>,
}

impl SharedPeaks {
    pub fn new(peaks: Peaks) -> Self {
        Self {
            peaks,
            unread: Rc::default(),
        }
    }

    pub fn reader(&self) -> PeaksReader {
        let mut unread = self.unread.borrow_mut();
        let place = match unread.iter().position(Option::is_none) {
            Some(place) => place,
            None => {
                unread.push(None);
                unread.len() - 1
            }
        };
        if let Some(slot) = unread.get_mut(place) {
            *slot = Some([0.0; 2]);
        }
        PeaksReader {
            shared: self.clone(),
            place,
        }
    }

    /// Whether these share `peaks`.
    pub fn shares(&self, peaks: &Peaks) -> bool {
        self.peaks.same(peaks)
    }

    /// Whether a reader of them is left.
    pub fn is_read(&self) -> bool {
        self.unread.borrow().iter().any(Option::is_some)
    }
}

/// One reader of [`SharedPeaks`].
#[derive(Debug)]
pub struct PeaksReader {
    shared: SharedPeaks,
    place: usize,
}

impl PeaksReader {
    /// The loudest of each channel since this reader last took, and zero from now on.
    pub fn take(&self) -> [f32; 2] {
        let fresh = self.shared.peaks.take();
        let mut unread = self.shared.unread.borrow_mut();
        for reader in unread.iter_mut().flatten() {
            reader[0] = reader[0].max(fresh[0]);
            reader[1] = reader[1].max(fresh[1]);
        }
        (unread.get_mut(self.place))
            .and_then(|reader| reader.replace([0.0; 2]))
            .unwrap_or_default()
    }

    /// Whether it reads `peaks`.
    pub fn reads(&self, peaks: &Peaks) -> bool {
        self.shared.shares(peaks)
    }

    /// Nothing came since this reader last took. Takes nothing.
    pub fn is_silent(&self) -> bool {
        let unread = self.shared.unread.borrow();
        let own = unread.get(self.place).copied().flatten();
        self.shared.peaks.is_silent() && own.is_none_or(|own| own == [0.0; 2])
    }
}

impl Drop for PeaksReader {
    fn drop(&mut self) {
        if let Some(slot) = self.shared.unread.borrow_mut().get_mut(self.place) {
            *slot = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use sound_core::Peaks;

    use super::{Metering, SharedPeaks, decibels};

    #[test]
    fn a_reading_takes_the_peaks_and_shows_them_in_decibels() {
        assert_eq!(decibels(1.0), 0.0);
        assert_eq!(decibels(0.0), f32::NEG_INFINITY);
        let peaks = Peaks::new();
        peaks.record_block([&[0.5], &[1.0]]);
        let mut metering = Metering::default();
        assert!(metering.read(Some(&peaks)));
        let level = metering.level();
        assert!((level.now[0] + 6.0206).abs() < 1e-3, "{:?}", level.now);
        assert_eq!(level.now[1], 0.0);
        // Taken: the next reading starts from nothing and the bars fall.
        assert!(metering.read(Some(&peaks)));
        assert!(metering.level().now[1] < 0.0);
        // A meter at rest does not change, and asks for no frame.
        let mut quiet = Metering::default();
        assert!(!quiet.read(None));
    }

    #[test]
    fn every_reader_of_shared_peaks_sees_the_loudest_since_its_own_last_read() {
        let peaks = Peaks::new();
        let shared = SharedPeaks::new(peaks.clone());
        let (transport, page) = (shared.reader(), shared.reader());
        peaks.record_block([&[0.9], &[0.2]]);
        assert_eq!(transport.take(), [0.9, 0.2]);
        peaks.record_block([&[0.3], &[0.4]]);
        // The page read neither take of the transport, and still sees both.
        assert_eq!(page.take(), [0.9, 0.4]);
        assert_eq!(transport.take(), [0.3, 0.4]);
        assert_eq!(page.take(), [0.0, 0.0]);
    }
}
