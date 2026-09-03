use crate::config::SourceCfg;
use anyhow::{Context as _, Result, bail};

/// A fetchable data source. Enum-dispatched by config `type`;
/// new types = new variant + a match arm here and in [`build`](fn@build).
#[derive(Clone)]
pub enum SourceKind {
    Http { url: String, selector: String },
    Script { command: String },
}

/// Validates type-specific params at startup (spec: source-configuration).
pub fn build(cfg: &SourceCfg) -> Result<SourceKind> {
    match cfg.kind.as_str() {
        "http" => Ok(SourceKind::Http {
            url: cfg.url.clone().ok_or_else(|| {
                anyhow::anyhow!("http source `{}` requires `url`", cfg.name)
            })?,
            selector: cfg.selector.clone().unwrap_or_default(),
        }),
        "script" => Ok(SourceKind::Script {
            command: cfg.command.clone().ok_or_else(|| {
                anyhow::anyhow!("script source `{}` requires `command`", cfg.name)
            })?,
        }),
        other => bail!("unknown source type `{other}`"),
    }
}

impl SourceKind {
    /// Returns the fetched value as a trimmed string.
    pub async fn fetch(&self) -> Result<String> {
        match self {
            SourceKind::Http { url, selector } => fetch_http(url, selector).await,
            SourceKind::Script { command } => fetch_script(command).await,
        }
    }
}

async fn fetch_http(url: &str, selector: &str) -> Result<String> {
    let resp = reqwest::get(url)
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

async fn fetch_script(command: &str) -> Result<String> {
    let out = run_shell(command).await?;
    Ok(out)
}

/// Runs a shell command and returns its trimmed stdout; non-zero exit is an
/// error carrying stderr. Shared by script sources and setup commands.
pub async fn run_shell(command: &str) -> Result<String> {
    let out = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .output()
        .await
        .context("spawning sh")?;
    if !out.status.success() {
        bail!(
            "command failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
