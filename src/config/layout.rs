//! Layout declarations: grid rows of [`Cell`]s, and the value-rendering
//! format shared by a `Cell::Text` panel and `SourceCfg::format`.

use super::GroupItem;
use anyhow::{Result, bail};
use serde::Deserialize;

pub const VALUE_FORMATS: &[&str] = &["text", "markdown"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValueFormat {
    #[default]
    Text,
    Markdown,
}

impl ValueFormat {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "text" => Ok(Self::Text),
            "markdown" => Ok(Self::Markdown),
            other => bail!(
                "unknown value format `{other}` (known formats: {})",
                VALUE_FORMATS.join(", ")
            ),
        }
    }
}

/// TUI content width: `"auto"` (scale to content, capped at the terminal) or
/// a fixed number of columns (also capped at the terminal) (spec: tui —
/// configurable TUI content width).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum TuiWidth {
    Named(String),
    Fixed(u16),
}

/// One grid cell in a layout row. Untagged so the TOML stays terse:
/// `"src"`, `{ id = "src", title = "Pane" }`, `{ kind = "space", colspan = 2 }`,
/// `{ title = "Pane", main = "a", secondary = ["b"], table = [{ id = "c", label = "C" }] }`.
/// A `Group` cell needs at least one of `main`/`secondary`/`table`; `title`
/// is itself optional too — a title-less pane falls back to `main`'s own
/// label when `main` is set, else renders with no header text (spec:
/// source-configuration — UI layouts are config-declared like sources).
///
/// Variant order matters for an untagged enum: serde tries each variant
/// top-to-bottom and stops at the first structural match. `Group`'s fields
/// are *all* optional, so it will happily match almost any table-shaped cell
/// that isn't `Pane`/`Space` (unknown fields are simply ignored — no variant
/// here uses `deny_unknown_fields`). Any variant added after `Group` needs a
/// required field of its own and must be placed *before* `Group` in this
/// enum, or a cell meant for that variant will silently become an empty,
/// then-rejected `Group` instead. `Text` is placed here for exactly that
/// reason: its `text` field is required, so a cell without one falls through
/// to `Group` exactly as before.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Cell {
    Source(String),
    Pane {
        id: String,
        title: Option<String>,
    },
    Space {
        kind: String,
        colspan: Option<usize>,
    },
    Text {
        #[serde(default)]
        title: Option<String>,
        // Deliberately `String`, not `ValueFormat`: this field lives inside
        // an untagged enum, where a field that fails to deserialize doesn't
        // produce a clear "invalid value" error — it just makes serde treat
        // this whole variant as not matching and fall through to `Group`
        // (see the untagged-ordering note above), turning "bad format" into
        // a confusing "empty group" error instead. Validated explicitly in
        // `validation::validate_cell` so the error names the actual value.
        format: Option<String>,
        text: String,
    },
    Group {
        #[serde(default)]
        title: Option<String>,
        main: Option<GroupItem>,
        #[serde(default)]
        secondary: Vec<GroupItem>,
        #[serde(default)]
        table: Vec<GroupItem>,
    },
}

impl Cell {
    /// Every source this cell references: one for `Source`/`Pane`, one per
    /// member for `Group` (across `main`, `secondary`, and `table`), none for
    /// `Space` or `Text` (a `Text` cell has no backing source at all).
    #[must_use]
    pub fn source_names(&self) -> Vec<&str> {
        match self {
            Cell::Source(name) | Cell::Pane { id: name, .. } => vec![name],
            Cell::Group {
                main,
                secondary,
                table,
                ..
            } => main
                .iter()
                .chain(secondary)
                .chain(table)
                .map(GroupItem::id)
                .collect(),
            Cell::Space { .. } | Cell::Text { .. } => vec![],
        }
    }

    #[must_use]
    pub fn span(&self) -> usize {
        match self {
            Cell::Space { colspan, .. } => colspan.unwrap_or(1),
            _ => 1,
        }
    }

    #[must_use]
    pub fn pane_title(&self) -> Option<&str> {
        match self {
            Cell::Pane { title, .. } | Cell::Group { title, .. } | Cell::Text { title, .. } => {
                title.as_deref()
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutCfg {
    pub title: String,
    pub rows: Vec<Vec<Cell>>,
}

impl LayoutCfg {
    /// Column count = widest row (cells weighted by colspan).
    #[must_use]
    pub fn columns(&self) -> usize {
        self.rows
            .iter()
            .map(|row| row.iter().map(Cell::span).sum())
            .max()
            .unwrap_or(0)
    }

    #[must_use]
    pub fn source_names(&self) -> Vec<&str> {
        self.rows
            .iter()
            .flatten()
            .flat_map(Cell::source_names)
            .collect()
    }
}
