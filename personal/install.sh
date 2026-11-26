#!/bin/sh
# Installs Herdr Browser (macOS Apple Silicon) from GitHub Releases, plus
# upstream herdr-core (hc) if none is installed yet:
#
#   curl -fsSL https://raw.githubusercontent.com/Volpestyle/herdr-browser/james/native-browser/personal/install.sh | sh
#
# Optional environment variables:
#   HERDR_BROWSER_VERSION=browser-v0.1.12   Install a specific release tag
#   HERDR_BROWSER_REPO=Volpestyle/herdr-browser
#   HERDR_BROWSER_BASE_URL=...              Override download directory (testing/mirrors)
#   HERDR_RELEASES_URL=https://releases.herdr.dev
set -eu

repo=${HERDR_BROWSER_REPO:-Volpestyle/herdr-browser}
herdr_releases=${HERDR_RELEASES_URL:-https://releases.herdr.dev}
bin="$HOME/.local/bin"
apps="$HOME/Applications"
app="$apps/Herdr Browser.app"
app_home="$HOME/.config/herdr-app-browser"
archive_name="herdr-browser-macos-aarch64.tar.gz"

say() { printf '%s\n' "$*"; }
fail() { say "herdr-browser: $*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || fail "needs $1"; }
need curl
need tar

[ "$(uname -s)" = Darwin ] || fail "Herdr Browser currently supports Apple Silicon macOS only. For Linux or Windows, install upstream Herdr: https://herdr.dev"

arch=$(uname -m)
if [ "$arch" = x86_64 ]; then
  if [ "$(sysctl -n hw.optional.arm64 2>/dev/null || true)" = 1 ] ||
     [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || true)" = 1 ]; then
    arch=arm64
  else
    fail "Intel Macs are not supported; Herdr Browser requires Apple Silicon."
  fi
fi
[ "$arch" = arm64 ] || [ "$arch" = aarch64 ] || fail "unsupported architecture: $arch"

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

if /bin/ps -axo comm= | grep -Fqx "$app/Contents/MacOS/HerdrBrowser"; then
  fail "Herdr Browser is running; quit it before installing or updating (your herdr-core terminals stay alive)."
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

if [ -n "${HERDR_BROWSER_BASE_URL:-}" ]; then
  base_url=${HERDR_BROWSER_BASE_URL%/}
  tag=${HERDR_BROWSER_VERSION:-custom}
else
  if [ -n "${HERDR_BROWSER_VERSION:-}" ]; then
    tag=$HERDR_BROWSER_VERSION
    base_url="https://github.com/$repo/releases/download/$tag"
  else
    api="https://api.github.com/repos/$repo/releases/latest"
    curl -fsSL -H "Accept: application/vnd.github+json" "$api" -o "$tmp/release.json" ||
      fail "no public GitHub release found yet at https://github.com/$repo/releases"
    tag=$(tr -d '\n' < "$tmp/release.json" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
    [ -n "$tag" ] || fail "could not determine latest release tag from $api"
    base_url="https://github.com/$repo/releases/download/$tag"
  fi
fi

say "Installing Herdr Browser ($tag) for macOS Apple Silicon..."
curl -fsSL "$base_url/SHA256SUMS" -o "$tmp/SHA256SUMS" || fail "failed to download SHA256SUMS from $base_url"
want=$(awk -v name="$archive_name" '$2 == name || $2 == "*" name { print $1; exit }' "$tmp/SHA256SUMS")
[ -n "$want" ] || fail "SHA256SUMS does not contain an entry for $archive_name"

curl -fsSL "$base_url/$archive_name" -o "$tmp/$archive_name" || fail "failed to download $archive_name"
got=$(sha256 "$tmp/$archive_name")
[ "$got" = "$want" ] || fail "checksum mismatch for $archive_name (expected $want, got $got)"

tar -xzf "$tmp/$archive_name" -C "$tmp"
[ -d "$tmp/Herdr Browser.app" ] || fail "archive did not contain Herdr Browser.app"

if signature=$(/usr/bin/codesign -dvv "$tmp/Herdr Browser.app" 2>&1); then
  if printf '%s\n' "$signature" | grep -q '^Authority=Developer ID Application:'; then
    if [ -n "${HERDR_BROWSER_TEAM_ID:-}" ]; then
      printf '%s\n' "$signature" | grep -qx "TeamIdentifier=$HERDR_BROWSER_TEAM_ID" ||
        fail "app signature TeamIdentifier does not match HERDR_BROWSER_TEAM_ID ($HERDR_BROWSER_TEAM_ID)"
    fi
    /usr/bin/codesign --verify --deep --strict "$tmp/Herdr Browser.app" || fail "invalid Developer ID signature"
  elif printf '%s\n' "$signature" | grep -qx 'Signature=adhoc'; then
    if [ "${HERDR_BROWSER_ALLOW_ADHOC:-0}" != 1 ]; then
      fail "refusing ad-hoc signed build (set HERDR_BROWSER_ALLOW_ADHOC=1 to allow an unsigned/ad-hoc build)"
    fi
    say "Warning: installing ad-hoc signed build (HERDR_BROWSER_ALLOW_ADHOC=1); macOS may ask you to allow it in Privacy & Security."
  else
    /usr/bin/codesign --verify --deep --strict "$tmp/Herdr Browser.app" || fail "invalid app signature"
  fi
else
  fail "cannot verify app signature: $signature"
fi

mkdir -p "$apps" "$bin"
incoming="$apps/Herdr Browser.app.incoming.noindex"
previous="$apps/Herdr Browser.app.previous.noindex"
rm -rf "$incoming"
mv "$tmp/Herdr Browser.app" "$incoming"
rm -rf "$previous"
if [ -e "$app" ]; then
  mv "$app" "$previous"
fi
if ! mv "$incoming" "$app"; then
  [ -e "$previous" ] && mv "$previous" "$app"
  fail "failed to move Herdr Browser.app into $apps"
fi
rm -rf "$previous"

cat > "$bin/herdr-browser" <<EOF
#!/bin/sh
export HERDR_APP_HOME="$app_home"
exec "$app/Contents/MacOS/HerdrBrowser" "\$@"
EOF
chmod 755 "$bin/herdr-browser"

if [ -f "$app/Contents/Resources/browser_open.py" ]; then
  cat > "$bin/herdr-browser-open" <<EOF
#!/bin/sh
exec /usr/bin/env python3 "$app/Contents/Resources/browser_open.py" --launcher "$bin/herdr-browser" --app "$app" -- "\$@"
EOF
  chmod 755 "$bin/herdr-browser-open"
fi

/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$app" 2>/dev/null || true

# Install a lightweight loader skill so coding agents automatically discover
# `herdr-browser --skill` without ever going stale across app updates.
# Never overwrite a user-customized skill file.
skill_dir="$HOME/.agents/skills/herdr-browser"
skill_file="$skill_dir/SKILL.md"
if [ ! -e "$skill_file" ] || { [ ! -L "$skill_file" ] && grep -q 'herdr-browser-managed-skill' "$skill_file" 2>/dev/null; }; then
  mkdir -p "$skill_dir"
  cat > "$skill_file" <<'EOF'
---
name: herdr-browser
description: "Open, inspect, and drive native Chromium browser panes inside Herdr Browser (`herdr-browser do browser.*` and guarded Playwright `browser.cdp`). Use whenever `$HERDR_BROWSER` is set or the user asks to browse, preview, click, fill forms, upload files, or record pages in Herdr Browser."
---
<!-- herdr-browser-managed-skill -->

# Herdr Browser

Load the live manual from the installed binary each session:

```sh
herdr-browser --skill
```

Follow that output as this skill's instructions. Use `profile: "agent"` for
autonomous browsing; pass `--confirmed true` on a `person` pane only when the
user authorized that page for this task. Keep this loader small and do not save
the manual's output over this file.
EOF
fi
for root in "$HOME/.claude/skills" "$HOME/.codex/skills"; do
  if [ -d "$root" ] && [ ! -e "$root/herdr-browser" ] && [ ! -L "$root/herdr-browser" ]; then
    ln -s "$skill_dir" "$root/herdr-browser"
  fi
done

# Install upstream herdr-core (hc) only if missing.
if [ ! -e "$bin/herdr-core" ] && ! command -v herdr-core >/dev/null 2>&1; then
  curl -fsSL "$herdr_releases/core/install.sh" | HC_INSTALL_DIR="$bin" HERDR_RELEASES_URL="$herdr_releases" sh ||
    say "herdr-core didn't install automatically; Herdr Browser will offer it when opened."
fi

say "Herdr Browser ($tag) is installed in $app."
say "Open it from ~/Applications, or run: herdr-browser"
case ":$PATH:" in
  *":$bin:"*) ;;
  *) say "Add $bin to your PATH to use herdr-browser, herdr-browser-open, and hc from a terminal." ;;
esac
