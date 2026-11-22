# Related browser work and sharing

Public-source check: 2026-10-30. GitHub issues/PRs for `herdrdev/herdr-app`
were searched for browser work, alongside related repositories and the public
Herdr community guide. Private Discord history was not searched.

- [penso/herdr-gpui](https://github.com/penso/herdr-gpui/blob/main/crates/herdr-gpui/README.md#browser-tabs)
  is a separate native Herdr client with embedded browser tabs. Its documented
  engines are WebKit on macOS and WebView2 on Windows. This is the closest
  native precedent found; browser tabs in Herdr are not a new idea.
- [ogulcancelik/herdr-browser](https://github.com/ogulcancelik/herdr-browser)
  is the maintainer's deprecated experiment: Chromium frames travel over CDP,
  PNG encoding and Herdr pane graphics. Its README redirects users to
  terminal-browser.
- [StructuPath/herdr-browser](https://github.com/StructuPath/herdr-browser)
  is a terminal plugin using streamed browser frames with screenshot/polling
  fallbacks and CDP/agent-browser integration.
- Official app issues [#22](https://github.com/herdrdev/herdr-app/issues/22),
  [#23](https://github.com/herdrdev/herdr-app/issues/23), and
  [#24](https://github.com/herdrdev/herdr-app/issues/24) document browser-preview
  image, pane-size and cleanup work. #23/#24 were closed after mod API additions.
  The search found no PR for the same native CEF integration into this app;
  absence from these search results is not proof that no one else has one.

This fork embeds native Chromium/CEF child views in the upstream desktop app's
Rust/GPUI code, with a small menu mod. It is currently tested on macOS on Apple
silicon. Manual testing confirmed Google sign-in to Linear after the
privacy-declaration packaging fix. Comparative
performance, other platforms and complete browser feature coverage are untested.

The project's [contribution guide](../CONTRIBUTING.md) recommends
[Discord](https://discord.gg/PsgRfzmUDg) for showing what people built, and an
issue discussion before a substantial core PR. Use the current invite from
the upstream guide if this copied invite changes. Lead with a short demo and
the fork link; ask the maintainer about a supported native browser host API
before preparing an upstream PR. A native browser cannot be distributed as a
normal JavaScript-only mod with the current API.

## Draft showcase message (not posted)

I built an Apple Silicon macOS fork of `herdr-app` with native Chromium (CEF)
browser panes right beside the terminals and coding agents, plus in-process
agent browser controls over the existing Herdr control socket.

What's working in it today:
- Native CEF child views embedded directly in GPUI (normal page rendering,
  Retina scaling, extensions, persistent storage, tab/address shortcuts,
  `browser.markdown` live Markdown + Mermaid previews).
- In-process agent browser commands (`browser.snapshot`, `browser.read`,
  `browser.click`, `browser.type`, `browser.press`, `browser.scroll`,
  `browser.wait`, `browser.screenshot`, `browser.record`, `browser.signin`)
  using CEF's in-process DevTools API — no open localhost debug port, and
  background-safe (clicks/keys never steal focus or move the pointer).
- A separate `agent` Chromium profile (`browser.open --profile agent`) isolated
  from the user's `person` profile, with `person` panes refused unless
  explicitly confirmed and password fields refused in `browser.type`.
- A guarded `browser.cdp` pipe scoped to agent-profile pages for Playwright.
- Stock `herdr-core` (`hc`) untouched, separate `~/.config/herdr-app-browser`
  config dir, and a stock-CEF macOS release pipeline.

Source: https://github.com/Volpestyle/herdr-browser

I saw the earlier `herdr-browser` experiments and `penso/herdr-gpui`. Keeping
this on top of `herdr-app` with in-process commands on the existing control
socket has made side-by-side coding-agent + browser workflows noticeably smooth.
Would a small native browser host API (and/or these pane-scoped `browser.*`
command semantics) be useful upstream? Happy to demo or compare notes.
