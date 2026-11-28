//! Native Chromium child views. CEF owns rendering, input and Retina scaling.
use super::{Event, PointerAction, PointerUpdate, runtime};
use cef::*;
use gpui::{Bounds, Pixels, Window as GpuiWindow};
use objc2::{
    DefinedClass, Encoding, MainThreadMarker, MainThreadOnly, RefEncode,
    class, define_class, msg_send,
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

#[repr(C)]
struct CGPath {
    _private: [u8; 0],
}

unsafe impl RefEncode for CGPath {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("CGPath", &[]));
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPathCreateMutable() -> *mut CGPath;
    fn CGPathAddRect(path: *mut CGPath, m: *const std::ffi::c_void, rect: NSRect);
    fn CGPathAddRoundedRect(
        path: *mut CGPath,
        m: *const std::ffi::c_void,
        rect: NSRect,
        corner_width: f64,
        corner_height: f64,
    );
    fn CGPathRelease(path: *const CGPath);
}

struct ContainerIvars {
    /// Active toast cutout rects in container-local bottom-left coordinates `[x, y, w, h]`.
    cutouts: Rc<RefCell<Vec<[f64; 4]>>>,
}

fn point_in_cutouts(point: NSPoint, cutouts: &[[f64; 4]]) -> bool {
    cutouts.iter().any(|r| {
        point.x >= r[0] && point.x <= r[0] + r[2] && point.y >= r[1] && point.y <= r[1] + r[3]
    })
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ContainerIvars]
    struct BrowserContainerView;

    impl BrowserContainerView {
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> *mut NSView {
            // `point` is in the superview's coordinate system; convert to local bounds.
            let superview = unsafe { self.superview() };
            let local = self.convertPoint_fromView(point, superview.as_deref());
            if point_in_cutouts(local, &self.ivars().cutouts.borrow()) {
                return std::ptr::null_mut();
            }
            unsafe { msg_send![super(self), hitTest: point] }
        }
    }
);

impl BrowserContainerView {
    fn new(
        mtm: MainThreadMarker,
        frame: NSRect,
        cutouts: Rc<RefCell<Vec<[f64; 4]>>>,
    ) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(ContainerIvars { cutouts });
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }
}

/// Convert window-space cutout rects (top-left origin) into container-local AppKit rects (bottom-left origin)
/// with per-cutout corner radii. Only `toast == true` cutouts contribute to the reported toast `cut` height.
fn local_toast_cutouts(
    bounds: [f64; 4],
    clear: &[([f64; 4], f64, bool)],
) -> (Vec<([f64; 4], f64)>, f64) {
    let mut out = Vec::new();
    let mut max_cut = 0.0_f64;
    let bx = bounds[0];
    let by = bounds[1];
    let bw = bounds[2];
    let bh = bounds[3];
    if bw < 1. || bh < 1. {
        return (out, 0.0);
    }
    for &(c, radius, is_toast) in clear {
        let x0 = (c[0] - bx).max(0.0);
        let y0 = (c[1] - by).max(0.0);
        let x1 = (c[0] + c[2] - bx).min(bw);
        let y1 = (c[1] + c[3] - by).min(bh);
        if x1 > x0 + 0.5 && y1 > y0 + 0.5 {
            // Preserve the unclamped overlay box so rounded corners (e.g. a
            // bottom-straddling update pill) match GPUI's exact corner arc.
            let lx = c[0] - bx;
            let ly = bh - (c[1] + c[3] - by);
            out.push(([lx, ly, c[2], c[3]], radius));
            if is_toast {
                max_cut = max_cut.max((bh - y0).clamp(0.0, bh));
            }
        }
    }
    (out, max_cut)
}

unsafe fn apply_cutout_mask(
    container: &NSView,
    width: f64,
    height: f64,
    cutouts: &[([f64; 4], f64)],
) {
    container.setWantsLayer(true);
    let root: *mut AnyObject = msg_send![container, layer];
    if root.is_null() {
        return;
    }
    let _: () = msg_send![class!(CATransaction), begin];
    let _: () = msg_send![class!(CATransaction), setDisableActions: true];
    if cutouts.is_empty() {
        let _: () = msg_send![root, setMask: std::ptr::null::<AnyObject>()];
    } else {
        let path = unsafe { CGPathCreateMutable() };
        unsafe {
            CGPathAddRect(
                path,
                std::ptr::null(),
                NSRect::new(NSPoint::new(0., 0.), NSSize::new(width, height)),
            );
            for &(c, corner_radius) in cutouts {
                let r = corner_radius.min(c[2] / 2.0).min(c[3] / 2.0).max(0.0);
                let rect = NSRect::new(NSPoint::new(c[0], c[1]), NSSize::new(c[2], c[3]));
                if r >= 0.5 {
                    CGPathAddRoundedRect(path, std::ptr::null(), rect, r, r);
                } else {
                    CGPathAddRect(path, std::ptr::null(), rect);
                }
            }
        }
        let mask: Retained<AnyObject> = msg_send![class!(CAShapeLayer), layer];
        let _: () = msg_send![&*mask, setFillRule: &*NSString::from_str("even-odd")];
        let _: () = msg_send![&*mask, setPath: path];
        let _: () = msg_send![root, setMask: &*mask];
        unsafe { CGPathRelease(path) };
    }
    let _: () = msg_send![class!(CATransaction), commit];
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
fn subframe_url(url: &str) -> bool {
    if web_url(url) || url == "about:srcdoc" {
        return true;
    }
    if let Some(inner) = url.strip_prefix("blob:") {
        return url::Url::parse(inner).is_ok_and(|u| matches!(u.scheme(), "http" | "https"));
    }
    url::Url::parse(url).is_ok_and(|u| matches!(u.scheme(), "data" | "javascript"))
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
        fn auth_credentials(&self, _browser: Option<&mut Browser>, origin: Option<&CefString>, proxy: i32, host: Option<&CefString>, port: i32, realm: Option<&CefString>, _scheme: Option<&CefString>, callback: Option<&mut AuthCallback>) -> i32 {
            // The pane asks for the sign-in itself: Chromium's own prompt
            // crashes the app when its pane closes first. A popup has no pane
            // to ask in, so its challenge is declined.
            let (Some(tx), Some(callback)) = (&self.tx, callback) else { return 0 };
            let callback = callback.clone();
            let reply: Box<dyn FnOnce(Option<(String, String)>)> = Box::new(move |login| match login {
                Some((user, password)) => callback.cont(Some(&user.as_str().into()), Some(&password.as_str().into())),
                None => callback.cancel(),
            });
            let origin = origin.map(CefString::to_string).unwrap_or_default();
            let host = host.map(CefString::to_string).unwrap_or_default();
            let realm = realm.map(CefString::to_string).unwrap_or_default();
            let _ = tx.try_send(Event::Auth(super::Auth {
                origin,
                host,
                port,
                is_proxy: proxy != 0,
                realm,
                reply: Some(reply),
            }));
            1
        }
        fn on_before_browse(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, request: Option<&mut Request>, _gesture: i32, _redirect: i32) -> i32 {
            let Some(url) = request.map(|r| CefString::from(&r.url()).to_string()) else { return 0 };
            if frame.is_some_and(|f| f.is_main() != 0) {
                return (!web_url(&url)) as i32;
            }
            (!subframe_url(&url)) as i32
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
    struct Life { tx: Tx, ask: bool, browser: std::rc::Weak<RefCell<Option<Browser>>>, closing: Rc<Cell<bool>>, pending: Rc<Cell<bool>> }
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
                if let Some(client) = client { *client = Some(PopupClient::new(self.ask)); }
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
    struct PopupClient { ask: bool }
    impl Client {
        fn request_handler(&self) -> Option<RequestHandler> { Some(Navigation::new(None)) }
        fn life_span_handler(&self) -> Option<LifeSpanHandler> { Some(PopupLife::new()) }
        fn download_handler(&self) -> Option<DownloadHandler> { Some(Download::new(None, self.ask)) }
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
const MAX_AGENT_DOWNLOAD_BYTES: i64 = 512 * 1024 * 1024;

wrap_download_handler! {
    // A person sees Chromium's save dialog; agent downloads save to the app home.
    struct Download { tx: Option<Tx>, ask: bool }
    impl DownloadHandler {
        fn can_download(&self, _browser: Option<&mut Browser>, url: Option<&CefString>, _method: Option<&CefString>) -> i32 {
            url.is_some_and(|url| download_url(&url.to_string())) as i32
        }
        fn on_before_download(&self, _browser: Option<&mut Browser>, item: Option<&mut DownloadItem>, name: Option<&CefString>, callback: Option<&mut BeforeDownloadCallback>) -> i32 {
            if !self.ask && item.as_ref().is_some_and(|i| i.total_bytes() > MAX_AGENT_DOWNLOAD_BYTES) {
                if let Some(tx) = &self.tx {
                    let _ = tx.try_send(Event::Notice("Refused unattended download over 512 MiB".into()));
                }
                return 1;
            }
            let folder = crate::mods::app_dir().join("downloads");
            if std::fs::create_dir_all(&folder).is_err() {
                if let Some(tx) = &self.tx {
                    let _ = tx.try_send(Event::Notice("Cannot create downloads folder".into()));
                }
                return 1;
            }
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700));
            }
            let suggested = name.map(CefString::to_string).unwrap_or_else(|| "download".into());
            let clean = std::path::Path::new(&suggested)
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.trim_matches('.'))
                .filter(|n| !n.is_empty())
                .unwrap_or("download");
            let mut path = folder.join(clean);
            let mut i = 1;
            while path.symlink_metadata().is_ok() {
                path = folder.join(format!("{i}-{clean}"));
                i += 1;
            }
            if let Some(callback) = callback {
                callback.cont(Some(&CefString::from(path.to_string_lossy().as_ref())), self.ask as i32);
                if let Some(tx) = &self.tx {
                    let _ = tx.try_send(Event::Notice(format!("Downloading to {}", path.display())));
                }
            }
            1
        }
        fn on_download_updated(&self, _browser: Option<&mut Browser>, item: Option<&mut DownloadItem>, callback: Option<&mut DownloadItemCallback>) {
            if let Some(item) = item {
                if !self.ask && item.received_bytes() > MAX_AGENT_DOWNLOAD_BYTES {
                    if let Some(callback) = callback {
                        callback.cancel();
                    }
                    if let Some(tx) = &self.tx {
                        let _ = tx.try_send(Event::Notice("Canceled unattended download over 512 MiB".into()));
                    }
                    return;
                }
                if let Some(tx) = &self.tx {
                    if item.is_complete() != 0 { let _ = tx.try_send(Event::Notice(format!("Download saved to {}", CefString::from(&item.full_path())))); }
                    else if item.is_canceled() != 0 { let _ = tx.try_send(Event::Notice("Download canceled".into())); }
                }
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

fn normalize_media_origin(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    let rest = if let Some(rest) = trimmed.strip_prefix("https://") {
        let authority = rest.split('/').next()?;
        if authority.is_empty() || authority.contains('@') {
            return None;
        }
        return Some(format!("https://{}", authority.to_ascii_lowercase()));
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        rest
    } else {
        return None;
    };
    let authority = rest.split('/').next()?;
    let host = if authority.starts_with('[') {
        authority.split(']').next()?.trim_start_matches('[')
    } else {
        authority.split(':').next()?
    };
    if matches!(host.to_ascii_lowercase().as_str(), "localhost" | "127.0.0.1" | "::1") {
        Some(format!("http://{}", authority.to_ascii_lowercase()))
    } else {
        None
    }
}

fn media_permission_kind(audio: bool, video: bool) -> Option<(&'static str, &'static str)> {
    match (audio, video) {
        (true, true) => Some(("camera_and_microphone", "camera and microphone")),
        (true, false) => Some(("microphone", "microphone")),
        (false, true) => Some(("camera", "camera")),
        (false, false) => None,
    }
}

fn media_permissions_path() -> std::path::PathBuf {
    crate::mods::app_dir().join("media-permissions.json")
}

fn load_media_permissions(path: &std::path::Path) -> HashMap<String, bool> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<HashMap<String, bool>>(&text).ok())
        .unwrap_or_default()
}

fn save_media_permissions(path: &std::path::Path, map: &HashMap<String, bool>) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(map) {
        let _ = std::fs::write(path, bytes);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
    }
}

fn prompt_media_permission_macos(origin: &str, label: &str) -> bool {
    autoreleasepool(|_| unsafe {
        let alert: Retained<AnyObject> = msg_send![class!(NSAlert), new];
        let title = NSString::from_str(&format!("Allow {origin} to use your {label}?"));
        let info = NSString::from_str("Herdr Browser will remember this choice for this site.");
        let allow = NSString::from_str("Allow");
        let deny = NSString::from_str("Don't Allow");
        let _: () = msg_send![&*alert, setMessageText: &*title];
        let _: () = msg_send![&*alert, setInformativeText: &*info];
        let _: *mut AnyObject = msg_send![&*alert, addButtonWithTitle: &*allow];
        let _: *mut AnyObject = msg_send![&*alert, addButtonWithTitle: &*deny];
        let response: isize = msg_send![&*alert, runModal];
        response == 1000
    })
}

fn decide_media_permission_with<F>(
    ask: bool,
    raw_origin: &str,
    audio: bool,
    video: bool,
    store_path: &std::path::Path,
    prompt: F,
) -> bool
where
    F: FnOnce(&str, &str) -> bool,
{
    if !ask {
        return false;
    }
    let Some((kind_key, label)) = media_permission_kind(audio, video) else {
        return false;
    };
    let Some(origin) = normalize_media_origin(raw_origin) else {
        return false;
    };
    let key = format!("{origin}|{kind_key}");
    let mut map = load_media_permissions(store_path);
    if let Some(&saved) = map.get(&key) {
        return saved;
    }
    let allowed = prompt(&origin, label);
    map.insert(key, allowed);
    save_media_permissions(store_path, &map);
    allowed
}

wrap_permission_handler! {
    struct Permissions { ask: bool }
    impl PermissionHandler {
        fn on_request_media_access_permission(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            requesting_origin: Option<&CefString>,
            requested_permissions: u32,
            callback: Option<&mut MediaAccessCallback>,
        ) -> i32 {
            let audio_bit = MediaAccessPermissionTypes::DEVICE_AUDIO_CAPTURE.get_raw();
            let video_bit = MediaAccessPermissionTypes::DEVICE_VIDEO_CAPTURE.get_raw();
            let device_mask = audio_bit | video_bit;
            let requested_device = requested_permissions & device_mask;
            if requested_device == 0 {
                return 0;
            }
            let origin = requesting_origin.map(CefString::to_string).unwrap_or_default();
            let allowed = decide_media_permission_with(
                self.ask,
                &origin,
                (requested_device & audio_bit) != 0,
                (requested_device & video_bit) != 0,
                &media_permissions_path(),
                prompt_media_permission_macos,
            );
            if let Some(callback) = callback {
                if allowed {
                    callback.cont(requested_device);
                } else {
                    callback.cancel();
                }
            }
            1
        }

        fn on_show_permission_prompt(
            &self,
            _browser: Option<&mut Browser>,
            _prompt_id: u64,
            requesting_origin: Option<&CefString>,
            requested_permissions: u32,
            callback: Option<&mut PermissionPromptCallback>,
        ) -> i32 {
            let mic_bit = PermissionRequestTypes::MIC_STREAM.get_raw();
            let cam_bit = PermissionRequestTypes::CAMERA_STREAM.get_raw();
            let media_mask = mic_bit | cam_bit;
            let requested_media = requested_permissions & media_mask;
            if requested_media == 0 {
                return 0;
            }
            let origin = requesting_origin.map(CefString::to_string).unwrap_or_default();
            let allowed = decide_media_permission_with(
                self.ask,
                &origin,
                (requested_media & mic_bit) != 0,
                (requested_media & cam_bit) != 0,
                &media_permissions_path(),
                prompt_media_permission_macos,
            );
            if let Some(callback) = callback {
                callback.cont(if allowed {
                    PermissionRequestResult::ACCEPT
                } else {
                    PermissionRequestResult::DENY
                });
            }
            1
        }
    }
}

wrap_client! {
    struct PageClient { tx: Tx, ask: bool, gate: Arc<AtomicBool>, browser: std::rc::Weak<RefCell<Option<Browser>>>, closing: Rc<Cell<bool>>, pending: Rc<Cell<bool>>, container: Retained<NSView> }
    impl Client {
        fn display_handler(&self) -> Option<DisplayHandler> { Some(Display::new(self.tx.clone())) }
        fn load_handler(&self) -> Option<LoadHandler> { Some(Load::new(self.tx.clone())) }
        fn request_handler(&self) -> Option<RequestHandler> { Some(Navigation::new(Some(self.tx.clone()))) }
        fn life_span_handler(&self) -> Option<LifeSpanHandler> { Some(Life::new(self.tx.clone(), self.ask, self.browser.clone(), self.closing.clone(), self.pending.clone())) }
        fn focus_handler(&self) -> Option<FocusHandler> { Some(Focus::new(self.gate.clone())) }
        fn keyboard_handler(&self) -> Option<KeyboardHandler> { Some(Keys::new()) }
        fn download_handler(&self) -> Option<DownloadHandler> { Some(Download::new(Some(self.tx.clone()), self.ask)) }
        fn find_handler(&self) -> Option<FindHandler> { Some(Find::new(self.tx.clone())) }
        fn permission_handler(&self) -> Option<PermissionHandler> { Some(Permissions::new(self.ask)) }
    }
}
struct CursorLayers {
    cursor: Retained<AnyObject>,
    badge: Retained<AnyObject>,
    ripple: Retained<AnyObject>,
    highlight: Retained<AnyObject>,
    rendered_color: &'static str,
    cute: bool,
    badge_label: String,
    badge_w: f64,
    badge_h: f64,
}

struct CursorSlot {
    layers: Option<CursorLayers>,
    color: &'static str,
    label: String,
    moves: u64,
    clicks: u64,
    highlights: u64,
    last_x: Option<f64>,
    last_y: Option<f64>,
    last_rect: Option<[f64; 4]>,
    last_rect_at: Option<Instant>,
    last_seen: Instant,
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
    cutouts: Rc<RefCell<Vec<[f64; 4]>>>,
    mask_state: RefCell<Option<(f64, f64, Vec<([f64; 4], f64)>)>>,
    focus_pending: Cell<bool>,
    focus_gate: Arc<AtomicBool>,
    monitor: Option<Retained<AnyObject>>,
}
const AGENT_CURSOR: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 28 28"><path d="M4 3.2 L4 21 Q4 23.2 5.9 21.7 L9.9 18.6 L12.9 24.5 Q13.7 26 15.2 25.3 L16.6 24.6 Q18 23.9 17.3 22.5 L14.4 16.9 L19.4 16.6 Q21.6 16.4 20 14.9 L6 2.2 Q4 0.9 4 3.2 Z" fill="#111111" stroke="#fff" stroke-width="1.8" stroke-linejoin="round"/></svg>"##;
const CUTE_AGENT_CURSOR: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 28 28"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#B69CFF"/><stop offset="1" stop-color="#7C5CFF"/></linearGradient></defs><path d="M4 3.2 L4 21 Q4 23.2 5.9 21.7 L9.9 18.6 L12.9 24.5 Q13.7 26 15.2 25.3 L16.6 24.6 Q18 23.9 17.3 22.5 L14.4 16.9 L19.4 16.6 Q21.6 16.4 20 14.9 L6 2.2 Q4 0.9 4 3.2 Z" fill="url(#g)" stroke="#fff" stroke-width="1.8" stroke-linejoin="round"/><circle cx="8" cy="11.6" r="1.25" fill="#2A1B5E"/><circle cx="11.5" cy="11.6" r="1.25" fill="#2A1B5E"/><circle cx="8.4" cy="11.1" r=".45" fill="#fff"/><circle cx="11.9" cy="11.1" r=".45" fill="#fff"/><ellipse cx="6.8" cy="14" rx="1.1" ry=".6" fill="#FF8FB8" opacity=".75"/><ellipse cx="12.8" cy="14" rx="1.1" ry=".6" fill="#FF8FB8" opacity=".75"/><path d="M8.9 13.5 Q9.75 14.5 10.6 13.5" fill="none" stroke="#2A1B5E" stroke-width=".8" stroke-linecap="round"/></svg>"##;

const CURSOR_SIZE: f64 = 20.;
const CURSOR_HOTSPOT_X: f64 = 4.5 * (CURSOR_SIZE / 28.);
const CURSOR_HOTSPOT_Y: f64 = 3.0 * (CURSOR_SIZE / 28.);
const RIPPLE_SIZE: f64 = 26.;
const NEUTRAL_CURSOR_COLOR: &str = "#111111";
const DEFAULT_AGENT_CURSOR_IDLE_SECS: f64 = 30.;
const MAX_AGENT_CURSOR_IDLE_SECS: f64 = 3600.;
const HIGHLIGHT_HOLD_SECS: f64 = 1.1;
const HIGHLIGHT_FADE_SECS: f64 = 0.45;
const BADGE_HEIGHT: f64 = 20.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CursorStyle {
    /// Plain neutral mouse for a single agent; distinct per-agent colors once >1 agent is active.
    Auto,
    /// Always use distinct per-agent colors.
    Colored,
    /// Always use the plain neutral mouse pointer.
    Neutral,
    /// Brighter cute cursor with a face.
    Cute,
    /// Hide the cursor overlay completely.
    Off,
}

impl CursorStyle {
    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Colored => "colored",
            Self::Neutral => "neutral",
            Self::Cute => "cute",
            Self::Off => "off",
        }
    }

    fn parse(raw: Option<&str>, default: Self) -> Self {
        match raw.map(str::trim) {
            Some("auto") => Self::Auto,
            Some("colored") => Self::Colored,
            Some("neutral") => Self::Neutral,
            Some("cute") => Self::Cute,
            Some("off" | "none") => Self::Off,
            _ => default,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct CursorPrefs {
    style: CursorStyle,
    labels: bool,
    highlight: bool,
    record_style: CursorStyle,
    /// Seconds before an idle cursor fades out; `0.0` keeps it visible.
    idle_secs: f64,
}

impl Default for CursorPrefs {
    fn default() -> Self {
        Self {
            style: CursorStyle::Auto,
            labels: false,
            highlight: true,
            record_style: CursorStyle::Neutral,
            idle_secs: DEFAULT_AGENT_CURSOR_IDLE_SECS,
        }
    }
}

fn parse_cursor_prefs(text: &str) -> CursorPrefs {
    let v = serde_json::from_str::<Value>(text).ok();
    let style = CursorStyle::parse(
        v.as_ref().and_then(|v| v.get("agent_cursor")).and_then(Value::as_str),
        CursorStyle::Auto,
    );
    let labels = v
        .as_ref()
        .and_then(|v| v.get("agent_cursor_labels"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let highlight = v
        .as_ref()
        .and_then(|v| v.get("agent_cursor_highlight"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let record_style = CursorStyle::parse(
        v.as_ref().and_then(|v| v.get("agent_cursor_record")).and_then(Value::as_str),
        CursorStyle::Neutral,
    );
    let idle_secs = v
        .as_ref()
        .and_then(|v| v.get("agent_cursor_idle_secs"))
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite() && *n >= 0.)
        .map(|n| n.min(MAX_AGENT_CURSOR_IDLE_SECS))
        .unwrap_or(DEFAULT_AGENT_CURSOR_IDLE_SECS);
    CursorPrefs { style, labels, highlight, record_style, idle_secs }
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
            .unwrap_or_default();
        *cache.borrow_mut() = Some((now, prefs));
        prefs
    })
}

pub(super) fn cursor_idle_ms() -> u64 {
    (cursor_prefs().idle_secs * 1000.).round() as u64
}

fn normalized_caller_key(caller: Option<&str>) -> &str {
    caller.map(str::trim).filter(|s| !s.is_empty()).unwrap_or("agent")
}

fn agent_slot_and_color(caller: Option<&str>) -> (usize, &'static str) {
    let key = normalized_caller_key(caller);
    let slot = AGENT_COLOR_SLOTS.with(|slots| {
        let mut slots = slots.borrow_mut();
        let next = slots.len();
        *slots.entry(key.to_owned()).or_insert(next)
    });
    (slot, AGENT_COLORS[slot % AGENT_COLORS.len()])
}

fn agent_color(caller: Option<&str>) -> &'static str {
    agent_slot_and_color(caller).1
}

/// Resolve the rendered fill color and cute flag for a live cursor slot given the active count on this pane.
fn resolve_cursor_appearance(style: CursorStyle, agent_color: &'static str, active_agents: usize) -> Option<(&'static str, bool)> {
    match style {
        CursorStyle::Off => None,
        CursorStyle::Neutral => Some((NEUTRAL_CURSOR_COLOR, false)),
        CursorStyle::Colored => Some((agent_color, false)),
        CursorStyle::Cute => Some((if active_agents > 1 { agent_color } else { "#7C5CFF" }, true)),
        CursorStyle::Auto => {
            if active_agents > 1 {
                Some((agent_color, false))
            } else {
                Some((NEUTRAL_CURSOR_COLOR, false))
            }
        }
    }
}

fn normalize_cursor_label(caller: Option<&str>, label: Option<&str>) -> String {
    let raw = label
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let c = caller.map(str::trim).filter(|s| !s.is_empty())?;
            if c.starts_with("cdp:") {
                Some("Playwright")
            } else {
                Some(c)
            }
        })
        .unwrap_or("Agent");
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = collapsed.chars();
    let head: String = chars.by_ref().take(20).collect();
    if chars.next().is_some() {
        let trimmed: String = head.chars().take(19).collect();
        format!("{trimmed}…")
    } else if head.is_empty() {
        "Agent".into()
    } else {
        head
    }
}

fn escape_xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn cursor_svg_for_color(color: &str, cute: bool) -> Vec<u8> {
    if cute {
        let svg = String::from_utf8_lossy(CUTE_AGENT_CURSOR).into_owned();
        if color == "#7C5CFF" {
            return svg.into_bytes();
        }
        return svg.replace(r#"fill="url(#g)""#, &format!(r#"fill="{color}""#)).into_bytes();
    }
    let svg = String::from_utf8_lossy(AGENT_CURSOR).into_owned();
    svg.replace(r##"fill="#111111""##, &format!(r#"fill="{color}""#)).into_bytes()
}

fn ripple_svg_for_color(color: &str) -> Vec<u8> {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="26" height="26" viewBox="0 0 32 32"><circle cx="16" cy="16" r="12" fill="none" stroke="#fff" stroke-opacity=".85" stroke-width="3.6"/><circle cx="16" cy="16" r="12" fill="none" stroke="{color}" stroke-opacity=".95" stroke-width="2"/></svg>"##
    )
    .into_bytes()
}

#[cfg(test)]
fn agent_ripple_svg(caller: Option<&str>) -> Vec<u8> {
    ripple_svg_for_color(agent_color(caller))
}

fn agent_badge_svg(color: &str, label: &str) -> (Vec<u8>, f64, f64) {
    let char_count = label.chars().count() as f64;
    let w = (char_count * 6.8 + 16.0).clamp(44.0, 172.0).ceil();
    let h = BADGE_HEIGHT;
    let escaped = escape_xml(label);
    let rw = (w - 2.0).max(1.0);
    let rh = (h - 2.0).max(1.0);
    let cx = w / 2.0;
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}"><rect x="1" y="1" width="{rw}" height="{rh}" rx="9" ry="9" fill="{color}" stroke="#fff" stroke-opacity=".92" stroke-width="1.5"/><text x="{cx}" y="13.8" text-anchor="middle" font-family="-apple-system, BlinkMacSystemFont, 'SF Pro Text', sans-serif" font-size="10.5" font-weight="600" fill="#fff">{escaped}</text></svg>"##
    );
    (svg.into_bytes(), w, h)
}

fn agent_highlight_svg(color: &str, width: f64, height: f64) -> (Vec<u8>, f64, f64) {
    let pad = 3.0;
    let w = (width.clamp(6.0, 4096.0) + pad * 2.0).ceil();
    let h = (height.clamp(6.0, 4096.0) + pad * 2.0).ceil();
    let rw = (w - 3.0).max(1.0);
    let rh = (h - 3.0).max(1.0);
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}"><rect x="1.5" y="1.5" width="{rw}" height="{rh}" rx="6" ry="6" fill="none" stroke="#fff" stroke-opacity=".7" stroke-width="3.6"/><rect x="1.5" y="1.5" width="{rw}" height="{rh}" rx="6" ry="6" fill="{color}" fill-opacity=".12" stroke="{color}" stroke-opacity=".95" stroke-width="2"/></svg>"##
    );
    (svg.into_bytes(), w, h)
}

fn resolve_pointer_position(
    incoming: Option<(f64, f64)>,
    last: Option<(f64, f64)>,
    bounds_w: f64,
    bounds_h: f64,
) -> Option<(f64, f64)> {
    let (x, y) = incoming.or(last)?;
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    Some((x.clamp(0., bounds_w), y.clamp(0., bounds_h)))
}

fn badge_origin(x: f64, y: f64, badge_w: f64, badge_h: f64, bounds_w: f64, bounds_h: f64) -> (f64, f64) {
    let mut bx = x + 13.0;
    let mut by = y + 16.0;
    if bx + badge_w + 8.0 > bounds_w {
        bx = (x - badge_w - 8.0).max(8.0);
    }
    if by + badge_h + 8.0 > bounds_h {
        by = (y - badge_h - 8.0).max(8.0);
    }
    (bx, by)
}

fn record_cursor_svg_for_prefs(caller: Option<&str>, prefs: CursorPrefs) -> Option<Vec<u8>> {
    match prefs.record_style {
        CursorStyle::Off => None,
        CursorStyle::Neutral | CursorStyle::Auto => Some(cursor_svg_for_color(NEUTRAL_CURSOR_COLOR, false)),
        CursorStyle::Colored => Some(cursor_svg_for_color(agent_color(caller), false)),
        CursorStyle::Cute => Some(cursor_svg_for_color("#7C5CFF", true)),
    }
}

fn record_cursor(caller: Option<&str>) -> Option<Vec<u8>> {
    record_cursor_svg_for_prefs(caller, cursor_prefs())
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

unsafe fn hide_layer(layer: &AnyObject) {
    let _: () = msg_send![layer, removeAnimationForKey: &*NSString::from_str("fade")];
    let _: () = msg_send![layer, removeAnimationForKey: &*NSString::from_str("highlightFade")];
    let _: () = msg_send![class!(CATransaction), begin];
    let _: () = msg_send![class!(CATransaction), setDisableActions: true];
    let _: () = msg_send![layer, setOpacity: 0_f32];
    let _: () = msg_send![class!(CATransaction), commit];
}

unsafe fn apply_fade(layer: &AnyObject, media_now: f64, idle_secs: f64) {
    if idle_secs <= 0. {
        let _: () = msg_send![layer, removeAnimationForKey: &*NSString::from_str("fade")];
        let _: () = msg_send![class!(CATransaction), begin];
        let _: () = msg_send![class!(CATransaction), setDisableActions: true];
        let _: () = msg_send![layer, setOpacity: 1_f32];
        let _: () = msg_send![class!(CATransaction), commit];
    } else {
        let _: () = msg_send![class!(CATransaction), begin];
        let _: () = msg_send![class!(CATransaction), setDisableActions: true];
        let _: () = msg_send![layer, setOpacity: 0_f32];
        let _: () = msg_send![class!(CATransaction), commit];
        let now: f64 = msg_send![layer, convertTime: media_now, fromLayer: std::ptr::null::<AnyObject>()];
        let fade: *mut AnyObject = msg_send![class!(CABasicAnimation), animationWithKeyPath: &*NSString::from_str("opacity")];
        let shown: *mut AnyObject = msg_send![class!(NSNumber), numberWithDouble: 1_f64];
        let _: () = msg_send![fade, setFromValue: shown];
        let hidden: *mut AnyObject = msg_send![class!(NSNumber), numberWithDouble: 0_f64];
        let _: () = msg_send![fade, setToValue: hidden];
        let _: () = msg_send![fade, setBeginTime: now + idle_secs];
        let _: () = msg_send![fade, setDuration: 0.4_f64];
        let _: () = msg_send![fade, setFillMode: &*NSString::from_str("backwards")];
        let _: () = msg_send![layer, addAnimation: fade, forKey: &*NSString::from_str("fade")];
    }
}

unsafe fn show_highlight(
    layer: &AnyObject,
    color: &str,
    rect: [f64; 4],
    bounds_h: f64,
    scale: f64,
    media_now: f64,
    hold_secs: f64,
) {
    let (svg, w, h) = agent_highlight_svg(color, rect[2], rect[3]);
    let size = NSSize::new(w, h);
    let pos = NSPoint::new(rect[0] - 3.0, bounds_h - (rect[1] - 3.0));
    let _: () = msg_send![class!(CATransaction), begin];
    let _: () = msg_send![class!(CATransaction), setDisableActions: true];
    let _: () = msg_send![layer, setBounds: NSRect::new(NSPoint::new(0., 0.), size)];
    let _: () = msg_send![layer, setPosition: pos];
    unsafe { set_layer_svg(layer, &svg, size, scale) };
    let _: () = msg_send![layer, setOpacity: 0_f32];
    let _: () = msg_send![class!(CATransaction), commit];

    let hnow: f64 = msg_send![layer, convertTime: media_now, fromLayer: std::ptr::null::<AnyObject>()];
    let fade: *mut AnyObject = msg_send![class!(CABasicAnimation), animationWithKeyPath: &*NSString::from_str("opacity")];
    let shown: *mut AnyObject = msg_send![class!(NSNumber), numberWithDouble: 1_f64];
    let hidden: *mut AnyObject = msg_send![class!(NSNumber), numberWithDouble: 0_f64];
    let _: () = msg_send![fade, setFromValue: shown];
    let _: () = msg_send![fade, setToValue: hidden];
    let _: () = msg_send![fade, setBeginTime: hnow + hold_secs];
    let _: () = msg_send![fade, setDuration: HIGHLIGHT_FADE_SECS];
    let _: () = msg_send![fade, setFillMode: &*NSString::from_str("backwards")];
    let _: () = msg_send![layer, addAnimation: fade, forKey: &*NSString::from_str("highlightFade")];
}

unsafe fn ensure_slot_layers(
    container: &NSView,
    slot: &mut CursorSlot,
    render_color: &'static str,
    cute: bool,
    scale: f64,
) -> Option<bool> {
    if slot.layers.is_some() {
        return Some(false);
    }
    container.setWantsLayer(true);
    let root: *mut AnyObject = msg_send![container, layer];
    if root.is_null() {
        return None;
    }

    let highlight: Retained<AnyObject> = msg_send![class!(CALayer), layer];
    let _: () = msg_send![&*highlight, setAnchorPoint: NSPoint::new(0.0, 1.0)];
    let _: () = msg_send![&*highlight, setZPosition: 999_998_f64];
    let _: () = msg_send![&*highlight, setOpacity: 0_f32];
    let _: () = msg_send![root, addSublayer: &*highlight];

    let ripple_size = NSSize::new(RIPPLE_SIZE, RIPPLE_SIZE);
    let ripple: Retained<AnyObject> = msg_send![class!(CALayer), layer];
    let _: () = msg_send![&*ripple, setBounds: NSRect::new(NSPoint::new(0., 0.), ripple_size)];
    let _: () = msg_send![&*ripple, setAnchorPoint: NSPoint::new(0.5, 0.5)];
    unsafe { set_layer_svg(&ripple, &ripple_svg_for_color(render_color), ripple_size, scale) };
    let _: () = msg_send![&*ripple, setZPosition: 999_999_f64];
    let _: () = msg_send![&*ripple, setOpacity: 0_f32];
    let _: () = msg_send![root, addSublayer: &*ripple];

    let size = NSSize::new(CURSOR_SIZE, CURSOR_SIZE);
    let cursor: Retained<AnyObject> = msg_send![class!(CALayer), layer];
    let _: () = msg_send![&*cursor, setBounds: NSRect::new(NSPoint::new(0., 0.), size)];
    let _: () = msg_send![&*cursor, setAnchorPoint: NSPoint::new(CURSOR_HOTSPOT_X / size.width, 1. - CURSOR_HOTSPOT_Y / size.height)];
    unsafe { set_layer_svg(&cursor, &cursor_svg_for_color(render_color, cute), size, scale) };
    let _: () = msg_send![&*cursor, setZPosition: 1e6_f64];
    let _: () = msg_send![&*cursor, setOpacity: 0_f32];
    let _: () = msg_send![&*cursor, setShadowOpacity: 0.35_f32];
    let _: () = msg_send![&*cursor, setShadowRadius: 3_f64];
    let _: () = msg_send![&*cursor, setShadowOffset: NSSize::new(0., -1.)];
    let _: () = msg_send![root, addSublayer: &*cursor];

    let (badge_svg, badge_w, badge_h) = agent_badge_svg(render_color, &slot.label);
    let badge_size = NSSize::new(badge_w, badge_h);
    let badge: Retained<AnyObject> = msg_send![class!(CALayer), layer];
    let _: () = msg_send![&*badge, setBounds: NSRect::new(NSPoint::new(0., 0.), badge_size)];
    let _: () = msg_send![&*badge, setAnchorPoint: NSPoint::new(0.0, 1.0)];
    unsafe { set_layer_svg(&badge, &badge_svg, badge_size, scale) };
    let _: () = msg_send![&*badge, setZPosition: 1_000_001_f64];
    let _: () = msg_send![&*badge, setOpacity: 0_f32];
    let _: () = msg_send![&*badge, setShadowOpacity: 0.28_f32];
    let _: () = msg_send![&*badge, setShadowRadius: 3_f64];
    let _: () = msg_send![&*badge, setShadowOffset: NSSize::new(0., -1.)];
    let _: () = msg_send![root, addSublayer: &*badge];

    slot.layers = Some(CursorLayers {
        cursor,
        badge,
        ripple,
        highlight,
        rendered_color: render_color,
        cute,
        badge_label: slot.label.clone(),
        badge_w,
        badge_h,
    });
    Some(true)
}

unsafe fn sync_slot_appearance(layers: &mut CursorLayers, label: &str, render_color: &'static str, cute: bool, scale: f64) {
    let color_changed = layers.rendered_color != render_color;
    if color_changed || layers.cute != cute {
        unsafe { set_layer_svg(&layers.cursor, &cursor_svg_for_color(render_color, cute), NSSize::new(CURSOR_SIZE, CURSOR_SIZE), scale) };
        layers.cute = cute;
    }
    if color_changed {
        unsafe { set_layer_svg(&layers.ripple, &ripple_svg_for_color(render_color), NSSize::new(RIPPLE_SIZE, RIPPLE_SIZE), scale) };
    }
    if color_changed || layers.badge_label != label {
        let (badge_svg, bw, bh) = agent_badge_svg(render_color, label);
        let badge_size = NSSize::new(bw, bh);
        let _: () = msg_send![&*layers.badge, setBounds: NSRect::new(NSPoint::new(0., 0.), badge_size)];
        unsafe { set_layer_svg(&layers.badge, &badge_svg, badge_size, scale) };
        layers.badge_label = label.to_owned();
        layers.badge_w = bw;
        layers.badge_h = bh;
    }
    layers.rendered_color = render_color;
}

fn active_slot_count(slots: &HashMap<String, CursorSlot>, now: Instant, idle_secs: f64) -> usize {
    slots
        .values()
        .filter(|s| {
            s.last_x.is_some()
                && s.last_y.is_some()
                && (idle_secs == 0. || now.saturating_duration_since(s.last_seen).as_secs_f64() < idle_secs)
        })
        .count()
}

pub(super) fn move_pointer_for_target(target_id: &str, update: PointerUpdate<'_>) -> bool {
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
    move_pointer(&container, &layers, update)
}

fn valid_rect(rect: Option<[f64; 4]>) -> Option<[f64; 4]> {
    rect.filter(|r| r.iter().all(|v| v.is_finite()) && r[2] >= 1. && r[3] >= 1.)
}

fn move_pointer(
    container: &NSView,
    slots: &RefCell<HashMap<String, CursorSlot>>,
    update: PointerUpdate<'_>,
) -> bool {
    let pos = match update.pos {
        Some((x, y)) if x.is_finite() && y.is_finite() => Some((x, y)),
        Some(_) => return false,
        None => None,
    };
    let rect = valid_rect(update.rect);
    let key = normalized_caller_key(update.caller).to_owned();
    let label = normalize_cursor_label(update.caller, update.label);
    let color = agent_color(Some(&key));
    let now_instant = Instant::now();
    let prefs = cursor_prefs();

    let mut slots = slots.borrow_mut();
    // Coordinate-less activity (read, snapshot, non-DOM eval) only keeps an already-visible
    // cursor warm at its last real target; it never spawns a parked cursor or resurrects an
    // expired one.
    if pos.is_none() {
        let can_refresh = slots.get(&key).is_some_and(|s| {
            s.last_x.is_some()
                && s.last_y.is_some()
                && (prefs.idle_secs == 0. || now_instant.saturating_duration_since(s.last_seen).as_secs_f64() < prefs.idle_secs)
        });
        if !can_refresh {
            return false;
        }
    }
    let slot = slots.entry(key.clone()).or_insert_with(|| CursorSlot {
        layers: None,
        color,
        label: label.clone(),
        moves: 0,
        clicks: 0,
        highlights: 0,
        last_x: None,
        last_y: None,
        last_rect: None,
        last_rect_at: None,
        last_seen: now_instant,
    });
    slot.label = label;
    slot.last_seen = now_instant;
    slot.moves += 1;
    if let Some((x, y)) = pos {
        slot.last_x = Some(x);
        slot.last_y = Some(y);
    }
    if let Some(r) = rect {
        slot.last_rect = Some(r);
        slot.last_rect_at = Some(now_instant);
        slot.highlights += 1;
    }
    if matches!(update.action, PointerAction::Click | PointerAction::Tap) {
        slot.clicks += 1;
    }

    if prefs.style == CursorStyle::Off {
        unsafe {
            for s in slots.values() {
                if let Some(layers) = &s.layers {
                    hide_layer(&layers.cursor);
                    hide_layer(&layers.badge);
                    hide_layer(&layers.highlight);
                }
            }
        }
        return false;
    }

    let Some(window) = container.window() else { return false };
    let bounds = container.bounds().size;
    if container.isHidden() || !window.isVisible() || bounds.width < 1. || bounds.height < 1. {
        return false;
    }

    let active_agents = active_slot_count(&slots, now_instant, prefs.idle_secs);
    let scale = window.backingScaleFactor();
    // If Auto mode just transitioned between 1 agent (neutral) and >1 agents (colored),
    // re-tint sibling active cursors on this pane right away so every agent is distinct.
    unsafe {
        for (other_key, other) in slots.iter_mut() {
            if other_key == &key {
                continue;
            }
            if let Some((other_color, other_cute)) = resolve_cursor_appearance(prefs.style, other.color, active_agents)
                && let Some(layers) = other.layers.as_mut()
            {
                sync_slot_appearance(layers, &other.label, other_color, other_cute, scale);
                if !prefs.labels {
                    hide_layer(&layers.badge);
                }
            }
        }
    }

    let slot = slots.get_mut(&key).expect("cursor slot");
    let Some((render_color, cute)) = resolve_cursor_appearance(prefs.style, slot.color, active_agents) else {
        return false;
    };
    let Some((x, y)) = resolve_pointer_position(None, slot.last_x.zip(slot.last_y), bounds.width, bounds.height) else {
        return false;
    };

    let position = NSPoint::new(x, bounds.height - y);
    unsafe {
        let Some(fresh) = ensure_slot_layers(container, slot, render_color, cute, scale) else {
            return false;
        };
        let label = slot.label.clone();
        let layers = slot.layers.as_mut().expect("cursor layers");
        sync_slot_appearance(layers, &label, render_color, cute, scale);
        let (bx, by) = badge_origin(x, y, layers.badge_w, layers.badge_h, bounds.width, bounds.height);
        let badge_pos = NSPoint::new(bx, bounds.height - by);
        let glide = match update.action {
            PointerAction::Click => 0.3_f64,
            PointerAction::Scroll => 0.2_f64,
            PointerAction::Move => 0.12_f64,
            PointerAction::Tap => 0.1_f64,
            PointerAction::Activity => 0.18_f64,
        };
        let _: () = msg_send![class!(CATransaction), begin];
        let _: () = msg_send![class!(CATransaction), setDisableActions: fresh];
        let _: () = msg_send![class!(CATransaction), setAnimationDuration: glide];
        let _: () = msg_send![&*layers.cursor, setPosition: position];
        let _: () = msg_send![&*layers.badge, setPosition: badge_pos];
        let _: () = msg_send![class!(CATransaction), commit];

        let media_now = CACurrentMediaTime();
        if prefs.highlight {
            if let Some(r) = rect {
                show_highlight(&layers.highlight, render_color, r, bounds.height, scale, media_now, HIGHLIGHT_HOLD_SECS);
            }
        } else {
            hide_layer(&layers.highlight);
        }

        if matches!(update.action, PointerAction::Click | PointerAction::Tap) {
            let now: f64 = msg_send![&*layers.cursor, convertTime: media_now, fromLayer: std::ptr::null::<AnyObject>()];
            let delay = match update.action {
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
            let _: () = msg_send![&*layers.cursor, addAnimation: squish, forKey: &*NSString::from_str("squish")];

            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: true];
            let _: () = msg_send![&*layers.ripple, setPosition: position];
            let _: () = msg_send![class!(CATransaction), commit];

            let rnow: f64 = msg_send![&*layers.ripple, convertTime: media_now, fromLayer: std::ptr::null::<AnyObject>()];
            let ease: *mut AnyObject = msg_send![class!(CAMediaTimingFunction), functionWithName: &*NSString::from_str("easeOut")];
            let rscale: *mut AnyObject = msg_send![class!(CABasicAnimation), animationWithKeyPath: &*NSString::from_str("transform.scale")];
            let sfrom: *mut AnyObject = msg_send![class!(NSNumber), numberWithDouble: 0.5_f64];
            let sto: *mut AnyObject = msg_send![class!(NSNumber), numberWithDouble: 1.9_f64];
            let _: () = msg_send![rscale, setFromValue: sfrom];
            let _: () = msg_send![rscale, setToValue: sto];
            let _: () = msg_send![rscale, setDuration: 0.45_f64];
            let _: () = msg_send![rscale, setBeginTime: rnow + delay];
            let _: () = msg_send![rscale, setTimingFunction: ease];
            let _: () = msg_send![&*layers.ripple, addAnimation: rscale, forKey: &*NSString::from_str("rippleScale")];

            let ropacity: *mut AnyObject = msg_send![class!(CABasicAnimation), animationWithKeyPath: &*NSString::from_str("opacity")];
            let ofrom: *mut AnyObject = msg_send![class!(NSNumber), numberWithDouble: 0.95_f64];
            let oto: *mut AnyObject = msg_send![class!(NSNumber), numberWithDouble: 0.0_f64];
            let _: () = msg_send![ropacity, setFromValue: ofrom];
            let _: () = msg_send![ropacity, setToValue: oto];
            let _: () = msg_send![ropacity, setDuration: 0.45_f64];
            let _: () = msg_send![ropacity, setBeginTime: rnow + delay];
            let _: () = msg_send![ropacity, setTimingFunction: ease];
            let _: () = msg_send![&*layers.ripple, addAnimation: ropacity, forKey: &*NSString::from_str("rippleOpacity")];
        }

        apply_fade(&layers.cursor, media_now, prefs.idle_secs);
        if prefs.labels {
            apply_fade(&layers.badge, media_now, prefs.idle_secs);
        } else {
            hide_layer(&layers.badge);
        }
    }
    true
}

fn sync_visible_cursors(container: &NSView, slots: &RefCell<HashMap<String, CursorSlot>>) {
    let Some(window) = container.window() else { return };
    let bounds = container.bounds().size;
    if container.isHidden() || !window.isVisible() || bounds.width < 1. || bounds.height < 1. {
        return;
    }
    let prefs = cursor_prefs();
    let scale = window.backingScaleFactor();
    let now_instant = Instant::now();
    let mut slots = slots.borrow_mut();
    if prefs.style == CursorStyle::Off {
        unsafe {
            for s in slots.values() {
                if let Some(layers) = &s.layers {
                    hide_layer(&layers.cursor);
                    hide_layer(&layers.badge);
                    hide_layer(&layers.highlight);
                }
            }
        }
        return;
    }
    let active_agents = active_slot_count(&slots, now_instant, prefs.idle_secs);
    unsafe {
        let media_now = CACurrentMediaTime();
        for slot in slots.values_mut() {
            let elapsed = now_instant.saturating_duration_since(slot.last_seen).as_secs_f64();
            if prefs.idle_secs > 0. && elapsed >= prefs.idle_secs {
                continue;
            }
            let Some((render_color, cute)) = resolve_cursor_appearance(prefs.style, slot.color, active_agents) else {
                continue;
            };
            let remaining = if prefs.idle_secs == 0. {
                0.
            } else {
                (prefs.idle_secs - elapsed).max(0.5)
            };
            let Some((x, y)) = resolve_pointer_position(None, slot.last_x.zip(slot.last_y), bounds.width, bounds.height) else {
                continue;
            };
            if ensure_slot_layers(container, slot, render_color, cute, scale).is_none() {
                continue;
            }
            let label = slot.label.clone();
            let last_rect = slot.last_rect;
            let last_rect_at = slot.last_rect_at;
            let layers = slot.layers.as_mut().expect("cursor layers");
            sync_slot_appearance(layers, &label, render_color, cute, scale);
            let position = NSPoint::new(x, bounds.height - y);
            let (bx, by) = badge_origin(x, y, layers.badge_w, layers.badge_h, bounds.width, bounds.height);
            let badge_pos = NSPoint::new(bx, bounds.height - by);
            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: true];
            let _: () = msg_send![&*layers.cursor, setPosition: position];
            let _: () = msg_send![&*layers.badge, setPosition: badge_pos];
            let _: () = msg_send![class!(CATransaction), commit];
            apply_fade(&layers.cursor, media_now, remaining);
            if prefs.labels {
                apply_fade(&layers.badge, media_now, remaining);
            } else {
                hide_layer(&layers.badge);
            }
            if prefs.highlight {
                if let (Some(r), Some(at)) = (last_rect, last_rect_at) {
                    let rect_age = now_instant.saturating_duration_since(at).as_secs_f64();
                    if rect_age < HIGHLIGHT_HOLD_SECS + 0.5 {
                        let hold = (HIGHLIGHT_HOLD_SECS - rect_age).max(0.3);
                        show_highlight(&layers.highlight, render_color, r, bounds.height, scale, media_now, hold);
                    }
                }
            } else {
                hide_layer(&layers.highlight);
            }
        }
    }
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
        ("=", false) => Some("pane.equalize"),
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
        let cutouts = Rc::new(RefCell::new(Vec::new()));
        let container_view = BrowserContainerView::new(
            mtm,
            NSRect::new(NSPoint::new(0., 0.), initial),
            cutouts.clone(),
        );
        let container: Retained<NSView> = Retained::into_super(container_view);
        container.setWantsLayer(true);
        container.setHidden(true);
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
        let event_cutouts = cutouts.clone();
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
                let in_cutout = point_in_cutouts(p, &event_cutouts.borrow());
                if !in_cutout && p.x >= 0. && p.y >= 0. && p.x < b.size.width && p.y < b.size.height {
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
            cutouts,
            mask_state: RefCell::new(None),
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
            Box::new(move |update| move_pointer(&container, &layers, update)),
            record_cursor,
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
    pub fn clear_agent_cursors(&self) {
        let mut slots = self.layers.borrow_mut();
        for slot in slots.values_mut() {
            slot.last_x = None;
            slot.last_y = None;
            slot.last_rect = None;
            slot.last_rect_at = None;
            if let Some(layers) = &slot.layers {
                unsafe {
                    hide_layer(&layers.cursor);
                    hide_layer(&layers.badge);
                    hide_layer(&layers.highlight);
                }
            }
        }
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
    pub fn place(&self, bounds: Bounds<Pixels>, allowed: bool, clear: &[super::ClearCutout]) {
        let rect = [
            f64::from(f32::from(bounds.origin.x)),
            f64::from(f32::from(bounds.origin.y)),
            f64::from(f32::from(bounds.size.width)),
            f64::from(f32::from(bounds.size.height)),
        ];
        if !allowed || rect[2] < 1. || rect[3] < 1. {
            self.set_allowed(false);
            return;
        }
        let clear_rects: Vec<([f64; 4], f64, bool)> = clear
            .iter()
            .map(|c| {
                (
                    [
                        f64::from(f32::from(c.bounds.origin.x)),
                        f64::from(f32::from(c.bounds.origin.y)),
                        f64::from(f32::from(c.bounds.size.width)),
                        f64::from(f32::from(c.bounds.size.height)),
                    ],
                    f64::from(f32::from(c.radius)),
                    c.toast,
                )
            })
            .collect();
        let (local_cutouts, cut) = local_toast_cutouts(rect, &clear_rects);
        // Keep the container at the full browser pane size and punch out only
        // the exact rounded toast/pill card rect(s) so the rest of the bottom strip stays rendered.
        let y = if self.parent.isFlipped() {
            rect[1]
        } else {
            self.parent.bounds().size.height - rect[1] - rect[3]
        };
        let view = self.native_view();
        if let Some(view) = view {
            view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewNotSizable);
        }
        self.container.setFrame(NSRect::new(
            NSPoint::new(rect[0], y),
            NSSize::new(rect[2], rect[3]),
        ));
        self.cut.set(cut);
        *self.cutouts.borrow_mut() = local_cutouts.iter().map(|(r, _)| *r).collect();
        if let Some(view) = view {
            view.setFrame(NSRect::new(NSPoint::new(0., 0.), NSSize::new(rect[2], rect[3])));
            view.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable,
            );
        }
        let prev_bounds = self.bounds.replace(Some(rect));
        self.focus_gate.store(true, Ordering::Relaxed);
        let became_shown = !self.shown.replace(true);
        if became_shown {
            self.container.setHidden(false);
        }
        let next_mask = (rect[2], rect[3], local_cutouts);
        let mask_changed = self.mask_state.borrow().as_ref() != Some(&next_mask);
        if mask_changed || became_shown {
            unsafe {
                apply_cutout_mask(&self.container, rect[2], rect[3], &next_mask.2);
            }
            *self.mask_state.borrow_mut() = Some(next_mask);
        }
        let resized = prev_bounds.is_none_or(|p| (p[2] - rect[2]).abs() > 0.5 || (p[3] - rect[3]).abs() > 0.5);
        let has_pending = self.layers.borrow().values().any(|s| s.layers.is_none());
        if became_shown || resized || has_pending {
            sync_visible_cursors(&self.container, &self.layers);
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
        let prefs = cursor_prefs();
        let slots = self.layers.borrow();
        let cursor_moves: u64 = slots.values().map(|s| s.moves).sum();
        let cursor_clicks: u64 = slots.values().map(|s| s.clicks).sum();
        let cursor_highlights: u64 = slots.values().map(|s| s.highlights).sum();
        let mut agents: Vec<Value> = slots
            .iter()
            .map(|(caller, s)| {
                json!({
                    "caller": caller,
                    "label": s.label,
                    "color": s.color,
                    "rendered_color": s.layers.as_ref().map(|l| l.rendered_color),
                    "x": s.last_x,
                    "y": s.last_y,
                    "moves": s.moves,
                    "clicks": s.clicks,
                    "highlights": s.highlights,
                    "rendered": s.layers.is_some(),
                })
            })
            .collect();
        agents.sort_by(|a, b| a["caller"].as_str().cmp(&b["caller"].as_str()));
        let agent_cursors = json!({
            "style": prefs.style.as_str(),
            "labels": prefs.labels,
            "highlight": prefs.highlight,
            "record_style": prefs.record_style.as_str(),
            "count": slots.len(),
            "moves": cursor_moves,
            "clicks": cursor_clicks,
            "highlights": cursor_highlights,
            "agents": agents,
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
        AGENT_COLORS, AGENT_CURSOR, CUTE_AGENT_CURSOR, CURSOR_SIZE, CursorPrefs, CursorStyle,
        DEFAULT_AGENT_CURSOR_IDLE_SECS, MAX_AGENT_CURSOR_IDLE_SECS, NEUTRAL_CURSOR_COLOR,
        agent_badge_svg, agent_color, agent_highlight_svg, agent_ripple_svg, badge_origin,
        ctrl_cmd_shortcut, ctrl_shortcut, download_url, local_toast_cutouts,
        normalize_cursor_label, parse_cursor_prefs, point_in_cutouts, record_cursor_svg_for_prefs,
        resolve_cursor_appearance, resolve_pointer_position, shortcut, subframe_url,
    };
    #[test]
    fn local_toast_cutouts_punches_only_intersecting_toast_rects() {
        // Browser pane at (200, 100), size 1000x600 (window y: 100..700).
        // Toast at bottom-right (844, 612), size 340x72 (window x: 844..1184, y: 612..684).
        let (cutouts, cut) = local_toast_cutouts([200., 100., 1000., 600.], &[([844., 612., 340., 72.], 8.0, true)]);
        assert_eq!(cutouts, vec![([644., 16., 340., 72.], 8.0)]);
        assert_eq!(cut, 88.0);
        // Point inside toast cutout passes through; point on left bottom strip stays in browser.
        let rects: Vec<[f64; 4]> = cutouts.iter().map(|(r, _)| *r).collect();
        assert!(point_in_cutouts(super::NSPoint::new(700., 40.), &rects));
        assert!(!point_in_cutouts(super::NSPoint::new(100., 40.), &rects));
        // Update pill masks out its full unclamped stadium box without inflating toast `cut`.
        let (pill_cutouts, pill_cut) = local_toast_cutouts([200., 100., 1000., 600.], &[([920., 675., 260., 34.], 17.0, false)]);
        assert_eq!(pill_cutouts, vec![([720., -9., 260., 34.], 17.0)]);
        assert_eq!(pill_cut, 0.0);
        // Non-intersecting toast produces no cutouts.
        let (none, cut0) = local_toast_cutouts([200., 100., 400., 600.], &[([844., 612., 340., 72.], 8.0, true)]);
        assert!(none.is_empty() && cut0 == 0.0);
    }
    #[test]
    fn subframe_url_blocks_external_protocols() {
        assert!(subframe_url("https://example.com/embed"));
        assert!(subframe_url("about:blank"));
        assert!(subframe_url("about:srcdoc"));
        assert!(subframe_url("blob:https://example.com/1234"));
        assert!(subframe_url("data:text/html,hi"));
        assert!(!subframe_url("file:///etc/passwd"));
        assert!(!subframe_url("ssh://evil.example"));
        assert!(!subframe_url("vscode://file/etc/passwd"));
        assert!(!subframe_url("blob:file:///etc/passwd"));
    }
    #[test]
    fn cursor_prefs_parses_style_and_idle_timeout() {
        assert_eq!(DEFAULT_AGENT_CURSOR_IDLE_SECS, 30.0);
        assert_eq!(parse_cursor_prefs("{}"), CursorPrefs::default());
        assert_eq!(
            parse_cursor_prefs(r#"{"agent_cursor":"cute","agent_cursor_labels":true,"agent_cursor_highlight":false,"agent_cursor_record":"off","agent_cursor_idle_secs":0}"#),
            CursorPrefs {
                style: CursorStyle::Cute,
                labels: true,
                highlight: false,
                record_style: CursorStyle::Off,
                idle_secs: 0.0,
            }
        );
        assert_eq!(
            parse_cursor_prefs(r#"{"agent_cursor":"neutral","agent_cursor_idle_secs":15.5}"#),
            CursorPrefs { style: CursorStyle::Neutral, idle_secs: 15.5, ..CursorPrefs::default() }
        );
        assert_eq!(
            parse_cursor_prefs(r#"{"agent_cursor_idle_secs":-3}"#),
            CursorPrefs::default()
        );
        assert_eq!(
            parse_cursor_prefs(r#"{"agent_cursor_idle_secs":99999}"#),
            CursorPrefs { idle_secs: MAX_AGENT_CURSOR_IDLE_SECS, ..CursorPrefs::default() }
        );
    }
    #[test]
    fn auto_style_uses_plain_mouse_for_one_agent_and_colors_for_multiple() {
        assert_eq!(
            resolve_cursor_appearance(CursorStyle::Auto, "#3E63DD", 1),
            Some((NEUTRAL_CURSOR_COLOR, false))
        );
        assert_eq!(
            resolve_cursor_appearance(CursorStyle::Auto, "#3E63DD", 2),
            Some(("#3E63DD", false))
        );
        assert_eq!(
            resolve_cursor_appearance(CursorStyle::Colored, "#E5484D", 1),
            Some(("#E5484D", false))
        );
        assert_eq!(
            resolve_cursor_appearance(CursorStyle::Neutral, "#E5484D", 3),
            Some((NEUTRAL_CURSOR_COLOR, false))
        );
        assert_eq!(resolve_cursor_appearance(CursorStyle::Off, "#E5484D", 2), None);
        // Recording defaults to a plain neutral mouse arrow and respects Off/Colored.
        let rec_default = String::from_utf8(record_cursor_svg_for_prefs(Some("a"), CursorPrefs::default()).unwrap()).unwrap();
        assert!(rec_default.contains(NEUTRAL_CURSOR_COLOR));
        assert!(record_cursor_svg_for_prefs(Some("a"), CursorPrefs { record_style: CursorStyle::Off, ..CursorPrefs::default() }).is_none());
        let rec_colored = String::from_utf8(
            record_cursor_svg_for_prefs(Some("a"), CursorPrefs { record_style: CursorStyle::Colored, ..CursorPrefs::default() }).unwrap()
        ).unwrap();
        assert!(rec_colored.contains(agent_color(Some("a"))));
    }
    #[test]
    fn agent_ripple_svg_uses_caller_color() {
        let a = String::from_utf8(agent_ripple_svg(Some("caller-alpha"))).unwrap();
        let b = String::from_utf8(agent_ripple_svg(Some("caller-beta"))).unwrap();
        assert_ne!(a, b);
        assert!(a.contains("<circle") && a.contains(r##"stroke="#fff""##));
        // Unlabeled callers still receive a real palette color instead of translucent black.
        assert!(AGENT_COLORS.contains(&agent_color(None)));
    }
    #[test]
    fn badge_and_highlight_svgs_render_escaped_labels_and_colors() {
        assert_eq!(normalize_cursor_label(None, None), "Agent");
        assert_eq!(normalize_cursor_label(Some("cdp:3"), None), "Playwright");
        assert_eq!(normalize_cursor_label(Some("t1v6"), Some("  claude  \t  demo  ")), "claude demo");
        let long = normalize_cursor_label(None, Some("very-long-agent-display-name-that-exceeds-twenty"));
        assert!(long.ends_with('…') && long.chars().count() == 20, "{long}");
        let (badge, bw, bh) = agent_badge_svg("#E5484D", "a < b & c");
        let badge = String::from_utf8(badge).unwrap();
        assert!(bw >= 44.0 && bh == 20.0);
        assert!(badge.contains("#E5484D") && badge.contains("a &lt; b &amp; c"));
        let (hi, hw, hh) = agent_highlight_svg("#30A46C", 120.0, 36.0);
        let hi = String::from_utf8(hi).unwrap();
        assert!(hw == 126.0 && hh == 42.0);
        assert!(hi.contains("#30A46C") && hi.contains(r##"fill-opacity=".12""##));
    }
    #[test]
    fn badge_origin_flips_inside_viewport_edges() {
        assert_eq!(badge_origin(100.0, 100.0, 80.0, 20.0, 800.0, 600.0), (113.0, 116.0));
        // Near right & bottom edges, flips left and up so the label never clips.
        let (bx, by) = badge_origin(760.0, 580.0, 80.0, 20.0, 800.0, 600.0);
        assert!(bx + 80.0 <= 800.0 && by + 20.0 <= 600.0, "({bx}, {by})");
    }
    #[test]
    fn pointer_position_never_invents_parked_coords_and_uses_native_cursor_scale() {
        assert_eq!(resolve_pointer_position(None, None, 1200.0, 800.0), None);
        assert_eq!(resolve_pointer_position(Some((250.0, 180.0)), None, 1200.0, 800.0), Some((250.0, 180.0)));
        assert_eq!(resolve_pointer_position(None, Some((250.0, 180.0)), 1200.0, 800.0), Some((250.0, 180.0)));
        assert_eq!(resolve_pointer_position(Some((1500.0, -10.0)), None, 1200.0, 800.0), Some((1200.0, 0.0)));
        assert_eq!(resolve_pointer_position(Some((f64::NAN, 10.0)), Some((50.0, 50.0)), 1200.0, 800.0), None);
        assert_eq!(CURSOR_SIZE, 20.0);
        assert!(String::from_utf8_lossy(AGENT_CURSOR).contains(r#"width="20" height="20""#));
        assert!(String::from_utf8_lossy(CUTE_AGENT_CURSOR).contains(r#"width="20" height="20""#));
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
        assert_eq!(ctrl_cmd_shortcut("=", false), Some("pane.equalize"));
        assert_eq!(
            ctrl_cmd_shortcut("=", false),
            keys["global"]["cmd-ctrl-="].as_str()
        );
        assert_eq!(ctrl_cmd_shortcut("b", false), Some("browser.split:right"));
        assert_eq!(keys["global"]["cmd-ctrl-["]["command"].as_str(), Some("tab.move"));
        assert_eq!(keys["global"]["cmd-ctrl-["]["args"]["by"].as_i64(), Some(-1));
        assert_eq!(keys["global"]["cmd-ctrl-]"]["command"].as_str(), Some("tab.move"));
        assert_eq!(keys["global"]["cmd-ctrl-]"]["args"]["by"].as_i64(), Some(1));
    }
    #[test]
    fn media_permission_policy_denies_agent_and_persists_person_choices() {
        use super::{decide_media_permission_with, normalize_media_origin};
        assert_eq!(
            normalize_media_origin("https://JCVOLPE.ME/admin"),
            Some("https://jcvolpe.me".into())
        );
        assert_eq!(
            normalize_media_origin("http://127.0.0.1:3000/"),
            Some("http://127.0.0.1:3000".into())
        );
        assert_eq!(normalize_media_origin("http://example.com/"), None);
        assert_eq!(normalize_media_origin("file:///tmp/test.html"), None);

        let dir = std::env::temp_dir().join(format!("hb-media-perm-{}", std::process::id()));
        let store = dir.join("media-permissions.json");
        let _ = std::fs::remove_dir_all(&dir);

        assert!(!decide_media_permission_with(
            false,
            "https://jcvolpe.me/",
            true,
            false,
            &store,
            |_, _| panic!("agent panes must never prompt")
        ));

        let mut prompts = 0;
        assert!(decide_media_permission_with(
            true,
            "https://jcvolpe.me/",
            true,
            false,
            &store,
            |origin, label| {
                prompts += 1;
                assert_eq!(origin, "https://jcvolpe.me");
                assert_eq!(label, "microphone");
                true
            }
        ));
        assert!(decide_media_permission_with(
            true,
            "https://jcvolpe.me/admin",
            true,
            false,
            &store,
            |_, _| panic!("persisted allow should not re-prompt")
        ));
        assert_eq!(prompts, 1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
