//! Images: listing, history, the file browser, pull/tag and `podman build`.

use super::*;

#[derive(Debug, Clone, Default)]
pub struct Image {
    pub id: String,
    pub name: String,
    pub digest: String,
    pub size: u64,
    pub containers: u64,
    pub created: String,
}

impl Image {
    pub fn id_short(&self) -> String {
        short_id(&self.id)
    }

    /// `sha256:abc…` shortened to `abc…`, or an empty string when unknown.
    pub fn digest_short(&self) -> String {
        let digest = self.digest.strip_prefix("sha256:").unwrap_or(&self.digest);
        if digest.is_empty() {
            String::new()
        } else {
            format!("digest {}", short_id(digest))
        }
    }

    pub fn size_label(&self) -> String {
        human_size(self.size)
    }
}

pub fn parse_images(json: &str) -> Result<Vec<Image>, String> {
    let raw: Vec<RawImage> = parse_json(json, "podman images")?;
    // `podman images --format json` emits one entry per repo tag for images
    // with several names, so deduplicate by image ID.
    let mut seen = HashSet::new();
    Ok(raw
        .into_iter()
        .filter(|raw| !raw.id.is_empty() && seen.insert(raw.id.clone()))
        .map(|raw| {
            let name = first_non_empty(raw.names.as_deref())
                .or_else(|| first_non_empty(raw.repo_tags.as_deref()))
                .unwrap_or_else(|| "<none>".to_string());
            Image {
                id: raw.id,
                name,
                digest: raw.digest.unwrap_or_default(),
                size: raw.size.unwrap_or_default(),
                containers: raw.containers.unwrap_or_default(),
                created: format_timestamp(raw.created_at.as_deref()),
            }
        })
        .collect())
}

pub fn remove_image(id: &str) -> Result<String, String> {
    run(&["rmi", "-f", id])
}

// ---------------------------------------------------------------------------
// Image history and file browser
// ---------------------------------------------------------------------------

/// One layer in `podman history`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageHistoryEntry {
    pub id: String,
    pub created: String,
    pub created_by: String,
    pub size: u64,
    pub comment: String,
}

impl ImageHistoryEntry {
    /// The base layer has no image ID (`<missing>` in the CLI output).
    pub fn id_label(&self) -> String {
        if self.id.is_empty() || self.id == "<missing>" {
            "-".to_string()
        } else {
            short_id(&self.id)
        }
    }

    pub fn size_label(&self) -> String {
        if self.size == 0 {
            "-".to_string()
        } else {
            human_size(self.size)
        }
    }
}

pub fn image_history(image: &str) -> Result<Vec<ImageHistoryEntry>, String> {
    let json = query(&["history", "--format", "json", image])?;
    parse_image_history(&json)
}

pub(super) fn parse_image_history(json: &str) -> Result<Vec<ImageHistoryEntry>, String> {
    let raw: Vec<RawHistory> = parse_json(json, "podman history")?;
    Ok(raw
        .into_iter()
        .map(|row| ImageHistoryEntry {
            id: row.id,
            created: format_timestamp(Some(row.created.as_str())),
            created_by: row.created_by.trim().to_string(),
            size: row.size,
            comment: row.comment,
        })
        .collect())
}

#[derive(Deserialize)]
pub(super) struct RawHistory {
    #[serde(default)]
    id: String,
    #[serde(default)]
    created: String,
    #[serde(default, rename = "CreatedBy")]
    created_by: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    comment: String,
}

/// How much of a file the browser reads before treating it as binary.
pub const IMAGE_FILE_LIMIT: usize = 128 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFileKind {
    File,
    Directory,
    Link,
    Other,
}

/// One path inside an image's filesystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageFile {
    pub path: String,
    pub kind: ImageFileKind,
    pub size: u64,
    /// Symlink target, when `kind` is [`ImageFileKind::Link`].
    pub target: Option<String>,
}

impl ImageFile {
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    /// Empty for entries directly below the root.
    pub fn parent(&self) -> &str {
        self.path.rsplit_once('/').map_or("", |(parent, _)| parent)
    }
}

/// List an image's filesystem.
///
/// `podman image mount` needs root or `podman unshare`, so the browser creates a
/// throwaway container and streams `podman export` through `tar`. Nothing is
/// written to disk, and the container is removed when the listing finishes.
pub fn image_files(image: &str) -> Result<Vec<ImageFile>, String> {
    let output = export_tar(image, &["-tvf", "-"], None)?;
    Ok(parse_tar_listing(&String::from_utf8_lossy(&output)))
}

/// Read one file out of the image. The returned bytes stop at `limit`.
pub fn image_file_content(image: &str, path: &str, limit: usize) -> Result<Vec<u8>, String> {
    let path = validate_archive_path(path)?;
    export_tar(image, &["-xOf", "-", "--", path], Some(limit))
}

pub(super) fn validate_archive_path(path: &str) -> Result<&str, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("path cannot be empty".to_string());
    }
    if path.starts_with('-') {
        return Err("path cannot start with '-'".to_string());
    }
    if path.split('/').any(|part| part == "..") {
        return Err("path cannot contain '..'".to_string());
    }
    Ok(path)
}

/// Read at most `limit` bytes from `reader`.
///
/// Returns the bytes and whether the limit was hit before EOF, in which case
/// the caller stops the producer instead of buffering the rest.
pub(super) fn read_bounded(
    reader: &mut impl Read,
    limit: usize,
) -> std::io::Result<(Vec<u8>, bool)> {
    let mut bytes = Vec::new();
    // One extra byte tells "exactly `limit`" apart from "more than `limit`".
    reader.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    let truncated = bytes.len() > limit;
    bytes.truncate(limit);
    Ok((bytes, truncated))
}

/// Pipe `podman export <temp container>` into `tar` with `tar_args`.
///
/// With `limit`, tar's output is read only up to that many bytes; once it is
/// reached both processes are stopped, so previewing a large file neither
/// buffers it in memory nor streams the rest of the image.
pub(super) fn export_tar(
    image: &str,
    tar_args: &[&str],
    limit: Option<usize>,
) -> Result<Vec<u8>, String> {
    let container = TempContainer::create(image)?;

    let mut export = base_command();
    export
        .args(["export", &container.name])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut export_child = export
        .spawn()
        .map_err(|error| format!("could not run podman export: {error}"))?;
    let stdout = export_child
        .stdout
        .take()
        .ok_or_else(|| "podman export produced no output".to_string())?;
    // Read stderr concurrently so a noisy export cannot block on a full pipe.
    let export_stderr = export_child.stderr.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = pipe.read_to_string(&mut text);
            text
        })
    });

    let mut tar = Command::new("tar");
    tar.args(tar_args)
        .env("LC_ALL", "C")
        .stdin(Stdio::from(stdout))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut tar_child = match tar.spawn() {
        Ok(child) => child,
        Err(error) => {
            // Do not leave `podman export` blocked on a closed pipe.
            let _ = export_child.kill();
            let _ = export_child.wait();
            return Err(if error.kind() == std::io::ErrorKind::NotFound {
                "`tar` was not found in PATH (needed for the image file browser)".to_string()
            } else {
                format!("could not run tar: {error}")
            });
        }
    };
    let tar_stderr = tar_child.stderr.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = pipe.read_to_string(&mut text);
            text
        })
    });

    let mut tar_stdout = tar_child
        .stdout
        .take()
        .ok_or_else(|| "tar produced no output".to_string())?;
    let read = match limit {
        Some(limit) => read_bounded(&mut tar_stdout, limit),
        None => {
            let mut bytes = Vec::new();
            tar_stdout.read_to_end(&mut bytes).map(|_| (bytes, false))
        }
    };
    drop(tar_stdout);
    let (bytes, truncated) = match read {
        Ok(result) => result,
        Err(error) => {
            let _ = tar_child.kill();
            let _ = export_child.kill();
            let _ = tar_child.wait();
            let _ = export_child.wait();
            return Err(format!("could not read tar output: {error}"));
        }
    };

    if truncated {
        // We have all we will show; the exit statuses of the stopped
        // processes say nothing about the data.
        let _ = tar_child.kill();
        let _ = export_child.kill();
        let _ = tar_child.wait();
        let _ = export_child.wait();
        return Ok(bytes);
    }

    let tar_status = tar_child
        .wait()
        .map_err(|error| format!("tar failed: {error}"))?;
    if !tar_status.success() {
        // tar stopped early (bad arguments, unreadable archive): kill the writer
        // instead of waiting for it forever.
        let _ = export_child.kill();
    }

    let export_status = export_child
        .wait()
        .map_err(|error| format!("podman export failed: {error}"))?;
    if !export_status.success() && tar_status.success() {
        let stderr = export_stderr
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default();
        let stderr = stderr.trim();
        return Err(if stderr.is_empty() {
            format!("`podman export {image}` exited with {export_status}")
        } else {
            stderr.to_string()
        });
    }

    if !tar_status.success() {
        let stderr = tar_stderr
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default();
        let stderr = stderr.trim();
        return Err(if stderr.is_empty() {
            format!("`tar {}` exited with {}", tar_args.join(" "), tar_status)
        } else {
            stderr.to_string()
        });
    }

    Ok(bytes)
}

/// A container that only exists so `podman export` has something to read.
pub(super) struct TempContainer {
    name: String,
}

impl TempContainer {
    fn create(image: &str) -> Result<Self, String> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            "pd-browse-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );

        let mut command = base_command();
        // The command is never run; it only satisfies `podman create`.
        command.args(["create", "--name", &name, image, "/bin/true"]);
        let output = command
            .output()
            .map_err(|error| format!("could not run podman create: {error}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(if stderr.is_empty() {
                format!("`podman create {image}` exited with {}", output.status)
            } else {
                stderr
            });
        }
        Ok(Self { name })
    }
}

impl Drop for TempContainer {
    fn drop(&mut self) {
        let mut command = base_command();
        command.args(["rm", "-f", &self.name]);
        let _ = command.output();
    }
}

/// Parse `tar -tv` output into entries.
///
/// GNU tar's verbose format is `mode owner size date time name`, with ` -> ` for
/// symlinks. Names may contain spaces, so only the first five whitespace-separated
/// fields are split off and the remainder is the name.
pub fn parse_tar_listing(text: &str) -> Vec<ImageFile> {
    let mut files = Vec::new();
    let mut directories: HashSet<String> = HashSet::new();

    for line in text.lines().take(200_000) {
        let Some([mode, _owner, size, _date, _time, rest]) = tar_line_fields(line) else {
            continue;
        };
        let kind = match mode.chars().next().unwrap_or('-') {
            'd' => ImageFileKind::Directory,
            'l' | 'h' => ImageFileKind::Link,
            '-' => ImageFileKind::File,
            _ => ImageFileKind::Other,
        };
        let (name, target) = match rest.split_once(" -> ") {
            Some((name, target)) => (name, Some(target.trim().to_string())),
            None => (rest, None),
        };
        let cleaned = name.trim_start_matches("./").trim_end_matches('/');
        if cleaned.is_empty() || cleaned == "." {
            continue;
        }
        let kind = if name.ends_with('/') {
            ImageFileKind::Directory
        } else {
            kind
        };
        if kind == ImageFileKind::Directory {
            directories.insert(cleaned.to_string());
        }
        files.push(ImageFile {
            path: cleaned.to_string(),
            kind,
            size: size.parse().unwrap_or(0),
            target,
        });
    }

    // Make sure every path is reachable even when the archive omits a directory
    // entry for an implied parent.
    let mut synthesized = Vec::new();
    for file in &files {
        let mut parent = file.parent();
        while !parent.is_empty() {
            if directories.insert(parent.to_string()) {
                synthesized.push(parent.to_string());
            }
            parent = parent.rsplit_once('/').map_or("", |(rest, _)| rest);
        }
    }
    files.extend(synthesized.into_iter().map(|path| ImageFile {
        path,
        kind: ImageFileKind::Directory,
        size: 0,
        target: None,
    }));

    files
}

pub(super) fn tar_line_fields(line: &str) -> Option<[&str; 6]> {
    let mut rest = line;
    let mut fields = [""; 6];
    for slot in fields.iter_mut().take(5) {
        rest = rest.trim_start();
        if rest.is_empty() {
            return None;
        }
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        *slot = &rest[..end];
        rest = &rest[end..];
    }
    fields[5] = rest.trim_start();
    if fields[5].is_empty() {
        return None;
    }
    Some(fields)
}

pub fn pull_command(reference: &str) -> Result<Command, String> {
    let reference = validate_image_reference(reference)?;
    let mut command = base_command();
    command.args(["pull", reference]);
    Ok(command)
}

pub fn tag_image(id: &str, target: &str) -> Result<String, String> {
    let target = validate_image_reference(target)?;
    run(&["tag", id, target])
}

#[derive(Debug, Clone, Default)]
pub struct BuildOptions {
    pub containerfile: String,
    pub tag: String,
    /// Empty/None uses the Containerfile's directory.
    pub context: Option<String>,
    /// Empty/None uses the engine default.
    pub platform: Option<String>,
    /// Already parsed `KEY=VALUE` (or `KEY`) tokens.
    pub args: Vec<String>,
}

/// Build an image from a Containerfile, using its directory as the default
/// build context.
pub fn build_command(options: &BuildOptions) -> Result<Command, String> {
    let containerfile = options.containerfile.trim();
    if containerfile.is_empty() {
        return Err("Containerfile path cannot be empty".to_string());
    }
    let tag = validate_image_reference(&options.tag)?;

    let context = match options
        .context
        .as_deref()
        .map(str::trim)
        .filter(|context| !context.is_empty())
    {
        Some(context) => validate_build_context(context)?.to_string(),
        None => default_build_context(containerfile),
    };
    let platform = options
        .platform
        .as_deref()
        .map(str::trim)
        .filter(|platform| !platform.is_empty());
    if let Some(platform) = platform {
        validate_platform(platform)?;
    }
    for arg in &options.args {
        validate_build_arg(arg)?;
    }

    let mut command = base_command();
    command.args(["build", "-f", containerfile, "-t", tag]);
    if let Some(platform) = platform {
        command.args(["--platform", platform]);
    }
    for arg in &options.args {
        command.args(["--build-arg", arg]);
    }
    command.arg(&context);
    Ok(command)
}

pub(super) fn default_build_context(containerfile: &str) -> String {
    std::path::Path::new(containerfile)
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".".to_string())
}

pub fn validate_build_context(value: &str) -> Result<&str, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("build context cannot be empty".to_string());
    }
    if !std::path::Path::new(value).is_dir() {
        return Err(format!("build context is not a directory: {value}"));
    }
    Ok(value)
}

/// Accept `os/arch` or `os/arch/variant` with the characters Podman allows.
pub fn validate_platform(value: &str) -> Result<&str, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("platform cannot be empty".to_string());
    }
    if value.starts_with('-') {
        return Err("platform cannot start with '-'".to_string());
    }
    if !value.contains('/') {
        return Err("platform must include an architecture (e.g. linux/amd64)".to_string());
    }
    let valid = value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '-' | '_' | '.'));
    if !valid {
        return Err(format!("invalid platform: {value}"));
    }
    Ok(value)
}

/// Split whitespace-separated `KEY=VALUE` build args and validate each token.
pub fn parse_build_args(value: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    for token in value.split_whitespace() {
        validate_build_arg(token)?;
        args.push(token.to_string());
    }
    Ok(args)
}

pub(super) fn validate_build_arg(token: &str) -> Result<(), String> {
    if token.starts_with('-') {
        return Err(format!("invalid build arg: {token}"));
    }
    let key = token.split('=').next().unwrap_or_default();
    let valid_key = !key.is_empty()
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-'));
    if !valid_key {
        return Err(format!("invalid build arg: {token} (expected KEY=VALUE)"));
    }
    Ok(())
}

pub fn validate_image_reference(value: &str) -> Result<&str, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("image reference cannot be empty".to_string());
    }
    if value.starts_with('-') {
        return Err("image reference cannot start with '-'".to_string());
    }
    if value.chars().any(char::is_whitespace) {
        return Err("image reference cannot contain whitespace".to_string());
    }
    Ok(value)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct RawImage {
    id: String,
    #[serde(default)]
    names: Option<Vec<String>>,
    #[serde(default)]
    repo_tags: Option<Vec<String>>,
    #[serde(default)]
    digest: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    containers: Option<u64>,
    #[serde(default)]
    created_at: Option<String>,
}
