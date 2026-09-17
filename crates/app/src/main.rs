mod agent;
mod agent_sidebar;
mod editing;
mod instruments;
mod timeline;
mod workspace;

use gpui::{
    App, Application, Bounds, KeyBinding, TitlebarOptions, WindowBounds, WindowOptions, prelude::*,
    px, size,
};
use sound_runtime::session::Session;
use sound_ui::Assets;
use workspace::{Redo, TogglePlay, Undo, Workspace};

const WORKSPACE_KEYS: &str = "Workspace && !TextInput";

fn main() {
    let mut args = std::env::args_os().skip(1);
    let Some(root) = args.next().and_then(|path| path.into_string().ok()) else {
        eprintln!("Usage: sound-app <project-directory>");
        std::process::exit(2);
    };
    if args.next().is_some() {
        eprintln!("Usage: sound-app <project-directory>");
        std::process::exit(2);
    }
    let session = match Session::open(&root) {
        Ok(session) => session,
        Err(error) => {
            eprintln!("Cannot open project {root}: {error}");
            std::process::exit(1);
        }
    };
    Application::new()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            sound_ui::init(cx);
            cx.bind_keys([
                KeyBinding::new("space", TogglePlay, Some(WORKSPACE_KEYS)),
                KeyBinding::new("ctrl-z", Undo, Some(WORKSPACE_KEYS)),
                KeyBinding::new("ctrl-shift-z", Redo, Some(WORKSPACE_KEYS)),
            ]);
            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            let result = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1280.), px(800.)),
                        cx,
                    ))),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Sound Tools".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |window, cx| cx.new(|cx| Workspace::new(session, root, window, cx)),
            );
            if let Err(error) = result {
                eprintln!("Cannot open window: {error}");
                cx.quit();
                return;
            }
            cx.activate(true);
        });
}

#[cfg(test)]
mod tests {
    #[test]
    fn workspace_shortcuts_do_not_match_text_input() {
        let predicate = gpui::KeyBindingContextPredicate::parse(super::WORKSPACE_KEYS).unwrap();
        let workspace = gpui::KeyContext::parse("Workspace").unwrap();
        let input = gpui::KeyContext::parse("TextInput").unwrap();
        assert!(
            predicate
                .depth_of(std::slice::from_ref(&workspace))
                .is_some()
        );
        assert!(predicate.depth_of(&[workspace, input]).is_none());
    }
}
