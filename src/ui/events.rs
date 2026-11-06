use std::process::{Child, Stdio};
use std::sync::mpsc::{self, TryRecvError};
use std::time::{Duration, Instant};

use serde::Deserialize;

use super::*;

/// How long to wait after the last event before refreshing, so bursts (a start
/// emits init/start in quick succession) produce one refresh.
const EVENT_COALESCE: Duration = Duration::from_millis(300);
/// Upper bound on coalescing while events keep arriving.
const EVENT_MAX_WAIT: Duration = Duration::from_secs(1);
/// Delay before restarting the stream after it ends or fails to start.
const EVENT_RETRY: Duration = Duration::from_secs(3);

/// Kills the `podman events` client when the supervisor task is cancelled.
struct EventChild(Option<Child>);

impl Drop for EventChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[derive(Deserialize)]
struct RawEvent {
    #[serde(default, rename = "Status")]
    status: String,
}

/// Health checks and exec sessions do not change the snapshot, but they can be
/// frequent; ignoring them keeps event-driven refreshes meaningful.
fn is_ignored_event(line: &str) -> bool {
    match serde_json::from_str::<RawEvent>(line) {
        Ok(event) => matches!(event.status.as_str(), "health_status" | "exec" | "exec_die"),
        Err(_) => false,
    }
}

impl PodmanDesktop {
    // -----------------------------------------------------------------------
    // Event-driven updates
    // -----------------------------------------------------------------------

    /// Watch `podman events` and refresh when something changes. The supervisor
    /// reconnects automatically; periodic polling remains as a safety net.
    pub(super) fn start_events(&mut self, cx: &mut Context<Self>) {
        self.stop_events();

        let task = cx.spawn(async move |this, cx| loop {
            let mut command = podman::base_command();
            command
                .args(["events", "--format", "json"])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            kill_with_parent(&mut command);

            let mut child = match command.spawn() {
                Ok(child) => child,
                Err(_) => {
                    if this
                        .update(cx, |this, cx| this.set_events_connected(false, cx))
                        .is_err()
                    {
                        break;
                    }
                    cx.background_executor().timer(EVENT_RETRY).await;
                    continue;
                }
            };

            if this
                .update(cx, |this, cx| this.set_events_connected(true, cx))
                .is_err()
            {
                break;
            }

            let (sender, receiver) = mpsc::channel::<String>();
            if let Some(stdout) = child.stdout.take() {
                spawn_line_reader(stdout, sender.clone());
            }
            drop(sender);
            let guard = EventChild(Some(child));

            let mut pending: Option<(Instant, Instant)> = None;
            let mut disconnected = false;
            loop {
                if !disconnected {
                    loop {
                        match receiver.try_recv() {
                            Ok(line) => {
                                if is_ignored_event(&line) {
                                    continue;
                                }
                                let now = Instant::now();
                                pending = Some(match pending {
                                    Some((first, _)) => (first, now),
                                    None => (now, now),
                                });
                            }
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => {
                                disconnected = true;
                                break;
                            }
                        }
                    }
                }

                if pending.is_some_and(|(first, last)| {
                    last.elapsed() >= EVENT_COALESCE || first.elapsed() >= EVENT_MAX_WAIT
                }) {
                    pending = None;
                    if this.update(cx, |this, cx| this.refresh(cx)).is_err() {
                        break;
                    }
                }

                if disconnected {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
            }

            if this
                .update(cx, |this, cx| this.set_events_connected(false, cx))
                .is_err()
            {
                break;
            }
            drop(guard);
            cx.background_executor().timer(EVENT_RETRY).await;
        });

        self.events_task = Some(task);
    }

    pub(super) fn stop_events(&mut self) {
        // Cancelling the task drops the guard, which kills the events client.
        self.events_task = None;
    }

    fn set_events_connected(&mut self, connected: bool, cx: &mut Context<Self>) {
        if self.events_connected != connected {
            self.events_connected = connected;
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_noisy_events() {
        assert!(is_ignored_event(
            r#"{"Status":"health_status","Type":"container"}"#
        ));
        assert!(is_ignored_event(r#"{"Status":"exec","Type":"container"}"#));
        assert!(is_ignored_event(
            r#"{"Status":"exec_die","Type":"container"}"#
        ));
        assert!(!is_ignored_event(
            r#"{"Status":"start","Type":"container"}"#
        ));
        assert!(!is_ignored_event("not json"));
    }
}
