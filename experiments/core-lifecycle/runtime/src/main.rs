use gpui::{
    AnyView, App, Application, Bounds, Context, Entity, Subscription, Timer, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, rgb, size,
};
use sound_core::{Project, Registry};
use sound_ui::{Session, Views};
use std::{
    fs,
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};

const REVISION: &str = "core-1";

fn snapshot(project: &Project) -> serde_json::Value {
    let records: serde_json::Map<_, _> = project
        .ids()
        .map(|id| {
            (
                id.to_string(),
                serde_json::to_value(project.record(id).unwrap()).unwrap(),
            )
        })
        .collect();
    serde_json::json!({"playing": project.playing(), "undo": project.undo_len(), "records": records, "connections": project.connections(), "revision": REVISION})
}
fn wav(path: &Path, samples: &[f32]) -> io::Result<()> {
    let mut file = fs::File::create(path)?;
    let length = samples.len() as u32 * 4;
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + length).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16_u32.to_le_bytes())?;
    file.write_all(&3_u16.to_le_bytes())?; // IEEE float
    file.write_all(&1_u16.to_le_bytes())?;
    file.write_all(&48_000_u32.to_le_bytes())?;
    file.write_all(&192_000_u32.to_le_bytes())?;
    file.write_all(&4_u16.to_le_bytes())?;
    file.write_all(&32_u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&length.to_le_bytes())?;
    for sample in samples {
        file.write_all(&sample.to_le_bytes())?;
    }
    Ok(())
}
struct Editor {
    id: String,
    view: AnyView,
}
struct Workspace {
    session: Entity<Session>,
    views: Views,
    editors: Vec<Editor>,
    _subscription: Subscription,
    first: bool,
}
impl Workspace {
    fn new(
        session: Entity<Session>,
        views: Views,
        quit: mpsc::Receiver<()>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.observe(&session, |_, _, cx| cx.notify());
        let project = session.read(cx).project();
        let layout = project.root().join("workspace.json");
        let ids: Vec<String> = if layout.exists() {
            serde_json::from_slice(&fs::read(layout).expect("read workspace"))
                .expect("decode workspace")
        } else {
            project
                .ids()
                .flat_map(|id| {
                    if id == "tone-a" {
                        vec![id.to_string(), id.to_string()]
                    } else {
                        vec![id.to_string()]
                    }
                })
                .collect()
        };
        let mut this = Self {
            session,
            views,
            editors: vec![],
            _subscription: subscription,
            first: true,
        };
        for id in ids {
            this.open(&id, cx);
        }
        cx.spawn(async move |weak, cx| {
            loop {
                Timer::after(Duration::from_millis(50)).await;
                if weak
                    .update(cx, |this, cx| {
                        this.session.update(cx, |session, cx| session.tick(cx));
                        if quit.try_recv().is_ok() {
                            this.session.update(cx, |session, cx| {
                                session.change(cx, |p| {
                                    p.set_playing(false);
                                    Ok(())
                                })
                            });
                            println!("STOPPED");
                            cx.quit();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        this
    }
    fn open(&mut self, id: &str, cx: &mut Context<Self>) {
        let name = self
            .session
            .read(cx)
            .project()
            .tool_name(id)
            .map(str::to_string);
        if let Some(name) = name {
            match self.views.open(&name, self.session.clone(), id, cx) {
                Ok(view) => self.editors.push(Editor {
                    id: id.into(),
                    view,
                }),
                Err(error) => eprintln!("{error}"),
            }
        }
    }
    fn save_layout(&self, cx: &mut Context<Self>) {
        let path = self
            .session
            .read(cx)
            .project()
            .root()
            .join("workspace.json");
        let ids: Vec<_> = self.editors.iter().map(|e| &e.id).collect();
        let temporary = path.with_extension("json.tmp");
        if let Err(error) = fs::write(&temporary, serde_json::to_vec_pretty(&ids).unwrap())
            .and_then(|_| fs::rename(temporary, path))
        {
            eprintln!("Workspace write: {error}");
        }
    }
}
impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.first {
            self.first = false;
            self.save_layout(cx);
            window.on_next_frame(|_, _| {
                println!("FIRST_FRAME {REVISION}");
                io::stdout().flush().unwrap();
            });
        }
        let session = self.session.read(cx);
        let playing = session.project().playing();
        let status = format!(
            "{} · rendered RMS {:.4} · undo {}",
            session.status,
            session.rms,
            session.project().undo_len()
        );
        let ids: Vec<_> = session.project().ids().map(str::to_string).collect();
        div()
            .id("workspace")
            .overflow_y_scroll()
            .size_full()
            .flex()
            .flex_col()
            .gap_4()
            .p_5()
            .bg(rgb(0x111c22))
            .text_color(rgb(0xe7f0ed))
            .child(
                div()
                    .text_xl()
                    .child("Sound Tools · core lifecycle prototype"),
            )
            .child("Offline rendering only · two Tone A views share one record")
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        div()
                            .id("play")
                            .p_2()
                            .bg(rgb(0x34515a))
                            .cursor_pointer()
                            .child(if playing {
                                "Stop rendering"
                            } else {
                                "Run renderer"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.session.update(cx, |session, cx| {
                                    session.change(cx, |p| {
                                        p.set_playing(!p.playing());
                                        Ok(())
                                    })
                                })
                            })),
                    )
                    .child(
                        div()
                            .id("undo")
                            .p_2()
                            .bg(rgb(0x34515a))
                            .cursor_pointer()
                            .child("Undo")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.session
                                    .update(cx, |session, cx| session.change(cx, Project::undo))
                            })),
                    )
                    .child(
                        div()
                            .id("redo")
                            .p_2()
                            .bg(rgb(0x34515a))
                            .cursor_pointer()
                            .child("Redo")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.session
                                    .update(cx, |session, cx| session.change(cx, Project::redo))
                            })),
                    ),
            )
            .child(status)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_3()
                    .children(self.editors.iter().enumerate().map(|(index, editor)| {
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(editor.view.clone())
                            .child(
                                div()
                                    .id(("close", index))
                                    .p_2()
                                    .cursor_pointer()
                                    .child("Close view")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.editors.remove(index);
                                        this.save_layout(cx);
                                        cx.notify();
                                    })),
                            )
                    })),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .children(ids.into_iter().enumerate().map(|(index, id)| {
                        div()
                            .id(("open", index))
                            .p_2()
                            .bg(rgb(0x283d46))
                            .cursor_pointer()
                            .child(format!("Open {id} view"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open(&id, cx);
                                this.save_layout(cx);
                                cx.notify();
                            }))
                    })),
            )
    }
}
struct Logger;
impl log::Log for Logger {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        eprintln!("{} {}", record.level(), record.args());
    }
    fn flush(&self) {}
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    static LOGGER: Logger = Logger;
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Warn);
    let args: Vec<_> = std::env::args().collect();
    let root = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("sound-core-lifecycle-demo"));
    let fresh = !root.join("project.json").exists();
    let mut registry = Registry::default();
    let tone = tone::register(&mut registry);
    let tremolo = tremolo::register(&mut registry);
    let mut views = Views::default();
    tone::register_view(&mut views, tone);
    tremolo::register_view(&mut views, tremolo);
    let mut project = Project::open(&root, registry)?;
    if fresh {
        let a = project.create(tone, "tone-a", tone::ToneState::default())?;
        project.create(
            tone,
            "tone-b",
            tone::ToneState {
                frequency_hz: 440.0,
                gain: 0.1,
            },
        )?;
        project.connect(&a, "audio")?;
        project.create(tremolo, "tremolo-a", tremolo::TremoloState::default())?;
        project.create(
            tremolo,
            "tremolo-b",
            tremolo::TremoloState {
                frequency_hz: 330.0,
                ..Default::default()
            },
        )?;
    }
    if args.get(2).map(String::as_str) == Some("--inspect") {
        println!("{}", serde_json::to_string_pretty(&snapshot(&project))?);
        return Ok(());
    }
    if args.get(2).map(String::as_str) == Some("--render") {
        let output = args.get(3).ok_or("--render requires an output WAV path")?;
        project.set_playing(true);
        let mut samples = vec![0.0; 48_000];
        project.render(&mut samples, 48_000.0);
        wav(Path::new(output), &samples)?;
        println!("Rendered {} samples to {output}", samples.len());
        return Ok(());
    }
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        for line in io::stdin().lock().lines().map_while(Result::ok) {
            if line == "quit" {
                let _ = send.send(());
                break;
            }
        }
    });
    Application::new().run(move |cx: &mut App| {
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let session = cx.new(|_| Session::new(project));
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(980.), px(740.)),
                    cx,
                ))),
                ..Default::default()
            },
            |_, cx| cx.new(|cx| Workspace::new(session, views, receive, cx)),
        )
        .unwrap();
        cx.activate(true);
    });
    Ok(())
}
