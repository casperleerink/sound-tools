//! Agent sidebar: header, conversation, sticky composer. 360 x 800.
//!
//! A turn is the composer's message and the agent's result text. Nothing else is shown: no
//! avatars, no timestamps, no tool rows. The four states are `Idle` (a finished turn),
//! `Working` (one pulsing line), `Done` (a muted `Worked for 12 s` line that expands to the
//! history) and `Failed` (the same line in red plus one sentence).
//!
//! Enter sends: it appends the message and switches to `Working`. Shift-enter would insert a
//! newline, but `TextInput` is single-line (only the box grows with `.lines(n)`), so the binding
//! is left out until real multi-line editing exists.

use gpui::{
    BoxShadow, Context, Entity, FontWeight, IntoElement, ParentElement, SharedString, Styled,
    Subscription, Window, div, hsla, point, prelude::*, px,
};
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::dropdown_menu::{DropdownMenu, MenuEntry, MenuGroup, MenuItem};
use sound_ui::components::indicator::{Indicator, IndicatorSize};
use sound_ui::components::popover::Align;
use sound_ui::components::text_input::TextInput;
use sound_ui::ActiveTheme;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AgentState {
    #[default]
    Idle,
    Working,
    Done,
    Failed,
}

impl AgentState {
    pub fn from_value(value: &str) -> Self {
        match value {
            "working" => Self::Working,
            "done" => Self::Done,
            "failed" => Self::Failed,
            _ => Self::Idle,
        }
    }

    pub fn value(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }
}

const FIRST_MESSAGE: &str =
    "Give these three voices independent rhythms and let me stretch each pattern by dragging it.";
const RESULT: &str =
    "Done. Each voice has its own length now: A 2.0 s, B 3.0 s, C 2.4 s. Drag a bar's right edge to stretch it.";
const FAILURE: &str = "Polyrhythm does not compile: the pattern length wants a Duration.";
const HISTORY: [&str; 4] = [
    "Read arrangement.json",
    "Edited polyrhythm/src/lib.rs",
    "Built",
    "Reloaded",
];

fn model_entries() -> Vec<MenuEntry> {
    vec![
        MenuEntry::Group(MenuGroup::new().label("Model").items([
            MenuItem::new("opus-5", "Opus 5").description("Deepest reasoning"),
            MenuItem::new("sonnet-4", "Sonnet 4.6").description("Balanced default"),
            MenuItem::new("haiku-3", "Haiku 3.5").description("Fast and cheap"),
            MenuItem::new("local-7b", "Local 7B").description("Runs on this machine"),
        ])),
        MenuEntry::Separator,
        MenuEntry::Group(MenuGroup::new().label("Effort").items([
            MenuItem::new("low", "low"),
            MenuItem::new("medium", "medium"),
            MenuItem::new("high", "high"),
        ])),
    ]
}

pub struct AgentSidebar {
    state: AgentState,
    messages: Vec<SharedString>,
    history_open: bool,
    input: Entity<TextInput>,
    model: Entity<DropdownMenu>,
    _input: Subscription,
}

impl AgentSidebar {
    pub fn new(state: AgentState, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextInput::new(cx)
                .placeholder("Ask for a change")
                .lines(2)
                .bare(true)
        });
        let weak = cx.weak_entity();
        input.update(cx, |input, _| {
            input.set_on_submit(move |text, _window, cx| {
                let text = text.trim().to_string();
                if text.is_empty() {
                    return;
                }
                let weak = weak.clone();
                // The submit handler runs inside the input's own update, so clearing it has to
                // wait until that update has finished.
                cx.defer(move |cx| {
                    weak.update(cx, |this, cx| this.send(text.into(), cx)).ok();
                });
            });
        });
        let subscription = cx.observe(&input, |_, _, cx| cx.notify());
        let model = cx.new(|cx| {
            DropdownMenu::new("Opus 5 · high", model_entries(), cx)
                .selected("opus-5")
                .ghost(true)
                .align(Align::Start)
                .width(280.)
        });

        Self {
            state,
            messages: vec![FIRST_MESSAGE.into()],
            history_open: false,
            input,
            model,
            _input: subscription,
        }
    }

    pub fn state(&self) -> AgentState {
        self.state
    }

    pub fn set_state(&mut self, state: AgentState, cx: &mut Context<Self>) {
        self.state = state;
        self.history_open = false;
        cx.notify();
    }

    pub fn open_model(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.model.update(cx, |menu, cx| menu.open(window, cx));
    }

    fn send(&mut self, text: SharedString, cx: &mut Context<Self>) {
        self.messages.push(text);
        self.state = AgentState::Working;
        self.history_open = false;
        self.input.update(cx, |input, cx| input.set_text("", cx));
        cx.notify();
    }

    fn send_from_button(&mut self, cx: &mut Context<Self>) {
        let text = self.input.read(cx).text().trim().to_string();
        if !text.is_empty() {
            self.send(text.into(), cx);
        }
    }

    fn header(&self) -> impl IntoElement {
        div()
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .h(px(56.))
            .child(
                div()
                    .text_size(px(14.))
                    .font_weight(FontWeight::MEDIUM)
                    .child("Agent"),
            )
            .child(
                div().absolute().right(px(16.)).child(
                    Button::icon_only("new-session", "plus")
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Xs)
                        .on_click(|_, _, _| {}),
                ),
            )
    }

    /// The muted meta line above the agent's text; clicking it shows the history.
    fn worked_line(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, bright, dim) = (theme.gray_700, theme.gray_900, theme.gray_600);
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .id("worked-for")
                    .w_full()
                    .text_size(px(12.))
                    .text_color(muted)
                    .cursor_pointer()
                    .hover(move |s| s.text_color(bright))
                    .child("Worked for 12 s")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.history_open = !this.history_open;
                        cx.notify();
                    })),
            )
            .when(self.history_open, |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .text_size(px(12.))
                        .text_color(dim)
                        .children(HISTORY.map(|line| div().child(line))),
                )
            })
    }

    fn turn(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (lavender, red, text) = (theme.lavender, theme.red, theme.gray_950);
        let body = |content: &'static str, color| {
            div()
                .text_size(px(15.))
                .line_height(px(22.))
                .text_color(color)
                .child(content)
        };

        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .map(|d| match self.state {
                AgentState::Idle => d.child(body(RESULT, text)),
                AgentState::Working => d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            Indicator::new("agent-working")
                                .size(IndicatorSize::Sm)
                                .color(lavender)
                                .pulse(true),
                        )
                        .child(
                            div()
                                .text_size(px(15.))
                                .line_height(px(22.))
                                .text_color(lavender)
                                .child("Building Polyrhythm"),
                        ),
                ),
                AgentState::Done => d
                    .child(self.worked_line(cx))
                    .child(body(RESULT, text)),
                AgentState::Failed => d
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(red)
                            .child("Build failed"),
                    )
                    .child(body(FAILURE, text)),
            })
    }

    fn composer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (border, fill) = (theme.alpha_at(0.10), theme.gray_50);
        let can_send = !self.input.read(cx).text().trim().is_empty();

        div()
            .flex_none()
            .p(px(24.))
            .pt(px(8.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .p(px(16.))
                    .rounded(px(16.))
                    .border_1()
                    .border_color(border)
                    .bg(fill)
                    .shadow(vec![BoxShadow {
                        color: hsla(0., 0., 0., 0.25),
                        offset: point(px(0.), px(8.)),
                        blur_radius: px(24.),
                        spread_radius: px(-8.),
                    }])
                    .child(self.input.clone())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(self.model.clone())
                            .child(
                                Button::icon_only("send", "arrow-up")
                                    .variant(ButtonVariant::Subtle)
                                    .size(ButtonSize::Sm)
                                    .rounded(true)
                                    .disabled(!can_send)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.send_from_button(cx)
                                    })),
                            ),
                    ),
            )
    }
}

impl Render for AgentSidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, border, text, surface) = (
            theme.gray_50,
            theme.alpha_at(0.10),
            theme.gray_950,
            theme.alpha_at(0.05),
        );
        let bubbles: Vec<_> = self
            .messages
            .iter()
            .map(|message| {
                div()
                    .p(px(14.))
                    .rounded(px(12.))
                    .bg(surface)
                    .text_size(px(15.))
                    .line_height(px(22.))
                    .child(message.clone())
            })
            .collect();

        div()
            .w(px(360.))
            .h(px(800.))
            .flex()
            .flex_col()
            .flex_none()
            .bg(bg)
            .border_r_1()
            .border_color(border)
            .text_color(text)
            .child(self.header())
            .child(
                div()
                    .id("agent-conversation")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(24.))
                    .px(px(24.))
                    .py(px(8.))
                    .children(bubbles)
                    .child(self.turn(cx)),
            )
            .child(self.composer(cx))
    }
}
