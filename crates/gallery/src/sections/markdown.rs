//! Markdown section: an agent's answer with every kind of block, and the same answer cut off
//! while it streams, in a column as wide as the text of the sidebar.

use gpui::{App, FontWeight, IntoElement, ParentElement, Styled, Window, div, px};
use sound_ui::ActiveTheme;
use sound_ui::components::markdown::{Markdown, MarkdownText};

const ANSWER: &str = r#"## Bass filter

The bass now has its own **filter** with a *slow* attack, set in `bass/state.json`. The [effect docs](https://example.com/effects) list every parameter.

### What I did

1. Opened the bass track
2. Added a filter after the synth
   - cutoff at 800 Hz
   - resonance at 0.3
3. Lowered the gain by **3 dB**

- Bullets wrap with a hanging indent, so a long line stays clear of the marker.

> The kick still masks the bass below 60 Hz. A high-pass on the bass would help.

```json
{ "type": "filter", "cutoff": 800, "resonance": 0.3 }
```

| Bars | Section | What plays |
| --- | --- | --- |
| 1–32 | **A** | Intro, then the theme on the lead |
| 33–38 | **B1**, a breath | New chords, the bass drops out |
| 39–44 | **B2** | The pad comes back, set in `pad/state.json` |

| Track | Gain |
| --- | ---: |
| Bass | -3 dB |
| Lead | 0 dB |

---

![a waveform](wave.png) and <kbd>html</kbd> show as text."#;

/// Where a streaming answer might be cut: inside a list, a half-written bold and an open fence.
const STREAMING: &str = r#"## Bass filter

The bass now has its own **filter** with a *slow* attack.

1. Opened the bass track
2. Added a **filter aft

```json
{ "type": "filter", "cut"#;

pub fn section(_window: &mut Window, cx: &mut App) -> impl IntoElement {
    div()
        .flex()
        .items_start()
        .gap(px(40.))
        .child(sample("Markdown / answer", ANSWER, cx))
        .child(sample("Markdown / streaming", STREAMING, cx))
}

/// A heading, then the text on the background of the sidebar, 360 wide with its padding.
fn sample(title: &'static str, source: &str, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let (muted, background, border) = (theme.gray_700, theme.gray_50, theme.alpha_at(0.10));
    div()
        .flex()
        .flex_col()
        .gap(px(12.))
        .child(
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(muted)
                .child(title),
        )
        .child(
            div()
                .w(px(360.))
                .p(px(24.))
                .bg(background)
                .border_1()
                .border_color(border)
                .child(MarkdownText::new(title, Markdown::parse(source))),
        )
}
