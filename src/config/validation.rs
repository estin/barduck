//! Per-source and per-cell validation, run over an already-parsed [`Config`]
//! by [`super::validate`].

use super::{Cell, Config, SourceCfg, SourceType, ValueFormat};
use anyhow::{Result, bail};

pub(crate) fn validate_source(s: &SourceCfg) -> Result<()> {
    if s.name.is_empty() {
        bail!("source with empty name");
    }
    // The name is embedded directly in URL path segments
    // (`/api/sources/{name}/history`, `/logs/{name}`) and shell-quoted
    // nowhere, so it's restricted to a safe charset rather than trusted as
    // opaque (spec: source-configuration — source names are URL-safe
    // identifiers); use `title` for a human-friendly display label instead.
    if !s.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        bail!(
            "source `{}` name must contain only ASCII letters, digits, `_`, or `-`",
            s.name
        );
    }
    if s.kind == SourceType::Http && s.url.as_deref().unwrap_or("").is_empty() {
        bail!("http source `{}` requires `url`", s.name);
    }
    if s.kind == SourceType::Script && s.command.as_deref().unwrap_or("").is_empty() {
        bail!("script source `{}` requires `command`", s.name);
    }
    if let Some(iv) = s.interval
        && iv.is_zero()
    {
        bail!("source `{}` interval must be > 0", s.name);
    }
    if s.interval.is_some() && s.cron.is_some() {
        bail!("source `{}` cannot declare both `interval` and `cron`", s.name);
    }
    if let Some(iv) = s.retry_interval
        && iv.is_zero()
    {
        bail!("source `{}` retry_interval must be > 0", s.name);
    }
    if s.retry_interval.is_some() && s.cron.is_some() {
        bail!(
            "source `{}` cannot declare `retry_interval` with `cron` — retry_interval has no effect on a cron-scheduled source",
            s.name
        );
    }
    if let Some(expr) = &s.cron {
        let cron: croner::Cron = expr
            .parse()
            .map_err(|e| anyhow::anyhow!("source `{}` has invalid cron expression `{expr}`: {e}", s.name))?;
        if cron.find_next_occurrence(&chrono::Utc::now(), true).is_err() {
            bail!("source `{}` cron expression `{expr}` has no future occurrence", s.name);
        }
    }
    for t in &s.thresholds {
        if !t.bound.is_finite() {
            bail!("source `{}` threshold bound must be finite", s.name);
        }
    }
    if s.thresholds.len() == 1 {
        bail!("source `{}` needs at least 2 thresholds to form bands", s.name);
    }
    if s.history_points == Some(0) {
        bail!("source `{}` history_points must be > 0", s.name);
    }
    Ok(())
}

pub(crate) fn validate_cell(cfg: &Config, layout_title: &str, row_idx: usize, cell: &Cell) -> Result<()> {
    match cell {
        Cell::Source(name) | Cell::Pane { id: name, .. } => {
            if !cfg.sources.iter().any(|s| &s.name == name) {
                bail!(
                    "layout `{}` row {} references unknown source `{}`",
                    layout_title,
                    row_idx + 1,
                    name
                );
            }
        }
        Cell::Space {
            colspan: Some(0), ..
        } => {
            bail!("layout `{}` row {} has a space with colspan 0", layout_title, row_idx + 1);
        }
        Cell::Space { .. } => {}
        Cell::Text { text, format, .. } => {
            if text.is_empty() {
                bail!("layout `{}` row {} has a text panel with empty text", layout_title, row_idx + 1);
            }
            if let Some(fmt) = format {
                ValueFormat::parse(fmt)?;
            }
        }
        Cell::Group { title, main, secondary, table } => {
            if title.as_deref() == Some("") {
                bail!("layout `{}` row {} has a group with an empty title", layout_title, row_idx + 1);
            }
            let group_label = title.as_deref().unwrap_or("<untitled>");
            if main.is_none() && secondary.is_empty() && table.is_empty() {
                bail!(
                    "layout `{}` row {} has group `{}` with none of main/secondary/table",
                    layout_title,
                    row_idx + 1,
                    group_label
                );
            }
            for item in main.iter().chain(secondary).chain(table) {
                if !cfg.sources.iter().any(|s| s.name == item.id()) {
                    bail!(
                        "layout `{}` row {} group `{}` references unknown source `{}`",
                        layout_title,
                        row_idx + 1,
                        group_label,
                        item.id()
                    );
                }
            }
        }
    }
    Ok(())
}
