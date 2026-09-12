# majsoul-autopilot

[中文文档](README.md)

A pure Rust Mahjong Soul autopilot powered by the Mortal model and the Liqi protocol.

The project provides a desktop GUI and a command-line tool. It logs in with an email account, joins ranked four-player rooms, connects to live games through the Liqi websocket protocol, and lets a Mortal model choose actions.

## Features

- Pure protocol automation without browser dependencies, screenshots, or coordinate clicking
- Dynamic client & resource version detection (adapts to game updates automatically)
- Four-player ranked matchmaking
- Automatic room selection by rank (Bronze / Silver / Gold / Jade / Throne)
- Native Mortal inference with Candle (pure Rust, no Python runtime needed)
- Tauri desktop GUI for settings, status, logs, and table view
- Reconnect support for active games
- Riichi declaration handling with Mortal's two-step decision flow
- Stale operation guard and discard acknowledgement checks
- Robust game-end handling (dealer agari-yame, tobu bankrupt detection, server code 1204 tolerance)

## Room Policy

The runner selects a target room based on the account rank:

| Rank | Target Room |
| --- | --- |
| Below Adept (Novice) | Bronze Room, East game |
| Adept | Silver Room, South game |
| Expert | Gold Room, South game |
| Master | Jade Room, South game |
| Saint / Celestial | Throne Room, South game |

Three-player mode is not supported.

## Download

Prebuilt macOS Apple Silicon packages are available from GitHub Releases:

[Download the latest release](https://github.com/happy-shine/majsoul-autopilot/releases/latest)

The package contains:

```text
majsoul-autopilot-macos-arm64/
  Majsoul Autopilot.dmg     # macOS desktop GUI installer
  majsoul-autopilot-rs      # Command-line tool
  settings.example.json     # Configuration template
  INSTALL.zh-CN.txt         # Installation notes
  models/
    mortal/
      model.safetensors     # Mortal safetensors model weights
      model_config.json     # Model config
```

## Quick Start

### Desktop GUI
1. Open `Majsoul Autopilot.dmg` and drag `Majsoul Autopilot.app` into `Applications`.
2. Launch the app, enter your credentials in Settings, and start.

### CLI
Unzip `majsoul-autopilot-macos-arm64.zip` and enter the directory:

```bash
cd majsoul-autopilot-macos-arm64
cp settings.example.json settings.json
```

Edit `settings.json`:

```json
{
  "model_path": "models/mortal",
  "autoplay_account": {
    "username": "your-email@example.com",
    "password": "your-password"
  }
}
```

Check the model:

```bash
./majsoul-autopilot-rs --settings settings.json check-model
```

Check login status and target room:

```bash
./majsoul-autopilot-rs --settings settings.json check-login
```

Run a single game:

```bash
./majsoul-autopilot-rs --settings settings.json run --max-games 1
```

Run continuously:

```bash
./majsoul-autopilot-rs --settings settings.json run
```

Stop with `Ctrl-C`.

## Configuration

`settings.json` is the only required configuration file:

```json
{
  "model_path": "models/mortal",
  "autoplay_account": {
    "username": "",
    "password": ""
  }
}
```

Field description:

| Field | Description |
| --- | --- |
| `model_path` | Mortal model directory, containing `model.safetensors` and `model_config.json` |
| `autoplay_account.username` | Mahjong Soul email account |
| `autoplay_account.password` | Mahjong Soul password |

`settings.json` contains account credentials and is excluded from git by default.

## Commands

```bash
majsoul-autopilot-rs --settings settings.json check-model
majsoul-autopilot-rs --settings settings.json check-login
majsoul-autopilot-rs --settings settings.json run
majsoul-autopilot-rs --settings settings.json run --max-games 1
majsoul-autopilot-rs --settings settings.json replay-fixture path/to/fixture.json
```

## Build from Source

With Rust installed:

```bash
cargo build --release -p majsoul-autopilot-rs
```

Binary output:

```text
target/release/majsoul-autopilot-rs
```

Build Desktop App:

```bash
npm --prefix apps/desktop install
npm --prefix apps/desktop run tauri -- build
```

When running from source locally, ensure you have:

```text
settings.json
models/mortal/model.safetensors
models/mortal/model_config.json
```

Model weights and local credentials are not tracked in git.

## Development

Run tests:

```bash
cargo test --workspace -- --nocapture
```

Run clippy:

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

## Project Structure

```text
crates/
  autoplay/      Autoplay action planning and operation guards
  cli/           CLI entrypoint
  liqi/          Protobuf types and Liqi framing
  mjai/          Liqi to MJAI event bridge
  mortal/        Mortal model inference and action decoding
  protocol/      Lobby and game websocket client
  riichi-core/   Riichi state and observation encoding
apps/
  desktop/       Tauri desktop app
```

## Disclaimer

This project is intended solely for research and experimental purposes. Please review the terms of service of the relevant platforms and use at your own risk.

## License

GPL-3.0-or-later. See [LICENSE.txt](LICENSE.txt).
