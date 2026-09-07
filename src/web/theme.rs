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

/// `"dark"`/`"light"` when the browser has made an explicit choice
/// (`bd_theme` cookie), else empty — an empty class lets the CSS
/// `prefers-color-scheme` media query (`assets/styles.css`) decide, so a
/// first-time visitor still gets their OS preference.
pub(super) fn theme_class(cx: &Cx) -> &'static str {
    match cookies(cx).get(THEME_COOKIE).as_ref().map(topcoat::cookie::Cookie::value) {
        Some("dark") => "dark",
        Some("light") => "light",
        _ => "",
    }
}
