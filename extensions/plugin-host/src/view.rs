//! The card of a hosted plugin in a rack.
//!
//! A plugin draws its own interface in a window of its own, so this card is small: a control
//! that opens that window and closes it again. There is no generic parameter view: what a
//! plugin's knobs are is the plugin's business, and the name of the plugin is what the rack
//! puts above the card.
//!
//! A plugin this machine does not have shows what is wrong and the id the record names, so a
//! composer can see which plugin to install and an agent can be asked to correct the record.
//! The record itself is untouched, as everywhere else.

use gpui::{Context, Entity, FocusHandle, SharedString, Window, div, prelude::*, px};
use sound_core::{Instance, ProjectEvent};
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::{ActiveTheme, Devices, Session, Views};

use crate::{PluginRecord, Plugins, WeakPlugins};

/// Registers the card of the `plugin` tool and what a rack calls one.
///
/// `plugins` is the host of this session, held weakly: the host must go when the project goes,
/// because that is what saves the state of every plugin.
pub fn register(views: &mut Views, devices: &mut Devices, plugins: WeakPlugins) {
    let for_view = plugins.clone();
    views.register(move |session, plugin, window, cx| {
        PluginView::new(for_view.clone(), session, plugin, window, cx)
    });
    devices.name::<PluginRecord>(move |record| match plugins.upgrade() {
        // The name its maker gave it, or the id, which is all that is left of a plugin this
        // machine does not have.
        Some(plugins) => plugins
            .installed_name(&record.plugin_id)
            .map_or_else(|| record.plugin_id.clone().into(), SharedString::from),
        None => record.plugin_id.clone().into(),
    });
}

pub struct PluginView {
    session: Entity<Session>,
    plugin: Instance<PluginRecord>,
    plugins: WeakPlugins,
    /// The button takes one to be a tab stop and to show a focus ring.
    window_focus: FocusHandle,
}

impl PluginView {
    pub fn new(
        plugins: WeakPlugins,
        session: Entity<Session>,
        plugin: Instance<PluginRecord>,
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
            window_focus: cx.focus_handle().tab_stop(true),
        }
    }

    /// Opens the plugin's own window, or closes the one that is open. Not an edit: nothing of
    /// the project changes and there is no undo step.
    fn toggle_window(&mut self, cx: &mut Context<Self>) {
        let Some(plugins) = self.plugins.upgrade() else {
            return;
        };
        let (id, title) = (self.plugin.id().clone(), self.window_title(&plugins, cx));
        if plugins.window_is_open(&id) {
            plugins.close_window(&id, cx);
            cx.notify();
            return;
        }
        if let Err(problem) = plugins.open_window(&id, &title, cx) {
            self.session
                .update(cx, |session, cx| session.report(problem, cx));
        }
        cx.notify();
    }

    /// What the host suggests the plugin call its window: the plugin and the piece it plays in.
    fn window_title(&self, plugins: &Plugins, cx: &Context<Self>) -> String {
        let project = self.session.read(cx).project();
        let name = project
            .state(&self.plugin)
            .and_then(|record| plugins.installed_name(&record.plugin_id))
            .unwrap_or_default();
        let folder = project.root().file_name().unwrap_or_default();
        format!("{name} — {}", folder.to_string_lossy())
    }
}

impl Render for PluginView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the record is deleted. Whatever hosts the view takes it away then.
        let Some(record) = self.session.read(cx).project().state(&self.plugin).cloned() else {
            return div();
        };
        let Some(plugins) = self.plugins.upgrade() else {
            return div();
        };
        let muted = cx.theme().gray_700;
        let line = |text: String| {
            div()
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(muted)
                .child(text)
        };

        if plugins.installed_name(&record.plugin_id).is_none() {
            // Missing. The card is named by the id, which is all that is left of the plugin.
            // The record stays as it is, the track is silent, and `problems.txt` says the
            // same thing to an agent.
            return div().child(line(format!(
                "This Mac has no {} plugin with this id. Install it, or pick another.",
                record.format.name()
            )));
        }

        let id = self.plugin.id();
        let (has_window, is_open) = (plugins.has_window(id), plugins.window_is_open(id));
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
        div()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(4.))
            .child(button)
            .children(
                (!has_window).then(|| line("This plugin has no window of its own.".to_string())),
            )
    }
}
