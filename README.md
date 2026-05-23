# ClashR

A desktop GUI for [mihomo](https://github.com/MetaCubeX/mihomo) (Clash.Meta), built with [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui).

![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-blue)
![Language](https://img.shields.io/badge/language-Rust-orange)

## Features

- **Home** — real-time traffic chart, upload/download speeds, active connections, system proxy and TUN mode toggle, IP info with country flag
- **Proxies** — proxy group cards with latency testing and node selection
- **Profiles** — subscription management, import from URL or local file
- **Connections** — live connection table with truncation and full-content preview on double-click
- **Rules** — full rule list with virtual scrolling
- **Logs** — live log stream from mihomo via WebSocket
- **Settings** — theme, language, system proxy, helper service, core management

## Requirements

- [mihomo](https://github.com/MetaCubeX/mihomo/releases) binary placed at `bin/mihomo` (macOS/Linux) or `bin/mihomo.exe` (Windows)
- Rust toolchain (for building from source)

## Building

```bash
cargo build --release
```

The release binary is at `target/release/clashr`.

## Running

```bash
# Place mihomo binary first
cp /path/to/mihomo bin/

cargo run --release
```

mihomo's external controller must be reachable at `127.0.0.1:9090` and its mixed port at `127.0.0.1:7890`.

## Helper Service (macOS)

ClashR ships an optional privileged helper (`clashr-service`) that owns the mihomo process as root, enabling TUN mode without keeping the GUI elevated.

```bash
# Build the service
cargo build --release -p clashr-service

# Install (requires sudo)
sudo target/release/clashr-service install
```

Install/uninstall can also be triggered from the Settings page via the native admin password dialog.

## Project Structure

```
src/
  pages/       — one file per page (home, proxies, profiles, …)
  core/        — mihomo process management, sysproxy, paths
  services/    — mihomo HTTP/WebSocket API clients
  theming/     — theme loading and preferences
  i18n.rs      — English / 中文 strings
crates/
  ipc/         — IPC protocol between GUI and helper service
  service/     — clashr-service daemon
```

## License

MIT
