// Context menus (Herdr's items). Opened as a popup with
// props { tab: "pane" | "tab" | "space" | "global", target }.
// ↑/↓ move, Enter runs, Esc closes. An item is [label, command, args];
// a command "worktree" opens the worktree dialog instead.

import { View, div, svg } from "gpui-kit";
import { v_flex, h_flex } from "gpui-base";
import { run, props, closeOverlay, openOverlay, theme, keys, terminals, spaces, panes, exec, shared, setShared } from "herdr";

// Where a space is now: its first terminal's live folder, else where it was made.
function place(s) {
  const tabs = new Set((s.tabs ?? []).map((c) => c.id));
  return panes().find((p) => tabs.has(p.tab) && p.cwd)?.cwd || s.folder;
}

// A space's checkout: "main" (a repo's own), "linked" (a worktree), or null.
async function checkout(folder, on) {
  const top = await exec("git", ["rev-parse", "--show-toplevel"], { cwd: folder, on });
  const common = await exec("git", ["rev-parse", "--path-format=absolute", "--git-common-dir"], { cwd: folder, on });
  if (top.code !== 0 || common.code !== 0) return null;
  return common.stdout.trim() === `${top.stdout.trim()}/.git` ? "main" : "linked";
}

// Space dot color choices: first six store theme keys matching spaces/picker,
// the extra two store direct hex colors.
const SWATCHES = [
  ["ansi6", "#22a99a", "Teal"],
  ["ansi3", "#e3814f", "Orange"],
  ["ansi4", "#5b8def", "Blue"],
  ["ansi5", "#8b7cf6", "Violet"],
  ["ansi2", "#3fae6a", "Green"],
  ["ansi1", "#d9649a", "Rose"],
  ["#e5b95c", "#e5b95c", "Amber"],
  ["#9aa3b2", "#9aa3b2", "Slate"],
];

function setSpaceColor(id, value) {
  const prev = shared("space.colors");
  const next = prev && typeof prev === "object" && !Array.isArray(prev) ? { ...prev } : {};
  next[id] = value;
  setShared("space.colors", next);
}

// Pinned tabs come first in their space and show a pin icon with a compact label in the tab strip (shared "tabs.pinned": tab ids).
function pin({ id, space }) {
  const was = shared("tabs.pinned") ?? [];
  const now = was.includes(id) ? was.filter((x) => x !== id) : [...was, id];
  setShared("tabs.pinned", now);
  const tabs = spaces().find((s) => s.id === space)?.tabs ?? [];
  run("tab.move", { id, index: tabs.filter((c) => c.id !== id && now.includes(c.id)).length + 1 });
}

function tabDisplayName(tab, ps) {
  if (tab.named) return tab.name;
  const mine = ps.filter((p) => p.tab === tab.id);
  const lead = mine.find((p) => p.program?.agent) ?? mine.find((p) => p.focused) ?? mine[0];
  if (lead?.mod === "native-browser") {
    const title = (lead.title ?? "").trim();
    if (title && title !== "native-browser" && title !== "Browser") return title;
    if (lead.url) {
      try {
        const u = new URL(lead.url);
        if (u.host) return u.host.replace(/^www\./, "");
      } catch {}
    }
    return "Browser";
  }
  return lead?.program?.name || tab.name;
}

function items(p, git, dir = "right") {
  p = p ?? {};
  const tg = p.target ?? {};
  switch (p.tab) {
    case "pane": {
      // A kept sub-agent can go back under its parent, still running.
      const all = terminals();
      const term = all.find((t) => t.address === tg.terminal);
      const parent = term?.parent && all.find((t) => t.address === term.parent);
      // Once they go to the program, Shift+right-click still opens this menu.
      const toProgram = term?.labels?.["herdr.right-click"] === "program";
      // A terminal shown outside a tab (a layout terminal block, a window) has
      // no pane to split, zoom or close: its pane is the block's id.
      const allPanes = panes();
      const me = allPanes.find((pane) => pane.pane === tg.pane);
      const inTab = Boolean(me);
      const panesInTab = me ? allPanes.filter((pane) => pane.tab === me.tab).length : 0;
      const mySpace = me ? spaces().find((s) => (s.tabs ?? []).some((c) => c.id === me.tab)) : null;
      const hasOtherTabs = Boolean(
        mySpace &&
          spaces().some(
            (s) =>
              (s.machine ?? null) === (mySpace.machine ?? null) &&
              (s.tabs ?? []).some((c) => c.id !== me.tab && !c.settled),
          ),
      );
      return [
        ...(inTab
          ? [
              ["Split right", "pane.split", { direction: "right", pane: tg.pane }],
              ["Split down", "pane.split", { direction: "down", pane: tg.pane }],
              ["Zoom", "pane.zoom", { pane: tg.pane }],
              ...(panesInTab > 1 ? [["Move to new tab", "pane.move", { pane: tg.pane, tab: "new" }]] : []),
              ...(hasOtherTabs ? [["Move to tab…", "move-pane", { pane: tg.pane }]] : []),
            ]
          : []),
        ...(term
          ? [
              toProgram
                ? ["Use Herdr right-click menu", "terminal.right_click", { terminal: term.address, to: "menu" }]
                : ["Send right-clicks to pane", "terminal.right_click", { terminal: term.address, to: "program" }],
            ]
          : []),
        ...(parent ? [[`Send back to ${parent.name || parent.address}`, "terminal.park", { terminal: term.address }]] : []),
        ...(inTab ? [["Close pane", "pane.close", { pane: tg.pane }]] : []),
      ];
    }
    case "move-pane": {
      const allPanes = panes();
      const me = allPanes.find((pane) => pane.pane === tg.pane);
      if (!me) return [];
      const allSpaces = spaces();
      const mySpace = allSpaces.find((s) => (s.tabs ?? []).some((c) => c.id === me.tab));
      if (!mySpace) return [];
      const panesInTab = allPanes.filter((pane) => pane.tab === me.tab).length;
      const out = [];
      if (panesInTab > 1) out.push(["New tab", "pane.move", { pane: me.pane, tab: "new", direction: dir }]);
      const sameMachine = allSpaces.filter((s) => (s.machine ?? null) === (mySpace.machine ?? null));
      const ordered = [mySpace, ...sameMachine.filter((s) => s.id !== mySpace.id)];
      for (const sp of ordered) {
        for (const tab of (sp.tabs ?? []).filter((c) => c.id !== me.tab && !c.settled)) {
          const title = tabDisplayName(tab, allPanes);
          const label = sp.id === mySpace.id ? title : `${sp.name} · ${title}`;
          out.push([label, "pane.move", { pane: me.pane, tab: tab.id, direction: dir }]);
        }
      }
      return out;
    }
    case "tab":
      return [
        ["Rename", "tab.rename", { id: tg.id }],
        [(shared("tabs.pinned") ?? []).includes(tg.id) ? "Unpin" : "Pin", "pin", { id: tg.id, space: tg.space }],
        ["New tab", "tab.new", { space: tg.space }],
        ["Close", "tab.close", { id: tg.id }],
      ];
    case "space":
      return [
        ["Rename", "space.rename", { id: tg.id }],
        ...(git ? [["New worktree", "worktree", { space: tg.id, action: "new" }], ["Open worktree", "worktree", { space: tg.id, action: "open" }]] : []),
        ...(git === "linked" ? [["Remove worktree", "worktree", { space: tg.id, action: "remove" }]] : []),
        ["Close", "space.close", { id: tg.id }],
      ];
    default:
      return [
        ["New space", "space.new", {}],
        ["Keybinds", "app.help", {}],
        ["Theme: graphite", "app.theme", { name: "graphite" }],
        ["Theme: ink", "app.theme", { name: "ink" }],
        ["Theme: moss", "app.theme", { name: "moss" }],
        ["Theme: atom one dark", "app.theme", { name: "atom-one-dark" }],
      ];
  }
}

function menuIcon(command) {
  const name = command.endsWith(".close") ? "x" : command.endsWith(".rename") ? "square-pen" : command.endsWith(".new") ? "plus" : command === "worktree" ? "git-branch" : command === "pin" ? "pin" : command === "app.help" ? "terminal" : command === "app.theme" ? "settings" : command === "pane.split" || command === "pane.move" || command === "move-pane" ? "panel-right" : command === "pane.zoom" ? "square" : "terminal";
  return `mods/workspace-chrome/${name}.svg`;
}

// The key bound to a command, for the hint on the right.
function shortcut(name, args) {
  // `pane.split:right` binds pane.split with { direction: "right" }.
  const value = args && Object.values(args)[0];
  const k = keys().find((b) => b.section === "global" && (b.action === name || b.action === `${name}:${value}`));
  return k ? k.keys : "";
}

export default class Menu extends View {
  init(_props, cx) {
    this.at = 0;
    this.dir = "right";
    this.git = null;
    this.handle = cx.focus_handle();
    this.handle.focus();
    const p = props();
    const space = p?.tab === "space" && spaces().find((s) => s.id === p.target?.id);
    const folder = space && place(space);
    if (folder)
      cx.spawn(async (cx) => {
        this.git = await checkout(folder, space.machine).catch(() => null);
        cx.notify();
      });
  }

  choose(i, cx) {
    const it = items(props(), this.git, this.dir)[i];
    if (!it) return;
    closeOverlay(props()?.overlay);
    if (it[1] === "worktree") openOverlay({ mod: "worktree", kind: "dialog", ...(it[2].action === "open" ? { width: 560, height: 480 } : { width: 440, height: 260 }), props: it[2] });
    else if (it[1] === "move-pane") openOverlay({ mod: "menu", kind: "popup", width: 260, height: 360, props: { tab: "move-pane", target: it[2] } });
    else if (it[1] === "pin") pin(it[2]);
    else run(it[1], it[2]);
  }

  render(cx) {
    const t = theme();
    const p = props();
    const list = items(p, this.git, this.dir);
    const movePane = p?.tab === "move-pane";
    const spaceId = p?.tab === "space" ? p.target?.id : null;
    const rawColors = shared("space.colors");
    const currentColor = spaceId && rawColors && typeof rawColors === "object" ? rawColors[spaceId] : null;
    const colorRow = spaceId
      ? v_flex()
          .child(div().mx(7).my(4).h(1).bg(t.border))
          .child(
            h_flex()
              .id("mi-space-colors")
              .px(8)
              .py(5)
              .gap(5)
              .items_center()
              .justify_between()
              .children(
                SWATCHES.map(([key, hex, label]) => {
                  const active = currentColor === key || currentColor === hex;
                  return h_flex()
                    .id(`mi-color-${label.toLowerCase()}`)
                    .size(18)
                    .flex_none()
                    .rounded_full()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .tooltip(label)
                    .when(active, (el) => el.border(1.5).border_color(t.foreground))
                    .when(!active, (el) => el.hover((s) => s.bg(t.hover)))
                    .on_click(() => {
                      setSpaceColor(spaceId, key);
                      closeOverlay(p?.overlay);
                    })
                    .child(div().size(12).rounded_full().bg(hex));
                }),
              ),
          )
      : null;
    const closeIndex = spaceId ? list.findIndex((it) => it[1] === "space.close") : -1;
    return v_flex()
      .id("menu")
      .track_focus(this.handle)
      .w_full()
      .min_h(0)
      .p(5)
      .bg(t.popover)
      .text_size(12.8)
      .on_key_down((e, cx) => {
        if (e.key === "down" || e.key === "j") this.at = list.length ? (this.at + 1) % list.length : 0;
        else if (e.key === "up" || e.key === "k") this.at = list.length ? (this.at + list.length - 1) % list.length : 0;
        else if (movePane && e.key === "r") this.dir = "right";
        else if (movePane && e.key === "d") this.dir = "down";
        else if (e.key === "enter") this.choose(this.at, cx);
        else if (e.key === "escape") closeOverlay(props().overlay);
        else return;
        cx.notify();
      })
      .when(movePane, (el) =>
        el.child(
          h_flex()
            .px(8)
            .pt(4)
            .pb(6)
            .mb(3)
            .border_b(1)
            .border_color(t.border)
            .items_center()
            .justify_between()
            .child(div().text_size(11).font_medium().text_color(t.dim).child("Move pane to tab"))
            .child(
              h_flex()
                .gap(3)
                .children(
                  ["right", "down"].map((d) =>
                    div()
                      .id(`move-dir-${d}`)
                      .px(6)
                      .py(2)
                      .rounded(5)
                      .text_size(10.5)
                      .cursor_pointer()
                      .bg(this.dir === d ? t.selected : t.popover)
                      .text_color(this.dir === d ? t.foreground : t.dim)
                      .hover((s) => s.bg(t.hover).text_color(t.foreground))
                      .on_click((_e, cx) => {
                        this.dir = d;
                        cx.notify();
                      })
                      .child(d === "right" ? "Right" : "Down"),
                  ),
                ),
            ),
        ),
      )
      .children(
        list.map((it, i) =>
          v_flex()
            .when(i === closeIndex && Boolean(colorRow), (el) => el.child(colorRow))
            .when(i > 0 && (it[1].endsWith(".close") || it[1] === "app.help" || (it[1] === "app.theme" && list[i - 1][1] !== "app.theme")), el => el.child(div().mx(7).my(4).h(1).bg(t.border)))
            .child(h_flex()
            .id(`mi-${i}`)
            .h(31)
            .px(8)
            .gap(9)
            .rounded(7)
            .items_center()
            .justify_between()
            .text_color(t.foreground)
            .when(i === this.at, (el) => el.bg(t.selected))
            .hover((s) => s.bg(t.hover))
            .on_click((_e, cx) => this.choose(i, cx))
            .child(svg(menuIcon(it[1])).size(14).flex_none().text_color(t.muted))
            .child(div().flex_1().min_w(0).truncate().child(it[0]))
            .child(div().text_size(11).text_color(t.dim).child(movePane ? "" : shortcut(it[1], it[2])))),
        ),
      );
  }
}
