import { afterAll, beforeEach, expect, setSystemTime, test } from "bun:test";
import { readFile } from "node:fs/promises";

// Exercise the shipped tab bar's wheel handler without a window or a core.
type Handler = (event: unknown) => void;
class Element {
  nodes: Element[] = [];
  calls: Record<string, unknown[]> = {};
  constructor() {
    const proxy: Element = new Proxy(this, {
      get: (target, key: string) => key in target ? Reflect.get(target, key) : (...args: unknown[]) => {
        if (key === "child") target.nodes.push(args[0] as Element);
        else if (key === "children") target.nodes.push(...(args[0] as Element[]));
        else if (key === "when") { if (args[0]) (args[1] as (el: Element) => void)(proxy); }
        else target.calls[key] = args;
        return proxy;
      },
    });
    return proxy;
  }
}
const source = await readFile(new URL("../defaults/mods/tabs/main.js", import.meta.url), "utf8");
const element = () => new Element();
const commands: string[] = [];
const store: Record<string, string> = {};
const values: Record<string, unknown> = {
  View: class {}, div: element, svg: element, image: element, h_flex: element,
  ScrollHandle: { new: () => ({ scroll_to_item() {} }) },
  spaces: () => [{ id: "s1", active: true, tabs: [{ id: "t1", name: "a" }, { id: "t2", name: "b", active: true }, { id: "t3", name: "c" }] }],
  panes: () => [], run(command: string) { commands.push(command); }, openOverlay() {}, theme: () => ({}),
  window: () => null, shared: () => null, setShared() {}, layout: () => null, settings: () => ({ zoom: 1 }),
  localStorage: { getItem: (key: string) => store[key] ?? null, setItem(key: string, value: string) { store[key] = value; } },
};
const Tabs = new Function(...Object.keys(values), source.replace(/^import .*;\n/gm, "").replace("export default class Tabs", "return class Tabs"))(...Object.values(values));

let now = 1_000_000;
function wheel() {
  const view = new Tabs();
  view.init();
  // The mod is plain JS: its handler's shape is the documented ScrollWheelEvent listener.
  const handler = (view.render() as Element).calls.on_scroll_wheel[0] as Handler;
  return (y: number, touch_phase = "moved", lines?: number, ms = 8) => {
    now += ms;
    setSystemTime(new Date(now));
    handler({ delta: { x: 0, y }, delta_lines: lines === undefined ? undefined : { x: 0, y: lines }, touch_phase });
  };
}
beforeEach(() => { commands.length = 0; });
// Bun runs every test file in one process: give the later ones the real clock.
afterAll(() => setSystemTime());

test("a short touchpad swipe switches one tab", () => {
  const send = wheel();
  for (let i = 0; i < 12; i++) send(-6);
  expect(commands).toEqual(["tab.next"]);
});

test("a nudge below the threshold switches nothing", () => {
  const send = wheel();
  for (let i = 0; i < 3; i++) send(-5);
  expect(commands).toEqual([]);
});

test("each notch of a smooth-scrolling mouse switches one tab", () => {
  const send = wheel();
  // macOS smooths one notch into about 28 events of 1 px.
  for (let i = 0; i < 28; i++) send(-1, "moved", undefined, 10);
  send(-1, "moved", undefined, 700);
  for (let i = 0; i < 27; i++) send(-1, "moved", undefined, 10);
  expect(commands).toEqual(["tab.next", "tab.next"]);
});

test("a long swipe keeps switching, a tab per 60 px", () => {
  const send = wheel();
  // 20 px for the first tab, then 60 px each: 400 px is seven tabs.
  for (let i = 0; i < 40; i++) send(10, "moved", undefined, 40);
  expect(commands).toEqual(Array(7).fill("tab.prev"));
});

test("a fast flick shows each tab it passes for at least 100 ms", () => {
  const send = wheel();
  // 1200 px within 240 ms: enough for twenty tabs, time for three.
  for (let i = 0; i < 30; i++) send(-40);
  expect(commands).toEqual(["tab.next", "tab.next", "tab.next"]);
});

test("a pause or a turn starts a new gesture", () => {
  const send = wheel();
  for (let i = 0; i < 12; i++) send(-6);
  send(-30, "moved", undefined, 400);
  for (let i = 0; i < 4; i++) send(-5);
  for (let i = 0; i < 12; i++) send(6, "moved", undefined, 20);
  expect(commands).toEqual(["tab.next", "tab.next", "tab.prev"]);
});

test("each wheel notch switches one tab", () => {
  const send = wheel();
  send(-20, "moved", -1);
  send(-20, "moved", -1);
  send(2, "moved", 0.1);
  expect(commands).toEqual(["tab.next", "tab.next", "tab.prev"]);
});

const label = new Function("View", `${source.replace(/^import .*;\n/gm, "").replace("export default class Tabs", "class Tabs")}; return label;`)(class {});

const claude = { program: { name: "Claude Code", key: "claude", agent: true }, title: "my-task" };

test("a tab Herdr named after its agent follows the agent's session title", () => {
  expect(label({ name: "Claude Code", named: true }, claude)).toBe("my-task");
  expect(label({ name: "", named: false }, claude)).toBe("my-task");
  expect(label({ name: "", named: false }, { ...claude, title: "✦ [web] login" })).toBe("[web] login");
  expect(label({ name: "Claude Code", named: true }, { ...claude, title: "renamed-task" })).toBe("renamed-task");
});

test("an empty OSC title published as the terminal address keeps the program label", () => {
  for (const terminal of ["t2", "remote/t2"]) {
    const lead = { ...claude, terminal, title: terminal };
    expect(label({ name: "2", named: false }, lead)).toBe("Claude Code");
    expect(label({ name: "Claude Code", named: true }, lead)).toBe("Claude Code");
    expect(label({ name: "fix login", named: true }, lead)).toBe("fix login");
    expect(label({ name: "2", named: false }, { ...lead, title: "renamed-task" })).toBe("renamed-task");
  }
});

test("a name the person gave stays", () => {
  expect(label({ name: "fix login", named: true }, claude)).toBe("fix login");
});

test("a title that says nothing new keeps the tab's own name", () => {
  expect(label({ name: "Claude Code", named: true }, { ...claude, title: "Claude Code" })).toBe("Claude Code");
  expect(label({ name: "Claude Code", named: true }, { ...claude, title: "me@build-box" })).toBe("Claude Code");
  expect(label({ name: "", named: false }, { ...claude, title: "" })).toBe("Claude Code");
  expect(label({ name: "", named: false }, { ...claude, title: "~/src/api" })).toBe("Claude Code");
  expect(label({ name: "", named: false }, { ...claude, title: "✦ /tmp" })).toBe("Claude Code");
});

test("a shell's title never replaces its tab's name", () => {
  const shell = { program: { name: "zsh", key: "zsh" }, title: "vim notes.md" };
  expect(label({ name: "", named: false }, shell)).toBe("zsh");
  expect(label({ name: "notes", named: true }, shell)).toBe("notes");
  expect(label({ name: "", named: false }, undefined)).toBe("Terminal");
});

test("an unnamed browser tab uses its page title or url host, while a manual name stays", () => {
  expect(label({ name: "", named: false }, { mod: "native-browser", title: "Linear", url: "https://linear.app/vibes" })).toBe("Linear");
  expect(label({ name: "", named: false }, { mod: "native-browser", title: "native-browser", url: "https://www.github.com/Volpestyle" })).toBe("github.com");
  expect(label({ name: "", named: false }, { mod: "native-browser", title: "", url: "http://localhost:3000/app" })).toBe("localhost:3000");
  expect(label({ name: "", named: false }, { mod: "native-browser", title: "", url: "about:blank" })).toBe("Browser");
  expect(label({ name: "Docs", named: true }, { mod: "native-browser", title: "Linear", url: "https://linear.app" })).toBe("Docs");
});

test("a browser tab renders multicolor images, brand-tinted svgs, and globe fallbacks", () => {
  const svgs: { path: string; color?: unknown }[] = [];
  const imgs: string[] = [];
  const view = new (new Function(...Object.keys(values), source.replace(/^import .*;\n/gm, "").replace("export default class Tabs", "return class Tabs"))(
    ...Object.entries(values).map(([k, v]) =>
      k === "svg" ? (path: string) => { const el = element(); svgs.push({ path, get color() { return el.calls.text_color?.[0]; } }); return el; }
      : k === "image" ? (path: string) => { imgs.push(path); return element(); }
      : k === "panes" ? () => [
        { pane: "p1", tab: "t1", mod: "native-browser", icon: "mods/native-browser/sites/linear.svg", icon_tint: true, icon_color: "#5E6AD2" },
        { pane: "p2", tab: "t2", mod: "native-browser", icon: "mods/native-browser/sites/gitlab.svg", icon_tint: false, icon_color: null },
        { pane: "p3", tab: "t3", mod: "native-browser" },
      ]
      : v,
    ),
  ))();
  view.init();
  view.render();
  expect(svgs.find(s => s.path === "mods/native-browser/sites/linear.svg")?.color).toBe("#5E6AD2");
  expect(imgs).toContain("mods/native-browser/sites/gitlab.svg");
  expect(svgs.some(s => s.path === "mods/native-browser/browser.svg")).toBe(true);
});

test("dropping a dragged pane onto another tab or + runs pane.move and clears pane.drag", () => {
  const ran: [string, unknown][] = [];
  let drag: unknown = { pane: "p1", tab: "t1", space: "s1", machine: null };
  const byId = new Map<string, Element>();
  const tracked = () => {
    const el = element();
    const orig = new Proxy(el, {
      get: (target, key: string) => key === "id" ? (id: string) => { target.calls.id = [id]; byId.set(id, target); return orig; } : (target as any)[key],
    });
    return orig;
  };
  const view = new (new Function(...Object.keys(values), source.replace(/^import .*;\n/gm, "").replace("export default class Tabs", "return class Tabs"))(
    ...Object.entries(values).map(([k, v]) =>
      k === "div" || k === "h_flex" ? tracked
      : k === "shared" ? (key: string) => (key === "pane.drag" ? drag : null)
      : k === "setShared" ? (key: string, val: unknown) => { if (key === "pane.drag") drag = val; }
      : k === "run" ? (cmd: string, args: unknown) => ran.push([cmd, args])
      : v,
    ),
  ))();
  view.init();
  view.render();
  (byId.get("tab-t2")!.calls.on_mouse_up[1] as any)({}, { stop_propagation() {} });
  expect(drag).toBeNull();
  expect(ran).toEqual([["pane.move", { pane: "p1", tab: "t2" }]]);

  ran.length = 0;
  drag = { pane: "p1", tab: "t1", space: "s1", machine: null };
  view.render();
  (byId.get("tab-new")!.calls.on_mouse_up[1] as any)({}, { stop_propagation() {} });
  expect(drag).toBeNull();
  expect(ran).toEqual([["pane.move", { pane: "p1", tab: "new", space: "s1" }]]);
});
