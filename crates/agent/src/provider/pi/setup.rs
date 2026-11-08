//! Setting up Pi: whether it is signed in, its models, the sign-in, and one question to its
//! model. Each runs Pi once with our extension and asks its commands.
//!
//! Pi keeps every credential in its own store; the sign-in runs Pi's own flow, and the app
//! only shows its steps and hands back the composer's answers.

use std::io;
use std::process::{Output, Stdio};

use smol::channel::{Receiver, Sender};
use smol::future;
use smol::io::{AsyncWriteExt, BufReader};

use super::mapper::listed;
use super::protocol::{
    Incoming, Models, Providers, Question, SignInRecord, SignOutRecord, Step, UiRequest,
};
use super::{command, rpc};
use crate::provider::jsonl::read_line;
use crate::provider::{
    Account, Answer, Installed, Model, Pick, Service, SignInChoice, SignInQuestion, SignInStep,
    SignedIn, Status, Way,
};

/// How Pi signs in to one of its providers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PiSignIn {
    /// Pi's id of the provider, such as `anthropic`.
    pub provider: String,
    /// In the browser, or with an API key.
    pub with_key: bool,
}

/// Runs Pi with no session, asks it `commands` of our extension, and gives the records it
/// said for them, once it answered every one.
async fn ask(installed: &Installed, commands: &[&str]) -> io::Result<Vec<String>> {
    let mut command = rpc(installed)?;
    command.args(["--no-session", "--no-tools", "--offline"]);
    let mut child = smol::process::Command::from(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("no stdin"))?;
    for (index, command) in commands.iter().enumerate() {
        let line =
            serde_json::json!({ "id": index.to_string(), "type": "prompt", "message": command });
        stdin.write_all(format!("{line}\n").as_bytes()).await?;
    }
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("no stdout"))?;
    let mut stdout = BufReader::new(stdout);
    let mut buffer = Vec::new();
    let (mut said, mut answered) = (Vec::new(), 0);
    while answered < commands.len() {
        let Some(line) = read_line(&mut stdout, &mut buffer).await else {
            let output = child.output().await?;
            return Err(failed(&output, "it ended before it answered"));
        };
        match serde_json::from_str::<Incoming>(&line) {
            Ok(Incoming::Response { success: true, .. }) => answered += 1,
            Ok(Incoming::Response { error, .. }) => {
                return Err(io::Error::other(
                    error.unwrap_or_else(|| "it refused".to_string()),
                ));
            }
            Ok(Incoming::ExtensionUiRequest(UiRequest::Notify { message, .. })) => {
                said.push(message);
            }
            // Its notes and events say nothing we asked.
            Ok(_) | Err(_) => {}
        }
    }
    Ok(said)
}

/// The first record of `said` that reads as `T`.
fn record<T: serde::de::DeserializeOwned>(said: &[String]) -> io::Result<T> {
    said.iter()
        .find_map(|record| serde_json::from_str(record).ok())
        .ok_or_else(|| io::Error::other("its answer was unreadable"))
}

/// Signed in when it has a model to run. The account names the providers it is signed in to.
/// The ways in are every provider's: in the browser first, then those that take a key.
pub(crate) async fn status(installed: &Installed) -> io::Result<Status> {
    let said = ask(
        installed,
        &["/sound-tools-models", "/sound-tools-providers"],
    )
    .await?;
    let (models, providers): (Models, Providers) = (record(&said)?, record(&said)?);
    let signed_in: Vec<&str> = (providers.providers.iter())
        .filter(|provider| provider.signed_in)
        .map(|provider| provider.name.as_str())
        .collect();
    let account = (!models.models.is_empty()).then(|| Account {
        email: None,
        plan: (!signed_in.is_empty()).then(|| signed_in.join(" · ")),
    });
    let choice = |label: &str, provider: &str, with_key| SignInChoice {
        provider: crate::Provider::Pi,
        label: label.to_string(),
        way: Way::Pi(PiSignIn {
            provider: provider.to_string(),
            with_key,
        }),
    };
    let services = (providers.providers.iter())
        .map(|provider| Service {
            id: provider.id.clone(),
            name: provider.name.clone(),
            in_browser: (provider.sign_in.as_deref())
                .map(|label| choice(label, &provider.id, false)),
            with_key: (provider.api_key).then(|| choice(&provider.name, &provider.id, true)),
            signed_in: (provider.signed_in).then_some(match provider.stored {
                true => SignedIn::Here,
                false => SignedIn::Elsewhere,
            }),
        })
        .collect();
    Ok(Status {
        account,
        choices: Vec::new(),
        services,
        models: Some(listed(models)),
    })
}

pub(crate) async fn models(installed: &Installed) -> io::Result<Vec<Model>> {
    let said = ask(installed, &["/sound-tools-models"]).await?;
    Ok(listed(record(&said)?))
}

/// Removes what a sign-in to the service `id` saved in Pi's store.
pub(crate) async fn sign_out(installed: &Installed, id: &str) -> io::Result<()> {
    let said = ask(installed, &[&format!("/sound-tools-sign-out {id}")]).await?;
    match record(&said)? {
        SignOutRecord::SignedOut(_) => Ok(()),
        SignOutRecord::Failed(error) => Err(io::Error::other(error)),
    }
}

/// Runs Pi's sign-in to one provider and waits for it to end. Each step goes to `steps`, and
/// a question waits for its answer on `answers`, by the question's id. Dropping the future
/// cancels it: Pi is killed.
pub(crate) async fn sign_in(
    installed: &Installed,
    way: &PiSignIn,
    steps: Sender<SignInStep>,
    answers: Receiver<(String, String)>,
) -> io::Result<()> {
    let mut command = rpc(installed)?;
    command.args(["--no-session", "--no-tools"]);
    let mut child = smol::process::Command::from(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("no stdin"))?;
    let kind = if way.with_key { "api_key" } else { "oauth" };
    let message = format!("/sound-tools-sign-in {} {kind}", way.provider);
    let line = serde_json::json!({ "id": "sign-in", "type": "prompt", "message": message });
    stdin.write_all(format!("{line}\n").as_bytes()).await?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("no stdout"))?;
    let reading = async move {
        let mut stdout = BufReader::new(stdout);
        let mut buffer = Vec::new();
        while let Some(line) = read_line(&mut stdout, &mut buffer).await {
            let Ok(Incoming::ExtensionUiRequest(request)) = serde_json::from_str(&line) else {
                continue;
            };
            let step = match request {
                UiRequest::Notify { message, .. } => match serde_json::from_str(&message) {
                    Ok(SignInRecord::SignedIn(_)) => return Ok(()),
                    Ok(SignInRecord::Failed(error)) => return Err(io::Error::other(error)),
                    Ok(SignInRecord::Step(step)) => shown(step),
                    Err(_) => None,
                },
                UiRequest::Input { id, title } => match serde_json::from_str(&title) {
                    Ok(question) => Some(SignInStep::Question(asked(id, question))),
                    Err(_) => None,
                },
                UiRequest::Select { .. } | UiRequest::Other => None,
            };
            // Closed only when the sidebar went, and then nobody reads it.
            if let Some(step) = step {
                steps.try_send(step).ok();
            }
        }
        let output = child.output().await?;
        Err(failed(&output, "it ended before the sign-in did"))
    };
    let answering = async move {
        while let Ok((question, value)) = answers.recv().await {
            let line = serde_json::json!({ "type": "extension_ui_response", "id": question, "value": value });
            stdin.write_all(format!("{line}\n").as_bytes()).await?;
        }
        future::pending().await
    };
    future::or(reading, answering).await
}

fn shown(step: Step) -> Option<SignInStep> {
    match step {
        Step::AuthUrl { url } => Some(SignInStep::Page { url }),
        Step::DeviceCode {
            user_code,
            verification_uri,
        } => Some(SignInStep::Code {
            url: verification_uri,
            code: user_code,
        }),
        Step::Info { message } | Step::Progress { message } => Some(SignInStep::Note(message)),
        Step::Other => None,
    }
}

fn asked(id: String, question: Question) -> SignInQuestion {
    let text = |message, secret| SignInQuestion {
        id: id.clone(),
        message,
        answer: Answer::Text { secret },
    };
    match question {
        Question::Text { message } => text(message, false),
        // Pi's words for it are a programmer's: a redirect URL and a callback.
        Question::ManualCode {} => text(
            "If the browser does not come back here, paste the address it ended on.".to_string(),
            false,
        ),
        Question::Secret { message } => text(message, true),
        // Pi asks which way to sign in, in its words: "Select Anthropic login method:",
        // "Copy code login (headless)".
        Question::Select { options } => SignInQuestion {
            id: id.clone(),
            message: "How do you want to sign in?".to_string(),
            answer: Answer::Pick(
                options
                    .into_iter()
                    .map(|option| Pick {
                        label: (option.label.split_once(" ("))
                            .map_or(option.label.clone(), |(label, _)| label.to_string()),
                        id: option.id,
                    })
                    .collect(),
            ),
        },
    }
}

/// `pi -p` on its default model, with no tools, no session and no project. The question goes
/// on stdin, which has no length limit as arguments do.
pub(crate) async fn ask_small_model(
    installed: &Installed,
    instructions: &str,
    question: &str,
) -> io::Result<String> {
    let mut command = command(installed)?;
    command
        .args(["-p", "--no-session", "--no-tools"])
        .arg("--no-context-files")
        .arg("--system-prompt")
        .arg(instructions)
        .current_dir(std::env::temp_dir());
    let mut child = smol::process::Command::from(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(question.as_bytes()).await?;
    }
    let output = child.output().await?;
    if !output.status.success() {
        return Err(failed(&output, &format!("it ended with {}", output.status)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The first line Pi wrote that is not one of its warnings, or `otherwise`.
fn failed(output: &Output, otherwise: &str) -> io::Error {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let line = (stderr.lines().map(str::trim))
        .find(|line| !line.is_empty() && !line.starts_with("Warning:"));
    io::Error::other(line.unwrap_or(otherwise).to_string())
}
