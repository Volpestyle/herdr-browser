//! Pane-local, in-process CDP. All calls and observer callbacks stay on CEF's UI thread.
//! No debugger socket, native input, or window activation is involved.
use super::{PointerAction, PointerUpdate};
use base64::Engine as _;
use cef::*;
use gpui::{BackgroundExecutor, FutureExt};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    time::{Duration, Instant},
};

static NEXT_REF: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static NEXT_ACK: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(1 << 30);

struct Recording {
    dir: std::path::PathBuf,
    t0: f64,
    size: (f64, f64, f64),
    script: Option<String>,
    frames: Vec<(std::path::PathBuf, f64)>,
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0., |d| d.as_secs_f64())
}

const MAX_RECORDING_FRAMES: usize = 18_000;

fn mkdir_private(dir: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn apply_quarantine(path: &std::path::Path) {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let Ok(c_path) = CString::new(path.as_os_str().as_bytes()) else { return };
    let Ok(c_name) = CString::new("com.apple.quarantine") else { return };
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let value = format!("0081;{secs:08x};Herdr Browser;");
    unsafe {
        libc::setxattr(
            c_path.as_ptr(),
            c_name.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
            0,
        );
    }
}

fn ffconcat_quote(path: &std::path::Path) -> String {
    path.display().to_string().replace('\'', "'\\''")
}

fn save_frame(recording: &mut Recording, params: &[u8], browser: Option<&mut Browser>) {
    let Ok(frame) = serde_json::from_slice::<Value>(params) else { return };
    if let Some(host) = browser.and_then(|b| b.host()) {
        let id = NEXT_ACK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let ack = json!({"id":id,"method":"Page.screencastFrameAck","params":{"sessionId":frame["sessionId"]}});
        host.send_dev_tools_message(Some(&serde_json::to_vec(&ack).unwrap_or_default()));
    }
    if recording.frames.len() >= MAX_RECORDING_FRAMES {
        return;
    }
    let (Some(data), Some(ts)) = (frame["data"].as_str(), frame["metadata"]["timestamp"].as_f64()) else { return };
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) else { return };
    let path = recording.dir.join("frames").join(format!("{:06}.jpg", recording.frames.len()));
    if std::fs::write(&path, bytes).is_ok() {
        recording.frames.push((path, ts));
    }
}

fn encode(recording: Recording, end: f64) -> Result<std::path::PathBuf, String> {
    let (width, height, scale) = recording.size;
    let events = json!([{"t":0,"type":"meta","width":width,"height":height,"scale":scale}]);
    std::fs::write(recording.dir.join("events.json"), serde_json::to_string_pretty(&events).unwrap_or_default())
        .map_err(|e| e.to_string())?;
    let at: Vec<f64> = recording.frames.iter().map(|(_, ts)| (ts - recording.t0).max(0.)).collect();
    let mut list = vec!["ffconcat version 1.0".to_string()];
    for (i, (file, _)) in recording.frames.iter().enumerate() {
        let from = if i == 0 { 0. } else { at[i] };
        let to = at.get(i + 1).copied().unwrap_or(end - recording.t0);
        list.push(format!("file '{}'", ffconcat_quote(file)));
        list.push(format!("duration {:.4}", (to - from).max(0.001)));
    }
    let last = recording.frames.last().ok_or("No frames were captured")?;
    list.push(format!("file '{}'", ffconcat_quote(&last.0)));
    let concat = recording.dir.join("frames.txt");
    std::fs::write(&concat, list.join("\n")).map_err(|e| e.to_string())?;
    let video = recording.dir.join("raw.mp4");
    let status = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "concat", "-safe", "0", "-i"])
        .arg(&concat)
        .args(["-vf", "fps=60,pad=ceil(iw/2)*2:ceil(ih/2)*2", "-c:v", "libx264", "-preset", "medium", "-crf", "12", "-pix_fmt", "yuv420p"])
        .arg(&video)
        .status()
        .map_err(|e| format!("ffmpeg is needed to encode recordings: {e}"))?;
    if !status.success() {
        return Err(format!("ffmpeg failed; frames kept in {}", recording.dir.display()));
    }
    let _ = std::fs::remove_dir_all(recording.dir.join("frames"));
    let _ = std::fs::remove_file(concat);
    Ok(video)
}

/// Roles an agent can act on: what `name` lookups and `filter: "interactive"` keep.
const INTERACTIVE: &[&str] = &[
    "button", "link", "textbox", "searchbox", "combobox", "checkbox", "radio", "switch",
    "tab", "menuitem", "menuitemcheckbox", "menuitemradio", "option", "slider",
    "spinbutton", "listbox", "treeitem",
];

/// Scroll offsets of every scroller under a point, innermost first, then the page.
/// `__POINT__` is `[x, y]` or `null` for the middle of the viewport. The elements stay
/// in the isolated world so the read after the wheel compares the same ones.
const SCROLLERS: &str = r#"(() => {
  const at = __POINT__ ?? [innerWidth / 2, innerHeight / 2];
  let e = document.elementFromPoint(at[0], at[1]);
  while (e?.shadowRoot) {
    const inner = e.shadowRoot.elementFromPoint(at[0], at[1]);
    if (!inner || inner === e) break;
    e = inner;
  }
  const frame = !!e?.matches?.('iframe,frame,object,embed');
  const chain = [];
  for (; e; e = e.parentElement || e.getRootNode?.()?.host || null) {
    if (e.scrollHeight <= e.clientHeight + 1 && e.scrollWidth <= e.clientWidth + 1) continue;
    const style = getComputedStyle(e);
    if (/auto|scroll|overlay/.test(style.overflowX + style.overflowY)) chain.push(e);
  }
  const page = document.scrollingElement;
  if (page && !chain.includes(page)) chain.push(page);
  const same = !frame && chain[0] === globalThis.herdrScrollers?.[0];
  globalThis.herdrScrollers = chain;
  return {at, frame, same, hidden: document.hidden, state: herdrScrollState()};
})()"#;
/// A page that is not on screen is not rendered, so Chromium never applies a wheel
/// there. Scroll the innermost scroller that can still move, as a wheel would chain.
const SCRIPT_SCROLL: &str = r#"(() => {
  const [left, top] = __DELTA__;
  for (const e of globalThis.herdrScrollers || []) {
    const x = e.scrollLeft, y = e.scrollTop;
    e.scrollBy({left, top, behavior: 'instant'});
    if (e.scrollLeft !== x || e.scrollTop !== y) break;
  }
  return herdrScrollState();
})()"#;
const SCROLL_STATE: &str = r#"globalThis.herdrScrollState = () => (globalThis.herdrScrollers || []).map(e =>
  [e.scrollLeft, e.scrollTop, Math.max(0, e.scrollWidth - e.clientWidth), Math.max(0, e.scrollHeight - e.clientHeight)]);"#;

/// True once the document has loaded, the images in view have arrived and the DOM
/// has been quiet for 400 ms. The observer lives in the isolated world per document.
const SETTLED: &str = r#"(() => {
  if (!globalThis.herdrSettle) {
    const s = globalThis.herdrSettle = {last: performance.now()};
    new MutationObserver(() => { s.last = performance.now(); })
      .observe(document, {subtree: true, childList: true, attributes: true, characterData: true});
  }
  const loading = [...document.images].some(i => {
    if (i.complete) return false;
    const r = i.getBoundingClientRect();
    return r.width > 0 && r.bottom > 0 && r.top < innerHeight && r.right > 0 && r.left < innerWidth;
  });
  return document.readyState === 'complete' && !loading && performance.now() - herdrSettle.last >= 400;
})()"#;

/// Chromium keeps a wheel gesture on the scroller it began on for half a second after
/// the last wheel; one aimed at another scroller inside that window is swallowed.
const WHEEL_LATCH: Duration = Duration::from_millis(700);

type Reply = async_channel::Sender<Result<Value, String>>;
wrap_dev_tools_message_observer! {
    struct Observer { pending: Rc<RefCell<HashMap<i32, Reply>>>, epoch: Rc<Cell<u64>>, recording: Rc<RefCell<Option<Recording>>> }
    impl DevToolsMessageObserver {
        fn on_dev_tools_method_result(&self, _browser: Option<&mut Browser>, id: i32, success: i32, result: Option<&[u8]>) {
            if let Some(tx) = self.pending.borrow_mut().remove(&id) {
                let value = serde_json::from_slice(result.unwrap_or(b"{}"));
                let answer = match value {
                    Ok(v) if success != 0 => Ok(v),
                    Ok(v) => Err(format!("Chromium: {v}")),
                    Err(e) => Err(format!("Invalid Chromium reply: {e}")),
                };
                let _ = tx.try_send(answer);
            }
        }
        fn on_dev_tools_event(&self, browser: Option<&mut Browser>, method: Option<&CefString>, params: Option<&[u8]>) {
            if method.is_some_and(|m| m.to_string() == "Page.screencastFrame") {
                if let (Some(recording), Some(params)) = (self.recording.borrow_mut().as_mut(), params) {
                    save_frame(recording, params, browser);
                }
                return;
            }
            if method.is_some_and(|m| matches!(m.to_string().as_str(),
                "DOM.documentUpdated" | "DOM.childNodeInserted" | "DOM.childNodeRemoved" |
                "DOM.attributeModified" | "DOM.attributeRemoved" | "DOM.characterDataModified" |
                "DOM.shadowRootPushed" | "DOM.shadowRootPopped" | "DOM.distributedNodesUpdated")) {
                self.epoch.set(self.epoch.get().wrapping_add(1));
            }
        }
        fn on_dev_tools_agent_detached(&self, _browser: Option<&mut Browser>) {
            for (_, tx) in self.pending.borrow_mut().drain() {
                let _ = tx.try_send(Err("Browser closed or DevTools detached; snapshot again".into()));
            }
        }
    }
}

#[derive(Default)]
struct Snapshot {
    generation: u64,
    version: Value,
    refs: HashMap<String, i64>,
}

pub struct Control {
    host: BrowserHost,
    pending: Rc<RefCell<HashMap<i32, Reply>>>,
    next_id: Cell<i32>,
    epoch: Rc<Cell<u64>>,
    _registration: Registration,
    snapshot: RefCell<Snapshot>,
    busy: Cell<bool>,
    pointer: Box<dyn Fn(PointerUpdate<'_>) -> bool>,
    caller: RefCell<Option<String>>,
    caller_label: RefCell<Option<String>>,
    cursor: fn(Option<&str>) -> Option<Vec<u8>>,
    idle_ms: fn() -> u64,
    on_target: Box<dyn Fn(String, bool)>,
    recording: Rc<RefCell<Option<Recording>>>,
    wheeled: Cell<Option<Instant>>,
}
struct Busy<'a>(&'a Cell<bool>);
impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

impl Control {
    pub fn new(
        browser: &Browser,
        pointer: Box<dyn Fn(PointerUpdate<'_>) -> bool>,
        cursor: fn(Option<&str>) -> Option<Vec<u8>>,
        idle_ms: fn() -> u64,
        on_target: Box<dyn Fn(String, bool)>,
    ) -> Result<Self, String> {
        let host = browser.host().ok_or("Browser is closed")?;
        let pending = Rc::new(RefCell::new(HashMap::new()));
        let epoch = Rc::new(Cell::new(0));
        let recording = Rc::new(RefCell::new(None));
        let mut observer = Observer::new(pending.clone(), epoch.clone(), recording.clone());
        let registration = host
            .add_dev_tools_message_observer(Some(&mut observer))
            .ok_or("Cannot observe Chromium")?;
        Ok(Self {
            host,
            pending,
            epoch,
            next_id: Cell::new(0),
            _registration: registration,
            snapshot: RefCell::new(Snapshot::default()),
            busy: Cell::new(false),
            pointer,
            caller: RefCell::new(None),
            caller_label: RefCell::new(None),
            cursor,
            idle_ms,
            on_target,
            recording,
            wheeled: Cell::new(None),
        })
    }

    fn emit_pointer(
        &self,
        pos: Option<(f64, f64)>,
        rect: Option<[f64; 4]>,
        action: PointerAction,
    ) -> bool {
        let caller = self.caller.borrow().clone();
        let label = self.caller_label.borrow().clone();
        (self.pointer)(PointerUpdate {
            caller: caller.as_deref(),
            label: label.as_deref(),
            pos,
            rect,
            action,
        })
    }

    pub async fn target_id(&self, ex: &BackgroundExecutor) -> Option<(String, bool)> {
        let info = self.call("Target.getTargetInfo", json!({}), ex).await.ok()?;
        let target = &info["targetInfo"];
        let id = target["targetId"].as_str()?.to_string();
        Some((id, target["type"] == "page"))
    }

    async fn call(
        &self,
        method: &str,
        params: Value,
        executor: &BackgroundExecutor,
    ) -> Result<Value, String> {
        self.call_within(method, params, Duration::from_secs(3), executor).await
    }

    async fn call_within(
        &self,
        method: &str,
        params: Value,
        wait: Duration,
        executor: &BackgroundExecutor,
    ) -> Result<Value, String> {
        let id = self
            .next_id
            .get()
            .checked_add(1)
            .ok_or("DevTools request limit reached")?;
        self.next_id.set(id);
        let (tx, rx) = async_channel::bounded(1);
        self.pending.borrow_mut().insert(id, tx);
        let message = serde_json::to_vec(&json!({"id":id,"method":method,"params":params}))
            .map_err(|e| e.to_string())?;
        if self.host.send_dev_tools_message(Some(&message)) == 0 {
            self.pending.borrow_mut().remove(&id);
            return Err("Chromium refused the command".into());
        }
        let answer = rx
            .recv()
            .with_timeout(wait, executor)
            .await;
        self.pending.borrow_mut().remove(&id);
        answer
            .map_err(|_| "Chromium command timed out; its outcome may be uncertain".to_string())?
            .map_err(|_| "Chromium observer closed".to_string())?
    }

    async fn context(&self, ex: &BackgroundExecutor) -> Result<i64, String> {
        let tree = self.call("Page.getFrameTree", json!({}), ex).await?;
        let frame = tree["frameTree"]["frame"]["id"]
            .as_str()
            .ok_or("No main frame")?;
        let world = self
            .call(
                "Page.createIsolatedWorld",
                json!({"frameId":frame,"worldName":"herdr-agent-control"}),
                ex,
            )
            .await?;
        world["executionContextId"]
            .as_i64()
            .ok_or("No isolated page context".into())
    }

    async fn evaluate(&self, expression: &str, ex: &BackgroundExecutor) -> Result<Value, String> {
        let context = self.context(ex).await?;
        let value = self
            .call(
                "Runtime.evaluate",
                json!({"expression":expression,"contextId":context,"returnByValue":true}),
                ex,
            )
            .await?;
        if value.get("exceptionDetails").is_some() {
            return Err("Page changed or script failed; snapshot again".into());
        }
        Ok(value["result"]["value"].clone())
    }

    async fn version(&self, ex: &BackgroundExecutor) -> Result<Value, String> {
        let context = self.context(ex).await?;
        let v = self.call("Runtime.evaluate", json!({"contextId":context,"returnByValue":true,"expression":r#"(() => {
            if (!globalThis.herdrVersion) {
                const state = globalThis.herdrVersion = {epoch: 0};
                state.observer = new MutationObserver(() => state.epoch++);
                state.observer.observe(document, {subtree:true, childList:true, attributes:true, characterData:true});
                document.addEventListener('input', () => state.epoch++, true);
                document.addEventListener('change', () => state.epoch++, true);
            }
            return {epoch:herdrVersion.epoch, url:location.href, title:document.title};
        })()"#}), ex).await?;
        if v.get("exceptionDetails").is_some() {
            return Err("Page changed; snapshot again".into());
        }
        Ok(json!({"context":context,"page":v["result"]["value"],"dom_epoch":self.epoch.get()}))
    }

    async fn node(&self, args: &Value, ex: &BackgroundExecutor) -> Result<i64, String> {
        let Some(reference) = args.get("ref").and_then(Value::as_str) else {
            let name = args
                .get("name")
                .and_then(Value::as_str)
                .ok_or("Pass ref (from a snapshot) or name")?;
            let role = args.get("role").and_then(Value::as_str);
            return self.find(name, role, args.get("nth").and_then(Value::as_u64), ex).await;
        };
        let (id, url) = {
            let s = self.snapshot.borrow();
            (
                *s.refs
                    .get(reference)
                    .ok_or("Unknown or stale ref; snapshot again")?,
                s.version["page"]["url"].clone(),
            )
        };
        if self.version(ex).await?["page"]["url"] != url {
            return Err("Page navigated; snapshot again".into());
        }
        self.connected(id, ex).await?;
        Ok(id)
    }

    async fn connected(&self, node: i64, ex: &BackgroundExecutor) -> Result<(), String> {
        let target = self
            .call(
                "DOM.resolveNode",
                json!({"backendNodeId":node,"executionContextId":self.context(ex).await?}),
                ex,
            )
            .await
            .map_err(|_| "Element disappeared; snapshot again")?;
        let object = target["object"]["objectId"]
            .as_str()
            .ok_or("Element disappeared; snapshot again")?;
        let live = self
            .call(
                "Runtime.callFunctionOn",
                json!({"objectId":object,"functionDeclaration":"function() { return this.isConnected; }","returnByValue":true}),
                ex,
            )
            .await?;
        self.call("Runtime.releaseObject", json!({"objectId":object}), ex)
            .await?;
        if live["result"]["value"] != true {
            return Err("Element disappeared; snapshot again".into());
        }
        Ok(())
    }

    async fn find(
        &self,
        name: &str,
        role: Option<&str>,
        nth: Option<u64>,
        ex: &BackgroundExecutor,
    ) -> Result<i64, String> {
        self.call("Accessibility.enable", json!({}), ex).await?;
        let tree = self
            .call("Accessibility.getFullAXTree", json!({}), ex)
            .await?;
        let want = name.trim();
        let matches: Vec<(&str, i64)> = tree["nodes"]
            .as_array()
            .ok_or("No accessibility tree")?
            .iter()
            .filter(|n| n["ignored"] != true)
            .filter_map(|n| {
                let r = n["role"]["value"].as_str()?;
                let found = n["name"]["value"].as_str()?.trim() == want
                    && role.map_or(INTERACTIVE.contains(&r), |w| w.eq_ignore_ascii_case(r));
                Some((r, n["backendDOMNodeId"].as_i64()?)).filter(|_| found)
            })
            .collect();
        match (matches.len(), nth) {
            (0, _) => Err(format!(
                "No {} named {want:?}; snapshot to see what is there",
                role.unwrap_or("interactive element")
            )),
            (count, Some(n)) => matches
                .get((n as usize).wrapping_sub(1))
                .map(|m| m.1)
                .ok_or_else(|| format!("nth is 1-based; {count} elements match {want:?}")),
            (1, None) => Ok(matches[0].1),
            (count, None) => Err(format!(
                "{count} elements named {want:?} ({}); pass role or nth",
                matches.iter().map(|m| m.0).collect::<Vec<_>>().join(", ")
            )),
        }
    }

    async fn field(&self, node: i64, ex: &BackgroundExecutor) -> Result<(), String> {
        let v = self
            .call("DOM.describeNode", json!({"backendNodeId":node}), ex)
            .await?;
        let attrs = v["node"]["attributes"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if attrs.chunks(2).any(|pair| {
            pair.len() == 2
                && pair[0]
                    .as_str()
                    .is_some_and(|s| s.eq_ignore_ascii_case("type"))
                && pair[1]
                    .as_str()
                    .is_some_and(|s| s.eq_ignore_ascii_case("password"))
        }) {
            return Err(
                "Password fields are refused; use browser.signin with a 1Password item".into(),
            );
        }
        Ok(())
    }

    async fn editable_target(&self, node: i64, ex: &BackgroundExecutor) -> Result<(), String> {
        let target = self
            .call(
                "DOM.resolveNode",
                json!({"backendNodeId":node,"executionContextId":self.context(ex).await?}),
                ex,
            )
            .await?;
        let object = target["object"]["objectId"]
            .as_str()
            .ok_or("Element disappeared; snapshot again")?;
        let result = self
            .call(
                "Runtime.callFunctionOn",
                json!({"objectId":object,"functionDeclaration":r#"function() {
            let e = document.activeElement;
            while (e?.shadowRoot?.activeElement) e = e.shadowRoot.activeElement;
            return (this === e || this.isContentEditable && this.contains(e)) &&
                (this.matches('input:not([type=password]),textarea') || this.isContentEditable) &&
                !this.disabled && !this.readOnly;
        }"#,"returnByValue":true}),
                ex,
            )
            .await?;
        self.call("Runtime.releaseObject", json!({"objectId":object}), ex)
            .await?;
        if result["result"]["value"] != true {
            return Err(
                "Target must remain the focused editable non-password field; snapshot again".into(),
            );
        }
        Ok(())
    }

    async fn point_and_rect(
        &self,
        node: i64,
        ex: &BackgroundExecutor,
    ) -> Result<((f64, f64), Option<[f64; 4]>), String> {
        self.call(
            "DOM.scrollIntoViewIfNeeded",
            json!({"backendNodeId":node}),
            ex,
        )
        .await?;
        let boxes = self
            .call("DOM.getContentQuads", json!({"backendNodeId":node}), ex)
            .await?;
        let q = boxes["quads"][0]
            .as_array()
            .filter(|q| q.len() == 8)
            .ok_or("Element has no visible box; snapshot again")?;
        let mut x = 0.;
        let mut y = 0.;
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for pair in q.chunks(2) {
            let px = pair[0].as_f64().ok_or("Invalid element box")?;
            let py = pair[1].as_f64().ok_or("Invalid element box")?;
            x += px;
            y += py;
            min_x = min_x.min(px);
            min_y = min_y.min(py);
            max_x = max_x.max(px);
            max_y = max_y.max(py);
        }
        let rect = (min_x.is_finite()
            && min_y.is_finite()
            && max_x > min_x
            && max_y > min_y)
            .then_some([min_x, min_y, max_x - min_x, max_y - min_y]);
        Ok(((x / 4., y / 4.), rect))
    }

    async fn active_element_target(&self, ex: &BackgroundExecutor) -> Option<((f64, f64), [f64; 4])> {
        let val = self
            .evaluate(
                "(() => { let e = document.activeElement; while (e?.shadowRoot?.activeElement) e = e.shadowRoot.activeElement; if (!e || e === document.body || e === document.documentElement) return null; const r = e.getBoundingClientRect(); return (r.width >= 1 && r.height >= 1) ? [r.left, r.top, r.width, r.height] : null; })()",
                ex,
            )
            .await
            .ok()?;
        let arr = val.as_array()?;
        if arr.len() != 4 {
            return None;
        }
        let rect = [
            arr[0].as_f64()?,
            arr[1].as_f64()?,
            arr[2].as_f64()?,
            arr[3].as_f64()?,
        ];
        if !rect.iter().all(|v| v.is_finite()) {
            return None;
        }
        Some(((rect[0] + rect[2] / 2., rect[1] + rect[3] / 2.), rect))
    }

    async fn click(&self, node: i64, ex: &BackgroundExecutor) -> Result<(), String> {
        let ((x, y), rect) = self.point_and_rect(node, ex).await?;
        // Hit-test after scrolling; never click a covering element by accident.
        let hit = self
            .call(
                "DOM.getNodeForLocation",
                json!({"x":x as i64,"y":y as i64}),
                ex,
            )
            .await?;
        let target = self
            .call(
                "DOM.resolveNode",
                json!({"backendNodeId":node,"executionContextId":self.context(ex).await?}),
                ex,
            )
            .await?;
        let object = target["object"]["objectId"]
            .as_str()
            .ok_or("Element disappeared; snapshot again")?;
        let hit_node = hit["backendNodeId"]
            .as_i64()
            .ok_or("Element is not visible")?;
        let hit_object = self
            .call(
                "DOM.resolveNode",
                json!({"backendNodeId":hit_node,"executionContextId":self.context(ex).await?}),
                ex,
            )
            .await?;
        let object_id = hit_object["object"]["objectId"]
            .as_str()
            .ok_or("Element disappeared")?;
        let contains = self.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function(other) { return this === other || this.contains(other); }","arguments":[{"objectId":object_id}],"returnByValue":true}), ex).await?;
        self.call("Runtime.releaseObject", json!({"objectId":object}), ex)
            .await?;
        self.call("Runtime.releaseObject", json!({"objectId":object_id}), ex)
            .await?;
        if contains["result"]["value"] != true {
            return Err("Element is covered; snapshot again".into());
        }
        let caller = self.caller.borrow().clone();
        if self.recording.borrow().is_some()
            && let Some(bytes) = (self.cursor)(caller.as_deref())
        {
            let svg = String::from_utf8_lossy(&bytes).into_owned();
            let expression = format!("globalThis.__herdrRecordAs?.({}, {})", json!(caller.as_deref().unwrap_or_default()), json!(svg));
            let _ = self.evaluate(&expression, ex).await;
        }
        if self.emit_pointer(Some((x, y)), rect, PointerAction::Click) {
            ex.timer(Duration::from_millis(350)).await;
        }
        for kind in ["mousePressed", "mouseReleased"] {
            self.call(
                "Input.dispatchMouseEvent",
                json!({"type":kind,"x":x,"y":y,"button":"left","clickCount":1}),
                ex,
            )
            .await?;
        }
        Ok(())
    }

    /// Wheels at `point` (default: the middle of the viewport, which is over the
    /// content on pages with fixed chrome at the edges) and reports what moved.
    async fn wheel(
        &self,
        point: Option<(f64, f64)>,
        rect: Option<[f64; 4]>,
        dx: f64,
        dy: f64,
        via: Option<&str>,
        ex: &BackgroundExecutor,
    ) -> Result<Value, String> {
        let point = point.map_or(Value::Null, |(x, y)| json!([x, y]));
        let script = format!("{SCROLL_STATE}\n{}", SCROLLERS.replace("__POINT__", &point.to_string()));
        let before = self.evaluate(&script, ex).await?;
        let (x, y) = (
            before["at"][0].as_f64().ok_or("Page has no viewport")?,
            before["at"][1].as_f64().ok_or("Page has no viewport")?,
        );
        let caller = self.caller.borrow().clone();
        if self.recording.borrow().is_some()
            && let Some(bytes) = (self.cursor)(caller.as_deref())
        {
            let svg = String::from_utf8_lossy(&bytes).into_owned();
            let expression = format!("globalThis.__herdrRecordAs?.({}, {})", json!(caller.as_deref().unwrap_or_default()), json!(svg));
            let _ = self.evaluate(&expression, ex).await;
        }
        self.emit_pointer(Some((x, y)), rect, PointerAction::Scroll);
        // Unasked, a page Chromium is not rendering gets the script: a wheel would stall there.
        let script = via.map_or(before["hidden"] == true, |via| via == "script");
        if script && before["frame"] != true {
            let source = SCRIPT_SCROLL.replace("__DELTA__", &json!([dx, dy]).to_string());
            let after = self.evaluate(&source, ex).await?;
            let mut report = scroll_report(&before["state"], &after, dx, dy);
            report["via"] = "script".into();
            return Ok(report);
        }
        if before["same"] != true {
            if let Some(left) = self.wheeled.get().and_then(|at| WHEEL_LATCH.checked_sub(at.elapsed())) {
                ex.timer(left).await;
            }
        }
        // A busy page can acknowledge late; what happened is measured below either way.
        match self
            .call(
                "Input.dispatchMouseEvent",
                json!({"type":"mouseWheel","x":x,"y":y,"deltaX":dx,"deltaY":dy}),
                ex,
            )
            .await
        {
            Err(error) if !error.contains("timed out") => return Err(error),
            _ => {}
        }
        self.wheeled.set(Some(Instant::now()));
        if before["frame"] == true {
            // The wheel went into another document; its offsets are not ours to read.
            return Ok(json!({"ok":true,"measured":false}));
        }
        // Chromium applies the wheel over the next frames; read until it rests. With
        // room to move that way, give a slow page longer before calling it unmoved.
        let room = before["state"].as_array().is_some_and(|scrollers| {
            scrollers.iter().any(|s| {
                let (left, top, max_left, max_top) = (number(s, 0), number(s, 1), number(s, 2), number(s, 3));
                dy > 0. && top < max_top - 1. || dy < 0. && top > 0. || dx > 0. && left < max_left - 1. || dx < 0. && left > 0.
            })
        });
        let idle = Duration::from_millis(if room { 1500 } else { 300 });
        let mut after = before["state"].clone();
        let started = Instant::now();
        loop {
            ex.timer(Duration::from_millis(50)).await;
            let Ok(now) = self.evaluate("globalThis.herdrScrollState?.() ?? null", ex).await else {
                return Ok(json!({"ok":true,"measured":false}));
            };
            let waited = started.elapsed();
            let rested = now == after && (now != before["state"] || waited >= idle);
            after = now;
            if rested || waited >= Duration::from_millis(3000) {
                break;
            }
        }
        let mut report = scroll_report(&before["state"], &after, dx, dy);
        report["via"] = "wheel".into();
        Ok(report)
    }

    async fn record_start(&self, ex: &BackgroundExecutor) -> Result<Value, String> {
        if self.recording.borrow().is_some() {
            return Err("This pane is already recording".into());
        }
        let size = self.evaluate("[innerWidth, innerHeight, devicePixelRatio]", ex).await?;
        let number = |i: usize| size[i].as_f64().filter(|n| *n > 0.).ok_or("Page has no viewport");
        let size = (number(0)?, number(1)?, number(2)?);
        let t0 = now();
        let recordings_root = crate::mods::app_dir().join("recordings");
        mkdir_private(&recordings_root)?;
        let dir = recordings_root.join(format!("{}", (t0 * 1000.) as u64));
        mkdir_private(&dir.join("frames"))?;
        self.call("Page.enable", json!({}), ex).await?;
        let script = if let Some(bytes) = (self.cursor)(None) {
            let source = RECORD_CURSOR
                .replace("__SVG__", &serde_json::to_string(&String::from_utf8_lossy(&bytes)).unwrap_or_default())
                .replace("__IDLE_MS__", &(self.idle_ms)().to_string());
            let id = self
                .call("Page.addScriptToEvaluateOnNewDocument", json!({"source":source,"worldName":"herdr-agent-control"}), ex)
                .await?["identifier"]
                .as_str()
                .map(str::to_string);
            self.evaluate(&source, ex).await?;
            id
        } else {
            None
        };
        *self.recording.borrow_mut() = Some(Recording { dir: dir.clone(), t0, size, script, frames: Vec::new() });
        let started = self
            .call("Page.startScreencast", json!({"format":"jpeg","quality":90,"maxWidth":(size.0 * size.2) as i64,"maxHeight":(size.1 * size.2) as i64,"everyNthFrame":1}), ex)
            .await;
        if let Err(error) = started {
            self.recording.borrow_mut().take();
            return Err(error);
        }
        Ok(json!({"recording":true,"dir":dir}))
    }

    async fn record_stop(&self, ex: &BackgroundExecutor) -> Result<Value, String> {
        let recording = self.recording.borrow_mut().take().ok_or("This pane is not recording")?;
        let end = now();
        let _ = self.call("Page.stopScreencast", json!({}), ex).await;
        if let Some(id) = &recording.script {
            let _ = self.call("Page.removeScriptToEvaluateOnNewDocument", json!({"identifier":id}), ex).await;
        }
        let _ = self.evaluate("globalThis.__herdrRecordStop?.()", ex).await;
        let dir = recording.dir.clone();
        let frames = recording.frames.len();
        if frames == 0 {
            return Err("No frames were captured; is the pane visible?".into());
        }
        ex.spawn(async move {
            match encode(recording, end) {
                Ok(video) => {
                    let _ = std::process::Command::new("open").arg("-R").arg(video).status();
                }
                Err(error) => tracing::warn!("browser recording: {error}"),
            }
        })
        .detach();
        Ok(json!({"recording":false,"dir":dir,"video":dir.join("raw.mp4"),"frames":frames,"encoding":true}))
    }

    async fn press(&self, key: &str, ex: &BackgroundExecutor) -> Result<(), String> {
        if self.evaluate("(() => { let e=document.activeElement; while(e?.shadowRoot?.activeElement) e=e.shadowRoot.activeElement; return !!e?.matches('input[type=password],iframe,frame,object,embed'); })()", ex).await? == true {
            return Err("Password fields are refused; use browser.signin".into());
        }
        let tree = self
            .call("Accessibility.getFullAXTree", json!({}), ex)
            .await?;
        if let Some(nodes) = tree["nodes"].as_array() {
            for node in nodes {
                let focused = node["properties"].as_array().is_some_and(|properties| {
                    properties
                        .iter()
                        .any(|p| p["name"] == "focused" && p["value"]["value"] == true)
                });
                if focused {
                    if let Some(id) = node["backendDOMNodeId"].as_i64() {
                        self.field(id, ex).await?;
                    }
                }
            }
        }
        self.send_key(key, ex).await
    }

    async fn send_key(&self, key: &str, ex: &BackgroundExecutor) -> Result<(), String> {
        let mut modifiers = 0;
        let parts: Vec<_> = key.split('+').collect();
        for part in &parts[..parts.len() - 1] {
            modifiers |= match part.to_ascii_lowercase().as_str() {
                "alt" => 1,
                "ctrl" | "control" => 2,
                "meta" | "cmd" => 4,
                "shift" => 8,
                _ => return Err("Unknown key modifier".into()),
            };
        }
        let k = *parts.last().unwrap_or(&"");
        let (key, code, vk): (String, String, i32) = match k.to_ascii_lowercase().as_str() {
            "enter" => ("Enter".into(),"Enter".into(),13), "tab" => ("Tab".into(),"Tab".into(),9),
            "escape"|"esc" => ("Escape".into(),"Escape".into(),27), "backspace" => ("Backspace".into(),"Backspace".into(),8),
            "delete" => ("Delete".into(),"Delete".into(),46), "arrowleft" => ("ArrowLeft".into(),"ArrowLeft".into(),37),
            "arrowup" => ("ArrowUp".into(),"ArrowUp".into(),38), "arrowright" => ("ArrowRight".into(),"ArrowRight".into(),39),
            "arrowdown" => ("ArrowDown".into(),"ArrowDown".into(),40), "home" => ("Home".into(),"Home".into(),36), "end" => ("End".into(),"End".into(),35),
            "pageup" => ("PageUp".into(),"PageUp".into(),33), "pagedown" => ("PageDown".into(),"PageDown".into(),34),
            "space" => (" ".into(),"Space".into(),32),
            _ if k.chars().count()==1 && k.is_ascii() => (k.to_string(), format!("Key{}",k.to_ascii_uppercase()),k.as_bytes()[0].to_ascii_uppercase() as i32),
            _ => return Err("Unsupported key; use Enter, Tab, Escape, arrows, or an ASCII character with optional Ctrl/Alt/Meta/Shift".into()),
        };
        let text = if modifiers & !8 == 0 && key.len() == 1 {
            key.clone()
        } else if key == "Enter" {
            "\r".into()
        } else {
            String::new()
        };
        let commands: Vec<&str> = if modifiers == 4 && key.eq_ignore_ascii_case("a") {
            vec!["selectAll"]
        } else {
            vec![]
        };
        self.call("Input.dispatchKeyEvent", json!({"type":"keyDown","key":key,"code":code,"windowsVirtualKeyCode":vk,"modifiers":modifiers,"text":text,"commands":commands}), ex).await?;
        self.call("Input.dispatchKeyEvent", json!({"type":"keyUp","key":key,"code":code,"windowsVirtualKeyCode":vk,"modifiers":modifiers}), ex).await?;
        Ok(())
    }

    async fn page_url(&self, ex: &BackgroundExecutor) -> Result<url::Url, String> {
        let page = self.version(ex).await?;
        let url = url::Url::parse(page["page"]["url"].as_str().unwrap_or(""))
            .map_err(|_| "Sign-in needs a web page")?;
        if !matches!(url.scheme(), "https" | "http") {
            return Err("Sign-in needs an http(s) page".into());
        }
        if url.host_str().is_none() {
            return Err("Sign-in needs a page with a host".into());
        }
        Ok(url)
    }

    async fn login_field(&self, password: bool, ex: &BackgroundExecutor) -> Result<Option<i64>, String> {
        let selectors = if password {
            json!(["input[type=password]"])
        } else {
            json!([
                "input[autocomplete~=username]", "input[type=email]", "input[name=identifier]",
                "input[name*=user i]", "input[id*=user i]", "input[name*=login i]", "input[name*=email i]"
            ])
        };
        self.call("DOM.enable", json!({}), ex).await?;
        let context = self.context(ex).await?;
        let found = self.call("Runtime.evaluate", json!({"contextId":context,"returnByValue":false,"expression":format!(
            "(() => {{ for (const s of {selectors}) for (const e of document.querySelectorAll(s)) if (e.getClientRects().length && !e.disabled && !e.readOnly) return e; return null; }})()"
        )}), ex).await?;
        let Some(object) = found["result"]["objectId"].as_str() else {
            return Ok(None);
        };
        let node = self
            .call("DOM.describeNode", json!({"objectId":object}), ex)
            .await;
        self.call("Runtime.releaseObject", json!({"objectId":object}), ex)
            .await?;
        Ok(node?["node"]["backendNodeId"].as_i64())
    }

    /// Checks the site both before clicking and again immediately before Input.insertText,
    /// so a redirect or focus hijack during the click animation never receives credentials.
    async fn fill(&self, node: i64, sites: &[SiteRule], value: &str, ex: &BackgroundExecutor) -> Result<(), String> {
        let before = self.page_url(ex).await?;
        if !listed_url(sites, &before) {
            return Err(format!("The credentials do not allow {}; refusing to fill", before.origin().ascii_serialization()));
        }
        if value.is_empty() {
            return Err("The credentials have an empty value for this field".into());
        }
        self.click(node, ex).await?;
        let target = self
            .call(
                "DOM.resolveNode",
                json!({"backendNodeId":node,"executionContextId":self.context(ex).await?}),
                ex,
            )
            .await?;
        let object = target["object"]["objectId"]
            .as_str()
            .ok_or("Login field disappeared")?;
        let focused = self.call("Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":r#"function() {
            this.focus();
            this.select?.();
            let e = document.activeElement;
            while (e?.shadowRoot?.activeElement) e = e.shadowRoot.activeElement;
            return this === e && !this.disabled && !this.readOnly;
        }"#,"returnByValue":true}), ex)
            .await?;
        self.call("Runtime.releaseObject", json!({"objectId":object}), ex)
            .await?;
        if focused["result"]["value"] != true {
            return Err("Login field lost focus before credentials could be filled".into());
        }
        let after = self.page_url(ex).await?;
        if !listed_url(sites, &after) {
            return Err(format!("Page changed to {} before filling; refusing to fill", after.origin().ascii_serialization()));
        }
        self.call("Input.insertText", json!({"text":value}), ex).await?;
        Ok(())
    }

    pub async fn run(
        &self,
        command: &str,
        args: &Value,
        ex: &BackgroundExecutor,
    ) -> Result<Value, String> {
        if self.busy.replace(true) {
            return Err("Another command is running in this pane; wait for its reply".into());
        }
        let _busy = Busy(&self.busy);
        *self.caller.borrow_mut() = args["caller"].as_str().map(str::to_owned);
        *self.caller_label.borrow_mut() = args["caller_label"].as_str().map(str::to_owned);
        if matches!(
            command,
            "browser.snapshot"
                | "browser.eval"
                | "browser.download"
                | "browser.read"
                | "browser.wait"
                | "browser.screenshot"
                | "browser.cdp"
        ) {
            self.emit_pointer(None, None, PointerAction::Activity);
        }
        match command {
            "browser.record" => match args["action"].as_str() {
                Some("start") => self.record_start(ex).await,
                Some("stop") => self.record_stop(ex).await,
                _ => Err("action must be start or stop".into()),
            },
            "browser.cdp" => {
                // The pane's own target names the agent profile the bridge exposes.
                let info = self.call("Target.getTargetInfo", json!({}), ex).await?;
                let target = &info["targetInfo"];
                if let Some(id) = target["targetId"].as_str() {
                    (self.on_target)(id.to_string(), target["type"] == "page");
                }
                let context = target["browserContextId"]
                    .as_str()
                    .ok_or("Chromium did not name this page's profile")?;
                let caller = self.caller.borrow().clone();
                let label = self.caller_label.borrow().clone();
                let endpoint = super::cdp::endpoint(
                    context,
                    args["pane"].as_str(),
                    caller.as_deref(),
                    label.as_deref(),
                )?;
                Ok(json!({"endpoint":endpoint,"targetId":target["targetId"],"type":target["type"],"url":target["url"]}))
            }
            "browser.snapshot" => {
                // A failed snapshot invalidates all previous refs, too.
                self.snapshot.borrow_mut().refs.clear();
                let interactive = match args.get("filter").map(Value::as_str) {
                    None => false,
                    Some(Some("interactive")) => true,
                    Some(Some("all")) => false,
                    _ => return Err("filter must be interactive or all".into()),
                };
                self.call("DOM.enable", json!({}), ex).await?;
                // Request all descendants so native DOM events also cover closed
                // shadow roots and frame documents, outside a document observer.
                self.call("DOM.getDocument", json!({"depth":-1,"pierce":true}), ex)
                    .await?;
                let before = self.version(ex).await?;
                self.call("Accessibility.enable", json!({}), ex).await?;
                let tree = self
                    .call("Accessibility.getFullAXTree", json!({}), ex)
                    .await?;
                let nodes = tree["nodes"].as_array().ok_or("No accessibility tree")?;
                let mut scope_id = None;
                if let Some(scope) = args.get("scope").and_then(Value::as_str) {
                    let doc = self.call("DOM.getDocument", json!({}), ex).await?;
                    let found = self
                        .call(
                            "DOM.querySelector",
                            json!({"nodeId":doc["root"]["nodeId"],"selector":scope}),
                            ex,
                        )
                        .await?;
                    // No match is node 0, which describeNode would report as a missing id.
                    if found["nodeId"].as_i64().unwrap_or(0) == 0 {
                        return Err(format!("No element matches scope {scope:?}"));
                    }
                    let node = self
                        .call("DOM.describeNode", json!({"nodeId":found["nodeId"]}), ex)
                        .await?;
                    let backend = node["node"]["backendNodeId"]
                        .as_i64()
                        .ok_or("Scope did not match an element")?;
                    scope_id = Some(
                        nodes
                            .iter()
                            .find(|n| n["backendDOMNodeId"].as_i64() == Some(backend))
                            .and_then(|n| n["nodeId"].as_str())
                            .ok_or("Scope is absent from the accessibility tree")?
                            .to_string(),
                    );
                }
                let after = self.version(ex).await?;
                if before["page"]["url"] != after["page"]["url"] {
                    return Err("Page navigated while reading; snapshot again".into());
                }
                let by_id: HashMap<_, _> = nodes
                    .iter()
                    .filter_map(|n| Some((n["nodeId"].as_str()?, n)))
                    .collect();
                let mut pending = vec![
                    scope_id
                        .as_deref()
                        .or_else(|| nodes.first().and_then(|n| n["nodeId"].as_str()))
                        .ok_or("Empty accessibility tree")?,
                ];
                let mut lines = Vec::new();
                let mut seen = std::collections::HashSet::new();
                let origin = url::Url::parse(after["page"]["url"].as_str().unwrap_or(""))
                    .map(|u| u.origin().ascii_serialization())
                    .unwrap_or_default();
                let mut s = self.snapshot.borrow_mut();
                s.generation += 1;
                s.version = after.clone();
                while let Some(id) = pending.pop() {
                    if !seen.insert(id) {
                        continue;
                    }
                    let Some(n) = by_id.get(id) else {
                        continue;
                    };
                    if let Some(children) = n["childIds"].as_array() {
                        pending.extend(children.iter().rev().filter_map(Value::as_str));
                    }
                    if n["ignored"] == true {
                        continue;
                    }
                    let role = n["role"]["value"].as_str().unwrap_or("unknown");
                    // Inline boxes repeat their text node and cannot be acted on.
                    if role == "InlineTextBox" || interactive && !INTERACTIVE.contains(&role) {
                        continue;
                    }
                    let name: String = n["name"]["value"]
                        .as_str()
                        .unwrap_or("")
                        .chars()
                        .take(300)
                        .collect();
                    let reference = if let Some(backend) = n["backendDOMNodeId"].as_i64() {
                        let r = format!(
                            "e{}",
                            NEXT_REF.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                        );
                        s.refs.insert(r.clone(), backend);
                        r
                    } else {
                        "-".into()
                    };
                    // Where a link goes or an image comes from, relative on this site.
                    let target = n["properties"]
                        .as_array()
                        .and_then(|p| p.iter().find(|p| p["name"] == "url"))
                        .and_then(|p| p["value"]["value"].as_str())
                        .filter(|u| !u.is_empty())
                        .map(|u| {
                            let u = u.strip_prefix(&origin).filter(|rest| rest.starts_with('/')).unwrap_or(u);
                            let cut: String = u.chars().take(300).collect();
                            format!(" -> {cut}{}", if cut.len() < u.len() { "…" } else { "" })
                        })
                        .unwrap_or_default();
                    lines.push(format!(
                        "{reference} {role} {}{target}",
                        serde_json::to_string(&name).unwrap()
                    ));
                    if lines.len() >= 1000 {
                        break;
                    }
                }
                Ok(
                    json!({"url":after["page"]["url"],"title":after["page"]["title"],"generation":s.generation,"tree":lines.join("\n"),"capped":!pending.is_empty()}),
                )
            }
            "browser.eval" => {
                // The page's own world, like its console: page variables, top-level await.
                // CLI values that parse as JSON arrive as JSON, which is a JS expression too.
                let js = match &args["js"] {
                    Value::String(js) => js.clone(),
                    Value::Null => return Err("js is required: the JavaScript to evaluate".into()),
                    other => format!("({other})"),
                };
                // Each call gets its own block: its let/const/class names neither leak into the
                // page's global scope (a person's real page, shared with other agents) nor clash
                // with an earlier call's (REPL mode refuses a const where a let was, and back).
                // A block still yields its last expression's value; `{` shares line 1 so error
                // line numbers stay the caller's.
                let js = format!("{{{js}\n}}");
                let timeout = match args.get("timeout") {
                    None => 5000,
                    Some(v) => v
                        .as_u64()
                        .filter(|v| (1..=120000).contains(v))
                        .ok_or("timeout must be 1..120000 milliseconds")?,
                };
                // Chromium's own timeout stops runaway synchronous code (it answers with an
                // internal error); ours also bounds a pending promise.
                let started = Instant::now();
                let value = self
                    .call_within(
                        "Runtime.evaluate",
                        json!({"expression":js,"replMode":true,"awaitPromise":true,"returnByValue":true,"timeout":timeout}),
                        Duration::from_millis(timeout + 1000),
                        ex,
                    )
                    .await
                    .map_err(|e| {
                        if started.elapsed() >= Duration::from_millis(timeout) {
                            format!("JavaScript did not finish within {timeout} ms")
                        } else {
                            e
                        }
                    })?;
                if let Some(thrown) = value.get("exceptionDetails") {
                    let detail = thrown["exception"]["description"]
                        .as_str()
                        .or(thrown["text"].as_str())
                        .unwrap_or("unknown error");
                    return Err(format!("JavaScript threw: {}", detail.chars().take(2000).collect::<String>()));
                }
                let result = &value["result"];
                // Values come back as JSON; what JSON cannot carry (a function, a symbol) as its description.
                let out = match result.get("value") {
                    Some(v) => v.clone(),
                    None => result["unserializableValue"]
                        .as_str()
                        .or(result["description"].as_str())
                        .map_or(Value::Null, |d| Value::String(d.into())),
                };
                if out.to_string().len() > EVAL_RESULT {
                    return Err(format!("Result exceeds {EVAL_RESULT} bytes; return less, e.g. slice or map to the fields needed"));
                }
                let page = self.version(ex).await.ok();
                Ok(json!({"result":out,"type":result["type"],"url":page.as_ref().map(|p| p["page"]["url"].clone())}))
            }
            "browser.download" => {
                let url = args["url"]
                    .as_str()
                    .filter(|u| !u.is_empty())
                    .ok_or("url is required: what to save")?;
                let timeout = match args.get("timeout") {
                    None => 30000,
                    Some(v) => v
                        .as_u64()
                        .filter(|v| (1..=120000).contains(v))
                        .ok_or("timeout must be 1..120000 milliseconds")?,
                };
                // The page fetches it, from Herdr's isolated world: the page's cookies and
                // sign-in, its CORS rules, relative and blob: URLs, and no page script can wrap
                // fetch there. The bytes cross DevTools as base64, as a screenshot's do.
                let context = self.context(ex).await?;
                let expression = format!("{DOWNLOAD}({}, {DOWNLOAD_BYTES})", Value::String(url.into()));
                let started = Instant::now();
                let value = self
                    .call_within(
                        "Runtime.evaluate",
                        json!({"expression":expression,"contextId":context,"awaitPromise":true,"returnByValue":true}),
                        Duration::from_millis(timeout),
                        ex,
                    )
                    .await
                    .map_err(|e| {
                        if started.elapsed() >= Duration::from_millis(timeout) {
                            format!("Download did not finish within {timeout} ms")
                        } else {
                            e
                        }
                    })?;
                if let Some(thrown) = value.get("exceptionDetails") {
                    let detail = thrown["exception"]["description"]
                        .as_str()
                        .or(thrown["text"].as_str())
                        .unwrap_or("unknown error");
                    let detail = detail.lines().next().unwrap_or(detail);
                    return Err(format!("The page could not fetch {url}: {detail}. A network error, or another site that does not let this page read it (CORS): open that URL in its own pane and download it there"));
                }
                let got = &value["result"]["value"];
                let status = got["status"].as_u64().unwrap_or(0);
                if got["ok"] != true {
                    return Err(format!("HTTP {status} for {url}; nothing saved"));
                }
                let size = got["bytes"].as_u64().unwrap_or(0);
                let encoded = got["data"].as_str().ok_or_else(|| {
                    format!("{url} is {size} bytes, over the {} MB download limit", DOWNLOAD_BYTES / 1024 / 1024)
                })?;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(encoded)
                    .map_err(|e| e.to_string())?;
                let kind = got["type"].as_str().unwrap_or("");
                let name = download_name(got["url"].as_str().unwrap_or(url), kind);
                let dir = crate::mods::app_dir().join("downloads");
                mkdir_private(&dir)?;
                use std::io::Write;
                let open = |path: &std::path::Path| {
                    std::fs::OpenOptions::new().write(true).create_new(true).open(path)
                };
                // Keep the site's name when it is free; never overwrite an earlier download.
                let mut path = dir.join(&name);
                let mut file = match open(&path) {
                    Ok(file) => file,
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                        let stamp = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_err(|e| e.to_string())?
                            .as_nanos();
                        path = dir.join(match name.rsplit_once('.') {
                            Some((stem, ext)) => format!("{stem}-{stamp}.{ext}"),
                            None => format!("{name}-{stamp}"),
                        });
                        open(&path).map_err(|e| e.to_string())?
                    }
                    Err(e) => return Err(e.to_string()),
                };
                file.write_all(&bytes).map_err(|e| e.to_string())?;
                #[cfg(target_os = "macos")]
                apply_quarantine(&path);
                Ok(json!({"path":path,"bytes":bytes.len(),"content_type":kind,"status":status,"url":got["url"]}))
            }
            "browser.read" => {
                let format = args["format"].as_str().unwrap_or("text");
                if !matches!(format, "text" | "markdown") {
                    return Err("format must be text or markdown".into());
                }
                // innerText includes only rendered text; no HTML, hidden inputs, or form values.
                // Markdown is deliberately plain text (valid Markdown), escaping syntax below.
                let mut result = self.evaluate("({url:location.href,title:document.title,text:(document.body?.innerText || '').slice(0,20000),capped:(document.body?.innerText.length || 0)>20000})", ex).await?;
                if format == "markdown" {
                    let text = result["text"].as_str().unwrap_or("");
                    let escaped: String = text
                        .chars()
                        .flat_map(|c| {
                            if "\\`*_{}[]<>#+-.!|".contains(c) {
                                vec!['\\', c]
                            } else {
                                vec![c]
                            }
                        })
                        .collect();
                    result["text"] = escaped.into();
                }
                result["format"] = format.into();
                Ok(result)
            }
            "browser.click" => {
                let node = self.node(args, ex).await?;
                self.click(node, ex).await?;
                Ok(json!({"ok":true,"url":self.version(ex).await?["page"]["url"]}))
            }
            "browser.type" => {
                let text = args["text"].as_str().ok_or("text is required")?;
                if text.len() > 20000 {
                    return Err("text exceeds 20000 bytes".into());
                }
                let node = self.node(args, ex).await?;
                self.field(node, ex).await?;
                self.click(node, ex).await?;
                self.editable_target(node, ex).await?;
                self.press("Meta+a", ex).await?;
                self.editable_target(node, ex).await?;
                self.call("Input.insertText", json!({"text":text}), ex)
                    .await?;
                self.emit_pointer(None, None, PointerAction::Activity);
                if args["submit"] == true {
                    self.press("Enter", ex).await?;
                }
                Ok(json!({"ok":true}))
            }
            "browser.signin" => {
                self.page_url(ex).await?;
                let login = login(args, ex).await?;
                let mut filled = Vec::new();
                if let Some(field) = self.login_field(false, ex).await? {
                    self.fill(field, &login.sites, &login.username, ex).await?;
                    filled.push("username");
                }
                let mut password = self.login_field(true, ex).await?;
                if password.is_none() && !filled.is_empty() {
                    self.send_key("Enter", ex).await?;
                    let deadline = Instant::now() + Duration::from_secs(15);
                    while password.is_none() && Instant::now() < deadline {
                        ex.timer(Duration::from_millis(250)).await;
                        password = self.login_field(true, ex).await.unwrap_or(None);
                    }
                }
                match password {
                    Some(field) => {
                        self.fill(field, &login.sites, &login.password, ex).await?;
                        filled.push("password");
                        self.send_key("Enter", ex).await?;
                    }
                    None if filled.is_empty() => {
                        return Err("No visible login fields on this page".into());
                    }
                    None => {}
                }
                Ok(json!({"ok":true,"filled":filled,"url":self.version(ex).await?["page"]["url"]}))
            }
            "browser.press" => {
                if let Some((pos, rect)) = self.active_element_target(ex).await {
                    self.emit_pointer(Some(pos), Some(rect), PointerAction::Activity);
                } else {
                    self.emit_pointer(None, None, PointerAction::Activity);
                }
                self.press(args["key"].as_str().ok_or("key is required")?, ex)
                    .await?;
                Ok(json!({"ok":true}))
            }
            "browser.scroll" => {
                let element = args.get("ref").is_some() || args.get("name").is_some();
                let delta = |key: &str| match args.get(key) {
                    None => Ok(0.),
                    Some(v) => v
                        .as_f64()
                        .filter(|n| n.is_finite() && n.abs() <= 100000.)
                        .ok_or(format!("{key} must be a finite number within 100000 pixels")),
                };
                let (dx, dy) = (delta("dx")?, delta("dy")?);
                let wheel = args.get("dx").is_some() || args.get("dy").is_some();
                let via = match args.get("via").map(Value::as_str) {
                    None => None,
                    Some(Some(via @ ("wheel" | "script"))) if wheel => Some(via),
                    Some(Some("wheel" | "script")) => return Err("via applies to a dy/dx scroll".into()),
                    _ => return Err("via must be wheel or script".into()),
                };
                if !element && !wheel {
                    return Err("Pass ref or name to bring an element into view, dy/dx to wheel the page, or both to wheel over that element".into());
                }
                if !wheel {
                    let node = self.node(args, ex).await?;
                    if let Ok((pos, rect)) = self.point_and_rect(node, ex).await {
                        self.emit_pointer(Some(pos), rect, PointerAction::Scroll);
                    } else {
                        self.call(
                            "DOM.scrollIntoViewIfNeeded",
                            json!({"backendNodeId":node}),
                            ex,
                        )
                        .await?;
                        self.emit_pointer(None, None, PointerAction::Scroll);
                    }
                    return Ok(json!({"ok":true}));
                }
                let (point, rect) = if element {
                    let (pos, rect) = self.point_and_rect(self.node(args, ex).await?, ex).await?;
                    (Some(pos), rect)
                } else {
                    (None, None)
                };
                self.wheel(point, rect, dx, dy, via, ex).await
            }
            "browser.wait" => {
                let count = ["text", "url", "load", "settled"]
                    .iter()
                    .filter(|k| args.get(**k).is_some())
                    .count();
                if count != 1 {
                    return Err("Pass exactly one of text, url, load, or settled".into());
                }
                if args.get("text").is_some_and(|v| !v.is_string())
                    || args.get("url").is_some_and(|v| !v.is_string())
                    || args.get("load").is_some_and(|v| v != true)
                    || args.get("settled").is_some_and(|v| v != true)
                {
                    return Err("text/url must be strings; load and settled must be true".into());
                }
                let timeout = match args.get("timeout") {
                    None => 5000,
                    Some(v) => v
                        .as_u64()
                        .filter(|v| *v <= 120000)
                        .ok_or("timeout must be 0..120000 milliseconds")?,
                };
                let deadline = Instant::now() + Duration::from_millis(timeout);
                loop {
                    let condition = if let Some(text) = args.get("text") {
                        format!("(document.body?.innerText || '').includes({text})")
                    } else if let Some(url) = args.get("url") {
                        format!("location.href.includes({url})")
                    } else if args.get("settled").is_some() {
                        SETTLED.into()
                    } else {
                        "document.readyState === 'complete'".into()
                    };
                    // A navigation can destroy a context between polling reads.
                    // Only reads are retried; input commands are never replayed.
                    if self.evaluate(&condition, ex).await.is_ok_and(|v| v == true) {
                        return Ok(json!({"ok":true}));
                    }
                    if Instant::now() >= deadline {
                        return Ok(json!({"ok":false,"timeout":true}));
                    }
                    ex.timer(Duration::from_millis(100)).await;
                }
            }
            "browser.screenshot" => {
                let scale = match args.get("scale") {
                    None => 1.,
                    Some(v) => v
                        .as_f64()
                        .filter(|s| (0.1..=1.).contains(s))
                        .ok_or("scale must be a number from 0.1 to 1")?,
                };
                let format = match args.get("format").map(Value::as_str) {
                    None | Some(Some("png")) => "png",
                    Some(Some("jpeg")) => "jpeg",
                    _ => return Err("format must be png or jpeg".into()),
                };
                let full = match args.get("full") {
                    None => false,
                    Some(Value::Bool(full)) => *full,
                    _ => return Err("full must be true or false".into()),
                };
                let mut params = json!({"format":format,"captureBeyondViewport":full});
                if format == "jpeg" {
                    params["quality"] = 85.into();
                }
                let mut capped = false;
                if full || scale != 1. {
                    // A clip is in page coordinates; its scale is relative to device pixels.
                    let metrics = self.call("Page.getLayoutMetrics", json!({}), ex).await?;
                    let number = |v: &Value| v.as_f64().filter(|n| n.is_finite()).ok_or("Page has no layout");
                    let view = &metrics["cssVisualViewport"];
                    let (x, y, width, mut height) = if full {
                        let content = &metrics["cssContentSize"];
                        (0., 0., number(&content["width"])?, number(&content["height"])?)
                    } else {
                        (number(&view["pageX"])?, number(&view["pageY"])?, number(&view["clientWidth"])?, number(&view["clientHeight"])?)
                    };
                    if height > FULL_HEIGHT {
                        (height, capped) = (FULL_HEIGHT, true);
                    }
                    params["clip"] = json!({"x":x,"y":y,"width":width,"height":height,"scale":scale});
                }
                let shot = self.call("Page.captureScreenshot", params, ex).await?;
                let encoded = shot["data"]
                    .as_str()
                    .filter(|s| s.len() <= 48 * 1024 * 1024)
                    .ok_or("No screenshot or capture too large")?;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(encoded)
                    .map_err(|e| e.to_string())?;
                let dir = crate::mods::app_dir().join("screenshots");
                mkdir_private(&dir)?;
                let path = dir.join(format!(
                    "browser-{}-{}.{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(|e| e.to_string())?
                        .as_nanos(),
                    if format == "jpeg" { "jpg" } else { "png" }
                ));
                use std::io::Write;
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .map_err(|e| e.to_string())?;
                file.write_all(&bytes).map_err(|e| e.to_string())?;
                let mut result = json!({"path":path});
                if capped {
                    result["capped"] = true.into();
                }
                Ok(result)
            }
            _ => Err(format!("Unknown browser command: {command}")),
        }
    }
}

fn number(v: &Value, i: usize) -> f64 {
    v[i].as_f64().unwrap_or(0.)
}

/// What a scroll did, from each scroller's `[left, top, max_left, max_top]` before and after.
fn scroll_report(before: &Value, after: &Value, dx: f64, dy: f64) -> Value {
    let pairs: Vec<(&Value, &Value)> = match (before.as_array(), after.as_array()) {
        (Some(was), Some(is)) if was.len() == is.len() => was.iter().zip(is).collect(),
        _ => return json!({"ok":true,"measured":false}),
    };
    // Report the scroller that moved; when none did, the innermost one under the point.
    let Some((was, is)) = pairs.iter().find(|(was, is)| was != is).or(pairs.first()) else {
        return json!({"ok":true,"moved":false,"dx":0,"dy":0});
    };
    let (moved_x, moved_y) = (number(is, 0) - number(was, 0), number(is, 1) - number(was, 1));
    // Offsets are fractional while the limits are whole pixels: allow one pixel.
    let stopped = |asked: f64, at: f64, max: f64| {
        if asked > 0. { at >= max - 1. } else { asked == 0. || at <= 0. }
    };
    json!({
        "ok": true,
        "moved": moved_x != 0. || moved_y != 0.,
        "dx": moved_x, "dy": moved_y,
        "left": number(is, 0), "top": number(is, 1),
        "max_left": number(is, 2), "max_top": number(is, 3),
        "at_end": stopped(dx, number(is, 0), number(is, 2)) && stopped(dy, number(is, 1), number(is, 3)),
    })
}

/// Largest browser.eval result, as JSON text.
const EVAL_RESULT: usize = 200_000;

/// Largest browser.download, in bytes; it crosses DevTools as base64, like a screenshot.
const DOWNLOAD_BYTES: u64 = 32 * 1024 * 1024;

/// Called with the URL and the byte limit; an HTTP error or an oversized body comes back
/// without its bytes. Only an http(s) final URL is returned: a data: URL is the content.
const DOWNLOAD: &str = r#"(async (url, max) => {
  const target = new URL(url, location.href);
  let response;
  try {
    response = await fetch(target, {credentials: 'include'});
  } catch (error) {
    // A site that lets any origin read it (`*`) refuses a read that carries cookies;
    // ask such a site again without them. A same-origin failure is a real one.
    if (target.origin === location.origin) throw error;
    response = await fetch(target, {credentials: 'same-origin'});
  }
  const out = {ok: response.ok, status: response.status, url: /^https?:/.test(response.url) ? response.url : null};
  if (!response.ok) return out;
  const blob = await response.blob();
  out.type = blob.type || response.headers.get('content-type') || '';
  out.bytes = blob.size;
  if (blob.size > max) return out;
  const bytes = new Uint8Array(await blob.arrayBuffer());
  let binary = '';
  for (let i = 0; i < bytes.length; i += 0x8000) binary += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
  out.data = btoa(binary);
  return out;
})"#;

/// A safe file name for a download: the URL's last path segment, else `download`, with an
/// extension from the content type when the name has none.
fn download_name(url: &str, content_type: &str) -> String {
    let segment = url::Url::parse(url)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        .and_then(|u| u.path_segments()?.filter(|s| !s.is_empty()).last().map(str::to_string))
        .unwrap_or_default();
    let mut name: String = segment
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' })
        .take(120)
        .collect();
    name = name.trim_matches('.').to_string();
    if name.is_empty() {
        name = "download".into();
    }
    if !name.contains('.') {
        let mime = content_type.split(';').next().unwrap_or("").trim();
        let ext = match mime {
            "image/png" => "png",
            "image/jpeg" => "jpg",
            "image/webp" => "webp",
            "image/gif" => "gif",
            "image/avif" => "avif",
            "image/svg+xml" => "svg",
            "video/mp4" => "mp4",
            "video/webm" => "webm",
            "audio/mpeg" => "mp3",
            "application/pdf" => "pdf",
            "application/json" => "json",
            "application/zip" => "zip",
            "text/html" => "html",
            "text/csv" => "csv",
            "text/plain" => "txt",
            _ => "",
        };
        if !ext.is_empty() {
            name = format!("{name}.{ext}");
        }
    }
    name
}

/// Tallest full-page capture, in CSS pixels; a longer page is cut there and says so.
const FULL_HEIGHT: f64 = 20000.;

const RECORD_CURSOR: &str = r#"(() => {
  if (globalThis.__herdrRecordStop) return;
  const cursors = new Map();
  let current = '', svg = __SVG__;
  const idleMs = __IDLE_MS__;
  const place = (c) => { c.el.style.transform = `translate(${c.x - 4.5}px, ${c.y - 3}px)`; };
  const tick = (c) => {
    c.x += (c.tx - c.x) * 0.45; c.y += (c.ty - c.y) * 0.45;
    if (Math.abs(c.tx - c.x) < 0.3 && Math.abs(c.ty - c.y) < 0.3) { c.x = c.tx; c.y = c.ty; c.moving = false; }
    place(c);
    if (c.moving) requestAnimationFrame(() => tick(c));
  };
  const mount = () => {
    let c = cursors.get(current);
    if (c?.el.isConnected || !document.documentElement) return c;
    const el = document.createElement('div');
    el.style.cssText = 'position:fixed;left:0;top:0;width:28px;height:28px;z-index:2147483647;pointer-events:none;opacity:0;transition:opacity .4s';
    el.innerHTML = svg;
    document.documentElement.appendChild(el);
    c = { el, x: 0, y: 0, tx: 0, ty: 0, moving: false, seen: false, timer: 0 };
    cursors.set(current, c);
    return c;
  };
  const to = (e) => {
    const c = mount();
    if (!c) return;
    c.tx = e.clientX; c.ty = e.clientY;
    if (!c.seen) { c.seen = true; c.x = c.tx; c.y = c.ty; place(c); }
    c.el.style.opacity = '1';
    clearTimeout(c.timer);
    if (idleMs > 0) c.timer = setTimeout(() => { c.el.style.opacity = '0'; }, idleMs);
    if (!c.moving) { c.moving = true; requestAnimationFrame(() => tick(c)); }
  };
  const ripple = (e) => {
    to(e);
    const r = document.createElement('div');
    r.style.cssText = `position:fixed;left:${e.clientX - 14}px;top:${e.clientY - 14}px;width:28px;height:28px;border-radius:50%;border:2px solid rgba(0,0,0,.4);box-shadow:0 0 0 1px rgba(255,255,255,.6);z-index:2147483646;pointer-events:none;transition:transform .45s ease-out,opacity .45s ease-out`;
    document.documentElement.appendChild(r);
    requestAnimationFrame(() => { r.style.transform = 'scale(1.9)'; r.style.opacity = '0'; });
    setTimeout(() => r.remove(), 500);
  };
  addEventListener('pointermove', to, true);
  addEventListener('pointerdown', ripple, true);
  globalThis.__herdrRecordAs = (key, next) => { current = key; svg = next; };
  globalThis.__herdrRecordStop = () => {
    removeEventListener('pointermove', to, true);
    removeEventListener('pointerdown', ripple, true);
    for (const c of cursors.values()) c.el.remove();
    delete globalThis.__herdrRecordAs;
    delete globalThis.__herdrRecordStop;
  };
})()"#;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SiteRule {
    pub(super) host: String,
    pub(super) scheme: Option<String>,
    pub(super) port: Option<u16>,
}

pub(super) struct Login {
    pub(super) username: String,
    pub(super) password: String,
    pub(super) sites: Vec<SiteRule>,
}

const MULTI_TENANT_HOSTS: &[&str] = &[
    "github.io",
    "gitlab.io",
    "vercel.app",
    "netlify.app",
    "herokuapp.com",
    "appspot.com",
    "pages.dev",
    "workers.dev",
    "fly.dev",
    "onrender.com",
    "web.app",
    "firebaseapp.com",
    "s3.amazonaws.com",
    "cloudfront.net",
    "azurewebsites.net",
    "notion.site",
];

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

pub(super) fn parse_site_rule(raw: &str) -> Option<SiteRule> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let has_scheme = trimmed.contains("://");
    let candidate = if has_scheme {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    let parsed = url::Url::parse(&candidate).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed.host_str()?.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return None;
    }
    Some(SiteRule {
        host,
        scheme: has_scheme.then(|| parsed.scheme().to_string()),
        port: parsed.port(),
    })
}

pub(super) fn listed_url(sites: &[SiteRule], target: &url::Url) -> bool {
    if !matches!(target.scheme(), "http" | "https") {
        return false;
    }
    let Some(raw_host) = target.host_str() else {
        return false;
    };
    let host = raw_host.trim_end_matches('.').to_ascii_lowercase();
    let loopback = is_loopback_host(&host);
    sites.iter().any(|rule| {
        if target.scheme() == "http" && !loopback && rule.scheme.as_deref() != Some("http") {
            return false;
        }
        if rule.port.is_some() && rule.port != target.port() {
            return false;
        }
        if host == rule.host {
            return true;
        }
        if !rule.host.contains('.') || MULTI_TENANT_HOSTS.contains(&rule.host.as_str()) {
            return false;
        }
        host.ends_with(&format!(".{}", rule.host))
    })
}

/// browser.signin's credentials: a 1Password item, or a dotenv file's test login.
pub(super) async fn login(args: &Value, ex: &BackgroundExecutor) -> Result<Login, String> {
    let text = |key: &str| args.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    match (text("item"), text("env_file")) {
        (Some(item), None) if !item.starts_with('-') => ex.spawn(async move { one_password(&item) }).await,
        (None, Some(file)) => {
            let user = text("user_key").ok_or("env_file needs user_key")?;
            let password = text("password_key").ok_or("env_file needs password_key")?;
            let site = text("site").ok_or("env_file needs site, the host it may fill (e.g. localhost)")?;
            env_login(&file, &user, &password, &site)
        }
        _ => Err("Pass exactly one of item (1Password) or env_file".into()),
    }
}

/// `op` asks the person to approve (Touch ID) on first use; its output never reaches a reply or log.
fn one_password(item: &str) -> Result<Login, String> {
    let output = ["op", "/opt/homebrew/bin/op", "/usr/local/bin/op"]
        .into_iter()
        .find_map(|op| {
            std::process::Command::new(op)
                .args(["item", "get", item, "--format", "json", "--reveal"])
                .stdin(std::process::Stdio::null())
                .output()
                .ok()
        })
        .ok_or("1Password CLI `op` is not installed")?;
    if !output.status.success() {
        return Err(format!(
            "op could not read {item:?}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let v: Value = serde_json::from_slice(&output.stdout).map_err(|_| "op returned unreadable output")?;
    let field = |purpose: &str| {
        v["fields"]
            .as_array()
            .and_then(|f| f.iter().find(|f| f["purpose"] == purpose))
            .and_then(|f| f["value"].as_str())
            .unwrap_or("")
            .to_string()
    };
    let sites: Vec<SiteRule> = v["urls"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|u| u["href"].as_str())
        .filter_map(parse_site_rule)
        .collect();
    if sites.is_empty() {
        return Err(format!("1Password item {item:?} lists no website; add one so sign-in knows where it may fill"));
    }
    Ok(Login { username: field("USERNAME"), password: field("PASSWORD"), sites })
}

/// Test credentials from a dotenv file; `site` stands in for a 1Password item's websites.
fn env_login(file: &str, user_key: &str, password_key: &str, site: &str) -> Result<Login, String> {
    let rule = parse_site_rule(site).ok_or_else(|| format!("Invalid site {site:?}"))?;
    let body = std::fs::read_to_string(file).map_err(|e| format!("Cannot read {file}: {e}"))?;
    let value = |key: &str| {
        body.lines()
            .filter_map(|line| line.trim().trim_start_matches("export ").split_once('='))
            .find(|(k, _)| k.trim() == key)
            .map(|(_, v)| v.trim().trim_matches(|c| c == '"' || c == '\'').to_string())
            .ok_or(format!("{file} has no {key}"))
    };
    Ok(Login {
        username: value(user_key)?,
        password: value(password_key)?,
        sites: vec![rule],
    })
}

#[cfg(test)]
mod tests {
    use super::{download_name, listed_url, parse_site_rule};

    #[test]
    fn download_names_keep_the_site_name_and_stay_inside_the_folder() {
        assert_eq!(download_name("https://cdn.example.com/a/0_1_640_N.webp?method=shortest", "image/webp"), "0_1_640_N.webp");
        assert_eq!(download_name("https://example.com/files/report", "application/pdf; charset=binary"), "report.pdf");
        assert_eq!(download_name("https://example.com/../..%2Fetc%2Fpasswd", "text/plain"), "_2Fetc_2Fpasswd.txt");
        assert_eq!(download_name("https://example.com/", "image/png"), "download.png");
        assert_eq!(download_name("data:image/png;base64,iVBOR", "image/png"), "download.png");
        assert_eq!(download_name("blob:https://example.com/1f2e", ""), "download");
        assert_eq!(download_name("/relative/thing.zip", ""), "download");
    }

    #[test]
    fn signin_site_matching_blocks_http_downgrade_port_mismatch_and_multitenant_wildcards() {
        let https_site = vec![parse_site_rule("https://example.com").unwrap()];
        assert!(listed_url(&https_site, &url::Url::parse("https://example.com/login").unwrap()));
        assert!(listed_url(&https_site, &url::Url::parse("https://app.example.com/login").unwrap()));
        // Plaintext HTTP downgrade is refused for an HTTPS item.
        assert!(!listed_url(&https_site, &url::Url::parse("http://example.com/login").unwrap()));
        assert!(!listed_url(&https_site, &url::Url::parse("https://notexample.com/login").unwrap()));

        // Explicit HTTP item or loopback allows HTTP.
        let explicit_http = vec![parse_site_rule("http://intranet.local:8080").unwrap()];
        assert!(listed_url(&explicit_http, &url::Url::parse("http://intranet.local:8080/login").unwrap()));
        assert!(!listed_url(&explicit_http, &url::Url::parse("http://intranet.local:9090/login").unwrap()));

        let local = vec![parse_site_rule("localhost").unwrap()];
        assert!(listed_url(&local, &url::Url::parse("http://localhost:3000/login").unwrap()));

        // Multi-tenant hosting suffixes do not wildcard subdomains.
        let shared = vec![parse_site_rule("https://github.io").unwrap()];
        assert!(listed_url(&shared, &url::Url::parse("https://github.io/").unwrap()));
        assert!(!listed_url(&shared, &url::Url::parse("https://evil.github.io/").unwrap()));
    }
}
