use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use obs_ctl::*;
use obws::common::MediaAction;
use obws::requests::inputs::{Create, InputId, SetSettings};
use obws::requests::sources::{SaveScreenshot, SourceId};
use obws::Client;

type Res<T> = Result<T, String>;

fn load_conn(flags: &Overrides) -> Res<(Conn, Option<WsConfig>, Option<PathBuf>)> {
    let env = env_overrides();
    let path = flags
        .config
        .clone()
        .or(env.config.clone())
        .or_else(default_config_path);
    let file = match &path {
        Some(p) if p.exists() => {
            let text = std::fs::read_to_string(p)
                .map_err(|e| format!("can't read {}: {e}", p.display()))?;
            Some(parse_ws_config(&text)?)
        }
        _ => None,
    };
    Ok((resolve_conn(file.as_ref(), &env, flags), file, path))
}

async fn connect(c: &Conn) -> Res<Client> {
    match tokio::time::timeout(
        Duration::from_secs(6),
        Client::connect(&c.host, c.port, c.password.as_deref()),
    )
    .await
    {
        Ok(Ok(cl)) => Ok(cl),
        Ok(Err(e)) => Err(format!(
            "can't connect to OBS at {}:{} ({e}). Is OBS open with Tools > WebSocket Server Settings > Enable on?",
            c.host, c.port
        )),
        Err(_) => Err(format!("timed out connecting to OBS at {}:{}", c.host, c.port)),
    }
}

fn e<E: std::fmt::Display>(x: E) -> String {
    x.to_string()
}

fn fmt_dur(d: time::Duration) -> String {
    let s = d.whole_seconds().max(0);
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

async fn scene_names(cl: &Client) -> Res<(Vec<String>, Option<String>)> {
    let s = cl.scenes().list().await.map_err(e)?;
    let mut scenes = s.scenes;
    scenes.sort_by_key(|x| std::cmp::Reverse(x.index)); // OBS UI order (top first)
    Ok((
        scenes.into_iter().map(|x| x.id.name).collect(),
        s.current_program_scene.map(|x| x.name),
    ))
}

async fn input_names(cl: &Client) -> Res<Vec<String>> {
    Ok(cl
        .inputs()
        .list(None)
        .await
        .map_err(e)?
        .into_iter()
        .map(|i| i.id.name)
        .collect())
}

async fn run(cmd: Cmd, conn: &Conn) -> Res<()> {
    let cl = connect(conn).await?;
    match cmd {
        Cmd::Status => {
            let (_, cur) = scene_names(&cl).await?;
            let r = cl.recording().status().await.map_err(e)?;
            let s = cl.streaming().status().await.map_err(e)?;
            println!("Scene:  {}", cur.unwrap_or_else(|| "?".into()));
            println!(
                "Record: {}",
                if r.active {
                    format!(
                        "ON{} {}",
                        if r.paused { " (paused)" } else { "" },
                        fmt_dur(r.duration)
                    )
                } else {
                    "off".into()
                }
            );
            println!(
                "Stream: {}",
                if s.active {
                    format!("LIVE {}", fmt_dur(s.duration))
                } else {
                    "off".into()
                }
            );
        }
        Cmd::Scenes => {
            let (names, cur) = scene_names(&cl).await?;
            for n in names {
                println!("{} {n}", if Some(&n) == cur.as_ref() { "*" } else { " " });
            }
        }
        Cmd::Scene(q) => {
            let (names, _) = scene_names(&cl).await?;
            let n = match_name(&q, &names)?;
            cl.scenes().set_current_program_scene(n).await.map_err(e)?;
            println!("Scene -> {n}");
        }
        Cmd::Record(a) => {
            let rec = cl.recording();
            match a {
                Action::Start => {
                    rec.start().await.map_err(e)?;
                    println!("Recording started");
                }
                Action::Stop => {
                    let p = rec.stop().await.map_err(e)?;
                    println!("Recording stopped: {p}");
                }
                Action::Toggle => {
                    let on = rec.toggle().await.map_err(e)?;
                    println!("Recording {}", if on { "started" } else { "stopped" });
                }
                Action::Status => {
                    let r = rec.status().await.map_err(e)?;
                    println!(
                        "Record: {}",
                        if r.active {
                            format!("ON {}", fmt_dur(r.duration))
                        } else {
                            "off".into()
                        }
                    );
                }
            }
        }
        Cmd::Stream(a) => {
            let st = cl.streaming();
            match a {
                Action::Start => {
                    st.start().await.map_err(e)?;
                    println!("Stream started");
                }
                Action::Stop => {
                    st.stop().await.map_err(e)?;
                    println!("Stream stopped");
                }
                Action::Toggle => {
                    let on = st.toggle().await.map_err(e)?;
                    println!("Stream {}", if on { "started" } else { "stopped" });
                }
                Action::Status => {
                    let s = st.status().await.map_err(e)?;
                    println!(
                        "Stream: {}",
                        if s.active {
                            format!("LIVE {}", fmt_dur(s.duration))
                        } else {
                            "off".into()
                        }
                    );
                }
            }
        }
        Cmd::Inputs => {
            for n in input_names(&cl).await? {
                let m = cl.inputs().muted(InputId::Name(&n)).await.ok();
                let tag = match m {
                    Some(true) => "[muted]",
                    Some(false) => "[on]   ",
                    None => "       ",
                };
                println!("{tag} {n}");
            }
        }
        Cmd::Mute(q, mute) => {
            let names = input_names(&cl).await?;
            let n = match_name(&q, &names)?;
            cl.inputs()
                .set_muted(InputId::Name(n), mute)
                .await
                .map_err(e)?;
            println!("{n}: {}", if mute { "muted" } else { "unmuted" });
        }
        Cmd::ToggleMute(q) => {
            let names = input_names(&cl).await?;
            let n = match_name(&q, &names)?;
            let m = cl.inputs().toggle_mute(InputId::Name(n)).await.map_err(e)?;
            println!("{n}: {}", if m { "muted" } else { "unmuted" });
        }
        Cmd::Screenshot { source, file } => {
            let (scenes, cur) = scene_names(&cl).await?;
            let src: String = match source {
                None => cur.ok_or("no live scene")?,
                Some(q) => {
                    let mut all = scenes.clone();
                    all.extend(input_names(&cl).await?);
                    all.dedup();
                    match_name(&q, &all)?.to_string()
                }
            };
            let file = match file {
                Some(f) => PathBuf::from(f),
                None => {
                    let desk = std::env::var_os("USERPROFILE")
                        .map(PathBuf::from)
                        .unwrap_or_default()
                        .join("Desktop");
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    desk.join(format!("obs-shot-{ts}.png"))
                }
            };
            let file = if file.is_absolute() {
                file
            } else {
                std::env::current_dir().map_err(e)?.join(file)
            };
            let fs = file.to_string_lossy().to_string();
            let fmt = image_format(&fs).ok_or("file must end in .png, .jpg, .bmp or .webp")?;
            cl.sources()
                .save_screenshot(SaveScreenshot {
                    source: SourceId::Name(&src),
                    format: fmt,
                    width: None,
                    height: None,
                    compression_quality: None,
                    file_path: &file,
                })
                .await
                .map_err(e)?;
            println!("Saved {src} -> {fs}");
        }
        Cmd::Deck(dir) => {
            let dir = PathBuf::from(dir.unwrap_or_else(default_deck_dir));
            let files = slide_files(&dir)?;
            let settings = slideshow_settings(&files);
            let (scenes, _) = scene_names(&cl).await?;
            if !scenes.iter().any(|s| s == DECK_SCENE) {
                cl.scenes().create(DECK_SCENE).await.map_err(e)?;
            }
            let inputs = input_names(&cl).await?;
            if inputs.iter().any(|i| i == DECK_INPUT) {
                cl.inputs()
                    .set_settings(SetSettings {
                        input: InputId::Name(DECK_INPUT),
                        settings: &settings,
                        overlay: Some(false),
                    })
                    .await
                    .map_err(e)?;
            } else {
                let kinds = cl.inputs().list_kinds(false).await.map_err(e)?;
                let kind = if kinds.iter().any(|k| k == "slideshow_v2") {
                    "slideshow_v2"
                } else {
                    "slideshow"
                };
                cl.inputs()
                    .create(Create {
                        scene: DECK_SCENE.into(),
                        input: DECK_INPUT,
                        kind,
                        settings: Some(&settings),
                        enabled: Some(true),
                    })
                    .await
                    .map_err(e)?;
            }
            cl.scenes()
                .set_current_program_scene(DECK_SCENE)
                .await
                .map_err(e)?;
            let _ = cl
                .media_inputs()
                .trigger_action(InputId::Name(DECK_INPUT), MediaAction::Restart)
                .await;
            println!(
                "Scene '{DECK_SCENE}' live with {} slides from {}",
                files.len(),
                dir.display()
            );
        }
        Cmd::Next | Cmd::Prev => {
            let fwd = matches!(cmd, Cmd::Next);
            let a = if fwd {
                MediaAction::Next
            } else {
                MediaAction::Previous
            };
            cl.media_inputs()
                .trigger_action(InputId::Name(DECK_INPUT), a)
                .await
                .map_err(e)?;
            println!("{}", if fwd { "Next slide" } else { "Previous slide" });
        }
        Cmd::Slide(n) => {
            let mi = cl.media_inputs();
            mi.trigger_action(InputId::Name(DECK_INPUT), MediaAction::Restart)
                .await
                .map_err(e)?;
            for _ in 1..n {
                tokio::time::sleep(Duration::from_millis(60)).await;
                mi.trigger_action(InputId::Name(DECK_INPUT), MediaAction::Next)
                    .await
                    .map_err(e)?;
            }
            println!("Slide {n}");
        }
        Cmd::Vcam(a) => {
            let v = cl.virtual_cam();
            match a {
                Action::Start => {
                    v.start().await.map_err(e)?;
                    println!("Virtual camera ON (pick 'OBS Virtual Camera' in Teams)");
                }
                Action::Stop => {
                    v.stop().await.map_err(e)?;
                    println!("Virtual camera off");
                }
                Action::Toggle => {
                    let on = v.toggle().await.map_err(e)?;
                    println!("Virtual camera {}", if on { "ON" } else { "off" });
                }
                Action::Status => {
                    let on = v.status().await.map_err(e)?;
                    println!("Virtual camera: {}", if on { "ON" } else { "off" });
                }
            }
        }
        Cmd::Help | Cmd::SelfTest => unreachable!(),
    }
    Ok(())
}

/// Read-only checks. Never starts/stops anything, never prints the password.
async fn self_test(flags: &Overrides) -> bool {
    let mut ok = true;
    let mut check = |name: &str, r: Res<String>| match r {
        Ok(m) => println!("PASS  {name}: {m}"),
        Err(m) => {
            ok = false;
            println!("FAIL  {name}: {m}");
        }
    };
    let loaded = load_conn(flags);
    let conn = match loaded {
        Ok((c, file, path)) => {
            let p = path
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "?".into());
            check(
                "config readable",
                match &file {
                    Some(f) => Ok(format!(
                        "{p} (server_enabled={}, port={}, auth={}, password {})",
                        f.server_enabled,
                        f.server_port,
                        f.auth_required,
                        if f.password.is_some() {
                            "set"
                        } else {
                            "not set"
                        }
                    )),
                    None => Err(format!("not found at {p}")),
                },
            );
            if let Some(f) = &file {
                check(
                    "server enabled",
                    if f.server_enabled {
                        Ok("yes".into())
                    } else {
                        Err("off - enable in OBS: Tools > WebSocket Server Settings".into())
                    },
                );
            }
            c
        }
        Err(m) => {
            check("config readable", Err(m));
            return false;
        }
    };
    let cl = match connect(&conn).await {
        Ok(cl) => {
            check(
                "connect",
                Ok(format!(
                    "{}:{} (credentials from {})",
                    conn.host, conn.port, conn.source
                )),
            );
            cl
        }
        Err(m) => {
            check("connect", Err(m));
            return false;
        }
    };
    check(
        "version",
        cl.general()
            .version()
            .await
            .map(|v| {
                format!(
                    "OBS {} / websocket {} / rpc {}",
                    v.obs_studio_version, v.obs_web_socket_version, v.rpc_version
                )
            })
            .map_err(e),
    );
    check(
        "list scenes",
        scene_names(&cl).await.and_then(|(n, cur)| {
            if n.is_empty() {
                Err("no scenes".into())
            } else {
                Ok(format!(
                    "{} scene(s), live: {}",
                    n.len(),
                    cur.unwrap_or_else(|| "?".into())
                ))
            }
        }),
    );
    check(
        "record status (read-only)",
        cl.recording()
            .status()
            .await
            .map(|r| {
                if r.active {
                    "recording".into()
                } else {
                    "idle".into()
                }
            })
            .map_err(e),
    );
    check(
        "stream status (read-only)",
        cl.streaming()
            .status()
            .await
            .map(|s| {
                if s.active {
                    "live".into()
                } else {
                    "idle".into()
                }
            })
            .map_err(e),
    );
    check(
        "list inputs",
        input_names(&cl)
            .await
            .map(|n| format!("{} input(s)", n.len())),
    );
    ok
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cmd, flags) = match parse_args(&args) {
        Ok(x) => x,
        Err(m) => {
            eprintln!("error: {m}\n\n{HELP}");
            return ExitCode::from(2);
        }
    };
    match cmd {
        Cmd::Help => {
            println!("{HELP}");
            ExitCode::SUCCESS
        }
        Cmd::SelfTest => {
            if self_test(&flags).await {
                println!("SELF-TEST: ALL PASS");
                ExitCode::SUCCESS
            } else {
                println!("SELF-TEST: FAILED");
                ExitCode::FAILURE
            }
        }
        cmd => {
            let r = match load_conn(&flags) {
                Ok((c, _, _)) => run(cmd, &c).await,
                Err(m) => Err(m),
            };
            match r {
                Ok(()) => ExitCode::SUCCESS,
                Err(m) => {
                    eprintln!("error: {m}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}
