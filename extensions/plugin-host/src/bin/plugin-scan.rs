//! The scan's child process for the tests of this crate, which have no runtime: lists one
//! bundle, one line of JSON per plugin. The runtime uses its own executable instead. Started
//! with `plugin_host::PLAY_ARGUMENT`, it plays one plugin in a process of its own.

use std::path::PathBuf;

fn main() -> std::process::ExitCode {
    // A plugin in a process of its own, as the runtime starts one with the same argument.
    #[cfg(unix)]
    {
        let arguments: Vec<_> = std::env::args_os().skip(1).collect();
        if arguments
            .first()
            .is_some_and(|first| first == plugin_host::PLAY_ARGUMENT)
        {
            if let Err(message) = plugin_host::serve_plugin(&arguments[1..]) {
                eprintln!("{message}");
            }
            return std::process::ExitCode::FAILURE;
        }
    }
    let mut arguments = std::env::args_os().skip(1);
    let format = arguments
        .next()
        .and_then(|format| plugin_host::PluginFormat::of_str(&format.to_string_lossy()));
    let bundle = arguments.next().map(PathBuf::from);
    let (Some(format), Some(bundle)) = (format, bundle) else {
        eprintln!("usage: plugin-scan <clap|vst3> <bundle>");
        return std::process::ExitCode::FAILURE;
    };
    match plugin_host::scan_one_bundle(format, &bundle) {
        Ok(lines) => {
            print!("{lines}");
            std::process::ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            std::process::ExitCode::FAILURE
        }
    }
}
