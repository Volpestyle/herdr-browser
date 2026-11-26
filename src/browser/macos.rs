//! Native Chromium child views. CEF owns rendering, input and Retina scaling.
use super::{Event, PointerAction, runtime};
use cef::*;
use gpui::{Bounds, Pixels, Window as GpuiWindow};
use objc2::{
    MainThreadMarker, MainThreadOnly,
    class, msg_send,
    rc::{Retained, Weak as ObjcWeak, autoreleasepool},
    runtime::AnyObject,
};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSEvent, NSEventMask, NSEventModifierFlags, NSView, NSWindow,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ptr::NonNull,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

#[link(name = "QuartzCore", kind = "framework")]
unsafe extern "C" {
    fn CACurrentMediaTime() -> f64;
}

type Tx = async_channel::Sender<Event>;
fn page_url(browser: Option<&Browser>) -> String {
    browser
        .and_then(|b| b.main_frame())
        .map(|f| CefString::from(&f.url()).to_string())
        .unwrap_or_default()
}
fn web_url(url: &str) -> bool {
    url == "about:blank"
        || url::Url::parse(url).is_ok_and(|u| matches!(u.scheme(), "http" | "https"))
}
fn download_url(url: &str) -> bool {
    if let Some(inner) = url.strip_prefix("blob:") {
        return url::Url::parse(inner).is_ok_and(|u| matches!(u.scheme(), "http" | "https"));
    }
    url != "about:blank" && web_url(url)
}
wrap_display_handler! {
    struct Display { tx: Tx }
    impl DisplayHandler {
        fn on_title_change(&self, _browser: Option<&mut Browser>, title: Option<&CefString>) {
            if let Some(title) = title { let _ = self.tx.try_send(Event::Title(title.to_string())); }
        }
        fn on_address_change(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, url: Option<&CefString>) {
            if frame.is_some_and(|f| f.is_main() != 0) {
                if let Some(url) = url { let _ = self.tx.try_send(Event::Loading(url.to_string())); }
            }
        }
    }
}
wrap_load_handler! {
    struct Load { tx: Tx }
    impl LoadHandler {
        fn on_loading_state_change(&self, browser: Option<&mut Browser>, loading: i32, _back: i32, _forward: i32) {
            let url = page_url(browser.as_deref());
            let _ = self.tx.try_send(if loading != 0 { Event::Loading(url) } else { Event::Loaded(url) });
        }
        fn on_load_error(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, code: Errorcode, text: Option<&CefString>, _url: Option<&CefString>) {
            if frame.is_some_and(|f| f.is_main() != 0) && code != Errorcode::ABORTED {
                let _ = self.tx.try_send(Event::Error(text.map(CefString::to_string).unwrap_or_else(|| "Page failed to load".into())));
            }
        }
    }
}
wrap_request_handler! {
    struct Navigation { tx: Option<Tx> }
    impl RequestHandler {
        fn auth_credentials(&self, _browser: Option<&mut Browser>, _origin: Option<&CefString>, _proxy: i32, host: Option<&CefString>, _port: i32, _realm: Option<&CefString>, _scheme: Option<&CefString>, callback: Option<&mut AuthCallback>) -> i32 {
            // The pane asks for the sign-in itself: Chromium's own prompt
            // crashes the app when its pane closes first. A popup has no pane
            // to ask in, so its challenge is declined.
            let (Some(tx), Some(callback)) = (&self.tx, callback) else { return 0 };
            let callback = callback.clone();
            let reply: Box<dyn FnOnce(Option<(String, String)>)> = Box::new(move |login| match login {
                Some((user, password)) => callback.cont(Some(&user.as_str().into()), Some(&password.as_str().into())),
                None => callback.cancel(),
            });
            let host = host.map(CefString::to_string).unwrap_or_default();
            let _ = tx.try_send(Event::Auth(super::Auth { host, reply: Some(reply) }));
            1
        }
        fn on_before_browse(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, request: Option<&mut Request>, _gesture: i32, _redirect: i32) -> i32 {
            if !frame.is_some_and(|f| f.is_main() != 0) { return 0; }
            request.is_some_and(|r| !web_url(&CefString::from(&r.url()).to_string())) as i32
        }
        // Cmd-click and middle-click ask for a new tab here, not as a popup;
        // left alone, Chromium has no tab strip to put it in and drops it.
        fn on_open_urlfrom_tab(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, url: Option<&CefString>, disposition: WindowOpenDisposition, _gesture: i32) -> i32 {
            let Some(tx) = &self.tx else { return 0 };
            let url = url.map(CefString::to_string).unwrap_or_default();
            let new_tab = matches!(disposition, WindowOpenDisposition::NEW_BACKGROUND_TAB | WindowOpenDisposition::NEW_FOREGROUND_TAB | WindowOpenDisposition::NEW_WINDOW);
            if !new_tab || !web_url(&url) { return 0; }
            let _ = tx.try_send(Event::Open(url));
            1
        }
    }
}
/// Herdr's own closes empty the pane's cell first, so a browser still in it
/// closed itself (window.close()). Its view is going away: the pane must stop
/// placing it and close, as a browser tab would.
fn closed_itself(cell: &std::rc::Weak<RefCell<Option<Browser>>>, tx: &Tx, browser: &Browser) {
    let Some(cell) = cell.upgrade() else { return };
    let Ok(mut held) = cell.try_borrow_mut() else { return };
    if held.as_ref().is_some_and(|b| b.identifier() == browser.identifier()) {
        held.take();
        let _ = tx.try_send(Event::Closed);
    }
}
wrap_life_span_handler! {
    struct Life { tx: Tx, browser: std::rc::Weak<RefCell<Option<Browser>>>, closing: Rc<Cell<bool>>, pending: Rc<Cell<bool>> }
    impl LifeSpanHandler {
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            let primary = self.pending.replace(false);
            if primary { runtime::PENDING.fetch_sub(1, Ordering::Relaxed); }
            runtime::ACTIVE.fetch_add(1, Ordering::Relaxed);
            if primary { if let Some(browser) = browser {
                if let Some(cell) = self.browser.upgrade().filter(|_| !self.closing.get()) {
                    *cell.borrow_mut() = Some(browser.clone());
                    let _ = self.tx.try_send(Event::Ready);
                } else if let Some(host) = browser.host() { host.close_browser(1); }
            } }
        }
        fn on_before_close(&self, browser: Option<&mut Browser>) {
            if let Some(browser) = browser { closed_itself(&self.browser, &self.tx, browser); }
            runtime::ACTIVE.fetch_sub(1, Ordering::Relaxed);
        }
        fn do_close(&self, browser: Option<&mut Browser>) -> i32 {
            let Some(browser) = browser else { return 1 };
            // Tear down this child view, never Herdr's top-level window.
            if let Some(host) = browser.host() {
                if let Some(view) = unsafe { host.window_handle().cast::<NSView>().as_ref() } { view.removeFromSuperview(); }
            }
            closed_itself(&self.browser, &self.tx, browser);
            1
        }
        fn on_before_popup(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, _id: i32, url: Option<&CefString>, _name: Option<&CefString>, disposition: WindowOpenDisposition, _gesture: i32, _features: Option<&PopupFeatures>, _window: Option<&mut WindowInfo>, client: Option<&mut Option<Client>>, _settings: Option<&mut BrowserSettings>, _extra: Option<&mut Option<DictionaryValue>>, _no_js: Option<&mut i32>) -> i32 {
            let url = url.map(CefString::to_string).unwrap_or_default();
            // A scripted window.open popup (OAuth sign-in) needs a real window
            // that keeps window.opener; cancelling it makes the page report
            // "popups blocked". Links that ask for a new tab become Herdr tabs.
            if disposition == WindowOpenDisposition::NEW_POPUP && (url.is_empty() || web_url(&url)) {
                if let Some(client) = client { *client = Some(PopupClient::new()); }
                return 0;
            }
            if web_url(&url) { let _ = self.tx.try_send(Event::Open(url)); }
            1
        }
    }
}
wrap_life_span_handler! {
    // Its own top-level Chromium window: CEF closes it, Herdr only tracks it.
    struct PopupLife;
    impl LifeSpanHandler {
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            runtime::ACTIVE.fetch_add(1, Ordering::Relaxed);
            if let Some(browser) = browser { runtime::add_popup(browser.clone()); }
        }
        fn on_before_close(&self, browser: Option<&mut Browser>) {
            if let Some(browser) = browser { runtime::remove_popup(browser); }
            runtime::ACTIVE.fetch_sub(1, Ordering::Relaxed);
        }
        fn on_before_popup(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, _id: i32, url: Option<&CefString>, _name: Option<&CefString>, disposition: WindowOpenDisposition, _gesture: i32, _features: Option<&PopupFeatures>, _window: Option<&mut WindowInfo>, _client: Option<&mut Option<Client>>, _settings: Option<&mut BrowserSettings>, _extra: Option<&mut Option<DictionaryValue>>, _no_js: Option<&mut i32>) -> i32 {
            let url = url.map(CefString::to_string).unwrap_or_default();
            (disposition != WindowOpenDisposition::NEW_POPUP || !(url.is_empty() || web_url(&url))) as i32
        }
    }
}
wrap_client! {
    struct PopupClient;
    impl Client {
        fn request_handler(&self) -> Option<RequestHandler> { Some(Navigation::new(None)) }
        fn life_span_handler(&self) -> Option<LifeSpanHandler> { Some(PopupLife::new()) }
    }
}
wrap_focus_handler! {
    struct Focus { gate: Arc<AtomicBool> }
    impl FocusHandler {
        fn on_set_focus(&self, _browser: Option<&mut Browser>, _source: FocusSource) -> i32 { (!self.gate.load(Ordering::Relaxed)) as i32 }
    }
}
wrap_keyboard_handler! {
    struct Keys;
    impl KeyboardHandler {
        // Edit shortcuts the page left unhandled. AppKit would hand them to
        // GPUI's menu, so the event monitor sends them to the page directly.
        fn on_key_event(&self, browser: Option<&mut Browser>, event: Option<&KeyEvent>, _os_event: *mut u8) -> i32 {
            use sys::cef_event_flags_t as Flag;
            let Some(event) = event else { return 0 };
            let held = |flag: Flag| event.modifiers & flag.0 != 0;
            if event.type_ != KeyEventType::RAWKEYDOWN || !held(Flag::EVENTFLAG_COMMAND_DOWN) || held(Flag::EVENTFLAG_CONTROL_DOWN) || held(Flag::EVENTFLAG_ALT_DOWN) { return 0; }
            let Some(frame) = browser.and_then(|b| b.focused_frame()) else { return 0 };
            match (event.windows_key_code, held(Flag::EVENTFLAG_SHIFT_DOWN)) {
                (67, false) => frame.copy(),
                (86, false) => frame.paste(),
                (88, false) => frame.cut(),
                (65, false) => frame.select_all(),
                (90, false) => frame.undo(),
                (90, true) => frame.redo(),
                _ => return 0,
            }
            1
        }
    }
}
wrap_download_handler! {
    // A person sees Chromium's save dialog; agent downloads save to the app home.
    struct Download { tx: Tx, ask: bool }
    impl DownloadHandler {
        fn can_download(&self, _browser: Option<&mut Browser>, url: Option<&CefString>, _method: Option<&CefString>) -> i32 {
            url.is_some_and(|url| download_url(&url.to_string())) as i32
        }
        fn on_before_download(&self, _browser: Option<&mut Browser>, _item: Option<&mut DownloadItem>, name: Option<&CefString>, callback: Option<&mut BeforeDownloadCallback>) -> i32 {
            let folder = crate::mods::app_dir().join("downloads");
            if std::fs::create_dir_all(&folder).is_err() { let _ = self.tx.try_send(Event::Notice("Cannot create downloads folder".into())); return 1; }
            let suggested = name.map(CefString::to_string).unwrap_or_else(|| "download".into());
            let name = std::path::Path::new(&suggested).file_name().unwrap_or_default();
            let mut path = folder.join(name); let mut i = 1;
            while path.exists() { path = folder.join(format!("{i}-{}", name.to_string_lossy())); i += 1; }
            if let Some(callback) = callback {
                callback.cont(Some(&CefString::from(path.to_string_lossy().as_ref())), self.ask as i32);
                let _ = self.tx.try_send(Event::Notice(format!("Downloading to {}", path.display())));
            }
            1
        }
        fn on_download_updated(&self, _browser: Option<&mut Browser>, item: Option<&mut DownloadItem>, _callback: Option<&mut DownloadItemCallback>) {
            if let Some(item) = item {
                if item.is_complete() != 0 { let _ = self.tx.try_send(Event::Notice(format!("Download saved to {}", CefString::from(&item.full_path())))); }
                else if item.is_canceled() != 0 { let _ = self.tx.try_send(Event::Notice("Download canceled".into())); }
            }
        }
    }
}
wrap_find_handler! {
    struct Find { tx: Tx }
    impl FindHandler {
        fn on_find_result(
            &self,
            _browser: Option<&mut Browser>,
            _identifier: i32,
            count: i32,
            _selection_rect: Option<&Rect>,
            active_match_ordinal: i32,
            final_update: i32,
        ) {
            let _ = self.tx.try_send(Event::FindResult {
                count: count.max(0) as usize,
                active: active_match_ordinal.max(0) as usize,
                final_update: final_update != 0,
            });
        }
    }
}
wrap_client! {
    struct PageClient { tx: Tx, ask: bool, gate: Arc<AtomicBool>, browser: std::rc::Weak<RefCell<Option<Browser>>>, closing: Rc<Cell<bool>>, pending: Rc<Cell<bool>>, container: Retained<NSView> }
    impl Client {
        fn display_handler(&self) -> Option<DisplayHandler> { Some(Display::new(self.tx.clone())) }
        fn load_handler(&self) -> Option<LoadHandler> { Some(Load::new(self.tx.clone())) }
        fn request_handler(&self) -> Option<RequestHandler> { Some(Navigation::new(Some(self.tx.clone()))) }
        fn life_span_handler(&self) -> Option<LifeSpanHandler> { Some(Life::new(self.tx.clone(), self.browser.clone(), self.closing.clone(), self.pending.clone())) }
        fn focus_handler(&self) -> Option<FocusHandler> { Some(Focus::new(self.gate.clone())) }
        fn keyboard_handler(&self) -> Option<KeyboardHandler> { Some(Keys::new()) }
        fn download_handler(&self) -> Option<DownloadHandler> { Some(Download::new(self.tx.clone(), self.ask)) }
        fn find_handler(&self) -> Option<FindHandler> { Some(Find::new(self.tx.clone())) }
    }
}
struct CursorSlot {
    cursor: Retained<AnyObject>,
    ripple: Retained<AnyObject>,
    cute: bool,
    moves: u64,
    clicks: u64,
    last_x: f64,
    last_y: f64,
}

type CursorSlots = Rc<RefCell<HashMap<String, CursorSlot>>>;

struct TargetHost {
    id: u64,
    target_id: Rc<RefCell<Option<String>>>,
    container: ObjcWeak<NSView>,
    layers: CursorSlots,
}

static NEXT_HOST_ID: AtomicU64 = AtomicU64::new(1);

pub struct Host {
    id: u64,
    browser: runtime::BrowserCell,
    control: RefCell<Option<Rc<super::control::Control>>>,
    layers: CursorSlots,
    target_id: Rc<RefCell<Option<String>>>,
    target_page: Rc<Cell<bool>>,
    closing: Rc<Cell<bool>>,
    container: Retained<NSView>,
    parent: Retained<NSView>,
    window: Retained<NSWindow>,
    shown: Cell<bool>,
    allowed: Rc<Cell<bool>>,
    find_open: Rc<Cell<bool>>,
    bounds: Cell<Option<[f64; 4]>>,
    cut: Cell<f64>,
    focus_pending: Cell<bool>,
    focus_gate: Arc<AtomicBool>,
    monitor: Option<Retained<AnyObject>>,
}
const AGENT_CURSOR: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="28" height="28" viewBox="0 0 28 28"><path d="M4 3.2 L4 21 Q4 23.2 5.9 21.7 L9.9 18.6 L12.9 24.5 Q13.7 26 15.2 25.3 L16.6 24.6 Q18 23.9 17.3 22.5 L14.4 16.9 L19.4 16.6 Q21.6 16.4 20 14.9 L6 2.2 Q4 0.9 4 3.2 Z" fill="#000" fill-opacity=".5" stroke="#fff" stroke-width="1.8" stroke-linejoin="round"/></svg>"##;
const CUTE_AGENT_CURSOR: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="28" height="28" viewBox="0 0 28 28"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#B69CFF"/><stop offset="1" stop-color="#7C5CFF"/></linearGradient></defs><path d="M4 3.2 L4 21 Q4 23.2 5.9 21.7 L9.9 18.6 L12.9 24.5 Q13.7 26 15.2 25.3 L16.6 24.6 Q18 23.9 17.3 22.5 L14.4 16.9 L19.4 16.6 Q21.6 16.4 20 14.9 L6 2.2 Q4 0.9 4 3.2 Z" fill="url(#g)" stroke="#fff" stroke-width="1.8" stroke-linejoin="round"/><circle cx="8" cy="11.6" r="1.25" fill="#2A1B5E"/><circle cx="11.5" cy="11.6" r="1.25" fill="#2A1B5E"/><circle cx="8.4" cy="11.1" r=".45" fill="#fff"/><circle cx="11.9" cy="11.1" r=".45" fill="#fff"/><ellipse cx="6.8" cy="14" rx="1.1" ry=".6" fill="#FF8FB8" opacity=".75"/><ellipse cx="12.8" cy="14" rx="1.1" ry=".6" fill="#FF8FB8" opacity=".75"/><path d="M8.9 13.5 Q9.75 14.5 10.6 13.5" fill="none" stroke="#2A1B5E" stroke-width=".8" stroke-linecap="round"/></svg>"##;

const DEFAULT_AGENT_CURSOR_IDLE_SECS: f64 = 5.;
const MAX_AGENT_CURSOR_IDLE_SECS: f64 = 3600.;

#[derive(Clone, Copy, Debug, PartialEq)]
struct CursorPrefs {
    cute: bool,
    /// Seconds before an idle cursor fades out; `0.0` keeps it visible.
    idle_secs: f64,
}

fn parse_cursor_prefs(text: &str) -> CursorPrefs {
    let v = serde_json::from_str::<Value>(text).ok();
    let cute = v.as_ref().is_some_and(|v| v["agent_cursor"] == "cute");
    let idle_secs = v
        .as_ref()
        .and_then(|v| v.get("agent_cursor_idle_secs"))
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite() && *n >= 0.)
        .map(|n| n.min(MAX_AGENT_CURSOR_IDLE_SECS))
        .unwrap_or(DEFAULT_AGENT_CURSOR_IDLE_SECS);
    CursorPrefs { cute, idle_secs }
}

const AGENT_COLORS: [&str; 8] = ["#3E63DD", "#E5484D", "#30A46C", "#F76B15", "#8E4EC6", "#12A594", "#D6409F", "#C29B00"];

thread_local! {
    static AGENT_COLOR_SLOTS: RefCell<HashMap<String, usize>> = RefCell::new(HashMap::new());
    static CURSOR_PREFS_CACHE: RefCell<Option<(Instant, CursorPrefs)>> = const { RefCell::new(None) };
    static TARGET_HOSTS: RefCell<Vec<TargetHost>> = const { RefCell::new(Vec::new()) };
}

fn cursor_prefs() -> CursorPrefs {
    CURSOR_PREFS_CACHE.with(|cache| {
        let now = Instant::now();
        if let Some((at, prefs)) = *cache.borrow()
            && now.duration_since(at) < Duration::from_millis(500)
        {
            return prefs;
        }
        let prefs = std::fs::read_to_string(crate::settings::path())
            .map(|text| parse_cursor_prefs(&text))
            .unwrap_or(CursorPrefs { cute: false, idle_secs: DEFAULT_AGENT_CURSOR_IDLE_SECS });
        *cache.borrow_mut() = Some((now, prefs));
        prefs
    })
}

pub(super) fn cursor_idle_ms() -> u64 {
    (cursor_prefs().idle_secs * 1000.).round() as u64
}

fn agent_color(caller: Option<&str>) -> Option<&'static str> {
    let caller = caller?;
    let slot = AGENT_COLOR_SLOTS.with(|slots| {
        let mut slots = slots.borrow_mut();
        let next = slots.len();
        *slots.entry(caller.to_owned()).or_insert(next)
    });
    Some(AGENT_COLORS[slot % AGENT_COLORS.len()])
}

fn agent_cursor_svg(caller: Option<&str>, cute: bool) -> Vec<u8> {
    let svg = String::from_utf8_lossy(if cute { CUTE_AGENT_CURSOR } else { AGENT_CURSOR }).into_owned();
    let Some(color) = agent_color(caller) else { return svg.into_bytes() };
    let fill = format!(r#"fill="{color}""#);
    svg.replace(r##"fill="#000" fill-opacity=".5""##, &fill).replace(r#"fill="url(#g)""#, &fill).into_bytes()
}

fn agent_ripple_svg(caller: Option<&str>) -> Vec<u8> {
    let color = agent_color(caller).unwrap_or("#111");
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32"><circle cx="16" cy="16" r="12" fill="none" stroke="#fff" stroke-opacity=".85" stroke-width="3.6"/><circle cx="16" cy="16" r="12" fill="none" stroke="{color}" stroke-opacity=".95" stroke-width="2"/></svg>"##
    )
    .into_bytes()
}

fn agent_cursor(caller: Option<&str>) -> Vec<u8> {
    agent_cursor_svg(caller, cursor_prefs().cute)
}

unsafe fn set_layer_svg(layer: &AnyObject, svg: &[u8], size: NSSize, scale: f64) {
    let data: *mut AnyObject = msg_send![class!(NSData), dataWithBytes: svg.as_ptr().cast::<std::ffi::c_void>(), length: svg.len()];
    let alloc: *mut AnyObject = msg_send![class!(NSImage), alloc];
    let raw: *mut AnyObject = msg_send![alloc, initWithData: data];
    if let Some(image) = unsafe { Retained::<AnyObject>::from_raw(raw) } {
        let _: () = msg_send![&*image, setSize: size];
        let _: () = msg_send![layer, setContents: &*image];
        let _: () = msg_send![layer, setContentsScale: scale];
    }
}

pub(super) fn move_pointer_for_target(
    target_id: &str,
    caller: Option<&str>,
    x: f64,
    y: f64,
    action: PointerAction,
) -> bool {
    let matched = TARGET_HOSTS.with(|hosts| {
        hosts
            .borrow()
            .iter()
            .find(|h| h.target_id.borrow().as_deref() == Some(target_id))
            .and_then(|h| Some((h.container.load()?, h.layers.clone())))
    });
    let Some((container, layers)) = matched else {
        return false;
    };
    move_pointer(&container, &layers, caller, x, y, action)
}

fn move_pointer(
    container: &NSView,
    slots: &RefCell<HashMap<String, CursorSlot>>,
    caller: Option<&str>,
    x: f64,
    y: f64,
    action: PointerAction,
) -> bool {
    if !x.is_finite() || !y.is_finite() {
        return false;
    }
    let Some(window) = container.window() else { return false };
    if container.isHidden() || !window.isVisible() {
        return false;
    }
    let prefs = cursor_prefs();
    let position = NSPoint::new(x, container.bounds().size.height - y);
    let scale = window.backingScaleFactor();
    let mut slots = slots.borrow_mut();
    let key = caller.unwrap_or_default().to_owned();
    let fresh = !slots.contains_key(&key);
    unsafe {
        if fresh {
            container.setWantsLayer(true);
            let root: *mut AnyObject = msg_send![container, layer];
            if root.is_null() {
                return false;
            }
            let ripple_size = NSSize::new(32., 32.);
            let ripple: Retained<AnyObject> = msg_send![class!(CALayer), layer];
            let _: () = msg_send![&*ripple, setBounds: NSRect::new(NSPoint::new(0., 0.), ripple_size)];
            let _: () = msg_send![&*ripple, setAnchorPoint: NSPoint::new(0.5, 0.5)];
            set_layer_svg(&ripple, &agent_ripple_svg(caller), ripple_size, scale);
            let _: () = msg_send![&*ripple, setZPosition: 999_999_f64];
            let _: () = msg_send![&*ripple, setOpacity: 0_f32];
            let _: () = msg_send![root, addSublayer: &*ripple];

            let size = NSSize::new(28., 28.);
            let hot = NSPoint::new(4.5, 3.);
            let cursor: Retained<AnyObject> = msg_send![class!(CALayer), layer];
            let _: () = msg_send![&*cursor, setBounds: NSRect::new(NSPoint::new(0., 0.), size)];
            let _: () = msg_send![&*cursor, setAnchorPoint: NSPoint::new(hot.x / size.width, 1. - hot.y / size.height)];
            set_layer_svg(&cursor, &agent_cursor_svg(caller, prefs.cute), size, scale);
            let _: () = msg_send![&*cursor, setZPosition: 1e6_f64];
            let _: () = msg_send![&*cursor, setOpacity: 0_f32];
            let _: () = msg_send![&*cursor, setShadowOpacity: 0.35_f32];
            let _: () = msg_send![&*cursor, setShadowRadius: 3_f64];
            let _: () = msg_send![&*cursor, setShadowOffset: NSSize::new(0., -1.)];
            let _: () = msg_send![root, addSublayer: &*cursor];

            slots.insert(
                key.clone(),
                CursorSlot { cursor, ripple, cute: prefs.cute, moves: 0, clicks: 0, last_x: x, last_y: y },
            );
        }
        let slot = slots.get_mut(&key).expect("cursor slot");
        if slot.cute != prefs.cute {
            set_layer_svg(&slot.cursor, &agent_cursor_svg(caller, prefs.cute), NSSize::new(28., 28.), scale);
            slot.cute = prefs.cute;
        }
        let glide = match action {
            PointerAction::Click => 0.3_f64,
            PointerAction::Scroll => 0.2_f64,
            PointerAction::Move => 0.12_f64,
            PointerAction::Tap => 0.1_f64,
        };
        let _: () = msg_send![class!(CATransaction), begin];
        let _: () = msg_send![class!(CATransaction), setDisableActions: fresh];
        let _: () = msg_send![class!(CATransaction), setAnimationDuration: glide];
        let _: () = msg_send![&*slot.cursor, setPosition: position];
        let _: () = msg_send![class!(CATransaction), commit];

        let media_now = CACurrentMediaTime();
        let now: f64 = msg_send![&*slot.cursor, convertTime: media_now, fromLayer: std::ptr::null::<AnyObject>()];

        if matches!(action, PointerAction::Click | PointerAction::Tap) {
            let delay = match action {
                PointerAction::Click => if fresh { 0.05 } else { 0.32 },
                _ => 0.0,
            };
            let squish: *mut AnyObject = msg_send![class!(CABasicAnimation), animationWithKeyPath: &*NSString::from_str("transform.scale")];
            let from: *mut AnyObject = msg_send![class!(NSNumber), numberWithDouble: 0.75_f64];
            let to: *mut AnyObject = msg_send![class!(NSNumber), numberWithDouble: 1.0_f64];
            let _: () = msg_send![squish, setFromValue: from];
            let _: () = msg_send![squish, setToValue: to];
            let _: () = msg_send![squish, setDuration: 0.25_f64];
            let _: () = msg_send![squish, setBeginTime: now + delay];
            let _: () = msg_send![&*slot.cursor, addAnimation: squish, forKey: &*NSString::from_str("squish")];

            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: true];
            let _: () = msg_send![&*slot.ripple, setPosition: position];
            let _: () = msg_send![class!(CATransaction), commit];

            let rnow: f64 = msg_send![&*slot.ripple, convertTime: media_now, fromLayer: std::ptr::null::<AnyObject>()];
            let ease: *mut AnyObject = msg_send![class!(CAMediaTimingFunction), functionWithName: &*NSString::from_str("easeOut")];
            let rscale: *mut AnyObject = msg_send![class!(CABasicAnimation), animationWithKeyPath: &*NSString::from_str("transform.scale")];
            let sfrom: *mut AnyObject = msg_send![class!(NSNumber), numberWithDouble: 0.5_f64];
            let sto: *mut AnyObject = msg_send![class!(NSNumber), numberWithDouble: 1.9_f64];
            let _: () = msg_send![rscale, setFromValue: sfrom];
            let _: () = msg_send![rscale, setToValue: sto];
            let _: () = msg_send![rscale, setDuration: 0.45_f64];
            let _: () = msg_send![rscale, setBeginTime: rnow + delay];
            let _: () = msg_send![rscale, setTimingFunction: ease];
            let _: () = msg_send![&*slot.ripple, addAnimation: rscale, forKey: &*NSString::from_str("rippleScale")];

            let ropacity: *mut AnyObject = msg_send![class!(CABasicAnimation), animationWithKeyPath: &*NSString::from_str("opacity")];
            let ofrom: *mut AnyObject = msg_send![class!(NSNumber), numberWithDouble: 0.95_f64];
            let oto: *mut AnyObject = msg_send![class!(NSNumber), numberWithDouble: 0.0_f64];
            let _: () = msg_send![ropacity, setFromValue: ofrom];
            let _: () = msg_send![ropacity, setToValue: oto];
            let _: () = msg_send![ropacity, setDuration: 0.45_f64];
            let _: () = msg_send![ropacity, setBeginTime: rnow + delay];
            let _: () = msg_send![ropacity, setTimingFunction: ease];
            let _: () = msg_send![&*slot.ripple, addAnimation: ropacity, forKey: &*NSString::from_str("rippleOpacity")];
            slot.clicks += 1;
        }

        if prefs.idle_secs == 0. {
            let _: () = msg_send![&*slot.cursor, removeAnimationForKey: &*NSString::from_str("fade")];
            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: true];
            let _: () = msg_send![&*slot.cursor, setOpacity: 1_f32];
            let _: () = msg_send![class!(CATransaction), commit];
        } else {
            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: true];
            let _: () = msg_send![&*slot.cursor, setOpacity: 0_f32];
            let _: () = msg_send![class!(CATransaction), commit];
            let fade: *mut AnyObject = msg_send![class!(CABasicAnimation), animationWithKeyPath: &*NSString::from_str("opacity")];
            let shown: *mut AnyObject = msg_send![class!(NSNumber), numberWithDouble: 1_f64];
            let _: () = msg_send![fade, setFromValue: shown];
            let hidden: *mut AnyObject = msg_send![class!(NSNumber), numberWithDouble: 0_f64];
            let _: () = msg_send![fade, setToValue: hidden];
            let _: () = msg_send![fade, setBeginTime: now + prefs.idle_secs];
            let _: () = msg_send![fade, setDuration: 0.4_f64];
            let _: () = msg_send![fade, setFillMode: &*NSString::from_str("backwards")];
            let _: () = msg_send![&*slot.cursor, addAnimation: fade, forKey: &*NSString::from_str("fade")];
        }
        slot.moves += 1;
        slot.last_x = x;
        slot.last_y = y;
    }
    true
}

fn focused(container: &NSView, window: &NSWindow) -> bool {
    let mut responder = window.firstResponder();
    for _ in 0..64 {
        let Some(current) = responder else {
            return false;
        };
        if let Some(view) = current.downcast_ref::<NSView>() {
            if view.isDescendantOf(container) {
                return true;
            }
        }
        responder = unsafe { current.nextResponder() };
    }
    false
}
fn ctrl_shortcut(key: &str, shift: bool) -> Option<&'static str> {
    match (key, shift) {
        ("`" | "~", false) => Some("focus.cycle"),
        ("`" | "~", true) => Some("focus.cycle:prev"),
        _ => None,
    }
}
fn ctrl_cmd_shortcut(key: &str, shift: bool) -> Option<&'static str> {
    match (key, shift) {
        ("[", false) => Some("tab.move:prev"),
        ("]", false) => Some("tab.move:next"),
        ("b", false) => Some("browser.split:right"),
        _ => None,
    }
}
fn shortcut(key: &str, shift: bool, option: bool) -> Option<&'static str> {
    if option {
        return match (key, shift) {
            ("b", false) => Some("browser.open"),
            _ => None,
        };
    }
    match (key, shift) {
        ("[" | "{", true) => Some("tab.prev"),
        ("]" | "}", true) => Some("tab.next"),
        ("[", false) => Some("browser.back"),
        ("]", false) => Some("browser.forward"),
        ("f", false) => Some("browser.find"),
        ("l", false) => Some("browser.focus_url"),
        ("r", false) => Some("browser.reload"),
        ("b", false) => Some("layout.toggle:sidebar"),
        ("b", true) => Some("browser.split:right"),
        ("t", false) => Some("tab.new"),
        ("w", false) => Some("tab.close"),
        ("p", true) => Some("app.palette"),
        (",", false) => Some("app.settings"),
        ("i", true) => Some("browser.devtools"),
        _ => None,
    }
}
impl Host {
    pub fn new(
        url: &str,
        profile: &str,
        allowed: Rc<Cell<bool>>,
        tx: Tx,
        window: &GpuiWindow,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            runtime::ready(),
            "Chromium runtime is unavailable; launch the packaged app"
        );
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| anyhow::anyhow!("Browser creation must run on the UI thread"))?;
        let RawWindowHandle::AppKit(handle) = HasWindowHandle::window_handle(window)?.as_raw()
        else {
            anyhow::bail!("Expected an AppKit window")
        };
        // GPUI owns this view for the window lifetime; retain our references.
        let parent = unsafe { Retained::retain(handle.ns_view.as_ptr().cast::<NSView>()) }
            .ok_or_else(|| anyhow::anyhow!("Missing parent view"))?;
        let native_window = parent
            .window()
            .ok_or_else(|| anyhow::anyhow!("Missing native window"))?;
        if std::env::var_os("HERDR_BROWSER_TEST_WINDOW_FRONT").is_some() {
            native_window.orderFrontRegardless();
        }
        // A fully occluded GPUI window may not paint its pane layout yet.
        // Give Chromium a usable provisional viewport immediately; the canvas
        // replaces it with exact pane bounds on its first paint. Keep the view
        // hidden until that placement, so it cannot cover unrelated app chrome.
        let initial = NSSize::new(
            parent.bounds().size.width.max(1.),
            (parent.bounds().size.height - 44.).max(1.),
        );
        let container = NSView::initWithFrame(
            NSView::alloc(mtm),
            NSRect::new(NSPoint::new(0., 0.), initial),
        );
        container.setHidden(true);
        // place() shortens the container over a taller page to keep toasts visible.
        container.setClipsToBounds(true);
        parent.addSubview(&container);
        let info = WindowInfo {
            runtime_style: RuntimeStyle::CHROME,
            ..Default::default()
        }
        .set_as_child(
            Retained::as_ptr(&container).cast_mut().cast(),
            &Rect {
                x: 0,
                y: 0,
                width: initial.width as i32,
                height: initial.height as i32,
            },
        );
        let focus_gate = Arc::new(AtomicBool::new(false));
        let browser = Rc::new(RefCell::new(None));
        let closing = Rc::new(Cell::new(false));
        let pending = Rc::new(Cell::new(true));
        // The client retains the parent until async creation finishes, even if
        // its pane closes while the request context is loading.
        let mut client = PageClient::new(
            tx.clone(),
            profile != "agent",
            focus_gate.clone(),
            Rc::downgrade(&browser),
            closing.clone(),
            pending.clone(),
            container.clone(),
        );
        let mut context = runtime::request_context(profile)?;
        runtime::PENDING.fetch_add(1, Ordering::Relaxed);
        let created = browser_host_create_browser(
            Some(&info),
            Some(&mut client),
            Some(&url.into()),
            Some(&BrowserSettings::default()),
            None,
            context.as_mut(),
        );
        if created == 0 {
            pending.set(false);
            runtime::PENDING.fetch_sub(1, Ordering::Relaxed);
            anyhow::bail!("Chromium could not schedule a native browser");
        }
        let weak_container = ObjcWeak::from_retained(&container);
        let weak_parent = ObjcWeak::from_retained(&parent);
        let teardown = closing.clone();
        runtime::register(&browser, move || {
            teardown.set(true);
            autoreleasepool(|_| {
                if let Some(container) = weak_container.load() {
                    if let (Some(window), Some(parent)) = (container.window(), weak_parent.load()) {
                        window.makeFirstResponder(Some(&parent));
                    }
                    container.setHidden(true);
                    container.removeFromSuperview();
                    // Drop the temporary array before the pool drains, releasing
                    // CEF's host view and delivering its WindowDestroyed callback.
                    let children = container.subviews();
                    for child in children.iter() {
                        child.removeFromSuperview();
                    }
                }
            })
        });
        let find_open = Rc::new(Cell::new(false));
        let event_allowed = allowed.clone();
        let event_find_open = find_open.clone();
        let event_container = container.clone();
        let event_parent = parent.clone();
        let event_window = native_window.clone();
        let block = block2::RcBlock::new(move |event: NonNull<NSEvent>| {
            let e = unsafe { event.as_ref() };
            if e.windowNumber() != event_window.windowNumber() {
                return event.as_ptr();
            }
            // Chromium replaces a page's view on a cross-site navigation; if it
            // held the keyboard, AppKit leaves it with the window and typed text
            // reaches neither the page nor GPUI.
            if e.r#type() == objc2_app_kit::NSEventType::KeyDown && !focused(&event_parent, &event_window) {
                event_window.makeFirstResponder(Some(&event_parent));
            }
            if !event_allowed.get() {
                if e.r#type() == objc2_app_kit::NSEventType::KeyDown && focused(&event_container, &event_window) {
                    event_window.makeFirstResponder(Some(&event_parent));
                }
                return event.as_ptr();
            }
            if e.r#type() == objc2_app_kit::NSEventType::LeftMouseDown {
                let p = event_container.convertPoint_fromView(e.locationInWindow(), None);
                let b = event_container.bounds();
                if p.x >= 0. && p.y >= 0. && p.x < b.size.width && p.y < b.size.height {
                    let _ = tx.try_send(Event::Focused);
                } else if focused(&event_container, &event_window) {
                    // GPUI inputs update their own focus handle on a click, but
                    // don't reclaim AppKit's first responder from a native page.
                    // Return the keyboard before GPUI handles the press so URL
                    // selection, typing and clipboard shortcuts share a target.
                    event_window.makeFirstResponder(Some(&event_parent));
                }
                return event.as_ptr();
            }
            if !focused(&event_container, &event_window) {
                return event.as_ptr();
            }
            let flags = e.modifierFlags();
            // AppKit offers arrows and paging keys to GPUI's view as key
            // equivalents first, and GPUI feeds them to its own text input
            // context even though the page has the keyboard. Hand them over.
            if matches!(e.keyCode(), 115..=117 | 119 | 121 | 123..=126)
                && !flags.intersects(
                    NSEventModifierFlags::Command
                        | NSEventModifierFlags::Control
                        | NSEventModifierFlags::Option,
                )
            {
                if let Some(responder) = event_window.firstResponder() {
                    responder.keyDown(e);
                    return std::ptr::null_mut();
                }
            }
            if flags.contains(NSEventModifierFlags::Control)
                && !flags.contains(NSEventModifierFlags::Command)
                && !flags.contains(NSEventModifierFlags::Option)
            {
                let key = e
                    .charactersIgnoringModifiers()
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                if let Some(command) = ctrl_shortcut(&key, flags.contains(NSEventModifierFlags::Shift)) {
                    event_window.makeFirstResponder(Some(&event_parent));
                    let _ = tx.try_send(Event::Command(command));
                    return std::ptr::null_mut();
                }
                return event.as_ptr();
            }
            if flags.contains(NSEventModifierFlags::Command)
                && flags.contains(NSEventModifierFlags::Control)
                && !flags.contains(NSEventModifierFlags::Option)
            {
                let key = e
                    .charactersIgnoringModifiers()
                    .map(|s| s.to_string().to_lowercase())
                    .unwrap_or_default();
                if let Some(command) =
                    ctrl_cmd_shortcut(&key, flags.contains(NSEventModifierFlags::Shift))
                {
                    let _ = tx.try_send(Event::Command(command));
                    return std::ptr::null_mut();
                }
                return event.as_ptr();
            }
            if !flags.contains(NSEventModifierFlags::Command)
                || flags.contains(NSEventModifierFlags::Control)
            {
                return event.as_ptr();
            }
            let key = e
                .charactersIgnoringModifiers()
                .map(|s| s.to_string().to_lowercase())
                .unwrap_or_default();
            let shift = flags.contains(NSEventModifierFlags::Shift);
            let option = flags.contains(NSEventModifierFlags::Option);
            // App navigation remains available before a page has a focused frame.
            if let Some(command) = shortcut(&key, shift, option) {
                let _ = tx.try_send(match command {
                    "browser.focus_url" => Event::FocusUrl,
                    "browser.find" => Event::Find,
                    _ => Event::Command(command),
                });
                return std::ptr::null_mut();
            }
            if option {
                return event.as_ptr();
            }
            if event_find_open.get() && key == "g" {
                let _ = tx.try_send(Event::FindNext(!shift));
                return std::ptr::null_mut();
            }
            // The page sees the edit shortcuts first (Linear copies an issue on
            // Cmd+C); Keys runs the command for the ones it leaves unhandled.
            if matches!((key.as_str(), shift), ("c" | "v" | "x" | "a" | "z", false) | ("z", true))
                && let Some(responder) = event_window.firstResponder()
            {
                responder.keyDown(e);
                return std::ptr::null_mut();
            }
            event.as_ptr()
        });
        let monitor = unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(
                NSEventMask::KeyDown | NSEventMask::LeftMouseDown,
                &block,
            )
        };
        let id = NEXT_HOST_ID.fetch_add(1, Ordering::Relaxed);
        let layers = Rc::new(RefCell::new(HashMap::new()));
        let target_id = Rc::new(RefCell::new(None));
        let target_page = Rc::new(Cell::new(false));
        TARGET_HOSTS.with(|hosts| {
            hosts.borrow_mut().push(TargetHost {
                id,
                target_id: target_id.clone(),
                container: ObjcWeak::from_retained(&container),
                layers: layers.clone(),
            });
        });
        Ok(Self {
            id,
            browser,
            control: RefCell::new(None),
            layers,
            target_id,
            target_page,
            closing,
            container,
            parent,
            window: native_window,
            shown: Cell::new(false),
            allowed,
            find_open,
            bounds: Cell::new(None),
            cut: Cell::new(0.),
            focus_pending: Cell::new(true),
            focus_gate,
            monitor,
        })
    }
    pub fn starting(&self) -> bool {
        !self.closing.get() && self.browser.borrow().is_none()
    }
    pub fn has_page_target(&self) -> bool {
        self.target_page.get()
    }
    pub fn set_target_id(&self, target_id: String, is_page: bool) {
        *self.target_id.borrow_mut() = Some(target_id);
        if is_page {
            self.target_page.set(true);
        }
    }
    pub fn control(&self) -> Result<Rc<super::control::Control>, String> {
        if let Some(control) = self.control.borrow().as_ref() {
            return Ok(control.clone());
        }
        let browser = self.browser.borrow();
        let container = self.container.clone();
        let layers = self.layers.clone();
        let target_id = self.target_id.clone();
        let target_page = self.target_page.clone();
        let control = Rc::new(super::control::Control::new(
            browser.as_ref().ok_or("Browser is closed")?,
            Box::new(move |caller, x, y, action| move_pointer(&container, &layers, caller, x, y, action)),
            agent_cursor,
            cursor_idle_ms,
            Box::new(move |id, is_page| {
                *target_id.borrow_mut() = Some(id);
                if is_page {
                    target_page.set(true);
                }
            }),
        )?);
        *self.control.borrow_mut() = Some(control.clone());
        Ok(control)
    }
    fn native_view(&self) -> Option<&NSView> {
        let browser = self.browser.borrow();
        let ptr = browser.as_ref()?.host()?.window_handle().cast::<NSView>();
        // CEF owns this view until close(); callers stay on the UI thread.
        unsafe { ptr.as_ref() }
    }
    pub fn set_allowed(&self, allowed: bool) {
        self.allowed.set(allowed);
        if !allowed {
            self.focus_gate.store(false, Ordering::Relaxed);
        }
        if !allowed && self.is_native_focused() {
            self.focus_pending.set(true);
            self.focus_parent();
        }
        if !allowed && self.shown.replace(false) {
            self.container.setHidden(true);
        }
    }
    pub fn place(&self, bounds: Bounds<Pixels>, allowed: bool, clear: Option<Bounds<Pixels>>) {
        let rect = [
            f64::from(f32::from(bounds.origin.x)),
            f64::from(f32::from(bounds.origin.y)),
            f64::from(f32::from(bounds.size.width)),
            f64::from(f32::from(bounds.size.height)),
        ];
        // The page keeps its size and loses its bottom rows to `clear`, so it
        // neither reflows nor blanks while a toast is up.
        let cut = clear.filter(|c| c.intersects(&bounds)).map_or(0., |c| f64::from(f32::from(bounds.bottom() - c.top())));
        if !allowed || rect[2] < 1. || rect[3] - cut < 1. {
            self.set_allowed(false);
            return;
        }
        let y = if self.parent.isFlipped() {
            rect[1]
        } else {
            self.parent.bounds().size.height - rect[1] - rect[3] + cut
        };
        let view = self.native_view();
        if let Some(view) = view {
            view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewNotSizable);
        }
        self.container.setFrame(NSRect::new(
            NSPoint::new(rect[0], y),
            NSSize::new(rect[2], rect[3] - cut),
        ));
        self.cut.set(cut);
        if let Some(view) = view {
            view.setFrame(NSRect::new(NSPoint::new(0., -cut), NSSize::new(rect[2], rect[3])));
            view.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable,
            );
        }
        self.bounds.set(Some(rect));
        self.focus_gate.store(true, Ordering::Relaxed);
        if !self.shown.replace(true) {
            self.container.setHidden(false);
        }
        if self.focus_pending.replace(false) {
            self.focus();
        }
    }
    fn is_native_focused(&self) -> bool {
        focused(&self.container, &self.window)
    }
    pub fn is_focused(&self) -> bool {
        self.shown.get() && self.is_native_focused()
    }
    pub fn focus(&self) {
        if self.shown.get() {
            if let Some(host) = self.browser.borrow().as_ref().and_then(|b| b.host()) {
                host.set_focus(1);
            }
        } else {
            self.focus_pending.set(true);
        }
    }
    pub fn focus_parent(&self) {
        self.window.makeFirstResponder(Some(&self.parent));
    }
    pub fn navigate(&self, url: &str) -> anyhow::Result<()> {
        let frame = self
            .browser
            .borrow()
            .as_ref()
            .and_then(|b| b.main_frame())
            .ok_or_else(|| anyhow::anyhow!("Browser is closed"))?;
        frame.load_url(Some(&url.into()));
        Ok(())
    }
    pub fn set_find_open(&self, open: bool) {
        self.find_open.set(open);
    }
    pub fn find(&self, text: &str, forward: bool, find_next: bool) {
        if let Some(host) = self.browser.borrow().as_ref().and_then(|b| b.host()) {
            if text.is_empty() {
                host.stop_finding(1);
            } else {
                host.find(Some(&text.into()), forward as i32, 0, find_next as i32);
            }
        }
    }
    pub fn stop_finding(&self, clear_selection: bool) {
        if let Some(host) = self.browser.borrow().as_ref().and_then(|b| b.host()) {
            host.stop_finding(clear_selection as i32);
        }
    }
    pub fn command(&self, command: &str) -> Result<(), String> {
        let browser = self.browser.borrow();
        let browser = browser.as_ref().ok_or("Browser is closed")?;
        match command {
            "browser.back" => browser.go_back(),
            "browser.forward" => browser.go_forward(),
            "browser.reload" => browser.reload(),
            "browser.devtools" => {
                if let Some(host) = browser.host() {
                    host.show_dev_tools(
                        Some(&WindowInfo::default()),
                        None,
                        Some(&BrowserSettings::default()),
                        None,
                    );
                }
            }
            _ => return Err(format!("Unknown browser command: {command}")),
        }
        Ok(())
    }
    pub fn inspect(&self) -> Value {
        let browser = self.browser.borrow();
        let browser = browser.as_ref();
        let slots = self.layers.borrow();
        let cursor_moves: u64 = slots.values().map(|s| s.moves).sum();
        let cursor_clicks: u64 = slots.values().map(|s| s.clicks).sum();
        let agent_cursors = json!({
            "count": slots.len(),
            "moves": cursor_moves,
            "clicks": cursor_clicks,
        });
        drop(slots);
        json!({"url":page_url(browser), "bounds":self.bounds.get(), "shown":self.shown.get(), "cut":self.cut.get(), "focused":self.is_focused(), "window":self.window.windowNumber(), "scale_factor":self.window.backingScaleFactor(), "window_visible":self.window.isVisible(), "window_key":self.window.isKeyWindow(), "parent_hidden":self.parent.isHidden(), "parent_bounds":[self.parent.bounds().size.width,self.parent.bounds().size.height], "can_go_back":browser.is_some_and(|b| b.can_go_back() != 0), "can_go_forward":browser.is_some_and(|b| b.can_go_forward() != 0), "active_browsers":runtime::ACTIVE.load(Ordering::Relaxed), "pump_ticks":runtime::TICKS.load(Ordering::Relaxed), "agent_cursors":agent_cursors})
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        self.closing.set(true);
        TARGET_HOSTS.with(|hosts| hosts.borrow_mut().retain(|h| h.id != self.id));
        if let Some(monitor) = self.monitor.take() {
            unsafe {
                NSEvent::removeMonitor(&monitor);
            }
        }
        self.container.setHidden(true);
        runtime::close(&self.browser);
        self.container.removeFromSuperview();
    }
}
#[cfg(test)]
mod tests {
    use super::{
        CursorPrefs, DEFAULT_AGENT_CURSOR_IDLE_SECS, MAX_AGENT_CURSOR_IDLE_SECS,
        agent_ripple_svg, ctrl_cmd_shortcut, ctrl_shortcut, download_url, parse_cursor_prefs,
        shortcut,
    };
    #[test]
    fn cursor_prefs_parses_style_and_idle_timeout() {
        assert_eq!(
            parse_cursor_prefs("{}"),
            CursorPrefs { cute: false, idle_secs: DEFAULT_AGENT_CURSOR_IDLE_SECS }
        );
        assert_eq!(
            parse_cursor_prefs(r#"{"agent_cursor":"cute","agent_cursor_idle_secs":0}"#),
            CursorPrefs { cute: true, idle_secs: 0.0 }
        );
        assert_eq!(
            parse_cursor_prefs(r#"{"agent_cursor_idle_secs":15.5}"#),
            CursorPrefs { cute: false, idle_secs: 15.5 }
        );
        assert_eq!(
            parse_cursor_prefs(r#"{"agent_cursor_idle_secs":-3}"#),
            CursorPrefs { cute: false, idle_secs: DEFAULT_AGENT_CURSOR_IDLE_SECS }
        );
        assert_eq!(
            parse_cursor_prefs(r#"{"agent_cursor_idle_secs":99999}"#),
            CursorPrefs { cute: false, idle_secs: MAX_AGENT_CURSOR_IDLE_SECS }
        );
    }
    #[test]
    fn agent_ripple_svg_uses_caller_color() {
        let a = String::from_utf8(agent_ripple_svg(Some("caller-alpha"))).unwrap();
        let b = String::from_utf8(agent_ripple_svg(Some("caller-beta"))).unwrap();
        assert_ne!(a, b);
        assert!(a.contains("<circle") && a.contains(r##"stroke="#fff""##));
    }
    #[test]
    fn download_url_allows_web_and_web_origin_blob_urls_only() {
        assert!(download_url("https://developer.apple.com/cert.cer"));
        assert!(download_url("http://127.0.0.1:8080/report.csv"));
        assert!(download_url("blob:https://appstoreconnect.apple.com/1234-5678"));
        assert!(download_url("blob:http://localhost:3000/abcd"));
        assert!(!download_url("about:blank"));
        assert!(!download_url("file:///etc/passwd"));
        assert!(!download_url("blob:file:///etc/passwd"));
        assert!(!download_url("blob:null/1234"));
        assert!(!download_url("javascript:alert(1)"));
        assert!(!download_url("data:text/plain,hello"));
    }
    #[test]
    fn shifted_brackets_always_switch_herdr_tabs() {
        assert_eq!(shortcut("[", true, false), Some("tab.prev"));
        assert_eq!(shortcut("{", true, false), Some("tab.prev"));
        assert_eq!(shortcut("]", true, false), Some("tab.next"));
        assert_eq!(shortcut("}", true, false), Some("tab.next"));
        assert_eq!(shortcut("[", false, false), Some("browser.back"));
    }
    #[test]
    fn browser_and_sidebar_shortcuts_match_the_global_keymap() {
        let keys: serde_json::Value =
            serde_json::from_str(include_str!("../../defaults/keys.macos.json")).unwrap();
        assert_eq!(shortcut("t", false, false), Some("tab.new"));
        assert_eq!(
            shortcut("t", false, false),
            keys["global"]["cmd-t"].as_str()
        );
        assert_eq!(
            shortcut("b", false, false),
            keys["global"]["cmd-b"].as_str()
        );
        assert_eq!(
            shortcut("b", true, false),
            keys["global"]["cmd-shift-b"].as_str()
        );
        assert_eq!(
            shortcut("b", false, true),
            keys["global"]["cmd-alt-b"].as_str()
        );
        assert_eq!(shortcut("f", false, false), Some("browser.find"));
        assert_eq!(shortcut("l", false, true), None);
        assert_eq!(
            ctrl_shortcut("`", false),
            keys["global"]["ctrl-`"].as_str()
        );
        assert_eq!(
            ctrl_shortcut("`", true),
            keys["global"]["ctrl-shift-`"].as_str()
        );
        assert_eq!(ctrl_shortcut("~", true), Some("focus.cycle:prev"));
        assert_eq!(ctrl_cmd_shortcut("[", false), Some("tab.move:prev"));
        assert_eq!(ctrl_cmd_shortcut("]", false), Some("tab.move:next"));
        assert_eq!(ctrl_cmd_shortcut("b", false), Some("browser.split:right"));
        assert_eq!(keys["global"]["cmd-ctrl-["]["command"].as_str(), Some("tab.move"));
        assert_eq!(keys["global"]["cmd-ctrl-["]["args"]["by"].as_i64(), Some(-1));
        assert_eq!(keys["global"]["cmd-ctrl-]"]["command"].as_str(), Some("tab.move"));
        assert_eq!(keys["global"]["cmd-ctrl-]"]["args"]["by"].as_i64(), Some(1));
    }
}
