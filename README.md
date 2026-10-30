# obs-ctl

[![CI](https://github.com/OWNER/obs-ctl/actions/workflows/ci.yml/badge.svg)](https://github.com/OWNER/obs-ctl/actions)

A small, safe command-line tool for controlling OBS Studio (28+) through its built-in
WebSocket server (obs-websocket 5). It reads the host, port and password from OBS's
own `config.json`, so you never type or paste the password, and it never prints it.

Handy for hands-free or accessibility setups: bind the commands to hotkeys, a
Stream Deck, voice control or scripts.

## Install

    cargo install --path .

## Usage

    obs-ctl status | scenes | scene <name> | inputs
    obs-ctl record start|stop|toggle|status
    obs-ctl stream start|stop|toggle|status
    obs-ctl vcam start|stop|toggle|status
    obs-ctl mute|unmute|togglemute <source>
    obs-ctl screenshot [source] [file.png]
    obs-ctl deck [folder]      # build a 'Slides' scene (manual slideshow)
    obs-ctl next | prev | slide <n>
    obs-ctl --self-test        # read-only checks; never starts or stops anything

Run `obs-ctl --help` for the full list.

## Connection settings

By default obs-ctl reads `obs-websocket/config.json` from OBS's config folder
(`%APPDATA%\obs-studio\plugin_config\obs-websocket\` on Windows). Override with
`--host --port --password --config`, or the env vars
`OBS_HOST OBS_PORT OBS_PASSWORD OBS_CONFIG`. The slide folder defaults to
`$OBS_SLIDES_DIR`, else `./slides`.

## Development

    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    cargo test          # the live test skips cleanly if OBS isn't running

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
