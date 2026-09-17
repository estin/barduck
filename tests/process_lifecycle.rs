#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use barduck::source::{StreamProc, run_shell};
use std::{fs::File, path::Path, process::Command, time::Duration};

// Invoked in a separate process by the lifecycle tests. Both generations hold
// a file lock: release proves actual termination, without PID-reuse races or
// treating a reaped parent's still-running descendants as success.
#[test]
#[ignore = "subprocess fixture; invoked by process lifecycle tests"]
fn process_fixture() {
    let descendant = std::env::var_os("BARDUCK_TEST_DESCENDANT").is_some();
    let name = if descendant { "descendant" } else { "parent" };
    let file = File::create(format!("{name}.lock")).unwrap();
    file.lock().unwrap();
    if descendant {
        std::fs::write("descendant.ready", "ready").unwrap();
        std::thread::sleep(Duration::from_secs(30));
    } else {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "process_fixture", "--nocapture"])
            .env("BARDUCK_TEST_DESCENDANT", "1")
            .spawn()
            .unwrap();
        std::fs::write("parent.ready", "ready").unwrap();
        // Bound fixture lifetime even if a regression leaves the tree alive.
        let _ = child.wait();
    }
}

fn fixture_command() -> String {
    let exe = std::env::current_exe().unwrap();
    #[cfg(unix)]
    let exe = format!("'{}'", exe.display().to_string().replace('\'', "'\\''"));
    #[cfg(windows)]
    let exe = format!("\"{}\"", exe.display());
    format!("{exe} --ignored --exact process_fixture --nocapture")
}

async fn tree_ready(dir: &Path) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !dir.join("parent.ready").exists() || !dir.join("descendant.ready").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("both process generations must start");
}

async fn tree_terminated(dir: &Path) {
    for name in ["parent", "descendant"] {
        let file = File::options()
            .read(true)
            .write(true)
            .open(dir.join(format!("{name}.lock")))
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match file.try_lock() {
                    Ok(()) => break,
                    Err(std::fs::TryLockError::WouldBlock) => {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                    Err(err) => panic!("checking {name} lifetime: {err}"),
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("{name} survived process-tree termination"));
    }
}

#[tokio::test]
async fn cancellation_terminates_parent_and_descendant() {
    let dir = tempfile::Builder::new()
        .prefix("barduck process ")
        .tempdir()
        .unwrap();
    let command = fixture_command();
    let mut pending = Box::pin(run_shell(&command, dir.path()));
    tokio::select! {
        result = &mut pending => panic!("fixture exited before cancellation: {result:?}"),
        () = tree_ready(dir.path()) => {}
    }
    drop(pending);
    tree_terminated(dir.path()).await;
}

#[tokio::test]
async fn stream_drop_terminates_parent_and_descendant() {
    let dir = tempfile::tempdir().unwrap();
    let stream = StreamProc::spawn(&fixture_command(), dir.path()).unwrap();
    tree_ready(dir.path()).await;
    drop(stream);
    tree_terminated(dir.path()).await;
}

#[tokio::test]
async fn stream_shutdown_terminates_parent_and_descendant() {
    let dir = tempfile::tempdir().unwrap();
    let mut stream = StreamProc::spawn(&fixture_command(), dir.path()).unwrap();
    tree_ready(dir.path()).await;
    tokio::time::timeout(Duration::from_secs(5), stream.shutdown())
        .await
        .expect("shutdown must reap the shell promptly");
    tree_terminated(dir.path()).await;
}

#[tokio::test]
async fn native_shell_preserves_quotes_working_directory_and_errors() {
    let dir = tempfile::Builder::new()
        .prefix("barduck shell ")
        .tempdir()
        .unwrap();
    std::fs::write(dir.path().join("value with spaces.txt"), "quoted value\n").unwrap();
    #[cfg(unix)]
    let command = "cat \"value with spaces.txt\" && echo second";
    #[cfg(windows)]
    let command = "type \"value with spaces.txt\" && echo second";
    let output = run_shell(command, dir.path()).await.unwrap();
    assert_eq!(output.replace("\r\n", "\n"), "quoted value\nsecond");

    #[cfg(unix)]
    let command = "echo failure >&2; exit 7";
    #[cfg(windows)]
    let command = "echo failure >&2 & exit /b 7";
    let error = run_shell(command, dir.path()).await.unwrap_err();
    assert!(error.to_string().contains("failure"), "{error:#}");

    #[cfg(unix)]
    let command = "printf 'first\\nsecond\\n'";
    #[cfg(windows)]
    let command = "echo first&echo second";
    let mut stream = StreamProc::spawn(command, dir.path()).unwrap();
    assert_eq!(stream.next_line().await.unwrap().as_deref(), Some("first"));
    assert_eq!(stream.next_line().await.unwrap().as_deref(), Some("second"));
    assert!(stream.next_line().await.unwrap().is_none());
    assert!(stream.wait_for_exit().await.unwrap().success());
}
