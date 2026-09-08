use crate::config::{SourceCfg, SourceType};
use anyhow::{Context as _, Result, bail};
use std::{path::Path, sync::LazyLock};

/// Shared client for HTTP sources: connection pooling and one consistent
/// timeout/redirect policy, instead of a new connection (and TLS handshake)
/// per fetch (`reqwest::get` builds and tears down its own client every
/// call).
static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);

/// A fetchable data source. Enum-dispatched by config `type`;
/// new types = new variant + a match arm here and in [`build`](fn@build).
#[derive(Clone)]
pub enum SourceKind {
    Http { url: String, selector: String },
    Script { command: String },
}

/// Validates type-specific params at startup (spec: source-configuration).
pub fn build(cfg: &SourceCfg) -> Result<SourceKind> {
    match cfg.kind {
        SourceType::Http => Ok(SourceKind::Http {
            url: cfg
                .url
                .clone()
                .ok_or_else(|| anyhow::anyhow!("http source `{}` requires `url`", cfg.name))?,
            selector: cfg.selector.clone().unwrap_or_default(),
        }),
        SourceType::Script => Ok(SourceKind::Script {
            command: cfg.command.clone().ok_or_else(|| {
                anyhow::anyhow!("script source `{}` requires `command`", cfg.name)
            })?,
        }),
    }
}

impl SourceKind {
    /// Returns the fetched value as a trimmed string. `dir` is the config
    /// file's own directory, used as the working directory for a `script`
    /// source's spawned process (spec: source-configuration — config-relative
    /// working directory); unused for `http`.
    pub async fn fetch(&self, dir: &Path) -> Result<String> {
        match self {
            SourceKind::Http { url, selector } => fetch_http(url, selector).await,
            SourceKind::Script { command } => fetch_script(command, dir).await,
        }
    }
}

async fn fetch_http(url: &str, selector: &str) -> Result<String> {
    let resp = HTTP_CLIENT
        .get(url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?
        .error_for_status()
        .with_context(|| format!("GET {url} returned error status"))?;
    let json: serde_json::Value = resp.json().await.context("decoding JSON body")?;
    let mut cur = &json;
    // ponytail: dotted-path selector ("a.b.c") only; add array indexes/jq when needed
    if !selector.is_empty() {
        for part in selector.split('.') {
            cur = cur.get(part).ok_or_else(|| {
                anyhow::anyhow!("selector `{selector}`: key `{part}` not found in response")
            })?;
        }
    }
    Ok(match cur {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    })
}

async fn fetch_script(command: &str, dir: &Path) -> Result<String> {
    let out = run_shell(command, dir).await?;
    Ok(out)
}

/// Kills a spawned command's whole process group (not just its direct PID)
/// if dropped before the command finishes — e.g. the collector's caller
/// cancels the surrounding `tokio::time::timeout` on a hung `script` source.
/// Without this, a hanging command (a slow API call, a `whois` lookup with
/// no timeout of its own) and everything it spawned are simply abandoned as
/// orphans instead of being cleaned up, quietly leaking processes/sockets on
/// every timeout (spec: data-collection — collector resilience: one
/// source's misbehavior must not degrade the whole daemon over time).
struct KillGroupOnDrop(Option<i32>);

impl Drop for KillGroupOnDrop {
    fn drop(&mut self) {
        if let Some(pgid) = self.0.take() {
            // Fire-and-forget: the negative pid targets the whole process
            // group `process_group(0)` put the command in at spawn time.
            let _ = std::process::Command::new("kill")
                .arg("-KILL")
                .arg(format!("-{pgid}"))
                .spawn();
        }
    }
}

/// Runs a shell command (working directory `dir` — the config file's own
/// directory, spec: source-configuration — config-relative working
/// directory) and returns its trimmed stdout; non-zero exit is an error
/// carrying stderr. Shared by script sources and setup commands. Spawned in
/// its own process group so a cancelled/timed-out fetch can be cleaned up
/// as a whole (see [`KillGroupOnDrop`]) instead of leaving orphans behind.
pub async fn run_shell(command: &str, dir: &Path) -> Result<String> {
    let child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(dir)
        .process_group(0)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("spawning sh")?;
    let mut guard = KillGroupOnDrop(child.id().map(u32::cast_signed));
    let out = child.wait_with_output().await.context("waiting for sh")?;
    guard.0 = None; // exited on its own — nothing left to clean up
    if !out.status.success() {
        bail!(
            "command failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::time::Duration;

    fn process_alive(pid: &str) -> bool {
        std::process::Command::new("kill")
            .arg("-0")
            .arg(pid)
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    /// A cancelled fetch (the collector's `tokio::time::timeout` racing a
    /// hung command) must not leave the command's process tree running as
    /// orphans — one misbehaving source script must not leak resources
    /// indefinitely (spec: data-collection — collector resilience).
    #[tokio::test]
    async fn cancelled_command_kills_its_whole_process_group() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        // The outer `sh -c` from `run_shell` is itself the process-group
        // leader; `$$` inside it is that leader's pid.
        let cmd = format!("echo $$ > {}; sleep 5", pid_file.display());

        let outcome =
            tokio::time::timeout(Duration::from_millis(200), run_shell(&cmd, dir.path())).await;
        assert!(
            outcome.is_err(),
            "expected the outer timeout to fire before `sleep 5` finishes"
        );

        let pid = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Ok(pid) = std::fs::read_to_string(&pid_file) {
                    return pid.trim().to_string();
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();

        // Give the fire-and-forget `kill -KILL` a moment to land, then
        // confirm the group leader (and thus `sleep 5`) is actually gone.
        let mut alive = true;
        for _ in 0..50 {
            alive = process_alive(&pid);
            if !alive {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            !alive,
            "process group {pid} is still alive after cancellation"
        );
    }
}
