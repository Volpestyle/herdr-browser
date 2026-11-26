//! Token-gated loopback Markdown preview server for native browser panes.
//!
//! Each explicitly opened file or folder registers its canonical root under an
//! unguessable per-session token on 127.0.0.1. Websites cannot read local files
//! (file:// stays blocked in Chromium, cross-origin requests are refused, and
//! every path is canonicalized strictly inside its registered root).
use notify::{EventKind, RecursiveMode, Watcher};
use serde_json::json;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, RwLock, mpsc};
use std::time::{Duration, Instant};

const PAGE_HTML: &str = include_str!("markdown/page.html");
const MARKED_ESM: &str = include_str!("markdown/marked.esm.js");
const README_NAMES: &[&str] = &["README.md", "readme.md", "index.md"];
const MAX_CONNECTIONS: usize = 64;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_DOC_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ASSET_BYTES: u64 = 128 * 1024 * 1024;

static SERVER: OnceLock<Result<Arc<Server>, String>> = OnceLock::new();
static CONNECTIONS: AtomicUsize = AtomicUsize::new(0);

struct Slot;
impl Drop for Slot {
    fn drop(&mut self) {
        CONNECTIONS.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "markdown" | "mdx"))
}

pub fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "avif" | "gif" | "jpeg" | "jpg" | "png" | "svg" | "webp"))
}

fn is_preview_path(path: &Path) -> bool {
    is_markdown_path(path) || is_image_path(path)
}

/// Information returned when registering a Markdown file or folder for preview.
#[derive(Clone, Debug)]
pub struct Preview {
    pub url: String,
    pub path: String,
    pub title: String,
}

/// Register a local Markdown file or folder on the loopback server and return its URL.
pub fn open(raw: &str) -> Result<Preview, String> {
    let server = SERVER.get_or_init(Server::start).clone()?;
    server.register(raw)
}

struct RootEntry {
    root_dir: PathBuf,
    default_doc: PathBuf,
}

struct Server {
    port: u16,
    roots: RwLock<HashMap<String, RootEntry>>,
    root_tokens: RwLock<HashMap<PathBuf, String>>,
}

impl Server {
    fn start() -> Result<Arc<Self>, String> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|e| format!("Cannot start Markdown preview server: {e}"))?;
        let port = listener
            .local_addr()
            .map_err(|e| format!("Cannot read Markdown preview port: {e}"))?
            .port();
        let server = Arc::new(Self {
            port,
            roots: RwLock::new(HashMap::new()),
            root_tokens: RwLock::new(HashMap::new()),
        });
        let acceptor = server.clone();
        std::thread::Builder::new()
            .name("herdr-md-listen".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    if CONNECTIONS.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                        CONNECTIONS.fetch_sub(1, Ordering::SeqCst);
                        drop(stream);
                        continue;
                    }
                    let srv = acceptor.clone();
                    let spawned = std::thread::Builder::new()
                        .name("herdr-md-client".into())
                        .spawn(move || {
                            let _slot = Slot;
                            srv.handle(stream);
                        });
                    if spawned.is_err() {
                        CONNECTIONS.fetch_sub(1, Ordering::SeqCst);
                    }
                }
            })
            .map_err(|e| format!("Cannot spawn Markdown listener: {e}"))?;
        Ok(server)
    }

    fn register(&self, raw: &str) -> Result<Preview, String> {
        let (doc_path, root_dir) = resolve_target(raw)?;
        let token = {
            let mut by_root = self.root_tokens.write().map_err(|_| "Markdown server lock poisoned")?;
            let mut roots = self.roots.write().map_err(|_| "Markdown server lock poisoned")?;
            if let Some(existing) = by_root.get(&root_dir).cloned() {
                if let Some(entry) = roots.get_mut(&existing) {
                    entry.default_doc = doc_path.clone();
                }
                existing
            } else {
                let tok = random_token()?;
                by_root.insert(root_dir.clone(), tok.clone());
                roots.insert(
                    tok.clone(),
                    RootEntry {
                        root_dir: root_dir.clone(),
                        default_doc: doc_path.clone(),
                    },
                );
                tok
            }
        };
        let rel = to_url_path(&root_dir, &doc_path)?;
        let title = doc_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Markdown")
            .to_string();
        Ok(Preview {
            url: format!("http://127.0.0.1:{}/{token}{rel}", self.port),
            path: doc_path.to_string_lossy().into_owned(),
            title,
        })
    }

    fn handle(&self, mut stream: TcpStream) {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
        let Some(req) = read_request(&mut stream) else {
            return;
        };
        if req.method != "GET" && req.method != "HEAD" {
            let _ = write_response(&mut stream, 405, "text/plain; charset=utf-8", b"method not allowed", false);
            return;
        }
        if let Err(msg) = check_host_and_origin(&req, self.port) {
            let _ = write_response(&mut stream, 403, "text/plain; charset=utf-8", msg.as_bytes(), false);
            return;
        }
        let parsed = match url::Url::parse(&format!("http://127.0.0.1{}", req.target)) {
            Ok(u) => u,
            Err(_) => {
                let _ = write_response(&mut stream, 400, "text/plain; charset=utf-8", b"malformed request target", false);
                return;
            }
        };
        let raw_path = parsed.path();
        let trimmed = raw_path.trim_start_matches('/');
        let (token_part, rest_raw) = match trimmed.split_once('/') {
            Some((t, r)) => (t, format!("/{r}")),
            None => (trimmed, "/".to_string()),
        };
        let Some(entry) = self.lookup_token(token_part) else {
            let _ = write_response(&mut stream, 404, "text/plain; charset=utf-8", b"not found", false);
            return;
        };
        let Ok(sub_path) = percent_decode(&rest_raw) else {
            let _ = write_response(&mut stream, 400, "text/plain; charset=utf-8", b"malformed path encoding", false);
            return;
        };
        if sub_path.contains('\0') {
            let _ = write_response(&mut stream, 400, "text/plain; charset=utf-8", b"invalid path", false);
            return;
        }
        let query_path = parsed
            .query_pairs()
            .find(|(k, _)| k == "path")
            .map(|(_, v)| v.into_owned())
            .unwrap_or_else(|| "/".to_string());
        if query_path.contains('\0') {
            let _ = write_response(&mut stream, 400, "text/plain; charset=utf-8", b"invalid path", false);
            return;
        }
        let head_only = req.method == "HEAD";
        match sub_path.as_str() {
            "/__vendor/marked.esm.js" => {
                let _ = write_response(&mut stream, 200, "text/javascript; charset=utf-8", MARKED_ESM.as_bytes(), head_only);
            }
            "/__doc" => {
                serve_doc(&mut stream, &entry, &query_path, head_only);
            }
            "/__tree" => {
                serve_tree(&mut stream, &entry, &query_path, head_only);
            }
            "/__events" => {
                serve_events(&mut stream, &entry, &query_path);
            }
            _ => {
                serve_asset(&mut stream, &entry, token_part, &sub_path, req.accept.as_deref(), head_only);
            }
        }
    }

    fn lookup_token(&self, given: &str) -> Option<RootEntry> {
        let roots = self.roots.read().ok()?;
        let mut matched: Option<&RootEntry> = None;
        for (tok, entry) in roots.iter() {
            if constant_time_eq(tok.as_bytes(), given.as_bytes()) {
                matched = Some(entry);
            }
        }
        matched.map(|e| RootEntry {
            root_dir: e.root_dir.clone(),
            default_doc: e.default_doc.clone(),
        })
    }
}

struct Request {
    method: String,
    target: String,
    host: Option<String>,
    origin: Option<String>,
    accept: Option<String>,
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut total = 0usize;
    let mut line = String::new();
    total += reader.read_line(&mut line).ok()?;
    if total == 0 || total > MAX_HEADER_BYTES {
        return None;
    }
    let mut parts = line.trim_end_matches(['\r', '\n']).split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    let mut host = None;
    let mut origin = None;
    let mut accept = None;
    loop {
        line.clear();
        let n = reader.read_line(&mut line).ok()?;
        total += n;
        if n == 0 || total > MAX_HEADER_BYTES {
            return None;
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            let val = v.trim().to_string();
            if k.eq_ignore_ascii_case("host") {
                host = Some(val);
            } else if k.eq_ignore_ascii_case("origin") {
                origin = Some(val);
            } else if k.eq_ignore_ascii_case("accept") {
                accept = Some(val);
            }
        }
    }
    Some(Request { method, target, host, origin, accept })
}

fn check_host_and_origin(req: &Request, port: u16) -> Result<(), &'static str> {
    let host = req.host.as_deref().ok_or("missing Host")?;
    let expected_ip = format!("127.0.0.1:{port}");
    let expected_lh = format!("localhost:{port}");
    if host != expected_ip && host != expected_lh {
        return Err("Host must be this loopback endpoint");
    }
    if let Some(origin) = req.origin.as_deref() {
        let allowed_ip = format!("http://{expected_ip}");
        let allowed_lh = format!("http://{expected_lh}");
        if origin != allowed_ip && origin != allowed_lh {
            return Err("cross-origin access refused");
        }
    }
    Ok(())
}

const ASSET_CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; sandbox";

fn write_response(stream: &mut TcpStream, status: u16, content_type: &str, body: &[u8], head_only: bool) -> std::io::Result<()> {
    write_response_with_csp(stream, status, content_type, Some(ASSET_CSP), body, head_only)
}

fn write_response_with_csp(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    csp: Option<&str>,
    body: &[u8],
    head_only: bool,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        _ => "Internal Server Error",
    };
    let mut header = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Referrer-Policy: no-referrer\r\n",
        body.len()
    );
    if let Some(policy) = csp {
        header.push_str(&format!("Content-Security-Policy: {policy}\r\n"));
    }
    header.push_str("Connection: close\r\n\r\n");
    stream.write_all(header.as_bytes())?;
    if !head_only && !body.is_empty() {
        stream.write_all(body)?;
    }
    stream.flush()
}

fn serve_doc(stream: &mut TcpStream, entry: &RootEntry, query_path: &str, head_only: bool) {
    let Some(file) = resolve_preview_file(entry, query_path).filter(|p| is_markdown_path(p)) else {
        let _ = write_response(stream, 404, "text/plain; charset=utf-8", b"not a markdown document", head_only);
        return;
    };
    let Ok(meta) = fs::metadata(&file) else {
        let _ = write_response(stream, 404, "text/plain; charset=utf-8", b"document not found", head_only);
        return;
    };
    if meta.len() > MAX_DOC_BYTES {
        let _ = write_response(stream, 413, "text/plain; charset=utf-8", b"document exceeds 16 MiB preview limit", head_only);
        return;
    }
    match fs::read(&file) {
        Ok(bytes) => {
            let _ = write_response(stream, 200, "text/markdown; charset=utf-8", &bytes, head_only);
        }
        Err(_) => {
            let _ = write_response(stream, 404, "text/plain; charset=utf-8", b"could not read document", head_only);
        }
    }
}

fn serve_tree(stream: &mut TcpStream, entry: &RootEntry, query_path: &str, head_only: bool) {
    let Some(dir) = resolve_within_root(&entry.root_dir, query_path) else {
        let _ = write_response(stream, 404, "text/plain; charset=utf-8", b"not a directory", head_only);
        return;
    };
    if !dir.is_dir() {
        let _ = write_response(stream, 404, "text/plain; charset=utf-8", b"not a directory", head_only);
        return;
    }
    let Ok(read_dir) = fs::read_dir(&dir) else {
        let _ = write_response(stream, 404, "text/plain; charset=utf-8", b"cannot read directory", head_only);
        return;
    };
    let mut entries: Vec<(bool, String, serde_json::Value)> = Vec::new();
    for item in read_dir.flatten() {
        let name = item.file_name().to_string_lossy().into_owned();
        if name == ".git" {
            continue;
        }
        let candidate = item.path();
        let Ok(resolved) = fs::canonicalize(&candidate) else {
            continue;
        };
        if !resolved.starts_with(&entry.root_dir) {
            continue;
        }
        let Ok(meta) = fs::metadata(&resolved) else {
            continue;
        };
        let is_dir = meta.is_dir();
        let kind = if is_dir {
            "directory"
        } else if is_markdown_path(&candidate) {
            "markdown"
        } else if is_image_path(&candidate) {
            "image"
        } else {
            "file"
        };
        let Ok(url_path) = to_url_path(&entry.root_dir, &resolved) else {
            continue;
        };
        entries.push((is_dir, name.to_ascii_lowercase(), json!({ "name": name, "path": url_path, "type": kind })));
    }
    entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let list: Vec<serde_json::Value> = entries.into_iter().map(|(_, _, v)| v).collect();
    let payload = json!({ "path": query_path, "entries": list }).to_string();
    let _ = write_response(stream, 200, "application/json; charset=utf-8", payload.as_bytes(), head_only);
}

fn serve_asset(stream: &mut TcpStream, entry: &RootEntry, token: &str, sub_path: &str, accept: Option<&str>, head_only: bool) {
    let resolved = if sub_path == "/" {
        Some(entry.default_doc.clone())
    } else {
        resolve_within_root(&entry.root_dir, sub_path)
    };
    let Some(mut file_path) = resolved else {
        let _ = write_response(stream, 403, "text/plain; charset=utf-8", b"path escapes the preview root", head_only);
        return;
    };
    if file_path.is_dir() {
        let Some(readme) = find_readme(&entry.root_dir, &file_path) else {
            let _ = write_response(stream, 404, "text/plain; charset=utf-8", b"not found", head_only);
            return;
        };
        file_path = readme;
    }
    if !file_path.starts_with(&entry.root_dir) {
        let _ = write_response(stream, 403, "text/plain; charset=utf-8", b"path escapes the preview root", head_only);
        return;
    }
    let Ok(meta) = fs::metadata(&file_path) else {
        let _ = write_response(stream, 404, "text/plain; charset=utf-8", b"not found", head_only);
        return;
    };
    if !meta.is_file() {
        let _ = write_response(stream, 404, "text/plain; charset=utf-8", b"not found", head_only);
        return;
    }
    let wants_html = accept.is_some_and(|a| a.contains("text/html"));
    if is_markdown_path(&file_path) || (is_image_path(&file_path) && wants_html) {
        let title = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("Markdown");
        let root_name = entry.root_dir.file_name().and_then(|n| n.to_str()).unwrap_or("root");
        let doc_url_path = to_url_path(&entry.root_dir, &file_path).unwrap_or_else(|_| "/".into());
        let nonce = random_nonce();
        let html = render_page(title, root_name, &format!("/{token}"), &doc_url_path, &nonce);
        let csp = format!(
            "default-src 'none'; \
             script-src 'nonce-{nonce}' 'self' https://cdn.jsdelivr.net; \
             style-src 'self' 'unsafe-inline' https://cdn.jsdelivr.net; \
             img-src 'self' https: http: data: blob:; \
             media-src 'self' data: blob:; \
             font-src 'self' data:; \
             connect-src 'self'; \
             object-src 'none'; \
             frame-src 'none'; \
             base-uri 'none'; \
             form-action 'none'; \
             frame-ancestors 'none'"
        );
        let _ = write_response_with_csp(stream, 200, "text/html; charset=utf-8", Some(&csp), html.as_bytes(), head_only);
        return;
    }
    if meta.len() > MAX_ASSET_BYTES {
        let _ = write_response(stream, 413, "text/plain; charset=utf-8", b"asset exceeds 128 MiB limit", head_only);
        return;
    }
    let mime = mime_for(&file_path);
    match fs::read(&file_path) {
        Ok(bytes) => {
            let _ = write_response(stream, 200, mime, &bytes, head_only);
        }
        Err(_) => {
            let _ = write_response(stream, 404, "text/plain; charset=utf-8", b"not found", head_only);
        }
    }
}

fn serve_events(stream: &mut TcpStream, entry: &RootEntry, query_path: &str) {
    let Some(doc_file) = resolve_preview_file(entry, query_path) else {
        let _ = write_response(stream, 404, "text/plain; charset=utf-8", b"not a previewable file", false);
        return;
    };
    let Some(watch_dir) = doc_file.parent().map(Path::to_path_buf) else {
        let _ = write_response(stream, 404, "text/plain; charset=utf-8", b"no parent directory", false);
        return;
    };
    let target_name = doc_file.file_name().map(|n| n.to_os_string());
    let (tx, rx) = mpsc::channel::<()>();
    let mut watcher = match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        if matches!(ev.kind, EventKind::Access(_)) {
            return;
        }
        let relevant = ev.paths.is_empty()
            || ev.paths.iter().any(|p| p.file_name().map(|n| n.to_os_string()) == target_name);
        if relevant {
            let _ = tx.send(());
        }
    }) {
        Ok(w) => w,
        Err(_) => {
            let _ = write_response(stream, 500, "text/plain; charset=utf-8", b"cannot watch file", false);
            return;
        }
    };
    if watcher.watch(&watch_dir, RecursiveMode::NonRecursive).is_err() {
        let _ = write_response(stream, 500, "text/plain; charset=utf-8", b"cannot watch directory", false);
        return;
    }
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let header = "HTTP/1.1 200 OK\r\n\
        Content-Type: text/event-stream\r\n\
        Cache-Control: no-store\r\n\
        X-Content-Type-Options: nosniff\r\n\
        Connection: keep-alive\r\n\r\n\
        retry: 1000\n\n";
    if stream.write_all(header.as_bytes()).is_err() || stream.flush().is_err() {
        return;
    }
    let file_stamp = |p: &Path| {
        fs::metadata(p)
            .ok()
            .map(|m| (m.len(), m.modified().ok()))
    };
    let mut last_stamp = file_stamp(&doc_file);
    let mut last_heartbeat = Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_millis(300)) {
            Ok(()) => {
                // Debounce rapid atomic writes / editor saves.
                std::thread::sleep(Duration::from_millis(80));
                while rx.try_recv().is_ok() {}
                last_stamp = file_stamp(&doc_file);
                if stream.write_all(b"event: change\ndata: {}\n\n").is_err() || stream.flush().is_err() {
                    break;
                }
                last_heartbeat = Instant::now();
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let current = file_stamp(&doc_file);
                if current != last_stamp {
                    last_stamp = current;
                    if stream.write_all(b"event: change\ndata: {}\n\n").is_err() || stream.flush().is_err() {
                        break;
                    }
                    last_heartbeat = Instant::now();
                } else if last_heartbeat.elapsed() >= Duration::from_secs(20) {
                    if stream.write_all(b": heartbeat\n\n").is_err() || stream.flush().is_err() {
                        break;
                    }
                    last_heartbeat = Instant::now();
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn render_page(title: &str, root_name: &str, base_path: &str, doc_url_path: &str, nonce: &str) -> String {
    let base_json = serde_json::to_string(base_path).unwrap_or_else(|_| "\"\"".into());
    let doc_json = serde_json::to_string(doc_url_path).unwrap_or_else(|_| "\"/\"".into());
    PAGE_HTML
        .replace("{{NONCE}}", nonce)
        .replace("{{TITLE}}", &escape_html(title))
        .replace("{{ROOT_NAME}}", &escape_html(root_name))
        .replace("{{BASE_PATH_JSON}}", &base_json)
        .replace("{{DOC_PATH_JSON}}", &doc_json)
}

fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

fn resolve_target(raw: &str) -> Result<(PathBuf, PathBuf), String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("path is required".into());
    }
    let path = if trimmed.starts_with("file://") {
        let parsed = url::Url::parse(trimmed).map_err(|e| format!("Invalid file URL: {e}"))?;
        parsed
            .to_file_path()
            .map_err(|_| "file:// URL must name a local path".to_string())?
    } else if let Some(rest) = trimmed.strip_prefix("~/") {
        std::env::home_dir()
            .ok_or("Cannot resolve home directory")?
            .join(rest)
    } else if trimmed == "~" {
        std::env::home_dir().ok_or("Cannot resolve home directory")?
    } else {
        PathBuf::from(trimmed)
    };
    let canonical = fs::canonicalize(&path).map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
    let (doc_path, root_dir) = if canonical.is_dir() {
        let start = find_start_doc(&canonical, &canonical)
            .ok_or_else(|| format!("No Markdown or image file found in {}", canonical.display()))?;
        (start, canonical)
    } else {
        if !is_preview_path(&canonical) {
            return Err(format!("Not a Markdown or image file: {}", canonical.display()));
        }
        let parent = canonical
            .parent()
            .ok_or("File has no parent directory")?
            .to_path_buf();
        (canonical, parent)
    };
    Ok((doc_path, root_dir))
}

fn find_readme(root_dir: &Path, dir: &Path) -> Option<PathBuf> {
    for name in README_NAMES {
        let candidate = dir.join(name);
        if candidate.is_file() {
            if let Ok(canonical) = fs::canonicalize(candidate) {
                if canonical.starts_with(root_dir) {
                    return Some(canonical);
                }
            }
        }
    }
    None
}

fn find_start_doc(root_dir: &Path, dir: &Path) -> Option<PathBuf> {
    if let Some(readme) = find_readme(root_dir, dir) {
        return Some(readme);
    }
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    files
        .iter()
        .find(|p| is_markdown_path(p))
        .or_else(|| files.iter().find(|p| is_image_path(p)))
        .and_then(|p| fs::canonicalize(p).ok())
        .filter(|p| p.starts_with(root_dir))
}

fn resolve_within_root(root_dir: &Path, url_path: &str) -> Option<PathBuf> {
    if !url_path.starts_with('/') {
        return None;
    }
    let mut joined = root_dir.to_path_buf();
    for segment in url_path.trim_start_matches('/').split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." || segment.contains('\\') {
            return None;
        }
        joined.push(segment);
    }
    let canonical = fs::canonicalize(&joined).ok()?;
    canonical.starts_with(root_dir).then_some(canonical)
}

fn resolve_preview_file(entry: &RootEntry, query_path: &str) -> Option<PathBuf> {
    if query_path == "/" {
        return Some(entry.default_doc.clone());
    }
    resolve_within_root(&entry.root_dir, query_path).filter(|p| p.is_file() && is_preview_path(p))
}

fn to_url_path(root_dir: &Path, file_path: &Path) -> Result<String, String> {
    let rel = file_path
        .strip_prefix(root_dir)
        .map_err(|_| "path is outside preview root".to_string())?;
    let mut out = String::new();
    for comp in rel.components() {
        if let Component::Normal(seg) = comp {
            out.push('/');
            out.push_str(&percent_encode_segment(&seg.to_string_lossy()));
        }
    }
    if out.is_empty() {
        out.push('/');
    }
    Ok(out)
}

fn percent_encode_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(*b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn percent_decode(s: &str) -> Result<String, ()> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(());
            }
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).map_err(|_| ())?;
            let val = u8::from_str_radix(hex, 16).map_err(|_| ())?;
            out.push(val);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| ())
}

fn mime_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "avif" => "image/avif",
        "css" => "text/css; charset=utf-8",
        "gif" => "image/gif",
        // Never serve raw HTML or JavaScript from a previewed folder with executable MIME types.
        "htm" | "html" | "js" | "mjs" | "txt" => "text/plain; charset=utf-8",
        "ico" => "image/x-icon",
        "jpeg" | "jpg" => "image/jpeg",
        "json" => "application/json; charset=utf-8",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "ttf" => "font/ttf",
        "wav" => "audio/wav",
        "webm" => "video/webm",
        "webp" => "image/webp",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn random_nonce() -> String {
    random_bytes(16)
        .unwrap_or_else(|_| {
            let mut fallback = [0u8; 16];
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0u128, |d| d.as_nanos());
            fallback.copy_from_slice(&nanos.to_le_bytes());
            fallback.to_vec()
        })
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn random_bytes(len: usize) -> Result<Vec<u8>, String> {
    let mut bytes = vec![0u8; len];
    #[cfg(unix)]
    {
        File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut bytes))
            .map_err(|e| format!("Cannot create Markdown preview token: {e}"))?;
    }
    #[cfg(not(unix))]
    {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(format!("{:?}-{:?}", Instant::now(), std::process::id()).as_bytes());
        let digest = h.finalize();
        bytes.copy_from_slice(&digest[..len]);
    }
    Ok(bytes)
}

fn random_token() -> Result<String, String> {
    Ok(random_bytes(24)?.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;

    fn http_get(port: u16, target: &str, extra_headers: &[(&str, &str)]) -> (u16, String, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut req = format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n");
        for (k, v) in extra_headers {
            req.push_str(&format!("{k}: {v}\r\n"));
        }
        req.push_str("\r\n");
        stream.write_all(req.as_bytes()).unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).unwrap();
        let text = String::from_utf8_lossy(&raw).into_owned();
        let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
        let status = head
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        (status, head.to_string(), body.to_string())
    }

    #[test]
    fn serves_markdown_tree_doc_vendor_and_blocks_escapes() {
        let root = std::env::temp_dir().join(format!("herdr-md-test-{}", std::process::id()));
        let outside = std::env::temp_dir().join(format!("herdr-md-outside-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(root.join("hello world.md"), "# Hello\n\n> [!WARNING]\n> Watch out.\n").unwrap();
        fs::write(root.join("sub/note.md"), "# Sub Note").unwrap();
        fs::write(outside.join("secret.md"), "top secret").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.join("secret.md"), root.join("escape.md")).unwrap();

        let preview = open(&root.join("hello world.md").to_string_lossy()).unwrap();
        let parsed = url::Url::parse(&preview.url).unwrap();
        let port = parsed.port().unwrap();
        let token = parsed.path().trim_start_matches('/').split('/').next().unwrap().to_string();

        // HTML shell
        let (status, head, body) = http_get(port, parsed.path(), &[("Accept", "text/html")]);
        assert_eq!(status, 200);
        assert!(head.contains("text/html"));
        assert!(body.contains("hello world.md"));
        assert!(body.contains(&format!("/{token}")));

        // Raw markdown doc
        let (status, _, body) = http_get(port, &format!("/{token}/__doc?path=%2Fhello%20world.md"), &[]);
        assert_eq!(status, 200);
        assert!(body.contains("Watch out."));

        // Directory tree excludes symlink escape outside root
        let (status, _, body) = http_get(port, &format!("/{token}/__tree?path=%2F"), &[]);
        assert_eq!(status, 200);
        assert!(body.contains("hello world.md"));
        assert!(!body.contains("escape.md"));

        // Vendor marked.esm.js
        let (status, _, body) = http_get(port, &format!("/{token}/__vendor/marked.esm.js"), &[]);
        assert_eq!(status, 200);
        assert!(body.len() > 10_000);

        // Wrong token rejected
        let (status, _, _) = http_get(port, "/deadbeef/__doc?path=%2Fhello%20world.md", &[]);
        assert_eq!(status, 404);

        // Path traversal rejected
        let (status, _, _) = http_get(port, &format!("/{token}/__doc?path=%2F..%2Fsecret.md"), &[]);
        assert_eq!(status, 404);

        // Symlink escape rejected
        #[cfg(unix)]
        {
            let (status, _, _) = http_get(port, &format!("/{token}/__doc?path=%2Fescape.md"), &[]);
            assert_eq!(status, 404);
        }

        // Cross-origin refused
        let (status, _, _) = http_get(
            port,
            &format!("/{token}/__doc?path=%2Fhello%20world.md"),
            &[("Origin", "https://evil.example")],
        );
        assert_eq!(status, 403);

        // DNS rebinding Host refused
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .write_all(format!("GET /{token}/__doc?path=%2F HTTP/1.1\r\nHost: evil.example:{port}\r\n\r\n").as_bytes())
            .unwrap();
        let mut resp = String::new();
        stream.read_to_string(&mut resp).unwrap();
        assert!(resp.starts_with("HTTP/1.1 403"));

        // SSE live reload emits change event on edit
        let mut sse = TcpStream::connect(("127.0.0.1", port)).unwrap();
        sse.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        sse.write_all(
            format!("GET /{token}/__events?path=%2Fhello%20world.md HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n")
                .as_bytes(),
        )
        .unwrap();
        let mut reader = BufReader::new(sse);
        let mut line = String::new();
        loop {
            line.clear();
            reader.read_line(&mut line).unwrap();
            if line.starts_with("retry:") {
                break;
            }
        }
        fs::write(root.join("hello world.md"), "# Updated!\n").unwrap();
        let mut saw_change = false;
        for _ in 0..10 {
            line.clear();
            if reader.read_line(&mut line).unwrap() == 0 {
                break;
            }
            if line.trim() == "event: change" {
                saw_change = true;
                break;
            }
        }
        assert!(saw_change, "expected SSE change event after saving markdown file");

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn resolves_file_urls_and_directories() {
        let dir = std::env::temp_dir().join(format!("herdr-md-dir-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("README.md"), "# Readme").unwrap();
        let p1 = open(&dir.to_string_lossy()).unwrap();
        assert_eq!(p1.title, "README.md");
        let file_url = format!("file://{}", dir.join("README.md").display());
        let p2 = open(&file_url).unwrap();
        assert_eq!(p1.url, p2.url);
        assert!(open(&dir.join("missing.md").to_string_lossy()).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn blocks_readme_symlink_escape_and_sets_csp_and_safe_asset_mimes() {
        let root = std::env::temp_dir().join(format!("herdr-md-sec-{}", std::process::id()));
        let outside = std::env::temp_dir().join(format!("herdr-md-sec-out-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(root.join("doc.md"), "# Doc\n").unwrap();
        fs::write(root.join("evil.html"), "<script>alert(1)</script>").unwrap();
        fs::write(root.join("evil.js"), "alert(1)").unwrap();
        fs::write(outside.join("secret.txt"), "top-secret-key").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.join("secret.txt"), root.join("sub/README.md")).unwrap();

        let preview = open(&root.join("doc.md").to_string_lossy()).unwrap();
        let parsed = url::Url::parse(&preview.url).unwrap();
        let port = parsed.port().unwrap();
        let token = parsed.path().trim_start_matches('/').split('/').next().unwrap().to_string();

        // Viewer HTML ships strict nonce-based CSP and no external connect-src
        let (status, head, body) = http_get(port, parsed.path(), &[("Accept", "text/html")]);
        assert_eq!(status, 200);
        assert!(head.contains("Content-Security-Policy:"));
        assert!(head.contains("connect-src 'self'"));
        assert!(head.contains("frame-src 'none'"));
        assert!(!body.contains("{{NONCE}}"));

        // Subdirectory README.md symlink pointing outside root must never be served
        #[cfg(unix)]
        {
            let (status, _, body) = http_get(port, &format!("/{token}/sub"), &[]);
            assert_eq!(status, 404);
            assert!(!body.contains("top-secret-key"));
        }

        // Raw .html and .js files are neutered to text/plain + sandbox CSP
        let (status, head, _) = http_get(port, &format!("/{token}/evil.html"), &[]);
        assert_eq!(status, 200);
        assert!(head.contains("Content-Type: text/plain"));
        assert!(head.contains("Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; sandbox"));

        let (status, head, _) = http_get(port, &format!("/{token}/evil.js"), &[]);
        assert_eq!(status, 200);
        assert!(head.contains("Content-Type: text/plain"));

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }
}
