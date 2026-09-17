use crate::{
    editing::{Edit, Selection},
    workspace::Workspace,
};
use gpui::{ClickEvent, Context, div, prelude::*, px};
use sound_daw::arrangement::BEAT;
use sound_runtime::session::Command;
use sound_ui::{
    ActiveTheme,
    components::button::{Button, ButtonSize},
};

const BEAT_WIDTH: f32 = 48.0;
const PAGE_BEATS: u64 = 128;
const HEADER_WIDTH: f32 = 320.0;
const ROW_HEIGHT: f32 = 112.0;

impl Workspace {
    pub(crate) fn timeline(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let first = self.first_beat;
        let last = first
            .saturating_add(PAGE_BEATS)
            .min((u64::MAX - BEAT) / BEAT);
        let width = (last - first) as f32 * BEAT_WIDTH;
        let origin = first * BEAT;
        let end = last * BEAT;
        let playhead =
            (self.snap.frame.saturating_sub(origin) as f64 / BEAT as f64) as f32 * BEAT_WIDTH;
        div().flex().flex_col().flex_1().min_w_0().min_h_0()
            .child(div().flex().flex_none().gap_2().p_2()
                .child(Button::new("previous-page", "Earlier bars").size(ButtonSize::Xs).disabled(first == 0)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.first_beat = this.first_beat.saturating_sub(PAGE_BEATS);
                        this.scroll.set_offset(gpui::point(px(0.), px(0.)));
                        cx.notify();
                    })))
                .child(Button::new("next-page", "Later bars").size(ButtonSize::Xs).disabled(last == (u64::MAX - BEAT) / BEAT)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.first_beat = last;
                        this.scroll.set_offset(gpui::point(px(0.), px(0.)));
                        cx.notify();
                    })))
                .child(Button::new("playhead-page", "Show playhead").size(ButtonSize::Xs)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.first_beat = (this.snap.frame / BEAT / PAGE_BEATS * PAGE_BEATS).min((u64::MAX - BEAT) / BEAT);
                        this.scroll.set_offset(gpui::point(px(0.), px(0.)));
                        cx.notify();
                    })))
                .child(format!("Bars {}–{} · 120 BPM · 4/4", first / 4 + 1, last.div_ceil(4))))
            .child(div().id("timeline-scroll").track_scroll(&self.scroll).overflow_scroll()
                .flex_1().min_h_0().min_w_0()
                .child(div().flex().flex_col().w(px(HEADER_WIDTH + width)).min_w(px(HEADER_WIDTH + width))
                    .child(div().flex().flex_none().h(px(32.))
                        .child(div().w(px(HEADER_WIDTH)).flex_none().px_2().child("Click a beat to seek"))
                        .children((first..last).map(|beat| {
                            div().id(("ruler-beat", beat)).flex_none().w(px(BEAT_WIDTH)).h_full()
                                .border_l_1().border_color(theme.alpha_at(if beat % 4 == 0 { 0.3 } else { 0.1 }))
                                .text_xs().cursor_pointer().hover(|style| style.bg(theme.alpha_at(0.1)))
                                .child(format!("{}.{}", beat / 4 + 1, beat % 4 + 1))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.transport(Command::Seek(beat * BEAT), cx);
                                }))
                        })))
                    .children(self.snap.arrangement.tracks.iter().enumerate().map(|(ti, track)| {
                        div().id(("track-row", ti)).flex().flex_none().h(px(ROW_HEIGHT))
                            .border_b_1().border_color(theme.alpha_at(0.1))
                            .child(div().flex().flex_col().gap_1().px_2().w(px(HEADER_WIDTH)).flex_none().overflow_hidden()
                                .child(div().truncate().child(track.name.clone()))
                                .child(div().flex().gap_1()
                                    .child(self.edit_button(("mute", ti), if track.muted { "Mute on" } else { "Mute" }, Edit::Mute(ti), cx))
                                    .child(self.edit_button(("solo", ti), if track.soloed { "Solo on" } else { "Solo" }, Edit::Solo(ti), cx))
                                    .child(self.edit_button(("add-clip", ti), "+ Clip", Edit::AddClip(ti), cx))
                                    .child(self.edit_button(("delete-track", ti), "Delete track", Edit::DeleteTrack(ti), cx)))
                                .child(div().flex().items_center().gap_1()
                                    .child(self.edit_button(("gain-down", ti), "-", Edit::Gain(ti, -0.1), cx))
                                    .child(format!("Gain {:.1}", track.gain))
                                    .child(self.edit_button(("gain-up", ti), "+", Edit::Gain(ti, 0.1), cx)))
                                .child(div().flex().items_center().gap_1()
                                    .child(self.edit_button(("pan-left", ti), "L", Edit::Pan(ti, -0.1), cx))
                                    .child(format!("Pan {:+.1}", track.pan))
                                    .child(self.edit_button(("pan-right", ti), "R", Edit::Pan(ti, 0.1), cx))))
                            .child(div().id(("lane", ti)).relative().w(px(width)).flex_none().h_full().overflow_hidden().bg(theme.alpha_at(0.03))
                                .children((first..last).map(|beat| div().absolute().top_0().left(px((beat - first) as f32 * BEAT_WIDTH))
                                    .w(px(1.)).h_full().bg(theme.alpha_at(if beat % 4 == 0 { 0.15 } else { 0.05 }))))
                                .children(track.clips.iter().enumerate().filter_map(|(ci, clip)| {
                                    let clip_end = clip.start.saturating_add(clip.length);
                                    if clip.start >= end || clip_end <= origin { return None; }
                                    let selection = Selection { track: ti, clip: ci };
                                    let selected = self.selection == Some(selection);
                                    let left = clip.start.max(origin) - origin;
                                    let length = clip_end.min(end) - clip.start.max(origin);
                                    Some(div().id(gpui::SharedString::from(format!("clip-{ti}-{ci}")))
                                        .absolute().top(px(12.)).left(px(left as f32 / BEAT as f32 * BEAT_WIDTH))
                                        .w(px((length as f32 / BEAT as f32 * BEAT_WIDTH).max(2.))).h(px(ROW_HEIGHT - 24.))
                                        .rounded(px(4.)).bg(if selected { theme.green } else { theme.lavender })
                                        .border_2().border_color(if selected { theme.gray_950 } else { theme.lavender })
                                        .overflow_hidden().text_color(theme.gray_100).cursor_pointer()
                                        .child(div().px_1().text_xs().child(format!("{} notes", clip.notes.len())))
                                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                            cx.stop_propagation();
                                            this.select(selection, cx);
                                        })))
                                }))
                                .when(self.snap.frame >= origin && self.snap.frame < end, |lane| lane.child(
                                    div().absolute().top_0().left(px(playhead)).w(px(2.)).h_full().bg(theme.red))))
                    }))
                    .when(self.snap.arrangement.tracks.is_empty(), |grid| grid.child(div().p_4().child("No tracks. Add a track to begin.")))))
    }
}
