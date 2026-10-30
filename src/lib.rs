//! obs-ctl library: config loading, argument parsing and name matching.
//! Everything here is pure (no network) so it is unit-tested below.

use std::fmt;
use std::path::{Path, PathBuf};

/// Connection settings. The password is never printed (custom Debug/Display).
#[derive(Clone, PartialEq)]
pub struct Conn {
    pub host: String,
    pub port: u16,
    pub password: Option<String>,
    pub source: String,
}

impl fmt::Debug for Conn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Conn {{ host: {:?}, port: {}, password: {}, source: {:?} }}",
            self.host,
            self.port,
            if self.password.is_some() {
                "<set>"
            } else {
                "<none>"
            },
            self.source
        )
    }
}

/// What we read out of obs-websocket's config.json.
#[derive(Debug, Clone, PartialEq)]
pub struct WsConfig {
    pub server_enabled: bool,
    pub server_port: u16,
    pub auth_required: bool,
    pub password: Option<String>,
}

/// Default location of obs-websocket's config on Windows.
pub fn default_config_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| {
        Path::new(&a)
            .join("obs-studio")
            .join("plugin_config")
            .join("obs-websocket")
            .join("config.json")
    })
}

/// Parse obs-websocket config.json text.
pub fn parse_ws_config(text: &str) -> Result<WsConfig, String> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("config.json is not valid JSON: {e}"))?;
    let obj = v.as_object().ok_or("config.json is not a JSON object")?;
    let server_enabled = obj
        .get("server_enabled")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let server_port = obj
        .get("server_port")
        .and_then(|x| x.as_u64())
        .map(|p| u16::try_from(p).map_err(|_| format!("server_port {p} out of range")))
        .transpose()?
        .unwrap_or(4455);
    let auth_required = obj
        .get("auth_required")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let password = obj
        .get("server_password")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    Ok(WsConfig {
        server_enabled,
        server_port,
        auth_required,
        password,
    })
}

/// Overrides from flags/env. `None` means "not given".
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Overrides {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub password: Option<String>,
    pub config: Option<PathBuf>,
}

/// Merge config file + env + flags. Precedence: flag > env > config.json > default.
pub fn resolve_conn(file: Option<&WsConfig>, env: &Overrides, flags: &Overrides) -> Conn {
    let host = flags
        .host
        .clone()
        .or_else(|| env.host.clone())
        .unwrap_or_else(|| "127.0.0.1".into());
    let port = flags
        .port
        .or(env.port)
        .or(file.map(|c| c.server_port))
        .unwrap_or(4455);
    let (password, source) = if let Some(p) = flags.password.clone() {
        (Some(p), "flag")
    } else if let Some(p) = env.password.clone() {
        (Some(p), "env")
    } else if let Some(c) = file {
        (
            if c.auth_required {
                c.password.clone()
            } else {
                None
            },
            "config.json",
        )
    } else {
        (None, "defaults")
    };
    Conn {
        host,
        port,
        password,
        source: source.into(),
    }
}

pub fn env_overrides() -> Overrides {
    Overrides {
        host: std::env::var("OBS_HOST").ok().filter(|s| !s.is_empty()),
        port: std::env::var("OBS_PORT").ok().and_then(|s| s.parse().ok()),
        password: std::env::var("OBS_PASSWORD").ok().filter(|s| !s.is_empty()),
        config: std::env::var_os("OBS_CONFIG").map(PathBuf::from),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    Help,
    SelfTest,
    Status,
    Scenes,
    Scene(String),
    Record(Action),
    Stream(Action),
    Mute(String, bool),
    ToggleMute(String),
    Inputs,
    Screenshot {
        source: Option<String>,
        file: Option<String>,
    },
    /// Build/refresh the 'Slides' scene with a manual slideshow.
    Deck(Option<String>),
    Next,
    Prev,
    Slide(u32),
    Vcam(Action),
}

pub const DECK_SCENE: &str = "Slides";
pub const DECK_INPUT: &str = "slideshow";
/// Default slide folder: `$OBS_SLIDES_DIR`, else `./slides`.
pub fn default_deck_dir() -> String {
    std::env::var("OBS_SLIDES_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "slides".into())
}

/// Image files in a folder, sorted by name (slide-01 .. slide-13).
pub fn slide_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("can't read {}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.to_str().map(looks_like_image_file).unwrap_or(false))
        .collect();
    v.sort();
    if v.is_empty() {
        return Err(format!("no images in {}", dir.display()));
    }
    Ok(v)
}

/// OBS image_source slideshow settings: manual advance, no loop, 1920x1080.
pub fn slideshow_settings(files: &[PathBuf]) -> serde_json::Value {
    serde_json::json!({
        "slide_mode": "mode_manual",
        "loop": false,
        "randomize": false,
        "transition": "cut",
        "use_custom_size": "1920x1080",
        "playback_behavior": "always_play",
        "hide": false,
        "files": files.iter().map(|f| serde_json::json!({
            "value": f.to_string_lossy(), "hidden": false, "selected": false
        })).collect::<Vec<_>>()
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    Start,
    Stop,
    Toggle,
    Status,
}

fn parse_action(s: Option<&str>) -> Result<Action, String> {
    match s.map(|x| x.to_ascii_lowercase()).as_deref() {
        None | Some("status") | Some("st") => Ok(Action::Status),
        Some("start") | Some("on") | Some("go") | Some("begin") => Ok(Action::Start),
        Some("stop") | Some("off") | Some("end") => Ok(Action::Stop),
        Some("toggle") | Some("t") | Some("tog") => Ok(Action::Toggle),
        Some(o) => Err(format!(
            "unknown action '{o}' (use start, stop, toggle or status)"
        )),
    }
}

/// Parse argv (without program name). Returns (command, flag overrides).
pub fn parse_args(args: &[String]) -> Result<(Cmd, Overrides), String> {
    let mut ov = Overrides::default();
    let mut rest: Vec<String> = Vec::new();
    let mut self_test = false;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let lower = a.to_ascii_lowercase();
        let mut take = |name: &str| -> Result<String, String> {
            i += 1;
            args.get(i)
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match lower.as_str() {
            "--self-test" | "--selftest" | "self-test" | "selftest" | "test" => self_test = true,
            "--host" => ov.host = Some(take("--host")?),
            "--port" => {
                let v = take("--port")?;
                ov.port = Some(v.parse().map_err(|_| format!("bad port '{v}'"))?)
            }
            "--password" => ov.password = Some(take("--password")?),
            "--config" => ov.config = Some(PathBuf::from(take("--config")?)),
            "-h" | "--help" | "help" | "/?" => return Ok((Cmd::Help, ov)),
            _ => rest.push(a.clone()),
        }
        i += 1;
    }
    if self_test {
        return Ok((Cmd::SelfTest, ov));
    }
    let Some(first) = rest.first() else {
        return Ok((Cmd::Status, ov));
    };
    let tail = rest[1..].join(" ");
    let tail_opt = if tail.trim().is_empty() {
        None
    } else {
        Some(tail.trim().to_string())
    };
    let cmd = match first.to_ascii_lowercase().as_str() {
        "status" | "st" | "s" => Cmd::Status,
        "scenes" | "list" | "ls" => Cmd::Scenes,
        "scene" | "sc" | "go" | "switch" => match tail_opt {
            Some(n) => Cmd::Scene(n),
            None => Cmd::Scenes,
        },
        "record" | "rec" | "r" => Cmd::Record(parse_action(rest.get(1).map(|s| s.as_str()))?),
        "stream" | "live" => Cmd::Stream(parse_action(rest.get(1).map(|s| s.as_str()))?),
        "mute" | "m" => Cmd::Mute(tail_opt.ok_or("mute needs a source name")?, true),
        "unmute" | "um" => Cmd::Mute(tail_opt.ok_or("unmute needs a source name")?, false),
        "togglemute" | "tm" => Cmd::ToggleMute(tail_opt.ok_or("togglemute needs a source name")?),
        "inputs" | "sources" | "audio" => Cmd::Inputs,
        "deck" | "pitch" | "slides" => Cmd::Deck(tail_opt),
        "next" | "n" | "fwd" | "forward" => Cmd::Next,
        "prev" | "p" | "back" | "previous" => Cmd::Prev,
        "slide" => {
            let t = tail_opt.ok_or("slide needs a number, e.g. slide 3")?;
            let n: u32 = t.parse().map_err(|_| format!("bad slide number '{t}'"))?;
            if n == 0 {
                return Err("slides start at 1".into());
            }
            Cmd::Slide(n)
        }
        "vcam" | "camera" | "virtualcam" => {
            Cmd::Vcam(parse_action(rest.get(1).map(|s| s.as_str()))?)
        }
        "screenshot" | "shot" | "ss" => {
            // screenshot [source...] [file]; a trailing arg with an image extension is the file.
            let mut parts: Vec<String> = rest[1..].to_vec();
            let file = if parts
                .last()
                .map(|p| looks_like_image_file(p))
                .unwrap_or(false)
            {
                parts.pop()
            } else {
                None
            };
            let src = parts.join(" ");
            Cmd::Screenshot {
                source: if src.trim().is_empty() {
                    None
                } else {
                    Some(src.trim().to_string())
                },
                file,
            }
        }
        other => return Err(format!("unknown command '{other}'. Try: obs-ctl help")),
    };
    Ok((cmd, ov))
}

pub fn looks_like_image_file(s: &str) -> bool {
    image_format(s).is_some()
}

/// Image format for OBS from a file extension.
pub fn image_format(file: &str) -> Option<&'static str> {
    let ext = Path::new(file).extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("png"),
        "jpg" | "jpeg" => Some("jpg"),
        "bmp" => Some("bmp"),
        "webp" => Some("webp"),
        _ => None,
    }
}

/// Forgiving name match: exact, then case-insensitive exact, then unique
/// case-insensitive prefix, then unique substring. Errors list candidates.
pub fn match_name<'a>(query: &str, names: &'a [String]) -> Result<&'a str, String> {
    let q = query.trim();
    if let Some(n) = names.iter().find(|n| n.as_str() == q) {
        return Ok(n);
    }
    let ql = q.to_lowercase();
    let ci: Vec<&String> = names.iter().filter(|n| n.to_lowercase() == ql).collect();
    if ci.len() == 1 {
        return Ok(ci[0]);
    }
    for pass in 0..2 {
        let hits: Vec<&String> = names
            .iter()
            .filter(|n| {
                let nl = n.to_lowercase();
                if pass == 0 {
                    nl.starts_with(&ql)
                } else {
                    nl.contains(&ql)
                }
            })
            .collect();
        match hits.len() {
            1 => return Ok(hits[0]),
            0 => continue,
            _ => {
                return Err(format!(
                    "'{q}' matches several: {}",
                    hits.iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            }
        }
    }
    Err(format!(
        "no match for '{q}'. Available: {}",
        names.join(", ")
    ))
}

pub const HELP: &str =
    "obs-ctl - control OBS Studio (reads host/port/password from OBS's own config)

  obs-ctl                     status (same as 'obs-ctl status')
  obs-ctl scenes              list scenes (* = live)
  obs-ctl scene <name>        switch scene (case-insensitive, prefix ok)
  obs-ctl record start|stop|toggle|status
  obs-ctl stream start|stop|toggle|status
  obs-ctl inputs              list inputs with mute state
  obs-ctl mute <source>       mute an input   (unmute / togglemute too)
  obs-ctl screenshot [source] [file.png]   default: live scene -> Desktop
  obs-ctl deck [folder]       make 'Slides' scene (manual slideshow; default folder $OBS_SLIDES_DIR or ./slides)
  obs-ctl next | prev | slide <n>   drive the slideshow
  obs-ctl vcam start|stop|status    OBS Virtual Camera (for Teams)
  obs-ctl --self-test         read-only checks, PASS/FAIL each

Overrides: --host --port --password --config, or env OBS_HOST OBS_PORT OBS_PASSWORD OBS_CONFIG
Short forms: rec, r, st, ls, sc, m, um, tm, ss, n, p, back";

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_full_config() {
        let c = parse_ws_config(r#"{"alerts_enabled":false,"auth_required":true,"first_load":false,"server_enabled":true,"server_password":"secret123","server_port":4455}"#).unwrap();
        assert!(c.server_enabled && c.auth_required);
        assert_eq!(c.server_port, 4455);
        assert_eq!(c.password.as_deref(), Some("secret123"));
    }

    #[test]
    fn config_defaults_and_empty_password() {
        let c = parse_ws_config(r#"{"server_password":""}"#).unwrap();
        assert!(!c.server_enabled);
        assert_eq!(c.server_port, 4455);
        assert_eq!(c.password, None);
    }

    #[test]
    fn config_rejects_garbage() {
        assert!(parse_ws_config("not json").is_err());
        assert!(parse_ws_config("[1,2]").is_err());
        assert!(parse_ws_config(r#"{"server_port":99999}"#).is_err());
    }

    #[test]
    fn precedence_flag_env_file() {
        let file = WsConfig {
            server_enabled: true,
            server_port: 4455,
            auth_required: true,
            password: Some("f".into()),
        };
        let env = Overrides {
            port: Some(5000),
            password: Some("e".into()),
            ..Default::default()
        };
        let flags = Overrides {
            password: Some("x".into()),
            ..Default::default()
        };
        let c = resolve_conn(Some(&file), &env, &flags);
        assert_eq!(
            (c.port, c.password.as_deref(), c.source.as_str()),
            (5000, Some("x"), "flag")
        );
        let c = resolve_conn(Some(&file), &env, &Overrides::default());
        assert_eq!(
            (c.password.as_deref(), c.source.as_str()),
            (Some("e"), "env")
        );
        let c = resolve_conn(Some(&file), &Overrides::default(), &Overrides::default());
        assert_eq!(
            (c.host.as_str(), c.port, c.password.as_deref()),
            ("127.0.0.1", 4455, Some("f"))
        );
    }

    #[test]
    fn no_password_when_auth_off() {
        let file = WsConfig {
            server_enabled: true,
            server_port: 4455,
            auth_required: false,
            password: Some("f".into()),
        };
        let c = resolve_conn(Some(&file), &Overrides::default(), &Overrides::default());
        assert_eq!(c.password, None);
    }

    #[test]
    fn debug_never_leaks_password() {
        let c = Conn {
            host: "h".into(),
            port: 1,
            password: Some("TOPSECRET".into()),
            source: "x".into(),
        };
        let s = format!("{c:?}");
        assert!(!s.contains("TOPSECRET"));
        assert!(s.contains("<set>"));
    }

    #[test]
    fn parses_commands() {
        assert_eq!(parse_args(&a(&[])).unwrap().0, Cmd::Status);
        assert_eq!(parse_args(&a(&["SCENES"])).unwrap().0, Cmd::Scenes);
        assert_eq!(
            parse_args(&a(&["scene", "Main", "Cam"])).unwrap().0,
            Cmd::Scene("Main Cam".into())
        );
        assert_eq!(
            parse_args(&a(&["rec", "Toggle"])).unwrap().0,
            Cmd::Record(Action::Toggle)
        );
        assert_eq!(
            parse_args(&a(&["record"])).unwrap().0,
            Cmd::Record(Action::Status)
        );
        assert_eq!(
            parse_args(&a(&["stream", "off"])).unwrap().0,
            Cmd::Stream(Action::Stop)
        );
        assert_eq!(
            parse_args(&a(&["mute", "Mic/Aux"])).unwrap().0,
            Cmd::Mute("Mic/Aux".into(), true)
        );
        assert_eq!(
            parse_args(&a(&["unmute", "desktop", "audio"])).unwrap().0,
            Cmd::Mute("desktop audio".into(), false)
        );
        assert_eq!(parse_args(&a(&["--self-test"])).unwrap().0, Cmd::SelfTest);
        assert!(parse_args(&a(&["record", "explode"])).is_err());
        assert!(parse_args(&a(&["mute"])).is_err());
        assert!(parse_args(&a(&["bogus"])).is_err());
    }

    #[test]
    fn parses_flags() {
        let (c, o) = parse_args(&a(&["--port", "4460", "--host", "10.0.0.2", "status"])).unwrap();
        assert_eq!(c, Cmd::Status);
        assert_eq!((o.port, o.host.as_deref()), (Some(4460), Some("10.0.0.2")));
        assert!(parse_args(&a(&["--port", "abc"])).is_err());
        assert!(parse_args(&a(&["--port"])).is_err());
    }

    #[test]
    fn parses_screenshot() {
        assert_eq!(
            parse_args(&a(&["ss"])).unwrap().0,
            Cmd::Screenshot {
                source: None,
                file: None
            }
        );
        assert_eq!(
            parse_args(&a(&["screenshot", "Game", "Capture", "C:\\x\\a.PNG"]))
                .unwrap()
                .0,
            Cmd::Screenshot {
                source: Some("Game Capture".into()),
                file: Some("C:\\x\\a.PNG".into())
            }
        );
        assert_eq!(image_format("a.jpeg"), Some("jpg"));
        assert_eq!(image_format("a.txt"), None);
    }

    #[test]
    fn parses_slide_commands() {
        assert_eq!(parse_args(&a(&["NEXT"])).unwrap().0, Cmd::Next);
        assert_eq!(parse_args(&a(&["back"])).unwrap().0, Cmd::Prev);
        assert_eq!(parse_args(&a(&["slide", "5"])).unwrap().0, Cmd::Slide(5));
        assert!(parse_args(&a(&["slide", "0"])).is_err());
        assert!(parse_args(&a(&["slide", "x"])).is_err());
        assert_eq!(parse_args(&a(&["deck"])).unwrap().0, Cmd::Deck(None));
        assert_eq!(
            parse_args(&a(&["vcam", "start"])).unwrap().0,
            Cmd::Vcam(Action::Start)
        );
    }

    #[test]
    fn slideshow_settings_are_manual_ordered() {
        let dir = std::env::temp_dir().join(format!("obsctl-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["slide-02.png", "slide-01.png", "notes.txt", "slide-10.PNG"] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        let files = slide_files(&dir).unwrap();
        let names: Vec<_> = files
            .iter()
            .map(|f| f.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, ["slide-01.png", "slide-02.png", "slide-10.PNG"]);
        let s = slideshow_settings(&files);
        assert_eq!(s["slide_mode"], "mode_manual");
        assert_eq!(s["loop"], false);
        assert_eq!(s["use_custom_size"], "1920x1080");
        assert_eq!(s["files"].as_array().unwrap().len(), 3);
        std::fs::remove_dir_all(&dir).ok();
        assert!(slide_files(&dir).is_err());
    }

    #[test]
    fn name_matching() {
        let n = a(&["Main Cam", "Main Screen", "BRB", "Desktop Audio", "Mic/Aux"]);
        assert_eq!(match_name("brb", &n).unwrap(), "BRB");
        assert_eq!(match_name("main cam", &n).unwrap(), "Main Cam");
        assert_eq!(match_name("main s", &n).unwrap(), "Main Screen");
        assert_eq!(match_name("mic", &n).unwrap(), "Mic/Aux");
        assert_eq!(match_name("audio", &n).unwrap(), "Desktop Audio");
        assert!(match_name("main", &n).unwrap_err().contains("several"));
        assert!(match_name("zzz", &n).unwrap_err().contains("no match"));
    }

    #[test]
    fn exact_case_wins_over_ci_duplicates() {
        let n = a(&["Scene", "scene"]);
        assert_eq!(match_name("scene", &n).unwrap(), "scene");
        assert_eq!(match_name("Scene", &n).unwrap(), "Scene");
    }
}
