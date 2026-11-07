//! The GPUI view: sidebar, tables, details pane, actions and toasts.

mod accounts;
mod compose;
mod dashboard;
mod details;
mod dialogs;
mod engines;
mod events;
mod helpers;
mod image;
mod output;
mod preferences;
mod resizer;
mod rows;
pub(crate) mod settings;
mod sidebar;
mod table;
mod terminal;
mod theme;

use std::{
    collections::HashSet,
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
    sync::mpsc::{self, TryRecvError},
    time::{Duration, Instant},
};

use gpui::{
    div, prelude::*, px, rgb, rgba, uniform_list, AnyElement, App, Bounds, ClickEvent,
    ClipboardItem, Context, Div, FocusHandle, FontWeight, KeyDownEvent, MouseButton,
    MouseDownEvent, Pixels, Point, Render, ScrollDelta, ScrollHandle, ScrollStrategy,
    ScrollWheelEvent, SharedString, Task, UniformListScrollHandle, Window,
};
use serde::{Deserialize, Serialize};

use crate::podman::{
    self, ComposeAction, ComposeProject, Connection, Container, ContainerAction, ContainerDetails,
    Image, LocalState, Machine, MachineAction, Network, NetworkDetails, Pod, PodAction, PodDetails,
    Snapshot, StatsSample, Volume, VolumeDetails,
};
use crate::terminal::TerminalSession;

use settings::Settings;
use theme::{set_kind, theme};

use helpers::*;

const TOAST_TTL: Duration = Duration::from_secs(5);
const MAX_LOG_LINES: usize = 800;
/// Lines fetched per compose `logs --tail` stream.
const COMPOSE_LOG_TAIL: usize = 200;
/// Extra lines fetched per "load older" request.
const LOG_CHUNK: usize = 500;
/// Hard cap once older history has been loaded into the live buffer.
const MAX_LOG_HISTORY: usize = 3000;
const MAX_OUTPUT_LINES: usize = 2000;
const MAX_STATS_SAMPLES: usize = 120;
const STATS_INTERVAL: Duration = Duration::from_secs(2);
const ROW_HEIGHT: f32 = 46.0;

/// `podman info` probes the store and network and takes seconds on some hosts,
/// so the Dashboard data is cached and refreshed on demand or when stale.
const DASHBOARD_TTL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) enum Tab {
    Dashboard,
    #[default]
    Containers,
    Compose,
    Images,
    Pods,
    Volumes,
    Networks,
    Machines,
    Connections,
    Accounts,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DetailsTab {
    Summary,
    History,
    Files,
    Logs,
    Stats,
    Terminal,
}

#[derive(Clone, PartialEq, Eq)]
enum Selection {
    Container(String),
    Compose(String),
    Image(String),
    Pod(String),
    Volume(String),
    Network(String),
    Machine(String),
    Connection(String),
}

/// What an open terminal session is attached to.
#[derive(Clone, PartialEq, Eq)]
enum TerminalTarget {
    Container(String),
    Machine(String),
}

/// What the Logs tab is streaming.
#[derive(Clone, PartialEq, Eq)]
enum LogTarget {
    Container(String),
    Compose {
        project: String,
        service: Option<String>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BuildField {
    Containerfile,
    Tag,
    Context,
    Platform,
    Args,
}

impl BuildField {
    const ALL: [BuildField; 5] = [
        BuildField::Containerfile,
        BuildField::Tag,
        BuildField::Context,
        BuildField::Platform,
        BuildField::Args,
    ];

    fn index(self) -> usize {
        match self {
            BuildField::Containerfile => 0,
            BuildField::Tag => 1,
            BuildField::Context => 2,
            BuildField::Platform => 3,
            BuildField::Args => 4,
        }
    }

    fn from_index(index: usize) -> Self {
        Self::ALL
            .get(index)
            .copied()
            .unwrap_or(BuildField::Containerfile)
    }

    fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RunField {
    Name,
    Ports,
    Env,
    Volumes,
    Command,
}

impl RunField {
    const ALL: [RunField; 5] = [
        RunField::Name,
        RunField::Ports,
        RunField::Env,
        RunField::Volumes,
        RunField::Command,
    ];

    fn index(self) -> usize {
        match self {
            RunField::Name => 0,
            RunField::Ports => 1,
            RunField::Env => 2,
            RunField::Volumes => 3,
            RunField::Command => 4,
        }
    }

    fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(RunField::Name)
    }

    fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LoginField {
    Registry,
    Username,
    Password,
}

impl LoginField {
    const ALL: [LoginField; 3] = [
        LoginField::Registry,
        LoginField::Username,
        LoginField::Password,
    ];

    fn index(self) -> usize {
        match self {
            LoginField::Registry => 0,
            LoginField::Username => 1,
            LoginField::Password => 2,
        }
    }

    fn from_index(index: usize) -> Self {
        Self::ALL
            .get(index)
            .copied()
            .unwrap_or(LoginField::Registry)
    }

    fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VolumeField {
    Name,
    Driver,
}

impl VolumeField {
    const ALL: [VolumeField; 2] = [VolumeField::Name, VolumeField::Driver];

    fn index(self) -> usize {
        match self {
            VolumeField::Name => 0,
            VolumeField::Driver => 1,
        }
    }

    fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(VolumeField::Name)
    }

    fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NetworkField {
    Name,
    Driver,
    Subnet,
    Gateway,
}

impl NetworkField {
    const ALL: [NetworkField; 4] = [
        NetworkField::Name,
        NetworkField::Driver,
        NetworkField::Subnet,
        NetworkField::Gateway,
    ];

    fn index(self) -> usize {
        match self {
            NetworkField::Name => 0,
            NetworkField::Driver => 1,
            NetworkField::Subnet => 2,
            NetworkField::Gateway => 3,
        }
    }

    fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(NetworkField::Name)
    }

    fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PodField {
    Name,
    Ports,
}

impl PodField {
    const ALL: [PodField; 2] = [PodField::Name, PodField::Ports];

    fn index(self) -> usize {
        match self {
            PodField::Name => 0,
            PodField::Ports => 1,
        }
    }

    fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(PodField::Name)
    }

    fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MachineField {
    Name,
    Cpus,
    Memory,
    Disk,
}

impl MachineField {
    const ALL: [MachineField; 4] = [
        MachineField::Name,
        MachineField::Cpus,
        MachineField::Memory,
        MachineField::Disk,
    ];

    fn index(self) -> usize {
        match self {
            MachineField::Name => 0,
            MachineField::Cpus => 1,
            MachineField::Memory => 2,
            MachineField::Disk => 3,
        }
    }

    fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(MachineField::Name)
    }

    fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }
}

#[derive(Clone)]
enum Modal {
    DeleteContainer {
        id: String,
        name: String,
    },
    RenameContainer {
        id: String,
        name: String,
        input: String,
    },
    DeleteImage {
        id: String,
        name: String,
    },
    DeletePod {
        id: String,
        name: String,
    },
    DeleteVolume {
        name: String,
    },
    DeleteNetwork {
        name: String,
    },
    ComposeDown {
        project: ComposeProject,
    },
    StopMachine {
        name: String,
    },
    BuildImage {
        containerfile: String,
        tag: String,
        context: String,
        platform: String,
        args: String,
        field: BuildField,
    },
    RunContainer {
        image: String,
        name: String,
        ports: String,
        env: String,
        volumes: String,
        command: String,
        field: RunField,
    },
    PushImage {
        image: String,
        destination: String,
    },
    RegistryLogin {
        registry: String,
        username: String,
        password: String,
        field: LoginField,
    },
    RegistryLogout {
        registry: String,
    },
    PullImage {
        input: String,
    },
    TagImage {
        id: String,
        source: String,
        input: String,
    },
    CreateVolume {
        name: String,
        driver: String,
        field: VolumeField,
    },
    CreateNetwork {
        name: String,
        driver: String,
        subnet: String,
        gateway: String,
        field: NetworkField,
    },
    CreatePod {
        name: String,
        ports: String,
        field: PodField,
    },
    PlayKube {
        path: String,
    },
    PruneVolumes,
    PruneNetworks,
    CreateMachine {
        name: String,
        cpus: String,
        memory: String,
        disk: String,
        field: MachineField,
    },
    DeleteMachine {
        name: String,
    },
}

impl Modal {
    fn run_container(image: impl Into<String>) -> Self {
        Modal::RunContainer {
            image: image.into(),
            name: String::new(),
            ports: String::new(),
            env: String::new(),
            volumes: String::new(),
            command: String::new(),
            field: RunField::Name,
        }
    }

    fn create_volume() -> Self {
        Modal::CreateVolume {
            name: String::new(),
            driver: String::new(),
            field: VolumeField::Name,
        }
    }

    fn create_network() -> Self {
        Modal::CreateNetwork {
            name: String::new(),
            driver: String::new(),
            subnet: String::new(),
            gateway: String::new(),
            field: NetworkField::Name,
        }
    }

    fn create_pod() -> Self {
        Modal::CreatePod {
            name: String::new(),
            ports: String::new(),
            field: PodField::Name,
        }
    }

    fn create_machine() -> Self {
        Modal::CreateMachine {
            name: String::new(),
            cpus: String::new(),
            memory: String::new(),
            disk: String::new(),
            field: MachineField::Name,
        }
    }
}

#[derive(Clone, Copy)]
enum ToastKind {
    Success,
    Error,
}

struct Toast {
    id: u64,
    kind: ToastKind,
    message: String,
}

#[derive(Clone)]
struct ContextMenuState {
    position: Point<Pixels>,
    selection: Selection,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct SortState {
    column: usize,
    ascending: bool,
}

/// Typed sort key so numeric columns (size, counts, CPUs) sort naturally.
enum SortKey {
    Text(String),
    Number(u64),
}

impl SortKey {
    fn compare(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (SortKey::Number(a), SortKey::Number(b)) => a.cmp(b),
            (SortKey::Text(a), SortKey::Text(b)) => a.to_lowercase().cmp(&b.to_lowercase()),
            (SortKey::Text(a), SortKey::Number(b)) => a.cmp(&b.to_string()),
            (SortKey::Number(a), SortKey::Text(b)) => a.to_string().cmp(b),
        }
    }
}

enum DetailsState {
    None,
    Loading,
    Container(Box<ContainerDetails>),
    Pod(Box<PodDetails>),
    Volume(Box<VolumeDetails>),
    Network(Box<NetworkDetails>),
    Error(String),
}

/// Raw `inspect` JSON for the current selection.
enum RawState {
    Loading,
    Text(String),
    Error(String),
}

/// `podman history` for the selected image.
enum HistoryState {
    Loading,
    Ready(Vec<podman::ImageHistoryEntry>),
    Error(String),
}

/// One image's filesystem listing, cached while it stays selected.
struct FilesState {
    entries: Vec<podman::ImageFile>,
    loading: bool,
    error: Option<String>,
}

/// The file open in the image browser.
struct FileViewState {
    path: String,
    /// Size from the listing, when known.
    size: Option<u64>,
    content: FileContent,
}

enum FileContent {
    Loading,
    Text { text: String, truncated: bool },
    Binary { size: usize },
    Error(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DragTarget {
    Sidebar,
    Details,
}

#[derive(Clone, Copy)]
enum AfterAction {
    Refresh,
    RefreshLogs,
    Deselect,
}

enum TextOutcome {
    Edited,
    Submit,
    Cancel,
    Ignored,
}

struct LogStream {
    child: Option<std::process::Child>,
    _task: Task<()>,
}

impl Drop for LogStream {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// A streaming command (compose action or image build).
/// Title, echoed command line and result messages for a streamed command.
struct OutputSpec {
    title: String,
    command_line: String,
    success: String,
    failure: String,
}

struct OutputRun {
    success: String,
    failure: String,
    child: Option<std::process::Child>,
    status: Option<std::process::ExitStatus>,
    receiver: mpsc::Receiver<String>,
    _task: Task<()>,
}

impl Drop for OutputRun {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Terminate the child when the app is killed without unwinding.
#[cfg(target_os = "linux")]
fn kill_with_parent(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        command.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(not(target_os = "linux"))]
fn kill_with_parent(_command: &mut Command) {}

pub struct PodmanDesktop {
    tab: Tab,
    settings: Settings,
    snapshot: Snapshot,
    local: LocalState,
    error: Option<String>,
    loading: bool,
    refresh_pending: bool,
    busy: Option<String>,
    last_updated: Option<Instant>,

    events_task: Option<Task<()>>,
    events_connected: bool,

    search: String,
    search_focus: FocusHandle,
    /// Current phase of the text caret (toggled by [`Self::blink_caret`]).
    caret_on: bool,
    visible_indices: Vec<usize>,
    sort: Option<SortState>,
    rows_scroll: UniformListScrollHandle,
    root_focus: FocusHandle,
    initial_focus_done: bool,
    viewport_width: f32,
    drag: Option<DragTarget>,
    drag_start_x: f32,
    drag_start_width: f32,

    selection: Option<Selection>,
    details_tab: DetailsTab,
    details: DetailsState,
    raw_view: bool,
    raw: Option<RawState>,

    history: Option<HistoryState>,
    history_image: Option<String>,
    files: Option<FilesState>,
    files_image: Option<String>,
    files_dir: String,
    file_view: Option<FileViewState>,

    context_menu: Option<ContextMenuState>,

    logs: Vec<String>,
    logs_follow: bool,
    logs_handle: ScrollHandle,
    log_stream: Option<LogStream>,
    logs_target: Option<LogTarget>,
    logs_error: Option<String>,
    logs_limit: usize,
    logs_loading_older: bool,

    stats: Vec<StatsSample>,
    stats_container: Option<String>,
    stats_error: Option<String>,
    stats_task: Option<Task<()>>,

    terminal: Option<TerminalSession>,
    terminal_target: Option<TerminalTarget>,
    terminal_label: String,
    terminal_error: Option<String>,
    terminal_task: Option<Task<()>>,
    terminal_focus: FocusHandle,
    /// Grid size measured during prepaint, applied on the next terminal poll.
    terminal_size: std::rc::Rc<std::cell::Cell<(u16, u16)>>,
    /// Window-space origin of the terminal text area, captured during prepaint.
    terminal_origin: std::rc::Rc<std::cell::Cell<(f32, f32)>>,
    /// Anchor and active cell of a terminal text selection.
    terminal_selection: Option<(terminal::TermPos, terminal::TermPos)>,
    /// Monospace font and cell width, resolved on the first terminal paint.
    terminal_font: std::rc::Rc<std::cell::RefCell<Option<terminal::TerminalMetrics>>>,

    /// Registry logins Podman would use, read from its auth file(s).
    accounts: Vec<podman::Account>,
    accounts_error: Option<String>,

    output_lines: Vec<String>,
    output_title: Option<String>,
    output_handle: ScrollHandle,
    output_run: Option<OutputRun>,

    modal: Option<Modal>,
    modal_focus: FocusHandle,

    dashboard: Option<Box<podman::Dashboard>>,
    dashboard_error: Option<String>,
    dashboard_fetched: Option<Instant>,

    toasts: Vec<Toast>,
    next_toast_id: u64,
}

impl PodmanDesktop {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let settings = Settings::load();
        set_kind(settings.theme);
        let tab = settings.tab;
        let mut view = Self {
            tab,
            settings,
            snapshot: Snapshot::default(),
            local: LocalState::default(),
            error: None,
            loading: false,
            refresh_pending: false,
            busy: None,
            last_updated: None,
            events_task: None,
            events_connected: false,
            search: String::new(),
            search_focus: cx.focus_handle(),
            caret_on: true,
            visible_indices: Vec::new(),
            sort: None,
            rows_scroll: UniformListScrollHandle::new(),
            root_focus: cx.focus_handle(),
            initial_focus_done: false,
            viewport_width: 0.0,
            drag: None,
            drag_start_x: 0.0,
            drag_start_width: 0.0,

            selection: None,
            details_tab: DetailsTab::Summary,
            details: DetailsState::None,
            raw_view: false,
            raw: None,
            history: None,
            history_image: None,
            files: None,
            files_image: None,
            files_dir: String::new(),
            file_view: None,

            context_menu: None,
            logs: Vec::new(),
            logs_follow: true,
            logs_handle: ScrollHandle::new(),
            log_stream: None,
            logs_target: None,
            logs_error: None,
            logs_limit: MAX_LOG_LINES,
            logs_loading_older: false,
            stats: Vec::new(),
            stats_container: None,
            stats_error: None,
            stats_task: None,
            terminal: None,
            terminal_target: None,
            terminal_label: String::new(),
            terminal_error: None,
            terminal_task: None,
            terminal_focus: cx.focus_handle(),
            terminal_size: std::rc::Rc::new(std::cell::Cell::new((0, 0))),
            terminal_origin: std::rc::Rc::new(std::cell::Cell::new((0.0, 0.0))),
            terminal_selection: None,
            terminal_font: std::rc::Rc::new(std::cell::RefCell::new(None)),
            accounts: Vec::new(),
            accounts_error: None,
            output_lines: Vec::new(),
            output_title: None,
            output_handle: ScrollHandle::new(),
            output_run: None,
            modal: None,
            modal_focus: cx.focus_handle(),
            dashboard: None,
            dashboard_error: None,
            dashboard_fetched: None,
            toasts: Vec::new(),
            next_toast_id: 1,
        };
        view.refresh(cx);
        view.start_events(cx);

        cx.spawn(async move |this, cx| loop {
            let secs = match this.update(cx, |this, _cx| this.settings.refresh_secs.max(1)) {
                Ok(secs) => secs,
                Err(_) => break,
            };
            cx.background_executor()
                .timer(Duration::from_secs(secs))
                .await;
            // Skip the poll when an event already refreshed the snapshot within
            // the interval; polling is only a fallback for a quiet event stream.
            if this
                .update(cx, |this, cx| {
                    let interval = Duration::from_secs(secs);
                    let recently_refreshed =
                        this.last_updated.is_some_and(|at| at.elapsed() < interval);
                    if !recently_refreshed {
                        this.refresh(cx);
                    }
                })
                .is_err()
            {
                break;
            }
        })
        .detach();

        view
    }

    // -----------------------------------------------------------------------
    // Data loading
    // -----------------------------------------------------------------------

    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        // Login and logout run through the output panel and end in a refresh, so
        // the credential list follows them.
        if self.tab == Tab::Accounts {
            self.load_accounts();
        }
        if self.loading {
            // Remember the request so an event arriving during a fetch still
            // results in a fresh snapshot once the current one lands.
            self.refresh_pending = true;
            return;
        }
        self.loading = true;
        cx.notify();

        // `podman info` and `podman system df` are only worth their extra
        // processes while the Dashboard is on screen, and `podman info` alone
        // can take seconds on some hosts, so the result is cached.
        let want_dashboard = self.tab == Tab::Dashboard
            && self
                .dashboard_fetched
                .is_none_or(|at| at.elapsed() >= DASHBOARD_TTL);

        cx.spawn(async move |this, cx| {
            let (local, result, dashboard) = cx
                .background_executor()
                .spawn(async move {
                    let local = podman::local_state();
                    let snapshot = podman::snapshot();
                    let dashboard = if want_dashboard {
                        Some(podman::dashboard())
                    } else {
                        None
                    };
                    (local, snapshot, dashboard)
                })
                .await;
            this.update(cx, |this, cx| {
                this.loading = false;
                // Machines and connections are local configuration and stay
                // available even when the selected engine cannot be reached.
                this.local = local;
                if let Some(dashboard) = dashboard {
                    this.dashboard_fetched = Some(Instant::now());
                    match dashboard {
                        Ok(data) => {
                            this.dashboard = Some(Box::new(data));
                            this.dashboard_error = None;
                        }
                        Err(error) => this.dashboard_error = Some(error),
                    }
                }
                match result {
                    Ok(snapshot) => {
                        this.snapshot = snapshot;
                        this.error = None;
                        this.last_updated = Some(Instant::now());
                        if this.selection_is_gone() {
                            this.clear_selection();
                        }
                        let menu_gone = this
                            .context_menu
                            .as_ref()
                            .is_some_and(|menu| this.is_gone(&menu.selection));
                        if menu_gone {
                            this.context_menu = None;
                        }
                    }
                    Err(error) => {
                        // Keep the last good data visible and surface the error.
                        this.error = Some(error);
                    }
                }
                let again = std::mem::take(&mut this.refresh_pending);
                cx.notify();
                if again {
                    this.refresh(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Refresh, and re-read the Dashboard's slow `podman info`/`system df` data.
    pub(crate) fn refresh_dashboard(&mut self, cx: &mut Context<Self>) {
        self.dashboard_fetched = None;
        self.refresh(cx);
    }

    /// Toggle the text caret while a text input has focus.
    ///
    /// Called from the 500 ms desktop tick in `main.rs`: a GPUI animation would
    /// redraw the whole window at display rate just to blink a 2 px bar.
    pub(crate) fn blink_caret(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.modal.is_some() || self.search_focus.is_focused(window) {
            self.caret_on = !self.caret_on;
            cx.notify();
        } else if self.terminal_focus.is_focused(window)
            && self
                .terminal
                .as_ref()
                .is_some_and(|session| session.cursor_blinks())
        {
            // Only a blinking cursor shape needs the redraw.
            self.caret_on = !self.caret_on;
            cx.notify();
        }
    }

    fn selection_is_gone(&self) -> bool {
        self.selection
            .as_ref()
            .is_some_and(|selection| self.is_gone(selection))
    }

    fn is_gone(&self, selection: &Selection) -> bool {
        match selection {
            Selection::Container(id) => !self.snapshot.containers.iter().any(|item| &item.id == id),
            Selection::Compose(name) => {
                !self.snapshot.compose.iter().any(|item| &item.name == name)
            }
            Selection::Image(id) => !self.snapshot.images.iter().any(|item| &item.id == id),
            Selection::Pod(id) => !self.snapshot.pods.iter().any(|item| &item.id == id),
            Selection::Volume(name) => !self.snapshot.volumes.iter().any(|item| &item.name == name),
            Selection::Network(name) => {
                !self.snapshot.networks.iter().any(|item| &item.name == name)
            }
            Selection::Machine(name) => !self.local.machines.iter().any(|item| &item.name == name),
            Selection::Connection(name) => {
                !self.local.connections.iter().any(|item| &item.name == name)
            }
        }
    }

    // -----------------------------------------------------------------------
    // Keyboard navigation and context menu
    // -----------------------------------------------------------------------

    fn root_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let modifiers = event.keystroke.modifiers;

        if (modifiers.control || modifiers.platform) && key == "r" {
            self.refresh_dashboard(cx);
            return;
        }
        if (modifiers.control || modifiers.platform) && key == "q" {
            cx.quit();
            return;
        }
        if modifiers.control || modifiers.platform || modifiers.alt {
            return;
        }
        if self.modal.is_some() || self.search_focus.is_focused(window) {
            return;
        }

        match key {
            "escape" => {
                if self.context_menu.is_some() {
                    self.close_context_menu(cx);
                } else if self.selection.is_some() {
                    self.clear_selection();
                    cx.notify();
                } else if !self.search.is_empty() {
                    self.search.clear();
                    cx.notify();
                }
            }
            "/" => {
                if matches!(self.tab, Tab::Dashboard | Tab::Accounts | Tab::Settings) {
                    return;
                }
                self.close_context_menu(cx);
                self.caret_on = true;
                self.search_focus.focus(window);
                cx.notify();
            }
            "j" | "down" => {
                self.close_context_menu(cx);
                self.move_selection(1, cx);
            }
            "k" | "up" => {
                self.close_context_menu(cx);
                self.move_selection(-1, cx);
            }
            "enter" | "return" => {
                if self.selection.is_none() {
                    self.select_index(0, cx);
                    if !self.visible_indices.is_empty() {
                        self.rows_scroll.scroll_to_item(0, ScrollStrategy::Center);
                    }
                }
            }
            _ => {}
        }
    }

    fn current_visible_position(&self) -> Option<usize> {
        match (&self.selection, self.tab) {
            (Some(Selection::Container(id)), Tab::Containers) => self
                .visible_indices
                .iter()
                .position(|&index| &self.snapshot.containers[index].id == id),
            (Some(Selection::Image(id)), Tab::Images) => self
                .visible_indices
                .iter()
                .position(|&index| &self.snapshot.images[index].id == id),
            (Some(Selection::Pod(id)), Tab::Pods) => self
                .visible_indices
                .iter()
                .position(|&index| &self.snapshot.pods[index].id == id),
            (Some(Selection::Volume(name)), Tab::Volumes) => self
                .visible_indices
                .iter()
                .position(|&index| &self.snapshot.volumes[index].name == name),
            (Some(Selection::Network(name)), Tab::Networks) => self
                .visible_indices
                .iter()
                .position(|&index| &self.snapshot.networks[index].name == name),
            _ => None,
        }
    }

    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(next) = next_position(
            self.current_visible_position(),
            self.visible_indices.len(),
            delta,
        ) else {
            return;
        };

        self.select_index(next, cx);
        self.rows_scroll
            .scroll_to_item(next, ScrollStrategy::Center);
    }

    fn select_index(&mut self, position: usize, cx: &mut Context<Self>) {
        let Some(&index) = self.visible_indices.get(position) else {
            return;
        };
        match self.tab {
            Tab::Containers => {
                let id = self.snapshot.containers[index].id.clone();
                self.select_container(id, cx);
            }
            Tab::Compose => {
                let name = self.snapshot.compose[index].name.clone();
                self.select_compose(name, cx);
            }
            Tab::Images => {
                let id = self.snapshot.images[index].id.clone();
                self.select_image(id, cx);
            }
            Tab::Pods => {
                let id = self.snapshot.pods[index].id.clone();
                self.select_pod(id, cx);
            }
            Tab::Volumes => {
                let name = self.snapshot.volumes[index].name.clone();
                self.select_volume(name, cx);
            }
            Tab::Networks => {
                let name = self.snapshot.networks[index].name.clone();
                self.select_network(name, cx);
            }
            Tab::Machines => {
                let name = self.local.machines[index].name.clone();
                self.select_machine(name, cx);
            }
            Tab::Connections => {
                let name = self.local.connections[index].name.clone();
                self.select_connection(name, cx);
            }
            Tab::Dashboard | Tab::Accounts | Tab::Settings => {}
        }
    }

    fn open_context_menu(
        &mut self,
        position: Point<Pixels>,
        selection: Selection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.root_focus.focus(window);
        match selection.clone() {
            Selection::Container(id) => self.select_container(id, cx),
            Selection::Compose(name) => self.select_compose(name, cx),
            Selection::Image(id) => self.select_image(id, cx),
            Selection::Pod(id) => self.select_pod(id, cx),
            Selection::Volume(name) => self.select_volume(name, cx),
            Selection::Network(name) => self.select_network(name, cx),
            Selection::Machine(name) => self.select_machine(name, cx),
            Selection::Connection(name) => self.select_connection(name, cx),
        }
        self.context_menu = Some(ContextMenuState {
            position,
            selection,
        });
        cx.notify();
    }

    fn close_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.context_menu.take().is_some() {
            cx.notify();
        }
    }

    fn copy_text(&mut self, text: String, label: &'static str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.toast(ToastKind::Success, format!("Copied {label}"), cx);
    }

    /// Persist the window size so the next launch opens with the same frame.
    pub(crate) fn remember_window(&mut self, bounds: Bounds<Pixels>, maximized: bool) {
        self.settings.remember_window(
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
            maximized,
        );
    }

    fn cycle_log_window(&mut self, cx: &mut Context<Self>) {
        self.settings.next_log_window();
        self.apply_log_window(cx);
    }

    /// Store a log window picked in Settings and restart any live stream so the
    /// change takes effect immediately.
    fn set_log_window(&mut self, window: &str, cx: &mut Context<Self>) {
        if self.settings.log_window == window {
            return;
        }
        self.settings.log_window = window.to_string();
        self.apply_log_window(cx);
    }

    fn apply_log_window(&mut self, cx: &mut Context<Self>) {
        self.settings.save();
        match self.logs_target.clone() {
            Some(LogTarget::Container(id)) => self.start_logs(&id, cx),
            // Compose logs use `--tail`, so the container window does not apply;
            // restart them only to keep the setting consistent for later.
            Some(LogTarget::Compose { project, service }) => {
                let project = self
                    .snapshot
                    .compose
                    .iter()
                    .find(|item| item.name == project)
                    .cloned();
                if let Some(project) = project {
                    self.start_compose_logs(&project, service, cx);
                }
            }
            None => {}
        }
        cx.notify();
    }

    fn load_details(&mut self, selection: Selection, cx: &mut Context<Self>) {
        self.details = DetailsState::Loading;
        cx.notify();

        let expected = selection.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    match &selection {
                        Selection::Container(id) => podman::inspect_container(id)
                            .map(|details| DetailsState::Container(Box::new(details))),
                        Selection::Pod(id) => podman::inspect_pod(id)
                            .map(|details| DetailsState::Pod(Box::new(details))),
                        Selection::Volume(name) => podman::inspect_volume(name)
                            .map(|details| DetailsState::Volume(Box::new(details))),
                        Selection::Network(name) => podman::inspect_network(name)
                            .map(|details| DetailsState::Network(Box::new(details))),
                        Selection::Image(_) | Selection::Compose(_) => Ok(DetailsState::None),
                        Selection::Machine(_) | Selection::Connection(_) => Ok(DetailsState::None),
                    }
                    .unwrap_or_else(DetailsState::Error)
                })
                .await;
            this.update(cx, |this, cx| {
                if this.selection.as_ref() == Some(&expected) {
                    this.details = result;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn reload_selection(&mut self, cx: &mut Context<Self>) {
        if let Some(selection) = self.selection.clone() {
            if matches!(
                selection,
                Selection::Container(_)
                    | Selection::Pod(_)
                    | Selection::Volume(_)
                    | Selection::Network(_)
            ) {
                self.load_details(selection, cx);
            }
        }
    }

    /// Drop the cached raw JSON whenever the selection changes.
    fn reset_raw(&mut self) {
        self.raw_view = false;
        self.raw = None;
    }

    /// Toggle between the summary and the raw `inspect` JSON for the current
    /// selection. The JSON is fetched once and cached until the selection moves.
    fn toggle_raw(&mut self, selection: Selection, cx: &mut Context<Self>) {
        if self.raw_view {
            self.raw_view = false;
            cx.notify();
            return;
        }
        let Some(kind) = inspect_kind(&selection) else {
            return;
        };
        let key = match &selection {
            Selection::Container(key)
            | Selection::Image(key)
            | Selection::Pod(key)
            | Selection::Volume(key)
            | Selection::Network(key) => key.clone(),
            Selection::Compose(_) | Selection::Machine(_) | Selection::Connection(_) => return,
        };

        self.raw_view = true;
        if self.raw.is_none() {
            self.raw = Some(RawState::Loading);
            let expected = selection;
            cx.spawn(async move |this, cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move { podman::inspect_json(kind, &key) })
                    .await;
                this.update(cx, |this, cx| {
                    if this.selection.as_ref() == Some(&expected) {
                        this.raw = Some(match result {
                            Ok(text) => RawState::Text(text),
                            Err(error) => RawState::Error(error),
                        });
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
        cx.notify();
    }

    // -----------------------------------------------------------------------
    // Selection, logs and commands
    // -----------------------------------------------------------------------

    fn select_container(&mut self, id: String, cx: &mut Context<Self>) {
        self.context_menu = None;
        self.reset_raw();
        self.selection = Some(Selection::Container(id.clone()));
        self.details_tab = DetailsTab::Summary;
        self.details = DetailsState::Loading;
        self.stop_stats();
        self.stats.clear();
        self.stats_container = None;
        self.stats_error = None;
        self.stop_terminal();
        self.start_logs(&id, cx);
        self.load_details(Selection::Container(id), cx);
        cx.notify();
    }

    fn select_compose(&mut self, name: String, cx: &mut Context<Self>) {
        self.context_menu = None;
        self.reset_raw();
        self.stop_logs();
        self.logs.clear();
        self.logs_error = None;
        self.selection = Some(Selection::Compose(name));
        self.details_tab = DetailsTab::Summary;
        self.details = DetailsState::None;
        cx.notify();
    }

    fn select_machine(&mut self, name: String, cx: &mut Context<Self>) {
        self.context_menu = None;
        self.reset_raw();
        self.stop_logs();
        self.stop_terminal();
        self.logs.clear();
        self.logs_error = None;
        self.selection = Some(Selection::Machine(name));
        self.details_tab = DetailsTab::Summary;
        self.details = DetailsState::None;
        cx.notify();
    }

    fn select_connection(&mut self, name: String, cx: &mut Context<Self>) {
        self.context_menu = None;
        self.reset_raw();
        self.stop_logs();
        self.logs.clear();
        self.logs_error = None;
        self.selection = Some(Selection::Connection(name));
        self.details_tab = DetailsTab::Summary;
        self.details = DetailsState::None;
        cx.notify();
    }

    /// Switch the engine the whole app talks to.
    fn use_connection(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        if self.settings.connection == name {
            return;
        }
        if self.output_run.is_some() {
            self.toast(
                ToastKind::Error,
                "Wait for the running compose command before switching engines",
                cx,
            );
            return;
        }
        self.settings.connection = name.clone();
        self.settings.save();
        podman::set_connection(name.clone());

        // The previous engine's data is meaningless now; start from empty and
        // keep the local machine/connection lists.
        self.stop_logs();
        self.stop_terminal();
        self.stop_events();
        self.dashboard = None;
        self.dashboard_fetched = None;
        self.dashboard_error = None;
        self.logs.clear();
        self.logs_error = None;
        self.snapshot = Snapshot::default();
        self.error = None;
        self.selection = None;
        self.details = DetailsState::None;
        self.details_tab = DetailsTab::Summary;
        self.raw_view = false;
        self.raw = None;
        self.refresh(cx);
        self.start_events(cx);

        let message = match &name {
            Some(name) => format!("Using connection {name}"),
            None => "Using the Podman default engine".to_string(),
        };
        self.toast(ToastKind::Success, message, cx);
        cx.notify();
    }

    fn machine_action(&mut self, name: String, action: MachineAction, cx: &mut Context<Self>) {
        let label = format!("{} machine {name}…", action.gerund());
        let success = format!("Machine {name} {}", action.done());
        self.run_task(
            label,
            success,
            AfterAction::Refresh,
            move || podman::machine_action(&name, action),
            cx,
        );
    }

    fn select_image(&mut self, id: String, cx: &mut Context<Self>) {
        self.context_menu = None;
        self.reset_raw();
        self.reset_image_views();
        self.stop_logs();
        self.logs.clear();
        self.logs_error = None;
        self.selection = Some(Selection::Image(id));
        self.details_tab = DetailsTab::Summary;
        self.details = DetailsState::None;
        cx.notify();
    }

    fn select_pod(&mut self, id: String, cx: &mut Context<Self>) {
        self.context_menu = None;
        self.reset_raw();
        self.stop_logs();
        self.logs.clear();
        self.logs_error = None;
        self.selection = Some(Selection::Pod(id.clone()));
        self.details_tab = DetailsTab::Summary;
        self.load_details(Selection::Pod(id), cx);
        cx.notify();
    }

    fn select_volume(&mut self, name: String, cx: &mut Context<Self>) {
        self.context_menu = None;
        self.reset_raw();
        self.stop_logs();
        self.logs.clear();
        self.logs_error = None;
        self.selection = Some(Selection::Volume(name.clone()));
        self.details_tab = DetailsTab::Summary;
        self.load_details(Selection::Volume(name), cx);
        cx.notify();
    }

    fn select_network(&mut self, name: String, cx: &mut Context<Self>) {
        self.context_menu = None;
        self.reset_raw();
        self.stop_logs();
        self.logs.clear();
        self.logs_error = None;
        self.selection = Some(Selection::Network(name.clone()));
        self.details_tab = DetailsTab::Summary;
        self.load_details(Selection::Network(name), cx);
        cx.notify();
    }

    fn clear_selection(&mut self) {
        self.context_menu = None;
        self.reset_raw();
        if self.selection.is_some() {
            self.selection = None;
            self.details_tab = DetailsTab::Summary;
            self.details = DetailsState::None;
            self.stop_logs();
            self.logs.clear();
            self.logs_error = None;
            self.stop_stats();
            self.stats.clear();
            self.stats_container = None;
            self.stats_error = None;
            self.stop_terminal();
        }
    }

    fn start_logs(&mut self, id: &str, cx: &mut Context<Self>) {
        let command = podman::container_logs_command(id, self.settings.log_window.as_str());
        self.start_log_stream(command, LogTarget::Container(id.to_string()), cx);
    }

    /// Stream `podman compose logs` for a project, or one of its services.
    fn start_compose_logs(
        &mut self,
        project: &ComposeProject,
        service: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let target = LogTarget::Compose {
            project: project.name.clone(),
            service: service.clone(),
        };
        match podman::compose_logs_command(project, service.as_deref(), COMPOSE_LOG_TAIL) {
            Ok(command) => self.start_log_stream(command, target, cx),
            Err(error) => {
                // Still show the Logs tab with the reason instead of a stale stream.
                self.stop_logs();
                self.logs.clear();
                self.logs_error = Some(error);
                self.logs_target = Some(target);
                cx.notify();
            }
        }
    }

    fn start_log_stream(
        &mut self,
        mut command: Command,
        target: LogTarget,
        cx: &mut Context<Self>,
    ) {
        self.stop_logs();
        self.logs.clear();
        self.logs_error = None;
        self.logs_limit = MAX_LOG_LINES;
        self.logs_loading_older = false;
        self.logs_follow = true;
        self.logs_target = Some(target);

        command.stdout(Stdio::piped()).stderr(Stdio::piped());

        // Do not leave `podman logs -f` behind if the app is killed without
        // unwinding (Drop would otherwise take care of it on a clean exit).
        kill_with_parent(&mut command);

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                self.logs_error = Some(format!("could not start log stream: {error}"));
                return;
            }
        };

        let (sender, receiver) = mpsc::channel::<String>();
        if let Some(stdout) = child.stdout.take() {
            spawn_line_reader(stdout, sender.clone());
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_line_reader(stderr, sender.clone());
        }
        drop(sender);

        let task = cx.spawn(async move |this, cx| loop {
            match receiver.try_recv() {
                Ok(line) => {
                    if this
                        .update(cx, |this, cx| {
                            this.push_log(line);
                            cx.notify();
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Err(TryRecvError::Empty) => {
                    cx.background_executor()
                        .timer(Duration::from_millis(60))
                        .await
                }
                Err(TryRecvError::Disconnected) => break,
            }
        });

        self.log_stream = Some(LogStream {
            child: Some(child),
            _task: task,
        });
    }

    fn stop_logs(&mut self) {
        // Dropping the stream kills `podman logs -f` and cancels the reader task.
        self.log_stream = None;
        self.logs_target = None;
    }

    /// Start sampling `podman stats` for a container, keeping history in memory.
    fn start_stats(&mut self, id: String, cx: &mut Context<Self>) {
        self.stop_stats();
        if self.stats_container.as_deref() != Some(id.as_str()) {
            self.stats.clear();
            self.stats_error = None;
            self.stats_container = Some(id.clone());
        }

        let task = cx.spawn(async move |this, cx| loop {
            let fetch_id = id.clone();
            let result = cx
                .background_executor()
                .spawn(async move { podman::stats(&fetch_id) })
                .await;

            let keep_going = this
                .update(cx, |this, cx| {
                    match result {
                        Ok(Some(sample)) => {
                            this.stats_error = None;
                            this.stats.push(sample);
                            if this.stats.len() > MAX_STATS_SAMPLES {
                                let excess = this.stats.len() - MAX_STATS_SAMPLES;
                                this.stats.drain(0..excess);
                            }
                        }
                        Ok(None) => {
                            this.stats_error = Some("Podman returned no statistics".to_string());
                        }
                        Err(error) => this.stats_error = Some(error),
                    }
                    cx.notify();

                    matches!(&this.selection, Some(Selection::Container(current)) if current == &id)
                        && this.details_tab == DetailsTab::Stats
                })
                .unwrap_or(false);

            if !keep_going {
                break;
            }
            cx.background_executor().timer(STATS_INTERVAL).await;
        });

        self.stats_task = Some(task);
    }

    fn stop_stats(&mut self) {
        // Dropping the task stops the sampling loop.
        self.stats_task = None;
    }

    fn push_log(&mut self, line: String) {
        self.logs.push(strip_ansi(&line));
        if self.logs.len() > self.logs_limit {
            let excess = self.logs.len() - self.logs_limit;
            self.logs.drain(0..excess);
        }
        if self.logs_follow {
            self.logs_handle.scroll_to_bottom();
        }
    }

    /// Fetch earlier log output with `podman logs --tail` and prepend it.
    ///
    /// The requested size grows with the current buffer, so repeated clicks
    /// reach further back. Only lines older than the first visible line are
    /// merged, which keeps the live stream and the one-shot fetch from
    /// duplicating each other.
    fn load_older_logs(&mut self, cx: &mut Context<Self>) {
        if self.logs_loading_older {
            return;
        }
        let Some(LogTarget::Container(id)) = self.logs_target.clone() else {
            return;
        };
        let requested = (self.logs.len() + LOG_CHUNK).min(MAX_LOG_HISTORY);
        if requested <= self.logs.len() {
            self.toast(ToastKind::Success, "Reached the log history limit", cx);
            return;
        }

        let mut command = podman::logs_tail_command(&id, requested);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        kill_with_parent(&mut command);

        self.logs_limit = requested;
        self.logs_loading_older = true;
        // Loading history means the user is reading older output; don't yank
        // the view back to the bottom while the fetch is in flight.
        self.logs_follow = false;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    command
                        .output()
                        .map_err(|error| format!("could not read older logs: {error}"))
                })
                .await;

            this.update(cx, |this, cx| {
                this.logs_loading_older = false;
                match result {
                    Ok(output) if output.status.success() => {
                        let text = String::from_utf8_lossy(&output.stdout);
                        let older = older_log_lines(&text, &this.logs);
                        if older.is_empty() {
                            this.toast(ToastKind::Success, "No older log output", cx);
                        } else {
                            let added = older.len();
                            let before = this.logs.len();
                            // Remember which line was at the top so the view can
                            // stay on the same content after prepending.
                            let anchor = this.logs_handle.top_item();
                            let mut merged = older;
                            merged.append(&mut this.logs);
                            this.logs = merged;
                            if this.logs.len() > this.logs_limit {
                                let excess = this.logs.len() - this.logs_limit;
                                this.logs.drain(0..excess);
                            }
                            let shift = this.logs.len().saturating_sub(before);
                            if shift > 0 && before > 0 {
                                this.logs_handle.scroll_to_top_of_item(anchor + shift);
                            }
                            this.toast(
                                ToastKind::Success,
                                format!("Loaded {added} older log lines"),
                                cx,
                            );
                        }
                    }
                    Ok(output) => {
                        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                        let message = if stderr.is_empty() {
                            format!("could not read older logs: {}", output.status)
                        } else {
                            stderr
                        };
                        this.toast(ToastKind::Error, message, cx);
                    }
                    Err(error) => this.toast(ToastKind::Error, error, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // -----------------------------------------------------------------------
    // Long-running commands with streamed output (compose, image builds)
    // -----------------------------------------------------------------------

    fn start_compose(
        &mut self,
        project: &ComposeProject,
        action: ComposeAction,
        cx: &mut Context<Self>,
    ) {
        let command = match podman::compose_command(project, action) {
            Ok(command) => command,
            Err(error) => {
                self.toast(ToastKind::Error, error, cx);
                return;
            }
        };

        self.start_output(
            OutputSpec {
                title: format!("compose {} · {}", action.verb(), project.name),
                command_line: format!("$ podman compose -p {} {}", project.name, action.verb()),
                success: format!(
                    "compose {} {} {}",
                    action.verb(),
                    project.name,
                    action.done()
                ),
                failure: format!("compose {} {} failed", action.verb(), project.name),
            },
            command,
            None,
            cx,
        );
    }

    /// Run a command, streaming stdout and stderr into the output panel.
    ///
    /// `input` is written to stdin and then closed (used for `--password-stdin`
    /// so credentials never appear on the command line).
    fn start_output(
        &mut self,
        spec: OutputSpec,
        mut command: Command,
        input: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let OutputSpec {
            title,
            command_line,
            success,
            failure,
        } = spec;
        if self.output_run.is_some() {
            self.toast(
                ToastKind::Error,
                "A build or compose command is already running",
                cx,
            );
            return;
        }

        command
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        kill_with_parent(&mut command);

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                self.toast(
                    ToastKind::Error,
                    format!("could not run podman: {error}"),
                    cx,
                );
                return;
            }
        };

        if let Some(input) = input {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(input.as_bytes());
                let _ = stdin.write_all(b"\n");
            }
        }

        self.output_lines.clear();
        self.output_lines.push(command_line);
        self.output_title = Some(title);
        self.output_handle.scroll_to_bottom();

        let (sender, receiver) = mpsc::channel::<String>();
        if let Some(stdout) = child.stdout.take() {
            spawn_line_reader(stdout, sender.clone());
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_line_reader(stderr, sender.clone());
        }
        drop(sender);

        let task = cx.spawn(async move |this, cx| loop {
            let finished = this
                .update(cx, |this, cx| this.poll_output(cx))
                .unwrap_or(true);
            if finished {
                break;
            }
            cx.background_executor()
                .timer(Duration::from_millis(80))
                .await;
        });

        self.output_run = Some(OutputRun {
            success,
            failure,
            child: Some(child),
            status: None,
            receiver,
            _task: task,
        });
        cx.notify();
    }

    /// Drain output and report whether the command has finished.
    fn poll_output(&mut self, cx: &mut Context<Self>) -> bool {
        let (lines, output_done, exited) = {
            let Some(run) = self.output_run.as_mut() else {
                return true;
            };

            let mut lines = Vec::new();
            let mut output_done = false;
            loop {
                match run.receiver.try_recv() {
                    Ok(line) => lines.push(line),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        output_done = true;
                        break;
                    }
                }
            }

            if run.status.is_none() {
                if let Some(child) = run.child.as_mut() {
                    if let Ok(Some(status)) = child.try_wait() {
                        run.status = Some(status);
                    }
                }
            }

            (lines, output_done, run.status.is_some())
        };

        let pushed = !lines.is_empty();
        for line in lines {
            self.push_output_line(line);
        }
        if pushed {
            cx.notify();
        }
        if output_done && exited {
            self.finish_output(cx);
            return true;
        }
        false
    }

    fn push_output_line(&mut self, line: String) {
        self.output_lines.push(strip_ansi(&line));
        if self.output_lines.len() > MAX_OUTPUT_LINES {
            let excess = self.output_lines.len() - MAX_OUTPUT_LINES;
            self.output_lines.drain(0..excess);
        }
        self.output_handle.scroll_to_bottom();
    }

    fn finish_output(&mut self, cx: &mut Context<Self>) {
        let title = self.output_title.clone().unwrap_or_default();
        let Some(run) = self.output_run.take() else {
            return;
        };
        let success = run.status.map(|status| status.success()).unwrap_or(false);
        let summary = match run.status {
            Some(status) if status.success() => format!("── {title} finished ──"),
            Some(status) => format!("── {title} failed: {status} ──"),
            None => format!("── {title} finished ──"),
        };
        self.output_lines.push(summary);
        self.output_handle.scroll_to_bottom();

        if success {
            self.toast(ToastKind::Success, run.success.clone(), cx);
        } else {
            self.toast(ToastKind::Error, run.failure.clone(), cx);
        }

        self.refresh(cx);
        cx.notify();
    }

    fn clear_output(&mut self, cx: &mut Context<Self>) {
        if self.output_run.is_some() {
            return;
        }
        self.output_lines.clear();
        self.output_title = None;
        cx.notify();
    }

    fn container_action(&mut self, id: String, action: ContainerAction, cx: &mut Context<Self>) {
        let label = format!("{}…", action.label());
        let success = action.done().to_string();
        self.run_task(
            label,
            success,
            AfterAction::RefreshLogs,
            move || podman::container_action(&id, action),
            cx,
        );
    }

    fn pod_action(&mut self, id: String, action: PodAction, cx: &mut Context<Self>) {
        let label = format!("{}…", action.label());
        let success = action.done().to_string();
        self.run_task(
            label,
            success,
            AfterAction::Refresh,
            move || podman::pod_action(&id, action),
            cx,
        );
    }

    fn run_task<F>(
        &mut self,
        label: String,
        success: String,
        after: AfterAction,
        task: F,
        cx: &mut Context<Self>,
    ) where
        F: FnOnce() -> Result<String, String> + Send + 'static,
    {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some(label);
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { task() }).await;
            this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(_) => {
                        this.toast(ToastKind::Success, success, cx);
                        match after {
                            AfterAction::Refresh => {
                                this.refresh(cx);
                                this.reload_selection(cx);
                            }
                            AfterAction::RefreshLogs => {
                                this.refresh(cx);
                                this.reload_selection(cx);
                                if let Some(Selection::Container(id)) = &this.selection {
                                    let id = id.clone();
                                    this.start_logs(&id, cx);
                                }
                            }
                            AfterAction::Deselect => {
                                this.clear_selection();
                                this.refresh(cx);
                            }
                        }
                    }
                    Err(error) => this.toast(ToastKind::Error, error, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // -----------------------------------------------------------------------
    // Modals
    // -----------------------------------------------------------------------

    fn open_modal(&mut self, modal: Modal, window: &mut Window, cx: &mut Context<Self>) {
        self.context_menu = None;
        self.modal = Some(modal);
        // Start with the caret on rather than waiting for the next blink tick.
        self.caret_on = true;
        self.modal_focus.focus(window);
        cx.notify();
    }

    fn close_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.modal = None;
        self.search_focus.focus(window);
        cx.notify();
    }

    fn confirm_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(modal) = self.modal.clone() else {
            return;
        };

        match &modal {
            Modal::PullImage { input } | Modal::TagImage { input, .. } => {
                if let Err(error) = podman::validate_image_reference(input) {
                    self.toast(ToastKind::Error, error, cx);
                    self.modal_focus.focus(window);
                    return;
                }
            }
            Modal::RenameContainer { input, .. } => {
                if let Err(error) = podman::validate_container_name(input) {
                    self.toast(ToastKind::Error, error, cx);
                    self.modal_focus.focus(window);
                    return;
                }
            }
            Modal::BuildImage {
                containerfile,
                tag,
                context,
                platform,
                args,
                ..
            } => {
                if let Err(error) = podman::validate_image_reference(tag) {
                    self.toast(ToastKind::Error, error, cx);
                    self.modal_focus.focus(window);
                    return;
                }
                let containerfile = containerfile.trim();
                if !std::path::Path::new(containerfile).is_file() {
                    self.toast(
                        ToastKind::Error,
                        format!("Containerfile not found: {containerfile}"),
                        cx,
                    );
                    self.modal_focus.focus(window);
                    return;
                }
                if !context.trim().is_empty() {
                    if let Err(error) = podman::validate_build_context(context) {
                        self.toast(ToastKind::Error, error, cx);
                        self.modal_focus.focus(window);
                        return;
                    }
                }
                if !platform.trim().is_empty() {
                    if let Err(error) = podman::validate_platform(platform) {
                        self.toast(ToastKind::Error, error, cx);
                        self.modal_focus.focus(window);
                        return;
                    }
                }
                if let Err(error) = podman::parse_build_args(args) {
                    self.toast(ToastKind::Error, error, cx);
                    self.modal_focus.focus(window);
                    return;
                }
            }
            Modal::RunContainer {
                name,
                ports,
                env,
                volumes,
                ..
            } => {
                if !name.trim().is_empty() {
                    if let Err(error) = podman::validate_container_name(name) {
                        self.toast(ToastKind::Error, error, cx);
                        self.modal_focus.focus(window);
                        return;
                    }
                }
                for result in [
                    podman::parse_ports(ports).map(|_| ()),
                    podman::parse_env_vars(env).map(|_| ()),
                    podman::parse_volume_mounts(volumes).map(|_| ()),
                ] {
                    if let Err(error) = result {
                        self.toast(ToastKind::Error, error, cx);
                        self.modal_focus.focus(window);
                        return;
                    }
                }
            }
            Modal::PushImage { image, destination } => {
                for result in [
                    podman::validate_image_reference(image).map(|_| ()),
                    podman::validate_image_reference(destination).map(|_| ()),
                ] {
                    if let Err(error) = result {
                        self.toast(ToastKind::Error, error, cx);
                        self.modal_focus.focus(window);
                        return;
                    }
                }
            }
            Modal::RegistryLogin {
                registry,
                username,
                password,
                ..
            } => {
                if let Err(error) = podman::validate_registry(registry) {
                    self.toast(ToastKind::Error, error, cx);
                    self.modal_focus.focus(window);
                    return;
                }
                if let Err(error) = podman::validate_username(username) {
                    self.toast(ToastKind::Error, error, cx);
                    self.modal_focus.focus(window);
                    return;
                }
                if password.is_empty() {
                    self.toast(ToastKind::Error, "password cannot be empty", cx);
                    self.modal_focus.focus(window);
                    return;
                }
            }
            Modal::RegistryLogout { registry } => {
                if !registry.trim().is_empty() {
                    if let Err(error) = podman::validate_registry(registry) {
                        self.toast(ToastKind::Error, error, cx);
                        self.modal_focus.focus(window);
                        return;
                    }
                }
            }
            Modal::CreateVolume { name, driver, .. } => {
                if let Err(error) = podman::validate_volume_name(name) {
                    self.toast(ToastKind::Error, error, cx);
                    self.modal_focus.focus(window);
                    return;
                }
                if !driver.trim().is_empty() {
                    if let Err(error) = podman::validate_driver(driver) {
                        self.toast(ToastKind::Error, error, cx);
                        self.modal_focus.focus(window);
                        return;
                    }
                }
            }
            Modal::CreateNetwork {
                name,
                driver,
                subnet,
                gateway,
                ..
            } => {
                if let Err(error) = podman::validate_network_name(name) {
                    self.toast(ToastKind::Error, error, cx);
                    self.modal_focus.focus(window);
                    return;
                }
                let optional = [
                    (
                        driver,
                        podman::validate_driver as fn(&str) -> Result<&str, String>,
                    ),
                    (subnet, podman::validate_subnet),
                    (gateway, podman::validate_gateway),
                ];
                for (value, validate) in optional {
                    if !value.trim().is_empty() {
                        if let Err(error) = validate(value) {
                            self.toast(ToastKind::Error, error, cx);
                            self.modal_focus.focus(window);
                            return;
                        }
                    }
                }
            }
            Modal::CreatePod { name, ports, .. } => {
                if let Err(error) = podman::validate_pod_name(name) {
                    self.toast(ToastKind::Error, error, cx);
                    self.modal_focus.focus(window);
                    return;
                }
                if let Err(error) = podman::parse_ports(ports) {
                    self.toast(ToastKind::Error, error, cx);
                    self.modal_focus.focus(window);
                    return;
                }
            }
            Modal::PlayKube { path } => {
                if let Err(error) = podman::validate_kube_file(path) {
                    self.toast(ToastKind::Error, error, cx);
                    self.modal_focus.focus(window);
                    return;
                }
            }
            Modal::CreateMachine {
                name,
                cpus,
                memory,
                disk,
                ..
            } => {
                if let Err(error) = podman::parse_machine_init(name, cpus, memory, disk) {
                    self.toast(ToastKind::Error, error, cx);
                    self.modal_focus.focus(window);
                    return;
                }
            }
            _ => {}
        }

        self.modal = None;
        self.search_focus.focus(window);

        match modal {
            Modal::DeleteContainer { id, name } => {
                let label = format!("Deleting {name}…");
                let success = format!("Deleted container {name}");
                self.run_task(
                    label,
                    success,
                    AfterAction::Deselect,
                    move || podman::remove_container(&id),
                    cx,
                );
            }
            Modal::RenameContainer { id, input, .. } => {
                let name = input.trim().to_string();
                let label = format!("Renaming to {name}…");
                let success = format!("Renamed container to {name}");
                self.run_task(
                    label,
                    success,
                    AfterAction::Refresh,
                    move || podman::rename_container(&id, &name),
                    cx,
                );
            }
            Modal::DeleteImage { id, name } => {
                let label = format!("Deleting {name}…");
                let success = format!("Deleted image {name}");
                self.run_task(
                    label,
                    success,
                    AfterAction::Deselect,
                    move || podman::remove_image(&id),
                    cx,
                );
            }
            Modal::DeletePod { id, name } => {
                let label = format!("Deleting pod {name}…");
                let success = format!("Deleted pod {name}");
                self.run_task(
                    label,
                    success,
                    AfterAction::Deselect,
                    move || podman::remove_pod(&id),
                    cx,
                );
            }
            Modal::DeleteVolume { name } => {
                let label = format!("Deleting volume {name}…");
                let success = format!("Deleted volume {name}");
                self.run_task(
                    label,
                    success,
                    AfterAction::Deselect,
                    move || podman::remove_volume(&name),
                    cx,
                );
            }
            Modal::DeleteNetwork { name } => {
                let label = format!("Deleting network {name}…");
                let success = format!("Deleted network {name}");
                self.run_task(
                    label,
                    success,
                    AfterAction::Deselect,
                    move || podman::remove_network(&name),
                    cx,
                );
            }
            Modal::ComposeDown { project } => {
                self.start_compose(&project, ComposeAction::Down, cx);
            }
            Modal::StopMachine { name } => {
                self.machine_action(name, MachineAction::Stop, cx);
            }
            Modal::BuildImage {
                containerfile,
                tag,
                context,
                platform,
                args,
                ..
            } => {
                let options = podman::BuildOptions {
                    containerfile: containerfile.trim().to_string(),
                    tag: tag.trim().to_string(),
                    context: Some(context.trim().to_string()).filter(|value| !value.is_empty()),
                    platform: Some(platform.trim().to_string()).filter(|value| !value.is_empty()),
                    args: podman::parse_build_args(&args).unwrap_or_default(),
                };
                match podman::build_command(&options) {
                    Ok(command) => {
                        let mut line = format!(
                            "$ podman build -f {} -t {}",
                            options.containerfile, options.tag
                        );
                        if let Some(platform) = &options.platform {
                            line.push_str(&format!(" --platform {platform}"));
                        }
                        for arg in &options.args {
                            line.push_str(&format!(" --build-arg {arg}"));
                        }
                        if let Some(context) = &options.context {
                            line.push_str(&format!(" {context}"));
                        }
                        self.start_output(
                            OutputSpec {
                                title: format!("build · {}", options.tag),
                                command_line: line,
                                success: format!("Built {}", options.tag),
                                failure: format!("Build failed for {}", options.tag),
                            },
                            command,
                            None,
                            cx,
                        );
                    }
                    Err(error) => self.toast(ToastKind::Error, error, cx),
                }
            }
            Modal::RunContainer {
                image,
                name,
                ports,
                env,
                volumes,
                command,
                ..
            } => {
                let options = podman::RunOptions {
                    image: image.clone(),
                    name: Some(name.trim().to_string()).filter(|value| !value.is_empty()),
                    ports: podman::parse_ports(&ports).unwrap_or_default(),
                    env: podman::parse_env_vars(&env).unwrap_or_default(),
                    volumes: podman::parse_volume_mounts(&volumes).unwrap_or_default(),
                    command: podman::parse_command_args(&command),
                };
                match podman::container_run_command(&options) {
                    Ok(_) => {
                        let label = format!("Starting a container from {image}…");
                        let success = format!("Started a container from {image}");
                        self.run_task(
                            label,
                            success,
                            AfterAction::Refresh,
                            move || podman::run_container(&options),
                            cx,
                        );
                    }
                    Err(error) => self.toast(ToastKind::Error, error, cx),
                }
            }
            Modal::PushImage { image, destination } => {
                match podman::push_command(&image, &destination) {
                    Ok(command) => {
                        self.start_output(
                            OutputSpec {
                                title: format!("push · {destination}"),
                                command_line: format!("$ podman push {image} {destination}"),
                                success: format!("Pushed {image} to {destination}"),
                                failure: format!("Push failed for {image}"),
                            },
                            command,
                            None,
                            cx,
                        );
                    }
                    Err(error) => self.toast(ToastKind::Error, error, cx),
                }
            }
            Modal::RegistryLogin {
                registry,
                username,
                password,
                ..
            } => match podman::login_command(&registry, &username) {
                Ok(command) => {
                    self.start_output(
                        OutputSpec {
                            title: format!("login · {registry}"),
                            command_line: format!(
                                "$ podman login --username {username} --password-stdin {registry}"
                            ),
                            success: format!("Logged in to {registry}"),
                            failure: format!("Login failed for {registry}"),
                        },
                        command,
                        Some(password),
                        cx,
                    );
                }
                Err(error) => self.toast(ToastKind::Error, error, cx),
            },
            Modal::RegistryLogout { registry } => {
                let target = registry.trim().to_string();
                let success = if target.is_empty() {
                    "Logged out of all registries".to_string()
                } else {
                    format!("Logged out of {target}")
                };
                let target = Some(target).filter(|value| !value.is_empty());
                self.run_task(
                    "Logging out…".to_string(),
                    success,
                    AfterAction::Refresh,
                    move || podman::logout(target.as_deref()),
                    cx,
                );
            }
            Modal::PullImage { input } => {
                let reference = input.trim().to_string();
                let command = match podman::pull_command(&reference) {
                    Ok(command) => command,
                    Err(error) => {
                        self.toast(ToastKind::Error, error, cx);
                        return;
                    }
                };
                self.start_output(
                    OutputSpec {
                        title: format!("pull · {reference}"),
                        command_line: format!("$ podman pull {reference}"),
                        success: format!("Pulled {reference}"),
                        failure: format!("Pull failed for {reference}"),
                    },
                    command,
                    None,
                    cx,
                );
            }
            Modal::TagImage { id, input, .. } => {
                let target = input.trim().to_string();
                let success = format!("Tagged image as {target}");
                self.run_task(
                    format!("Tagging as {target}…"),
                    success,
                    AfterAction::Refresh,
                    move || podman::tag_image(&id, &target),
                    cx,
                );
            }
            Modal::CreateVolume { name, driver, .. } => {
                let name = name.trim().to_string();
                let driver = Some(driver.trim().to_string()).filter(|value| !value.is_empty());
                let success = format!("Created volume {name}");
                self.run_task(
                    format!("Creating volume {name}…"),
                    success,
                    AfterAction::Refresh,
                    move || podman::create_volume(&name, driver.as_deref()),
                    cx,
                );
            }
            Modal::CreateNetwork {
                name,
                driver,
                subnet,
                gateway,
                ..
            } => {
                let name = name.trim().to_string();
                let optional =
                    |value: &str| Some(value.trim().to_string()).filter(|value| !value.is_empty());
                let driver = optional(&driver);
                let subnet = optional(&subnet);
                let gateway = optional(&gateway);
                let success = format!("Created network {name}");
                self.run_task(
                    format!("Creating network {name}…"),
                    success,
                    AfterAction::Refresh,
                    move || {
                        podman::create_network(
                            &name,
                            driver.as_deref(),
                            subnet.as_deref(),
                            gateway.as_deref(),
                        )
                    },
                    cx,
                );
            }
            Modal::CreatePod { name, ports, .. } => {
                let name = name.trim().to_string();
                let ports = podman::parse_ports(&ports).unwrap_or_default();
                let success = format!("Created pod {name}");
                self.run_task(
                    format!("Creating pod {name}…"),
                    success,
                    AfterAction::Refresh,
                    move || podman::create_pod(&name, &ports),
                    cx,
                );
            }
            Modal::PlayKube { path } => {
                let path = path.trim().to_string();
                match podman::kube_play_command(&path) {
                    Ok(command) => {
                        self.start_output(
                            OutputSpec {
                                title: format!("kube play · {path}"),
                                command_line: format!("$ podman kube play {path}"),
                                success: format!("Played {path}"),
                                failure: format!("kube play failed for {path}"),
                            },
                            command,
                            None,
                            cx,
                        );
                    }
                    Err(error) => self.toast(ToastKind::Error, error, cx),
                }
            }
            Modal::PruneVolumes => {
                self.run_task(
                    "Pruning volumes…".to_string(),
                    "Pruned unused volumes".to_string(),
                    AfterAction::Deselect,
                    podman::prune_volumes,
                    cx,
                );
            }
            Modal::PruneNetworks => {
                self.run_task(
                    "Pruning networks…".to_string(),
                    "Pruned unused networks".to_string(),
                    AfterAction::Deselect,
                    podman::prune_networks,
                    cx,
                );
            }
            Modal::CreateMachine {
                name,
                cpus,
                memory,
                disk,
                ..
            } => {
                let options = match podman::parse_machine_init(&name, &cpus, &memory, &disk) {
                    Ok(options) => options,
                    Err(error) => {
                        self.toast(ToastKind::Error, error, cx);
                        return;
                    }
                };
                let label = if options.name.is_empty() {
                    "the default machine".to_string()
                } else {
                    options.name.clone()
                };
                match podman::machine_init_command(&options) {
                    Ok(command) => self.start_output(
                        OutputSpec {
                            title: format!("machine init · {label}"),
                            command_line: format!("$ {}", podman::machine_init_display(&options)),
                            success: format!("Created machine {label}"),
                            failure: format!("machine init failed for {label}"),
                        },
                        command,
                        None,
                        cx,
                    ),
                    Err(error) => self.toast(ToastKind::Error, error, cx),
                }
            }
            Modal::DeleteMachine { name } => {
                self.run_task(
                    format!("Deleting machine {name}…"),
                    format!("Deleted machine {name}"),
                    AfterAction::Deselect,
                    move || podman::machine_remove(&name),
                    cx,
                );
            }
        }
        cx.notify();
    }

    fn modal_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let outcome = match &mut self.modal {
            Some(Modal::PullImage { input })
            | Some(Modal::TagImage { input, .. })
            | Some(Modal::RenameContainer { input, .. }) => edit_text(input, event, cx),
            Some(Modal::BuildImage {
                field,
                containerfile,
                tag,
                context,
                platform,
                args,
            }) => {
                if event.keystroke.key.as_str() == "tab" {
                    *field = field.next();
                    TextOutcome::Edited
                } else {
                    let target = match field {
                        BuildField::Containerfile => containerfile,
                        BuildField::Tag => tag,
                        BuildField::Context => context,
                        BuildField::Platform => platform,
                        BuildField::Args => args,
                    };
                    edit_text(target, event, cx)
                }
            }
            Some(Modal::RunContainer {
                field,
                name,
                ports,
                env,
                volumes,
                command,
                ..
            }) => {
                if event.keystroke.key.as_str() == "tab" {
                    *field = field.next();
                    TextOutcome::Edited
                } else {
                    let target = match field {
                        RunField::Name => name,
                        RunField::Ports => ports,
                        RunField::Env => env,
                        RunField::Volumes => volumes,
                        RunField::Command => command,
                    };
                    edit_text(target, event, cx)
                }
            }
            Some(Modal::PushImage { destination, .. }) => edit_text(destination, event, cx),
            Some(Modal::RegistryLogout { registry }) => edit_text(registry, event, cx),
            Some(Modal::RegistryLogin {
                field,
                registry,
                username,
                password,
            }) => {
                if event.keystroke.key.as_str() == "tab" {
                    *field = field.next();
                    TextOutcome::Edited
                } else {
                    let target = match field {
                        LoginField::Registry => registry,
                        LoginField::Username => username,
                        LoginField::Password => password,
                    };
                    edit_text(target, event, cx)
                }
            }
            Some(Modal::CreateVolume {
                field,
                name,
                driver,
            }) => {
                if event.keystroke.key.as_str() == "tab" {
                    *field = field.next();
                    TextOutcome::Edited
                } else {
                    let target = match field {
                        VolumeField::Name => name,
                        VolumeField::Driver => driver,
                    };
                    edit_text(target, event, cx)
                }
            }
            Some(Modal::CreateNetwork {
                field,
                name,
                driver,
                subnet,
                gateway,
            }) => {
                if event.keystroke.key.as_str() == "tab" {
                    *field = field.next();
                    TextOutcome::Edited
                } else {
                    let target = match field {
                        NetworkField::Name => name,
                        NetworkField::Driver => driver,
                        NetworkField::Subnet => subnet,
                        NetworkField::Gateway => gateway,
                    };
                    edit_text(target, event, cx)
                }
            }
            Some(Modal::CreatePod { field, name, ports }) => {
                if event.keystroke.key.as_str() == "tab" {
                    *field = field.next();
                    TextOutcome::Edited
                } else {
                    let target = match field {
                        PodField::Name => name,
                        PodField::Ports => ports,
                    };
                    edit_text(target, event, cx)
                }
            }
            Some(Modal::PlayKube { path }) => edit_text(path, event, cx),
            Some(Modal::CreateMachine {
                field,
                name,
                cpus,
                memory,
                disk,
            }) => {
                if event.keystroke.key.as_str() == "tab" {
                    *field = field.next();
                    TextOutcome::Edited
                } else {
                    let target = match field {
                        MachineField::Name => name,
                        MachineField::Cpus => cpus,
                        MachineField::Memory => memory,
                        MachineField::Disk => disk,
                    };
                    edit_text(target, event, cx)
                }
            }
            Some(_) => match event.keystroke.key.as_str() {
                "escape" => TextOutcome::Cancel,
                "enter" | "return" => TextOutcome::Submit,
                _ => TextOutcome::Ignored,
            },
            None => TextOutcome::Ignored,
        };

        match outcome {
            TextOutcome::Submit => {
                cx.stop_propagation();
                self.confirm_modal(window, cx)
            }
            TextOutcome::Cancel => {
                cx.stop_propagation();
                self.close_modal(window, cx)
            }
            TextOutcome::Edited => {
                self.caret_on = true;
                cx.notify();
            }
            TextOutcome::Ignored => cx.notify(),
        }
    }

    // -----------------------------------------------------------------------
    // Search
    // -----------------------------------------------------------------------

    fn search_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match edit_text(&mut self.search, event, cx) {
            TextOutcome::Cancel => {
                cx.stop_propagation();
                if self.search.is_empty() {
                    self.root_focus.focus(window);
                } else {
                    self.search.clear();
                }
                cx.notify();
            }
            TextOutcome::Edited => {
                // Keep the caret visible while the user is typing.
                self.caret_on = true;
                cx.notify();
            }
            TextOutcome::Submit | TextOutcome::Ignored => {}
        }
    }

    fn rebuild_filter(&mut self) {
        let query = self.search.trim();
        let indices = match self.tab {
            Tab::Containers => self
                .snapshot
                .containers
                .iter()
                .enumerate()
                .filter(|(_, container)| {
                    matches_query(
                        query,
                        &[
                            &container.name,
                            &container.image,
                            &container.id,
                            &container.state,
                            &container.status,
                        ],
                    )
                })
                .map(|(index, _)| index)
                .collect(),
            Tab::Compose => self
                .snapshot
                .compose
                .iter()
                .enumerate()
                .filter(|(_, project)| {
                    let files = project.config_files.join(" ");
                    let dir = project.working_dir.clone().unwrap_or_default();
                    let status = project.status_label();
                    let mut fields: Vec<&String> = vec![&project.name, &files, &dir, &status];
                    fields.extend(project.services.iter().map(|service| &service.name));
                    matches_query(query, &fields)
                })
                .map(|(index, _)| index)
                .collect(),
            Tab::Images => self
                .snapshot
                .images
                .iter()
                .enumerate()
                .filter(|(_, image)| matches_query(query, &[&image.name, &image.id, &image.digest]))
                .map(|(index, _)| index)
                .collect(),
            Tab::Pods => self
                .snapshot
                .pods
                .iter()
                .enumerate()
                .filter(|(_, pod)| matches_query(query, &[&pod.name, &pod.id, &pod.status]))
                .map(|(index, _)| index)
                .collect(),
            Tab::Volumes => self
                .snapshot
                .volumes
                .iter()
                .enumerate()
                .filter(|(_, volume)| {
                    matches_query(
                        query,
                        &[
                            &volume.name,
                            &volume.driver,
                            &volume.mountpoint,
                            &volume.scope,
                        ],
                    )
                })
                .map(|(index, _)| index)
                .collect(),
            Tab::Networks => self
                .snapshot
                .networks
                .iter()
                .enumerate()
                .filter(|(_, network)| {
                    matches_query(
                        query,
                        &[
                            &network.name,
                            &network.id,
                            &network.driver,
                            &network.interface,
                            &network.subnet_label(),
                        ],
                    )
                })
                .map(|(index, _)| index)
                .collect(),
            Tab::Machines => self
                .local
                .machines
                .iter()
                .enumerate()
                .filter(|(_, machine)| {
                    let state = machine.state_label().to_string();
                    matches_query(query, &[&machine.name, &machine.vm_type, &state])
                })
                .map(|(index, _)| index)
                .collect(),
            Tab::Connections => self
                .local
                .connections
                .iter()
                .enumerate()
                .filter(|(_, connection)| {
                    matches_query(
                        query,
                        &[&connection.name, &connection.uri, &connection.identity],
                    )
                })
                .map(|(index, _)| index)
                .collect(),
            Tab::Dashboard | Tab::Accounts | Tab::Settings => Vec::new(),
        };
        self.visible_indices = self.sorted(indices);
    }

    // -----------------------------------------------------------------------
    // Sorting
    // -----------------------------------------------------------------------

    fn toggle_sort(&mut self, column: usize, cx: &mut Context<Self>) {
        self.sort = Some(match self.sort {
            Some(state) if state.column == column => SortState {
                column,
                ascending: !state.ascending,
            },
            _ => SortState {
                column,
                ascending: true,
            },
        });
        cx.notify();
    }

    fn sorted(&self, indices: Vec<usize>) -> Vec<usize> {
        let Some(sort) = self.sort else {
            return indices;
        };
        let mut keyed: Vec<(SortKey, usize)> = indices
            .into_iter()
            .map(|index| (self.sort_key(index), index))
            .collect();
        keyed.sort_by(|a, b| a.0.compare(&b.0));
        if !sort.ascending {
            keyed.reverse();
        }
        keyed.into_iter().map(|(_, index)| index).collect()
    }

    /// Column indices are per tab, matching the non-compact table header.
    fn sort_key(&self, index: usize) -> SortKey {
        let column = self.sort.map(|state| state.column).unwrap_or(0);
        let text = |value: &str| SortKey::Text(value.to_string());
        let number = |value: u64| SortKey::Number(value);

        match self.tab {
            Tab::Containers => {
                let item = &self.snapshot.containers[index];
                match column {
                    1 => text(&item.image),
                    2 => text(&item.state),
                    3 => text(&item.ports),
                    4 => text(&item.status),
                    5 => text(&item.id),
                    _ => text(&item.name),
                }
            }
            Tab::Compose => {
                let item = &self.snapshot.compose[index];
                match column {
                    1 => text(&item.status_label()),
                    2 => number(item.total as u64),
                    3 => text(&item.config_label()),
                    _ => text(&item.name),
                }
            }
            Tab::Images => {
                let item = &self.snapshot.images[index];
                match column {
                    1 => text(&item.id),
                    2 => number(item.size),
                    3 => number(item.containers),
                    4 => text(&item.created),
                    _ => text(&item.name),
                }
            }
            Tab::Pods => {
                let item = &self.snapshot.pods[index];
                match column {
                    1 => text(&item.status),
                    2 => number(item.containers),
                    3 => text(&item.created),
                    4 => text(&item.id),
                    _ => text(&item.name),
                }
            }
            Tab::Volumes => {
                let item = &self.snapshot.volumes[index];
                match column {
                    1 => text(&item.driver),
                    2 => number(item.mounts),
                    3 => text(&item.scope),
                    4 => text(&item.created),
                    _ => text(&item.name),
                }
            }
            Tab::Networks => {
                let item = &self.snapshot.networks[index];
                match column {
                    1 => text(&item.driver),
                    2 => text(&item.subnet_label()),
                    3 => text(&item.interface),
                    4 => text(bool_label(item.dns)),
                    _ => text(&item.name),
                }
            }
            Tab::Machines => {
                let item = &self.local.machines[index];
                match column {
                    1 => text(item.state_label()),
                    2 => number(item.cpus),
                    3 => number(item.memory),
                    4 => number(item.disk_size),
                    5 => text(&item.last_up),
                    _ => text(&item.name),
                }
            }
            Tab::Connections => {
                let item = &self.local.connections[index];
                match column {
                    1 => text(&item.uri),
                    2 => text(&item.identity_label()),
                    3 => number(
                        (self.settings.connection.as_deref() == Some(item.name.as_str())) as u64,
                    ),
                    _ => text(&item.name),
                }
            }
            Tab::Dashboard | Tab::Accounts | Tab::Settings => text(""),
        }
    }

    fn rows_len(&self) -> usize {
        self.visible_indices.len()
    }

    // -----------------------------------------------------------------------
    // Small UI helpers
    // -----------------------------------------------------------------------

    fn subtitle(&self) -> String {
        let total = match self.tab {
            Tab::Containers => self.snapshot.containers.len(),
            Tab::Compose => self.snapshot.compose.len(),
            Tab::Images => self.snapshot.images.len(),
            Tab::Pods => self.snapshot.pods.len(),
            Tab::Volumes => self.snapshot.volumes.len(),
            Tab::Networks => self.snapshot.networks.len(),
            Tab::Machines => self.local.machines.len(),
            Tab::Connections => self.local.connections.len(),
            Tab::Accounts => self.accounts.len(),
            Tab::Dashboard | Tab::Settings => 0,
        };
        let shown = self.visible_indices.len();
        let noun = match self.tab {
            Tab::Containers => "container",
            Tab::Compose => "project",
            Tab::Images => "image",
            Tab::Pods => "pod",
            Tab::Volumes => "volume",
            Tab::Networks => "network",
            Tab::Machines => "machine",
            Tab::Connections => "connection",
            Tab::Accounts => "registry login",
            Tab::Dashboard | Tab::Settings => "resource",
        };
        if self.tab == Tab::Dashboard {
            return if self.snapshot.version.is_empty() {
                "resources, disk usage and engine details".to_string()
            } else {
                format!(
                    "resources, disk usage and engine details · {}",
                    self.snapshot.version
                )
            };
        }
        if self.tab == Tab::Settings {
            return "appearance, refresh, panels and engine details".to_string();
        }
        if self.tab == Tab::Accounts {
            let count = self.accounts.len();
            let plural = if count == 1 { "" } else { "s" };
            return if self.snapshot.version.is_empty() {
                format!("{count} registry login{plural}")
            } else {
                format!("{count} registry login{plural} · {}", self.snapshot.version)
            };
        }
        let plural = if total == 1 { "" } else { "s" };
        let count = if shown == total {
            format!("{total} {noun}{plural}")
        } else {
            format!("{shown} of {total} {noun}{plural}")
        };
        if self.snapshot.version.is_empty() {
            count
        } else {
            format!("{count} · {}", self.snapshot.version)
        }
    }

    fn refresh_label(&self) -> String {
        if self.loading {
            "Refreshing…".to_string()
        } else {
            match self.last_updated {
                Some(at) if at.elapsed() < Duration::from_secs(2) => "updated just now".to_string(),
                Some(at) if at.elapsed() < Duration::from_secs(60) => {
                    format!("updated {}s ago", at.elapsed().as_secs())
                }
                Some(at) => format!("updated {}m ago", at.elapsed().as_secs() / 60),
                None => "waiting for podman".to_string(),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
/// Lines from a `--tail` fetch that come before the oldest buffered line.
///
/// When timestamps are available they decide the cut; otherwise lines already
/// present are skipped by exact match.
fn older_log_lines(fetched: &str, existing: &[String]) -> Vec<String> {
    let first = existing
        .first()
        .and_then(|line| podman::log_timestamp(line));
    let seen: HashSet<&str> = existing.iter().map(String::as_str).collect();
    let mut older = Vec::new();
    for raw in fetched.lines() {
        let line = strip_ansi(raw);
        if line.trim().is_empty() {
            continue;
        }
        let keep = match (podman::log_timestamp(&line), first) {
            (Some(timestamp), Some(first)) => timestamp < first,
            _ => !seen.contains(line.as_str()),
        };
        if keep {
            older.push(line);
        }
    }
    older
}

fn inspect_kind(selection: &Selection) -> Option<podman::InspectKind> {
    match selection {
        Selection::Container(_) => Some(podman::InspectKind::Container),
        Selection::Image(_) => Some(podman::InspectKind::Image),
        Selection::Pod(_) => Some(podman::InspectKind::Pod),
        Selection::Volume(_) => Some(podman::InspectKind::Volume),
        Selection::Network(_) => Some(podman::InspectKind::Network),
        Selection::Compose(_) | Selection::Machine(_) | Selection::Connection(_) => None,
    }
}

impl Render for PodmanDesktop {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.rebuild_filter();
        self.viewport_width = f32::from(window.bounds().size.width);
        if !self.initial_focus_done {
            self.initial_focus_done = true;
            window.focus(&self.root_focus);
        }

        let mut column = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .child(self.header(cx));
        column = match self.tab {
            Tab::Dashboard => column.child(self.dashboard_view()),
            Tab::Accounts => column.child(self.accounts_view(cx)),
            Tab::Settings => column.child(self.settings_view(cx)),
            _ => column
                .child(self.toolbar(window, cx))
                .child(self.table_header(cx))
                .child(self.body(cx)),
        };

        let mut main_view = div()
            .flex()
            .flex_1()
            .min_h(px(0.))
            .child(column.child(self.output_panel(cx)));

        if self.selection.is_some() {
            main_view = main_view
                .child(self.resizer(DragTarget::Details, cx))
                .child(self.details_panel(cx));
        }

        let mut root = div()
            .relative()
            .flex()
            .h_full()
            .w_full()
            .bg(rgb(theme().bg))
            .text_color(rgb(theme().text))
            .track_focus(&self.root_focus)
            .on_key_down(cx.listener(Self::root_key))
            .on_mouse_move(cx.listener(Self::resize_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.end_resize(cx)),
            )
            .child(self.sidebar(cx))
            .child(self.resizer(DragTarget::Sidebar, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .min_w(px(0.))
                    .child(main_view)
                    .child(self.status_bar()),
            );

        if !self.toasts.is_empty() {
            root = root.child(self.toasts_view());
        }
        if self.modal.is_some() {
            root = root.child(self.modal_overlay(cx));
        }
        if self.context_menu.is_some() {
            root = root.child(self.context_menu_view(cx));
        }

        root
    }
}

// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarizes_ports() {
        assert_eq!(ports_summary("-"), "-");
        assert_eq!(ports_summary("5432->5432/tcp"), "5432->5432/tcp");
        assert_eq!(
            ports_summary("4222->4222/tcp, 6222->6222/tcp, 8222->8222/tcp"),
            "4222->4222/tcp +2"
        );
    }

    #[test]
    fn keeps_only_older_timestamped_lines() {
        let existing = vec![
            "2026-11-05T10:00:03Z live".to_string(),
            "2026-11-05T10:00:04Z newer".to_string(),
        ];
        let fetched = "2026-11-05T10:00:01Z a\n2026-11-05T10:00:02Z b\n\
                       2026-11-05T10:00:03Z live\n2026-11-05T10:00:04Z newer\n";
        assert_eq!(
            older_log_lines(fetched, &existing),
            vec!["2026-11-05T10:00:01Z a", "2026-11-05T10:00:02Z b"]
        );
    }

    #[test]
    fn dedupes_untimestamped_older_lines() {
        let existing = vec!["keep me".to_string()];
        let fetched = "older line\nkeep me\n";
        assert_eq!(older_log_lines(fetched, &existing), vec!["older line"]);

        // Without a first line there is nothing to compare against.
        assert_eq!(older_log_lines("one\ntwo\n", &[]), vec!["one", "two"]);
    }

    #[test]
    fn truncates_with_ellipsis_budget() {
        assert_eq!(truncate_text("short", 100.0, 8.0), "short");
        assert_eq!(
            truncate_text("abcdefghijklmnopqrstuvwxyz", 80.0, 8.0),
            "abcdefghi…"
        );
        // Unicode counts characters, not bytes (budget has a floor of 3).
        assert_eq!(truncate_text("ééééé", 16.0, 8.0), "éé…");
        // Tiny widths still keep an ellipsis instead of disappearing.
        assert_eq!(truncate_text("abcdef", 1.0, 8.0), "ab…");
    }

    #[test]
    fn filters_case_insensitively() {
        let name = "spamurai-postgres-dev".to_string();
        let image = "docker.io/timescale/timescaledb:latest-pg15".to_string();
        let fields = [&name, &image];

        assert!(matches_query("", &fields));
        assert!(matches_query("postgres", &fields));
        assert!(matches_query("TIMESCALE", &fields));
        assert!(!matches_query("redis", &fields));
    }

    #[test]
    fn strips_ansi_sequences() {
        assert_eq!(strip_ansi("\u{1b}[31mred\u{1b}[0m text"), "red text");
        assert_eq!(strip_ansi("plain"), "plain");
    }

    #[test]
    fn computes_keyboard_positions() {
        assert_eq!(next_position(None, 0, 1), None);
        assert_eq!(next_position(None, 5, 1), Some(0));
        assert_eq!(next_position(None, 5, -1), Some(4));
        assert_eq!(next_position(Some(2), 5, 1), Some(3));
        assert_eq!(next_position(Some(4), 5, 1), Some(4));
        assert_eq!(next_position(Some(0), 5, -1), Some(0));
    }
}
