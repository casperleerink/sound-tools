//! The arrangement view: track headers, a bar ruler, clips with a miniature of their notes,
//! the playhead, and one detail panel below: the note editor of a clip or the track panel of a
//! track, one at a time. Clips are added, selected, moved, resized, copied, pasted and deleted
//! here with the mouse and the keys, tracks are renamed, and tempo changes are added and
//! removed in the ruler.
//!
//! The views, split so that a moving playhead repaints almost nothing:
//! - [`ArrangementView`] is what the window shows. It stacks the timeline over the detail
//!   panel, and opens, swaps and closes what the panel shows.
//! - [`Timeline`] draws everything that changes with the project, the scroll and the zoom on
//!   one canvas, and only what is visible. GPUI keeps its painted frame while it is not
//!   notified, so playback does not run this code.
//! - [`NoteEditor`] does the same for the notes of one clip.
//! - [`TrackPanel`] shows the devices of one track, each in the view of its own tool.
//! - [`MasterPanel`] shows the master: its volume and its limiter. The master row under the
//!   tracks opens it.
//! - A `PlayheadLine` on top of each draws one line, every frame while the project plays.
//!
//! All positions come from [`layout`], and what a drag does to a clip from [`gesture`]. The
//! timeline gives the [`Scene`] it painted to its mouse listeners, so a click hits exactly
//! what is on screen. Every change goes through the session: a drag is one gesture and one
//! undo step.

pub mod clipboard;
pub mod editor;
pub mod gesture;
pub mod layout;
pub mod master_panel;
mod paint;
pub mod roll;
pub mod selection;
pub mod snap;
mod timeline;
pub mod track_panel;

use gpui::{
    App, Context, Entity, FocusHandle, Focusable, FontWeight, KeyDownEvent, MouseButton,
    StyleRefinement, Subscription, Window, div, prelude::*, px,
};
use sound_core::{Instance, InstanceId, ProjectEvent};
use sound_notes::Clip;
use sound_ui::{ActiveTheme, KeyboardFocus, NoticeRoom, Session, Views};

use crate::{ArrangementState, TrackState};
use clipboard::SharedClipboard;
use editor::EditorEvent;
pub use editor::NoteEditor;
use layout::HEADER_WIDTH;
pub use master_panel::MasterPanel;
use master_panel::{MASTER_NAME, MasterPanelEvent};
use paint::PlayheadLine;
use roll::EDITOR_HEIGHT;
use snap::SharedSnap;
use timeline::scrolled_or_zoomed;
pub use timeline::{ClipShape, Scene, Timeline, TimelineEvent};
pub use track_panel::TrackPanel;
use track_panel::TrackPanelEvent;

/// Registers the view of the `arrangement` tool.
pub fn register(views: &mut Views) {
    views.register(ArrangementView::new);
}

/// The note editor while it is open, with its own playhead line.
struct OpenEditor {
    editor: Entity<NoteEditor>,
    playhead_line: Entity<PlayheadLine>,
    _events: Subscription,
}

struct OpenTrackPanel {
    panel: Entity<TrackPanel>,
    _events: Subscription,
}

struct OpenMasterPanel {
    panel: Entity<MasterPanel>,
    _events: Subscription,
}

/// What the panel below the timeline shows. One thing at a time: opening another takes its
/// place.
enum Detail {
    Editor(OpenEditor),
    Track(OpenTrackPanel),
    Master(OpenMasterPanel),
}

/// The height of the master row, pinned under the tracks.
pub const MASTER_ROW_HEIGHT: f32 = 40.;

pub struct ArrangementView {
    session: Entity<Session>,
    arrangement: Instance<ArrangementState>,
    timeline: Entity<Timeline>,
    playhead_line: Entity<PlayheadLine>,
    detail: Option<Detail>,
    /// The snap setting of the window, shared by the timeline and the note editor.
    snap: SharedSnap,
    /// The clipboard of the window, shared by the timeline and the note editor.
    clipboard: SharedClipboard,
    /// The master row is a tab stop after the timeline, and enter opens its panel.
    master_focus: FocusHandle,
    master_keyboard: KeyboardFocus,
}

impl ArrangementView {
    pub fn new(
        session: Entity<Session>,
        arrangement: Instance<ArrangementState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let playhead = session.read(cx).playhead().clone();
        let (snap, clipboard) = (SharedSnap::default(), SharedClipboard::default());
        let timeline = cx.new(|cx| {
            let shared = (snap.clone(), clipboard.clone());
            Timeline::new(session.clone(), arrangement.clone(), shared, cx)
        });
        let painted = timeline.read(cx).painted();
        let playhead_line = cx.new(|cx| PlayheadLine::new(playhead, &timeline, painted, cx));

        cx.subscribe_in(
            &timeline,
            window,
            |view, _, event, window, cx| match event {
                TimelineEvent::OpenEditor(clip) => view.open_editor(clip.clone(), window, cx),
                TimelineEvent::OpenTrack(track) => view.open_track_panel(track.clone(), window, cx),
            },
        )
        .detach();
        // The timeline is a cached view and works out its focus ring while it paints. The
        // master row next to it takes the focus without painting it, so the timeline is told
        // to paint again, or it would still think it had the focus when tab brings it back.
        let focus = timeline.focus_handle(cx);
        cx.on_focus_out(&focus, window, |view, _, _, cx| {
            view.timeline.update(cx, |_, cx| cx.notify());
        })
        .detach();
        // What is open follows the selection to another clip or another track.
        cx.observe_in(&timeline, window, |view, _, window, cx| {
            view.follow_selection(window, cx);
        })
        .detach();
        // A move to another track, and the undo of one, delete the clip at one id and create
        // it at another in one group of events. So the editor is not closed at the delete, but
        // after the group: the timeline has selected the clip at its new id by then, and the
        // editor goes with it. Only a clip that is really gone closes the editor. A track
        // keeps its id, so its panel closes with it at once.
        cx.subscribe_in(&session, window, |view, _, event, window, cx| {
            let ProjectEvent::Deleted(id) = event else {
                return;
            };
            if Some(id) == view.editor_clip(cx).as_ref() {
                cx.defer_in(window, |view, window, cx| {
                    let gone = view.editor_clip(cx).is_some_and(|clip| {
                        let project = view.session.read(cx).project();
                        project.resolve::<Clip>(&clip).is_none()
                    });
                    if gone && !view.follow_selection(window, cx) {
                        view.close_detail(window, cx);
                    }
                });
            }
            let shown = view.track_panel().map(|panel| panel.read(cx).track().id());
            if Some(id) == shown {
                view.close_detail(window, cx);
            }
        })
        .detach();
        // The notices of the window sit right of the track headers and above the panel below.
        // A view that is gone keeps no room.
        cx.on_release(|view, cx| {
            let room = NoticeRoom::default();
            view.session
                .update(cx, |session, cx| session.set_notice_room(room, cx));
        })
        .detach();
        let view = Self {
            session,
            arrangement,
            timeline,
            playhead_line,
            detail: None,
            snap,
            clipboard,
            master_focus: cx.focus_handle().tab_stop(true),
            master_keyboard: KeyboardFocus::default(),
        };
        view.publish_notice_room(cx);
        view
    }

    /// Tells the window where the notices go: right of the header column and above the panel
    /// below, whichever is open, so a notice never covers the mixer strip of a track.
    fn publish_notice_room(&self, cx: &mut Context<Self>) {
        let bottom = match &self.detail {
            Some(Detail::Editor(_)) => EDITOR_HEIGHT,
            Some(Detail::Track(_) | Detail::Master(_)) => track_panel::PANEL_HEIGHT,
            None => 0.,
        };
        let room = NoticeRoom {
            left: HEADER_WIDTH,
            bottom,
        };
        self.session
            .update(cx, |session, cx| session.set_notice_room(room, cx));
    }

    pub fn timeline(&self) -> &Entity<Timeline> {
        &self.timeline
    }

    /// The note editor, while it is open.
    pub fn editor(&self) -> Option<&Entity<NoteEditor>> {
        match &self.detail {
            Some(Detail::Editor(open)) => Some(&open.editor),
            _ => None,
        }
    }

    /// The track panel, while it is open.
    pub fn track_panel(&self) -> Option<&Entity<TrackPanel>> {
        match &self.detail {
            Some(Detail::Track(open)) => Some(&open.panel),
            _ => None,
        }
    }

    /// The master panel, while it is open.
    pub fn master_panel(&self) -> Option<&Entity<MasterPanel>> {
        match &self.detail {
            Some(Detail::Master(open)) => Some(&open.panel),
            _ => None,
        }
    }

    /// Opens the panel of the master. It takes the place of what the panel below showed.
    pub fn open_master_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.master_panel().is_some() {
            return;
        }
        self.close_detail(window, cx);
        let (session, arrangement) = (self.session.clone(), self.arrangement.clone());
        let panel = cx.new(|cx| MasterPanel::new(session, arrangement, cx));
        let events = cx.subscribe_in(&panel, window, |view, _, event, window, cx| {
            let MasterPanelEvent::Close = event;
            view.close_detail(window, cx);
        });
        self.detail = Some(Detail::Master(OpenMasterPanel {
            panel,
            _events: events,
        }));
        self.publish_notice_room(cx);
        cx.notify();
    }

    fn editor_clip(&self, cx: &App) -> Option<InstanceId> {
        let editor = self.editor()?.read(cx);
        Some(editor.clip().id().clone())
    }

    /// Opens the note editor for a clip and gives it the focus, so the keys edit notes. It
    /// takes the place of an open track panel.
    pub fn open_editor(
        &mut self,
        clip: Instance<Clip>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(editor) = self.editor() {
            editor.update(cx, |editor, cx| editor.set_clip(clip, cx));
        } else {
            self.close_detail(window, cx);
            let width = self.timeline.read(cx).painted_width();
            let (session, snap) = (self.session.clone(), self.snap.clone());
            let clipboard = self.clipboard.clone();
            let editor = cx.new(|cx| NoteEditor::new(session, clip, width, snap, clipboard, cx));
            let playhead = self.session.read(cx).playhead().clone();
            let painted = editor.read(cx).painted();
            let playhead_line = cx.new(|cx| PlayheadLine::new(playhead, &editor, painted, cx));
            let events = cx.subscribe_in(&editor, window, |view, _, event, window, cx| {
                let EditorEvent::Close = event;
                view.close_detail(window, cx);
            });
            self.detail = Some(Detail::Editor(OpenEditor {
                editor,
                playhead_line,
                _events: events,
            }));
            self.publish_notice_room(cx);
            cx.notify();
        }
        if let Some(editor) = self.editor() {
            window.focus(&editor.focus_handle(cx), cx);
        }
    }

    /// Opens the track panel for a track. It takes the place of an open note editor. The
    /// focus stays where it is: the timeline keeps the keys that pick another track, and tab
    /// goes into the panel.
    pub fn open_track_panel(
        &mut self,
        track: Instance<TrackState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(panel) = self.track_panel() {
            if panel.read(cx).track().id() != track.id() {
                panel.update(cx, |panel, cx| panel.set_track(track, window, cx));
            }
            return;
        }
        self.close_detail(window, cx);
        let session = self.session.clone();
        let panel = cx.new(|cx| TrackPanel::new(session, track, window, cx));
        let events = cx.subscribe_in(&panel, window, |view, _, event, window, cx| {
            let TrackPanelEvent::Close = event;
            view.close_detail(window, cx);
        });
        self.detail = Some(Detail::Track(OpenTrackPanel {
            panel,
            _events: events,
        }));
        self.publish_notice_room(cx);
        cx.notify();
    }

    /// Closes what the panel below the timeline shows. The focus goes back to the timeline
    /// when it was inside.
    pub fn close_detail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focus_handle = match self.detail.take() {
            Some(Detail::Editor(open)) => {
                // A note drag may be going on: its gesture ends here, not with the editor.
                open.editor.update(cx, |editor, cx| editor.end_drag(cx));
                open.editor.focus_handle(cx)
            }
            // A knob drag of a device ends when its view is released with the panel.
            Some(Detail::Track(open)) => open.panel.focus_handle(cx),
            Some(Detail::Master(open)) => open.panel.focus_handle(cx),
            None => return,
        };
        if focus_handle.contains_focused(window, cx) {
            window.focus(&self.timeline.focus_handle(cx), cx);
        }
        self.publish_notice_room(cx);
        cx.notify();
    }

    /// Shows the selected clip in the open editor, or the selected track in the open track
    /// panel. Whether there was one to show.
    fn follow_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let project = self.session.read(cx).project();
        let timeline = self.timeline.read(cx);
        match &self.detail {
            Some(Detail::Editor(open)) => {
                let Some(clip) = timeline.selected_instance(cx) else {
                    return false;
                };
                if open.editor.read(cx).clip().id() != clip.id() {
                    open.editor
                        .update(cx, |editor, cx| editor.set_clip(clip, cx));
                }
                true
            }
            Some(Detail::Track(_)) => {
                let selected = timeline.selected_track();
                let Some(track) = selected.and_then(|track| project.resolve(track)) else {
                    return false;
                };
                self.open_track_panel(track, window, cx);
                true
            }
            Some(Detail::Master(_)) | None => false,
        }
    }

    /// The master row: pinned under the tracks, with a ring where a track has its dot. A click,
    /// or enter when it has the focus, opens the panel of the master.
    fn master_row(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = cx.theme();
        let (hairline, ring, text, selected, focus) = (
            theme.alpha_at(0.05),
            theme.gray_800,
            theme.gray_900,
            theme.alpha_at(0.05),
            theme.lavender,
        );
        let open = self.master_panel().is_some();
        let keyboard_ring = self.master_keyboard.shows_ring(&self.master_focus, window);
        let header = div()
            .id("master-row")
            .debug_selector(|| "master-row".to_string())
            .track_focus(&self.master_focus)
            .relative()
            .flex_none()
            .w(px(HEADER_WIDTH))
            .h_full()
            .border_r_1()
            .border_color(hairline)
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| view.master_keyboard.pressed(cx)),
            )
            .on_click(cx.listener(|view, _, window, cx| view.open_master_panel(window, cx)))
            .child(
                // The fill of a selected track header, in the same place.
                div()
                    .absolute()
                    .left(px(8.))
                    .top(px(4.))
                    .w(px(HEADER_WIDTH - 16.))
                    .h(px(MASTER_ROW_HEIGHT - 8.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(match keyboard_ring {
                        true => focus,
                        false => gpui::transparent_black(),
                    })
                    .when(open, |fill| fill.bg(selected)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(24.))
                    .top(px(MASTER_ROW_HEIGHT / 2. - 4.))
                    .size(px(8.))
                    .rounded_full()
                    .border(px(1.5))
                    .border_color(ring),
            )
            .child(
                div()
                    .absolute()
                    .left(px(44.))
                    .top(px(MASTER_ROW_HEIGHT / 2. - 10.))
                    .line_height(px(20.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(text)
                    .child(MASTER_NAME),
            );
        div()
            .flex_none()
            .h(px(MASTER_ROW_HEIGHT))
            .flex()
            .border_t_1()
            .border_color(hairline)
            .child(header)
            .into_any_element()
    }
}

impl Render for ArrangementView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let timeline = self.timeline.clone();
        let fill_parent = || StyleRefinement::default().size_full();
        // Notes need the room and devices do not, so the two details have heights of their
        // own and a swap between them moves the lower edge of the timeline. Both are cached:
        // the playhead line above draws this view again on every frame.
        let detail = self.detail.as_ref().map(|detail| {
            // Named for tests: `note-editor`, `track-panel`.
            let panel = |name: &'static str, height: f32| {
                div()
                    .debug_selector(move || name.to_string())
                    .flex_none()
                    .h(px(height))
                    .relative()
            };
            match detail {
                Detail::Editor(open) => panel("note-editor", EDITOR_HEIGHT)
                    .child(open.editor.clone().cached(fill_parent()))
                    .child(open.playhead_line.clone()),
                Detail::Track(open) => {
                    let height = track_panel::PANEL_HEIGHT;
                    panel("track-panel", height).child(open.panel.clone().cached(fill_parent()))
                }
                Detail::Master(open) => {
                    let height = track_panel::PANEL_HEIGHT;
                    panel("master-panel", height).child(open.panel.clone().cached(fill_parent()))
                }
            }
        });
        let master_row = self.master_row(window, cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            // Escape closes the detail. It comes here from the timeline and from inside the
            // track panel when nothing there used it, as a knob does to cancel its drag. The
            // note editor handles its own.
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                let escape =
                    event.keystroke.key == "escape" && !event.keystroke.modifiers.modified();
                if escape && view.detail.is_some() {
                    view.close_detail(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    // Cached: a frame that only moves the playhead reuses what was painted.
                    .child(timeline.cached(fill_parent()))
                    .child(self.playhead_line.clone()),
            )
            .child(master_row)
            .children(detail)
    }
}
