//! Per-source and per-cell validation, run over an already-parsed [`Config`]
//! by [`super::validate`].

use super::{Cell, Config, GroupItem, SourceCfg, ValueFormat, validate_thresholds};
use anyhow::{Result, bail};
use std::time::Duration;

pub(crate) fn validate_source(s: &SourceCfg) -> Result<()> {
    if s.name().is_empty() {
        bail!("source with empty name");
    }
    // The name is embedded directly in URL path segments
    // (`/api/sources/{name}/history`, `/logs/{name}`) and shell-quoted
    // nowhere, so it's restricted to a safe charset rather than trusted as
    // opaque (spec: source-configuration — source names are URL-safe
    // identifiers); use `title` for a human-friendly display label instead.
    // A child's full name (`<parent>::<bare>`) is exempt here — both halves
    // are already charset-checked separately: the parent through its own
    // `validate_source` call, the bare child name by `expand_composites`.
    if !s.is_child()
        && !s
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
            timeout,
            ..
        } => validate_query(
            s.name(),
            command,
            *interval,
            cron.as_deref(),
            *retry_interval,
            *timeout,
        ),
        SourceCfg::Stream {
            command,
            expected_interval,
            retry_interval,
            timeout,
            ..
        } => validate_stream(
            s.name(),
            command,
            *expected_interval,
            *retry_interval,
            *timeout,
        ),
        SourceCfg::Ingest {
            expected_interval, ..
        } => {
            if expected_interval.is_zero() {
                bail!("ingest source `{}` expected_interval must be > 0", s.name());
            }
            Ok(())
        }
        // A child's shape (no command/schedule/setup of its own, unique full
        // name, valid bare name) is fully validated by `expand_composites`,
        // which produced it; the shared threshold/history_points checks
        // below still apply.
        SourceCfg::Child { .. } => Ok(()),
    }?;
    for t in s.thresholds() {
        if !t.bound.is_finite() {
            bail!("source `{}` threshold bound must be finite", s.name());
        }
    }
    validate_thresholds(s.name(), s.thresholds())?;
    if let Some(h) = s.history_points()
        && (h == 0 || i64::from(h) > crate::db::MAX_HISTORY_LIMIT)
    {
        bail!(
            "source `{}` history_points must be in 1..={}",
            s.name(),
            crate::db::MAX_HISTORY_LIMIT
        );
    }
    Ok(())
}

fn validate_query(
    name: &str,
    command: &str,
    interval: Option<Duration>,
    cron: Option<&str>,
    retry_interval: Option<Duration>,
    timeout: Duration,
) -> Result<()> {
    if command.is_empty() {
        bail!("query source `{name}` requires `command`");
    }
    if let Some(iv) = interval
        && iv.is_zero()
    {
        bail!("source `{name}` interval must be > 0");
    }
    if interval.is_some() && cron.is_some() {
        bail!("source `{name}` must declare only one of `interval`/`cron`");
    }
    if timeout.is_zero() {
        bail!("source `{name}` timeout must be > 0");
    }
    if let Some(iv) = retry_interval
        && iv.is_zero()
    {
        bail!("source `{name}` retry_interval must be > 0");
    }
    if retry_interval.is_some() && cron.is_some() {
        bail!("source `{name}` retry_interval has no effect on a cron-scheduled source");
    }
    if let Some(expr) = cron {
        validate_cron(name, expr)?;
    }
    Ok(())
}

fn validate_stream(
    name: &str,
    command: &str,
    expected_interval: Duration,
    retry_interval: Option<Duration>,
    timeout: Duration,
) -> Result<()> {
    if command.is_empty() {
        bail!("stream source `{name}` requires `command`");
    }
    if expected_interval.is_zero() {
        bail!("stream source `{name}` expected_interval must be > 0");
    }
    if timeout.is_zero() {
        bail!("stream source `{name}` timeout must be > 0");
    }
    if let Some(iv) = retry_interval
        && iv.is_zero()
    {
        bail!("source `{name}` retry_interval must be > 0");
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
        Cell::Space { kind, colspan } => {
            validate_space_cell(layout_title, row_idx, kind, *colspan)?;
        }
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
        Cell::Group { .. } => {}
    }
    // An explicitly empty `title` renders a blank header rather than falling
    // back to the cell's own default label, so it's a mistake on every
    // titled variant alike (only `Group`'s was checked before).
    if let Some("") = cell.pane_title() {
        bail!(
            "layout `{}` row {} has a {} with an empty title",
            layout_title,
            row_idx + 1,
            cell_kind_name(cell)
        );
    }
    if let Cell::Group {
        title,
        main,
        secondary,
        table,
        ..
    } = cell
    {
        validate_group_cell(
            cfg,
            layout_title,
            row_idx,
            title.as_deref(),
            main.as_ref(),
            secondary,
            table,
        )?;
    }
    Ok(())
}

/// Human-readable variant name for the empty-title diagnostic. Only the
/// titled variants can reach it — `Source` and `Space` carry no `title`.
fn cell_kind_name(cell: &Cell) -> &'static str {
    match cell {
        Cell::Pane { .. } => "pane",
        Cell::Text { .. } => "text panel",
        Cell::Group { .. } | Cell::Source(_) | Cell::Space { .. } => "cell",
    }
}

fn validate_space_cell(
    layout_title: &str,
    row_idx: usize,
    kind: &str,
    colspan: Option<usize>,
) -> Result<()> {
    // `kind`'s only purpose in the schema is disambiguating this variant
    // from `Group` while parsing (its value is otherwise unused downstream)
    // — but that also means a typo like `kind = "spacer"` would otherwise
    // silently succeed as a valid space cell instead of surfacing as the
    // mistake it is.
    if kind != "space" {
        bail!(
            "layout `{}` row {} has a cell with unknown `kind` `{}` (only `space` is valid)",
            layout_title,
            row_idx + 1,
            kind
        );
    }
    if colspan == Some(0) {
        bail!(
            "layout `{}` row {} has a space with colspan 0",
            layout_title,
            row_idx + 1
        );
    }
    // A `colspan` feeds the layout's column count verbatim, so an unbounded
    // one is either a mistake or a way to overflow the count; a real
    // dashboard column is nowhere near this wide.
    if let Some(c) = colspan
        && c > super::layout::MAX_COLSPAN
    {
        bail!(
            "layout `{}` row {} has a space with colspan {} above the maximum of {}",
            layout_title,
            row_idx + 1,
            c,
            super::layout::MAX_COLSPAN
        );
    }
    Ok(())
}

fn validate_group_cell(
    cfg: &Config,
    layout_title: &str,
    row_idx: usize,
    title: Option<&str>,
    main: Option<&GroupItem>,
    secondary: &[GroupItem],
    table: &[GroupItem],
) -> Result<()> {
    let group_label = title.unwrap_or("<untitled>");
    if main.is_none() && secondary.is_empty() && table.is_empty() {
        bail!(
            "layout `{}` row {} has group `{}` with none of main/secondary/table",
            layout_title,
            row_idx + 1,
            group_label
        );
    }
    for item in main.into_iter().chain(secondary).chain(table) {
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
    Ok(())
}
