//! Chats with Claude Code in the terminal, in a folder:
//!
//! ```sh
//! cargo run -p sound-agent --example chat -- <folder>
//! ```
//!
//! Uses the `claude` on the login shell's `PATH` and the default approval mode, "ask before
//! commands". Type a message and press return. Answer a question with y (allow), a (allow
//! for this thread) or n (deny). ctrl-c stops the turn, ctrl-d quits.

use std::error::Error;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use smol::channel;
use smol::future;
use sound_agent::{
    AgentEvent, ApprovalAnswer, ApprovalId, ApprovalMode, Provider, Session, StepOutcome, Thread,
    ThreadOptions, login_shell_environment, program_on_path,
};

enum Input {
    Event(Option<AgentEvent>),
    Line(Option<String>),
    Interrupt,
}

fn main() -> Result<(), Box<dyn Error>> {
    let folder = std::env::args_os()
        .nth(1)
        .ok_or("usage: cargo run -p sound-agent --example chat -- <folder>")?;
    let folder = PathBuf::from(folder).canonicalize()?;

    // Lines from the terminal, read on a thread of their own because reading blocks.
    let (lines_sender, lines) = channel::unbounded();
    std::thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if lines_sender.send_blocking(line).is_err() {
                break;
            }
        }
    });
    let (interrupts_sender, interrupts) = channel::unbounded();
    ctrlc::set_handler(move || {
        // The chat is gone, so ctrl-c quits as usual.
        if interrupts_sender.try_send(()).is_err() {
            std::process::exit(130);
        }
    })?;

    smol::block_on(async {
        let environment = login_shell_environment().await;
        let program = program_on_path("claude", &environment).ok_or("claude is not on PATH")?;
        let (thread, mut events) = Thread::start(ThreadOptions {
            provider: Provider::Claude,
            program,
            folder,
            model: None,
            approval_mode: ApprovalMode::default(),
            session: Session::New,
            environment,
        })?;
        let mut thread = Some(thread);
        let mut approval: Option<ApprovalId> = None;
        // Whether the text block on screen came as deltas, so its done text is not printed twice.
        let mut streamed = false;
        prompt();

        loop {
            let input_open = thread.is_some();
            let event = async { Input::Event(events.next().await) };
            let line = async {
                if input_open {
                    Input::Line(lines.recv().await.ok())
                } else {
                    future::pending().await
                }
            };
            let interrupt = async {
                match interrupts.recv().await {
                    Ok(()) => Input::Interrupt,
                    Err(_) => future::pending().await,
                }
            };
            match future::or(event, future::or(line, interrupt)).await {
                Input::Event(None) => break,
                Input::Event(Some(event)) => match event {
                    AgentEvent::Started {
                        session_id,
                        account,
                        models,
                    } => {
                        let email = account.email.unwrap_or_default();
                        let plan = account.plan.unwrap_or_default();
                        let models: Vec<_> = models.iter().map(|model| model.id.as_str()).collect();
                        println!("\r[{email}, {plan}. Session {session_id}]");
                        println!("[Models: {}]", models.join(", "));
                        prompt();
                    }
                    AgentEvent::TurnStarted => {}
                    AgentEvent::TextDelta { text } => {
                        streamed = true;
                        print!("{text}");
                        io::stdout().flush()?;
                    }
                    AgentEvent::TextDone { text } => {
                        if !streamed {
                            print!("{text}");
                        }
                        println!();
                        streamed = false;
                    }
                    AgentEvent::StepStarted { title, .. } => println!("  · {title}"),
                    AgentEvent::StepDone { outcome, .. } => match outcome {
                        StepOutcome::Done => {}
                        StepOutcome::Failed => println!("    failed"),
                        StepOutcome::Denied => println!("    denied"),
                    },
                    AgentEvent::ApprovalRequested { id, title } => {
                        println!("? {title}  [y] allow, [a] allow for this thread, [n] deny");
                        approval = Some(id);
                    }
                    AgentEvent::TurnEnded { outcome } => {
                        println!("[{outcome:?}]");
                        approval = None;
                        prompt();
                    }
                    AgentEvent::Error { message } => eprintln!("[error: {message}]"),
                    AgentEvent::Exited { reason } => println!("[exited: {reason:?}]"),
                },
                Input::Line(None) => {
                    // ctrl-d. Dropping the thread ends Claude Code, then the events end.
                    thread = None;
                }
                Input::Line(Some(line)) => {
                    let Some(thread) = &thread else { continue };
                    let line = line.trim();
                    if let Some(id) = approval.take() {
                        let answer = match line {
                            "y" => ApprovalAnswer::Allow,
                            "a" => ApprovalAnswer::AllowForThread,
                            "n" => ApprovalAnswer::Deny,
                            _ => {
                                println!("y, a or n");
                                approval = Some(id);
                                continue;
                            }
                        };
                        thread.answer(id, answer)?;
                    } else if !line.is_empty() {
                        thread.send(line)?;
                    }
                }
                Input::Interrupt => {
                    if let Some(thread) = &thread {
                        println!("\n[stopping]");
                        thread.interrupt()?;
                    }
                }
            }
        }
        Ok::<(), Box<dyn Error>>(())
    })
}

fn prompt() {
    print!("> ");
    if let Err(error) = io::stdout().flush() {
        eprintln!("{error}");
    }
}
