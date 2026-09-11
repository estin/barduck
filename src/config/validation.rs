//! Per-source and per-cell validation, run over an already-parsed [`Config`]
//! by [`super::validate`].

use super::{Cell, Config, SourceCfg, ValueFormat, validate_thresholds};
use anyhow::{Result, bail};

pub(crate) fn validate_source(s: &SourceCfg) -> Result<()> {
    if s.name().is_empty() {
        bail!("source with empty name");
    }
    // The name is embedded directly in URL path segments
    // (`/api/sources/{name}/history`, `/logs/{name}`) and shell-quoted
    // nowhere, so it's restricted to a safe charset rather than trusted as
    // opaque (spec: source-configuration — source names are URL-safe
    // identifiers); use `title` for a human-friendly display label instead.
    if !s
        .name()
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        bail!(
            "source `{}` name must contain only ASCII letters, digits, `_`, or `-`",
            s.name()
        );
    }
    match s {
        SourceCfg::Query {
            command,
            interval,
            cron,
            retry_interval,
            ..
        } => {
            if command.is_empty() {
                bail!("query source `{}` requires `command`", s.name());
            }
            if let Some(iv) = interval
                && iv.is_zero()
            {
                bail!("source `{}` interval must be > 0", s.name());
            }
            if interval.is_some() && cron.is_some() {
                bail!(
                    "source `{}` must declare only one of `interval`/`cron`",
                    s.name()
                );
            }
            if let Some(iv) = retry_interval
                && iv.is_zero()
            {
                bail!("source `{}` retry_interval must be > 0", s.name());
            }
            if retry_interval.is_some() && cron.is_some() {
                bail!(
                    "source `{}` retry_interval has no effect on a cron-scheduled source",
                    s.name()
                );
            }
            if let Some(expr) = cron {
                validate_cron(s.name(), expr)?;
            }
        }
        SourceCfg::Stream {
            command,
            expected_interval,
            retry_interval,
            ..
        } => {
            if command.is_empty() {
                bail!("stream source `{}` requires `command`", s.name());
            }
            if expected_interval.is_zero() {
                bail!("stream source `{}` expected_interval must be > 0", s.name());
            }
            if let Some(iv) = retry_interval
                && iv.is_zero()
            {
                bail!("source `{}` retry_interval must be > 0", s.name());
            }
        }
        SourceCfg::Ingest {
            expected_interval,
            ..
        } => {
            if expected_interval.is_zero() {
                bail!("ingest source `{}` expected_interval must be > 0", s.name());
            }
        }
    }
    for t in s.thresholds() {
        if !t.bound.is_finite() {
            bail!("source `{}` threshold bound must be finite", s.name());
        }
    }
    validate_thresholds(s.name(), s.thresholds())?;
    if s.history_points() == Some(0) {
        bail!("source `{}` history_points must be > 0", s.name());
    }
    Ok(())
}

fn validate_cron(name: &str, expr: &str) -> Result<()> {
    let cron: croner::Cron = expr.parse().map_err(|e| {
        anyhow::anyhow!("source `{name}` has invalid cron expression `{expr}`: {e}")
    })?;
    if cron
        .find_next_occurrence(&chrono::Utc::now(), true)
        .is_err()
    {
        bail!("source `{name}` cron expression `{expr}` has no future occurrence");
    }
    Ok(())
}

pub(crate) fn validate_cell(
    cfg: &Config,
    layout_title: &str,
    row_idx: usize,
    cell: &Cell,
) -> Result<()> {
    match cell {
        Cell::Source(name) | Cell::Pane { id: name, .. } => {
            if !cfg.sources.iter().any(|s| s.name() == name) {
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
            bail!(
                "layout `{}` row {} has a space with colspan 0",
                layout_title,
                row_idx + 1
            );
        }
        Cell::Space { .. } => {}
        Cell::Text { text, format, .. } => {
            if text.is_empty() {
                bail!(
                    "layout `{}` row {} has a text panel with empty text",
                    layout_title,
                    row_idx + 1
                );
            }
            if let Some(fmt) = format {
                ValueFormat::parse(fmt)?;
            }
        }
        Cell::Group {
            title,
            main,
            secondary,
            table,
            ..
        } => {
            if title.as_deref() == Some("") {
                bail!(
                    "layout `{}` row {} has a group with an empty title",
                    layout_title,
                    row_idx + 1
                );
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
                if !cfg.sources.iter().any(|s| s.name() == item.id()) {
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
