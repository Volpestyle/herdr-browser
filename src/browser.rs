//! A native browser hosted by the window, using existing mod-pane identities.
//! Website JavaScript never receives a bridge to Herdr's privileged mod runtime.
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_base::input::{Input, InputEvent, InputState};
use serde_json::{Value, json};
use std::cell::Cell;
use std::rc::Rc;

pub mod icons;
pub mod markdown;
pub use icons::{pane_icon, site_icon};
#[cfg(target_os = "macos")]
pub mod cdp;
#[cfg(target_os = "macos")]
mod control;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub mod runtime;

pub fn is_page_command(name: &str) -> bool {
    matches!(
        name,
        "browser.snapshot"
            | "browser.cdp"
            | "browser.read"
            | "browser.eval"
            | "browser.download"
            | "browser.click"
            | "browser.type"
            | "browser.signin"
            | "browser.press"
            | "browser.scroll"
            | "browser.wait"
            | "browser.screenshot"
            | "browser.record"
    )
}

pub const MOD: &str = "native-browser";

/// Whether followed links open here: settings.json `links: "system"` sends them to the default browser.
pub fn opens_links() -> bool {
    !std::fs::read_to_string(crate::settings::path())
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .is_some_and(|v| v["links"] == "system")
}

#[derive(Debug, Clone, Copy)]
pub enum BrowserEvent {
    MetaChanged,
}

#[derive(Debug)]
pub enum Event {
    Ready,
    Loading(String),
    Loaded(String),
    Title(String),
    Open(String),
    Command(&'static str),
    FocusUrl,
    Find,
    FindNext(bool),
    FindResult {
        count: usize,
        active: usize,
        final_update: bool,
    },
    Focused,
    Notice(String),
    Error(String),
    Auth(Auth),
    Closed,
}

pub fn url_label(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.is_empty() || trimmed == "about:blank" {
        return "Browser".into();
    }
    let Ok(parsed) = url::Url::parse(trimmed) else {
        return "Browser".into();
    };
    let Some(host) = parsed.host_str() else {
        return "Browser".into();
    };
    let host = host.strip_prefix("www.").unwrap_or(host);
    match parsed.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    }
}

fn same_host(a: &str, b: &str) -> bool {
    let host = |s: &str| {
        url::Url::parse(s)
            .ok()
            .and_then(|u| Some((u.host_str()?.to_owned(), u.port())))
    };
    matches!((host(a), host(b)), (Some(ha), Some(hb)) if ha == hb)
}

/// A page's HTTP sign-in challenge, answered from the pane. Dropping it cancels.
pub struct Auth {
    host: String,
    reply: Option<Box<dyn FnOnce(Option<(String, String)>)>>,
}
impl Drop for Auth {
    fn drop(&mut self) {
        if let Some(reply) = self.reply.take() {
            reply(None);
        }
    }
}
impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Auth({})", self.host)
    }
}

/// Normalize address-bar input, while keeping non-web schemes out of navigation.
pub fn address(input: &str) -> Result<String, String> {
    let text = input.trim();
    if text.is_empty() || text == "about:blank" {
        return Ok("about:blank".into());
    }
    let local =
        text.starts_with("localhost") || text.starts_with("127.") || text.starts_with("[::1]");
    let candidate = if local {
        format!("http://{text}")
    } else if text.contains("://")
        || text.starts_with("javascript:")
        || text.starts_with("data:")
        || text.starts_with("file:")
        || text.starts_with("about:")
    {
        text.into()
    } else if !text.chars().any(char::is_whitespace) && text.contains('.') {
        format!("https://{text}")
    } else {
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("q", text)
            .finish();
        format!("https://duckduckgo.com/?{query}")
    };
    let parsed = url::Url::parse(&candidate).map_err(|e| format!("Invalid address: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err("Only HTTP and HTTPS pages can be opened here.".into());
    }
    Ok(parsed.into())
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Favorite {
    title: String,
    url: String,
}

/// The favorites bar every browser pane shows, saved in favorites.json.
struct Favorites(Vec<Favorite>);
impl Global for Favorites {}

impl Favorites {
    fn path() -> std::path::PathBuf {
        crate::mods::app_dir().join("favorites.json")
    }
    fn load() -> Self {
        let path = Self::path();
        let Ok(bytes) = std::fs::read(&path) else { return Self(Vec::new()) };
        match serde_json::from_slice(&bytes) {
            Ok(list) => Self(list),
            Err(e) => {
                tracing::warn!("favorites.json unreadable ({e}); kept as favorites.json.bad");
                let _ = std::fs::rename(&path, path.with_extension("json.bad"));
                Self(Vec::new())
            }
        }
    }
    fn save(&self) {
        let path = Self::path();
        let temp = path.with_extension("json.tmp");
        let written = serde_json::to_vec_pretty(&self.0)
            .map_err(std::io::Error::other)
            .and_then(|bytes| std::fs::write(&temp, bytes))
            .and_then(|_| std::fs::rename(&temp, &path));
        if let Err(e) = written {
            tracing::warn!("Cannot save favorites.json: {e}");
        }
    }
}

pub struct Browser {
    pane: String,
    profile: String,
    input: Entity<InputState>,
    find_input: Entity<InputState>,
    find_open: bool,
    find_query: String,
    find_matches: Option<(usize, usize)>,
    find_final: bool,
    url: String,
    title: String,
    loading: bool,
    error: Option<String>,
    notice: Option<String>,
    auth: Option<Auth>,
    credentials: [Entity<InputState>; 2],
    allowed: Rc<Cell<bool>>,
    clear: Rc<Cell<Option<Bounds<Pixels>>>>,
    #[cfg(target_os = "macos")]
    host: Option<Rc<macos::Host>>,
    recording: Cell<bool>,
    _input: Subscription,
    _find_input: Subscription,
    _credentials: [Subscription; 2],
    _favorites: Subscription,
    _events: Task<()>,
}

impl EventEmitter<BrowserEvent> for Browser {}

impl Browser {
    pub fn new(pane: &str, props: &Value, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (url, initial_title) = match props.get("markdown_path").and_then(Value::as_str) {
            Some(path) => match markdown::open(path) {
                Ok(preview) => (preview.url, preview.title),
                Err(_) => (
                    props
                        .get("url")
                        .and_then(Value::as_str)
                        .and_then(|u| address(u).ok())
                        .unwrap_or_else(|| "about:blank".into()),
                    "Markdown".into(),
                ),
            },
            None => {
                let url = address(
                    props
                        .get("url")
                        .and_then(Value::as_str)
                        .unwrap_or("https://www.google.com"),
                )
                .unwrap_or_else(|_| "about:blank".into());
                let label = url_label(&url);
                (url, label)
            }
        };
        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search or enter an address"));
        input.update(cx, |i, cx| i.set_value(url.clone(), window, cx));
        let subscription = cx.subscribe_in(&input, window, |this, _, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                let text = this.input.read(cx).value().to_string();
                let _ = this.navigate(&text, window, cx);
                this.focus_page();
            }
        });
        let find_input = cx.new(|cx| InputState::new(window, cx).placeholder("Find in page"));
        let find_subscription =
            cx.subscribe_in(&find_input, window, |this, input, event, _window, cx| match event {
                InputEvent::Change => {
                    let next = input.read(cx).value().to_string();
                    if this.find_query != next {
                        this.find_query = next;
                        this.find_matches = None;
                        this.find_final = false;
                        this.run_find(true, false);
                        cx.notify();
                    }
                }
                InputEvent::PressEnter { shift, .. } => {
                    this.find_step(!shift, cx);
                }
                _ => {}
            });
        let credentials = [
            cx.new(|cx| InputState::new(window, cx).placeholder("Username")),
            cx.new(|cx| InputState::new(window, cx).placeholder("Password").masked(true)),
        ];
        let submit = credentials.each_ref().map(|input| {
            cx.subscribe_in(input, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.sign_in(true, window, cx);
                }
            })
        });
        if !cx.has_global::<Favorites>() {
            cx.set_global(Favorites::load());
        }
        let favorites = cx.observe_global::<Favorites>(|_, cx| cx.notify());
        let profile = props
            .get("profile")
            .and_then(Value::as_str)
            .unwrap_or("person")
            .to_string();
        let (tx, rx) = async_channel::unbounded();
        let allowed = Rc::new(Cell::new(false));
        #[cfg(target_os = "macos")]
        let (host, error) =
            match macos::Host::new(&url, &profile, allowed.clone(), tx.clone(), window) {
                Ok(h) => (Some(Rc::new(h)), None),
                Err(e) => (None, Some(format!("Cannot start Chromium: {e:#}"))),
            };
        #[cfg(not(target_os = "macos"))]
        let error = Some("Native browser panes currently require macOS.".into());
        let events = cx.spawn_in(window, async move |this, cx| {
            while let Ok(event) = rx.recv().await {
                if this
                    .update_in(cx, |this, window, cx| this.event(event, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            pane: pane.into(),
            profile,
            input,
            find_input,
            find_open: false,
            find_query: String::new(),
            find_matches: None,
            find_final: false,
            url,
            title: initial_title,
            loading: true,
            error,
            notice: None,
            auth: None,
            credentials,
            recording: Cell::new(false),
            allowed,
            clear: Rc::default(),
            #[cfg(target_os = "macos")]
            host,
            _input: subscription,
            _find_input: find_subscription,
            _credentials: submit,
            _favorites: favorites,
            _events: events,
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn display_title(&self) -> String {
        let title = self.title.trim();
        if !title.is_empty()
            && title != "Browser"
            && title != "about:blank"
            && !title.starts_with("http://")
            && !title.starts_with("https://")
        {
            return title.to_owned();
        }
        url_label(&self.url)
    }

    pub fn owns_focus(&self, window: &Window, cx: &App) -> bool {
        self.is_focused()
            || self.input.read(cx).focus_handle(cx).is_focused(window)
            || self.find_input.read(cx).focus_handle(cx).is_focused(window)
            || self
                .credentials
                .iter()
                .any(|c| c.read(cx).focus_handle(cx).is_focused(window))
    }

    fn run_find(&mut self, forward: bool, find_next: bool) {
        #[cfg(target_os = "macos")]
        if let Some(host) = &self.host {
            if self.find_query.is_empty() {
                host.stop_finding(true);
            } else {
                host.find(&self.find_query, forward, find_next);
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (forward, find_next);
    }

    pub fn open_find(&mut self, query: Option<&str>, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        if let Some(host) = &self.host {
            host.focus_parent();
            host.set_find_open(true);
        }
        let reopening = !self.find_open;
        self.find_open = true;
        if let Some(query) = query {
            self.find_query = query.to_owned();
            self.find_matches = None;
            self.find_final = false;
            self.find_input
                .update(cx, |i, cx| i.set_value(query.to_owned(), window, cx));
            self.run_find(true, false);
        } else if reopening && !self.find_query.is_empty() {
            self.find_matches = None;
            self.find_final = false;
            self.run_find(true, false);
        }
        self.find_input.update(cx, |i, cx| {
            i.focus(window, cx);
            i.select_all(window, cx);
        });
        cx.notify();
    }

    pub fn close_find(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.find_open {
            return;
        }
        self.find_open = false;
        self.find_matches = None;
        self.find_final = false;
        #[cfg(target_os = "macos")]
        if let Some(host) = &self.host {
            host.set_find_open(false);
            host.stop_finding(true);
        }
        self.focus_page();
        cx.notify();
    }

    fn find_step(&mut self, forward: bool, cx: &mut Context<Self>) {
        if !self.find_open || self.find_query.is_empty() {
            return;
        }
        self.run_find(forward, true);
        cx.notify();
    }

    fn event(&mut self, event: Event, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            Event::Ready => {}
            Event::Loading(url) => {
                self.loading = true;
                self.auth = None;
                let url_changed = self.url != url;
                if !same_host(&self.url, &url) {
                    self.title = url_label(&url);
                }
                self.url = url;
                if self.find_open {
                    self.find_matches = None;
                    self.find_final = false;
                }
                if !self.input.read(cx).focus_handle(cx).is_focused(window) {
                    self.input
                        .update(cx, |i, cx| i.set_value(self.url.clone(), window, cx));
                }
                if url_changed {
                    cx.emit(BrowserEvent::MetaChanged);
                }
            }
            Event::Loaded(url) => {
                self.loading = false;
                let url_changed = self.url != url;
                self.url = url;
                if !self.input.read(cx).focus_handle(cx).is_focused(window) {
                    self.input
                        .update(cx, |i, cx| i.set_value(self.url.clone(), window, cx));
                }
                if self.find_open && !self.find_query.is_empty() {
                    self.run_find(true, false);
                }
                if url_changed {
                    cx.emit(BrowserEvent::MetaChanged);
                }
            }
            Event::Title(title) => {
                if self.title != title {
                    self.title = title;
                    cx.emit(BrowserEvent::MetaChanged);
                }
            }
            Event::Open(url) => {
                crate::mods::request(crate::mods::Request::Run {
                    name: "browser.open".into(),
                    args: json!({"url":url,"profile":self.profile}),
                });
            }
            Event::Command(command) => {
                if matches!(
                    command,
                    "browser.back" | "browser.forward" | "browser.reload" | "browser.devtools" | "browser.find"
                ) {
                    let _ = self.command(command, &Value::Null, window, cx);
                } else if let Some(by) = match command {
                    "tab.move:prev" => Some(-1),
                    "tab.move:next" => Some(1),
                    _ => None,
                } {
                    crate::mods::request(crate::mods::Request::Run {
                        name: "tab.move".into(),
                        args: json!({ "by": by }),
                    });
                } else {
                    crate::mods::request(crate::mods::Request::Run {
                        name: command.into(),
                        args: Value::Null,
                    });
                }
            }
            Event::FocusUrl => self.focus_url(window, cx),
            Event::Find => self.open_find(None, window, cx),
            Event::FindNext(forward) => self.find_step(forward, cx),
            Event::FindResult {
                count,
                active,
                final_update,
            } => {
                if self.find_open && !self.find_query.is_empty() {
                    self.find_matches = Some((active, count));
                    self.find_final = final_update;
                }
            }
            Event::Focused => {
                crate::mods::request(crate::mods::Request::Run {
                    name: "pane.focus".into(),
                    args: json!({"pane":self.pane}),
                });
            }
            Event::Closed => {
                crate::mods::request(crate::mods::Request::Run {
                    name: "pane.close".into(),
                    args: json!({"pane":self.pane}),
                });
            }
            Event::Notice(message) => {
                self.loading = false;
                self.notice = Some(message);
            }
            Event::Error(message) => {
                self.loading = false;
                self.error = Some(message);
            }
            Event::Auth(auth) => {
                self.loading = false;
                self.error = None;
                self.auth = Some(auth);
                if self.is_focused() {
                    self.release_focus();
                    self.credentials[0].update(cx, |i, cx| i.focus(window, cx));
                }
            }
        }
        cx.notify();
    }

    fn answer(&mut self, login: Option<(String, String)>) {
        if let Some(reply) = self.auth.take().and_then(|mut auth| auth.reply.take()) {
            reply(login);
        }
    }

    #[cfg(target_os = "macos")]
    fn http_sign_in(&mut self, login: control::Login, cx: &mut Context<Self>) -> Result<Value, String> {
        let host = self
            .auth
            .as_ref()
            .map(|a| a.host.to_ascii_lowercase())
            .ok_or("The page no longer asks for a sign-in")?;
        if !control::listed(&login.urls, &host) {
            return Err(format!("The credentials do not list {host}; refusing to sign in"));
        }
        self.answer(Some((login.username, login.password)));
        cx.notify();
        Ok(json!({"ok":true,"filled":["username","password"],"auth":host}))
    }

    fn sign_in(&mut self, submit: bool, window: &mut Window, cx: &mut Context<Self>) {
        let [user, password] = self.credentials.each_ref().map(|i| i.read(cx).value().to_string());
        self.answer(submit.then_some((user, password)));
        for input in &self.credentials {
            input.update(cx, |i, cx| i.set_value("", window, cx));
        }
        self.focus_page();
        cx.notify();
    }

    pub fn set_allowed(&self, allowed: bool) {
        self.allowed.set(allowed);
        #[cfg(target_os = "macos")]
        if let Some(host) = &self.host {
            host.set_allowed(allowed);
        }
    }

    /// Window area the page must leave uncovered (toasts): it draws above GPUI.
    pub fn set_clear(&self, area: Option<Bounds<Pixels>>) {
        self.clear.set(area);
    }

    pub fn is_focused(&self) -> bool {
        #[cfg(target_os = "macos")]
        if let Some(host) = &self.host {
            return host.is_focused();
        }
        false
    }

    pub fn focus_page(&self) {
        #[cfg(target_os = "macos")]
        if let Some(host) = &self.host {
            host.focus();
        }
    }

    /// Hand AppKit's keyboard back to the window if this page holds it, so a
    /// GPUI pane focused elsewhere actually receives typing.
    pub fn release_focus(&self) {
        #[cfg(target_os = "macos")]
        if let Some(host) = &self.host {
            if host.is_focused() {
                host.focus_parent();
            }
        }
    }

    fn toggle_favorite(&mut self, cx: &mut Context<Self>) {
        let url = self.url.clone();
        let title = if self.title.trim().is_empty() { url.clone() } else { self.title.clone() };
        cx.update_global::<Favorites, _>(|favorites, _| {
            match favorites.0.iter().position(|f| f.url == url) {
                Some(i) => {
                    favorites.0.remove(i);
                }
                None => favorites.0.push(Favorite { title, url }),
            }
            favorites.save();
        });
    }

    fn focus_url(&self, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        if let Some(host) = &self.host {
            host.focus_parent();
        }
        self.input.update(cx, |i, cx| {
            i.focus(window, cx);
            i.select_all(window, cx);
        });
    }

    fn navigate(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let url = match address(text) {
            Ok(url) => url,
            Err(error) => {
                self.error = Some(error.clone());
                cx.notify();
                return Err(error);
            }
        };
        #[cfg(target_os = "macos")]
        if let Some(host) = &self.host {
            host.navigate(&url).map_err(|e| e.to_string())?;
        }
        if !same_host(&self.url, &url) {
            self.title = url_label(&url);
        }
        self.url = url.clone();
        self.loading = true;
        self.error = None;
        self.notice = None;
        self.auth = None;
        if self.find_open {
            self.find_matches = None;
            self.find_final = false;
        }
        self.input
            .update(cx, |i, cx| i.set_value(url.clone(), window, cx));
        cx.emit(BrowserEvent::MetaChanged);
        cx.notify();
        Ok(json!({"url":url}))
    }

    pub fn inspect(&self) -> Value {
        let mut out = json!({"engine":"Chromium/CEF", "pane":self.pane, "profile":self.profile, "url":self.url, "title":self.title, "loading":self.loading, "page_error":self.error, "notice":self.notice, "auth":self.auth.as_ref().map(|a| &a.host), "visible":self.allowed.get(), "find_open":self.find_open, "find_query":self.find_query, "find_matches":self.find_matches.map(|(a, t)| json!([a, t]))});
        #[cfg(target_os = "macos")]
        if let Some(host) = &self.host {
            out["native"] = host.inspect();
        }
        out
    }

    /// How `herdr-app ls` lists this pane. A person pane's page stays out of the listing:
    /// reading it is `browser.inspect` with `confirmed`, like every other read.
    pub fn listing(&self) -> Value {
        if self.profile == "agent" {
            json!({"pane":self.pane, "profile":self.profile, "url":self.url, "title":self.title})
        } else {
            json!({"pane":self.pane, "profile":self.profile})
        }
    }

    pub fn check_access(&self, args: &Value) -> Result<(), String> {
        self.check_command_access("", args)
    }

    fn check_command_access(&self, name: &str, args: &Value) -> Result<(), String> {
        if name == "browser.cdp" && self.profile != "agent" {
            return Err("Playwright access is limited to agent-profile panes; open one with profile: \"agent\"".into());
        }
        if self.profile != "agent" && args["confirmed"] != true {
            return Err("This is a person-profile pane; owner authorization and confirmed: true are required".into());
        }
        Ok(())
    }

    /// Control replies wait asynchronously on CEF while the window keeps serving events.
    pub fn page_command(
        &self,
        name: &str,
        args: &Value,
        reply: Option<std::sync::mpsc::Sender<Value>>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        self.check_command_access(name, args)?;
        #[cfg(target_os = "macos")]
        {
            let host = Rc::downgrade(self.host.as_ref().ok_or("Chromium is unavailable")?);
            let name = name.to_string();
            let mut args = args.clone();
            let pane = self.pane.clone();
            if name == "browser.cdp" {
                // New Playwright pages open beside the pane that minted the endpoint.
                args["pane"] = pane.clone().into();
            }
            cx.spawn(async move |this, cx| {
                let result = async {
                    if name == "browser.signin"
                        && this.update(cx, |b, _| b.auth.is_some()).unwrap_or(false)
                    {
                        let login = control::login(&args, cx.background_executor()).await?;
                        return this
                            .update(cx, |b, cx| b.http_sign_in(login, cx))
                            .map_err(|_| "Browser pane was closed".to_string())?;
                    }
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
                    let control = loop {
                        let page = host.upgrade().ok_or("Browser pane was closed")?;
                        if page.starting() {
                            if std::time::Instant::now() >= deadline {
                                return Err("Browser is still starting; try again".into());
                            }
                            drop(page);
                            cx.background_executor()
                                .timer(std::time::Duration::from_millis(25))
                                .await;
                        } else {
                            break page.control()?;
                        }
                    };
                    control.run(&name, &args, cx.background_executor()).await
                }
                .await;
                if name == "browser.record" {
                    let recording = match &result {
                        Ok(value) => value["recording"] == true,
                        Err(_) => args["action"] != "start",
                    };
                    let _ = this.update(cx, |browser, cx| {
                        browser.recording.set(recording);
                        cx.notify();
                    });
                }
                let value = match result {
                    Ok(mut value) => {
                        value["pane"] = pane.into();
                        value
                    }
                    Err(error) => json!({"error":error}),
                };
                if let Some(reply) = reply {
                    let _ = reply.send(value);
                } else if let Some(error) = value.get("error") {
                    tracing::warn!("browser command: {error}");
                }
            })
            .detach();
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (name, reply, cx);
            Err("Native browsers require macOS".into())
        }
    }

    pub fn command(
        &mut self,
        command: &str,
        args: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        if is_page_command(command) {
            self.page_command(command, args, None, cx)?;
            return Ok(json!({"pending":true}));
        }
        match command {
            "browser.navigate" => {
                return self.navigate(
                    args.get("url")
                        .and_then(Value::as_str)
                        .ok_or("url is required")?,
                    window,
                    cx,
                );
            }
            "browser.focus_url" => self.focus_url(window, cx),
            "browser.find" => self.open_find(args.get("query").and_then(Value::as_str), window, cx),
            "browser.inspect" => return Ok(self.inspect()),
            #[cfg(target_os = "macos")]
            _ => {
                if let Some(host) = &self.host {
                    host.command(command)?;
                }
            }
            #[cfg(not(target_os = "macos"))]
            _ => return Err("Native browsers require macOS".into()),
        }
        cx.notify();
        Ok(json!({"ok":true}))
    }
}

impl Render for Browser {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let color = crate::settings::color;
        let favorites = cx.global::<Favorites>().0.clone();
        let bar = (!favorites.is_empty()).then(|| {
            div()
                .flex_none()
                .h(px(30.))
                .w_full()
                .flex()
                .items_center()
                .gap(px(2.))
                .px(px(8.))
                .overflow_hidden()
                .bg(color("popover"))
                .border_b_1()
                .border_color(color("border"))
                .text_size(px(12.))
                .children(favorites.iter().enumerate().map(|(i, f)| {
                    let url = f.url.clone();
                    let icon = site_icon(&f.url);
                    let icon_el: AnyElement = if icon.tint {
                        svg()
                            .path(icon.path)
                            .size(px(12.))
                            .flex_none()
                            .text_color(
                                icon.color
                                    .and_then(crate::settings::parse_hex)
                                    .unwrap_or_else(|| color("muted")),
                            )
                            .into_any_element()
                    } else {
                        img(crate::mods::app_dir().join(icon.path))
                            .w(px(12.))
                            .h(px(12.))
                            .flex_none()
                            .into_any_element()
                    };
                    div()
                        .id(("browser-favorite", i))
                        .flex_none()
                        .max_w(px(180.))
                        .h(px(24.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .rounded_md()
                        .cursor_pointer()
                        .hover(|s| s.bg(color("hover")))
                        .on_click(cx.listener(move |this, _, w, cx| {
                            if this.navigate(&url, w, cx).is_ok() {
                                this.focus_page();
                            }
                        }))
                        .child(icon_el)
                        .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(f.title.clone()))
                }))
        });
        let button = |id: &'static str, label: &'static str, command: &'static str| {
            div()
                .id(id)
                .w(px(30.))
                .h(px(30.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .cursor_pointer()
                .text_color(color("foreground"))
                .hover(|s| s.bg(color("hover")))
                .on_click(cx.listener(move |this, _, w, cx| {
                    let _ = this.command(command, &Value::Null, w, cx);
                }))
                .child(label)
        };
        let toolbar =
            div()
                .flex_none()
                .h(px(44.))
                .w_full()
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(8.))
                .bg(color("popover"))
                .border_b_1()
                .border_color(color("border"))
                .child(button("browser-back", "‹", "browser.back"))
                .child(button("browser-forward", "›", "browser.forward"))
                .child(button("browser-reload", "↻", "browser.reload"))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h(px(30.))
                        .flex()
                        .items_center()
                        .px(px(10.))
                        .rounded_md()
                        .bg(color("surface"))
                        .border_1()
                        .border_color(color("border"))
                        .text_size(px(13.))
                        .child(Input::new(&self.input)),
                )
                .child({
                    let starred = favorites.iter().any(|f| f.url == self.url);
                    div()
                        .id("browser-favorite")
                        .w(px(30.))
                        .h(px(30.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .cursor_pointer()
                        .text_color(if starred { color("foreground") } else { color("muted") })
                        .hover(|s| s.bg(color("hover")))
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_favorite(cx)))
                        .child(if starred { "★" } else { "☆" })
                })
                .child({
                    let recording = self.recording.get();
                    div()
                        .id("browser-record")
                        .h(px(30.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .rounded_md()
                        .cursor_pointer()
                        .text_size(px(12.))
                        .text_color(if recording { color("blocked") } else { color("muted") })
                        .hover(|s| s.bg(color("hover")))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let action = if this.recording.get() { "stop" } else { "start" };
                            this.recording.set(action == "start");
                            if let Err(error) = this.page_command("browser.record", &json!({"action":action,"confirmed":true}), None, cx) {
                                this.recording.set(false);
                                this.error = Some(error);
                            }
                            cx.notify();
                        }))
                        .child(div().size(px(9.)).rounded(px(if recording { 2. } else { 5. })).bg(color("blocked")))
                        .child(if recording { "Stop" } else { "Rec" })
                })
                .child(div().text_size(px(11.)).text_color(color("muted")).child(
                    if self.loading {
                        "Loading…"
                    } else {
                        "Chromium"
                    },
                ));
        #[cfg(target_os = "macos")]
        let host = self.host.clone();
        let allowed = self.allowed.clone();
        let clear = self.clear.clone();
        let body = div()
            .flex_1()
            .min_h_0()
            .w_full()
            .relative()
            .overflow_hidden()
            .bg(color("background"))
            .child(
                canvas(
                    move |bounds, _, _| {
                        #[cfg(target_os = "macos")]
                        if let Some(host) = &host {
                            host.place(bounds, allowed.get(), clear.get());
                        }
                    },
                    |_, _, _, _| {},
                )
                .size_full(),
            );
        let find_bar = self.find_open.then(|| {
            let find_label = if self.find_query.is_empty() {
                String::new()
            } else if let Some((active, total)) = self.find_matches {
                if total == 0 {
                    if self.find_final {
                        "No matches".to_owned()
                    } else {
                        "Searching…".to_owned()
                    }
                } else if active == 0 {
                    format!("{total}")
                } else {
                    format!("{active} / {total}")
                }
            } else {
                "Searching…".to_owned()
            };
            let nav_button = |id: &'static str, glyph: &'static str, forward: bool| {
                div()
                    .id(id)
                    .w(px(26.))
                    .h(px(26.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_color(color("foreground"))
                    .hover(|s| s.bg(color("hover")))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.find_step(forward, cx);
                        this.find_input.update(cx, |i, cx| i.focus(window, cx));
                    }))
                    .child(glyph)
            };
            div()
                .id("browser-find-bar")
                .flex_none()
                .h(px(36.))
                .w_full()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(4.))
                .px(px(8.))
                .bg(color("popover"))
                .border_b_1()
                .border_color(color("border"))
                .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                    if ev.keystroke.key == "escape" {
                        this.close_find(window, cx);
                        cx.stop_propagation();
                    } else if ev.keystroke.key == "g"
                        && (ev.keystroke.modifiers.platform || ev.keystroke.modifiers.control)
                        && !ev.keystroke.modifiers.alt
                    {
                        this.find_step(!ev.keystroke.modifiers.shift, cx);
                        cx.stop_propagation();
                    } else if ev.keystroke.key == "f"
                        && ev.keystroke.modifiers.platform
                        && !ev.keystroke.modifiers.control
                        && !ev.keystroke.modifiers.alt
                        && !ev.keystroke.modifiers.shift
                    {
                        this.find_input.update(cx, |i, cx| i.select_all(window, cx));
                        cx.stop_propagation();
                    }
                }))
                .child(
                    div()
                        .flex_1()
                        .max_w(px(320.))
                        .h(px(26.))
                        .flex()
                        .items_center()
                        .px(px(8.))
                        .rounded_md()
                        .bg(color("surface"))
                        .border_1()
                        .border_color(color("border"))
                        .text_size(px(12.5))
                        .child(Input::new(&self.find_input)),
                )
                .child(
                    div()
                        .w(px(64.))
                        .text_xs()
                        .text_color(color("muted"))
                        .child(find_label),
                )
                .child(nav_button("browser-find-prev", "↑", false))
                .child(nav_button("browser-find-next", "↓", true))
                .child(
                    div()
                        .id("browser-find-close")
                        .w(px(26.))
                        .h(px(26.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_sm()
                        .cursor_pointer()
                        .text_color(color("foreground"))
                        .hover(|s| s.bg(color("hover")))
                        .on_click(cx.listener(|this, _, window, cx| this.close_find(window, cx)))
                        .child("×"),
                )
        });
        div()
            .size_full()
            .flex()
            .flex_col()
            .text_color(color("foreground"))
            .child(toolbar)
            .when_some(bar, |el, bar| el.child(bar))
            .when_some(find_bar, |el, find_bar| el.child(find_bar))
            .when_some(self.notice.clone(), |el, message| {
                el.child(
                    div()
                        .px(px(10.))
                        .py(px(4.))
                        .text_size(px(12.))
                        .child(message),
                )
            })
            .when_some(self.auth.as_ref().map(|a| a.host.clone()), |el, host| {
                let field = |input: &Entity<InputState>| {
                    div()
                        .w(px(180.))
                        .h(px(28.))
                        .flex()
                        .items_center()
                        .px(px(8.))
                        .rounded_md()
                        .bg(color("surface"))
                        .border_1()
                        .border_color(color("border"))
                        .child(Input::new(input))
                };
                let answer = |id: &'static str, label: &'static str, submit: bool| {
                    div()
                        .id(id)
                        .h(px(28.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .rounded_md()
                        .cursor_pointer()
                        .hover(|s| s.bg(color("hover")))
                        .on_click(cx.listener(move |this, _, w, cx| this.sign_in(submit, w, cx)))
                        .child(label)
                };
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(10.))
                        .py(px(6.))
                        .bg(color("popover"))
                        .border_b_1()
                        .border_color(color("border"))
                        .text_size(px(12.))
                        .child(format!("Sign in to {host}"))
                        .child(field(&self.credentials[0]))
                        .child(field(&self.credentials[1]))
                        .child(answer("browser-auth-submit", "Sign in", true))
                        .child(answer("browser-auth-cancel", "Cancel", false)),
                )
            })
            .when_some(self.error.clone(), |el, error| {
                el.child(
                    div()
                        .p(px(10.))
                        .bg(color("popover"))
                        .text_color(color("blocked"))
                        .text_size(px(12.))
                        .child(error),
                )
            })
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::address;

    #[cfg(target_os = "macos")]
    #[gpui::test]
    fn selected_address_copies_with_the_input_shortcut(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_base::init);
        let url = "https://example.com/a/path?q=copy&mode=browser";
        let (browser, cx) = cx.add_window_view(|window, cx| {
            super::Browser::new("browser-test", &serde_json::json!({"url": url}), window, cx)
        });
        browser.update_in(cx, |browser, window, cx| browser.focus_url(window, cx));
        cx.run_until_parked();
        cx.simulate_keystrokes("cmd-c");
        assert_eq!(
            cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()))
                .as_deref(),
            Some(url)
        );
    }

    #[gpui::test]
    fn typed_sign_in_answers_the_challenge_and_leaving_cancels_it(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_base::init);
        let (browser, cx) = cx.add_window_view(|window, cx| {
            super::Browser::new("browser-test", &serde_json::json!({"url": "https://example.com"}), window, cx)
        });
        let answers = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let challenge = || {
            let answers = answers.clone();
            super::Event::Auth(super::Auth {
                host: "example.com".into(),
                reply: Some(Box::new(move |login| answers.borrow_mut().push(login))),
            })
        };
        browser.update_in(cx, |browser, window, cx| {
            browser.event(challenge(), window, cx);
            browser.credentials[0].update(cx, |i, cx| i.set_value("me", window, cx));
            browser.credentials[1].update(cx, |i, cx| i.set_value("secret", window, cx));
            browser.sign_in(true, window, cx);
            browser.event(challenge(), window, cx);
            browser.event(super::Event::Loading("https://example.com/next".into()), window, cx);
        });
        assert_eq!(*answers.borrow(), [Some(("me".to_string(), "secret".to_string())), None]);
    }

    #[test]
    fn addresses_and_searches() {
        assert_eq!(
            address("localhost:3000/test").unwrap(),
            "http://localhost:3000/test"
        );
        assert_eq!(address("example.com").unwrap(), "https://example.com/");
        assert!(
            address("some search terms")
                .unwrap()
                .starts_with("https://duckduckgo.com/?q=")
        );
        assert_eq!(address("").unwrap(), "about:blank");
    }
    #[test]
    fn url_labels_and_display_titles_follow_page_state() {
        assert_eq!(super::url_label("https://www.google.com/search?q=hi"), "google.com");
        assert_eq!(super::url_label("https://linear.app/vibes/issue/ORC-1"), "linear.app");
        assert_eq!(super::url_label("http://localhost:3000/dashboard"), "localhost:3000");
        assert_eq!(super::url_label("http://127.0.0.1:8080/"), "127.0.0.1:8080");
        assert_eq!(super::url_label("about:blank"), "Browser");
        assert_eq!(super::url_label(""), "Browser");
    }

    #[gpui::test]
    fn find_bar_opens_tracks_results_and_closes_on_escape(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_base::init);
        let (browser, cx) = cx.add_window_view(|window, cx| {
            super::Browser::new("browser-test", &serde_json::json!({"url": "https://example.com/docs"}), window, cx)
        });
        browser.read_with(cx, |browser, _| {
            assert_eq!(browser.display_title(), "example.com");
            assert!(!browser.find_open);
        });
        browser.update_in(cx, |browser, window, cx| {
            browser.event(super::Event::Title("Example Docs".into()), window, cx);
            assert_eq!(browser.display_title(), "Example Docs");
            browser.open_find(Some("needle"), window, cx);
            browser.event(
                super::Event::FindResult {
                    count: 3,
                    active: 1,
                    final_update: true,
                },
                window,
                cx,
            );
        });
        cx.run_until_parked();
        browser.read_with(cx, |browser, _| {
            assert!(browser.find_open);
            assert_eq!(browser.find_query, "needle");
            assert_eq!(browser.find_matches, Some((1, 3)));
        });
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        browser.read_with(cx, |browser, _| {
            assert!(!browser.find_open);
            assert_eq!(browser.find_matches, None);
        });
    }

    #[test]
    fn does_not_turn_privileged_schemes_into_page_navigation() {
        for input in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "data:text/html,hi",
            "about:config",
        ] {
            assert!(address(input).is_err(), "{input}");
        }
    }
}
