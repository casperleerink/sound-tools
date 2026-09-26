//! The card of a hosted plugin in a rack.
//!
//! A plugin draws its own interface in a window of its own, so this card is small, 200 pt: a
//! control that opens that window and closes it again at the top of the body, and the format
//! and the maker on the value line of the second row, `CLAP · <maker>`. There is no generic
//! parameter view: what a plugin's knobs are is the plugin's business. The rack gives the
//! frame of the card, whose title is the name of the plugin and where another one is picked.
//! Nothing is hidden, so there is no expand, and there is no power: the rack has no bypass yet.
//!
//! A plugin this machine does not have shows what is wrong and the id the record names, so a
//! composer can see which plugin to install and an agent can be asked to correct the record.
//! The record itself is untouched, as everywhere else.

use gpui::{Context, Div, Entity, FocusHandle, SharedString, Window, div, prelude::*, px};
use sound_core::{Instance, ProjectEvent};
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::cell::ROW_HEIGHT;
use sound_ui::components::device_card::{BODY_VALUE_LINE, CardFrame, PLAIN_CARD_WIDTH};
use sound_ui::{ActiveTheme, DeviceLabel, Devices, Session, Views};

use crate::{PluginRecord, WeakPlugins};

/// Registers the card of the `plugin` tool and what a rack calls one.
///
/// `plugins` is the host of this session, held weakly: the host must go when the project goes,
/// because that is what saves the state of every plugin.
pub fn register(views: &mut Views, devices: &mut Devices, plugins: WeakPlugins) {
    let for_view = plugins.clone();
    views.register_card(move |session, plugin, frame, window, cx| {
        PluginView::new(for_view.clone(), session, plugin, frame, window, cx)
    });
    devices.describe::<PluginRecord>(move |record| {
        // The name its maker gave it, or the id, which is all that is left of a plugin this
        // machine does not have.
        let installed = plugins
            .upgrade()
            .and_then(|plugins| plugins.installed_name(record.format, &record.plugin_id));
        DeviceLabel {
            key: PluginRecord::offer_key(record.format, &record.plugin_id).into(),
            name: installed.map_or_else(|| record.plugin_id.clone().into(), SharedString::from),
        }
    });
}

pub struct PluginView {
    session: Entity<Session>,
    plugin: Instance<PluginRecord>,
    plugins: WeakPlugins,
    /// The title and the close icon the rack gives the card.
    frame: CardFrame,
    /// The button takes one to be a tab stop and to show a focus ring.
    window_focus: FocusHandle,
}

impl PluginView {
    pub fn new(
        plugins: WeakPlugins,
        session: Entity<Session>,
        plugin: Instance<PluginRecord>,
        frame: CardFrame,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Every notify of the session, not only a change of this record: a plugin's window can
        // also close by itself, and the poll that saw that notifies.
        cx.observe(&session, |_, _, cx| cx.notify()).detach();
        cx.subscribe(&session, |view, _, event: &ProjectEvent, cx| {
            if matches!(event, ProjectEvent::Changed(id) if id == view.plugin.id()) {
                cx.notify();
            }
        })
        .detach();
        Self {
            session,
            plugin,
            plugins,
            frame,
            window_focus: cx.focus_handle().tab_stop(true),
        }
    }

    /// Opens the plugin's own window, or closes the one that is open. Not an edit: nothing of
    /// the project changes and there is no undo step. The host remembers it for the next time
    /// the project opens, in `workspace.json`.
    fn toggle_window(&mut self, cx: &mut Context<Self>) {
        let Some(plugins) = self.plugins.upgrade() else {
            return;
        };
        let id = self.plugin.id().clone();
        if plugins.window_is_open(&id) {
            plugins.close_window(&id, cx);
            cx.notify();
            return;
        }
        if let Err(problem) = plugins.open_window(&id, cx) {
            self.session
                .update(cx, |session, cx| session.report(problem, cx));
        }
        cx.notify();
    }
}

impl Render for PluginView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the record is deleted. Whatever hosts the view takes it away then.
        let Some(record) = self.session.read(cx).project().state(&self.plugin).cloned() else {
            return div().into_any_element();
        };
        let Some(plugins) = self.plugins.upgrade() else {
            return div().into_any_element();
        };
        let muted = cx.theme().gray_800;
        let line = |text: String| {
            div()
                .text_size(px(12.))
                .line_height(px(14.))
                .text_color(muted)
                .child(text)
        };
        let card = self.frame.card().w(px(PLAIN_CARD_WIDTH));
        // The body: what is at its top, and the line on the value line of the second row.
        let body = |top: Div, bottom: Option<Div>| {
            div()
                .relative()
                .w_full()
                .h(px(ROW_HEIGHT * 2.))
                .child(top)
                .children(bottom.map(|bottom| bottom.absolute().top(px(BODY_VALUE_LINE))))
        };

        let Some(installed) = plugins.installed(record.format, &record.plugin_id) else {
            // Missing. The card is named by the id, which is all that is left of the plugin.
            // The record stays as it is, the track is silent, and `problems.txt` says the
            // same thing to an agent.
            let text = format!(
                "This Mac has no {} plugin with this id. Install it, or pick another.",
                record.format.name()
            );
            return card.child(body(line(text), None)).into_any_element();
        };
        let detail = line(installed.detail());

        let id = self.plugin.id();
        // `None`: this machine has the plugin but it did not load, which is reported already.
        let Some(has_window) = plugins.window_offered(id) else {
            let text = "This plugin did not load, so there is nothing to open. See problems.txt.";
            return card
                .child(body(line(text.to_string()), Some(detail)))
                .into_any_element();
        };
        let is_open = plugins.window_is_open(id);
        let label = if is_open {
            "Close window"
        } else {
            "Open window"
        };
        let button = Button::new("plugin-window", label)
            .debug_selector(|| "plugin-window".to_string())
            .variant(ButtonVariant::Subtle)
            .size(ButtonSize::Sm)
            .disabled(!has_window)
            .focus_handle(&self.window_focus)
            .on_click(cx.listener(|view, _, _, cx| view.toggle_window(cx)));
        let top = div()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(4.))
            .child(button)
            .children(
                (!has_window).then(|| line("This plugin has no window of its own.".to_string())),
            );
        card.child(body(top, Some(detail))).into_any_element()
    }
}
