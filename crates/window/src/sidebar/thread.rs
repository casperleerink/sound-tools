//! What the sidebar shows: the thread with the composer under it, the instructions, the
//! shape of a thread while the agents are looked for, or the way to Settings.

use std::time::SystemTime;

use rune::{
    Color, Cycle, El, GrowthSpec, Key, KeyEvent, ScrollSpec, TextEdit, Tx, col, el, icon, rich,
    row, text, text_input,
};
use rune_kit::{Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Markdown};
use sound_agent::{
    APPROVAL_ANSWERS, Below, Chat, Entry, FIX_THEM, Footer, Meta, NEW_THREAD, OPEN_SETTINGS,
    Shows, StepShown, TRY_AGAIN, TurnShown,
};
use sound_core::Project;

use super::{Act, INSTRUCTIONS, Sidebar, THREAD, composer, control, icons, settings};
use crate::app::State;
use crate::controls::Control;
use crate::theme::Theme;

/// Room around an entry: the thread reads as a column with air at its sides.
const SIDE: f32 = 24.0;
/// What a turn's answer fades in over, word by word, while it streams.
const WORD_FADE: f32 = 0.18;
/// The working line's band sweeps across it this often, in seconds.
const SWEEP: f64 = 2.2;
const SMALL_LINE: f32 = 16.0;

/// The Agent tab's body.
pub(super) fn body(state: &State, sidebar: &Sidebar) -> El {
    let theme = &state.theme;
    if sidebar.shows_settings {
        return col()
            .key("agent-settings")
            .grow(1.0)
            .min_h(0.0)
            .w_full()
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(settings::agents_settings(state, sidebar).padding(theme.space(4)));
    }
    let shown = match sidebar.chat.shows() {
        Shows::Instructions => instructions(state, sidebar),
        Shows::Looking => skeleton(theme),
        Shows::NoAgent(sentence) => no_agent(theme, sentence),
        Shows::Thread(footer) => {
            let entries = sidebar.chat.conversation().entries().len();
            let list = col()
                .key("agent-entries")
                .w_full()
                .growth(GrowthSpec::default())
                .virtual_rows(
                    entries,
                    120.0,
                    |_: &State, index| format!("agent-entry-{index}").into(),
                    |state: &State, index| {
                        super::sidebar(state).map_or_else(el, |sidebar| entry(state, sidebar, index))
                    },
                );
            let thread = col()
                .key(THREAD)
                .grow(1.0)
                .min_h(0.0)
                .w_full()
                .scroll(ScrollSpec { stick_bottom: true })
                .child(list);
            let footer = match footer {
                Footer::Composer => composer::view(state, sidebar),
                Footer::CannotContinue { say } => cannot_continue(theme, say),
            };
            col().grow(1.0).min_h(0.0).w_full().child(thread).child(footer)
        }
    };
    col().key("agent").grow(1.0).min_h(0.0).w_full().child(shown)
}

/// One entry of the thread, with its room around it.
fn entry(state: &State, sidebar: &Sidebar, index: usize) -> El {
    let theme = &state.theme;
    let chat = &sidebar.chat;
    let content = match chat.conversation().entries().get(index) {
        Some(Entry::Message { text, attachments }) => message(theme, text, attachments),
        Some(Entry::Notice(notice)) => text(notice.clone())
            .font(theme.text("label"))
            .color(theme.color("text-muted"))
            .wrap(),
        Some(Entry::Turn(_)) => match chat.turn(index, &state.project, SystemTime::now()) {
            Some(shown) => turn(state, index, shown),
            None => el(),
        },
        None => el(),
    };
    col()
        .w_full()
        .px(SIDE)
        .pb(SIDE)
        .pt(if index == 0 { 8.0 } else { 0.0 })
        .child(content)
}

/// The composer's message: a bubble at the right, with the names of the files it took.
fn message(theme: &Theme, words: &str, attachments: &[String]) -> El {
    let muted = theme.color("text-secondary");
    let attached = attachments.iter().map(|name| {
        row()
            .gap(6.0)
            .items_center()
            .child(icon(&icons::PAPERCLIP, 12.0).color(muted))
            .child(
                text(name.clone())
                    .font(theme.text("label"))
                    .color(muted)
                    .truncate()
                    .shrink(),
            )
    });
    let bubble = col()
        .max_w(282.0)
        .gap(theme.space(2))
        .padding(14.0)
        .radius(theme.kit.radius.lg)
        .fill(theme.color("fill"))
        .child(
            text(words.to_string())
                .font(theme.text("body"))
                .color(theme.color("text"))
                .wrap()
                .selectable(true),
        )
        .children(attached);
    row().w_full().justify_end().child(bubble)
}

fn turn(state: &State, index: usize, shown: TurnShown<'_>) -> El {
    let theme = &state.theme;
    let kit = &theme.kit;
    let TurnShown {
        meta,
        steps,
        answer,
        streaming,
        working,
        below,
    } = shown;
    let meta = meta.map(|meta| meta_line(theme, index, meta, steps.is_some()));
    let steps = steps.map(|steps| {
        col()
            .key(format!("agent-steps-{index}"))
            .children(steps.into_iter().map(|step| step_line(theme, step)))
    });
    let blocks = answer.len();
    let answer = answer.into_iter().enumerate().map(|(block, source)| {
        let last = streaming && block + 1 == blocks;
        let markdown = Markdown::new(source)
            .streaming(last)
            .selectable(true)
            .scroll_wide(true)
            // An answer is about music, not code: no colours in it.
            .highlight(|_, _| Vec::new())
            .on_link_tx(|_: &mut State, url: &str, tx: &mut Tx| {
                // The agent wrote the link: a `file:` or an app's link must not start anything.
                if sound_agent::link_opens(url)
                    && let Err(error) = tx.open_url(url)
                {
                    eprintln!("{url} did not open: {error}");
                }
            });
        let markdown = if streaming {
            markdown.reveal(WORD_FADE)
        } else {
            markdown
        };
        el().key(format!("agent-answer-{index}-{block}"))
            .w_full()
            .child(markdown.build(kit))
    });
    let working = working.map(|working| working_line(theme, index, working));
    let below = below.map(|below| below_line(theme, index, below));
    col()
        .key(format!("agent-turn-{index}"))
        .w_full()
        .gap(theme.space(3))
        .children(meta)
        .children(steps)
        .children(answer)
        .children(working)
        .children(below)
}

/// A quiet line that does `act`: muted, brighter under the pointer, a tab stop that Return and
/// Space press.
fn quiet(theme: &Theme, key: String, label: String, ink: Color, body: El, act: Act) -> El {
    row()
        .on_press_tx(move |s: &mut State, tx: &mut Tx| press(s, act, tx))
        .on_key(move |s: &mut State, key: &KeyEvent, tx: &mut Tx| {
            let enter = key.key == Key::Enter;
            if enter {
                press(s, act, tx);
            }
            enter
        })
        .key(key)
        .role("button")
        .label(label)
        .focusable(true)
        .hover_group()
        .gap(theme.space(1))
        .items_start()
        .px(4.0)
        .ml(-4.0)
        .radius(theme.kit.radius.sm)
        .child(body.color(ink).group_color(theme.color("text")))
}

/// "Worked for 12 s · Changed 1 file", which opens the steps; a failure in red.
fn meta_line(theme: &Theme, index: usize, meta: Meta, open: bool) -> El {
    let Meta {
        line,
        summary,
        failed,
        opens,
    } = meta;
    let ink = theme.color(if failed { "danger" } else { "text-muted" });
    let font = theme.text("label").lh(SMALL_LINE);
    let words = text(line.clone()).font(font).color(ink);
    let words = match &summary {
        Some(_) => words,
        None => words.wrap(),
    };
    let summary = summary.map(|summary| text(summary).font(font).color(ink).truncate().shrink());
    let content = row().gap(theme.space(1)).min_w(0.0).child(words).children(summary);
    if !opens {
        return row().key(format!("agent-worked-for-{index}")).child(content);
    }
    let chevron = if open {
        &icons::CHEVRON_DOWN
    } else {
        &icons::CHEVRON_RIGHT
    };
    let body = row()
        .gap(theme.space(1))
        .min_w(0.0)
        .child(icon(chevron, 12.0).color(ink).mt(2.0).group_color(theme.color("text")))
        .child(content);
    let act = Act::Steps(index);
    let label = line;
    row().child(
        quiet(theme, format!("agent-worked-for-{index}"), label, ink, body, act).expanded(open),
    )
}

/// A step: its icon and words, a failure or a denial said in words, and its command.
fn step_line(theme: &Theme, step: StepShown) -> El {
    let muted = theme.color("text-secondary");
    let font = theme.text("label").lh(SMALL_LINE);
    let mark = step.mark.map(|mark| {
        let ink = match mark {
            sound_agent::Mark::Failed => theme.color("danger"),
            sound_agent::Mark::Denied => muted,
        };
        text(mark.words()).font(font).color(ink)
    });
    let line = row()
        .h(20.0)
        .gap(6.0)
        .items_center()
        .child(icon(icons::named(step.icon), 12.0).color(theme.color("text-muted")))
        .child(
            row()
                .gap(theme.space(1))
                .min_w(0.0)
                .shrink()
                .child(text(step.words).font(font).color(muted).truncate().shrink())
                .children(mark),
        );
    let command = step.command.map(|command| {
        text(command)
            .font(theme.kit.ty.mono.lh(SMALL_LINE))
            .color(muted)
            .truncate()
            .pl(18.0)
            .pb(2.0)
    });
    col().child(line).children(command)
}

/// While the agent works: what it does, a band of light sweeping across it, and for how long.
fn working_line(theme: &Theme, index: usize, working: sound_agent::Working) -> El {
    let lavender = theme.color("focus");
    let mark = match working.icon {
        Some(name) => icon(icons::named(name), 14.0).color(lavender),
        None => el()
            .size(8.0, 8.0)
            .radius(4.0)
            .fill(lavender)
            .cycle(pulse())
            .a11y_hidden(),
    };
    row()
        .key(format!("agent-working-{index}"))
        .role("status")
        .label(working.words.clone())
        .gap(theme.space(2))
        .items_center()
        .child(el().size(14.0, 14.0).center().child(mark))
        .child(shimmer(theme, working.words))
        .child(
            text(working.elapsed)
                .font(theme.text("label"))
                .color(theme.color("text-muted")),
        )
}

/// A gentle breath, for what waits.
fn pulse() -> Cycle {
    Cycle {
        period: 1.6,
        phase: 0.0,
        stops: &[(0.0, 0.35), (0.5, 1.0), (1.0, 0.35)],
        still: 0.7,
        fps: 30.0,
    }
}

/// The words in lavender with a brighter band sweeping across them: a window slides over the
/// line while the bright copy inside it slides back, so the copy stays on the words.
fn shimmer(theme: &Theme, words: String) -> El {
    const BAND: f32 = 48.0;
    const WIDTH: f32 = 280.0;
    let font = theme.text("body");
    let slide = |from: f32, to: f32| {
        rune::LoopSpec::slide(SWEEP, [from, 0.0], [to, 0.0])
            .easing(rune::Easing::LINEAR)
    };
    let (from, to) = (-1.0, WIDTH / BAND);
    let lit = el()
        .absolute(0.0, 0.0)
        .w(WIDTH)
        .h_full()
        .looping(slide(-from, -to))
        .child(text(words.clone()).font(font).color(theme.color("text")).a11y_hidden());
    let band = el()
        .absolute(0.0, 0.0)
        .w(BAND)
        .h_full()
        .clip_fade(BAND / 2.0, 0.0, BAND / 2.0, 0.0)
        .looping(slide(from, to))
        .child(lit);
    row()
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .clip_soft(0.0)
        .child(text(words).font(font).color(theme.color("focus")).truncate())
        .child(band)
}

/// What goes under a turn: the question it waits on, the files it left not live, or the way
/// to send its message again.
fn below_line(theme: &Theme, index: usize, below: Below<'_>) -> El {
    match below {
        Below::Approval { words, command } => approval(theme, &words, command),
        Below::Problems { line, lines, fix } => {
            let open = lines.is_some();
            let peach = theme.color("peach");
            let font = theme.text("label").lh(SMALL_LINE);
            let chevron = if open {
                &icons::CHEVRON_DOWN
            } else {
                &icons::CHEVRON_RIGHT
            };
            let body = row()
                .gap(theme.space(1))
                .child(icon(chevron, 12.0).color(peach).mt(2.0).group_color(theme.color("text")))
                .child(text(line.clone()).font(font).color(peach).group_color(theme.color("text")));
            let act = Act::Problems(index);
            let toggle =
                quiet(theme, format!("agent-problems-{index}"), line, peach, body, act).expanded(open);
            let lines = lines.into_iter().flatten().map(|line| {
                text(line)
                    .font(font)
                    .color(theme.color("text-secondary"))
                    .wrap()
                    .pl(16.0)
            });
            let problems = col()
                .gap(theme.space(1))
                .child(row().child(toggle))
                .children(lines);
            let fix = fix.then(|| action(theme, "agent-fix", FIX_THEM, Act::Fix));
            col().gap(theme.space(2)).child(problems).children(fix)
        }
        Below::TryAgain => action(theme, "agent-try-again", TRY_AGAIN, Act::TryAgain),
    }
}

/// A quiet line with an icon that sends a message as if typed.
fn action(theme: &Theme, key: &str, (name, label): (&str, &str), act: Act) -> El {
    let muted = theme.color("text-muted");
    let body = row()
        .gap(theme.space(1))
        .items_center()
        .child(icon(icons::named(name), 12.0).color(muted).group_color(theme.color("text")))
        .child(
            text(label)
                .font(theme.text("label").lh(SMALL_LINE))
                .color(muted)
                .group_color(theme.color("text")),
        );
    row().child(
        quiet(theme, key.into(), label.into(), muted, body, act).items_center(),
    )
}

/// The question the agent waits on, as the last thing of the thread. Nothing is modal: music
/// and editing go on while it waits.
fn approval(theme: &Theme, words: &[sound_agent::Span], command: Option<&str>) -> El {
    let kit = &theme.kit;
    let lavender = theme.color("focus");
    let mut line = String::new();
    let mut spans = Vec::new();
    for span in words {
        let start = line.len();
        line.push_str(&span.text);
        if span.code {
            let mut code = rune::Span::new(start, line.len());
            (code.mono, code.bg, code.bg_pad) = (true, Some(theme.color("fill-strong")), 2.0);
            spans.push(code);
        }
    }
    let words = rich(line, spans)
        .font(theme.text("body"))
        .color(theme.color("text"))
        .wrap();
    let command = command.map(|command| {
        text(command.to_string())
            .font(kit.ty.mono)
            .color(theme.color("text"))
            .wrap_anywhere()
            .selectable(true)
            .w_full()
            .padding(theme.space(2))
            .radius(kit.radius.md)
            .fill(theme.color("well"))
    });
    let answers = APPROVAL_ANSWERS.iter().enumerate().map(|(place, (answer, label))| {
        let variant = match place {
            0 => ButtonVariant::Submit,
            1 => ButtonVariant::Neutral,
            _ => ButtonVariant::Ghost,
        };
        let act = Act::Answer(*answer);
        Button::new(*label)
            .variant(variant)
            .control_size(ControlSize::Large)
            .on_press_tx(move |s: &mut State, tx: &mut Tx| press(s, act, tx))
            .build(kit)
            .key(approval_key(place))
    });
    col()
        .key("agent-approval")
        .role("group")
        .gap(theme.space(3))
        .padding(14.0)
        .radius(kit.radius.lg)
        .fill(lavender.alpha(0.06))
        .border(1.0, lavender.alpha(0.3))
        .child(words)
        .children(command)
        .child(row().flex_wrap().gap(theme.space(2)).children(answers))
}

fn approval_key(place: usize) -> String {
    format!("agent-approval-{place}")
}

/// In place of the composer once the agent lost the thread's session.
fn cannot_continue(theme: &Theme, say: Option<&str>) -> El {
    let say = say.map(|say| {
        text(say)
            .font(theme.text("body"))
            .color(theme.color("text-muted"))
            .grow(1.0)
    });
    row()
        .w_full()
        .gap(theme.space(3))
        .items_center()
        .padding(SIDE)
        .pt(8.0)
        .children(say)
        .child(new_thread_button(theme).key("agent-new-thread-instead"))
}

fn new_thread_button(theme: &Theme) -> El {
    Button::new(NEW_THREAD)
        .icon(&icons::PLUS)
        .variant(ButtonVariant::Neutral)
        .control_size(ControlSize::Large)
        .on_press_tx(|s: &mut State, tx: &mut Tx| press(s, Act::NewThread, tx))
        .build(&theme.kit)
}

/// No agent is signed in: why, and the way to Settings, where agents are set up.
fn no_agent(theme: &Theme, sentence: String) -> El {
    let open = Button::new(OPEN_SETTINGS)
        .variant(ButtonVariant::Submit)
        .control_size(ControlSize::ExtraLarge)
        .on_press_tx(|s: &mut State, tx: &mut Tx| press(s, Act::OpenSettings, tx))
        .build(&theme.kit)
        .key("agent-open-settings")
        .w_full();
    col()
        .key("agent-no-agent")
        .grow(1.0)
        .w_full()
        .center()
        .gap(theme.space(4))
        .px(40.0)
        .child(
            text(sentence)
                .font(theme.text("body"))
                .color(theme.color("text"))
                .wrap()
                .align(rune::Align::Center),
        )
        .child(el().w_full().max_w(280.0).child(open))
}

/// While the agents are looked for: the thread and the composer as grey shapes of their size,
/// breathing, so nothing jumps once they are found.
fn skeleton(theme: &Theme) -> El {
    let shape = |w: f32, h: f32| {
        el().w(w)
            .h(h)
            .radius(theme.kit.radius.md)
            .fill(theme.color("fill"))
    };
    let line = |w: f32| shape(w, 12.0);
    let bubble = row().w_full().justify_end().child(shape(220.0, 44.0));
    let answer = col()
        .gap(theme.space(2))
        .child(line(120.0))
        .child(line(330.0))
        .child(line(300.0))
        .child(line(210.0));
    let thread = col()
        .grow(1.0)
        .gap(SIDE)
        .px(SIDE)
        .pt(8.0)
        .child(bubble)
        .child(answer);
    let composer = el().px(12.0).pb(12.0).pt(8.0).child(
        el().w_full()
            .h(106.0)
            .radius(theme.kit.radius.lg)
            .border(1.0, theme.color("border")),
    );
    col()
        .key("agent-looking")
        .role("status")
        .label(sound_agent::LOOKING_FOR_AGENTS)
        .grow(1.0)
        .w_full()
        .cycle(pulse())
        .child(thread)
        .child(composer)
}

/// The instructions in place of the thread: their title with the way back, what they are
/// for, and a box as large as the panel to write them in.
fn instructions(state: &State, sidebar: &Sidebar) -> El {
    let theme = &state.theme;
    let kit = &theme.kit;
    let Some(shown) = sidebar.chat.instructions() else {
        return el();
    };
    let scope = shown.scope();
    let back = IconButton::new(&icons::ARROW_LEFT)
        .variant(IconButtonVariant::Ghost)
        .control_size(ControlSize::Large)
        .label("Back to the thread")
        .on_press_tx(|s: &mut State, tx: &mut Tx| press(s, Act::CloseInstructions, tx))
        .build(kit)
        .key("agent-instructions-back");
    let header = row()
        .h(40.0)
        .px(theme.space(3))
        .gap(theme.space(1))
        .items_center()
        .child(back)
        .child(
            text(sound_agent::INSTRUCTIONS)
                .font(theme.text("heading"))
                .color(theme.color("text")),
        );
    let field = match &sidebar.instructions {
        Some(edit) => text_input(edit, theme.text("body"))
            .key(INSTRUCTIONS)
            .label(format!("{}: {}", sound_agent::INSTRUCTIONS, scope.label()))
            .w_full()
            .color(theme.color("text"))
            .placeholder(scope.placeholder(), theme.color("text-muted"))
            .caret(theme.color("text"), kit.selection())
            .on_edit(instructions_edit)
            .on_input_change(|s: &mut State, _: &mut Tx| {
                if let Some(sidebar) = s.panel.agent_mut::<Sidebar>()
                    && let Some(edit) = &sidebar.instructions
                {
                    let typed = edit.text.clone();
                    sidebar.chat.edit_instructions(&typed);
                }
            })
            // Return adds a line: the instructions are a page, not a message.
            .on_submit(|s: &mut State, _: &mut Tx| instructions_edit(s).insert("\n"))
            .on_input_key(|s: &mut State, key: &KeyEvent, tx: &mut Tx| {
                let close = key.key == Key::Escape;
                if close {
                    press(s, Act::CloseInstructions, tx);
                }
                close
            }),
        // Read in a moment.
        None => el(),
    };
    let page = col()
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .padding(14.0)
        .radius(kit.radius.lg)
        .border(1.0, theme.color("border-strong"))
        .fill(theme.color("surface"))
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(field);
    col()
        .key("agent-instructions-page")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .child(header)
        .child(
            col()
                .grow(1.0)
                .min_h(0.0)
                .gap(theme.space(3))
                .px(theme.space(3))
                .pb(theme.space(3))
                .child(
                    text(scope.explanation())
                        .font(theme.text("label"))
                        .color(theme.color("text-secondary"))
                        .wrap()
                        .px(theme.space(1)),
                )
                .child(page),
        )
}

fn instructions_edit(s: &mut State) -> &mut TextEdit {
    match s.panel.agent_mut::<Sidebar>().and_then(|sidebar| sidebar.instructions.as_mut()) {
        Some(edit) => edit,
        // Only the sidebar draws the field, and only while the instructions show.
        None => Box::leak(Box::default()),
    }
}

/// A control of the sidebar pressed by a pointer or a key.
pub(super) fn press(s: &mut State, act: Act, tx: &mut Tx) {
    act.run(s, Some(tx));
    s.handled();
}

/// What the working line of the last turn says of the time, while the agent works.
pub(super) fn working_elapsed(chat: &Chat, project: &Project) -> Option<String> {
    if !chat.is_working() {
        return None;
    }
    let last = chat.conversation().entries().len().checked_sub(1)?;
    Some(chat.turn(last, project, SystemTime::now())?.working?.elapsed)
}

/// The controls of the thread for `--tree` and `--act`, and the one in place of the composer.
pub(super) fn controls(state: &State, sidebar: &Sidebar) -> Vec<Control> {
    let chat = &sidebar.chat;
    let mut controls = Vec::new();
    match chat.shows() {
        Shows::NoAgent(_) => controls.push(control(
            "agent-open-settings",
            OPEN_SETTINGS,
            "button",
            Act::OpenSettings,
        )),
        Shows::Instructions => controls.push(control(
            "agent-instructions-back",
            "Back to the thread",
            "button",
            Act::CloseInstructions,
        )),
        Shows::Looking => {}
        Shows::Thread(footer) => {
            let now = SystemTime::now();
            for index in 0..chat.conversation().entries().len() {
                let Some(shown) = chat.turn(index, &state.project, now) else {
                    continue;
                };
                if let Some(meta) = shown.meta.filter(|meta| meta.opens) {
                    controls.push(Control {
                        expanded: Some(shown.steps.is_some()),
                        ..control(
                            format!("agent-worked-for-{index}"),
                            meta.line,
                            "button",
                            Act::Steps(index),
                        )
                    });
                }
                match shown.below {
                    Some(Below::Approval { .. }) => {
                        for (place, (answer, label)) in APPROVAL_ANSWERS.iter().enumerate() {
                            controls.push(control(
                                approval_key(place),
                                *label,
                                "button",
                                Act::Answer(*answer),
                            ));
                        }
                    }
                    Some(Below::Problems { line, lines, fix }) => {
                        controls.push(Control {
                            expanded: Some(lines.is_some()),
                            ..control(
                                format!("agent-problems-{index}"),
                                line,
                                "button",
                                Act::Problems(index),
                            )
                        });
                        if fix {
                            controls.push(control("agent-fix", FIX_THEM.1, "button", Act::Fix));
                        }
                    }
                    Some(Below::TryAgain) => controls.push(control(
                        "agent-try-again",
                        TRY_AGAIN.1,
                        "button",
                        Act::TryAgain,
                    )),
                    None => {}
                }
            }
            if let Footer::CannotContinue { .. } = footer {
                controls.push(control(
                    "agent-new-thread-instead",
                    NEW_THREAD,
                    "button",
                    Act::NewThread,
                ));
            }
        }
    }
    controls
}
