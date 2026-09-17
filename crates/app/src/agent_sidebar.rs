use crate::agent::{self, Run, Update};
use gpui::{Context, Entity, Render, Window, div, prelude::*, px};
use sound_ui::components::{
    button::{Button, ButtonSize},
    text_input::TextInput,
};
use std::{path::PathBuf, time::Duration};

pub struct AgentSidebar {
    root: PathBuf,
    input: Entity<TextInput>,
    run: Option<Run>,
    update: Update,
    prompt: String,
}

impl AgentSidebar {
    pub fn new(root: PathBuf, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextInput::new(cx).placeholder("Describe a project task"));
        let weak = cx.weak_entity();
        input.update(cx, |input, _| {
            input.set_on_submit(move |text, _, cx| {
                weak.update(cx, |this, cx| this.send(text, cx)).ok();
            });
        });
        cx.on_app_quit(|this, _| {
            this.run.take();
            async {}
        })
        .detach();
        cx.spawn(async move |this, cx| {
            loop {
                gpui::Timer::after(Duration::from_millis(100)).await;
                if this
                    .update(cx, |this, cx| {
                        if let Some(run) = &this.run {
                            this.update = run.snapshot();
                            if this.update.finished {
                                this.run.take();
                            }
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        Self {
            root,
            input,
            run: None,
            update: Update {
                status: agent::binary().map_or_else(str::to_owned, |_| {
                    "Pi executable found. Provider not checked. Nothing runs until Send.".into()
                }),
                ..Update::default()
            },
            prompt: String::new(),
        }
    }

    fn send(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.run.is_some() {
            return;
        }
        match Run::start(&self.root, text) {
            Ok(run) => {
                self.prompt = text.to_owned();
                self.update = run.snapshot();
                self.run = Some(run);
            }
            Err(error) => self.update.status = error.into(),
        }
        cx.notify();
    }
}

impl Render for AgentSidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().flex().flex_col().gap_3().min_w_0()
            .child("Pi agent")
            .child(div().text_xs().child(self.root.to_string_lossy().into_owned()))
            .child(div().text_xs().child("Send (or Enter) authorizes Pi to use coding tools on this project with your configured provider. Not sandboxed. Project Pi config, extensions, skills and context discovery are disabled. Requires a Pi version supporting --no-approve. Each task starts fresh."))
            .child(self.input.clone())
            .child(div().flex().gap_2()
                .child(Button::new("agent-send", "Send").size(ButtonSize::Sm).disabled(self.run.is_some())
                    .on_click(cx.listener(|this, _, _, cx| {
                        let text = this.input.read(cx).text().to_owned();
                        this.send(&text, cx);
                    })))
                .child(Button::new("agent-cancel", "Cancel").size(ButtonSize::Sm).disabled(self.run.is_none())
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(run) = &this.run { run.cancel(); }
                        this.update.status = "Cancelling Pi...".into();
                        cx.notify();
                    }))))
            .child(div().text_xs().child(self.update.status.clone()))
            .child(div().id("agent-transcript").h(px(240.)).overflow_y_scroll().text_xs()
                .child(div().child(if self.prompt.is_empty() { String::new() } else { format!("You: {}", self.prompt) }))
                .child(div().mt_2().child(self.update.text.clone()))
                .when(self.update.truncated, |view| view.child("Transcript truncated at 32 KiB.")))
    }
}
