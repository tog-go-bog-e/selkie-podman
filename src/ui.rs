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
mod loading;
mod modals;
mod navigation;
mod output;
mod preferences;
mod resizer;
mod rows;
mod search;
mod selection;
pub(crate) mod settings;
mod sidebar;
mod sorting;
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
    Secret, Snapshot, StatsSample, Volume, VolumeDetails,
};
use crate::terminal::TerminalSession;

use settings::Settings;
use theme::{set_kind, theme};

use helpers::*;

const TOAST_TTL: Duration = Duration::from_secs(5);
const MAX_LOG_LINES: usize = 800;
/// Most log lines applied in one UI update; the rest wait for the next tick.
const LOG_BATCH: usize = 5_000;
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
/// How long machine and connection lists are reused by background refreshes.
/// `podman machine list` queries every VM and is slow on some hosts; it is
/// re-read sooner on the Machines/Connections pages and after any action.
const LOCAL_TTL: Duration = Duration::from_secs(15);

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
    Secrets,
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
    /// A Podman secret, by ID.
    Secret(String),
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

/// One `KEY=VALUE` row in the build dialog.
#[derive(Clone, Default)]
struct BuildArg {
    key: String,
    value: String,
}

/// A well-known platform offered as a card in the build dialog.
struct PlatformPreset {
    /// The value passed to `--platform` (e.g. `linux/amd64`).
    platform: &'static str,
    description: &'static str,
    /// Short architecture label shown in the card's chip.
    arch: &'static str,
}

const BUILD_PLATFORMS: [PlatformPreset; 5] = [
    PlatformPreset {
        platform: "linux/amd64",
        description: "Intel and AMD x86_64 systems",
        arch: "AMD64",
    },
    PlatformPreset {
        platform: "linux/arm64",
        description: "ARM® aarch64 systems",
        arch: "ARM64",
    },
    PlatformPreset {
        platform: "linux/ppc64le",
        description: "Power ppc64le systems",
        arch: "PPC64LE",
    },
    PlatformPreset {
        platform: "linux/s390x",
        description: "IBM s390x zsystems",
        arch: "S390X",
    },
    PlatformPreset {
        platform: "wasi/wasm",
        description: "WebAssembly",
        arch: "WASI",
    },
];

/// The platform Podman builds for when `--platform` is omitted: the host's.
fn default_platform() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "linux/arm64",
        "powerpc64" => "linux/ppc64le",
        "s390x" => "linux/s390x",
        "wasm32" => "wasi/wasm",
        _ => "linux/amd64",
    }
}

/// Which build-dialog input has the keyboard.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BuildField {
    Containerfile,
    Tag,
    Context,
    /// Key cell of build-argument row `index`.
    ArgKey(usize),
    /// Value cell of build-argument row `index`.
    ArgValue(usize),
    /// The inline “New platform” input.
    NewPlatform,
}

/// State of the build dialog: everything the form needs, including which
/// fields are expanded and which one has focus.
#[derive(Clone)]
struct BuildForm {
    containerfile: String,
    tag: String,
    context: String,
    args: Vec<BuildArg>,
    /// Selected target platforms; empty uses the engine default.
    platforms: Vec<String>,
    /// Platforms added through “New platform”, shown as extra cards.
    custom_platforms: Vec<String>,
    /// Text typed into the “New platform” card.
    new_platform: String,
    new_platform_open: bool,
    /// Check the Containerfile's registries are known before building.
    validate_registries: bool,
    /// Whether the advanced fields are expanded.
    show_options: bool,
    field: BuildField,
}

impl Default for BuildForm {
    fn default() -> Self {
        Self {
            containerfile: String::new(),
            tag: String::new(),
            context: String::new(),
            // The dialog always shows one empty row, like upstream.
            args: vec![BuildArg::default()],
            platforms: vec![default_platform().to_string()],
            custom_platforms: Vec::new(),
            new_platform: String::new(),
            new_platform_open: false,
            validate_registries: true,
            show_options: false,
            field: BuildField::Containerfile,
        }
    }
}

impl BuildForm {
    fn selected(&self, platform: &str) -> bool {
        self.platforms.iter().any(|value| value == platform)
    }

    fn toggle_platform(&mut self, platform: &str) {
        match self.platforms.iter().position(|value| value == platform) {
            Some(index) => {
                self.platforms.remove(index);
            }
            None => self.platforms.push(platform.to_string()),
        }
    }

    /// Add the platform typed into the “New platform” card.
    fn add_custom_platform(&mut self) -> Result<(), String> {
        let platform = podman::validate_platform(&self.new_platform)?.to_string();
        if !self.selected(&platform) {
            self.platforms.push(platform.clone());
            if !BUILD_PLATFORMS
                .iter()
                .any(|preset| preset.platform == platform)
            {
                self.custom_platforms.push(platform);
            }
        }
        self.new_platform.clear();
        self.new_platform_open = false;
        self.field = BuildField::Tag;
        Ok(())
    }

    fn arg_tokens(&self) -> Result<Vec<String>, String> {
        let rows: Vec<(String, String)> = self
            .args
            .iter()
            .map(|arg| (arg.key.clone(), arg.value.clone()))
            .collect();
        podman::build_args_from_rows(&rows)
    }

    /// Inputs in Tab order. The advanced fields only participate when visible.
    fn fields(&self) -> Vec<BuildField> {
        let mut fields = vec![BuildField::Containerfile, BuildField::Tag];
        if self.show_options {
            fields.push(BuildField::Context);
            for index in 0..self.args.len() {
                fields.push(BuildField::ArgKey(index));
                fields.push(BuildField::ArgValue(index));
            }
            if self.new_platform_open {
                fields.push(BuildField::NewPlatform);
            }
        }
        fields
    }

    fn next_field(&self) -> BuildField {
        let fields = self.fields();
        let current = fields
            .iter()
            .position(|field| *field == self.field)
            .unwrap_or(0);
        fields[(current + 1) % fields.len()]
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RunField {
    Name,
    Ports,
    Env,
    Volumes,
    Secrets,
    Command,
}

impl RunField {
    const ALL: [RunField; 6] = [
        RunField::Name,
        RunField::Ports,
        RunField::Env,
        RunField::Volumes,
        RunField::Secrets,
        RunField::Command,
    ];

    fn index(self) -> usize {
        match self {
            RunField::Name => 0,
            RunField::Ports => 1,
            RunField::Env => 2,
            RunField::Volumes => 3,
            RunField::Secrets => 4,
            RunField::Command => 5,
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
enum SecretField {
    Name,
    Data,
}

impl SecretField {
    const ALL: [SecretField; 2] = [SecretField::Name, SecretField::Data];

    fn index(self) -> usize {
        match self {
            SecretField::Name => 0,
            SecretField::Data => 1,
        }
    }

    fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(SecretField::Name)
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
    DeleteSecret {
        id: String,
        name: String,
    },
    ComposeDown {
        project: ComposeProject,
    },
    StopMachine {
        name: String,
    },
    BuildImage {
        form: BuildForm,
    },
    RunContainer {
        image: String,
        name: String,
        ports: String,
        env: String,
        volumes: String,
        /// `name`, `name:/path` or `name:VAR` tokens.
        secrets: String,
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
    CreateSecret {
        name: String,
        /// Never shown: the dialog renders a mask.
        data: String,
        field: SecretField,
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
            secrets: String::new(),
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

    fn create_secret() -> Self {
        Modal::CreateSecret {
            name: String::new(),
            data: String::new(),
            field: SecretField::Name,
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

/// The `⋮` menu shown on a credential row.
struct AccountMenuState {
    position: Point<Pixels>,
    registry: String,
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
    // If the app dies between fork and `prctl`, the signal is lost; checking
    // the parent afterwards closes that window (as in `terminal.rs`).
    let parent = std::process::id() as libc::pid_t;
    // SAFETY: runs in the forked child before exec; `prctl`, `getppid` and
    // `_exit` are async-signal-safe.
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                libc::_exit(1);
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
    /// Last window height, used to keep dialogs inside the window.
    viewport_height: f32,
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

    account_menu: Option<AccountMenuState>,

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
    /// When machines and connections were last read (see [`LOCAL_TTL`]).
    local_fetched: Option<Instant>,

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
            viewport_height: 0.0,
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
            account_menu: None,
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
            local_fetched: None,
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

        view.load_accounts();
        view
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
            Tab::Secrets => self.snapshot.secrets.len(),
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
            Tab::Secrets => "secret",
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
            return "configuration, registries and engine details".to_string();
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
        Selection::Secret(_) => Some(podman::InspectKind::Secret),
        Selection::Compose(_) | Selection::Machine(_) | Selection::Connection(_) => None,
    }
}

impl Render for PodmanDesktop {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.rebuild_filter();
        self.viewport_width = f32::from(window.bounds().size.width);
        self.viewport_height = f32::from(window.bounds().size.height);
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
            Tab::Accounts => column.child(self.registries_view(cx)),
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
        if self.account_menu.is_some() {
            root = root.child(self.account_menu_view(cx));
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
        assert_eq!(strip_ansi("\u{1b}]0;title\u{7}after"), "after");
        assert_eq!(
            strip_ansi("\u{1b}]8;;https://example.com\u{1b}\\link\u{1b}]8;;\u{1b}\\"),
            "link"
        );
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

    #[test]
    fn capped_line_reader_splits_long_lines() {
        let input = format!("short\r\n{}\nend", "x".repeat(10));
        let mut reader = std::io::BufReader::new(input.as_bytes());
        assert_eq!(read_capped_line(&mut reader, 4).as_deref(), Some("shor"));
        assert_eq!(read_capped_line(&mut reader, 64).as_deref(), Some("t"));
        assert_eq!(
            read_capped_line(&mut reader, 64).as_deref(),
            Some("xxxxxxxxxx")
        );
        assert_eq!(read_capped_line(&mut reader, 64).as_deref(), Some("end"));
        assert_eq!(read_capped_line(&mut reader, 64), None);
    }
}
