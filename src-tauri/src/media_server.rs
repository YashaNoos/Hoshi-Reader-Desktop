//! Loopback HTTP server for audio playback on Linux.
//!
//! WebKitGTK cannot play `<audio>` from custom URI schemes (`audiobook://`,
//! `audio://`): the element fails with MEDIA_ERR_SRC_NOT_SUPPORTED. Plain
//! `http://` URLs work, so on Linux the audiobook and local word audio are
//! served from a tiny HTTP server bound to 127.0.0.1. Every URL carries a random
//! per-run token, so other local processes and web pages cannot read the files.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use tauri::AppHandle;

/// `http://127.0.0.1:<port>/<token>/`, set once the server is running.
pub static BASE: OnceLock<String> = OnceLock::new();

pub enum Media {
    File { path: PathBuf, mime: &'static str },
    Bytes { data: Vec<u8>, mime: &'static str },
}

/// Maps the request target (the part after the token, e.g. `audiobook/<id>`) to media.
type Resolver = dyn Fn(&str) -> Option<Media> + Send + Sync;

/// Starts the server for the app and returns the script that tells the frontend
/// where it is. Returns an empty script if the server could not be started.
pub fn init_script(app: &AppHandle) -> String {
    let handle = app.clone();
    let resolver = move |target: &str| -> Option<Media> {
        if let Some(id) = target.strip_prefix("audiobook/") {
            let id = id.split('?').next()?;
            if id.is_empty() || id.contains(['/', '\\']) || id.contains("..") {
                return None;
            }
            let path = crate::sasayaki::audio_path(&handle, id)?;
            let mime = crate::sasayaki::audio_mime(&path);
            return Some(Media::File { path, mime });
        }
        if target.starts_with("audio/") || target.starts_with("audio?") {
            let url = format!("{}{target}", BASE.get()?);
            let (source, file) = crate::local_audio::file_params(&url)?;
            let data = crate::local_audio::audio_bytes(&handle, &source, &file)?;
            let mime = crate::local_audio::audio_format(&data).1;
            return Some(Media::Bytes { data, mime });
        }
        None
    };
    match start(resolver) {
        Ok(base) => {
            let script = format!(
                "window.__HOSHI_MEDIA__ = {};",
                serde_json::to_string(&base).unwrap()
            );
            let _ = BASE.set(base);
            script
        }
        Err(error) => {
            eprintln!("could not start the local media server: {error}");
            String::new()
        }
    }
}

fn start(resolver: impl Fn(&str) -> Option<Media> + Send + Sync + 'static) -> std::io::Result<String> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let port = listener.local_addr()?.port();
    let token = uuid::Uuid::new_v4().simple().to_string();
    let resolver: std::sync::Arc<Resolver> = std::sync::Arc::new(resolver);
    {
        let token = token.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (resolver, token) = (resolver.clone(), token.clone());
                std::thread::spawn(move || {
                    let _ = handle(stream, &token, port, &*resolver);
                });
            }
        });
    }
    Ok(format!("http://127.0.0.1:{port}/{token}/"))
}

fn respond(stream: &mut TcpStream, status: &str, headers: &[(&str, String)]) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {status}\r\nConnection: close\r\nCache-Control: no-store\r\n");
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())
}

/// Parses a `Range` header into an inclusive `(start, end)` for a body of `len`
/// bytes. `Ok(None)` means no usable range (serve everything); `Err` means the
/// range cannot be satisfied.
fn parse_range(value: Option<&str>, len: u64) -> Result<Option<(u64, u64)>, ()> {
    let Some(spec) = value.and_then(|value| value.trim().strip_prefix("bytes=")) else {
        return Ok(None);
    };
    if spec.contains(',') {
        return Ok(None);
    }
    let Some((start, end)) = spec.split_once('-') else {
        return Ok(None);
    };
    let (start, end) = (start.trim(), end.trim());
    if start.is_empty() {
        // Suffix range: the last `n` bytes.
        let Ok(count) = end.parse::<u64>() else {
            return Ok(None);
        };
        if count == 0 {
            return Err(());
        }
        return Ok(Some((len.saturating_sub(count), len - 1)));
    }
    let Ok(start) = start.parse::<u64>() else {
        return Ok(None);
    };
    let end = if end.is_empty() {
        len - 1
    } else {
        match end.parse::<u64>() {
            Ok(end) => end.min(len - 1),
            Err(_) => return Ok(None),
        }
    };
    if start >= len || start > end {
        return Err(());
    }
    Ok(Some((start, end)))
}

fn handle(mut stream: TcpStream, token: &str, port: u16, resolver: &Resolver) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let mut reader = BufReader::new(stream.try_clone()?.take(64 * 1024));

    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return respond(&mut stream, "400 Bad Request", &[("Content-Length", "0".into())]);
    };
    let (method, target) = (method.to_owned(), target.to_owned());

    let (mut range, mut host) = (None, String::new());
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            match name.trim().to_ascii_lowercase().as_str() {
                "range" => range = Some(value.trim().to_owned()),
                "host" => host = value.trim().to_ascii_lowercase(),
                _ => {}
            }
        }
    }

    let not_found = |stream: &mut TcpStream| {
        respond(stream, "404 Not Found", &[("Content-Length", "0".into())])
    };
    let host_ok = host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}");
    let prefix = format!("/{token}/");
    let (true, Some(rest)) = (host_ok, target.strip_prefix(&prefix)) else {
        return not_found(&mut stream);
    };
    if method != "GET" && method != "HEAD" {
        return respond(&mut stream, "405 Method Not Allowed", &[("Content-Length", "0".into())]);
    }
    let Some(media) = resolver(rest) else {
        return not_found(&mut stream);
    };

    let (len, mime) = match &media {
        Media::File { path, mime } => match std::fs::metadata(path) {
            Ok(metadata) => (metadata.len(), *mime),
            Err(_) => return not_found(&mut stream),
        },
        Media::Bytes { data, mime } => (data.len() as u64, *mime),
    };
    if len == 0 {
        return not_found(&mut stream);
    }

    let (status, start, end, content_range) = match parse_range(range.as_deref(), len) {
        Ok(Some((start, end))) => (
            "206 Partial Content",
            start,
            end,
            Some(format!("bytes {start}-{end}/{len}")),
        ),
        Ok(None) => ("200 OK", 0, len - 1, None),
        Err(()) => {
            return respond(
                &mut stream,
                "416 Range Not Satisfiable",
                &[("Content-Range", format!("bytes */{len}")), ("Content-Length", "0".into())],
            );
        }
    };
    let count = end - start + 1;
    let mut headers = vec![
        ("Content-Type", mime.to_string()),
        ("Content-Length", count.to_string()),
        ("Accept-Ranges", "bytes".to_string()),
        ("Access-Control-Allow-Origin", "*".to_string()),
    ];
    if let Some(content_range) = content_range {
        headers.push(("Content-Range", content_range));
    }
    respond(&mut stream, status, &headers)?;
    if method == "HEAD" {
        return Ok(());
    }

    match media {
        Media::File { path, .. } => {
            let mut file = File::open(path)?;
            file.seek(SeekFrom::Start(start))?;
            std::io::copy(&mut file.take(count), &mut stream)?;
        }
        Media::Bytes { data, .. } => stream.write_all(&data[start as usize..=end as usize])?,
    }
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fetch(base: &str, path: &str, extra: &str) -> (String, Vec<u8>) {
        let (host, rest) = base.strip_prefix("http://").unwrap().split_once('/').unwrap();
        let mut stream = TcpStream::connect(host).unwrap();
        let request = format!("GET /{rest}{path} HTTP/1.1\r\nHost: {host}\r\n{extra}\r\n");
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        let split = response.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        (
            String::from_utf8_lossy(&response[..split]).into_owned(),
            response[split + 4..].to_vec(),
        )
    }

    #[test]
    fn serves_files_bytes_and_ranges() {
        let data: Vec<u8> = (0..=255u8).cycle().take(100_000).collect();
        let path = std::env::temp_dir().join(format!("hoshi-media-test-{}", std::process::id()));
        std::fs::write(&path, &data).unwrap();
        let (file_path, bytes) = (path.clone(), data[..1000].to_vec());
        let base = start(move |target| match target {
            "file" => Some(Media::File { path: file_path.clone(), mime: "audio/mpeg" }),
            "bytes" => Some(Media::Bytes { data: bytes.clone(), mime: "audio/ogg" }),
            _ => None,
        })
        .unwrap();

        let (head, body) = fetch(&base, "file", "");
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        assert!(head.contains("Content-Type: audio/mpeg") && head.contains("Content-Length: 100000"));
        assert_eq!(body, data);

        let (head, body) = fetch(&base, "file", "Range: bytes=10-19\r\n");
        assert!(head.starts_with("HTTP/1.1 206") && head.contains("Content-Range: bytes 10-19/100000"));
        assert_eq!(body, data[10..20]);

        let (head, body) = fetch(&base, "file", "Range: bytes=99990-\r\n");
        assert!(head.contains("Content-Range: bytes 99990-99999/100000"));
        assert_eq!(body, data[99_990..]);

        let (_, body) = fetch(&base, "file", "Range: bytes=-5\r\n");
        assert_eq!(body, data[99_995..]);

        let (head, _) = fetch(&base, "file", "Range: bytes=200000-\r\n");
        assert!(head.starts_with("HTTP/1.1 416") && head.contains("bytes */100000"));

        let (head, body) = fetch(&base, "bytes?x=1", "");
        assert!(head.starts_with("HTTP/1.1 404"), "unknown target: {head}");
        assert!(body.is_empty());
        let (head, body) = fetch(&base, "bytes", "Range: bytes=0-9\r\n");
        assert!(head.contains("audio/ogg") && head.starts_with("HTTP/1.1 206"));
        assert_eq!(body, data[..10]);

        let wrong_token = format!("{}x/", base.trim_end_matches('/'));
        assert!(fetch(&wrong_token, "file", "").0.starts_with("HTTP/1.1 404"));
        let (host, rest) = base.strip_prefix("http://").unwrap().split_once('/').unwrap();
        let mut stream = TcpStream::connect(host).unwrap();
        write!(stream, "GET /{rest}file HTTP/1.1\r\nHost: evil.example\r\n\r\n").unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 404"), "bad Host header accepted");
        std::fs::remove_file(path).ok();
    }
}
