# Roadmap — Selkie

Status as of 2026-11-05. This is the durable plan for `selkie/`.
It records what is done, the architectural decisions behind it, and the
recommended next steps. Update the status markers as work lands.

Milestone A, milestone B (B1 libpod REST deliberately deferred, B2–B8 done) and
the stats / build / exec parts of milestone C are complete. Remaining work is
the quick-wins list and packaging.

## Product intent

A small, fast, read-write desktop UI for the **local** Podman engine. Less
surface than Podman Desktop, no extension platform, no Kubernetes. Optimize for:

1. Quick daily container triage (what is running, why did it fail, get the logs).
2. A small, understandable codebase (single GPUI entity, no framework layer).
3. Works with a stock Podman install; prefer the libpod socket when available,
   keep the CLI as a fallback.

## Current state

### Done (Docker engine)

- [x] Select Podman or Docker under `Settings → Engine`; the choice persists
- [x] Docker snapshot, dashboard, inspect, stats and container/volume/network/image
      actions through the Docker HTTP API (`bollard`)
- [x] Docker events stream and registry accounts from `~/.docker/config.json`
- [x] Capability flags hide Pods, Secrets, Machines and Connections (and the Run
      dialog's Secrets field) when Docker is selected
- [x] Streaming/interactive surfaces (logs, terminal, compose, build, pull/push,
      login) use the Docker CLI

### Done (Milestone A)

- [x] Container list: name, image, state, ports, status, ID
- [x] Image list: name, digest, ID, size, in-use count, created
- [x] Search/filter per tab (case-insensitive; clear button; Escape clears)
- [x] Selection + details pane (container inspect: command, mounts, env, networks,
      PID, exit code, created; image summary)
- [x] Container actions: start, stop, restart, delete (confirmation + force)
- [x] Streaming logs: follow, clear, copy, 800-line cap, ANSI stripping,
      auto-scroll
- [x] Image actions: pull, tag, delete
- [x] Image-reference validation before invoking Podman
- [x] Toast notifications for action results/errors
- [x] Responsive table columns when the details pane is open
- [x] Log child process dies with the app (`PR_SET_PDEATHSIG`, Linux)

### Done (Milestone B3)

- [x] Hover actions on container rows: Start / Stop / Restart / Delete
- [x] Hover actions on image rows: Tag / Delete
- [x] Right-click context menu: lifecycle actions, Tag/Delete, Copy ID/name
- [x] Keyboard navigation: `j`/`k` and arrows, auto-scroll into view
- [x] `Enter` selects the first row when nothing is selected
- [x] `/` focuses search; `Esc` closes menu/details then clears search
- [x] `Ctrl/Cmd+R` refreshes
- [x] Initial keyboard focus and click-to-focus handling for the list
- [x] Key propagation fixed so Escape/Enter handled by search/modals does not
      also trigger the root shortcuts

### Done (Milestone B4)

- [x] Pods tab: list, search, details, start/stop/restart/delete,
      hover actions and context menu
- [x] Volumes tab: list, search, details, delete, context menu (incl. copy
      mountpoint)
- [x] Networks tab: list, search, details (subnets + connected containers),
      delete, context menu
- [x] Sidebar counts and per-tab empty/search states
- [x] Keyboard navigation and details loading generalized across all tabs

### Done (Milestone B6 + module split)

- [x] Themes: dark, light, high contrast (`ui/theme.rs`); every color in the UI
      resolves through `theme()` — no literal color constants remain
- [x] Persisted settings (`ui/settings.rs`): theme, active tab, refresh interval,
      log window; stored at `$XDG_CONFIG_HOME|~/.config/selkie/settings.json`
      with per-field validation, saved on every change
- [x] Sidebar settings rows: theme cycle, auto-refresh cycle (3/5/10/30 s)
- [x] Log window cycle in the log toolbar (5m/10m/30m/1h) feeding
      `podman logs --since`; changing it restarts the stream
- [x] `src/ui.rs` (4464 → ~1160 lines) split by concern into `ui/sidebar.rs`,
      `ui/table.rs`, `ui/rows.rs`, `ui/details.rs`, `ui/dialogs.rs`,
      `ui/helpers.rs`, plus `ui/theme.rs` and `ui/settings.rs`. Shared state and
      polling stay in `ui.rs`; cross-module methods are `pub(super)`

### Done (Milestone B7)

- [x] Single instance: exclusive `flock` in `$XDG_RUNTIME_DIR` (temp-dir fallback)
      plus a Unix socket; a second launch sends `activate`, exits in a few
      milliseconds, and the running instance raises its window
- [x] Tray icon: freedesktop StatusNotifierItem over D-Bus via `ksni`
      (async-io backend, no GTK/libdbus C libraries); runtime-generated ARGB
      icon, menu with Show window / Refresh / Quit, click to activate
- [x] Quit semantics: `Ctrl/Cmd+Q`, tray Quit, and closing the window all quit;
      log children are still reaped by `PR_SET_PDEATHSIG`
- [x] Window size and maximized state persisted (watched every 500 ms and on
      close, written only on change) and restored on startup
- [x] Tray and second-launch signals reach the UI through channels polled by a
      single GPUI task that also owns the window-size watcher
- [x] Native menu investigated and deferred: GPUI's `set_menus` is a no-op on
      Linux and Windows (macOS only), so there is no native menu bar to hook into
- [ ] Window **position** persistence — dropped after testing: GPUI reports
      frame-relative origins on X11 (and ignores them on resize), its raw window
      handle panics, and window managers applied `_NET_MOVERESIZE_WINDOW` with
      inconsistent gravity offsets, so placement stays with the window manager

### Done (Milestone B5)

- [x] Compose projects derived from container labels, understanding both
      `com.docker.compose.*` (Compose v2) and `io.podman.compose.*`
      (podman-compose); project name, service, compose file and working directory
- [x] Compose tab: project name, running/total status (green/yellow/red), service
      count, compose file; search matches project, service, file and directory
- [x] Details pane: status, container/service counts, compose files, working
      directory and per-service running counts; Up/Down actions
- [x] `Up` = `podman compose … up -d`, `Down` = `podman compose … down` behind a
      confirmation modal; both stream stdout/stderr into an output panel with
      auto-scroll, Clear/Close, status line and success/failure toast
- [x] Output panel survives the project disappearing (Down removes the
      containers) and is capped at 2000 lines with ANSI stripping
- [x] Container rows show `project/service`, and container details gain a COMPOSE
      section; hover actions and context menu (Up / Down / Copy project name)
- [x] Compose child process dies with the app (`PR_SET_PDEATHSIG`), and only one
      compose action may run at a time

### Done (Milestone B8)

- [x] Machines tab: `podman machine list` (name, state, Podman default, driver,
      CPUs, memory, disk, last up) with search, details and hover/context actions
- [x] Machine Start / Stop (Stop behind a confirmation modal); the local lists and
      engine snapshot refresh afterwards
- [x] Connections tab: `podman system connection list` (name, URI, identity,
      Podman default, in use), search, details, context menu (Use / Use Podman
      default / Copy URI)
- [x] Engine switching: every engine command targets the selected connection with
      `--connection <name>`; the choice persists in settings and is never written
      to the user's Podman configuration. "Use Podman default" reverts
- [x] Machines/connections are fetched as a separate `LocalState` from local
      Podman config, so the Connections tab still works when a remote engine is
      unreachable; switching clears the stale engine snapshot
- [x] Sidebar gained an ENGINES section and the engine card shows
      "local default" / "via <name>" plus offline state
- [x] A connection removed from the Podman configuration is dropped from settings
      automatically at startup

### Done (Milestone C — Stats)

- [x] Stats tab in the container details pane, next to Summary and Logs
- [x] Sampling via `podman stats --no-stream --interval 1 --format json` every
      2 s, parsed into CPU %, memory used/limit, network and block I/O counters
      and PIDs (SI/IEC size strings such as `1.712MB` are understood)
- [x] Bar sparklines for CPU (0-100% scale), memory (scaled to the visible peak)
      and network (bytes/s derived from cumulative counters), with current values
      and totals/PIDs below
- [x] History kept in memory (120 samples ≈ 4 min), preserved across tab
      switches, cleared on container change or Clear
- [x] Sampling stops when the tab or selection changes; stopped containers show
      a notice and resume automatically once running

### Done (Milestone C — Build images)

- [x] "Build image" in the Images toolbar opens a dialog with Containerfile
      path and image name; **More options** adds the context directory, a
      key/value build-argument table, platform cards (amd64, arm64, ppc64le,
      s390x, wasi, new platforms) and registry validation. Tab switches fields,
      Enter builds, Esc cancels
- [x] Validation: tag checked with the image-reference rules, Containerfile must
      exist locally (missing path/field shows a toast and keeps the modal open)
- [x] Runs `podman build -f <Containerfile> -t <tag> --platform <a>
      --build-arg <key=value> <context>`, or `--manifest <tag>` instead of `-t`
      when several platforms are selected so Podman stores a manifest list;
      the Containerfile's directory is the default context
- [x] Before building, optionally checks that the registries in the
      Containerfile's `FROM` lines are public or have credentials in Podman's
      auth files
- [x] Output streams into the shared output panel (stdout + stderr, ANSI
      stripped, 2000-line cap, auto-scroll, Clear/Close, running/finished state)
- [x] Success/failure toast, image list refresh on completion, and compose/build
      mutually exclusive while one is running
- [x] Compose streaming generalized into `src/ui/output.rs` so both actions share
      one panel and runner; build child dies with the app (`PR_SET_PDEATHSIG`)

### Done (Milestone C — Exec terminal)

- [x] Terminal tab in the container details pane: PTY allocated with `openpty`,
      `podman exec -it -e TERM=xterm-256color <id> /bin/sh` attached to the slave,
      fixed 50×24 grid
- [x] `vt100` terminal emulation rendered as style-grouped monospace runs with
      ANSI/256/truecolor support, bold/inverse, cursor block and dark/light theme
      defaults
- [x] Input: printable characters, Enter/Tab/Backspace/Escape, arrows,
      Home/End/Delete/PageUp/PageDown, Ctrl+letters and Alt; keys are captured
      while the terminal is focused
- [x] Session lifecycle: starts when the tab is first opened, kept while
      switching Summary/Logs/Stats, restarted explicitly, and closed when the
      container selection changes; stopped containers show a notice
- [x] Exit/close handling: "session ended" state, child reaped (no zombies), and
      the PTY is closed before the client is killed so no stale conmon is left
- [x] `Restart` button re-opens a shell; the exec client dies with the app on
      **hard kills too** (`PR_SET_PDEATHSIG=SIGTERM` set in `pre_exec` next to
      `setsid`, with a `getppid` check for the fork/prctl race). An abrupt kill can
      still leave the podman-side exec conmon and its idle `/bin/sh` inside the
      container, because podman has no API to reap exec sessions; the graceful
      quit path (PTY closed first) remains fully clean. The 2026-11-05
      investigation found nine leaked exec conmons from earlier hard kills and
      cleaned them; all future sessions get the parent-death signal

- [x] The grid picks the first installed monospace family from a preference
      list (JetBrains Mono / JetBrainsMono Nerd Font Mono / DejaVu Sans Mono /
      Liberation Mono / …), measures that font's real advance with
      `TextSystem::advance` once, and derives the column count from it, so the
      pty width matches the drawn text exactly at every pane width; the same
      family is used for the runs, so no glyph advances are guessed
- [x] Full cell styling: bold, dim (blended toward the background), italic,
      underline, inverse and explicit fg/bg, merged into one run per style; any
      cell whose background is not the default uses a non-breaking space so
      blank highlighted cells still paint
- [x] Text selection with the mouse: drag, double-click for a word and
      triple-click for a line, rendered as an `accent_soft` background over the
      cell range; `Ctrl+Shift+C` copies the range through
      `Screen::contents_between` with trailing blanks trimmed per line, and
      scrolling or a click without a drag clears it
- [x] `Ctrl+Shift+V` pastes the clipboard, converting newlines to carriage
      returns, or wrapping the text in `ESC[200~`/`ESC[201~` when the program
      enabled bracketed paste (`DECSET 2004`); pasted `ESC[201~` is stripped so
      text cannot close the bracket itself
- [x] Mouse reporting when the program asks for it (`DECSET 9/1000/1002/1003`):
      press/release/motion and wheel are encoded as SGR (`ESC[<b;x;yM/m`), the
      legacy X10 form or the UTF-8 form, with shift/alt/ctrl modifiers; wheel
      events go to the program instead of the scrollback and one event is sent
      per notch
- [x] Cursor shapes from `DECSCUSR`: steady/blinking block (reverse video),
      underline and vertical bar (a 2 px rectangle laid over the cell), with
      blinking styles driven by the existing 2 Hz desktop tick so idle CPU stays
      flat
- [x] `OSC 0/2` window titles are captured through the emulator's callbacks and
      shown in the pane header (falling back to the launched command)

### Done (Milestone B2 — event-driven updates)

- [x] A long-lived `podman events --format json` child (`src/ui/events.rs`) replaces
      3-second polling as the primary update path; events are read line by line
      over a channel and trigger a snapshot refresh
- [x] Bursts are coalesced: one refresh 300 ms after the last event, with a hard
      1 s upper bound while events keep arriving (a `start` emits init/start in
      quick succession)
- [x] Health-check and exec events (`health_status`, `exec`, `exec_die`) are
      ignored, since they do not change the snapshot but can be frequent
- [x] The supervisor restarts the stream 3 s after it exits or fails to start,
      and reports connection state to the UI
- [x] Polling remains as a fallback: the periodic timer skips when an event has
      refreshed within the configured interval
- [x] A refresh requested while a fetch is running is remembered and re-run
      afterwards instead of being dropped
- [x] Status bar shows `live` while the event stream is connected and `auto Ns`
      otherwise; switching connections stops and restarts the stream (every
      command goes through `podman::base_command()`)
- [x] The events client dies with the app/supervisor task (`EventChild` guard,
      plus `PR_SET_PDEATHSIG`)

### Done (quick wins)

- [x] Pause / Resume containers: `podman pause`/`unpause` in the row hover
      actions, action bar and context menu; the paused state gets its own pill
      color and the action bar swaps Start for Resume
- [x] Rename container: `podman rename` behind a validated modal (letters,
      digits, `-`, `_`, `.`; no leading `-`), available from the context menu
- [x] Sortable tables: click any column header to sort, click again to reverse
      (accent header with ▲/▼); numeric columns (size, in-use, mounts, CPUs,
      memory, disk, services) sort numerically, text columns case-insensitively;
      sort is per tab and resets on tab switch
- [x] Raw inspect JSON viewer: a `JSON` toggle in the details header for
      containers, images, pods, volumes and networks renders the pretty-printed
      `podman … inspect` output with Copy; fetched lazily and cached until the
      selection changes
- [x] Pull progress: pulls now stream through the shared output panel like
      builds and compose actions, showing `podman pull` output and errors
- [x] Details header text now truncates properly, so long image names/digests no
      longer overlap the JSON and close buttons (which also fixed a hit-testing
      dead zone over those buttons)

### Done (logs — "load older")

- [x] **Load older** button in the log toolbar fetches history with a one-shot
      `podman logs --tail <current + 500> --timestamps` (no follow), so the
      request grows with the buffer and repeated clicks reach further back
- [x] Only lines older than the oldest buffered line are merged (compared by
      the `--timestamps` prefix, falling back to exact-line dedupe), so the
      live stream and the one-shot fetch cannot duplicate each other
- [x] The viewport is anchored on the line that was at the top before the fetch
      (`ScrollHandle::top_item` + `scroll_to_top_of_item`), so loading does not
      jump the view; Follow is switched off to keep live lines from scrolling
- [x] Buffer hard cap of 3000 lines once history is loaded (live-only cap stays
      800); reaching the end shows "No older log output" instead of looping
- [x] The fetch runs on the background executor with a "Loading…" label,
      streams its error message into a toast on failure, and its child dies with
      the app (`PR_SET_PDEATHSIG`)
- [x] Clear resets the buffer, the cap and the loading state

### Done (resizable panels)

- [x] Draggable splitter between the sidebar and the content, and between the
      list and the details pane (`src/ui/resizer.rs`); a 6 px hit area draws a
      1 px separator that highlights on hover/drag
- [x] Widths live in `Settings` (`sidebar_width`, `details_width`), are saved on
      mouse release and double-click resets them to 224 / 400 px
- [x] Limits keep the layout usable: sidebar 160–400 px, details 320–900 px, and
      both leave at least 320 px for the list; the effective widths are clamped
      to the current window each frame, so shrinking the window cannot overflow
      (stored values return when it grows again)
- [x] Invalid or out-of-range values in `settings.json` are repaired on load
      (non-finite → default, otherwise clamped); unit tested
- [x] Tables, details content, logs, stats and the output panel reflow; the
      terminal keeps its fixed 50×24 grid and clips in a narrow pane

### Done (build options + text ellipsis)

- [x] Build modal gained **context**, **platform** and **build args** on top of
      Containerfile and tag; Tab and clicking move between all five fields
- [x] `--platform <os/arch[/variant]>` and repeated `--build-arg KEY=VALUE` are
      passed before the context; the context defaults to the Containerfile's
      directory and an explicit one must be an existing directory
- [x] Validation runs before the modal closes (tag, Containerfile, context,
      platform, args) so mistakes keep the form open with an error toast;
      validators and the full command are unit tested
- [x] Table cells and the details header ellipsize long values using a
      character budget derived from the column width (GPUI 0.2's `truncate()`
      clips without emitting an ellipsis here), so narrow/resized columns end in
      `…` instead of a hard cut

### Done (run container from image)

- [x] Run action on image rows, the image context menu and the image details
      action bar opens a form for name, ports, environment, volumes and a command
      override
- [x] `podman run -d` with optional `--name`, repeated `-p` / `-e` / `-v` and the
      command after the image; empty name lets Podman generate one
- [x] Inputs are validated before the modal closes (container name,
      `host:container[/proto]` mappings, `KEY=VALUE` env, `source:destination`
      mounts), with unit-tested parsers and command construction
- [x] The started container appears through the normal refresh/event path and is
      detached, so it can be opened in the Terminal tab
- [x] Fixed `run_command` appending its display arguments to a prebuilt command
      (found in testing: the container ran `sleep 120 run <image>`); `execute` now
      runs the command as built and takes a display string only for errors, with
      a regression test

### Done (image push + registry auth)

- [x] **Push** action on image rows, the image context menu and the image details
      action bar; the destination field is prefilled with the image name and
      validated, and output streams into the shared panel
- [x] **Login**/**Logout** buttons in the Images toolbar; the login form takes
      registry, username and a masked password, and pipes the password to
      `podman login --password-stdin` (never an argument)
- [x] Logout accepts a registry or logs out of everything when the field is empty
- [x] Loopback registries (`localhost`, `127.0.0.1`, `[::1]`) automatically use
      `--tls-verify=false` for push/login/logout, so a local dev registry works
      without extra configuration
- [x] Fixed duplicate image rows: `podman images --format json` emits one entry
      per repo tag for multi-tagged images, so `parse_images` now deduplicates by
      image ID (regression test added)
- [x] `start_output` can feed stdin and takes an `OutputSpec`, keeping the
      streamed-command signature under the clippy threshold

### Done (create & prune resources, kube play)

- [x] **Create volume** and **Create network** in their toolbars; the volume form
      takes a name plus an optional driver, the network form a name plus optional
      driver, IPv4 subnet (CIDR) and IPv4 gateway
- [x] **Create pod** (name + optional `host:container` port mappings) and **Play
      kube** (Kubernetes YAML path) in the Pods toolbar; `kube play` streams into
      the shared output panel and reports a non-zero exit as an error
- [x] **Prune** for volumes and networks, both behind a danger-styled confirmation
      dialog (Podman keeps in-use volumes/networks)
- [x] Inputs are validated before the modal closes (names, driver, subnet,
      gateway, port mappings, YAML path must exist) with unit-tested command
      construction; invalid input keeps the modal open with a toast
- [x] Command builders (`volume_create_command`, `network_create_command`,
      `pod_create_command`, `kube_play_command`) run through `execute`, never a
      prebuilt command plus arguments

### Done (machine lifecycle: init / rm / ssh)

- [x] **Create machine** in the Machines toolbar: optional name (empty keeps
      Podman's `podman-machine-default`), CPUs, memory (MiB) and disk size (GiB),
      echoed and streamed through the shared output panel because the VM image
      download can take minutes
- [x] **Delete** from the machine action bar and context menu, behind a danger
      confirmation that spells out the loss of the VM's containers and volumes
      (`podman machine rm -f`)
- [x] **Terminal** on a running machine opens `podman machine ssh` in the details
      pane; the PTY session code is shared with the container exec terminal and
      the title shows which command is attached
- [x] Machine fields are validated before the modal closes (name characters,
      `--cpus` 1-128, `--memory` 512-262144 MiB, `--disk-size` 1-4096 GiB) with
      unit-tested command construction; invalid input keeps the modal open
- [x] **Text caret** in the search box and every modal field (blinking 2 px accent
      bar, positioned before a placeholder and after the text), plus a visible
      block cursor at the shell prompt in container and machine terminals
- [x] Machine commands stay local: they ignore the selected engine connection
      and are validated before they reach the CLI
- [x] Fixed machine-list parsing: `podman machine ls --format json` reports
      `Memory` and `DiskSize` as JSON **strings** while `CPUs` is a number, so the
      whole list failed to deserialize and the app showed “0 machines” (silently,
      through `unwrap_or_default`) as soon as any machine existed. Sizes now accept
      both forms, and the zero `LastUp` timestamp shows `-` instead of
      `0001-01-01 00:00`; regression test added with the real JSON payload

### Done (compose restart / build / service logs)

- [x] **Restart** and **Build** for a compose project, in the action bar and the
      context menu; both stream through the shared output panel like Up/Down
- [x] **Logs** tab for compose projects: `podman compose logs -f --tail 200`
      streams into the same log viewer as container logs, with chips for
      "All services" and each service
- [x] Compose logs use `--tail` rather than `--since` because both the Docker
      Compose v2 provider and `podman-compose` accept it; the window and
      "Load older" controls are hidden for compose (they only apply to
      container log files)
- [x] The log stream is now target-aware (`LogTarget::Container` vs
      `LogTarget::Compose`), so the Logs tab, Follow/Clear/Copy and restarts on
      settings changes work for both
- [x] Compose commands set `PODMAN_COMPOSE_WARNING_LOGS=false`, so Podman's
      "Executing external compose provider" banner no longer pollutes the output
      panel or the log view

### Done (dashboard)

- [x] **Dashboard** tab with resource cards (containers running/total, images,
      pods, volumes, networks, machines); the counts come from the normal snapshot,
      so they follow live events without extra processes
- [x] **Disk usage** from `podman system df` (images, containers, volumes: size
      and reclaimable share with a bar)
- [x] **Engine details** from `podman info`: version, OS/arch, host, kernel, CPUs,
      host memory use, OCI runtime (short form, e.g. `crun 1.21`), network backend,
      storage driver, event log, rootless/rootful, active connection, socket path
      with presence, and graph root
- [x] The dashboard data is fetched only while the tab is visible and cached for
      30 s, because `podman info` takes ~2 s on this host; tab entry, the Refresh
      button and `Ctrl+R` bypass the cache, and switching connections invalidates it
- [x] `Tab::Dashboard` renders instead of the table/toolbar, `/` search is ignored
      there, and the sidebar shows the new entry without a count

### Done (image history and file browser)

- [x] **History** tab on images: `podman history --format json` rendered as layers
      with short ID (base layer shows `-`), size, timestamp, created-by and comment
- [x] **Files** tab: directory browser over the image filesystem — folders first,
      sizes, symlink targets, `Up`/`Back` navigation and a breadcrumb
- [x] Text files preview inline (first 128 KiB, with a truncation note); binary
      files report their real size instead of dumping bytes
- [x] `podman image mount` is unavailable rootless, so the listing uses a
      throwaway container plus `podman export | tar -tvf -`, and file reads use
      `tar -xOf - -- <path>`; the temporary container is removed on every path
      (including errors) and the listing is cached per image
- [x] GNU tar has no `--fast-read`; an early tar exit left `podman export`
      holding the storage lock (found in testing, blocking other podman
      commands). The reader now consumes the whole stream and kills the exporter
      if tar fails.

### Done (close to tray)

- [x] The window's close request is vetoed and the X11 window is unmapped instead
      (GPUI has no hide API and a closed window is gone for good), so the process,
      the tray icon and all in-memory state survive with **0 CPU** while docked
- [x] The tray's **Show window** and a second launch map the window again and
      activate it; `Ctrl/Cmd+Q` and the tray's **Quit** still end the process
      (quit does not go through the close handler)
- [x] Without an X display to unmap (native Wayland) there is nothing to dock, so
      closing falls through to `cx.quit()`

### Done (Accounts & Settings pages)

- [x] **Settings** is a sub-navigation plus a pane, like the upstream app:
      `Registries`, `Appearance`, `Engine`, `About`. The selected pane is stored
      in `settings.json` (`settings_section`) and restored on launch
- [x] **Registries** pane (also the whole **Accounts** tab, from one shared
      builder): a table of every credential Podman would use, read from
      `REGISTRY_AUTH_FILE`, the runtime `containers/auth.json`, the config one and
      `~/.docker/config.json`, merged and sorted by registry. Columns are
      registry location, username (base64-decoded from `auth`) and a masked
      `••••••••` password — `token` for identity-token entries. Each row has a
      `⋮` menu (clamped to the window) with **Log out**, which goes through the
      confirmation dialog so a logout only ever runs `podman logout <registry>`.
      **Add registry** opens the login dialog with empty fields
- [x] Well-known **providers** underneath (Docker Hub, Red Hat Quay, GitHub,
      Google Container Registry) with their logo mark and a **Configure** button
      that opens the login dialog prefilled with that host and the Username field
      focused; the username is shown once the registry is configured
- [x] The credential files actually read are listed at the bottom, and the
      Accounts subtitle counts the logins
- [x] **Appearance** pane: theme pills, auto-refresh and log-window choices (the
      log stream restarts when the window changes) and panel-width resets
- [x] **Engine** pane: the current connection with a way back to Podman's
      default, plus machine/connection counts
- [x] **About** pane: app version, Podman version, settings path, renderer

### Done (connections: add, edit, delete + reachability)

- [x] **Add connection** in the Connections toolbar opens a form for name, URI
      and an optional identity file; it runs the local
      `podman system connection add [--identity FILE] NAME URI`. It never passes
      `--default`, though Podman may itself mark the first connection as its
      default
- [x] URI validation accepts `ssh://` and absolute `unix://` endpoints only;
      an identity file must exist, is rejected for `unix://` (Podman refuses it)
      and `~/` is expanded because the command is spawned without a shell;
      invalid input keeps the modal open with a toast
- [x] SSH connections with an empty identity use the running **ssh-agent**. If
      the app did not inherit `SSH_AUTH_SOCK` (common for desktop launches), the
      engine and probe commands are pointed at `$XDG_RUNTIME_DIR/openssh_agent`
      or `$XDG_RUNTIME_DIR/keyring/ssh` when a socket exists there. Interactive
      password/passphrase prompts cannot be answered because commands run
      without a TTY, and the dialog says so
- [x] **Edit…** (details action bar, row hover, context menu) reopens the form
      prefilled. Podman has no edit command, so a name change runs
      `system connection rename` first (keeping the default flag) and the
      URI/identity update re-runs `add` under the same name, which replaces them
      in place; clearing the identity removes it, and renaming into an existing
      name is rejected. When the edited connection is in use, the stored
      connection name follows the rename
- [x] **Delete…** opens a danger confirmation and runs
      `podman system connection remove NAME`; deleting the connection this app
      uses clears the engine state and falls back to Podman's default engine
      (the same guard as switching, so it is blocked while a build/compose runs)
- [x] The Connections table gets a reachability **STATUS** column and the
      details pane a **REACHABILITY** section with Podman's error text; rows,
      the context menu, the details action bar and a toolbar **Check all** probe
      with `podman --connection <name> info`
- [x] Probes run on the background executor, one task per connection, with a
      15 s cap so a dead SSH host cannot hang the UI; results are cached in
      memory for the session and re-run on demand. They start on the first visit
      to the tab and when a newly added connection appears
- [x] Explicit checks toast `reachable` / `unreachable: <error>`; automatic ones
      only update the column and details pane to avoid noise
- [x] The in-use connection keeps an accent dot in the row and “Podman default”
      moves to the subtitle, so STATUS no longer doubles as the in-use marker

### Done (packaging & desktop integration)

- [x] `packaging/selkie.svg`: scalable app icon matching the tray mark
      (Podman purple `#892ca0`, rounded square, white “P”), rendered to 16–512 px
      PNGs by `packaging/render-icons.sh` (rsvg-convert → Inkscape → ImageMagick)
- [x] `packaging/selkie.desktop`: `Exec`/`TryExec`, `Icon`, keywords,
      `StartupWMClass=selkie` and a single main category; passes
      `desktop-file-validate`
- [x] The window now sets `app_id`, which GPUI turns into `WM_CLASS` on X11
      (verified with `xprop`), so launchers associate the window with the entry
- [x] `packaging/install.sh`: user-level install into `$PREFIX/bin`,
      `$PREFIX/share/applications` and `hicolor/<size>/apps`, with `--uninstall`,
      `PREFIX`/`BINDIR`/`DATADIR`/`DESTDIR` overrides and desktop/icon cache
      refresh; works both in-tree and from an extracted archive
- [x] `packaging/build-release.sh`: builds the release binary and writes
      `dist/selkie-<version>-<target>.tar.gz` (binary, README, ROADMAP,
      LICENSE, desktop entry, icons, installer) plus a `.sha256`
- [x] `packaging/build-deb.sh`: builds
      `dist/selkie_<version>-1_<arch>.deb` with the stripped binary in
      `/usr/bin`, the desktop entry in `/usr/share/applications`, the seven icons
      in the hicolor theme, a gzipped man page and DEP-5 docs; computes
      `Depends` with `dpkg-shlibdeps` when available and adds the libraries the
      renderer `dlopen`s (`libvulkan1`, `libwayland-client0`) plus `podman`, with
      `mesa-vulkan-drivers` as a recommendation; maintainer scripts refresh the
      desktop/icon caches; `lintian` is clean
- [x] The window icon is set from the binary: a second, short-lived `x11rb`
      connection finds the window by `_NET_WM_PID` (the window manager reparents
      it under a frame, so the search walks the tree) and writes
      `_NET_WM_ICON` with 16/22/24/32/48/64/128 px renditions of the same runtime
      icon the tray uses, retrying until the property sticks; window managers
      that draw the icon themselves (Cinnamon/Muffin, XFCE, KWin) no longer
      fall back to a generic one. The drawing code now scales to any size with
      4×4 supersampling, and the glyph scale rounds to whole pixels so 16 px
      stays bold

### Done (terminal: resize, scrollback, cursor keys)

- [x] **Resize**: the grid measures itself during prepaint and hands the size to
      the session, which calls `Parser::set_size` and `TIOCSWINSZ`, so the pty and
      full-screen programs follow the pane (`stty size` reported 28×61 before a
      splitter drag and 28×84 after it)
- [x] **Scrollback**: 2000 lines kept by the emulator; mouse wheel scrolls three
      rows per notch, the header shows `N lines back` plus a **Live** button, and
      the block cursor is hidden while scrolled back
- [x] **Application cursor keys**: `DECCKM` is honoured, so arrows/Home/End send
      `ESC O x` in application mode and `ESC [ x` otherwise (`cat -v` showed
      `^[OA` vs `^[[A`), which fixes arrows in full-screen programs
- [x] Clicking the terminal header (`Live`/`Restart`) returns focus to the grid, so
      typing keeps working afterwards

### Current architecture

| Concern | Implementation |
| --- | --- |
| Data access | Podman: CLI + JSON (`ps`, `images`, `pods`, `volume`, `network`, `inspect`, `logs`, `compose`). Docker: HTTP API via `bollard` (`src/podman/docker.rs`), with the Docker CLI for streaming/interactive commands |
| UI | One GPUI entity (`Selkie`) in `src/ui.rs`; rendering split by concern under `src/ui/` |
| Styling | `ui/theme.rs` palettes (dark / light / high contrast) via a thread-local current theme |
| Layout | Sidebar and details widths from settings, adjusted by draggable splitters (`ui/resizer.rs`) and clamped per frame to the window |
| Persistence | `ui/settings.rs` → JSON in the XDG config dir (engine, theme, tab, refresh, log window, panel widths, window size) |
| Desktop integration | `src/desktop.rs`: `flock` + Unix socket for single instance; `ksni` (D-Bus) tray; close-to-tray by unmapping the X11 window (map + activate from the tray or a second launch) |
| Engines | `src/podman/engine.rs` selects Podman (CLI) or Docker (API) and exposes capability flags; Podman machines/connections are parsed into a separate `LocalState`, and the selected connection is injected as `--connection` |
| Machines | `podman machine init` (streamed), `machine rm -f` behind a confirmation, and `machine ssh` in the shared PTY terminal; machine commands are local and ignore `--connection` |
| Lifecycle | Close handler + `Ctrl/Cmd+Q` + tray Quit; window size watched every 500 ms |
| Updates | `podman events` stream (`src/ui/events.rs`) refreshes the snapshot; polling task with a configurable interval (3/5/10/30 s) remains as fallback |
| Actions | `std::process::Command` (Podman) or `bollard` (Docker) on a background executor, then refresh |
| Creating | `podman run -d` from an image with validated name, ports, env, volumes and command (`src/podman.rs`) |
| Creating resources | `podman volume create` / `network create` / `pod create` with validated optional flags; `podman kube play` streams through the shared output panel |
| Pruning | `podman volume prune -f` / `network prune -f` behind a confirmation dialog |
| Inspection | Per-resource `inspect` summaries; raw JSON viewer fetches and pretty-prints `podman … inspect` on demand |
| Image history | `podman history --format json` parsed into layers (ID, size, created, created-by, comment) |
| Image files | `podman create` (throwaway) → `podman export` piped into `tar -tvf` for the listing and `tar -xOf - -- <path>` for file content; listing cached per image, container removed afterwards |
| Logs | `podman logs -f --since <window> --timestamps` for containers and `podman compose logs -f --tail 200 [service]` for projects, both through one reader-thread → mpsc → UI task pipeline |
| Compose | Projects grouped from `ps` labels; `podman compose up -d` / `restart` / `build` / `down` stream through the shared panel, the Logs tab streams from the provider |
| Long-running commands | `src/ui/output.rs` + `start_output`: compose up/down, image builds and pulls stream stdout/stderr into one panel, one at a time |
| Registries | `podman push` with a validated destination; `podman login --password-stdin` / `podman logout` with credentials kept out of argv; loopback registries skip TLS verification |
| Accounts | Registry logins parsed from Podman's auth files (`REGISTRY_AUTH_FILE`, runtime/config `auth.json`, Docker config); username and a masked password (or `token`), a `⋮` menu with Log out, Add registry, and Configure buttons for Docker Hub/Quay/GitHub/GCR |
| Connections | `podman system connection list` read locally; **Add**/**Edit** run `podman system connection add [--identity FILE] NAME URI` (ssh:// or absolute unix://, `~/` expanded, no `--default`; edits rename first and then replace URI/identity in place) and **Delete** runs `system connection remove`; reachability probes run `podman --connection NAME info` in background tasks (15 s cap) and feed the STATUS column, details pane and explicit-check toasts; every engine command gets `--connection NAME` |
| Settings page | Sub-navigation with Registries, Appearance (theme, refresh, log window, panel resets), Engine and About; the selected pane is persisted |
| Builds | `podman build -f <Containerfile> (-t <tag> | --manifest <tag>) [--platform …] [--build-arg …] <context>` (manifest lists for multi-platform); context defaults to the Containerfile directory, all inputs validated, shared output panel |
| Terminal | `openpty` + `podman exec -it` (containers) or `podman machine ssh` (VMs) + `vt100` emulator; pane measured per frame, column count derived from the chosen monospace font's real advance (`TextSystem::advance`) → `Parser::set_size` + `TIOCSWINSZ`, 2000-line scrollback, SS3 cursor keys in DECCKM mode, `OSC 0/2` titles, `DECSCUSR` cursor shapes, mouse-drag/word/line selection with copy and bracketed paste, mouse reporting in SGR/legacy/UTF-8 encodings |
| Stats | Podman `stats --no-stream` or Docker `/stats?stream=false`, polled every 2 s while the Stats tab is open; 120 in-memory samples rendered as bar sparklines |
| Dashboard | Counts from the snapshot; `podman info` + `podman system df` or Docker `info` + `system df` fetched only while the tab is visible and cached for 30 s (info probes the store and takes seconds) |
| Packaging | `packaging/` holds the SVG/PNG icons, desktop entry, `install.sh` (user-level, `DESTDIR`-capable, `--uninstall`) and `build-release.sh` (versioned tarball + sha256); `app_id` sets `WM_CLASS`, and the app writes `_NET_WM_ICON` itself (16–128 px, drawn at runtime) |
| Selection | Stored by ID; cleared when the item disappears from the snapshot |
| Errors | Last good snapshot kept; error shown in status bar + toasts |

### Environment notes

- `libc` is pinned to `=0.2.189`: `xattr 0.2.3` (via `gpui_http_client`) still
  references `ENOATTR`, removed in `libc 0.2.191`. Revisit when `xattr` is fixed.
- `build.rs` supplies the unversioned `libxkbcommon-x11.so` link when only the
  runtime package is installed.
- `--tail N` on large Podman log files can take seconds to emit; `--since` is
  used for the live stream and `--tail` only runs on demand for "Load older",
  where the delay is user-initiated. Keep this in mind before making the live
  stream use `--tail`.

## Roadmap

### Milestone B — make it a proper desktop app

Priorities, not a strict order. S/M/L are rough effort sizes; see
[Recommended next step](#recommended-next-step).

1. **libpod REST migration (M) — deferred (decision 2026-11-05, re-confirmed
   2026-11-05)**
   - Decision: stay CLI-first. The CLI needs no socket, keeps the app simple,
     handles SSH connections via `--connection`, and covers the current feature
     set; `podman machine`, `podman system connection` and `podman compose` stay
     CLI-only regardless, so a library would add a second backend rather than
     replace one.
   - Revisit only when one of these triggers appears: snapshot refresh cost
     (6 processes per refresh on a chatty engine), stats process churn during
     long sessions, or a need for per-layer pull progress / structured build
     output. If that happens, start with a narrow, measured spike for
     `snapshot` + `stats` on the local engine (dedicated tokio thread, socket
     probe, CLI default and fallback) before designing any trait.
   - The CLI implementation must remain the default; any future REST work should
     sit behind a backend trait with the CLI as fallback.
   - Talk to `http://d/...` over the Unix socket
     (`$XDG_RUNTIME_DIR/podman/podman.sock`, rootful `/run/podman/podman.sock`,
     or a machine socket).
   - Remaining value: process-free stats streaming, multiplexed snapshot reads,
     per-layer pull progress. Exec, logs, events and pull already work well over
     the CLI.
   - Candidate crates (evaluated 2026-11):
     - `podman-api` 0.11 (vv9k) — libpod-native, async (tokio), Unix socket + TLS,
       containers/images/pods/volumes/networks/secrets/manifests/exec, streaming
       logs/stats/pull. Targets libpod API 4.5.1. **Recommended.**
     - `podman-rest-client` 0.13 (blazzy) — generated from the Podman v5 swagger;
       UDS + SSH; broad endpoint coverage, less ergonomic generated API. Use if v5
       fidelity matters more than ergonomics.
     - `podtender` 0.5 — smaller async Podman-native client (containers/images/
       pods/volumes/networks/logs).
     - `bollard` 0.21 — mature async Docker API client; works against Podman's
       Docker-compatible API. Best-in-class streaming, but no Podman pods and
       Docker semantics can differ.
     - `docker-api` — sync Docker API client, same Docker-compat caveat.
   - Runtime integration: these clients are tokio/hyper based while GPUI uses
     smol. Run the client on a dedicated tokio runtime thread and bridge results
     to the entity over channels (or a small backend actor).
   - Keep a `PodmanBackend` trait so the CLI implementation remains as fallback.
2. **Event-driven updates (M) — done (2026-11-05)** — long-lived `podman events`
   stream with 300 ms coalescing and 3 s reconnects; polling stays as a
   low-frequency fallback. The REST events endpoint is only relevant if REST
   lands later.
3. **Row-level actions + shortcuts (S/M) — done (2026-11-05)** — hover action
   buttons, right-click context menu, `j/k` navigation, `Enter`, `/`, `Esc`,
   `Ctrl/Cmd+R`.
4. **More resources (M each) — done (2026-11-05)** — Pods, Volumes and Networks
   tabs with their actions and details, reusing the table/details patterns.
5. **Compose awareness (L) — done (2026-11-05)** — projects grouped from
   container labels, Compose tab with details, `up -d`/`down` actions whose output
   is streamed into a panel, and compose info on containers.
6. **Settings + appearance (S/M) — done (2026-11-05)** — persisted theme, tab,
   refresh interval and log window; light and high-contrast themes.
7. **Native desktop behavior (M) — done (2026-11-05)** — tray icon
   (StatusNotifierItem via `ksni`), single-instance lock + activation socket,
   quit semantics, window size/maximized persistence. Native menu deferred (GPUI
   implements menus on macOS only); window position is left to the WM (see B7).
8. **Connections/machines (M/L) — done (2026-11-05)** — Machines and Connections
   tabs, start/stop machines, and app-scoped engine switching through
   `--connection` (no changes to the user's Podman config).

### Milestone C — platform features

- **Exec terminal (L) — done (2026-11-05)** — PTY-backed `/bin/sh` in the container
  details pane with full-screen terminal emulation.
- **Stats (M) — done (2026-11-05)** — live CPU/memory/network graphs in a Stats
  tab for the selected container.
- **Build images (M) — done (2026-11-05)** — Containerfile build with streamed
  output in a shared panel with compose actions.
- **Kubernetes (L)** — contexts, pods, apply/yaml — only if the product intent
  changes; deliberately out of favor today.
- **Extension API (XL)** — intentionally not planned until the core is stable.
- **Auto-update / packaging / telemetry (M)** — distribution concerns, not UI.

### Quick wins worth slotting earlier

- Build cache/target options (`--no-cache`, `--target`, cache mounts).
- Periodic re-probing of connections instead of on-demand checks, and `tcp://`
  connections with TLS options.

Done already: pause/resume, rename, sortable columns, raw inspect JSON viewer,
streamed pull progress, "load older" logs, resizable panels, build
context/platform/args and table ellipsis; configurable log window (B6) and
Copy ID/name (B3); adding, editing and deleting connections and checking their
reachability (2026-11-07).

## Recommended next step

REST is deferred (see B1), so continue on the CLI. The B-milestone items are now
all delivered. Recommended order:

1. **Quick wins** — done: pause/resume, rename, sortable columns, raw inspect
   JSON viewer, streamed pull progress, "load older" logs, resizable panels,
   build options and text ellipsis. Remaining candidate: build cache/target
   options.
2. **Parity backlog** — complete: run-from-image, push/login, create/prune,
   `kube play`, machine lifecycle, compose actions/logs, dashboard, image history,
   the file browser and packaging.
3. **Distribution** — the release tarball is reproducible locally; publishing it
   (CI, checksums, release notes) is the remaining step, plus a wider cleanup of
   the GPUI-version pin when upstream allows.

Revisit **B1 (REST)** only when pull progress or streaming efficiency become
priorities; keep the CLI as the fallback in that migration.

## Parity backlog (vs the Electron app in this repo)

Captured 2026-11-06 from `packages/renderer/src/App.svelte`, the container/image
`*Details.svelte` tabs and `packages/main/src/plugin/`. Ranked by daily
usefulness for a local-engine app:

1. **Run/create container from an image** — done (2026-11-06)
2. **Image push plus registry login/logout** — done (2026-11-06)
3. Create/prune volumes and networks; create pods and `kube play` — done
   (2026-11-06)
4. Machine `init` / `rm` / `ssh` — done (2026-11-06)
5. Compose restart/build/service logs — done (2026-11-06)
6. Dashboard (resource counts and host stats) — done (2026-11-06)
7. Image history and image file browser — done (2026-11-06)
8. Packaging and desktop integration — done (2026-11-06)
9. Podman secrets (list, create, delete, details/inspect, run mappings) — done
   (2026-11-07). Upstream added the Secrets page on 2026-07-08
   (`packages/renderer/src/lib/secrets/`); the 2026-11-06 capture missed it.

The parity backlog is now empty.

Explicitly not planned: Kubernetes resource management, the extension
API/catalog, additional VM runtimes (Lima), telemetry, auto-update and
i18n. See "Non-goals" above.

## Non-goals (for now)

- Extension system / plugin marketplace
- Kubernetes management
- Multi-user or multi-engine orchestration
- Cross-platform hardening beyond the developer's Linux machine. macOS/Windows
  need a different child-process cleanup strategy than `PR_SET_PDEATHSIG`.

## Open questions

- ~~Hybrid REST + CLI, or REST-only?~~ Decided 2026-11-05: CLI-first; REST
  deferred and, if adopted later, gated behind a backend trait with CLI fallback.
- ~~Should "open logs" be instant (`--since`, current) or complete (`--tail`,
  slow on huge files)?~~ Answered 2026-11-05: instant stream via `--since`, with
  an explicit "Load older" that runs `--tail` in the background.
- How much compose support is genuinely useful without becoming a compose UI?
- Is a terminal (`exec`) worth the PTY complexity before pods/volumes/networks?

## Known risks

- Podman's user socket is often not enabled by default on Linux; the fallback
  path must stay functional. A socket unit can also be active while the socket
  file is stale; verify the file (observed 2026-11-05) and restart the unit.
- `podman-api` targets libpod API 4.5.1. Podman 5.4 reports `MinAPIVersion 4.0.0`,
  so endpoints should work, but newer 5.x-only fields/endpoints may be missing.
  `podman-rest-client` is the v5-oriented alternative.
- GPUI is pre-1.0; API churn between 0.2.x releases is expected.
- `xattr`/`libc` pin and the `libxkbcommon-x11` build shim are workarounds that
  should be removed when upstream fixes land.
- CLI process-per-action overhead. Pulls and builds now stream their CLI output
  into the shared panel, but there is still no structured progress bar (that
  would need the REST API).
- The event stream is one extra long-lived `podman events` process per running
  instance and is not scoped: any container/image/pod/volume/network event
  refreshes the whole snapshot. That is cheap at this size, but a very chatty
  engine (other tools constantly creating and removing containers) will cause
  frequent full refreshes. Health-check and exec events are filtered out. When
  `podman events` is unavailable the app falls back to polling and shows
  `auto Ns`, so missing live updates are visible.
- Log streaming uses `--since` for the live window and a user-triggered
  `--tail` fetch for history; the merged buffer is capped at 3000 lines and
  viewport anchoring is line-granular.
- The tray icon requires a StatusNotifierItem host; on GNOME that means the
  AppIndicator extension. Without one the icon silently does not appear (a
  warning goes to stderr) and the app is only reachable through its window.
- Window position is not persisted, by design: GPUI's X11 backend reports
  frame-relative origins (ignored on resize) and its raw window handle is
  unimplemented, while `_NET_MOVERESIZE_WINDOW` offsets varied by window manager
  during testing (2026-11-05). Revisit only with a dedicated platform shim.
- `ksni` pulls `zbus` + `async-io`; it compiles without C libraries, but increases
  build time. Keep the `blocking` feature in sync if `ksni` is upgraded.
- Compose depends on a `podman compose` provider being installed and configured;
  when it is missing the action fails with the provider's error in the output
  panel. `down` can take ~10 s when containers ignore SIGTERM (observed
  2026-11-05). Projects are only detected through the standard labels.
- `podman volume prune` and `podman network prune` are **engine-wide**: they
  delete every unused volume/network, not just the ones this app created. The
  confirmation dialog says so, but there is no undo and no dry run. Observed
  first-hand on 2026-11-06, when a `volume prune -f` run during verification
  removed 25 unrelated volumes from the local engine. Never confirm a prune (or
  run the CLI form) against an engine whose unused volumes matter, and do not
  assume environment variables such as `CONTAINERS_STORAGE_CONF` isolate the
  storage — verify `podman info --format '{{.Store.GraphRoot}}'` first.
- Machine support depends on distro packaging of the helper binaries. On
  Debian/Ubuntu `gvproxy` needs `helper_binaries_dir` to include `/usr/bin`, and
  `virtiofsd` must be installed and put on `PATH`; without them `podman machine
  start` fails with “could not find …” errors. Machine start/stop blocks the task
  until the VM is up, and `machine init` downloads a multi-gigabyte image.
- The release archive is built locally and is unsigned, unpublished and
  Linux-only; `install.sh` touches only the chosen prefix and there is no package
  for distro repositories. The window icon is set through `_NET_WM_ICON` on the
  main window; a window manager that reads the icon only when the window is
  mapped may need a restart to show it the first time. On Wayland the icon comes
  from the desktop entry.
- The image file browser relies on `podman export | tar` instead of
  `podman image mount` (rootless mounts need `podman unshare`, and the mount does
  not survive the command). Each file preview re-streams the image, which is slow
  for multi-gigabyte images, and a missing `tar` disables the browser with an
  error. Do not add `--fast-read`: it is a BSD-tar option, and an early tar exit
  leaves `podman export` holding the storage lock.
- `podman info` is expensive (observed ~2.2 s on this host, dominated by its
  host/store probes), so the Dashboard caches it for 30 s and only fetches while
  the tab is open. Disk usage therefore lags behind changes made elsewhere until
  the next refresh cycle, entry into the tab, or an explicit Refresh.
- Compose logs stream `--tail 200` (a line count, not a time window), because
  `--since` is not portable across compose providers; the window setting and
  "Load older" are therefore hidden for projects, and provider stderr (warnings,
  banners) appears in the log pane. Compose output also depends on the provider
  intercepting signals cleanly when the app closes the stream.
- Machine start/stop is untested against a real VM in this environment (no
  machine was created); parsing is covered by unit tests. Remote switching was
  verified with an unreachable SSH connection only — a healthy remote engine was
  not available. `podman compose` against a remote connection may depend on the
  provider's handling of the connection flag. `podman machine init` also
  downloads a multi-gigabyte image and needs a working VM provider (libkrun and
  the `gvproxy`/`vfkit`/`qemu` backends), so it is validated only at the command
  and UI level here; the SSH terminal only works once a machine is running.
- `podman stats` costs one process per sample; the 2 s cadence is only active
  while the Stats tab is open. `--interval 1` shortens the sampling window but
  still sums CPU across cores, so values above 100% are possible and the graph
  rescales to them.
- Builds assume the Containerfile lives on the local machine and use its parent
  directory as context; there is no override yet, and remote builds depend on
  Podman's context transfer. Build and compose actions share one runner, so only
  one can run at a time.
- The terminal captures app shortcuts while focused, always runs `/bin/sh`, and
  hyperlinks (`OSC 8`) are unavailable because `vt100` does not expose them.
  Closing the PTY is what stops the shell; killing `podman exec` directly can
  leave a stale conmon, so the shutdown path closes the master first. The exec
  client also carries `PR_SET_PDEATHSIG=SIGTERM` so it cannot outlive a hard
  kill. Exec sessions themselves cannot be reaped when the client dies
  abruptly: podman leaves the conmon and its idle shell until the container
  stops, and those processes mostly ignore SIGTERM, so manual cleanup needs
  SIGKILL from inside the container. Selection and mouse coordinates are
  viewport cells, so a selection does not follow text as the screen scrolls and
  wheel scrolling clears it; dragging outside the pane stops extending it
  because GPUI only delivers move events while the grid is hovered.

## Verification checklist

Run for every change:

```bash
cd selkie
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build
```

Manual smoke test:

_Last verified 2026-11-05: after the module split — themes (dark → light →
high contrast → dark), settings persistence across restarts, log-window cycling
with stream restart, all tabs, details panes, context menu, `Esc`/`j`
keyboard handling, and no `podman logs` orphans after quit. Also after B7 —
tray item registered with a live StatusNotifierWatcher (menu and Quit driven
over D-Bus), second launch exits in ~4 ms with the first instance still running,
window size persisted on resize and restored on relaunch, `Ctrl+Q` quits. After
B5 — Compose tab and details, Up/Down streamed output against a real two-service
project, project removal after Down with output retained, and compose project
labels on container rows and details. After B8 — Connections tab against a real
configured SSH connection, engine switching to it (offline error shown, local
lists still usable) and back to the Podman default engine, plus the Machines
empty state; machine/connection parsing covered by unit tests. Connection
add/reachability (2026-11-07) verified against an unreachable `unix://`
endpoint in an isolated config: the automatic probe marked it `unreachable`, the
details pane showed Podman's error, the add dialog rendered with its validation
fields, **Edit…** renamed it and changed the URI (default flag kept, toast
shown), and **Delete…** removed it; deleting the in-use connection switched the
app back to Podman's default engine. Stats verified on
running containers (graphs update, history survives tab switches) and on a
stopped container (notice instead of graphs). Build verified end to end through
the modal: two-field entry with Tab switching, streamed STEP/COMMIT output,
success toast, refreshed image list, and cleanup of the produced image. Terminal
verified live: prompt, typed commands and output, ANSI rendering, session kept
across tab switches, stopped on selection change, `exit` shows "session ended"
with the client reaped, and Restart opens a new shell. After B2 — status bar
showed `live`, `podman stop`/`start` from a shell flipped the row state within
about 1 s without polling, killing the events child made the supervisor
reconnect within ~3 s while `auto Ns` polling covered the gap, and quitting left
no `podman events` client behind. Quick wins verified live: sorting by NAME and
SIZE (asc/desc, numeric sizes), pause/resume from the context menu and action
bar, rename through the validated modal (row, details and selection followed the
new name), raw JSON for a container and an image, and a pull streaming through
through the output panel. "Load older" verified against a container that wrote
2000 lines: the 800-line live buffer grew by 500, then 500, then 200 with the
same visible lines throughout, and finally reported "No older log output".
Terminal parent-death handling verified by opening a shell and `kill -9`ing the
app: the `podman exec` client disappeared with the app; leaking exec conmons
from earlier hard kills were then identified and cleaned. Resizable panels
verified live: sidebar dragged 224 → 304 px and details 400 → 540 px with
reflow, clamping stopped a 656 px details drag at the window-derived limit,
double-click restored 224/400, and both widths survived a restart. Build options
verified end to end: a Containerfile in `/tmp` built with `linux/amd64` and
`MESSAGE=hello`, producing an amd64 image whose container prints `hello`.
Ellipsis verified with the details pane open: image references and names end in
`…` instead of clipping mid-glyph. Run-from-image verified against
`alpine:latest`: name `pd-run-test`, port `18080:8080`, `FOO=bar`, a bind mount to
`/data` and command `sleep 120`; `podman inspect` showed exactly that argv, env,
mount and port mapping. Push/login/logout verified against a local
`registry:2` on `127.0.0.1:5001`: login streamed `Login Succeeded!` and created
`auth.json`, push streamed the layers and added `app-from-ui` to `/v2/_catalog`,
and logout emptied the credential file with a toast. Duplicate rows for a
multi-tagged image also disappeared after the ID dedupe. Resource creation
verified live on 2026-11-06: a volume (`pd-verify-vol`), a network created with
subnet `10.99.0.0/24` (visible in the table and in `podman network ls` on
interface `podman2`) and a pod created with `8080:80` (`podman pod inspect`
reported `{"80/tcp":[{"HostIp":"0.0.0.0","HostPort":"8080"}]}`); a mistyped port
`8080;80` and a missing YAML path each kept their modal open with an error
toast, and `kube play` streamed its output and failure into the output panel.
The prune confirmations were exercised only as far as their confirmation screen
— `podman volume prune -f` deletes every unused volume on the engine, so it must
never be confirmed against a machine whose unused volumes matter. Terminal grid
width verified live on 2026-11-06: the app resolved `JetBrainsMono Nerd Font
Mono` (a 7.2 px advance at the 12 px size), `stty size` answered 28×53, 28×65
and 28×100 for 384, 470 and 720 px panes, and a line of exactly 100 characters
reached the right edge of the 720 px pane without wrapping or clipping. Terminal
fidelity verified live the same day: `OSC 2` replaced the header label, bold /
italic / dim / underline rendered distinctly, `DECSCUSR` produced an underline,
a bar and a 2 Hz blinking bar, a double-click selected exactly the word and a
`Ctrl+Shift+C` → `Ctrl+Shift+V` round trip pasted it back byte for byte (od showed
`PASTED-OK`), `cat -v` printed `^[[200~PASTED-OK^[[201~` once bracketed paste was
enabled, and mouse reporting produced `^[[<0;12;4M`/`m` for a click and one
`^[[<64;12;4M`/`^[[<65;12;4M` per wheel notch._

Machine lifecycle verified in part on 2026-11-06: the Create machine dialog
rendered all four optional fields, Tab moved focus between them, and
`--cpus 0` / `--memory 100` were rejected with the dialog kept open and the
error in a toast; command construction and the validators are covered by the
`builds_machine_commands` unit test. A full `podman machine init` was not run
here because it downloads a multi-gigabyte VM image onto a 92%-full disk, and
the delete and SSH paths need a real machine — those remain unverified live.

Compose restart/build/logs verified live on 2026-11-06 against a throwaway
`pd-verify-compose` project (two alpine services, one of them built from a
temporary Containerfile): the Logs tab streamed both services with `built-1 |` /
`ticker-1 |` prefixes, the service chips narrowed the stream to `ticker` and
updated the header, **Build** streamed `=> COMMIT` / `Successfully tagged` into
the output panel and raised `compose build … built`, **Restart** streamed the
container restarts (`podman ps` showed fresh uptimes) while the log stream kept
following, and the context menu offered Up / Restart / Build / Down. Setting
`PODMAN_COMPOSE_WARNING_LOGS=false` removed the provider banner that previously
appeared as the first log line. The project, its containers, network, built image
and temp files were cleaned up afterwards. Verifying compose also required
restarting a stale `podman.socket` (the unit was active but the socket file was
missing) — the same issue already recorded under known risks.

Dashboard verified live on 2026-11-06: the cards matched the sidebar counts
(4 containers / 4 running, 21 images, 4 pods, 3 volumes, 3 networks, 0 machines),
creating a volume moved the Volumes card to 4 without leaving the tab, disk
usage reported 10.1 GB images with 7.8 GB reclaimable (bar), and the engine card
listed 5.4.2, `linux/amd64`, host `darkstar`, kernel `7.0.12+`, 12 CPUs, host
memory use, `crun 1.21`, `netavark`, `overlay`, `journald`, `rootless`,
`Podman default`, the socket path marked present and the graph root. Measuring
the refresh pipeline revealed `podman info` alone takes ~2.2 s here, which made
a per-poll fetch unusable; the result is now cached for 30 s and re-read on tab
entry, Refresh and `Ctrl+R`, after which the header settles on `updated just
now`. Light theme was checked for the cards, bars and engine rows.

Image history and the file browser were verified live on 2026-11-06 against
`alpine:latest`: History listed both layers (the base layer with `-` and 8.7 MB),
Files listed the root directories, `etc` and `bin` browsed correctly with
symlink targets (`bin/arch → /bin/busybox`), `etc/passwd` rendered as text,
`bin/busybox` reported `Binary file (804.6 kB)` from the listing size, and no
`pd-browse-*` container was left behind. Testing also caught a real bug: GNU tar
rejects `--fast-read`, and when tar exited early `podman export` kept the
storage lock, hanging every later podman command (including the app's refresh)
until it was killed. The pipeline now lets tar consume the stream and kills the
exporter on any tar failure; the leftover container and process from that
experiment were removed manually.

A real machine was created and started on 2026-11-06 (Fedora CoreOS 41 on qemu,
6 CPUs / 2 GiB). Starting it exposed two environment gaps that are now documented
in the README: Debian installs `gvproxy` in `/usr/bin` (not in Podman's default
`helper_binaries_dir`) and ships `virtiofsd` at `/usr/libexec/virtiofsd` (not on
`PATH`); with both reachable the machine boots. That also exposed a real bug:
the Machines tab showed “0 machines” because `podman machine ls --format json`
returns `Memory`/`DiskSize` as strings and the parser demanded numbers, so the
list silently fell back to empty. After the fix the app lists the running machine
(6 CPUs, 2.1 GB, 107.4 GB), its **Terminal** action opens `podman machine ssh` in
the details pane and `uname -r` answered `6.12.13-200.fc41.x86_64`, and the
Connections tab lists the two machine connections.

Re-verified on 2026-11-06 with `virtiofsd` on `PATH` and a freshly created
machine, all through the app: **Start** showed `Starting machine …` in the status
bar and flipped the row, details pill and action bar to `running` after about
30 s; **Terminal** opened `podman machine ssh`, whose OSC 2 title replaced the
header label with `core@localhost:~`, `uname -r` answered and `stty size`
reported `30 53` for that pane; the Connections tab listed
`podman-machine-default` (marked `default`) and `-root`, **Use in this app**
switched every list to the VM engine (0 containers/images/pods/volumes, 1
network) with `via podman-machine-default` in the status card, and **Use Podman
default** switched back. Podman itself reports `LastUp` as
`0001-01-01T00:00:00Z` while the machine runs, so the app shows `-`. For a couple
of seconds while `machine start` rewrote the connections config the sidebar
showed `Containers 0`; the next refresh restored the list.

Text cursors were added on 2026-11-06: the search box and modal fields showed no
caret at all. A 2 px accent bar now sits before a placeholder or after the text,
follows Tab between modal fields, and is kept visible while typing. The first
implementation used a GPUI `with_animation` loop, which kept redrawing the whole
window at display rate (~24% CPU just to blink); it now toggles on the existing
500 ms desktop tick at ~1%. The terminal block cursor was also invisible: a blank
cursor cell collapsed to zero width, so the reversed background never painted;
blank cells now use a non-breaking space and the block shows at the prompt.

The terminal was upgraded on 2026-11-06: `stty size` reports the pane size
(28×61, then 28×84 after dragging the splitter, with rows unchanged), which also
proves `TIOCSWINSZ` reaches the pty; `seq 1 300` plus wheel-up showed
`36 lines back`, the Live button and the cursor hidden while scrolled back;
`cat -v` printed `^[OA` with `DECCKM` on and `^[[A` with it off. Testing also
showed that clicking a header button moved focus to the root, so a later `k`
re-selected the row and reset the tab — the Live/Restart handlers now refocus the
terminal grid.

Packaging was verified on 2026-11-06: the SVG rendered to 16/32/48/64/128/256/512
PNGs (checked visually at 256 and 16), `desktop-file-validate` is clean after
reducing `Categories` to `Development;`, `DESTDIR=/tmp/pd-stage
packaging/install.sh` staged the binary, entry and seven icons and `--uninstall`
removed all of them, the tarball layout installed correctly into a throwaway
`PREFIX` (including `mimeinfo.cache` and the icon cache), the archive contains
LICENSE/README/ROADMAP/desktop/icons/installer with a matching `.sha256`, and
`xprop -id <window> WM_CLASS` reports `"selkie", "selkie"`
after adding `app_id` to the window options.

Window icons were verified on 2026-11-06 on Cinnamon (X11): before the change
`xprop _NET_WM_ICON` answered “not found” and the window manager drew a generic
icon; afterwards the property carries seven renditions (16 through 128 px), the
48 px one decodes to the expected purple rounded square with a white “P”, the
property survives three consecutive restarts, and the Cinnamon window list shows
the purple “P”. The first attempt failed intermittently because the property was
written before the window manager had finished reparenting the window, so the
writer now retries until it reads the property back.

Debian packaging was added on 2026-11-06: `packaging/build-deb.sh` produces
`selkie_0.1.0-1_amd64.deb` (5.6 MB) with a stripped binary, desktop
entry, seven hicolor icons, man page and DEP-5 docs; `lintian` reports nothing;
`desktop-file-validate` passes on the packaged entry; `dpkg-deb -c` shows
`root/root` ownership and 755/644 modes; `apt-get -s install ./dist/…deb`
resolves to a clean single-package install; the extracted binary starts, renders
and still sets all seven `_NET_WM_ICON` sizes.

Accounts and Settings were verified on 2026-11-06 with a throwaway
`REGISTRY_AUTH_FILE` holding two fake registries alongside the real one: the
Registries pane listed all three as location / username / masked password (the
token-only entry reads `token`) and named both auth files; the `⋮` menu opened
clamped to the window and its **Log out** removed exactly the chosen entry and
row; **Configure** on the Red Hat Quay row prefilled `quay.io` and focused the
username; the Accounts tab showed the same table; the sub-navigation switched
panes and `settings_section` followed; and the 5 s refresh, 30 m log window,
theme pills and panel **Reset** all persisted as before.

Close-to-tray was verified on 2026-11-06 on Cinnamon (X11): sending
`WM_DELETE_WINDOW` to the app window left the process running with the window
`IsUnMapped` (it leaves the taskbar and Alt-Tab), the tray D-Bus names still
owned and 0 CPU ticks over five seconds; launching the binary again sent
`activate`, and within one 500 ms tick the window was `IsViewable` again and
rendering the previous selection. `Ctrl+Q` still ended the process.

1. Select a container: Summary renders, Logs streams and follows.
2. Start/stop/restart a disposable container; state and buttons update.
3. Delete with confirmation; selection and logs close.
4. Pull/tag/delete an image; toasts report success/errors.
5. Search filters both tabs; Escape/clear resets.
6. Kill the app with SIGKILL; confirm no `podman logs -f` orphans remain.
7. Without clicking, press `j`/`k`/arrows: select rows, details follow, list
   scrolls the selection into view. `Enter` selects the first row.
8. Hover a row: only that row reveals its actions. Right-click: menu opens at
   the cursor; Copy ID shows a toast and closes the menu; Escape closes it.
9. Press `/`, type, `Esc` clears search, `Esc` again returns focus to the list
   without clearing the current selection.
10. Check Pods, Volumes and Networks: lists populate, keyboard navigation opens
    details, hover actions and context-menu deletions open the right modal.
    Cancel a pod delete to confirm no destructive action ran.
11. Cycle the theme through Dark → Light → High contrast; colors, tables and
    modals stay legible, and the choice survives a restart.
12. Cycle auto refresh (3/5/10/30 s) and confirm the list still updates; change
    the log window and confirm the stream restarts and shows older output.
13. Change tabs, restart, and confirm the same tab is opened again. Corrupt
    `settings.json` and confirm the app starts with defaults.
14. Launch the app twice: the second process exits immediately and the running
    window is raised.
15. Tray host present: the icon appears with the Show window / Refresh / Quit
    menu; each entry works. Tray host absent: the app still starts and prints a
    warning.
16. Resize the window, quit with `Ctrl/Cmd+Q`, relaunch: the same size comes back.
    Maximize, quit, relaunch: the window is maximized and unmaximizing restores
    the previous size.
17. Confirm `Ctrl/Cmd+Q` and the window close button both quit and leave no
    `podman logs -f` child behind.
18. Start a compose project with `podman compose up -d`: the Compose tab lists it,
    details show services/files, and its containers display `project/service`.
19. Stop its containers, select the project and press `Up`: the output panel
    streams `podman compose` logs and the status returns to N/N running.
20. Press `Down`, confirm, and check that the containers/network are removed, the
    project disappears while its output stays visible, and no orphan processes
    remain. Cancel a Down once to confirm nothing runs.
21. With `podman machine init` available, the Machines tab lists it; Start and
    Stop update the state, and Stop asks for confirmation. Without machines the
    empty state explains how to create one.
22. Add a connection with `podman system connection add`, open Connections, press
    Use: the sidebar shows `via <name>`, the engine error appears for an
    unreachable host, and the Connections tab still works. Press Use Podman
    default and confirm the local engine comes back.
23. Restart with a selected connection and confirm it is still used; remove it
    from Podman and confirm the app falls back to the default engine.
24. Select a running container and open Stats: CPU, memory and network graphs
    update every 2 s with plausible values. Switch tabs and come back: history is
    kept. Select another container: history resets.
25. Open Stats for a stopped container: a notice replaces the graphs; start it
    and sampling resumes without leaving the tab.
26. Images → Build image: a missing Containerfile shows an error toast and the
    modal stays open; a valid path + tag builds, streams STEP/COMMIT output,
    toasts success and adds the image to the list. Cancel once and confirm
    nothing runs.
27. Start a build and try a compose action (or vice versa): the second is
    refused while one runs, and the output panel's Clear/Close are hidden.
28. Kill the app during a build: no `podman build` process is left behind.
29. Open Terminal for a running container: a `/bin/sh` prompt appears; run
    `echo ok`, `ls`, arrow keys and Ctrl+C. Switch to Summary and back: the shell
    and its output survive. Select another container: the exec session ends.
30. Type `exit`: the status turns to "session ended" (red) and no zombie or
    `podman exec` process remains; Restart opens a fresh shell.
31. Open Terminal for a stopped container: a notice appears instead of a shell.
32. With the status bar showing `live`, stop and start a container from another
    terminal (`podman stop <name>` / `podman start <name>`): the row updates
    within about a second without waiting for the poll interval. The same holds
    for `podman run -d`, `podman rmi` and `podman volume rm`.
33. Kill the `podman events` child (`pkill -f 'podman events'`): the status bar
    falls back to `auto Ns`, polling keeps the lists current, and the stream is
    reconnected within about 3 s (`live` returns). Quit the app and confirm no
    `podman events` client remains.
34. Pause a running container from the row actions: the state turns `paused`, the
    hover actions become Resume / Stop / Delete and the action bar replaces Start
    with Resume. Resume it and confirm it returns to `running` and Start/Pause are
    back.
35. Right-click a container → Rename…, enter a name with a space or `/`: an error
    toast appears and the modal stays open. Enter a valid name: the row, details
    and selection follow the new name and a success toast appears.
36. Click a column header: the list sorts with ▲; click again for ▼. Check a
    numeric column (Images → SIZE or MOUNTS in Volumes) sorts by value, not text,
    and that switching tabs resets the sort.
37. Select a container, image, pod, volume and network in turn and press JSON:
    pretty-printed inspect output appears with Copy; Summary switches back. The
    JSON is re-fetched after changing the selection.
38. Pull an image: the output panel streams `podman pull` progress (or the error
    for an unknown short name) and the image list refreshes on success.
39. Open Logs on a container with more output than the live buffer. The toolbar
    shows `800 lines` and Follow on. Press **Load older**: a toast reports the
    number of lines added, the counter grows, Follow switches off and the lines
    you were reading stay in place. Repeat until "No older log output"; Clear
    resets the counter to 0 and the cap to 800.
40. Open Terminal, confirm a shell starts, then `kill -9` the app: no
    `podman exec` client remains (it gets `PR_SET_PDEATHSIG`). Note that an exec
    conmon and its idle shell may still exist inside the container until it is
    restarted — podman offers no exec-session cleanup. Quitting normally must
    leave neither the client nor the conmon.
41. Drag the sidebar divider right and the details divider left: both panes
    resize smoothly and the table/details content reflows. Restart the app: both
    widths are restored. Drag past the limits and confirm they stop at 400 px
    (sidebar) and at a details width that still leaves 320 px for the list; with
    a narrow window the effective widths shrink instead of overflowing.
    Double-click each divider and confirm the defaults (224 / 400 px). Check
    that the terminal clips rather than resizing when the pane is too narrow.
42. Shrink the details pane so the table is narrow: long names, image refs and
    subtitles end in `…` rather than being cut mid-character, and widening the
    pane brings more characters back.
43. Images → Build image: fill any subset of context/platform/args. An invalid
    platform (`amd64`) or context shows a toast and keeps the modal open; a valid
    build runs with `--platform` and repeated `--build-arg` in the streamed
    command line, the image lands with that platform, and the build arg is
    visible in the image's output (`ARG` → `RUN echo`). Select two platforms and
    confirm the command uses `--manifest` and that `podman images` lists the
    manifest list.
44. Images → select an image → Run (row action, context menu or action bar):
    fill name, ports, environment, volumes and command, then run. The container
    appears running with the port mapping, env and mount recorded; leaving the
    name empty lets Podman generate one. Enter a bad mapping (`8080`) and confirm
    the modal stays open with a toast. Clear the ports/env/mounts and run again
    to confirm the minimal command works.
45. Against a local registry (`podman run -d -p 5001:5000 docker.io/library/registry:2`):
    Images → Login with `127.0.0.1:5001`, any username/password; the panel shows
    `Login Succeeded!` and `Login` is followed by a toast. Select an image →
    Push, set `127.0.0.1:5001/app:latest`, and confirm the registry `/v2/_catalog`
    lists it. Logout with an empty registry clears the stored credentials. Enter
    a registry with a path (`quay.io/x`) and confirm the modal stays open.
46. Volumes → **Create volume**: a name plus an empty driver creates it and the
    toast names it; enter a name starting with `-` or containing a space and
    confirm the modal stays open with a toast. Networks → **Create network**: a
    name plus a subnet (`10.99.0.0/24`) creates it with that subnet and a new
    interface; a malformed subnet (`10.99.0.0`) or gateway (`999.1.1.1`) keeps
    the modal open.
47. Pods → **Create pod**: name plus `8080:80` creates a pod whose
    `podman pod inspect` shows the port mapping; a bad mapping (`8080`) keeps the
    modal open. **Play kube** with a missing path shows an error and keeps the
    modal open; with a real YAML the output panel streams the command and its
    result, and failures also raise an error toast.
48. **Prune** (volumes and networks) shows a danger-styled confirmation whose
    text names the resource; Cancel leaves everything untouched. Only confirm
    this against a throwaway engine: `podman volume prune -f` and
    `podman network prune -f` delete **every** unused volume/network on the
    selected engine, not just the ones this app created.
49. Machines → **Create machine**: the dialog takes an optional name, CPUs, memory
    and disk size and streams `podman machine init` into the output panel. A full
    run downloads a multi-gigabyte VM image, so only do it on a machine with disk
    space to spare. `--cpus 0`, `--memory 100` and a name with a space each keep
    the dialog open with a toast. Select a running machine → **Terminal** (action
    bar or context menu) and confirm an SSH shell appears; a stopped machine shows
    "Machine is not running". **Delete** asks for confirmation and removes the VM.
50. Compose → select a project → **Logs**: lines from every service stream in with
    `service |` prefixes, the chips switch to a single service (the header names
    it), and Follow / Clear / Copy work. **Build** streams `compose build` into the
    output panel and **Restart** shows the containers restarting; the live log
    stream keeps running across a restart. The `last Nm` and Load older controls
    are absent for compose because it uses `--tail` instead of a time window.
51. **Dashboard**: the cards match the sidebar counts; creating or removing a
    volume updates the Volumes card without leaving the tab. Disk usage shows a
    size and a reclaimable share per resource type, and the engine card lists the
    expected version, OS/arch, runtime, mode (rootless/rootful) and socket state.
    Switch tabs away and back: the engine card is re-read (the header shows
    `Refreshing…` for the ~2 s the probe takes), and Refresh / `Ctrl+R` do the
    same. Switch connections and confirm the dashboard reloads for the new engine.
    Check the Dashboard in light and high-contrast themes.
52. Images → select an image → **History**: layers appear with short IDs, sizes,
    timestamps, created-by and comments; the base layer shows `-`. **Files**: the
    root listing loads after a short export, folders come first, `Up` and the
    breadcrumb navigate, and opening `etc/passwd` shows its text. A symlink shows
    `→ target` without a second export, and a binary (e.g. `bin/busybox`) reports
    its size. Confirm `podman ps -a` has no `pd-browse-*` container afterwards,
    including after an error (browse a nonexistent path via a stale listing).
    Switching to another image clears both tabs back to Summary.
53. Packaging: `packaging/render-icons.sh` regenerates the seven PNGs;
    `desktop-file-validate packaging/selkie.desktop` is clean;
    `DESTDIR=/tmp/stage packaging/install.sh` places the binary, entry and icons
    under `/tmp/stage/...` and `--uninstall` removes them. Extract a release
    archive elsewhere, run its `install.sh` with a throwaway `PREFIX`, launch the
    installed binary and confirm `xprop WM_CLASS` reports `selkie` and
    `xprop _NET_WM_ICON` lists 16–128 px icons (the window list and Alt-Tab then
    show the purple “P”, not a generic icon). Check the extracted icon renders
    and that the archive's `.sha256` matches. `packaging/build-deb.sh` builds a
    `.deb` that `lintian` accepts, `apt-get -s install ./dist/…deb` resolves, and
    after `sudo apt install` the app appears in the desktop menu under
    **Development** with its icon.
54. With a real machine (needs `gvproxy` and `virtiofsd` on `PATH`, see the
    README): the Machines tab lists it once with state, CPUs, memory, disk and
    last-up; **Start** in the action bar runs `podman machine start`, showing a
    `Starting machine …` status until the row and pill turn `running`;
    **Terminal** opens an SSH shell where `uname -r` answers and the pane title
    becomes the shell's OSC 2 title (e.g. `core@localhost:~`); Connections
    lists `podman-machine-default` and `podman-machine-default-root`; **Use in
    this app** switches every list to the VM engine (empty containers/images,
    `via podman-machine-default` in the status card) and **Use Podman default**
    switches back.
55. Caret: press `/` — a 2 px bar blinks after the typed text (and before the
    placeholder when the box is empty); it stays visible while typing. Open any
    modal: the caret is on the active field and moves with Tab. Idle CPU stays at
    ~0% (the blink is a 2 Hz tick, not a GPUI animation). A container or machine
    **Terminal** shows a block cursor at the prompt.
56. Terminal: open one, run `stty size`, drag the details splitter, and run it
    again — the columns change (rows only change with the window height) and a
    line of exactly that many characters reaches the right edge of the pane
    without wrapping or clipping. Run `seq 1 300`, scroll up with the wheel: the
    header shows `N lines back` with a **Live** button, the block cursor
    disappears, Live returns to the prompt, and typing a key jumps back to the
    bottom. `printf '\033[?1h'; cat -v` then Up prints `^[OA`; after
    `printf '\033[?1l'` it prints `^[[A`. Clicking Live/Reload must not steal the
    keyboard.
57. Terminal styling and cursor: `printf
    '\033[1mB\033[0m \033[2mD\033[0m \033[3mI\033[0m \033[4mU\033[0m\n'`
    renders bold, dim, italic and underlined letters; `printf '\033[4 q'`,
    `\033[6 q` and `\033[1 q` switch the cursor to an underline, a bar and a
    blinking block; `printf '\033]2;hello\007'` replaces the header label with
    `hello`.
58. Terminal clipboard and mouse: drag across output — the range is highlighted;
    `Ctrl+Shift+C` then `echo ` + `Ctrl+Shift+V` pastes exactly that text (a
    double-click selects one word, a triple-click the line). With bracketed
    paste enabled (`printf '\033[?2004h'`) a paste into `cat -v` prints
    `^[[200~…^[[201~`. With `printf '\033[?1000h\033[?1006h'` a click in
    `cat -v` prints `^[[<0;x;yM` and `…m`, one wheel notch prints
    `^[[<64;x;yM` (or `<65` scrolling down), and no selection is created.
59. Registries (Accounts tab, or Settings → Registries — the same view): the
    table lists the auth-file entries as registry location / username / masked
    password (or `token`), and CREDENTIAL FILES names the files read. **Add
    registry** opens the login dialog empty. A provider row's **Configure** opens
    it with e.g. `quay.io` filled in and the Username field focused. The `⋮`
    button opens a small menu with **Log out** that clamps to the window; the
    menu closes on `Esc` or a click outside, and confirming the logout removes
    the entry, shows a toast and drops the row. Run this against a throwaway
    `REGISTRY_AUTH_FILE` if the real credentials matter.
60. Settings: the sidebar ends with Accounts and Settings (no inline theme or
    refresh rows). Settings has a Registries / Appearance / Engine / About
    sub-navigation whose choice is written to `settings.json` and restored on
    launch. Appearance: each theme repaints the window, refresh and log-window
    choices update the saved value (and restart a live log stream), and **Reset**
    puts the sidebar/details widths back to 224/400 px after a drag. Engine shows
    the current connection; About shows the app version, Podman version and
    settings path.
61. Close to tray: click the window's X. The window disappears and leaves the
    taskbar, but `pgrep -f '\./selkie$'` (or the process list) still
    shows it, `xwininfo -id <window> | grep 'Map State'` says `IsUnMapped`, the
    tray tooltip is still there and idle CPU stays at ~0. **Show window** in the
    tray — or launching the binary again — maps it back with the previous
    selection and logs, and `Ctrl+Q` (or the tray's **Quit**) still exits.
62. Connections → **Add connection**: add a `unix://` endpoint that does not
    exist (or an SSH host that is down) with no identity. It appears with
    STATUS `unreachable` after the automatic probe; selecting it shows the
    REACHABILITY section with Podman's error, and **Check** toasts the result.
    A bad name/URI, a missing identity file or an identity on `unix://` keeps
    the dialog open with a toast. **Edit…** renames it and changes the URI; a
    name that already exists is rejected. **Delete…** removes it after the
    danger confirmation, and deleting the connection selected with **Use in this
    app** switches the app back to Podman's default engine. `podman system
    connection list` should agree after each step.

## File map

- `src/podman.rs` — CLI commands, JSON parsing, inspect mapping, validation
- `src/terminal.rs` — PTY + `podman exec` session and `vt100` emulator state
- `src/desktop.rs` — single-instance lock/socket, tray icon, signal channel
- `src/ui.rs` — entity, shared state, polling, selection, logs plumbing, actions
- `src/ui/sidebar.rs` — navigation, engine status, Accounts/Settings entries
- `src/ui/accounts.rs` — registry credentials read from Podman's auth files
- `src/ui/preferences.rs` — the Settings page
- `src/ui/table.rs` — toolbars, search field, table headers and bodies
- `src/ui/rows.rs` — row builders and hover action buttons
- `src/ui/events.rs` — `podman events` stream, coalescing, reconnects, filters
- `src/ui/compose.rs` — compose summary, actions and streamed-output panel
- `src/ui/output.rs` — shared streamed-output panel for compose and builds
- `src/ui/engines.rs` — machine and connection summaries and actions
- `src/ui/terminal.rs` — terminal grid rendering, key encoding and lifecycle
- `src/ui/details.rs` — inspect panes and the streaming log viewer
- `src/ui/image.rs` — image history and the filesystem browser
- `src/ui/dialogs.rs` — status bar, toasts, context menu, confirm modals
- `src/ui/helpers.rs` — shared widget builders
- `src/ui/resizer.rs` — draggable sidebar/details splitters and width limits
- `src/ui/theme.rs` — dark / light / high-contrast palettes
- `src/ui/settings.rs` — persisted settings
- `src/main.rs` — window bootstrap, close handler, desktop/window-state task
- `build.rs` — Linux linker shim
- `packaging/` — icon SVG + PNGs, desktop entry, man page, `install.sh`,
  `build-release.sh`, `build-deb.sh`, `render-icons.sh`
- `README.md` — user-facing usage and limitations
