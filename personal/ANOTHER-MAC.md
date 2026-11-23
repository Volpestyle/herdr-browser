# Herdr Browser on another Mac

This build targets macOS on Apple silicon. The source is maintained in
`Volpestyle/herdr-browser` on `james/native-browser`. The optional appearance profile
is maintained separately in the private `Volpestyle/dotfiles` repository under
`config/herdr-app-browser`.

## Push signed updates over LAN without shell access

To push signed local builds (including H.264/AAC codecs) from a build Mac to
another Mac on the same LAN without giving unattended SSH shell access:

1. Generate a dedicated staging key on the build Mac (`~/.ssh/id_ed25519_herdr_stage`)
   and add an SSH alias (`Host other-mac-stage`) using `AddressFamily inet` and
   `IdentitiesOnly yes`. Keep any regular interactive SSH alias password-only.
2. On the receiving Mac, install a wrapper at `~/.local/bin/herdr-browser-receive`
   that execs `python3 ~/dev/herdr-app-browser/personal/stage_receive.py`, and add
   the staging public key to `~/.ssh/authorized_keys` locked down with OpenSSH
   `restrict`:

```text
restrict,from="<lan-cidr>,fe80::/10",command="/Users/<user>/.local/bin/herdr-browser-receive" ssh-ed25519 <pubkey> herdr-browser-stage-only
```

3. Push a signed bundle from the build Mac:

```sh
python3 personal/stage_push.py other-mac-stage --status    # read-only status
python3 personal/stage_push.py other-mac-stage             # push installed app
python3 personal/stage_push.py other-mac-stage --pending   # push freshly staged build
```

`stage_receive.py` allows only `status` and `stage` (rejecting all other commands),
validates the tar stream against path traversal, escaping symlinks/hardlinks, and
size limits, never executes the received binary, verifies `codesign --verify --deep --strict -R`
against the installed app's Apple designated requirement (or a pinned requirement
at `~/.config/herdr-browser-stage/requirement`), refuses version/Chromium downgrades,
and stages the bundle into `state.json` without quitting the running app. Activate it
on the receiving Mac whenever you are ready to restart:

```sh
herdr-browser-update install && open "$HOME/Applications/Herdr Browser.app"
```

## Copy the tested app manually

Install ordinary [Herdr](https://herdr.dev/download/) on the new Mac first so
herdr-core is available (or install a notarized stock-CEF build via
`personal/install.sh`). Keep stock Herdr alongside the custom app.

To transfer a local development-signed build offline (for example via AirDrop),
archive `~/Applications/Herdr Browser.app` with `ditto -c -k --keepParent` on the
source Mac. The archive contains the signed application only; browser profiles,
cookies, Keychain data, running sessions and personal/work agent credentials
remain on their original Mac.

On a Mac without Herdr Browser installed, extract the archive into
`~/Applications`; if a copy is already installed, close that custom app and
back it up before replacing it. Finder can extract it, or use:

```sh
mkdir -p ~/Applications
ditto -x -k ~/Downloads/Herdr-Browser-apple-silicon.zip ~/Applications
codesign --verify --deep --strict "$HOME/Applications/Herdr Browser.app"
```

Local builds are development-signed and are not notarized for public distribution.
macOS or a managed Mac's policy may require approval to open them (for a notarized
build without proprietary H.264/AAC codecs, use a public `browser-v*` GitHub Release
via `personal/install.sh`).
The portable bundle selects the current account's `~/.config/herdr-app-browser`;
it has no baked-in user path. Sign in to websites normally on the second Mac.

## Restore the appearance and keyboard shortcuts

Use your personal GitHub account to access the private dotfiles repository.
These steps install only Herdr's appearance; they do not install or switch
personal agent accounts on the work Mac. Reuse existing clones when present.

```sh
git clone git@github.com:Volpestyle/dotfiles.git ~/dotfiles
git clone --branch james/native-browser --recurse-submodules \
  https://github.com/Volpestyle/herdr-browser.git ~/dev/herdr-app-browser
python3 ~/dotfiles/scripts/herdr-browser-setup.py
python3 ~/dotfiles/scripts/herdr-browser-setup.py --apply
python3 ~/dev/herdr-app-browser/personal/browser_update.py install
open "$HOME/Applications/Herdr Browser.app"
```

Use Python 3.11 or newer. The setup previews by default, backs up replaced
files, links authored settings to dotfiles, and merges just the saved appearance
settings and selected layout. It restores the full-app wallpaper, transparent
Ghostty layout, Browser menu, tab bar and Cmd+Shift+[ / ] shortcuts. It leaves
machine connection settings and local session/browser state in place. Generated
mod typings live under ignored `.local/herdr-app-browser` in dotfiles.

## Build and update from source

Copying the signed app does not require transferring a signing private key.
For local builds and automatic updates, install Rust (tested with 1.96), Bun,
Python 3.11+, and cargo-about 0.9.3. Configure a stable personal signing identity
available in this Mac's Keychain; list identities with:

```sh
security find-identity -v -p codesigning
```

Put the selected identity's fingerprint in the ignored
`~/dev/herdr-app-browser/.local/browser-updater/signing_identity` file. Each
machine owns its signing selection, cache, updater state and LaunchAgent.
Then, from the fork:

```sh
python3 personal/browser_update.py snapshot
python3 personal/browser_update.py build --tag v0.1.10
python3 personal/browser_update.py install
python3 personal/browser_update.py enable
```

The updater checks official Herdr and stable CEF releases, builds the fork,
and installs validated updates after the custom app closes. Zig and the CEF
SDK are downloaded into local caches during the build. Keep both Git clones
in place for the updater, URL opener and configuration links. Closing the app keeps core
terminals running. Fork source and configuration are backed up on GitHub;
build caches, signing credentials and live app data are deliberately local.
