//! The Settings page, over the whole window: its sections on the left, the one picked on the
//! right. Agents is what the installed [`SettingsSlot`] makes; Audio is the window's own, the
//! output and the input device.
//!
//! A device picked here is kept for this machine, never in the project, and the app starts
//! again on it: the engine runs at the rate of its output device.

use std::sync::Arc;

use gpui::{
    AnyElement, AnyView, Context, Entity, FocusHandle, FontWeight, SharedString, Task, Window, div,
    prelude::*, px,
};
use sound_core::DeviceChoice;
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::icon::Icon;
use sound_ui::focus::TabStop;
use sound_ui::{ActiveTheme, Session};

#[cfg(doc)]
use super::SettingsSlot;
use super::project_menu::switch_devices;
use super::{CloseSettings, LEFT_PANEL_WIDTH, SETTINGS_CONTEXT, Shell, TOP_ROW_HEIGHT};
use crate::app::ChosenDevices;

/// The audio devices of this machine, and which the composer chose.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceList {
    pub outputs: Vec<DeviceChoice>,
    pub inputs: Vec<DeviceChoice>,
    pub chosen: ChosenDevices,
}

/// How the window lists the audio devices of this machine, or simulated ones. It is called on a
/// background thread: asking can take a while.
pub type ListDevices = Arc<dyn Fn() -> DeviceList + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Agents,
    Audio,
}

impl Section {
    fn name(self) -> &'static str {
        match self {
            Self::Agents => "Agents",
            Self::Audio => "Audio",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Agents => "bot",
            Self::Audio => "volume-2",
        }
    }
}

/// The value of the row for the default device of the system. A device's id never is empty.
const SYSTEM_DEFAULT: &str = "";

pub struct SettingsPage {
    session: Entity<Session>,
    section: Section,
    /// The Agents section, when the app has one.
    agents: Option<AnyView>,
    /// The output in use, which is another than the chosen one when that is not there.
    output_name: SharedString,
    /// The devices to pick from, once they are listed. A test window has none.
    devices: Option<DeviceList>,
    _listing: Task<()>,
    focus: FocusHandle,
    back: FocusHandle,
}

impl SettingsPage {
    /// Lists the devices each time the page opens, so an interface plugged in since is offered.
    /// Never on a timer: listing can take a while, and on Linux it opens each device.
    pub(super) fn new(
        session: Entity<Session>,
        agents: Option<AnyView>,
        output_name: SharedString,
        list_devices: Option<ListDevices>,
        cx: &mut Context<Self>,
    ) -> Self {
        let listing = list_devices.map_or(Task::ready(()), |list| {
            let listed = cx.background_spawn(async move { list() });
            cx.spawn(async move |page, cx| {
                let devices = listed.await;
                page.update(cx, |page, cx| page.set_devices(devices, cx))
                    .ok();
            })
        });
        Self {
            session,
            section: if agents.is_some() {
                Section::Agents
            } else {
                Section::Audio
            },
            agents,
            output_name,
            devices: None,
            _listing: listing,
            focus: cx.focus_handle(),
            back: cx.focus_handle().tab_stop(true),
        }
    }

    /// The way back, which has the keys when the page opens.
    pub(super) fn back(&self) -> &FocusHandle {
        &self.back
    }

    pub fn section(&self) -> Section {
        self.section
    }

    pub fn show(&mut self, section: Section, cx: &mut Context<Self>) {
        self.section = section;
        cx.notify();
    }

    pub fn devices(&self) -> Option<&DeviceList> {
        self.devices.as_ref()
    }

    /// The devices to pick from, as listed, or as a snapshot shows them.
    pub fn set_devices(&mut self, devices: DeviceList, cx: &mut Context<Self>) {
        self.devices = Some(devices);
        cx.notify();
    }

    fn pick(&mut self, output: bool, value: &str, cx: &mut Context<Self>) {
        let Some(devices) = &self.devices else {
            return;
        };
        let id = (value != SYSTEM_DEFAULT).then(|| value.to_string());
        let mut chosen = devices.chosen.clone();
        match output {
            true => chosen.output = id,
            false => chosen.input = id,
        }
        if chosen != devices.chosen {
            self.session.update(cx, |_, cx| switch_devices(chosen, cx));
        }
    }

    fn sections(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().gray_700;
        let back = Button::new("settings-back", "Back")
            .icon("arrow-left")
            .debug_selector(|| "settings-back".to_string())
            .variant(ButtonVariant::GhostColor(muted))
            .size(ButtonSize::Sm)
            .focus_handle(&self.back)
            .on_click(|_, window, cx| window.dispatch_action(Box::new(CloseSettings), cx));
        let (strong, tint) = (cx.theme().gray_950, cx.theme().alpha_at(0.05));
        let chevron = cx.theme().gray_500;
        let shown = self.agents.is_some().then_some(Section::Agents);
        let items = (shown.into_iter().chain([Section::Audio])).map(|section| {
            let selected = section == self.section;
            let name = section.name();
            let row = div()
                .id(name)
                .debug_selector(move || format!("settings-{}", name.to_lowercase()))
                .flex()
                .items_center()
                .gap(px(10.))
                .w_full()
                .h(px(32.))
                .px(px(10.))
                .rounded(px(8.))
                .text_size(px(14.))
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .map(|row| match selected {
                    true => row.bg(tint).text_color(strong),
                    false => row.text_color(muted).hover(move |row| row.bg(tint)),
                })
                .child(Icon::new(section.icon()))
                .child(div().flex_1().child(name))
                .child(Icon::new("chevron-right").size(14.).color(chevron));
            TabStop::new(format!("settings-tab-{name}"), row)
                .on_click(cx.listener(move |page, _, _, cx| page.show(section, cx)))
        });
        div()
            .flex()
            .flex_none()
            .flex_col()
            .w(px(LEFT_PANEL_WIDTH))
            .h_full()
            .border_r_1()
            .border_color(cx.theme().alpha_at(0.05))
            .child(Shell::drag_region().flex_none().h(px(TOP_ROW_HEIGHT)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .px(px(12.))
                    .child(div().flex().pb(px(14.)).child(back))
                    .children(items),
            )
            .child(Shell::drag_region().flex_1())
    }

    fn audio(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (strong, text, muted, faint, accent, hole, live) = (
            theme.gray_950,
            theme.gray_900,
            theme.gray_700,
            theme.gray_500,
            theme.lavender,
            theme.gray_50,
            theme.green,
        );
        let border = theme.alpha_at(0.08);
        let intro = div()
            .text_color(muted)
            .child("System default follows your computer. Picking a device starts the app again.");
        let Some(devices) = &self.devices else {
            return div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(intro)
                .child(div().text_color(muted).child("Looking for devices…"))
                .into_any_element();
        };
        let page = cx.weak_entity();
        let list = |output: bool| {
            let (key, choices, chosen) = match output {
                true => ("output", &devices.outputs, &devices.chosen.output),
                false => ("input", &devices.inputs, &devices.chosen.input),
            };
            let chosen = chosen.as_deref().unwrap_or(SYSTEM_DEFAULT);
            let each = (choices.iter()).map(|choice| (choice.id.as_str(), choice.name.as_str()));
            let all: Vec<_> = std::iter::once((SYSTEM_DEFAULT, "System default"))
                .chain(each)
                .collect();
            let last = all.len() - 1;
            let rows = all.into_iter().enumerate().map(|(index, (id, name))| {
                let page = page.clone();
                let value = id.to_string();
                let picked = id == chosen;
                // Only the output in use is known: the engine reports no input.
                let in_use = output && id != SYSTEM_DEFAULT && *name == *self.output_name;
                let dot = div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(px(16.))
                    .rounded_full()
                    .border(px(1.5))
                    .map(|dot| match picked {
                        true => (dot.border_color(accent).bg(accent))
                            .child(div().size(px(6.)).rounded_full().bg(hole)),
                        false => (dot.border_color(faint))
                            .group_hover("device", |dot| dot.border_color(muted)),
                    });
                let row = div()
                    .id(SharedString::from(format!("settings-{key}-{id}")))
                    .group("device")
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .h(px(48.))
                    .cursor_pointer()
                    .when(index < last, |row| row.border_b_1().border_color(border))
                    .child(dot)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(if picked { strong } else { text })
                            .child(name.to_string()),
                    )
                    .when(in_use, |row| {
                        row.child(
                            div()
                                .flex()
                                .flex_none()
                                .items_center()
                                .gap(px(6.))
                                .text_size(px(12.))
                                .text_color(muted)
                                .child(div().size(px(6.)).rounded_full().bg(live))
                                .child("In use"),
                        )
                    });
                TabStop::new(format!("settings-{key}-row-{id}"), row).on_click(move |_, _, cx| {
                    page.update(cx, |page, cx| page.pick(output, &value, cx))
                        .ok();
                })
            });
            div()
                .id(key)
                .flex()
                .flex_col()
                .max_h(px(352.))
                .overflow_y_scroll()
                .children(rows)
        };
        let card = |icon: &'static str, title: &'static str, output: bool| {
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .px(px(20.))
                .pt(px(20.))
                .pb(px(8.))
                .rounded(px(12.))
                .border_1()
                .border_color(border)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(Icon::new(icon).size(16.).color(strong))
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(strong)
                                .child(title),
                        ),
                )
                .child(list(output))
        };
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(intro)
            .child(card("volume-2", "Output", true))
            .child(card("mic", "Input", false))
            .into_any_element()
    }
}

impl Render for SettingsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match (self.section, &self.agents) {
            (Section::Agents, Some(agents)) => agents.clone().into_any_element(),
            _ => self.audio(cx),
        };
        div()
            .id("settings")
            .key_context(SETTINGS_CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .child(self.sections(cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .h(px(TOP_ROW_HEIGHT))
                            .child(Shell::drag_region().flex_1())
                            .children(Shell::window_controls(window)),
                    )
                    .child(
                        div()
                            .id("settings-section")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(
                                div()
                                    .mx_auto()
                                    .max_w(px(640.))
                                    .px(px(40.))
                                    .pb(px(32.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(24.))
                                    .child(
                                        div()
                                            .text_size(px(20.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(cx.theme().gray_950)
                                            .child(self.section.name()),
                                    )
                                    .child(body),
                            ),
                    ),
            )
    }
}
