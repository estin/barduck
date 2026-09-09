#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{Read, Write};

use barduck::{
    AppState, build_router_with_bundle, collect_once, config, db::Db, health, query::Backend,
};
use fs2::FileExt as _;
use std::sync::{Arc, OnceLock};

/// The asset bundle for page-rendering tests, built once per test-binary run.
///
/// `AssetBundle::load()` (used by `build_router`/production) looks next to
/// the current executable — correct for the real `barduck` binary,
/// but this test binary isn't it. `topcoat asset bundle` bundles the
/// `barduck` bin target itself, writing to its own
/// `target/debug/assets`; loading that explicitly via `load_dir` is what
/// lets a test render `dashboard()`/`source_logs()` (both reference bundled
/// assets: the Tailwind stylesheet, the Geist font) without panicking.
///
/// `cargo nextest` runs every test as its own process, so every
/// page-rendering test in this binary calls this function in a *separate*
/// process running in parallel with the others — the `OnceLock` above only
/// dedups within one process. Two concurrent `topcoat asset bundle`
/// invocations writing the same `target/debug/assets` directory can
/// interleave, leaving a manifest that's missing (or has a torn/inconsistent
/// entry for) an asset a reader expects — the same "concurrent writers can
/// see torn state" hazard `src/db.rs`'s own advisory lock exists for, just
/// in this CLI's asset bundler instead of `DuckDB`. So the bundle-and-load
/// step is itself guarded by a cross-process advisory lock.
fn test_asset_bundle() -> Option<topcoat::asset::AssetBundle> {
    static BUNDLE: OnceLock<Option<topcoat::asset::AssetBundle>> = OnceLock::new();
    BUNDLE
        .get_or_init(|| {
            let target_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
            std::fs::create_dir_all(&target_dir).expect("creating target directory");
            let lock_file = std::fs::File::create(target_dir.join(".topcoat-asset-bundle.lock"))
                .expect("creating asset-bundle lockfile");
            lock_file
                .lock_exclusive()
                .expect("locking asset-bundle lockfile");

            let status = std::process::Command::new("topcoat")
                .args(["asset", "bundle"])
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .status();
            if !matches!(status, Ok(s) if s.success()) {
                eprintln!(
                    "topcoat asset bundle failed ({status:?}); page-rendering tests will fail"
                );
            }
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/debug/assets");
            let bundle = topcoat::asset::AssetBundle::load_dir(dir).ok();

            let _ = lock_file.unlock();
            bundle
        })
        .clone()
}

fn test_server() -> (String, std::thread::JoinHandle<()>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let handle = std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { break };
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf);
            let body = r#"{"balance": 123.45}"#;
            s.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .ok();
        }
    });
    (addr, handle)
}

/// Builds a config with: a script source (echo), an http source (test server),
/// and a failing http source (closed port). `failure_threshold = 1` so one
/// failure flips health to failing.
fn test_config(
    db_path: &std::path::Path,
    http_addr: &str,
    marker: &std::path::Path,
) -> config::Config {
    let toml = format!(
        r#"
database_path = "{db}"
failure_threshold = 1

[[sources]]
name = "echo"
type = "script"
command = "echo 42"
unit = "x"

[[sources]]
name = "balance"
type = "http"
url = "http://{http}/"
selector = "balance"

[[sources]]
name = "dead"
type = "http"
url = "http://127.0.0.1:9/nope"

[[sources]]
name = "gated"
type = "script"
setup = "test -f {marker}"
command = "echo gated"

[[layouts]]
title = "Overview"
rows = [
  ["echo", {{ id = "balance", title = "Balance" }}],
  [{{ kind = "space" }}, "dead"],
]
"#,
        db = db_path.display(),
        marker = marker.display(),
        http = http_addr
    );
    let cfg: config::Config = toml::from_str(&toml).unwrap();
    if let Err(e) = config::validate(&cfg) {
        panic!("invalid demo config: {e:#}");
    }
    cfg
}

#[tokio::test]
async fn collection_writes_readings_logs_and_health_and_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.duckdb");
    let (addr, _server) = test_server();

    // Port 0 of the OS is closed for most purposes; pick a definitely-closed one.
    let cfg = test_config(&db_path, &addr, &dir.path().join("marker.absent"));

    let db = Db::open_rw(&db_path).unwrap();
    collect_once(&db, &cfg).await;

    // Readings from the two working sources.
    let latest = db.latest_values().await.unwrap();
    let echo = latest
        .iter()
        .find(|r| r.source == "echo")
        .expect("echo reading");
    assert_eq!(echo.value, "42");
    assert_eq!(echo.unit.as_deref(), Some("x"));
    let bal = latest
        .iter()
        .find(|r| r.source == "balance")
        .expect("balance reading");
    assert_eq!(bal.value, "123.45");

    // Fetch logs exist for all four configured sources; `dead` failed with an
    // error message, and `gated` logs its own failed setup attempt (its
    // marker file doesn't exist yet) rather than a fetch attempt.
    let logs = db.logs(None, 100).await.unwrap();
    assert_eq!(logs.len(), 4);
    let dead = logs.iter().find(|l| l.source == "dead").unwrap();
    assert!(dead.error.is_some());

    // One failure with threshold 1 → failing.
    let h = health::compute(&db, &cfg, "dead").await.unwrap();
    assert_eq!(h.status, health::Health::Failing);
    let h = health::compute(&db, &cfg, "echo").await.unwrap();
    assert_eq!(h.status, health::Health::Healthy);

    drop(db);

    // Restart: reopen the same file, history survives (spec: data-storage).
    let reopened = Db::open_rw(&db_path).unwrap();
    let hist = reopened.history("echo", None, None, None).await.unwrap();
    assert_eq!(hist.len(), 1);
    assert_eq!(hist[0].value, "42");
}

async fn start_daemon(cfg: &config::Config, db: &Db) -> String {
    let state = AppState {
        db: db.clone(),
        cfg: Arc::new(cfg.clone()),
    };
    let router = barduck::build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { topcoat::serve(listener, router).await });
    format!("http://{addr}")
}

#[tokio::test]
async fn daemon_api_parity_with_direct_mode_and_error_handling() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.duckdb");
    let (addr, _server) = test_server();
    let cfg = test_config(&db_path, &addr, &dir.path().join("marker.absent"));

    let db = Db::open_rw(&db_path).unwrap();
    collect_once(&db, &cfg).await;
    let base = start_daemon(&cfg, &db).await;

    let direct = Backend::new(&cfg, false).unwrap();
    let daemon = Backend::Daemon {
        base,
        client: reqwest::Client::new(),
    };

    // Parity: both backends return the same data.
    let d1 = direct.latest().await.unwrap();
    let d2 = daemon.latest().await.unwrap();
    assert_eq!(d1.len(), d2.len());
    assert_eq!(
        d1.iter().map(|r| (&r.source, &r.value)).collect::<Vec<_>>(),
        d2.iter().map(|r| (&r.source, &r.value)).collect::<Vec<_>>()
    );

    let h1 = direct.health(&cfg).await.unwrap();
    let h2 = daemon.health(&cfg).await.unwrap();
    assert_eq!(h1.len(), h2.len());
    assert_eq!(h1[0].status, h2[0].status);

    let l1 = direct.history("echo", None, None).await.unwrap();
    let l2 = daemon.history("echo", None, None).await.unwrap();
    assert_eq!(l1.len(), l2.len());

    // Unknown source → JSON error, not 500 (spec: http-api).
    let resp = reqwest::get(format!(
        "{}/api/sources/doesnotexist/history",
        daemon_base(&daemon)
    ))
    .await
    .unwrap();
    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("doesnotexist"));
}

fn daemon_base(b: &Backend) -> String {
    match b {
        Backend::Daemon { base, .. } => base.clone(),
        Backend::Direct(_) => panic!("expected daemon backend"),
    }
}

#[tokio::test]
async fn web_ui_renders_layout_panels_with_status_styles() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.duckdb");
    let (addr, _server) = test_server();
    let cfg = test_config(&db_path, &addr, &dir.path().join("marker.absent"));

    let db = Db::open_rw(&db_path).unwrap();
    collect_once(&db, &cfg).await;

    let state = AppState {
        db: db.clone(),
        cfg: Arc::new(cfg.clone()),
    };
    let router = build_router_with_bundle(state, test_asset_bundle());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move { topcoat::serve(listener, router).await });

    let html = reqwest::get(&url).await.unwrap().text().await.unwrap();
    assert!(html.contains("echo"));
    assert!(html.contains("42"));
    assert!(
        html.contains(r#"rel="stylesheet" href="/_topcoat/assets/"#),
        "bundled tailwind stylesheet expected"
    );
    // `dead` (failing, unbanded) gets a red accent from its health status
    // (inline style, not a class: see `Panel::status_style`), alongside a
    // plain uncolored label. `echo` (healthy, unbanded) gets no accent color
    // at all — not even green — since it has nothing to accent.
    assert!(html.contains("border-color:var(--status-red-border)"));
    assert!(
        html.contains("<span>failing</span>"),
        "plain failing label expected for the unbanded dead source"
    );
    assert!(
        !html.contains("border-color:var(--status-green-border)"),
        "a healthy unbanded source should get no accent color"
    );
    // Live updates: shard scope markers + vendored runtime script tag.
    assert!(
        html.contains("::topcoat::scope::"),
        "shard reactive scope expected"
    );
    assert!(
        html.contains("/assets/bd-runtime.js"),
        "runtime script tag expected"
    );
    let js = reqwest::get(format!("{url}assets/bd-runtime.js"))
        .await
        .unwrap();
    assert_eq!(js.status(), 200);
    assert!(!js.text().await.unwrap().trim().is_empty());
    // Grid arrangement: two rows, two columns; second row starts with a spacer.
    let base = url.trim_end_matches('/');
    let page = reqwest::get(base).await.unwrap().text().await.unwrap();
    assert!(
        page.contains("grid-template-columns: repeat(2"),
        "2-column grid expected"
    );
    // Counts grids by their own per-grid inline-style marker, not the bare
    // "grid-template-columns" text — the responsive `<style>` block (spec:
    // web-ui — responsive layout for small viewports) also mentions that
    // property once, statically, regardless of how many grids there are.
    assert_eq!(page.matches("--bd-cols:").count(), 1);
    assert!(
        page.contains(">Balance</span>"),
        "custom pane title expected"
    );
    // Log link on the time-ago text, opening in the current tab (spec:
    // web-ui — per-source log view linked from panels).
    assert!(
        page.contains(r#"href="/logs/balance""#),
        "per-source log link expected"
    );
    assert!(
        !page.contains(r#"target="_blank""#),
        "log link must not open in a new tab"
    );
}

/// `days-left` (5, red band) and `balance` (90, green band) grouped into one
/// "ihor" pane (spec: web-ui — group panes show multiple labeled,
/// independently colored values).
fn group_pane_config(db_path: &std::path::Path) -> config::Config {
    let toml = format!(
        r#"
database_path = "{db}"

[[sources]]
name = "days-left"
type = "script"
command = "echo 5"
unit = "d"
thresholds = [
  {{ bound = 10, level = "red" }},
  {{ bound = 30, level = "yellow" }},
  {{ bound = 3650, level = "green" }},
]

[[sources]]
name = "balance"
type = "script"
command = "echo 90"
unit = "USD"
thresholds = [
  {{ bound = 10, level = "red" }},
  {{ bound = 30, level = "yellow" }},
  {{ bound = 3650, level = "green" }},
]

[[sources]]
name = "note"
type = "script"
command = "echo hi"

[[layouts]]
title = "VDS"
rows = [
  [{{ title = "ihor", table = [{{ id = "days-left", label = "days left" }}, {{ id = "balance", label = "balance" }}, {{ id = "note", label = "note" }}] }}],
]
"#,
        db = db_path.display(),
    );
    let cfg: config::Config = toml::from_str(&toml).unwrap();
    if let Err(e) = config::validate(&cfg) {
        panic!("invalid group pane config: {e:#}");
    }
    cfg
}

#[tokio::test]
async fn web_ui_group_pane_renders_labeled_independently_colored_values() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.duckdb");
    let cfg = group_pane_config(&db_path);

    let db = Db::open_rw(&db_path).unwrap();
    collect_once(&db, &cfg).await;

    let state = AppState {
        db: db.clone(),
        cfg: Arc::new(cfg.clone()),
    };
    let router = build_router_with_bundle(state, test_asset_bundle());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move { topcoat::serve(listener, router).await });

    let html = reqwest::get(&url).await.unwrap().text().await.unwrap();
    // One card titled with the group's title, not the member sources' names.
    assert!(html.contains(">ihor</span>"), "group pane title expected");
    // Both labeled values present, each colored by its own threshold band.
    assert!(html.contains("days left"));
    assert!(html.contains("5 d"));
    assert!(html.contains("balance"));
    assert!(html.contains("90 USD"));
    // Rows carry only text color (on the value, not the label), no
    // border/background of their own.
    assert!(
        html.contains(r#"style="color:var(--status-red-text)""#),
        "red text for days-left row expected"
    );
    assert!(
        html.contains(r#"style="color:var(--status-green-text)""#),
        "green text for balance row expected"
    );
    // The card's own border is the worst color among its rows (red, here).
    // Chips use background-color, never border-color, so this is unambiguous.
    assert!(
        html.contains("border-color:var(--status-red-border)"),
        "group card border should reflect the worst row"
    );
    // Neither the card nor its rows carry a background color of their own —
    // scoped past the summary-strip chips (which legitimately use
    // background-color) and past each row's own history-bar segments, which
    // also legitimately use background-color (spec: web-ui — panel
    // retrospective history bar; dark theme uses moderated contrast and
    // desaturated status colors — segments are now inline-styled rather than
    // Tailwind classes, so they must be excluded explicitly here).
    let id_idx = html
        .find("id=\"panel-days-left\"")
        .expect("group card expected");
    let mut scoped = html[id_idx..].to_string();
    let segment_prefix = r#"<div class="flex-1" style="background-color"#;
    while let Some(start) = scoped.find(segment_prefix) {
        let end = scoped[start..]
            .find("></div>")
            .map_or(scoped.len(), |e| start + e + "></div>".len());
        scoped.replace_range(start..end, "");
    }
    assert!(
        !scoped.contains("background-color"),
        "group card/rows should carry no background color"
    );
    // Each row's label (not the "updated ago" text) links to its own
    // source's log view, in the current tab, and carries no color of its own.
    assert!(
        html.contains(r#"<a href="/logs/days-left" class="text-xs tracking-wide opacity-70 hover:opacity-100 hover:underline">days left</a>"#),
        "days-left label should link to its log view, uncolored"
    );
    assert!(
        html.contains(r#"href="/logs/balance""#),
        "balance row log link expected"
    );
    assert!(
        html.contains(r#"href="/logs/note""#),
        "note row log link expected"
    );
    assert!(
        !html.contains(r#"target="_blank""#),
        "log links must not open in a new tab"
    );
    // These readings were all just collected, so no row is lagging — no
    // "updated ago" text should appear anywhere in the group card.
    assert!(
        !html[id_idx..].contains("updated"),
        "fresh group rows should show no 'updated ago' text"
    );
    // Only the two banded members (days-left, balance) render a history bar;
    // the unbanded `note` member renders none.
    assert_eq!(
        html.matches("mt-1 flex h-1").count(),
        2,
        "exactly the banded group members should render a history bar"
    );
}

/// `cpu` has 3 thresholded readings and `history_points = 3` (no padding);
/// `sparse` has `history_points = 5` but only 2 readings (left-padded);
/// `plain` has no thresholds at all (no bar).
/// (spec: web-ui — panel retrospective history bar; source-configuration —
/// configurable history bar depth)
fn history_bar_config(db_path: &std::path::Path) -> config::Config {
    let toml = format!(
        r#"
database_path = "{db}"

[[sources]]
name = "cpu"
type = "script"
command = "echo 0"
history_points = 3
thresholds = [
  {{ bound = 60.0, level = "green" }},
  {{ bound = 85.0, level = "yellow" }},
  {{ bound = 100.0, level = "red" }},
]

[[sources]]
name = "sparse"
type = "script"
command = "echo 0"
history_points = 5
thresholds = [
  {{ bound = 60.0, level = "green" }},
  {{ bound = 85.0, level = "yellow" }},
  {{ bound = 100.0, level = "red" }},
]

[[sources]]
name = "plain"
type = "script"
command = "echo 0"

[[layouts]]
title = "Overview"
rows = [["cpu", "plain", "sparse"]]
"#,
        db = db_path.display(),
    );
    let cfg: config::Config = toml::from_str(&toml).unwrap();
    if let Err(e) = config::validate(&cfg) {
        panic!("invalid history-bar config: {e:#}");
    }
    cfg
}

/// Returns the HTML slice for one panel: from its title marker up to the
/// next title marker (or end of page).
fn panel_slice<'a>(page: &'a str, title: &str, next_title: Option<&str>) -> &'a str {
    let start = page.find(&format!(">{title}</span>")).expect("panel title");
    match next_title {
        Some(next) => {
            &page[start
                ..page
                    .find(&format!(">{next}</span>"))
                    .expect("next panel title")]
        }
        None => &page[start..],
    }
}

#[tokio::test]
async fn web_ui_history_bar_reflects_recent_readings() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.duckdb");
    let cfg = history_bar_config(&db_path);

    let db = Db::open_rw(&db_path).unwrap();
    // cpu: exactly 3 readings for history_points = 3 -> green, yellow, red, no padding.
    for v in ["40", "70", "95"] {
        db.insert_reading("cpu", v, None, None, None, None).await.unwrap();
    }
    // sparse: only 2 of 5 history_points -> 3 neutral padding segments, then green, red.
    for v in ["40", "95"] {
        db.insert_reading("sparse", v, None, None, None, None).await.unwrap();
    }
    db.insert_reading("plain", "hello", None, None, None, None)
        .await
        .unwrap();

    let state = AppState {
        db: db.clone(),
        cfg: Arc::new(cfg.clone()),
    };
    let router = build_router_with_bundle(state, test_asset_bundle());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move { topcoat::serve(listener, router).await });

    let page = reqwest::get(&url).await.unwrap().text().await.unwrap();

    // cpu: 3 segments, colored in reading order, no neutral padding.
    let cpu = panel_slice(&page, "cpu", Some("plain"));
    assert_eq!(
        cpu.matches("class=\"flex-1\" style=\"background-color:var(--")
            .count(),
        3,
        "cpu bar should have exactly 3 segments"
    );
    let (i_green, i_yellow, i_red) = (
        cpu.find("--status-green-border").expect("green segment"),
        cpu.find("--status-yellow-border").expect("yellow segment"),
        cpu.find("--status-red-border").expect("red segment"),
    );
    assert!(
        i_green < i_yellow && i_yellow < i_red,
        "segments should read green, yellow, red left to right"
    );

    // plain: no thresholds -> no history bar at all.
    let plain = panel_slice(&page, "plain", Some("sparse"));
    assert!(
        !plain.contains("class=\"flex-1\" style=\"background-color:var(--"),
        "unbanded panel should have no history bar"
    );

    // sparse: 5 segments, left-padded with 3 neutral placeholders, then green, red.
    let sparse = panel_slice(&page, "sparse", None);
    assert_eq!(
        sparse
            .matches("class=\"flex-1\" style=\"background-color:var(--")
            .count(),
        5,
        "sparse bar should be padded to 5 segments"
    );
    let neutral_count = sparse
        .matches("background-color:var(--border)")
        .count();
    assert_eq!(
        neutral_count, 3,
        "3 padding segments expected for 2 readings out of 5 history_points"
    );
    let i_neutral3 = sparse.rfind("background-color:var(--border)").unwrap();
    let i_green = sparse.find("--status-green-border").expect("green segment");
    let i_red = sparse.find("--status-red-border").expect("red segment");
    assert!(
        i_neutral3 < i_green && i_green < i_red,
        "padding segments should precede the real readings"
    );
}

/// (spec: web-ui — health visible at a glance)
#[tokio::test]
async fn web_ui_unbanded_failing_source_colors_red_with_plain_label() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.duckdb");
    let (addr, _server) = test_server();
    let cfg = test_config(&db_path, &addr, &dir.path().join("marker.absent"));

    let db = Db::open_rw(&db_path).unwrap();
    collect_once(&db, &cfg).await;

    let state = AppState {
        db: db.clone(),
        cfg: Arc::new(cfg.clone()),
    };
    let router = build_router_with_bundle(state, test_asset_bundle());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move { topcoat::serve(listener, router).await });

    let html = reqwest::get(&url).await.unwrap().text().await.unwrap();
    // Neither `echo` (healthy) nor `dead` (failing) declares thresholds.
    // `dead`'s failing health colors it red, with a plain uncolored label
    // alongside that color, not instead of it. `echo` being healthy and
    // unbanded gets no accent color at all — not even green.
    assert!(
        html.contains("border-color:var(--status-red-border)"),
        "failing unbanded panel should render red"
    );
    assert!(
        html.contains("<span>failing</span>"),
        "plain uncolored failing label expected alongside the red style"
    );
    assert!(
        !html.contains("border-color:var(--status-green-border)"),
        "a healthy unbanded source should get no accent color"
    );
}

/// `shown` renders its history bar as usual; `hidden` declares the same
/// bands but opts out with `show_history = false`.
/// (spec: source-configuration — per-source history bar visibility)
fn show_history_config(db_path: &std::path::Path) -> config::Config {
    let toml = format!(
        r#"
database_path = "{db}"

[[sources]]
name = "shown"
type = "script"
command = "echo 40"
thresholds = [
  {{ bound = 60.0, level = "green" }},
  {{ bound = 85.0, level = "yellow" }},
  {{ bound = 100.0, level = "red" }},
]

[[sources]]
name = "hidden"
type = "script"
command = "echo 40"
show_history = false
thresholds = [
  {{ bound = 60.0, level = "green" }},
  {{ bound = 85.0, level = "yellow" }},
  {{ bound = 100.0, level = "red" }},
]

[[layouts]]
title = "Overview"
rows = [["shown", "hidden"]]
"#,
        db = db_path.display(),
    );
    let cfg: config::Config = toml::from_str(&toml).unwrap();
    if let Err(e) = config::validate(&cfg) {
        panic!("invalid show_history config: {e:#}");
    }
    cfg
}

#[tokio::test]
async fn web_ui_show_history_false_hides_bar_for_banded_source() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.duckdb");
    let cfg = show_history_config(&db_path);

    let db = Db::open_rw(&db_path).unwrap();
    collect_once(&db, &cfg).await;

    let state = AppState {
        db: db.clone(),
        cfg: Arc::new(cfg.clone()),
    };
    let router = build_router_with_bundle(state, test_asset_bundle());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move { topcoat::serve(listener, router).await });

    let page = reqwest::get(&url).await.unwrap().text().await.unwrap();
    let shown = panel_slice(&page, "shown", Some("hidden"));
    assert!(
        shown.contains("class=\"flex-1\" style=\"background-color:var(--"),
        "shown source should render its history bar"
    );
    let hidden = panel_slice(&page, "hidden", None);
    assert!(
        !hidden.contains("class=\"flex-1\" style=\"background-color:var(--"),
        "show_history=false should hide the bar even though banded"
    );
}

/// `days-left` is threshold-banded; `flaky` has no thresholds and fails.
/// (spec: web-ui — group panes show multiple labeled, independently colored values)
fn group_pane_with_unbanded_failing_config(db_path: &std::path::Path) -> config::Config {
    let toml = format!(
        r#"
database_path = "{db}"
failure_threshold = 1

[[sources]]
name = "days-left"
type = "script"
command = "echo 5"
unit = "d"
thresholds = [
  {{ bound = 10, level = "red" }},
  {{ bound = 30, level = "yellow" }},
  {{ bound = 3650, level = "green" }},
]

[[sources]]
name = "flaky"
type = "http"
url = "http://127.0.0.1:9/nope"

[[layouts]]
title = "VDS"
rows = [
  [{{ title = "ihor", table = [{{ id = "days-left", label = "days left" }}, {{ id = "flaky", label = "flaky" }}] }}],
]
"#,
        db = db_path.display(),
    );
    let cfg: config::Config = toml::from_str(&toml).unwrap();
    if let Err(e) = config::validate(&cfg) {
        panic!("invalid group config: {e:#}");
    }
    cfg
}

#[tokio::test]
async fn web_ui_group_row_unbanded_member_colors_red_with_plain_label() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.duckdb");
    let cfg = group_pane_with_unbanded_failing_config(&db_path);

    let db = Db::open_rw(&db_path).unwrap();
    collect_once(&db, &cfg).await;

    let state = AppState {
        db: db.clone(),
        cfg: Arc::new(cfg.clone()),
    };
    let router = build_router_with_bundle(state, test_asset_bundle());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move { topcoat::serve(listener, router).await });

    let html = reqwest::get(&url).await.unwrap().text().await.unwrap();
    // Card border flags the failing unbanded member (aggregate rule unchanged).
    assert!(
        html.contains("border-color:var(--status-red-border)"),
        "card border should reflect the failing member"
    );
    // That member's own row now also renders red text (health-derived, since
    // it has no bands), with the plain label alongside it, not instead of it.
    // (Not the summary-strip chip link, which also renders the text "flaky" —
    // scope to the row's own `/logs/<source>` link.)
    let row_idx = html.find(r#"href="/logs/flaky""#).expect("flaky row label");
    let row_slice = &html[row_idx..(row_idx + 300).min(html.len())];
    assert!(
        row_slice.contains("color:var(--status-red-text)"),
        "unbanded failing row should render red text: {row_slice}"
    );
    assert!(
        row_slice.contains("failing"),
        "plain failing label expected alongside the red text: {row_slice}"
    );
}

/// A generalized pane cell declaring only `main` (no `secondary`/`table`)
/// (spec: web-ui — group panes show multiple labeled, independently colored
/// values — main section renders like a single-source panel).
fn main_only_pane_config(db_path: &std::path::Path) -> config::Config {
    let toml = format!(
        r#"
database_path = "{db}"

[[sources]]
name = "cpu-load"
type = "script"
command = "echo 70"
unit = "%"
thresholds = [
  {{ bound = 60.0, level = "green" }},
  {{ bound = 85.0, level = "yellow" }},
  {{ bound = 100.0, level = "red" }},
]

[[layouts]]
title = "L"
rows = [
  [{{ title = "CPU Pane", main = "cpu-load" }}],
]
"#,
        db = db_path.display(),
    );
    let cfg: config::Config = toml::from_str(&toml).unwrap();
    if let Err(e) = config::validate(&cfg) {
        panic!("invalid main-only pane config: {e:#}");
    }
    cfg
}

#[tokio::test]
async fn web_ui_main_only_pane_renders_like_single_source_panel() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.duckdb");
    let cfg = main_only_pane_config(&db_path);

    let db = Db::open_rw(&db_path).unwrap();
    collect_once(&db, &cfg).await;

    let state = AppState {
        db: db.clone(),
        cfg: Arc::new(cfg.clone()),
    };
    let router = build_router_with_bundle(state, test_asset_bundle());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move { topcoat::serve(listener, router).await });

    let html = reqwest::get(&url).await.unwrap().text().await.unwrap();
    // The cell's own title is used, not `main`'s source name.
    assert!(html.contains(">CPU Pane</span>"), "cell title expected");
    assert!(html.contains("70"), "main value missing");
    // Full single-panel styling — border AND background, unlike a table/
    // secondary row which only ever carries a text color.
    assert!(
        html.contains(
            "border-color:var(--status-yellow-border);background-color:var(--status-yellow-bg)"
        ),
        "full yellow panel style expected"
    );
    // `main`'s own history bar (thresholds + a reading) renders, using the
    // same wrapper class a plain single-source panel uses.
    assert!(
        html.contains("mt-1.5 flex h-1"),
        "main's own history bar expected"
    );
}

/// A generalized pane combining `main`, `secondary`, and `table`: `secondary`
/// always shows its age and never a history bar (even when banded), and an
/// unbanded failing `secondary` member still drives the card's own border
/// color, not just `table` members (spec: web-ui — group panes show multiple
/// labeled, independently colored values).
fn combined_pane_config(db_path: &std::path::Path) -> config::Config {
    let toml = format!(
        r#"
database_path = "{db}"
failure_threshold = 1

[[sources]]
name = "cpu-load"
type = "script"
command = "echo 42"
unit = "%"
thresholds = [
  {{ bound = 60.0, level = "green" }},
  {{ bound = 85.0, level = "yellow" }},
  {{ bound = 100.0, level = "red" }},
]

[[sources]]
name = "mem-warn"
type = "script"
command = "echo 70"
unit = "%"
thresholds = [
  {{ bound = 60.0, level = "green" }},
  {{ bound = 85.0, level = "yellow" }},
  {{ bound = 100.0, level = "red" }},
]

[[sources]]
name = "flaky"
type = "http"
url = "http://127.0.0.1:9/nope"

[[sources]]
name = "days-left"
type = "script"
command = "echo 90"
unit = "d"
thresholds = [
  {{ bound = 10, level = "red" }},
  {{ bound = 30, level = "yellow" }},
  {{ bound = 3650, level = "green" }},
]

[[layouts]]
title = "L"
rows = [
  [{{
      title = "Server",
      main = "cpu-load",
      secondary = [{{ id = "mem-warn", label = "mem" }}, {{ id = "flaky", label = "flaky" }}],
      table = [{{ id = "days-left", label = "balance" }}],
  }}],
]
"#,
        db = db_path.display(),
    );
    let cfg: config::Config = toml::from_str(&toml).unwrap();
    if let Err(e) = config::validate(&cfg) {
        panic!("invalid combined pane config: {e:#}");
    }
    cfg
}

#[tokio::test]
async fn web_ui_combined_pane_renders_all_three_sections() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.duckdb");
    let cfg = combined_pane_config(&db_path);

    let db = Db::open_rw(&db_path).unwrap();
    collect_once(&db, &cfg).await;

    let state = AppState {
        db: db.clone(),
        cfg: Arc::new(cfg.clone()),
    };
    let router = build_router_with_bundle(state, test_asset_bundle());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move { topcoat::serve(listener, router).await });

    let html = reqwest::get(&url).await.unwrap().text().await.unwrap();
    assert!(html.contains(">Server</span>"), "cell title expected");
    assert!(html.contains("42"), "main value missing");
    assert!(html.contains("70"), "secondary value missing");
    assert!(html.contains("balance"), "table label missing");
    assert!(html.contains("90"), "table value missing");

    // Both `secondary` members render inside one shared row, as plain
    // colored value+unit links to their own log view — no separate label.
    let secondary_row_idx = html
        .find(r#"class="flex flex-wrap items-center gap-2.5 mb-1.5""#)
        .expect("secondary row container expected");
    let secondary_row = &html[secondary_row_idx..(secondary_row_idx + 600).min(html.len())];
    assert!(
        secondary_row.contains(r#"href="/logs/mem-warn""#),
        "mem-warn secondary link expected in the shared row: {secondary_row}"
    );
    assert!(
        secondary_row.contains(r#"href="/logs/flaky""#),
        "flaky secondary link expected in the same shared row: {secondary_row}"
    );

    // The failing, unbanded `flaky` secondary member alone is enough to turn
    // the whole card's border red — not just a table member — and its value
    // is replaced with the plain word "FAILING", not a stale/missing value.
    assert!(
        html.contains("border-color:var(--status-red-border)"),
        "card border should reflect the failing secondary member"
    );
    let flaky_idx = html
        .find(r#"href="/logs/flaky""#)
        .expect("flaky link expected");
    let flaky_slice = &html[flaky_idx..(flaky_idx + 200).min(html.len())];
    assert!(
        flaky_slice.contains("FAILING"),
        "failing secondary member should show FAILING text: {flaky_slice}"
    );
    assert!(
        flaky_slice.contains("color:var(--status-red-text)"),
        "FAILING text should render red: {flaky_slice}"
    );

    // `main`'s own history bar (2-unit height) renders once; `table`'s single
    // banded row (1.5-unit height) renders once too — but `mem-warn`, also
    // banded, contributes no history bar at all as a `secondary` member.
    assert_eq!(
        html.matches("mt-1.5 flex h-1").count(),
        1,
        "only main should render its own history bar"
    );
    assert_eq!(
        html.matches("mt-1 flex h-1").count(),
        1,
        "only the table row should render a history bar, not the banded secondary member"
    );
}

/// `tui-only` is restricted to the TUI (`show_in = "tui"`); `visible` has no
/// restriction (spec: web-ui — hidden sources render as space in the web
/// dashboard).
fn show_in_config(db_path: &std::path::Path) -> config::Config {
    let toml = format!(
        r#"
database_path = "{db}"

[[sources]]
name = "visible"
type = "script"
command = "echo 1"

[[sources]]
name = "tui-only"
type = "script"
command = "echo 2"
show_in = "tui"

[[layouts]]
title = "Overview"
rows = [["visible", "tui-only"]]
"#,
        db = db_path.display(),
    );
    let cfg: config::Config = toml::from_str(&toml).unwrap();
    if let Err(e) = config::validate(&cfg) {
        panic!("invalid show_in config: {e:#}");
    }
    cfg
}

/// A source restricted to the TUI renders as an empty grid position on the
/// web dashboard — no card markup, no chip in the summary strip — while a
/// source with no restriction still renders normally, from the very same
/// layout (spec: web-ui — hidden sources render as space in the web
/// dashboard; source summary strip). This is the web half of "same layout
/// renders differently per view" — the TUI half is proven in
/// `src/tui.rs`'s own `same_layout_renders_differently_per_view` test, since
/// the TUI's rendering internals aren't reachable from here.
#[tokio::test]
async fn web_ui_hides_a_tui_only_source_but_keeps_the_unrestricted_one() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.duckdb");
    let cfg = show_in_config(&db_path);

    let db = Db::open_rw(&db_path).unwrap();
    collect_once(&db, &cfg).await;

    let state = AppState {
        db: db.clone(),
        cfg: Arc::new(cfg.clone()),
    };
    let router = build_router_with_bundle(state, test_asset_bundle());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move { topcoat::serve(listener, router).await });

    let page = reqwest::get(&url).await.unwrap().text().await.unwrap();
    // The unrestricted source renders its card and value as usual.
    assert!(
        page.contains(r#"id="panel-visible""#),
        "unrestricted source's card expected"
    );
    assert!(
        page.contains(r##"href="#panel-visible""##),
        "unrestricted source's chip expected"
    );
    // The TUI-only source gets no card and no chip at all.
    assert!(
        !page.contains("panel-tui-only"),
        "a TUI-only source should render no card or chip on the web dashboard"
    );
}

/// A generalized pane's `secondary` member restricted to the TUI is omitted
/// from the web pane, while its other members still render; a pane whose
/// only member is TUI-only renders as an empty grid position (spec: web-ui —
/// hidden sources render as space in the web dashboard).
fn show_in_pane_config(db_path: &std::path::Path) -> config::Config {
    let toml = format!(
        r#"
database_path = "{db}"

[[sources]]
name = "cpu"
type = "script"
command = "echo 42"
unit = "%"

[[sources]]
name = "tui-only"
type = "script"
command = "echo 2"
show_in = "tui"

[[layouts]]
title = "Overview"
rows = [
  [{{ title = "grp", secondary = ["cpu", "tui-only"] }}],
  [{{ title = "solo", secondary = ["tui-only"] }}],
]
"#,
        db = db_path.display(),
    );
    let cfg: config::Config = toml::from_str(&toml).unwrap();
    if let Err(e) = config::validate(&cfg) {
        panic!("invalid show_in pane config: {e:#}");
    }
    cfg
}

#[tokio::test]
async fn web_ui_omits_hidden_pane_member_and_collapses_all_hidden_pane() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.duckdb");
    let cfg = show_in_pane_config(&db_path);

    let db = Db::open_rw(&db_path).unwrap();
    collect_once(&db, &cfg).await;

    let state = AppState {
        db: db.clone(),
        cfg: Arc::new(cfg.clone()),
    };
    let router = build_router_with_bundle(state, test_asset_bundle());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move { topcoat::serve(listener, router).await });

    let page = reqwest::get(&url).await.unwrap().text().await.unwrap();
    // "grp" pane: the visible member renders, the hidden one is omitted.
    assert!(page.contains(">grp</span>"), "grp pane title expected");
    assert!(
        page.contains(r#"href="/logs/cpu""#),
        "visible secondary member expected in grp"
    );
    assert!(
        !page.contains("/logs/tui-only"),
        "TUI-only member should not appear anywhere on the web dashboard"
    );
    // "solo" pane: its only member is hidden, so the whole cell renders empty
    // (no title span, no card content) — the same treatment as an explicit
    // `space` cell.
    assert!(
        !page.contains(">solo</span>"),
        "a pane whose only member is hidden should render no title"
    );
}

#[test]
fn history_points_zero_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let toml = format!(
        r#"
database_path = "{db}"

[[sources]]
name = "cpu"
type = "script"
command = "echo 0"
history_points = 0
"#,
        db = dir.path().join("t.duckdb").display(),
    );
    let cfg: config::Config = toml::from_str(&toml).unwrap();
    let err = config::validate(&cfg).unwrap_err();
    assert!(
        format!("{err:#}").contains("cpu"),
        "error should name the offending source"
    );
}

/// (spec: http-api — ping/pong health endpoint)
#[tokio::test]
async fn ping_endpoint_returns_server_time() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.duckdb");
    let (addr, _server) = test_server();
    let cfg = test_config(&db_path, &addr, &dir.path().join("marker.absent"));
    let db = Db::open_rw(&db_path).unwrap();
    let base = start_daemon(&cfg, &db).await;

    let resp = reqwest::get(format!("{base}/api/ping")).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body["server_time"].as_str().is_some(),
        "server_time field expected"
    );
}

/// (spec: web-ui — global connection health indicator)
#[tokio::test]
async fn dashboard_includes_connection_indicator() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.duckdb");
    let (addr, _server) = test_server();
    let cfg = test_config(&db_path, &addr, &dir.path().join("marker.absent"));
    let db = Db::open_rw(&db_path).unwrap();
    collect_once(&db, &cfg).await;

    let state = AppState {
        db: db.clone(),
        cfg: Arc::new(cfg.clone()),
    };
    let router = build_router_with_bundle(state, test_asset_bundle());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move { topcoat::serve(listener, router).await });

    let page = reqwest::get(&url).await.unwrap().text().await.unwrap();
    assert!(
        page.contains(r#"id="bd-conn-dot""#),
        "connection dot expected"
    );
    assert!(
        page.contains(r#"id="bd-conn-label""#),
        "connection label expected"
    );
    assert!(
        page.contains("checking…"),
        "initial checking state expected"
    );
    assert!(
        page.contains("/api/ping"),
        "ping script should reference /api/ping"
    );
}

/// (spec: web-ui — global connection health indicator: favicon turns red
/// when offline, offline banner and dim shown/cleared)
#[tokio::test]
async fn dashboard_includes_offline_banner_and_dim_toggle() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.duckdb");
    let (addr, _server) = test_server();
    let cfg = test_config(&db_path, &addr, &dir.path().join("marker.absent"));
    let db = Db::open_rw(&db_path).unwrap();
    collect_once(&db, &cfg).await;

    let state = AppState {
        db: db.clone(),
        cfg: Arc::new(cfg.clone()),
    };
    let router = build_router_with_bundle(state, test_asset_bundle());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move { topcoat::serve(listener, router).await });

    let page = reqwest::get(&url).await.unwrap().text().await.unwrap();
    // Banner exists, starts hidden, and is styled with the status-red tokens
    // rather than a raw hardcoded color (spec: web-ui — consistent
    // token-based visual theme).
    assert!(
        page.contains(r#"id="bd-offline-banner""#),
        "offline banner element expected"
    );
    assert!(
        page.contains(r#"id="bd-offline-banner" hidden="""#),
        "offline banner should start hidden"
    );
    assert!(
        page.contains("--status-red-border") && page.contains("--status-red-bg"),
        "offline banner should be styled with status-red tokens"
    );
    // Panel wrapper exists as the dim-toggle hook.
    assert!(
        page.contains(r#"id="bd-panel-wrapper""#),
        "panel wrapper hook for dimming expected"
    );
    // The connection script toggles the banner/dim/favicon-flag together.
    assert!(
        page.contains("document.body.dataset.bdConnection = state"),
        "connection script should publish state for the favicon script to read"
    );
    assert!(
        page.contains("banner.hidden = !offline"),
        "connection script should toggle the offline banner"
    );
    assert!(
        page.contains("classList.toggle('opacity-50', offline)"),
        "connection script should dim the panel wrapper when offline"
    );
    // The favicon script checks the same shared flag before falling back to
    // the health-derived status.
    assert!(
        page.contains("document.body.dataset.bdConnection === 'offline'"),
        "favicon script should override to red when offline"
    );
}

/// (spec: web-ui — light/dark theme toggle; responsive layout for small viewports)
#[tokio::test]
async fn dashboard_includes_theme_toggle_viewport_and_responsive_grid_classes() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.duckdb");
    let (addr, _server) = test_server();
    let cfg = test_config(&db_path, &addr, &dir.path().join("marker.absent"));
    let db = Db::open_rw(&db_path).unwrap();
    collect_once(&db, &cfg).await;

    let state = AppState {
        db: db.clone(),
        cfg: Arc::new(cfg.clone()),
    };
    let router = build_router_with_bundle(state, test_asset_bundle());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move { topcoat::serve(listener, router).await });

    let page = reqwest::get(&url).await.unwrap().text().await.unwrap();
    assert!(
        page.contains(r#"name="viewport" content="width=device-width, initial-scale=1""#),
        "viewport meta tag expected"
    );
    assert!(
        page.contains(r#"id="bd-theme-toggle""#),
        "theme toggle button expected"
    );
    // The toggle reloads the page after persisting the choice, rather than
    // only flipping the class client-side, so the server-rendered page is
    // always the single source of truth for what's currently shown.
    assert!(
        page.contains("location.reload()"),
        "theme toggle should reload the page after persisting the choice"
    );
    assert!(
        page.contains("bd-panel-grid"),
        "responsive grid class expected"
    );
    assert!(
        page.contains("bd-panel-cell"),
        "responsive cell class expected"
    );
    assert!(
        page.contains(r#"<html class="">"#),
        "no theme cookie yet: no explicit class rendered"
    );
    // Header and footer stay pinned while the panel grid scrolls beneath
    // them (spec: web-ui — dashboard header and footer stay pinned while
    // scrolling): both use sticky positioning with an opaque background.
    let header_start = page.find(r#"class="sticky top-0"#).expect("sticky header wrapper expected");
    let banner_idx = page.find("bd-offline-banner").expect("offline banner expected");
    let h1_idx = page.find("bd-theme-toggle").expect("header content expected");
    assert!(
        header_start < banner_idx && banner_idx < h1_idx,
        "the offline banner and the h1 header must share the same sticky wrapper"
    );
    assert!(
        page.contains(r#"class="sticky bottom-0"#),
        "sticky footer expected"
    );

    // Setting the theme cookie server-side changes what the server renders on
    // the very next request, with no client-side bootstrap script involved —
    // "the backend knows the user's theme" (spec: web-ui — light/dark theme
    // toggle).
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{url}api/theme"))
        .json(&serde_json::json!({ "theme": "dark" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let set_cookie = resp
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let cookie_pair = set_cookie.split(';').next().unwrap().to_string();

    let page2 = client
        .get(&url)
        .header("Cookie", cookie_pair)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        page2.contains(r#"<html class="dark">"#),
        "server should render the dark class from the cookie"
    );

    // An invalid theme value is rejected, not silently accepted.
    let bad = client
        .post(format!("{url}api/theme"))
        .json(&serde_json::json!({ "theme": "purple" }))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), 400);
}

/// A generalized pane cell with no `title` and no `main` renders no title
/// span at all (spec: web-ui — panel title rendered on the card border).
#[tokio::test]
async fn untitled_pane_without_main_renders_no_title_span() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.duckdb");
    let toml = format!(
        r#"
database_path = "{db}"

[[sources]]
name = "cpu"
type = "script"
command = "echo 0"

[[layouts]]
title = "L"
rows = [
  [{{ secondary = ["cpu"] }}],
]
"#,
        db = db_path.display(),
    );
    let cfg: config::Config = toml::from_str(&toml).unwrap();
    config::validate(&cfg).unwrap();

    let db = Db::open_rw(&db_path).unwrap();
    collect_once(&db, &cfg).await;

    let state = AppState {
        db: db.clone(),
        cfg: Arc::new(cfg.clone()),
    };
    let router = build_router_with_bundle(state, test_asset_bundle());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move { topcoat::serve(listener, router).await });

    let html = reqwest::get(&url).await.unwrap().text().await.unwrap();
    assert!(html.contains("cpu"), "secondary member's log link expected");
    assert!(
        !html.contains(r#"class="absolute top-0 -translate-y-1/2"#),
        "an untitled, main-less pane should render no title span at all"
    );
}

/// A titled static-text panel renders its markdown content as HTML, with a
/// border-title span, no footer/log-link/history-bar, and no health/
/// threshold color (spec: web-ui — static-text panel rendering).
#[tokio::test]
async fn web_ui_text_cell_renders_titled_markdown_with_no_source_extras() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.duckdb");
    let toml = format!(
        r#"
database_path = "{db}"

[[layouts]]
title = "L"
rows = [
  [{{ title = "Links", format = "markdown", text = "- [GitHub](https://github.com)" }}],
]
"#,
        db = db_path.display(),
    );
    let cfg: config::Config = toml::from_str(&toml).unwrap();
    config::validate(&cfg).unwrap();

    let db = Db::open_rw(&db_path).unwrap();
    let state = AppState {
        db: db.clone(),
        cfg: Arc::new(cfg.clone()),
    };
    let router = build_router_with_bundle(state, test_asset_bundle());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move { topcoat::serve(listener, router).await });

    let html = reqwest::get(&url).await.unwrap().text().await.unwrap();
    // Border-title span, present and titled.
    assert!(
        html.contains(r#"class="absolute top-0 -translate-y-1/2"#),
        "text panel should render a border-title span"
    );
    assert!(html.contains(">Links<"), "text panel title expected");
    // Markdown rendered as HTML, not left as literal `- [GitHub]...` text.
    assert!(
        html.contains(r#"<a href="https://github.com">GitHub</a>"#),
        "markdown should render as HTML"
    );
    // No footer, no log link, no history bar — there's no source behind it.
    assert!(
        !html.contains("updated "),
        "a text panel has no age/footer text"
    );
    assert!(
        !html.contains("/logs/"),
        "a text panel has no per-source log link"
    );
    assert!(
        !html.contains("flex h-1"),
        "a text panel has no history bar"
    );
    // Never colored: no status_style-style border/background/text override.
    // Checked as the concrete `<property>:var(--status-...)` style pattern,
    // not a bare `--status-` substring, and past the page-wide offline
    // banner (spec: web-ui — global connection health indicator: offline
    // banner) — both legitimately reference `--status-red-*` on every page
    // regardless of content, which a bare/page-wide check would wrongly
    // trip on here.
    let banner_start = html.find(r#"id="bd-offline-banner""#).expect("offline banner expected");
    let banner_end = html[banner_start..]
        .find("</div>")
        .map_or(html.len(), |e| banner_start + e + "</div>".len());
    let mut scoped = html.clone();
    scoped.replace_range(banner_start..banner_end, "");
    assert!(
        !scoped.contains("color:var(--status-"),
        "a text panel's card must never carry a health/threshold color"
    );
}

/// An untitled static-text panel renders no title span, and defaults to
/// plain text when no `format` is declared (spec: web-ui — static-text panel
/// rendering).
#[tokio::test]
async fn web_ui_text_cell_without_title_or_format() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.duckdb");
    let toml = format!(
        r#"
database_path = "{db}"

[[layouts]]
title = "L"
rows = [
  [{{ text = "Just a note." }}],
]
"#,
        db = db_path.display(),
    );
    let cfg: config::Config = toml::from_str(&toml).unwrap();
    config::validate(&cfg).unwrap();

    let db = Db::open_rw(&db_path).unwrap();
    let state = AppState {
        db: db.clone(),
        cfg: Arc::new(cfg.clone()),
    };
    let router = build_router_with_bundle(state, test_asset_bundle());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move { topcoat::serve(listener, router).await });

    let html = reqwest::get(&url).await.unwrap().text().await.unwrap();
    assert!(
        !html.contains(r#"class="absolute top-0 -translate-y-1/2"#),
        "an untitled text panel should render no title span"
    );
    assert!(
        html.contains("Just a note."),
        "plain text content expected, rendered as-is"
    );
}

/// (spec: web-ui — source summary strip)
#[tokio::test]
async fn web_ui_summary_strip_lists_chips_in_layout_order_with_matching_colors() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.duckdb");
    let (addr, _server) = test_server();
    let cfg = test_config(&db_path, &addr, &dir.path().join("marker.absent"));

    let db = Db::open_rw(&db_path).unwrap();
    collect_once(&db, &cfg).await;

    let state = AppState {
        db: db.clone(),
        cfg: Arc::new(cfg.clone()),
    };
    let router = build_router_with_bundle(state, test_asset_bundle());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move { topcoat::serve(listener, router).await });

    let page = reqwest::get(&url).await.unwrap().text().await.unwrap();

    // Layout order: echo, Balance (source "balance"), then dead.
    let i_echo = page.find("#panel-echo").expect("echo chip link");
    let i_balance = page.find("#panel-balance").expect("balance chip link");
    let i_dead = page.find("#panel-dead").expect("dead chip link");
    assert!(
        i_echo < i_balance && i_balance < i_dead,
        "chips should follow layout order"
    );

    // "gated" isn't placed in any layout, so it must not get a chip.
    assert!(
        !page.contains("panel-gated"),
        "unplaced source should not get a chip"
    );

    // No thresholds are configured in test_config; `background-color` here
    // can only come from summary chips (panels use border+bg-50 style; both
    // are inline style, not a class: see `Panel::chip_style`).
    assert!(
        page.contains("background-color:var(--status-green-border)"),
        "healthy chip color expected"
    );
    assert!(
        page.contains("background-color:var(--status-red-border)"),
        "failing chip color expected"
    );
}

/// (spec: web-ui — source summary strip)
#[tokio::test]
async fn web_ui_summary_chip_href_matches_panel_id() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.duckdb");
    let (addr, _server) = test_server();
    let cfg = test_config(&db_path, &addr, &dir.path().join("marker.absent"));

    let db = Db::open_rw(&db_path).unwrap();
    collect_once(&db, &cfg).await;

    let state = AppState {
        db: db.clone(),
        cfg: Arc::new(cfg.clone()),
    };
    let router = build_router_with_bundle(state, test_asset_bundle());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move { topcoat::serve(listener, router).await });

    let page = reqwest::get(&url).await.unwrap().text().await.unwrap();
    assert!(
        page.contains(r##"href="#panel-balance""##),
        "chip href should target the panel's id"
    );
    assert!(
        page.contains(r#"id="panel-balance""#),
        "panel should carry the matching id"
    );
}

#[tokio::test]
async fn setup_command_gates_fetch_and_recovers() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.duckdb");
    let (addr, _server) = test_server();
    // Marker does not exist yet: setup must fail.
    let marker = dir.path().join("tunnel.up");
    let cfg = test_config(&db_path, &addr, &marker);

    let db = Db::open_rw(&db_path).unwrap();
    collect_once(&db, &cfg).await;

    // Failed setup logged with prefix, no reading fetched, health failing.
    let logs = db.logs(Some("gated"), 10).await.unwrap();
    assert_eq!(logs.len(), 1);
    let err = logs[0].error.as_deref().unwrap();
    assert!(err.starts_with("setup:"), "unexpected error: {err}");
    assert!(
        db.latest_values()
            .await
            .unwrap()
            .iter()
            .all(|r| r.source != "gated")
    );
    let h = health::compute(&db, &cfg, "gated").await.unwrap();
    assert_eq!(h.status, health::Health::Failing);

    // Dependency appears: next round's setup succeeds and fetching starts.
    std::fs::write(&marker, b"up").unwrap();
    collect_once(&db, &cfg).await;
    let latest = db.latest_values().await.unwrap();
    let gated = latest
        .iter()
        .find(|r| r.source == "gated")
        .expect("gated reading after recovery");
    assert_eq!(gated.value, "gated");
    let h = health::compute(&db, &cfg, "gated").await.unwrap();
    assert_eq!(h.status, health::Health::Healthy);
}

/// Spawns the real binary (chdir behavior lives in `main()`, not the library)
/// to prove `database_path` and a relative `script` command resolve against the
/// config file's own directory, not wherever the process was launched from
/// (spec: source-configuration — config-relative working directory).
#[tokio::test]
async fn daemon_resolves_relative_paths_against_config_directory() {
    let config_dir = tempfile::tempdir().unwrap();
    let launch_dir = tempfile::tempdir().unwrap();

    std::fs::write(
        config_dir.path().join("script.sh"),
        "#!/bin/sh\necho hello-from-script\n",
    )
    .unwrap();

    let config_path = config_dir.path().join("config.toml");
    std::fs::write(
        &config_path,
        r#"
database_path = "sub/data.duckdb"
listen = "127.0.0.1:0"

[[sources]]
name = "script-source"
type = "script"
command = "sh script.sh"
interval = "1s"
"#,
    )
    .unwrap();

    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_barduck"))
        .arg("--config")
        .arg(&config_path)
        .arg("daemon")
        .current_dir(launch_dir.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let _ = child.kill();
    let _ = child.wait();

    let db_path = config_dir.path().join("sub/data.duckdb");
    assert!(
        db_path.exists(),
        "database_path should resolve against the config file's directory"
    );
    assert!(!launch_dir.path().join("sub/data.duckdb").exists());

    let db = Db::open_rw(&db_path).unwrap();
    let latest = db.latest_values().await.unwrap();
    let reading = latest
        .iter()
        .find(|r| r.source == "script-source")
        .expect("relative script command should resolve and run against the config directory");
    assert_eq!(reading.value, "hello-from-script");
}

/// Proves a `cron`-scheduled source is actually driven by the collector's
/// scheduling loop (not just accepted by config validation): a source ticking
/// every second should accumulate several readings within a few seconds
/// (spec: data-collection — per-source schedules, cron schedule respected).
#[tokio::test]
async fn cron_schedule_fetches_repeatedly() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("cron.duckdb");
    let toml = format!(
        r#"
database_path = "{db}"

[[sources]]
name = "ticker"
type = "script"
command = "echo tick"
cron = "* * * * * *"
"#,
        db = db_path.display()
    );
    let cfg: config::Config = toml::from_str(&toml).unwrap();
    config::validate(&cfg).unwrap();

    let db = Db::open_rw(&db_path).unwrap();
    barduck::collector::spawn_all(&db, &cfg);

    tokio::time::sleep(std::time::Duration::from_millis(3500)).await;

    let hist = db.history("ticker", None, None, None).await.unwrap();
    assert!(
        hist.len() >= 2,
        "expected multiple cron-triggered fetches, got {}",
        hist.len()
    );
}

/// A source that fails every fetch retries at `retry_interval`, not the
/// (much longer) full `interval` (spec: data-collection — per-source
/// schedules, failed fetch retries sooner than the full interval).
#[tokio::test]
async fn failing_interval_source_retries_at_retry_interval() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("retry.duckdb");
    let toml = format!(
        r#"
database_path = "{db}"

[[sources]]
name = "flaky"
type = "script"
command = "sh -c 'exit 1'"
interval = "10s"
retry_interval = "100ms"
"#,
        db = db_path.display()
    );
    let cfg: config::Config = toml::from_str(&toml).unwrap();
    config::validate(&cfg).unwrap();

    let db = Db::open_rw(&db_path).unwrap();
    barduck::collector::spawn_all(&db, &cfg);

    tokio::time::sleep(std::time::Duration::from_millis(700)).await;

    let logs = db.logs(Some("flaky"), 100).await.unwrap();
    assert!(
        logs.len() >= 3,
        "expected several retry attempts within 700ms at a 100ms retry_interval (10s interval would give ~1), got {}",
        logs.len()
    );
    assert!(
        logs.iter().all(|l| l.error.is_some()),
        "every attempt should have failed"
    );
}

/// Once a retrying source's fetch succeeds, it resumes waiting the normal
/// `interval` instead of continuing to retry at `retry_interval` (spec:
/// data-collection — per-source schedules, recovery resumes the normal
/// interval).
#[tokio::test]
async fn recovered_interval_source_resumes_normal_interval() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("recover.duckdb");
    let marker = dir.path().join("recovered");
    let toml = format!(
        r#"
database_path = "{db}"

[[sources]]
name = "recovers"
type = "script"
command = "sh -c 'test -f {marker} && echo ok || (touch {marker}; exit 1)'"
interval = "2s"
retry_interval = "100ms"
"#,
        db = db_path.display(),
        marker = marker.display(),
    );
    let cfg: config::Config = toml::from_str(&toml).unwrap();
    config::validate(&cfg).unwrap();

    let db = Db::open_rw(&db_path).unwrap();
    barduck::collector::spawn_all(&db, &cfg);

    // First attempt (immediate) fails and creates the marker; the retry
    // 100ms later succeeds. If recovery didn't resume the 2s interval, a
    // third retry-interval-spaced attempt would land well before 900ms.
    tokio::time::sleep(std::time::Duration::from_millis(900)).await;

    let logs = db.logs(Some("recovers"), 100).await.unwrap();
    assert_eq!(
        logs.len(),
        2,
        "expected exactly one failed attempt then one successful attempt, then a pause for the full interval, got {} log entries",
        logs.len()
    );
    assert_eq!(
        logs.iter().filter(|l| l.error.is_none()).count(),
        1,
        "expected exactly one successful attempt"
    );
    assert_eq!(
        logs.iter().filter(|l| l.error.is_some()).count(),
        1,
        "expected exactly one failed attempt"
    );
}

/// A cron-scheduled source's next attempt is always the next cron
/// occurrence, never sped up by a failing fetch (spec: data-collection —
/// per-source schedules, cron-scheduled source ignores fetch outcome).
#[tokio::test]
async fn failing_cron_source_ignores_fetch_outcome() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("cron-fail.duckdb");
    let toml = format!(
        r#"
database_path = "{db}"

[[sources]]
name = "flaky-ticker"
type = "script"
command = "sh -c 'exit 1'"
cron = "* * * * * *"
"#,
        db = db_path.display()
    );
    let cfg: config::Config = toml::from_str(&toml).unwrap();
    config::validate(&cfg).unwrap();

    let db = Db::open_rw(&db_path).unwrap();
    barduck::collector::spawn_all(&db, &cfg);

    tokio::time::sleep(std::time::Duration::from_millis(3500)).await;

    let logs = db.logs(Some("flaky-ticker"), 100).await.unwrap();
    assert!(
        logs.len() >= 2,
        "expected several cron-cadence attempts despite every fetch failing, got {}",
        logs.len()
    );
    assert!(
        logs.iter().all(|l| l.error.is_some()),
        "every attempt should have failed"
    );
}

/// `latest` sets markdown-format ("text") sources aside from the scalar
/// table by default, and drops them entirely with `--no-text` (spec: cli —
/// query commands accept repeatable `--source` filters; this exercises the
/// analogous text/value separation).
#[tokio::test]
async fn cli_latest_separates_text_sources_from_the_table() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.duckdb");
    let toml = format!(
        r#"
database_path = "{db}"

[[sources]]
name = "cpu"
type = "script"
command = "echo 42"

[[sources]]
name = "notes"
type = "script"
command = "echo '# Heading'"
format = "markdown"
"#,
        db = db_path.display(),
    );
    let cfg: config::Config = toml::from_str(&toml).unwrap();
    config::validate(&cfg).unwrap();

    let db = Db::open_rw(&db_path).unwrap();
    collect_once(&db, &cfg).await;
    drop(db); // release the file lock before the subprocess opens it

    let config_path = dir.path().join("config.toml");
    std::fs::write(&config_path, &toml).unwrap();

    let default_output = std::process::Command::new(env!("CARGO_BIN_EXE_barduck"))
        .arg("--config")
        .arg(&config_path)
        .arg("latest")
        .output()
        .unwrap();
    let default_stdout = String::from_utf8_lossy(&default_output.stdout);
    assert!(
        default_output.status.success(),
        "latest should succeed: {default_stdout}"
    );
    let cpu_idx = default_stdout
        .find("cpu")
        .expect("cpu row expected in the table");
    let marker_idx = default_stdout
        .find("TEXT SOURCES")
        .expect("TEXT SOURCES section expected");
    assert!(
        cpu_idx < marker_idx,
        "the scalar table should come before the TEXT SOURCES section"
    );
    assert!(
        default_stdout[marker_idx..].contains("notes"),
        "the markdown source should appear in the text section"
    );
    assert!(
        !default_stdout[..marker_idx].contains("# Heading"),
        "the markdown source's content should not appear inside the scalar table"
    );

    let filtered_output = std::process::Command::new(env!("CARGO_BIN_EXE_barduck"))
        .arg("--config")
        .arg(&config_path)
        .arg("latest")
        .arg("--no-text")
        .output()
        .unwrap();
    let filtered_stdout = String::from_utf8_lossy(&filtered_output.stdout);
    assert!(filtered_output.status.success());
    assert!(
        filtered_stdout.contains("cpu"),
        "the scalar source should still be shown"
    );
    assert!(
        !filtered_stdout.contains("TEXT SOURCES"),
        "--no-text should drop the text section entirely"
    );
    assert!(
        !filtered_stdout.contains("notes"),
        "--no-text should exclude the markdown source entirely"
    );
}

/// A source's declared `value_type` populates the matching typed column on
/// `readings`, alongside the unchanged string `value` column (spec:
/// data-storage — typed value columns; source-configuration — configurable
/// stored value type).
#[tokio::test]
async fn typed_source_stores_matching_typed_column_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.duckdb");
    let toml = "[[sources]]\nname = \"count\"\ntype = \"script\"\ncommand = \"echo 7\"\nvalue_type = \"bigint\"\n\n\
                [[sources]]\nname = \"flag\"\ntype = \"script\"\ncommand = \"echo true\"\nvalue_type = \"json\"\n";
    let cfg: config::Config = toml::from_str(toml).unwrap();

    let db = Db::open_rw(&db_path).unwrap();
    collect_once(&db, &cfg).await;

    let conn = duckdb::Connection::open(&db_path).unwrap();
    let row = |source: &str| -> (String, Option<i64>, Option<f64>, Option<String>) {
        conn.query_row(
            "SELECT value, value_bigint, value_double, value_json FROM readings WHERE source = ?",
            duckdb::params![source],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, Option<String>>(3)?)),
        )
        .unwrap()
    };
    assert_eq!(row("count"), ("7".to_string(), Some(7), None, None));
    assert_eq!(
        row("flag"),
        ("true".to_string(), None, None, Some("true".to_string()))
    );
}

/// A source with no `value_type` keeps producing rows shaped exactly as
/// before this change: only `value` populated, all three new columns `NULL`
/// (spec: data-storage — typed value columns).
#[tokio::test]
async fn default_value_type_leaves_typed_columns_null() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.duckdb");
    let toml = "[[sources]]\nname = \"plain\"\ntype = \"script\"\ncommand = \"echo hello\"\n";
    let cfg: config::Config = toml::from_str(toml).unwrap();

    let db = Db::open_rw(&db_path).unwrap();
    collect_once(&db, &cfg).await;

    let conn = duckdb::Connection::open(&db_path).unwrap();
    let row: (String, Option<i64>, Option<f64>, Option<String>) = conn
        .query_row(
            "SELECT value, value_bigint, value_double, value_json FROM readings WHERE source = 'plain'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, Option<String>>(3)?)),
        )
        .unwrap();
    assert_eq!(row, ("hello".to_string(), None, None, None));
}

/// The log view shows a relative timestamp (not the raw stored one), colors
/// the VALUE cell by the source's threshold band, and offers a back link
/// (spec: web-ui — per-source log view linked from panels; log view relative
/// timestamps and threshold coloring).
#[tokio::test]
async fn log_view_shows_relative_time_threshold_color_and_back_link() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("t.duckdb");
    let toml = format!(
        r#"
database_path = "{}"

[[sources]]
name = "cpu"
type = "script"
command = "echo 92"
thresholds = [
  {{ bound = 60.0, level = "green" }},
  {{ bound = 85.0, level = "yellow" }},
  {{ bound = 100.0, level = "red" }},
]

[[layouts]]
title = "Overview"
rows = [["cpu"]]
"#,
        db_path.display()
    );
    let cfg: config::Config = toml::from_str(&toml).unwrap();

    let db = Db::open_rw(&db_path).unwrap();
    collect_once(&db, &cfg).await;
    let raw_ts = db.logs(Some("cpu"), 1).await.unwrap()[0].ts.clone();

    let state = AppState {
        db: db.clone(),
        cfg: Arc::new(cfg.clone()),
    };
    let router = build_router_with_bundle(state, test_asset_bundle());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/logs/cpu", listener.local_addr().unwrap());
    tokio::spawn(async move { topcoat::serve(listener, router).await });

    let html = reqwest::get(&url).await.unwrap().text().await.unwrap();

    assert!(
        html.contains(r#"<a href="/""#) && html.contains("Back to dashboard"),
        "log view should link back to the dashboard"
    );
    assert!(
        html.contains("ago") || html.contains("just now"),
        "TIME cell should show relative time"
    );
    assert!(
        html.contains(&format!("title=\"{raw_ts}\"")),
        "TIME cell should carry the full raw timestamp as a hover tooltip"
    );
    assert!(
        html.contains("color:var(--status-red-text)"),
        "VALUE cell should carry the red threshold color"
    );
    // Live updates (spec: web-ui — log view live-refreshes without a full
    // page reload): the table is served by the `log_rows` shard, wired the
    // same way the dashboard wires `panels_grid` — a tick signal, a runtime
    // script to drive it, and the shard's reactive-scope marker in the page.
    assert!(
        html.contains("data-bd-tick"),
        "log view should declare the same kind of tick signal the dashboard uses"
    );
    assert!(
        html.contains("/assets/bd-runtime.js"),
        "log view should load the runtime script the tick signal needs"
    );
    assert!(
        html.contains("::topcoat::scope::"),
        "log table should be a shard with a reactive scope"
    );
    // Shared page chrome (spec: web-ui — header and footer stay pinned
    // across pages): the log view renders through the same `page_chrome`
    // the dashboard uses, so it gets the same header and footer.
    assert!(
        html.contains(r#"id="bd-theme-toggle""#),
        "log view should share the dashboard's theme toggle"
    );
    assert!(
        html.contains(r#"class="sticky top-0"#),
        "log view should share the dashboard's sticky header"
    );
    assert!(
        html.contains(r#"class="sticky bottom-0"#),
        "log view should share the dashboard's sticky footer"
    );
}
