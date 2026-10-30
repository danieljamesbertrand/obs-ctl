//! Integration test against a live OBS. Skips cleanly (passes with a note)
//! when OBS / its websocket server isn't reachable. Read-only: never records/streams.
use std::net::{TcpStream, ToSocketAddrs};
use std::process::Command;
use std::time::Duration;

fn reachable() -> Option<(String, u16)> {
    let path = obs_ctl::env_overrides()
        .config
        .or_else(obs_ctl::default_config_path)?;
    let file = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| obs_ctl::parse_ws_config(&t).ok());
    let c = obs_ctl::resolve_conn(
        file.as_ref(),
        &obs_ctl::env_overrides(),
        &Default::default(),
    );
    let addr = (c.host.as_str(), c.port).to_socket_addrs().ok()?.next()?;
    TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    Some((c.host, c.port))
}

fn run(args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_obs-ctl"))
        .args(args)
        .output()
        .expect("run obs-ctl");
    let s = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), s)
}

fn secret() -> Option<String> {
    let p = obs_ctl::default_config_path()?;
    obs_ctl::parse_ws_config(&std::fs::read_to_string(p).ok()?)
        .ok()?
        .password
}

#[test]
fn live_obs_readonly() {
    let Some((h, p)) = reachable() else {
        eprintln!("SKIP: OBS websocket not reachable; live test skipped");
        return;
    };
    eprintln!("live OBS at {h}:{p}");
    for args in [
        &["--self-test"][..],
        &["status"],
        &["scenes"],
        &["record", "status"],
        &["inputs"],
    ] {
        let (ok, out) = run(args);
        assert!(ok, "obs-ctl {args:?} failed:\n{out}");
        if let Some(pw) = secret() {
            assert!(!out.contains(&pw), "password leaked in output of {args:?}");
        }
    }
}

#[test]
fn bad_command_exits_2() {
    let (ok, out) = run(&["definitely-not-a-command"]);
    assert!(!ok);
    assert!(out.contains("unknown command"));
}

#[test]
fn help_works_offline() {
    let (ok, out) = run(&["help"]);
    assert!(ok && out.contains("--self-test"));
}
