# herdr-app: drive the person's window

Herdr shows spaces, tabs and panes. herdr-core (`hc`) runs their terminals
and keeps them alive when the window closes. Use `herdr-app` only to change
what the person sees. For starting, reading, messaging, waiting on or cleaning
up agents and programs, read `hc --skill` and use hc.
Your terminal address is `$HC_TERMINAL_ID`; it is not a pane ID.

## Inspect before acting

```sh
herdr-app ls          # spaces > tabs > terminals and browsers, including pane IDs
herdr-app status      # current layout, focus, overlays, zoom and file/mod errors
herdr-app commands    # available commands, arguments and descriptions
```

`ls` and `status` talk to the running window, not a new app. `commands`
prints the built-in command list even without an open window. Do not launch bare
`herdr-app` to inspect it: that opens or raises the window. Do not stop,
signal or restart the person's app or core. A window closing does not kill
its terminals.

`app.check_updates` checks for newer releases. Settings reports "Up to date"
only after a successful check; on failure, check the connection and retry.
Package-managed cores are checked but updated through their package manager.
Dismiss a core update error with its × or `overlay.close {id:"core-update-error"}`;
it stays dismissed until the error changes or the update recovers.

## Show something when asked

New spaces and tabs are shown immediately: only create them when asked.
To start one the person will look at later without moving them there, pass
`--focus false`:

```sh
herdr-app do space.new --name eng-123 --folder ~/code/app/eng-123 --argv '["claude"]' --task "Implement ENG-123" --focus false
```

Pick an explicit, useful name for every new space and tab; don't rely on
default names. Use IDs from `ls` rather than guessing or acting on the wrong
focused pane.

```sh
herdr-app do space.new --name api --folder /path/to/api
herdr-app do tab.new --name "fix login" --argv '["claude"]' --task "Look at the login bug"
herdr-app do pane.split --direction right --pane p12
herdr-app do agent.focus --terminal t7
```

For a remote space:

```sh
herdr-app do space.new --name "remote api" --machine vps --folder /srv/api
```

The folder is on that machine; tabs and splits in the space stay there.
New-space keys and clicks follow the focused space's machine. Script commands
without `--machine` still use this computer; `--machine ''` explicitly chooses it.
The agent menu checks and starts agents in the remote Unix user's
login/interactive shell (sh, bash, zsh, dash, ksh or fish), including its
configured PATH. Other shells keep the sh launcher.
`terminal.open --folder /path --place down` starts the split in that folder,
including when `--argv` supplies a program. Without `--folder`, it follows the
focused terminal's folder. If an inherited local folder was deleted, new spaces,
tabs and splits retry in that machine's home. An explicit invalid folder
still fails.
`hc machine ls` lists machines. Use `hc run --on vps ...`, `hc exec --on vps ...`
and `hc file ... --on vps ...` for remote work, not your local file tools.

`do` accepts `--key value` arguments (JSON for arrays/objects) or one JSON
object. Unknown argument names fail and list valid names. Target IDs (`id`,
`pane`, `space`, `target`, `terminal`) must be strings; other non-null types fail before
the current-target default can apply. Null keeps its omitted-argument meaning.
Use `commands` to confirm the command's exact shape:

```sh
herdr-app do pane.split '{"direction":"right","pane":"p12"}'
```

`pane.swap {pane?, direction | target}` trades the places of `pane` (default:
the focused one) and its neighbor in a direction, or `target`, another pane of
the same tab. Give `direction` or `target`, not both. Focus stays with the same pane:

```sh
herdr-app do pane.swap '{"pane":"p12","target":"p14"}'
```

`terminal.select_all {terminal?}` selects a terminal's whole screen and scrollback,
ready for `terminal.copy`. Without a target it follows keyboard focus. In vim and
other alternate-screen apps, it selects only that screen. Cmd+A runs it on macOS;
Linux and Windows have no default, so Ctrl+A stays with the shell.

`terminal.select {terminal?, at?: {row, col} | "cursor", mode?: "cell" | "word" | "line"}`
replaces the selection, at the program cursor by default. Cells are zero-based; rows
count from the oldest retained history through the active screen. Word and line
match double/triple click. `terminal.adjust_selection {terminal?, direction}` moves
the inclusive moving end and follows it in the view: left, right, up, down, page_up,
page_down, home, end, line_start, line_end. It fails without a selection.
`terminal.clear_selection {terminal?}` clears it; `terminal.copy` copies the focused terminal's selection.
These add no default keys or built-in copy mode. Mods read the inclusive selection
endpoints from `focused().selection` or `terminals()`; null means none (or no
attached view). Use `selection.end` as a copy-mode cursor. Selection facts update
when the selection changes, without refreshing readers on ordinary terminal output.

`terminal.search` opens a find bar in the focused terminal (or opens find-in-page
when a native browser pane is focused). It searches that
terminal's screen and scrollback; Enter and Shift-Enter move between matches,
Escape closes the bar while its find field has focus, and each terminal keeps its own query (reopening selects it, so typing replaces it).
Clicking the terminal while the bar is open puts typing, including Escape, back in the program; the bar and its matches stay.

It prints the created IDs (for example `{ "pane": "...", "terminal": "..." }`)
or `{ "error": "..." }` and exits 1. Read the result, then inspect `ls` or
`status` to confirm. Commands invoked by an agent never open a missing-argument
dialog: supply all required arguments. A successful launch reply means the
terminal was created, not that its program started correctly. If it exits
with a nonzero code within two seconds, the window shows an error toast with
the exit code and last output lines.

A terminal started with hc opts into the window with `herdr.space=<id>`.
`herdr.tab=<id>` chooses its tab; without it, Herdr creates a tab in that space.
Unlabeled workers stay hidden. Do not add labels just to run a background
worker. Use app commands when the
user wants to see a new tab or split, hc for orchestration that need not change
the screen.

## Customize layouts, keys, or mods

First read `~/.config/herdr-app/AGENTS.md` (or `$HERDR_APP_HOME/AGENTS.md` if
set). It is the installed guide to settings, layouts, commands and mods.
Read the generated `.d.ts` files beside it for full API details; do not edit
those files or the generated guide. The app rewrites them on start.

- `terminal_line_height` (default 1, 0.8..2) adjusts terminal rows;
  `terminal_cell_width` (default 1, 0.8..2) adjusts terminal columns;
  `terminal_scroll_speed` (default 1, 0.1..5) adjusts wheel and trackpad
  scrolling, including mouse reports. Set them in settings.json or the
  layout's look; the layout wins.
- Home defaults to `~/.config/herdr-app`; `HERDR_APP_HOME` selects another.
- `worktrees.directory` in settings.json sets the shipped New worktree destination,
  e.g. `{"worktrees":{"directory":"~/Developer/worktrees"}}`. Default:
  `~/.herdr/worktrees`; checkouts use `<directory>/<repo>/<branch>` (slashes in
  branches become dashes). Use an absolute path or `~` / `~/...`, resolved on the
  space's machine. Read on dialog open; existing checkouts stay put. No clones
  needed, and the app does not read the TUI's config.toml.
- Mods live in `mods/<name>/` with `main.js` and optional `mod.json`.
  They are trusted JavaScript with command and network access.
  `"focusable": true` in `mod.json` lets `pane.go` and `focus.cycle` (`Ctrl+\`` / `Ctrl+Shift+\``) move onto a copy; it receives
  `"focus"` from `nextCommand()` and focuses its own keyboard handle.
- A mod's `watchFolder(dir)` promise has `cancel()`; cancel it when the folder
  is no longer shown. It resolves with `[]` and releases only that call.
- Shipped mods belong to Herdr: duplicate with `mod.clone` before editing.
  Shipped layouts likewise use `layout.clone` for a personal copy.
  Discover required arguments with `herdr-app commands`.
- Save to reload. A broken save keeps the old version running and reports
  an error in `status`; a still-working screen is not proof the edit worked.
- Make only the requested change. Don't touch unrelated mods or settings.
- Use theme colors. Do not retain a mod's `cx` across callbacks; each
  callback receives its own. Native animations, not redraw timers, keep
  the window cheap to run. Root `on_size_change((size, cx) => …)` reports
  logical pixels after layout, including resizes without pointer movement.
  Optional `View.close(cx)` gets one bounded final call; start cleanup there,
  not in an earlier task. The installed guide explains its 250 ms quit budget
  and which cleanup commands go out before shutdown.

After every mod change:

```sh
herdr-app check ~/.config/herdr-app/mods/<name>
herdr-app status
herdr-app shot /tmp/mod.png   # then look at the picture
```

`check` loads the mod and renders it once. It exits nonzero with the exact
error. Its shared state starts empty and stays in memory; it leaves shared.json
untouched. Its `init()` still runs: import `checking` from `"herdr"` and guard
process side effects with `if (checking()) return;`. It is true throughout a
check and false in a live window; `exec`, `hc` and `writeTextFile` still run
and warn during checks. Never call a mod done without a passing check.
Then confirm it is shown correctly in the running window with `shot`; don't claim a visual
check you haven't made.

`herdr-app shot [file.png]` saves the window's current frame and prints its
absolute path (default: `herdr-shot.png` in your current folder). It is the
app's own rendering at full pixel size: terminals, images, overlays and
floats included, even when the window is covered or minimized. No screen
capture or permissions. It needs a running window that has drawn a frame;
`HERDR_APP_HOME` selects the window, as for `status`.

## Problems and safety

If Herdr misbehaves, `herdr-app feedback "<what happened>"` drafts a report.
The person reviews and sends it; it does not silently send feedback.

Never experiment with pointer/key automation on the person's real display.
When developing the app on Linux, its `scripts/sandbox` provides an isolated
home, core and display: `up`, `do`, `status`, `shot`, then `down` when done.
Never stop a server or session you didn't start.


## Native browser (Herdr Browser, macOS)

When `$HERDR_BROWSER` is set you are running inside Herdr Browser: always
show web pages in its native browser, never the system browser, Chrome or a
terminal browser. `herdr-browser do browser.open --profile agent --url <url>` opens a tab;
add `--place right --pane <your pane id from herdr-browser ls>` to open it
beside you. `$BROWSER` points at `herdr-browser-open`, so tools that open
URLs land there too. Cmd/Ctrl-click on a link in a terminal opens a browser
tab, and adding Shift opens it beside that pane.

`browser.open {url?, space?, name?, place?: "tab" | "right" | "down", pane?, profile?: "person" | "agent", focus?: boolean}`
opens a native Chromium/CEF browser (`browser.split {direction?: "right" | "down", url?, pane?, profile?}`
splits one beside the focused pane, or `pane`; Cmd+Shift+B or Cmd+Ctrl+B and the + menu's "Browser beside" use it), using the existing `native-browser` mod-pane identity.
From a script or agent (`herdr-browser do`), `browser.open` and `browser.markdown` default to `focus: false` — adding the tab unselected at the end of the caller terminal's own space (or the active space if the caller has no pane) without switching tabs or inserting beside the person's active tab. Pass `focus: true` only when you intentionally want to bring a tab straight to the front.
`browser.markdown {path, space?, name?, place?: "tab" | "right" | "down", pane?, profile?: "person" | "agent", focus?: boolean}`
opens a local Markdown file or folder on a token-gated loopback preview server with live reload, TOC, syntax highlighting, GitHub alerts, and interactive Mermaid diagrams (Cmd-click on a `.md` link in a terminal opens a tab; Shift+Cmd-click splits it beside that pane).
The + menu's Browser item uses it. It runs on this Mac even in a remote space.
`browser.navigate {url, pane?}`, `browser.back {pane?}`, `browser.forward {pane?}`,
`browser.reload {pane?}`, `browser.focus_url {pane?}`, `browser.find {query?, pane?}`, `browser.devtools {pane?}`
and `browser.inspect {pane?}` address an explicit pane. From a terminal, omitting
`pane` targets the browser that terminal opened most recently (`HC_TERMINAL_ID`, or `HERDR_PANE_ID`),
never the focused pane. With no caller mapping, pass `pane` explicitly. Mappings
last until the app closes; a closed target is an error, never a focus fallback.
The + menu and keyboard shortcuts still target the person's focused browser.
`herdr-browser ls` lists each tab's browser panes under `browsers`: `pane` and
`profile`, plus `url` and `title` for agent panes, so a pane opened earlier or
by someone else can be found again. A person pane's page stays out of the
listing; read it with `browser.inspect` and `confirmed`.
Responsive sites switch to their phone layout in a narrow split, which can drop
button labels and move controls; open a wide web app with `place: "tab"` (from a
script it is added unselected, so the person's view does not change), or widen
its split with `layout.resize {id: <pane>, weight}`. Weights are relative to the
sibling panes (read them before changing a split the person arranged, and put
them back); a tab nobody is viewing takes the new size when it is next shown.

Use `profile: "agent"` for agent work: cookies and storage live separately under
`chromium/Agent` in the app home. The default `person` profile retains existing
sign-ins. All programmatic reading/input/navigation commands on a person pane
require `confirmed: true`; pass it only when the owner authorized that page for
this task. This is an accidental-access guard, not authentication.

Page commands return their result through `herdr-browser do`:

- `browser.snapshot {pane?, scope?, filter?: "interactive" | "all"}`: accessibility
  tree as compact `e12 button "Sign in"` lines, URL, title and generation. Links and
  images end with where they point, `e7 link "" -> /explore` (site-relative on the
  page's own origin), so unlabeled icons and lists of links can be told apart.
  Optional `scope` is a CSS selector (one that matches nothing is an error);
  `filter: "interactive"` keeps only controls.
  At most 1000 nodes, with names capped to 300 characters; `capped` says if cut.
- `browser.read {pane?, format?: "text" | "markdown"}`: rendered visible text,
  URL and title, capped to 20000 characters. Markdown is escaped plain text.
- `browser.eval {pane?, js, timeout?}`: evaluate JavaScript in the page, like its
  console: page variables, top-level `await`, and the last expression's value back as
  JSON in `result` (up to 200000 bytes; return only the fields needed). One call can
  list a page's links, count items or read its state, where a snapshot would take
  several. It reads and does whatever the page can, including field values and
  storage, with the page's sign-in: on a person pane pass `confirmed` only for a
  page the owner authorized for this task, and never return secrets. Timeout is in
  milliseconds (default 5000, maximum 120000). Each call runs in its own block, so
  its `const`/`let` names neither persist nor clash with an earlier call's; assign
  `window.name = …` to keep a value. Never call `alert`, `confirm` or
  `prompt`: a dialog blocks the page.
  - **File inputs (`<input type="file">`)**: attach local non-secret files without
    opening a native file picker by building a `DataTransfer` + `new File(...)` in
    `browser.eval`, assigning `input.files = dt.files`, and dispatching `input` and
    `change` events.
  - **Downloads**: `http`/`https` and web-origin `blob:` downloads save under
    `~/.config/herdr-app-browser/downloads/` on agent panes (person panes show the
    save dialog; save a known URL there with `browser.download`).
  - **Secret-bearing downloads or tokens**: never print a secret into `browser.eval`
    stdout or chat. Wrap `herdr-browser do browser.eval` in a local script that reads
    the subprocess stdout in memory and pipes it straight to a `0600` file or a
    secret CLI (`gh secret set`, Keychain) without echoing a byte.
- `browser.download {pane?, url, timeout?}`: save a file with the page's own
  sign-in. The page fetches `url` (relative, `data:` and `blob:` URLs too; another
  site only if it lets this page read it, CORS, and without cookies when that site
  lets any origin read it, as public CDNs do) and the bytes land under the app
  home's `downloads/`, keeping the site's file name and never overwriting. Replies
  `path`, `bytes`, `content_type` and `status`; an HTTP error saves nothing. Up to
  32 MB; timeout in milliseconds (default 30000, maximum 120000). Use it for
  images a CDN serves only to the signed-in site (`curl` gets 403) rather than
  base64 through `browser.eval`. Like `browser.eval` it acts with the page's
  sign-in, so a person pane needs `confirmed`.
- `browser.click {pane?, ref? | name, role?, nth?}`: click a snapshot element, or
  the one whose accessible name is exactly `name` (no snapshot needed). Without
  `role` only interactive roles match; `role`/`nth` (1-based) pick among several.
  Refs survive unrelated DOM churn and expire on navigation or node removal.
- `browser.type {pane?, ref? | name, role?, nth?, text, submit?}`: replace a field's
  text, optionally Enter to submit. Password fields are refused; use `browser.signin`.
- `browser.signin {pane?, item}`: fill and submit the page's login form from a
  1Password item. The app runs `op` itself (the person approves with Touch ID), and
  fills only while the page's host matches a website on the item; values never
  appear in replies, argv or logs. Handles username-then-password pages.
  For checked-in test credentials, `browser.signin {env_file, user_key, password_key, site}`
  reads the two values from a dotenv file instead and fills only on `site` (a host).
  When the page asks for an HTTP sign-in instead (`browser.inspect` reports the
  asking host as `auth`, and the pane shows a username/password row), the same
  command answers it, under the same host rule.
- `browser.press {pane?, key}`: a page key such as Enter, Tab, Escape,
  ArrowDown, or a character/chord such as `Meta+a`. Password input is refused.
- `browser.scroll {pane?, ref? | name, dy?, dx?, via?: "wheel" | "script"}`: with only `ref`/`name`, scroll that
  element into view. With `dy`/`dx` (CSS pixels), wheel at the middle of the page, or
  over the element when one is also given (a list that scrolls inside the page). A
  wheel replies with what moved: `moved`, `dy`/`dx` actually scrolled, `top`/`left`
  and `max_top`/`max_left` of the scroller, and `at_end` once it can go no further
  that way. `moved: false` means nothing scrolled there. `measured: false`: the
  wheel went into a frame. The reply's `via` says how it scrolled: `"wheel"` is a
  real wheel event; `"script"` moves the same scroller without one, exactly and at
  once. A page that is not on screen is not rendered and a wheel stalls there, so it
  gets the script automatically; content that loads on scroll may then wait for a
  screenshot or for the pane to be shown. Pass `via` to choose.
- `browser.wait {pane?, text? | url? | load? | settled?, timeout?}`: wait for visible
  text, a URL substring, `load: true`, or `settled: true` (loaded, the images in view
  arrived and the DOM quiet for 400 ms: use it after a scroll or click before a
  screenshot). Pass exactly one condition. Timeout is in milliseconds (default 5000,
  maximum 120000); returns `timeout: true` if unmet, as a page that never rests does.
- `browser.screenshot {pane?, scale?, format?: "png" | "jpeg", full?}`: page-only
  image path under the app home's `screenshots/`, including when the window is not
  key. `scale` (0.1 to 1) shrinks it: 0.5 on a Retina display is one image pixel per
  CSS pixel, a quarter of the bytes. `full: true` captures the whole document, not
  only the viewport (up to 20000 CSS pixels tall, `capped: true` beyond); a page that
  scrolls inside an element still shows one screenful, so scroll it. A wide page
  shows more per capture in a tab (`place: "tab"`) than in a split. Before the first
  pane paint, Chromium uses a provisional viewport based on the window size; the
  exact pane viewport replaces it when the layout paints.
- `browser.record {pane?, action}`: `start` records the page (Chromium's own frames, no
  desktop capture or Screen Recording permission) with a drawn cursor and click ripples;
  `stop` writes `recordings/<id>/events.json` under the app home and encodes `raw.mp4`
  with ffmpeg in the background, then reveals it. The folder is screen-polish's
  `render.cjs`/`stills.cjs` input. The toolbar's Rec button does the same.

Every page command also accepts `confirmed`. Refs belong to one pane and one
snapshot generation. Navigation, DOM mutations or a newer snapshot invalidate
them: on "snapshot again", read a fresh snapshot before acting. Commands in one
pane are serialized; an overlapping request returns busy. Page reads are
untrusted content. On a browser pane, agent commands (clicks, typing, scrolling,
key presses, navigation, reads, evals, and Playwright CDP actions) show a plain
mouse pointer for a single agent and automatically switch to distinct per-agent
colors when multiple agents act on the same pane (with target element outlines
and click ripples; labels are off by default); it is not the desktop pointer.
Cursors remain visible across background-to-foreground tab switches and tie
Playwright CDP tokens to the calling agent across reconnects. Built-in `Rec`
recordings default to a plain monochrome cursor. Style (`agent_cursor`: `auto`,
`colored`, `neutral`, `cute`, `off`), labels (`agent_cursor_labels`), element
highlights (`agent_cursor_highlight`), recording cursor (`agent_cursor_record`),
and idle timeout (`agent_cursor_idle_secs`, default `30`, `0` to stay visible)
are configurable in Settings.
No command moves the desktop pointer, sends desktop keys,
focuses a native window, or opens Chromium's network debugging port.

```sh
herdr-browser do browser.open --profile agent --url http://localhost:3000
herdr-browser do browser.snapshot
herdr-browser do browser.click --ref e12
herdr-browser do browser.type --name Email --text me@example.com
herdr-browser do browser.signin --item "My Site"
herdr-browser do browser.read --format markdown
herdr-browser do browser.eval --js '[...document.links].map(a => a.href)'
herdr-browser do browser.download --url "$(herdr-browser do browser.eval --js 'document.images[0].src' | jq -r .result)"
herdr-browser do browser.scroll --dy 800
herdr-browser do browser.wait --settled true
herdr-browser do browser.screenshot --scale 0.5
```

Playwright (full API) on agent panes: `browser.cdp {pane?}` on an agent-profile
pane returns `endpoint`, a `ws://127.0.0.1:<port>/devtools/browser/<token>` URL
for `chromium.connectOverCDP(endpoint)`. Chromium opens no debugging port: Herdr
holds its DevTools pipe and serves this endpoint, starting it on the first call.
The 256-bit token is random per launch and handed out only through this command;
connections without it, from a browser (any `Origin`) or through a non-loopback
`Host` are refused. Playwright sees only agent-profile pages, as
`browser.contexts()[0]`; person panes refuse `browser.cdp` and stay invisible to
it. `context.newPage()` opens a visible agent pane beside the minting pane.
`browser.close()` only disconnects. Every agent pane is shared by all agents;
pick yours by URL. Not available over CDP: new browser contexts, permissions
grants, `clearCookies`, Playwright download events (agent downloads save under
the app home's `downloads/`), and popups opened by a page (they become Herdr
panes Playwright does not adopt). Chromium restores back/forward navigations
from cache without a `load` event, so use `page.goBack({ waitUntil: 'commit' })`.
Raw CDP bypasses the password-field and `confirmed` guards of the commands
above, which is why it reaches only the agent profile.

```js
const { endpoint } = JSON.parse(execSync('herdr-browser do browser.cdp --pane ' + pane));
const browser = await chromium.connectOverCDP(endpoint);
const page = browser.contexts()[0].pages().find((p) => p.url().startsWith('http://localhost:3000'));
await page.getByRole('button', { name: 'Sign in' }).click();
await page.screenshot({ path: 'page.png', fullPage: true });
```

The control socket waits for page command completion without blocking the UI.
Mods can dispatch the same commands; their synchronous `run()` acknowledges
`pending: true` (use the CLI for returned page data).
HTTP/HTTPS, localhost addresses and search queries work; website JavaScript has
no access to the privileged Herdr mod runtime. Cmd+L selects the address bar;
Cmd+Shift+[ / ] switch Herdr tabs even while the native page holds focus.
Cmd+T opens a terminal tab even while a webpage has focus.
Cmd+Option+B opens a browser tab; Cmd+B toggles the sidebar.
The ☆ beside the address bar saves the page to the favorites bar under it
(every browser pane, `favorites.json` in the app home); ★ removes it.
Browser documents stay alive across tab switches; closing their pane destroys
the native view. With the current GPUI backend, pages are temporarily hidden
while app menus/dialogs are open so native views cannot cover those controls.
A notification only cuts the bottom rows off the pages under it; they stay live.
Personal build updates are managed by `herdr-browser-update`, not the stock
Herdr binary updater. See `personal/README.md` in the fork.
