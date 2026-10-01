//! Light/dark theme persistence (spec: web-ui — light/dark theme toggle).
//! `POST /api/theme` (`src/api.rs`) sets the cookie; every server-rendered
//! page reads it back via [`theme_class`] so the right `dark`/`light` class
//! on `<html>` is there from the first byte, with no client-side bootstrap.

use topcoat::{
    context::Cx,
    cookie::{Cookies, cookies},
};

/// Name of the cookie persisting the browser's explicit light/dark choice.
/// Public within the crate: `src/api.rs`'s `POST /api/theme` handler sets it.
pub(crate) const THEME_COOKIE: &str = "bd_theme";

/// Name of the cookie persisting the browser's explicit wide/narrow content
/// choice. Public within the crate: `src/api.rs`'s `POST /api/width`
/// handler sets it.
pub(crate) const WIDTH_COOKIE: &str = "bd_width";

/// `"dark"`/`"light"` when the browser has made an explicit choice
/// (`bd_theme` cookie), else empty — an empty class lets the CSS
/// `prefers-color-scheme` media query (`assets/styles.css`) decide, so a
/// first-time visitor still gets their OS preference.
pub(super) fn theme_class(cx: &Cx) -> &'static str {
    match cookies(cx)
        .get(THEME_COOKIE)
        .as_ref()
        .map(topcoat::cookie::Cookie::value)
    {
        Some("dark") => "dark",
        Some("light") => "light",
        _ => "",
    }
}

/// Content-width class for the `page_chrome` wrappers: the `bd_width`
/// cookie wins when it names a known width, else the server-wide
/// `web_content_width` default applies (spec: web-ui — per-browser width
/// override). An unknown cookie value falls back to the default rather
/// than rendering a half-known state. `"wide"` is full viewport width
/// (no cap class); `"narrow"` restores the centered `max-w-5xl` column.
/// The `max-w-5xl` literal must stay spelled out here (never built at
/// runtime): Tailwind scans Rust sources for class names, so only a
/// literal keeps the rule in the bundled stylesheet.
pub(super) fn width_class(cx: &Cx, default: &str) -> &'static str {
    let jar = cookies(cx);
    let found = jar.get(WIDTH_COOKIE);
    let cookie = found.as_ref().map(topcoat::cookie::Cookie::value);
    match cookie {
        Some("wide") => "",
        Some("narrow") => "max-w-5xl",
        // Validation constrains the config value to the same two tokens,
        // so anything that isn't an explicit wide default is narrow.
        _ => {
            if default == "wide" {
                ""
            } else {
                "max-w-5xl"
            }
        }
    }
}
