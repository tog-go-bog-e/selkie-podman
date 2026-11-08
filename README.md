# Selkie

A native desktop client for [Podman](https://podman.io) and Docker, written in
Rust with [`gpui`](https://gpui.rs), Zed's GPU-accelerated UI framework. It is a
deliberately small, lightweight alternative to
[Podman Desktop](https://podman-desktop.io).

Selkie is an independent project. It is not affiliated with or endorsed by the
Podman, Podman Desktop or Docker projects.

Current scope: **manage containers and images on the local Podman or Docker
engine**.

## Features

- **Engines**: choose **Podman** (default) or **Docker** under
  `Settings → Engine`. Podman keeps using its CLI, so every Podman version that
  prints `--format json` works. Docker is reached through its HTTP API with
  [`bollard`](https://docs.rs/bollard), so the Docker socket is used directly.
  Docker has no pods, standalone secrets, machines or Podman-style named
  connections; those views are hidden when Docker is selected, and the Run
  dialog's Secrets field is omitted. The Docker CLI is still used for commands
  that stream a child process (logs, the interactive terminal, compose, build,
  pull/push and registry login).
- **Dashboard**: a landing overview with resource counts (containers running and
  total, images, pods, volumes, networks, machines), **disk usage** from
  `podman system df` (size and reclaimable space for images, containers and
  volumes, with a bar for what can be reclaimed) and **engine details** from
  `podman info` (version, OS/arch, host, kernel, CPUs, host memory use, runtime,
  network backend, storage driver, event log, rootless/rootful, connection,
  socket and graph root). Counts follow live updates; the slower `podman info`
  data is cached and re-read on entry, on Refresh or at most every 30 s.
- **Containers**: list, search, inspect summary, **run from an image** (name,
  ports, environment, volumes, secrets and command override), start / stop / restart /
  pause / resume / rename / delete, streaming logs (configurable window,
  auto-follow, **Load older**, clear and copy), a **Stats** tab with live CPU,
  memory and network graphs, and an **interactive terminal** (`podman exec` +
  `/bin/sh`) that resizes with the pane, keeps 2000 lines of scrollback (mouse
  wheel plus a **Live** button), honours application cursor keys, and supports
  drag/double-click selection with copy (`Ctrl+Shift+C`) and paste
  (`Ctrl+Shift+V`, bracketed when the program asks). Containers belonging to a
  compose project show `project/service` under their name.
- **Compose**: projects grouped from container labels (`com.docker.compose.*`
  and `io.podman.compose.*`), with running/total status, services, compose file
  and working directory. **Up** (`up -d`), **Restart**, **Build** and **Down**
  (confirmed) run `podman compose`, streaming their output into a panel below the
  list. A **Logs** tab streams `podman compose logs -f --tail 200`, with chips to
  switch between all services and a single service. Container details show the
  project and service.
- **Images**: list, search, inspect summary, **run**, pull (with streamed
  progress), **push**, tag, delete, and **build** from a Containerfile with
  streamed output. The build dialog mirrors upstream Podman Desktop: the
  Containerfile path and image name are always visible, **More options** adds the
  build context, a **Key/Value** build-argument table and selectable platform
  cards (amd64, arm64, ppc64le, s390x, wasi, plus **New platform** for any other
  `os/arch`), passed as a comma-separated `--platform`; several platforms build
  into a manifest list. **Validate registries
  before building** checks that the registries its `FROM` lines pull from are
  public or have credentials in Podman's auth files. The context defaults to the
  Containerfile's directory. Image details add a **History** tab
  (`podman history`: layer ID, size, timestamp, created-by and comment) and a
  **Files** tab that browses the image filesystem by directory, previews text
  files, reports binary files and shows symlink targets.
- **Registries**: **Login** and **Logout** from the Images toolbar. The login
  dialog sends the password to `podman login` on stdin (never on the command
  line) and shows it masked; push destinations are validated and streamed into
  the output panel.
- **Pods**: list, search, inspect summary, **create** (name + optional port
  mappings), **play a Kubernetes YAML file** (streamed output), start / stop /
  restart / delete.
- **Volumes**: list, search, inspect summary (mountpoint, driver, usage),
  **create** (name + optional driver), **prune unused** (confirmed), delete.
- **Networks**: list, search, inspect summary (subnets, connected containers),
  **create** (name, optional driver / subnet / gateway), **prune unused**
  (confirmed), delete.
- **Secrets**: list (name, ID, driver, created, updated), search (including
  labels), a summary with the engine and labels, **create** (name + data; the
  data is masked in the dialog, sent to `podman secret create` on stdin and never
  read back), delete (confirmed), and the raw `podman secret inspect` JSON. The
  Run dialog's **Secrets** field maps them into a container: `name` mounts it at
  `/run/secrets/name`, `name:/path` mounts it at a path and `name:VAR` exposes it
  as an environment variable.
- **Machines**: list Podman machines (state, driver, CPUs, memory, disk, last up),
  **create** them (`podman machine init` with optional name, CPUs, memory and
  disk size, streamed into the output panel), **Start** / **Stop** (stop asks for
  confirmation), **Delete** (confirmed, force-removes the VM), and open an
  **SSH terminal** on a running machine in the details pane
  (`podman machine ssh`).
- **Connections**: list configured connections (name, URI, identity, Podman
  default) with a reachability **STATUS**, and choose which engine this app talks
  to. **Add connection** runs `podman system connection add` for an SSH
  (`ssh://user@host:22/run/user/1000/podman/podman.sock`) or `unix://` endpoint,
  with an optional identity file (`~` is expanded). Leaving the identity empty
  uses the running **ssh-agent**; if the app did not inherit `SSH_AUTH_SOCK`,
  Podman is pointed at `$XDG_RUNTIME_DIR/openssh_agent` or
  `$XDG_RUNTIME_DIR/keyring/ssh` when present. Interactive password or
  passphrase prompts cannot be answered in-app, so load the key into the agent
  or pick an identity file that needs no passphrase. **Check all**, a row's
  **Check** or the details action bar probes
  each connection with `podman --connection <name> info`; the column, details
  pane and a toast report `reachable` / `unreachable` with Podman's error.
  **Edit…** reopens the same form prefilled (a name change renames the
  connection first, a URI/identity change re-adds it in place) and **Delete…**
  removes it after a confirmation; deleting the connection this app uses falls
  back to Podman's default engine. Switching uses `--connection <name>` for
  every command and persists across restarts; **Use Podman default** switches
  back. Switching never modifies the user's Podman configuration, and the
  connection form does not pass `--default` (Podman itself may mark the first
  connection as its default).
- **Details pane**: image, ID, status, created, PID, exit code, networks,
  command, mounts and environment from `podman inspect`; separate Summary, Logs,
  Stats and Terminal tabs for containers, and a Terminal tab for running
  machines (SSH). A **JSON** button switches any
  inspectable resource (containers, images, pods, volumes, networks, secrets) to its raw,
  pretty-printed `inspect` output, with Copy.
- **Resizable panels**: drag the divider next to the sidebar or the details pane;
  both widths persist across restarts and are clamped so the list keeps a usable
  size. Double-click a divider to restore the default width.
- **Sortable tables**: click a column header to sort by it, click again to
  reverse (▲/▼); numeric columns such as size, mounts and CPUs sort numerically.
  Sorting resets when you switch tabs. Long values ellipsize in narrow columns,
  including when the resizable panels leave little space.
- **Live data**: `podman events` drives updates as soon as containers, images, pods,
  volumes, networks or compose projects change; a periodic poll
  (3 / 5 / 10 / 30 s, configurable) remains as a fallback. The status bar shows
  `live` when the event stream is connected, or `auto Ns` when polling. The last
  good snapshot stays visible if the engine goes away.
- **Safe actions**: destructive actions ask for confirmation, image references
  are validated, and results/errors are shown as toasts.
- **Search**: case-insensitive filtering by name, image, ID, state or status.
- **Row actions**: hover a row for Start / Stop / Pause / Restart / Delete
  (containers), Start / Stop / Restart / Delete (pods), Tag / Delete (images),
  Delete (volumes, networks and secrets), Up / Down (compose projects), Start / Stop /
  Delete (machines) or Use (connections); right-click for a context menu with the
  same actions plus Pause / Resume / Rename (containers), Terminal (SSH) and
  Delete (machines) and Copy ID/name/mountpoint/URI.
- **Keyboard**: `j`/`k` or arrows navigate, `Enter` opens the first row when
  nothing is selected, `/` focuses search, `Esc` closes menus/details or clears
  search, `Ctrl/Cmd+R` refreshes and `Ctrl/Cmd+Q` quits. In dialogs, `Enter`
  confirms and `Esc` cancels (in the build dialog's “New platform” field,
  `Enter` adds the platform).
- **Themes and settings**: dark, light and high-contrast themes; theme, active
  tab, refresh interval, log window and window size all persist across restarts.
  **Settings** is split into `Registries`, `Appearance`, `Engine` and `About`,
  mirroring the upstream layout.
- **Accounts**: the **Accounts** page shows the same registry table as
  `Settings → Registries` — every registry Podman has credentials for (read from
  its auth files), with the username and a masked password (or `token`), a `⋮`
  menu with **Log out**, **Add registry**, and one-click **Configure** buttons for
  Docker Hub, Red Hat Quay, GitHub and Google Container Registry.
- **Desktop integration**: a system tray icon (Show window / Refresh / Quit), a
  single-instance lock so re-launching focuses the running app, and window size
  (plus maximized state) restored on startup. **Closing the window docks the app
  in the tray** instead of quitting: its window is unmapped (no taskbar entry),
  the process keeps running at ~0% CPU, and the tray's *Show window* or another
  launch brings it back. Quit from the tray or with `Ctrl/Cmd+Q`; without an X
  display to unmap, closing quits as usual.

## Requirements

- Rust 1.85+ (tested with 1.92)
- Podman in `PATH` (tested with 5.4), **or** Docker Engine reachable through its
  socket (`DOCKER_HOST` is honoured). The Docker CLI is needed for the
  streaming/interactive features listed under Engines.
- `tar` in `PATH` for the image file browser (GNU tar or busybox tar)
- Linux: the usual GPUI build dependencies, e.g. on Fedora:

  ```bash
  sudo dnf install clang libxkbcommon-devel libxkbcommon-x11-devel wayland-devel \
      vulkan-loader-devel fontconfig-devel freetype-devel alsa-lib-devel
  ```

  On Debian/Ubuntu the equivalents are `libxkbcommon-dev`, `libxkbcommon-x11-dev`,
  `libwayland-dev`, `libvulkan-dev`, `libfontconfig1-dev`, `libfreetype-dev`,
  `libasound2-dev`. If only the runtime `libxkbcommon-x11` package is installed,
  `build.rs` creates the missing unversioned link automatically.
- The tray icon needs a StatusNotifierItem host: KDE and XFCE provide one,
  GNOME needs the *AppIndicator and KStatusNotifierItem Support* extension. Without
  a host the app still runs, just without the icon (a warning is printed to
  stderr).

## Run

```bash
cd selkie
cargo run          # debug, optimized deps
cargo run --release
```

The first build compiles GPUI and takes a while.

## Install

A user-level install puts the binary in `~/.local/bin`, a desktop entry in
`~/.local/share/applications` and the icons in the hicolor theme, so the app
shows up in your launcher with its icon:

```bash
packaging/render-icons.sh   # only needed after editing the SVG (needs Inkscape/rsvg)
packaging/build-release.sh  # builds target/release and dist/<name>.tar.gz
./install.sh                # from an extracted release archive
# or, from the source tree:
packaging/install.sh
```

`install.sh` works from both layouts and accepts `--uninstall` plus the usual
`PREFIX` (default `~/.local`), `BINDIR`, `DATADIR` and `DESTDIR` overrides:

```bash
PREFIX=$HOME/.local packaging/install.sh
DESTDIR=/tmp/stage packaging/install.sh        # packaging for a distro/AppImage
packaging/install.sh --uninstall
```

`packaging/build-release.sh` writes
`dist/selkie-<version>-<target>.tar.gz` (binary, README, ROADMAP,
LICENSE, desktop entry, icons, installer) and a `.sha256` next to it.

### Debian package

`packaging/build-deb.sh` builds a `.deb` that installs the binary system-wide
together with the desktop entry, the icons and a man page:

```bash
packaging/build-deb.sh
sudo apt install ./dist/selkie_<version>-1_<arch>.deb
```

The package is named `selkie`, depends on `podman`, `libvulkan1` and
`libwayland-client0` (plus the X11/XKB libraries the binary links) and recommends
`mesa-vulkan-drivers` for the software renderer. It installs:

| Path | Contents |
| ---- | -------- |
| `/usr/bin/selkie` | stripped release binary |
| `/usr/share/applications/selkie.desktop` | menu entry (`Development` category) |
| `/usr/share/icons/hicolor/*/apps/selkie.png` | 16–512 px icons |
| `/usr/share/man/man1/selkie.1.gz` | manual page |
| `/usr/share/doc/selkie/` | README, ROADMAP, copyright, changelog |

The entry appears in the desktop menu under **Development** (Cinnamon, GNOME,
KDE, XFCE all read it); `update-desktop-database` and the icon cache are
refreshed by the package's maintainer scripts. Only `dpkg-deb` is required to
build it — when `dpkg-shlibdeps` (from `dpkg-dev`) is available the shared
library dependencies are computed from the binary instead of guessed. Use
`DEB_REVISION=<n>` to change the Debian revision (default `1`), and `lintian`
runs automatically when installed.

Notes:

- The window sets `WM_CLASS=selkie`, matching the desktop entry's
  `StartupWMClass`, so launchers group the window with its icon. GPUI has no icon
  API, so the app sets `_NET_WM_ICON` itself (sizes 16–128, drawn at runtime from
  the same code as the tray icon); window managers that draw the icon in the
  titlebar, window list or Alt-Tab pick it up. On Wayland the compositor uses the
  desktop entry instead.
- `tar` is required at runtime for the image file browser
  (see [Requirements](#requirements)).

### Machines on Debian/Ubuntu

`podman machine start` needs helper binaries that Debian/Ubuntu install outside
Podman's built-in lookup paths. Two fixes are needed:

- **`gvproxy`** lives in `/usr/bin`, which the built-in `helper_binaries_dir`
  does not include, so starting fails with *could not find "gvproxy"*. Add the
  directory in your user config:

  ```toml
  # ~/.config/containers/containers.conf
  [engine]
  helper_binaries_dir = ["/usr/libexec/podman", "/usr/lib/podman", "/usr/bin"]
  ```

- **`virtiofsd`** (used for the default `$HOME:$HOME` mount) comes from the
  `virtiofsd` package but is installed as `/usr/libexec/virtiofsd`, which is not
  on `PATH`. Put it on `PATH`, for example:

  ```bash
  sudo apt install virtiofsd
  ln -s /usr/libexec/virtiofsd ~/.local/bin/virtiofsd   # if ~/.local/bin is on PATH
  ```

  (Without the package, extracting `usr/libexec/virtiofsd` from the `.deb` into
  any directory on `PATH` also works.) Check with `command -v virtiofsd`. The VM
  also needs `qemu-system-x86` and a usable `/dev/kvm`.

After a machine starts, Podman adds `podman-machine-default` (and `-root`) as
connections. The host CLI keeps using the local engine; select the connection in
the **Connections** tab to make this app talk to the VM.

## How it works

- **Podman**: `src/podman.rs` talks to the CLI: `podman ps`, `podman images`,
  `podman inspect`, `podman start/stop/restart/pause/unpause/rename/rm`,
  `podman run`, `podman pull/tag/rmi`, `podman compose`, `podman info` /
  `podman system df` (Dashboard only) and a long-running `podman logs -f`.
  Registry accounts are read straight from Podman's auth files
  (`REGISTRY_AUTH_FILE`, the runtime `auth.json` and `~/.docker/config.json`);
  only the username and credential type are shown, never the secret.
- **Docker**: `src/podman/docker.rs` uses [`bollard`](https://docs.rs/bollard)
  against the Docker socket (`DOCKER_HOST` is honoured) for the snapshot,
  dashboard, details, stats and container/volume/network/image actions. A small
  Tokio runtime bridges bollard's async API to the app's synchronous background
  calls. Podman-only resources return an error, and the UI hides their views.
  Operations that stream a child process use the `docker` CLI (selected by
  `src/podman/exec.rs`); `src/podman/engine.rs` holds the current engine and its
  capability flags.
  All commands use argument arrays (no shell), and user-supplied image
  references, names, port mappings, env entries and mounts are validated before
  being passed through. Compose projects are derived from the labels on
  `podman ps` rather than from a separate state file. The client is split by
  area under `src/podman/` (`exec` for running commands and the selected
  connection, then `containers`, `images`, `compose`, `resources` for pods,
  volumes and networks, `machines`, `registry`, `system` and `util`), all
  re-exported from `src/podman.rs`.
- `src/ui.rs` holds the `Selkie` entity: shared state, the constructor
  and the render entry point. Behaviour and rendering are split by concern
  (all `impl Selkie` or shared helpers, reusing the state in `ui.rs`):
  - `src/ui/loading.rs` — snapshot loading and the refresh cycle
  - `src/ui/navigation.rs` — keyboard navigation and the row context menu
  - `src/ui/selection.rs` — selection, details data, log streams, stats, actions
  - `src/ui/modals.rs` — opening and submitting modal dialogs
  - `src/ui/search.rs` / `src/ui/sorting.rs` — table search and column sorting
  - `src/ui/sidebar.rs` — navigation, engine status, Accounts/Settings entries
  - `src/ui/dashboard.rs` — resource cards, disk usage and engine details
  - `src/ui/accounts.rs` — registry credentials read from the engine's credential files
  - `src/ui/preferences.rs` — the Settings page (appearance, panels, engine, about)
  - `src/ui/table.rs` — toolbars, search field, sortable headers and table bodies
  - `src/ui/rows.rs` — all row builders and hover action buttons
  - `src/ui/compose.rs` — compose project summary and actions
  - `src/ui/output.rs` — streamed-output runner and panel for compose actions and builds
  - `src/ui/engines.rs` — machine and connection summaries and actions
  - `src/ui/details.rs` — inspect panes and the streaming log viewer
  - `src/ui/dialogs.rs` — status bar, toasts, context menus, confirm modals
  - `src/ui/helpers.rs` — shared widget builders (`pill_button`, `menu_item`, …)
  - `src/ui/resizer.rs` — draggable sidebar/details splitters, width limits
  - `src/ui/theme.rs` — palettes for dark / light / high contrast
  - `src/ui/settings.rs` — persisted settings (load, save, cycling)

  Background tasks call Podman and feed results back into the entity.
- Logs use `podman logs -f --since <window> --timestamps` rather than `--tail N`,
  because `--tail` can scan very large log files for seconds before emitting
  anything. Output is capped at 800 lines and ANSI escapes are stripped. The
  **Load older** button runs a one-shot `podman logs --tail <current + 500>` in
  the background, merges only lines older than the oldest buffered one, and
  anchors the viewport so the lines you were reading stay put. Repeated clicks
  reach further back (buffer capped at 3000 lines), and Follow switches off so
  live output does not jump the view.
- Stats sample `podman stats --no-stream --interval 1 --format json` every 2 s
  while the Stats tab is open (one process per sample, ~50-100 ms each), keep the
  last 120 samples in memory and render them as bar sparklines: CPU on a 0-100%
  scale, memory scaled to the visible peak, network as bytes/second derived from
  the cumulative counters. Sampling stops when the tab or selection changes and
  preserves history when reopening the tab for the same container.
- Pushes run `podman push <image> <destination>` and stream into the shared
  output panel; the destination is validated like any image reference. Registry
  login uses `podman login --username <user> --password-stdin <registry>` with
  the password written to stdin and closed, so it never appears in the process
  list or logs. `podman logout <registry>` (or `--all` when the field is empty)
  removes the stored credentials. Loopback registries (`localhost`, `127.0.0.1`,
  `[::1]`) automatically get `--tls-verify=false`, matching local dev registries.
- Runs use `podman run -d` with optional `--name`, repeated `-p`/`-e`/`-v`/`--secret`
  and a command appended after the image. Secret mappings must name an existing
  secret. Name, port mappings (`host:container[/proto]`),
  environment entries (`KEY=VALUE`) and mounts (`source:destination[:options]`)
  are validated; space-separated lists are split without shell quoting, while
  the command accepts shell-style `'…'`/`"…"` quoting and `\` escapes. The
  container is left detached so it appears in the list and can be opened in the
  Terminal tab.
- Builds run `podman build -f <Containerfile>`, then `-t <tag>` for one platform
  or `--manifest <tag>` for several (Podman stores those as a manifest list),
  followed by `--platform <platform>[,<platform>…]` and one `--build-arg
  KEY=VALUE` per filled-in argument row, then the build context. The context
  defaults to the Containerfile's directory and must be an existing directory
  when supplied.
  Paths are validated and the Containerfile must exist locally; when registry
  validation is on, the registries in its `FROM` lines must be public or
  authenticated before the build starts. stdout/stderr stream into the shared
  output panel (Clear/Close, success/failure toast, image list refresh). Pulls
  use the same panel, so `podman pull` progress and errors are visible instead
  of being hidden behind a spinner. Only one build, compose action or pull can
  run at a time.
- Machines and connections are read from local Podman configuration
  (`podman machine list`, `podman system connection list`) as a separate
  `LocalState`, so they stay visible even when the selected engine is
  unreachable. When a connection is selected, `podman::base_command()` prepends
  `--connection <name>` to every engine command (including logs and compose);
  machine and connection commands always run locally. **Add connection** builds
  `podman system connection add [--identity FILE] NAME URI` (only `ssh://` and
  absolute `unix://` URIs are accepted, and the identity file must exist).
  Reachability probes run in the background, one per connection, with a 15 s
  timeout: on the first visit to the tab, when a new connection appears and on
  demand. Results live in memory per session; switching the engine does not
  re-probe.
- Updates: `src/ui/events.rs` runs a long-lived `podman events --format json`
  child (restarted after 3 s if it exits), ignores health-check/exec events,
  coalesces bursts for 300 ms and refreshes the snapshot. The refresh path
  remembers a request that arrives while a fetch is running, and the periodic
  timer skips when an event already refreshed within its interval — so polling
  only acts as a fallback.
- Terminal: `src/terminal.rs` allocates a PTY (`openpty`), attaches
  `podman exec -it -e TERM=xterm-256color <id> /bin/sh` to the slave side and
  reads the master on a thread; `vt100` emulates the screen (colors, cursor,
  alternate screen) and `src/ui/terminal.rs` renders it as style-grouped
  monospace runs. The grid follows the pane: the details layout is measured
  during prepaint, the chosen font's advance (from `TextSystem::advance`) turns
  the pixel width into columns, and `Parser::set_size` plus `TIOCSWINSZ` resize
  the emulator and the pty. The emulator's callbacks feed `OSC 0/2` titles into
  the pane header. Keystrokes are translated to PTY bytes (printable
  characters, arrows, Home/End, Delete, Tab, Enter, Backspace, Ctrl+letter,
  Alt); `Ctrl+Shift+C`/`V` copy and paste (wrapped in `ESC[200~`/`ESC[201~`
  when the program enabled bracketed paste), and mouse events are reported to
  the program when it asked for them (`DECSET 9/1000/1002/1003`, SGR or the
  legacy encodings). Leaving the terminal closes the PTY so the shell exits
  cleanly, and the session is kept when switching to Summary/Logs/Stats. The
  exec client also gets `PR_SET_PDEATHSIG`, so it cannot outlive a hard kill of
  the app.
- `src/desktop.rs` covers the desktop integration:
  - the single instance is an exclusive `flock` on
    `$XDG_RUNTIME_DIR/selkie.lock`; the primary instance also listens
    on a Unix socket there and a second launch sends `activate` before exiting;
  - the tray icon is a freedesktop StatusNotifierItem served over D-Bus with
    `ksni` (no GTK/AppIndicator C libraries); its icon is generated at runtime;
  - the main window gets a `_NET_WM_ICON` property over a short-lived second X11
    connection (`x11rb`), once the window manager has created the window, so the
    icon drawn at runtime also shows up in titlebars, window lists and Alt-Tab;
  - tray and second-launch signals travel over channels to the GPUI thread,
    which polls them every 500 ms together with the window size.
- `src/main.rs` opens the window, wires the close handler, and owns the
  single-instance/tray/window-state task.

## Settings

Settings live in `$XDG_CONFIG_HOME/selkie/settings.json`, falling back
to `~/.config/selkie/settings.json`, and are saved whenever they
change:

- **Theme** — Dark / Light / High contrast in **Settings → Appearance**.
- **Auto refresh** — 3 / 5 / 10 / 30 seconds in **Settings → Appearance**; used as
  the fallback poll when the `podman events` stream is unavailable or quiet.
- **Log window** — 5m / 10m / 30m / 1h in **Settings → Appearance** or via the
  `last …` button in the log toolbar; passed to `podman logs --since`.
- **Active tab and Settings pane** — restored on the next launch.
- **Sidebar and details widths** — set by dragging the dividers, saved on
  release, double-click to reset (or **Reset** in Settings → Appearance);
  clamped on load and when the window shrinks.
- **Window size and maximized state** — recorded every 500 ms when they change,
  and on close.
- **Connection** — the Podman connection (or the local default) used for every
  command.

Invalid values in the file fall back to defaults per field; delete the file to
reset everything.

## Tests

```bash
cargo test          # parsers, formatting, filtering, validation
cargo clippy
```

## Roadmap

See [ROADMAP.md](ROADMAP.md) for the delivered scope, architecture notes,
prioritized next milestones and verification checklist.

## Known limitations

- **Podman engine**: the CLI is used instead of the libpod REST API: one process
  per action and no structured progress reporting (pull/build output is streamed
  as plain text). This is deliberate for now (no socket dependency); revisit the
  REST API if those capabilities become priorities — see [ROADMAP.md](ROADMAP.md).
- **Docker engine**: pods, standalone secrets, machines and Podman-style named
  connections do not exist, so their views and the Run dialog's Secrets field are
  hidden. Docker uses its HTTP API for lists, details, stats and actions, but the
  Docker CLI is still spawned for logs, the interactive terminal, compose, build,
  pull/push and registry login. Multi-platform Docker builds keep one tag
  (`-t`); Docker has no `podman build --manifest` equivalent here. Insecure
  loopback registries rely on Docker's daemon configuration because Docker has
  no per-command `--tls-verify`.
- Logs stream the selected window (default 10 minutes) into an 800-line buffer.
  **Load older** grows that buffer with `--tail` up to 3000 lines and turns
  Follow off; it stops with a "No older log output" toast when history is
  exhausted. `--tail` can take a moment on very large log files.
- Stats are only sampled while the Stats tab is visible, keep at most 120 samples
  (about four minutes at 2 s) and are not persisted. Offline/stopped containers
  show a notice instead of graphs.
- The terminal resizes with the details pane (the grid is measured each frame and
  the pty is resized with `TIOCSWINSZ`, so full-screen programs reflow) and
  keeps 2000 lines of scrollback. It supports mouse-drag selection, double-click
  for a word and triple-click for a line, `Ctrl+Shift+C` to copy and
  `Ctrl+Shift+V` to paste (bracketed when the program enables it), the cursor
  shapes programs request with `DECSCUSR`, dim/italic/underline/bold/inverse
  text, `OSC 0/2` window titles and mouse reporting; hyperlinks (`OSC 8`) are
  not supported because the `vt100` crate does not expose them, and the shell
  is always `/bin/sh`. While it has focus, keys (including app shortcuts such as
  `Ctrl+Q`) go to the shell; click elsewhere to use shortcuts. `podman exec`
  requires a running container. If the app is hard-killed while a terminal is
  open, the exec client dies with it, but podman can leave the exec conmon and
  an idle shell inside the container until it is restarted (exec sessions have
  no reap API). Grid sizing measures the real advance of the chosen monospace
  font (the first installed family from a preference list such as JetBrains
  Mono, DejaVu Sans Mono or Liberation Mono), so the terminal always fills the
  pane exactly at any width and font.
- The event stream depends on `podman events`; when it is unavailable the app
  silently falls back to polling, and it reconnects every 3 s after a failure.
  Health-check and exec events are ignored, so health status text may lag until
  the next poll or unrelated event.
- Compose actions need a working `podman compose` provider (Compose v2 via
  `docker-compose`, or `podman-compose`). Projects are only detected through the
  standard compose labels, so containers started by hand are not grouped.
- `Down` stops and removes the project's containers and networks (not volumes or
  images). Stop time depends on how the containers handle SIGTERM.
- No compose project grouping beyond the label-derived view and no `compose.yml`
  editing; `up`/`down`/`restart`/`build` and read-only service logs are
  supported, but there are no per-service start/stop actions.
- The image file browser has no `podman image mount` (unavailable rootless), so it
  creates a throwaway container and streams `podman export` through `tar`. The
  directory listing is cached per image, but opening each file re-reads the
  stream, which is slow for multi-gigabyte images. Only the first 128 KiB of a
  file is read, and there is no download/save action.
- No Kubernetes or extension system yet.
- Machines can be created, removed and SSH'd into; `podman machine init`
  downloads a multi-gigabyte VM image and streams through the output panel, so it
  needs a working VM provider and disk space. Start/stop is a blocking task with
  a toast (no streamed output). Machines only exist where `podman machine` is
  supported.
- Push and registry login rely on Podman's credential store and whatever auth the
  registry requires; there is no registry management page, no insecure-registry
  configuration beyond the automatic loopback `--tls-verify=false`, and no
  multi-arch manifest push. Passwords are masked in the dialog and passed via
  stdin, but stay in memory until the command finishes.
- Running an image always detaches (`podman run -d`); there is no interactive
  run or attach mode. Restart policies, resource limits, networks, user and
  privileged options are not exposed yet.
- Builds default to the Containerfile's directory as context and accept an
  optional context, multiple `--platform` values and per-row `--build-arg`
  entries. Registry validation only checks the registries named in `FROM` lines
  against Podman's auth files and a list of public registries; it does not probe
  the registries. There is no cache control, target selection or secrets support
  yet. The Containerfile and any explicit context must exist on the machine
  running the app.
- Remote engines must already be configured in Podman (SSH keys/agent, socket
  path), or added from the Connections tab. The connection form covers `ssh://`
  and `unix://` only (no `tcp://` TLS options), and reachability results are
  in-memory and only refreshed on demand. SSH password and key-passphrase
  prompts cannot be answered because commands run without a TTY; load the key
  into the ssh-agent (or use an identity file without a passphrase). Compose
  against a remote connection depends on the compose provider. Switching a
  connection is local to this app and never runs
  `podman system connection default`.
- No multi-select, bulk actions or command palette yet. Sort is per tab, resets
  on tab switch, and is not persisted.
- The window **position** is not persisted (only its size and maximized state):
  GPUI exposes no reliable window origin on X11, and window managers apply
  requested positions inconsistently. Placement is left to the window manager.
- No native application menu bar: GPUI only implements one on macOS, so there is
  no Linux/Windows menu (actions live in the sidebar, context menus and tray).
