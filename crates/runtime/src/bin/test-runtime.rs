//! The program of this crate's tests: the forms of `runtime::cli::USAGE` without a window, the
//! child of a plugin scan and a plugin in a process of its own. The app is `sound-tools`.

fn main() -> anyhow::Result<()> {
    if let Some(served) = runtime::cli::plugin_process() {
        return served;
    }
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
    runtime::cli::run(&arguments).unwrap_or_else(|| anyhow::bail!(runtime::cli::USAGE))
}
