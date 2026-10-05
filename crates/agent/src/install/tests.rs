//! The download against local files and a local server, with the real curl and no network.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use smol::future;

use super::*;

/// Bytes that differ along the file, so a part in the wrong place shows.
fn content() -> Vec<u8> {
    (0..300_000u32).map(|index| (index % 251) as u8).collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// `path` as a `file://` URL. A Windows path starts with its drive, such as `C:\`, and needs
/// a slash before it.
fn file_url(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    let slash = if path.starts_with('/') { "" } else { "/" };
    format!("file://{slash}{path}")
}

/// The download of `content`, served from `source` as a `file://` URL.
fn download(source: &Path, content: &[u8]) -> Download {
    Download {
        name: "tool",
        version: "2.0.0".to_string(),
        url: file_url(source),
        sha256: sha256_hex(content),
        size: content.len() as u64,
    }
}

fn run(download: &Download, agents: &Path) -> (Result<PathBuf, InstallError>, Vec<u64>) {
    let mut heard = Vec::new();
    let result = smol::block_on(install(download, agents, |bytes| heard.push(bytes)));
    (result, heard)
}

#[test]
fn installs_the_program_executable_where_the_version_says() {
    let folder = tempfile::tempdir().unwrap();
    let source = folder.path().join("source");
    fs::write(&source, content()).unwrap();
    let agents = folder.path().join("agents");
    let download = download(&source, &content());

    let (result, heard) = run(&download, &agents);
    let program = result.unwrap();
    assert_eq!(program, agents.join("tool/2.0.0/tool"));
    assert_eq!(fs::read(&program).unwrap(), content());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&program).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111);
    }
    assert!(!download.partial(&agents).exists());
    assert_eq!(heard.last(), Some(&download.size));
}

/// On Windows the program is `claude.exe`, and its versions stay in `claude/`, as elsewhere.
#[test]
fn a_program_with_an_extension_keeps_its_versions_under_its_stem() {
    let download = Download {
        name: "claude.exe",
        ..download(Path::new("/unused"), &content())
    };
    assert_eq!(
        download.program(Path::new("agents")),
        Path::new("agents/claude/2.0.0/claude.exe")
    );
}

#[test]
fn a_damaged_download_is_deleted() {
    let folder = tempfile::tempdir().unwrap();
    let source = folder.path().join("source");
    let mut damaged = content();
    damaged[1000] ^= 1;
    fs::write(&source, &damaged).unwrap();
    let agents = folder.path().join("agents");
    let download = download(&source, &content());

    let (result, _) = run(&download, &agents);
    assert!(matches!(result, Err(InstallError::Damaged)), "{result:?}");
    assert!(!download.partial(&agents).exists());
    assert!(!download.program(&agents).exists());
}

#[test]
fn a_broken_download_resumes_where_it_stopped() {
    let folder = tempfile::tempdir().unwrap();
    let content = content();
    let (start, _) = content.split_at(100_000);
    // The source starts with other bytes: only a download that resumes and keeps the start
    // on disk matches the checksum.
    let mut source_bytes = vec![0; start.len()];
    source_bytes.extend_from_slice(&content[start.len()..]);
    let source = folder.path().join("source");
    fs::write(&source, &source_bytes).unwrap();
    let agents = folder.path().join("agents");
    let download = download(&source, &content);
    fs::create_dir_all(download.folder(&agents)).unwrap();
    fs::write(download.partial(&agents), start).unwrap();

    let (result, _) = run(&download, &agents);
    assert_eq!(fs::read(result.unwrap()).unwrap(), content);
}

#[test]
fn the_other_versions_are_removed_once_the_new_one_checks_out() {
    let folder = tempfile::tempdir().unwrap();
    let source = folder.path().join("source");
    fs::write(&source, content()).unwrap();
    let agents = folder.path().join("agents");
    let older = agents.join("tool/1.0.0");
    fs::create_dir_all(&older).unwrap();
    fs::write(older.join("tool"), "older").unwrap();
    let download = download(&source, &content());

    // A damaged download keeps the older one.
    let mut damaged = download.clone();
    damaged.sha256 = sha256_hex(b"something else");
    assert!(run(&damaged, &agents).0.is_err());
    assert!(older.exists());

    run(&download, &agents).0.unwrap();
    assert!(!older.exists());
    assert!(download.program(&agents).exists());
}

#[test]
fn cancelling_kills_curl_and_keeps_what_came() {
    // A server that takes the connection and never answers.
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = server.local_addr().unwrap();
    let folder = tempfile::tempdir().unwrap();
    let agents = folder.path().join("agents");
    let mut download = download(Path::new("/unused"), &content());
    download.url = format!("http://{address}/tool");
    fs::create_dir_all(download.folder(&agents)).unwrap();
    fs::write(download.partial(&agents), &content()[..1000]).unwrap();

    let cancelled = smol::block_on(future::or(
        async {
            install(&download, &agents, |_| {}).await.ok();
            false
        },
        async {
            #[allow(clippy::disallowed_methods)]
            smol::Timer::after(Duration::from_millis(500)).await;
            true
        },
    ));
    assert!(cancelled);

    // The connection ends, because curl is gone.
    let (mut connection, _) = server.accept().unwrap();
    connection
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut request = Vec::new();
    connection.read_to_end(&mut request).unwrap();
    assert!(request.starts_with(b"GET /tool"));
    assert_eq!(
        fs::read(download.partial(&agents)).unwrap(),
        &content()[..1000]
    );
}

/// A server that answers every request with `status` and `body`, with no range, as an error
/// page or a server that does not resume does. Gives its URL and the requests it got.
fn serve(status: &'static str, body: Vec<u8>) -> (String, Arc<Mutex<Vec<String>>>) {
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/tool", server.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    std::thread::spawn({
        let requests = requests.clone();
        move || {
            for connection in server.incoming() {
                let mut connection = connection.unwrap();
                let mut request = [0; 4096];
                let read = connection.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..read]).to_string();
                requests.lock().unwrap().push(request);
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                // curl may hang up first, when it does not like the answer.
                connection.write_all(head.as_bytes()).ok();
                connection.write_all(&body).ok();
            }
        }
    });
    (url, requests)
}

#[test]
fn a_server_that_does_not_resume_starts_the_download_again() {
    let (url, requests) = serve("200 OK", content());
    let folder = tempfile::tempdir().unwrap();
    let agents = folder.path().join("agents");
    let mut download = download(Path::new("/unused"), &content());
    download.url = url;
    fs::create_dir_all(download.folder(&agents)).unwrap();
    fs::write(download.partial(&agents), vec![7; 1000]).unwrap();

    let (result, _) = run(&download, &agents);
    assert_eq!(fs::read(result.unwrap()).unwrap(), content());
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2, "{requests:?}");
    let ranged = |request: &String| request.to_lowercase().contains("range:");
    assert!(ranged(&requests[0]));
    assert!(!ranged(&requests[1]));
}

#[test]
fn a_partial_file_longer_than_the_program_starts_again() {
    let folder = tempfile::tempdir().unwrap();
    let source = folder.path().join("source");
    fs::write(&source, content()).unwrap();
    let agents = folder.path().join("agents");
    let download = download(&source, &content());
    fs::create_dir_all(download.folder(&agents)).unwrap();
    fs::write(download.partial(&agents), vec![7; content().len() + 10]).unwrap();

    let (result, _) = run(&download, &agents);
    assert_eq!(fs::read(result.unwrap()).unwrap(), content());
}

#[test]
fn two_installs_at_once_download_once_and_both_get_the_program() {
    let folder = tempfile::tempdir().unwrap();
    let source = folder.path().join("source");
    fs::write(&source, content()).unwrap();
    let agents = folder.path().join("agents");
    let download = download(&source, &content());

    let installs: Vec<_> = (0..2)
        .map(|_| {
            let (download, agents) = (download.clone(), agents.clone());
            std::thread::spawn(move || run(&download, &agents))
        })
        .collect();
    let mut heard = Vec::new();
    for install in installs {
        let (result, progress) = install.join().unwrap();
        assert_eq!(fs::read(result.unwrap()).unwrap(), content());
        heard.push(progress);
    }
    // The one that waited found the program there and downloaded nothing.
    assert!(heard.iter().any(Vec::is_empty), "{heard:?}");
}

#[test]
fn a_refused_download_says_what_the_server_said() {
    // An error page with a line, as a region block might.
    let body = b"Claude Code is not available in your region.\n".to_vec();
    let (url, _) = serve("403 Forbidden", body);
    let folder = tempfile::tempdir().unwrap();
    let agents = folder.path().join("agents");
    let mut download = download(Path::new("/unused"), &content());
    download.url = url;
    // A start on disk too: the server's error page has no range to resume.
    fs::create_dir_all(download.folder(&agents)).unwrap();
    fs::write(download.partial(&agents), &content()[..1000]).unwrap();

    let (result, _) = run(&download, &agents);
    let Err(InstallError::Refused { message }) = result else {
        panic!("{result:?}");
    };
    assert_eq!(message, "Claude Code is not available in your region.");
    assert!(!download.partial(&agents).exists());
}

#[test]
fn reads_the_message_of_an_answer() {
    assert_eq!(
        server_message(br#"{"error": {"type": "forbidden", "message": "Not in your region"}}"#),
        Some("Not in your region".to_string())
    );
    assert_eq!(
        server_message(b"\n  Access denied  \nmore"),
        Some("Access denied".to_string())
    );
    assert_eq!(server_message(b"<html><body>403</body></html>"), None);
    assert_eq!(server_message(b""), None);
    assert_eq!(
        curl_line("curl: (6) Could not resolve host: downloads.claude.ai\n"),
        "Could not resolve host: downloads.claude.ai"
    );
}
