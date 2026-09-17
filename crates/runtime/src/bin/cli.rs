use sound_core::Result;
use sound_runtime::{inspect, open_or_create, play_device, register_tools, render_wav, watch};
use std::sync::Arc;

fn usage() -> ! {
    eprintln!(
        "usage: sound-cli <project-dir> [--inspect] [--render <out.wav>] [--seconds <n>] [--watch]"
    );
    std::process::exit(2);
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let Some(root) = args.next() else { usage() };
    let mut inspect_flag = false;
    let mut render: Option<String> = None;
    let mut seconds = 4.0;
    let mut watch_flag = false;
    let mut play_flag = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--inspect" => inspect_flag = true,
            "--render" => render = Some(args.next().unwrap_or_else(|| usage())),
            "--seconds" => {
                seconds = args
                    .next()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or_else(|| usage())
            }
            "--watch" => watch_flag = true,
            "--play" => play_flag = true,
            _ => usage(),
        }
    }
    let mut registry = sound_core::registry::Registry::default();
    let (arrangement_tool, mixer_tool) = register_tools(&mut registry)?;
    let registry = Arc::new(registry);
    let mut project = open_or_create(&root, registry, arrangement_tool, mixer_tool)?;
    if inspect_flag {
        inspect(&project);
    }
    if let Some(out) = render {
        render_wav(&project, seconds, &out)?;
    }
    if watch_flag {
        watch(&mut project, seconds)?;
    }
    if play_flag {
        play_device(&mut project, seconds)?;
    }
    Ok(())
}
