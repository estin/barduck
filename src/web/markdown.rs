//! Value-format rendering: markdown-as-HTML or plain text
//! (spec: web-ui — static-text panel rendering). Shared by a single-source
//! panel's content and a static-text cell's content.

use crate::config::ValueFormat;
use pulldown_cmark::Options;
use topcoat::{
    Result,
    view::{Unescaped, component, view},
};

/// Schemes safe to leave as a live link/image destination. Anything else —
/// notably `javascript:` and `data:` — is neutralized instead.
const SAFE_URL_SCHEMES: &[&str] = &["http", "https", "mailto"];

/// Whether `url` is safe to render as a link/image destination: either it
/// has no scheme at all (a relative path, `#fragment`, or `?query`, which
/// can't execute anything on its own), or its scheme is one of
/// [`SAFE_URL_SCHEMES`]. A scheme is `[a-zA-Z][a-zA-Z0-9+.-]*` followed by
/// `:` (RFC 3986); the first character that's neither part of that alphabet
/// nor `:` settles it either way.
///
/// The check runs against [`browser_normalized`] rather than the raw
/// destination: `CommonMark` decodes character references inside a link
/// destination, so `[x](&#9;javascript:alert(1))` and
/// `[x](java&#9;script:alert(1))` both reach us as strings a naive scheme
/// scan reads as *relative* (the tab isn't `:`) while the browser strips the
/// tab and executes the scheme anyway.
fn is_safe_url(url: &str) -> bool {
    let url = browser_normalized(url);
    match url.find(|c: char| !(c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')) {
        Some(i) if url.as_bytes()[i] == b':' => {
            SAFE_URL_SCHEMES.contains(&url[..i].to_ascii_lowercase().as_str())
        }
        // No `:` before the first non-scheme character (or no such
        // character at all): no scheme, so it's a relative reference.
        _ => true,
    }
}

/// A URL as the browser will actually see it when resolving the scheme: tab,
/// LF, and CR are stripped from *anywhere* in the string, and leading C0
/// controls/spaces are ignored (WHATWG URL parsing). Scheme checks must run
/// on this form, never on the raw text.
fn browser_normalized(url: &str) -> String {
    url.chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .skip_while(|&c| c <= ' ')
        .collect()
}

/// Rewrites a link/image `Start` event's destination to the empty string
/// when [`is_safe_url`] rejects it, leaving every other event untouched.
fn neutralize_unsafe_destination(event: pulldown_cmark::Event<'_>) -> pulldown_cmark::Event<'_> {
    use pulldown_cmark::{Event, Tag};
    match event {
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) if !is_safe_url(&dest_url) => Event::Start(Tag::Link {
            link_type,
            dest_url: "".into(),
            title,
            id,
        }),
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) if !is_safe_url(&dest_url) => Event::Start(Tag::Image {
            link_type,
            dest_url: "".into(),
            title,
            id,
        }),
        other => other,
    }
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
/// `push_html` like any other text event) before rendering. Link/image
/// destinations are markdown *syntax*, not raw HTML, so that rewrite doesn't
/// cover them — `pulldown-cmark` writes `dest_url` straight into the
/// rendered `href`/`src` attribute (HTML-attribute-escaped, but not
/// scheme-filtered), so a `[text](javascript:...)` link from an untrusted
/// source is neutralized separately, by [`neutralize_unsafe_destination`].
pub(super) fn markdown_to_html(value: &str) -> Unescaped<String> {
    Unescaped::new_unchecked(render_markdown(value))
}

/// The actual rendering behind [`markdown_to_html`], factored out so tests
/// can assert on the plain `String` instead of reaching into `Unescaped`.
fn render_markdown(value: &str) -> String {
    let events = pulldown_cmark::Parser::new_ext(value, Options::ENABLE_TABLES)
        .map(|event| match event {
            pulldown_cmark::Event::Html(html) | pulldown_cmark::Event::InlineHtml(html) => {
                pulldown_cmark::Event::Text(html)
            }
            other => neutralize_unsafe_destination(other),
        });
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, events);
    html
}

/// Renders `value` (with `unit` appended when non-empty, except for
/// markdown, which never gets a unit suffix) according to `format`: markdown
/// as HTML, otherwise plain text. Shared by the single-source panel's
/// content and the static-text cell's content, so the two-way format branch
/// isn't copy-pasted a third time (design.md — factor format-rendering logic).
#[component]
pub(super) async fn formatted_content(format: ValueFormat, value: String, unit: String) -> Result {
    let value_and_unit = if unit.is_empty() {
        value.clone()
    } else {
        format!("{value} {unit}")
    };
    view! {
        if format == ValueFormat::Markdown {
            <div class="prose prose-sm max-w-none">(markdown_to_html(&value))</div>
        } else {
            <div class="text-2xl font-semibold">(value_and_unit)</div>
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `javascript:` link destination from an untrusted `format =
    /// "markdown"` source must never reach the rendered `href` — that's a
    /// same-origin XSS the moment a viewer clicks the link.
    #[test]
    fn javascript_scheme_link_is_neutralized() {
        let html = render_markdown("[click me](javascript:alert(document.cookie))");
        assert!(
            !html.to_lowercase().contains("javascript:"),
            "javascript: scheme leaked into rendered HTML: {html}"
        );
    }

    /// Same for image destinations, and for `data:` (which can also carry
    /// script, e.g. an SVG payload).
    #[test]
    fn unsafe_schemes_neutralized_for_links_and_images() {
        for md in [
            "[x](data:text/html,<script>alert(1)</script>)",
            "![x](javascript:alert(1))",
            "[x](VBScript:msgbox(1))",
        ] {
            let html = render_markdown(md);
            assert!(
                !html.to_lowercase().contains("javascript:")
                    && !html.to_lowercase().contains("data:")
                    && !html.to_lowercase().contains("vbscript:"),
                "unsafe scheme leaked into rendered HTML for {md:?}: {html}"
            );
        }
    }

    /// Ordinary links must still work: this isn't a blanket link-stripping
    /// pass, only unsafe schemes are touched.
    #[test]
    fn safe_link_schemes_pass_through() {
        for (md, want_fragment) in [
            ("[site](https://example.com/page)", "https://example.com/page"),
            ("[mail](mailto:a@example.com)", "mailto:a@example.com"),
            ("[rel](/dashboard)", "/dashboard"),
            ("[anchor](#panel-cpu)", "#panel-cpu"),
        ] {
            let html = render_markdown(md);
            assert!(
                html.contains(want_fragment),
                "expected {want_fragment:?} in rendered HTML for {md:?}: {html}"
            );
        }
    }

    #[test]
    fn is_safe_url_accepts_allowlisted_and_relative_urls() {
        for url in [
            "http://example.com",
            "HTTPS://example.com",
            "mailto:a@example.com",
            "/relative/path",
            "#fragment",
            "?query=1",
            "plain-text-not-a-url",
        ] {
            assert!(is_safe_url(url), "expected safe: {url}");
        }
    }

    #[test]
    fn is_safe_url_rejects_dangerous_schemes() {
        for url in [
            "javascript:alert(1)",
            "JaVaScRiPt:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "vbscript:msgbox(1)",
            "file:///etc/passwd",
        ] {
            assert!(!is_safe_url(url), "expected unsafe: {url}");
        }
    }

    /// Browsers strip tab/LF/CR from anywhere in a URL and skip leading
    /// control characters, so a scheme check on the raw text is bypassable:
    /// these all execute `javascript:` despite not *looking* like a scheme
    /// to a left-to-right scan.
    #[test]
    fn is_safe_url_rejects_schemes_hidden_by_stripped_characters() {
        for url in [
            "\tjavascript:alert(1)",
            "\njavascript:alert(1)",
            "java\tscript:alert(1)",
            "java\nscript:alert(1)",
            " \u{1}javascript:alert(1)",
            "\rdata:text/html,<script>alert(1)</script>",
        ] {
            assert!(!is_safe_url(url), "expected unsafe: {url:?}");
        }
    }

    /// The same bypass reached through markdown's own character-reference
    /// decoding, which is where an untrusted source's value actually enters.
    #[test]
    fn character_reference_hidden_scheme_is_neutralized() {
        for md in [
            "[x](&#9;javascript:alert&#40;1&#41;)",
            "[x](java&#9;script:alert&#40;1&#41;)",
            "![x](&#10;javascript:alert&#40;1&#41;)",
        ] {
            let html = render_markdown(md);
            let flat: String = html
                .to_lowercase()
                .chars()
                .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
                .collect();
            assert!(
                !flat.contains("javascript:"),
                "javascript: survived for {md:?}: {html}"
            );
        }
    }

    /// Raw HTML (blocks or inline spans) is still fully neutralized —
    /// this coverage predates the scheme filter and must keep holding.
    #[test]
    fn raw_html_is_escaped_not_rendered() {
        let html = render_markdown("<script>alert(1)</script>text");
        assert!(!html.contains("<script>"));
    }
}
