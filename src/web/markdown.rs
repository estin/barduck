//! Value-format rendering: markdown-as-HTML, pretty JSON, or plain text
//! (spec: web-ui — static-text panel rendering). Shared by a single-source
//! panel's content and a static-text cell's content.

use crate::config::ValueFormat;
use topcoat::{
    Result,
    view::{Unescaped, component, view},
};

/// Pretty JSON when parseable; raw text otherwise.
pub(super) fn json_pretty(value: &str) -> String {
    serde_json::from_str::<serde_json::Value>(value)
        .and_then(|v| serde_json::to_string_pretty(&v))
        .unwrap_or_else(|_| value.to_string())
}

/// Markdown rendered to HTML. The *Markdown* itself is trusted to become
/// styled markup (config-authored: either a static-text cell's literal text,
/// or a source's fetched value when that source's panel is configured with
/// `format = "markdown"`) — but a `format = "markdown"` source can be an
/// `http` source's live response body, which is emphatically not trusted to
/// inject arbitrary HTML/`<script>` into this page. `pulldown-cmark` parses
/// raw HTML blocks/inline spans as part of the `CommonMark` spec regardless of
/// which `Options` are enabled, so disabling extensions isn't enough —
/// any `Html`/`InlineHtml` event is rewritten to literal text (escaped by
/// `push_html` like any other text event) before rendering, and only the
/// Markdown *syntax* (links, lists, emphasis, …) still becomes markup.
pub(super) fn markdown_to_html(value: &str) -> Unescaped<String> {
    let events = pulldown_cmark::Parser::new(value).map(|event| match event {
        pulldown_cmark::Event::Html(html) | pulldown_cmark::Event::InlineHtml(html) => {
            pulldown_cmark::Event::Text(html)
        }
        other => other,
    });
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, events);
    Unescaped::new_unchecked(html)
}

/// Renders `value` (with `unit` appended when non-empty, except for
/// markdown, which never gets a unit suffix) according to `format`: markdown
/// as HTML, JSON pretty-printed below the raw value, otherwise plain text.
/// Shared by the single-source panel's content and the static-text cell's
/// content, so the three-way format branch isn't copy-pasted a third time
/// (design.md — factor format-rendering logic).
#[component]
pub(super) async fn formatted_content(format: ValueFormat, value: String, unit: String) -> Result {
    let value_and_unit = if unit.is_empty() { value.clone() } else { format!("{value} {unit}") };
    view! {
        if format == ValueFormat::Markdown {
            <div class="prose prose-sm max-w-none">(markdown_to_html(&value))</div>
        } else if format == ValueFormat::Json {
            <div class="text-2xl font-semibold">(value_and_unit)</div>
            <pre class="mt-1.5 text-xs font-mono whitespace-pre-wrap break-all max-h-40 overflow-y-auto">(json_pretty(&value))</pre>
        } else {
            <div class="text-2xl font-semibold">(value_and_unit)</div>
        }
    }
}
