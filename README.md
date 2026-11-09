# Selkie

A native desktop client for [Podman](https://podman.io) and Docker, written in
Rust with [`gpui`](https://gpui.rs), Zed's GPU-accelerated UI framework — a
deliberately small, lightweight alternative to
[Podman Desktop](https://podman-desktop.io).

Selkie is an independent project. It is not affiliated with or endorsed by the
Podman, Podman Desktop or Docker projects.

[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Rust 1.85+](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org)
[![UI: gpui](https://img.shields.io/badge/ui-gpui-8a2be2.svg)](https://gpui.rs)
[![Engines: Podman and Docker](https://img.shields.io/badge/engines-Podman%20%26%20Docker-2e7dbe.svg)](https://podman.io)
[![Platform: Linux](https://img.shields.io/badge/platform-Linux-lightgrey.svg)](#requirements)

## Features

- **Engines** — Podman through its CLI (default), Podman (API) and Docker through
  their HTTP APIs ([`bollard`](https://docs.rs/bollard)). The engine's CLI is
  still used for anything that streams a child process: logs, the terminal,
  compose, build, pull/push and registry login. Anything the selected engine
  cannot do is hidden rather than shown broken.
- **Dashboard** — resource counts, disk usage (`podman system df`) and engine
  details (`podman info`).
- **Containers** — list, search, inspect, run from an image (name, ports,
  environment, volumes, secrets, command), start / stop / restart / pause /
  resume / rename / delete, streaming logs with **Load older**, a live **Stats**
  tab, and an interactive terminal with 2000 lines of scrollback, copy/paste and
  mouse reporting.
- **Images** — pull, push, tag, delete, and build from a Containerfile (build
  context, `--build-arg` table, multi-platform manifest lists), plus a
  **History** tab and a **Files** browser for the image filesystem.
- **Registries** — login and logout with the password sent over stdin, and an
  **Accounts** page built from Podman's auth files.
- **Pods, volumes, networks, secrets** — list, create, inspect and delete; prune
  unused volumes and networks; `kube play` a Kubernetes YAML; secrets can be
  mounted into a container from the run dialog.
- **Compose** — projects grouped from container labels, with **Up**
  (`up -d`), **Restart**, **Build**, **Down** and streamed service logs.
- **Machines** — list, `machine init`, start/stop, force delete and an SSH
  terminal.
- **Connections** — add, edit and delete `ssh://` and `unix://` connections,
  check reachability, and choose which engine this app talks to. A
  passphrase-protected SSH key is remembered by the app, unlocked with a prompt,
  and held by a private `ssh-agent` that loads only that key.
- **Bulk delete** — **Select** turns rows into checkboxes on the container,
  image, pod, volume, network and secret lists; **Delete (N)** removes every
  checked row after a single confirmation.
- **UI** — details pane with a raw **JSON** view, hover row actions and a
  right-click context menu, resizable sidebar and details panels, sortable
  columns, search, confirmations for destructive actions, live updates driven by
  `podman events`, dark / light / high-contrast themes, and keyboard navigation
  (`j`/`k`, `/` search, `Esc`, `Ctrl/Cmd+R` refresh, `Ctrl/Cmd+Q` quit).
- **Desktop integration** — tray icon, single instance, close-to-tray, and window
  size / maximized state restored on startup.

## Requirements

- Linux with the usual GPUI build dependencies. Fedora:

  ```bash
  sudo dnf install clang libxkbcommon-devel libxkbcommon-x11-devel wayland-devel \
      vulkan-loader-devel fontconfig-devel freetype-devel alsa-lib-devel
  ```

  Debian/Ubuntu equivalents: `clang libxkbcommon-dev libxkbcommon-x11-dev
  libwayland-dev libvulkan-dev libfontconfig1-dev libfreetype-dev
  libasound2-dev`.
- Rust 1.85+ (tested with 1.99).
- Podman in `PATH` (tested with 5.4) **or** Docker reachable through its socket
  (`DOCKER_HOST` is honoured). The **Podman (API)** engine additionally needs the
  Podman socket — `systemctl --user enable --now podman.socket` (rootless),
  `/run/podman/podman.sock` (rootful) or whatever `CONTAINER_HOST` names.
- `tar` in `PATH` for the image file browser.
- The tray icon needs a StatusNotifierItem host: KDE and XFCE provide one, GNOME
  needs the *AppIndicator and KStatusNotifierItem Support* extension. Without a
  host the app runs without an icon and prints a warning.

## Build and run

```bash
cargo run                 # debug build with optimized dependencies
cargo run --release
```

The first build compiles GPUI and takes a while.

## Install

### From the source tree (user-level)

```bash
packaging/install.sh                # ~/.local, desktop entry and icons included
packaging/install.sh --uninstall
```

`install.sh` also works from an extracted release archive and accepts `PREFIX`,
`BINDIR`, `DATADIR` and `DESTDIR`.

### Release archive and Debian package

```bash
packaging/build-release.sh   # dist/selkie-<version>-<target>.tar.gz (+ .sha256)
packaging/build-deb.sh       # dist/selkie_<version>-1_<arch>.deb
sudo apt install ./dist/selkie_<version>-1_<arch>.deb
```

The archive contains the binary, docs, desktop entry, icons and `install.sh`.
The Debian package installs `/usr/bin/selkie`, the desktop entry (menu category
*Development*), 16–512 px icons, a manual page and the docs under
`/usr/share/doc/selkie/`. It depends on `podman`, `libvulkan1`,
`libwayland-client0` and the X11/XKB libraries, and recommends
`mesa-vulkan-drivers`. Building it needs only `dpkg-deb`; with `dpkg-dev`
installed the shared-library versions are computed from the binary.
`DEB_REVISION=<n>` changes the Debian revision (default `1`).

### Machines on Debian/Ubuntu

`podman machine start` needs two helpers that Debian/Ubuntu install outside
Podman's search paths:

- **`gvproxy`** lives in `/usr/bin`. Add that directory to
  `~/.config/containers/containers.conf`:

  ```toml
  [engine]
  helper_binaries_dir = ["/usr/libexec/podman", "/usr/lib/podman", "/usr/bin"]
  ```

- **`virtiofsd`** is installed as `/usr/libexec/virtiofsd`, which is not on
  `PATH`. Install the `virtiofsd` package and link it somewhere on `PATH`, for
  example `ln -s /usr/libexec/virtiofsd ~/.local/bin/virtiofsd`.

The VM also needs `qemu-system-x86` and a usable `/dev/kvm`. Once it runs, pick
the `podman-machine-default` connection in the **Connections** tab to make this
app talk to it.

## Settings

Settings live in `$XDG_CONFIG_HOME/selkie/settings.json` (falling back to
`~/.config/selkie/settings.json`) and are saved whenever they change: theme,
active tab and Settings pane, auto-refresh interval (3 / 5 / 10 / 30 s), log
window (5 m – 1 h), sidebar and details widths, window size and maximized state,
and the selected connection. Invalid fields fall back to their defaults per
field; delete the file to reset everything.

## Tests and CI

```bash
cargo test
cargo clippy
```

Codeberg runs the same checks on every push and pull request through
[`.woodpecker.yaml`](.woodpecker.yaml). A `vX.Y.Z` tag must match `version` in
`Cargo.toml`; the pipeline then builds the `.deb` and attaches it, with
`SHA256SUMS`, to the Codeberg release. Everything runs on `rust:1.99-trixie`, so
the published package records `libc6 (>= 2.39)` — fine on Ubuntu 24.04 and
newer, while Debian 12 users should build it locally. The upload step needs a
`codeberg_token` Woodpecker secret with the `write:repository` scope.

## Known limitations

- **Podman (CLI)** runs one process per action and streams pull/build output as
  plain text. This is deliberate: no socket is required and every Podman version
  that prints `--format json` works.
- **Podman (API) and Docker** expose their Docker-compatible surface, so pods,
  standalone secrets, machines and Podman-style named connections are hidden.
  SSH connections are CLI-only — use the CLI engine, or forward the socket
  yourself (`ssh -L`). Docker has no multi-platform manifest build here (`-t`
  only) and no per-command `--tls-verify`, so insecure loopback registries depend
  on the daemon configuration.
- **Live updates** come from `podman events`; when it is unavailable the app
  falls back to polling and reconnects every 3 s. Health-status text can lag
  until the next poll.
- **Logs** keep 800 lines; **Load older** grows the buffer with `--tail` to at
  most 3000 lines and turns Follow off (slow on very large log files).
- **Stats** are sampled every 2 s only while the tab is open, keep 120 samples
  (about four minutes) and are not persisted.
- **Bulk delete** runs one command per row, so a large selection takes a moment.
  It covers the container, image, pod, volume, network and secret lists, not
  compose projects, machines or connections.
- **Terminal** always runs `/bin/sh` for `podman exec`; while it has focus, keys
  including app shortcuts go to the shell. `OSC 8` hyperlinks are drawn as plain
  text, the kitty keyboard protocol is not negotiated, and a hard kill of the app
  can leave an idle exec shell in the container until it restarts.
- **Compose** needs a working provider (`docker-compose` v2 or `podman-compose`)
  and only groups containers carrying the standard compose labels. **Down**
  removes containers and networks, not volumes or images, and there are no
  per-service actions or compose file editing.
- **Image files** stream `podman export` through `tar` from a throwaway
  container: each opened file re-reads the stream, only the first 128 KiB is
  shown, and there is no download action.
- **Machines** only exist where `podman machine` is supported; `machine init`
  downloads a multi-gigabyte VM image, and start/stop is a blocking task with a
  toast rather than streamed output.
- **Registries** — there is no registry management page, no multi-arch manifest
  push and no insecure-registry configuration beyond the automatic loopback
  `--tls-verify=false`. Passwords are masked and sent over stdin but stay in
  memory until the command finishes.
- **Run** always detaches (`podman run -d`): no attach mode, restart policies,
  resource limits, custom networks, user or privileged options yet.
- **Builds** validate only the registries named in `FROM` lines (no probing) and
  offer no cache control, target selection or secrets. The Containerfile and any
  explicit context must exist on the machine running the app.
- **Remote engines** must be reachable already: the connection form covers
  `ssh://` and `unix://` only, and reachability results are in-memory. Key
  passphrases are prompted in the app, but **SSH password prompts and host-key
  confirmation are not supported** — use a key/agent and keep the host in
  `known_hosts`. Encrypted key paths live in this app's settings, so other tools
  using `podman system connection` do not see them, and switching connections
  never changes your Podman default.
- **Misc** — no command palette, no Kubernetes or extension system; sorting is
  per tab and resets on tab switch; window position is not persisted; GPUI only
  implements a native menu bar on macOS, so actions live in the sidebar, context
  menus and the tray.

## Roadmap and internals

[ROADMAP.md](ROADMAP.md) holds the delivered scope, the per-module architecture
table, the file map and the verification checklist.
