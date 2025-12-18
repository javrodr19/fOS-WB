# fOS - Unified Desktop Ecosystem

A lightweight, high-performance desktop ecosystem built in Rust.

## Components

| Component | Description | Port | Size |
|-----------|-------------|------|------|
| **web-browser** | Custom browser with `fos://` protocol | N/A | 925 KB |
| **file-backend** | Zero-copy file server with thumbnails | 3000 | 2.6 MB |
| **chat-server** | Protobuf WebSocket chat | 9000 | 1.1 MB |

## Prerequisites

### All Platforms
- [Rust](https://rustup.rs/) (1.75+)

### Linux (Debian/Ubuntu)
```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev
```

### Linux (Fedora)
```bash
sudo dnf install webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3-devel
```

### Linux (Arch)
```bash
sudo pacman -S webkit2gtk-4.1 gtk3 libappindicator-gtk3
```

### macOS
```bash
xcode-select --install
```

### Windows
- Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/downloads/) with C++ workload
- WebView2 is included in Windows 10/11

## Build

```bash
# Clone
git clone https://github.com/your-username/fOS-WB.git
cd fOS-WB

# Build all (release)
cargo build --release --manifest-path web-browser/Cargo.toml
cargo build --release --manifest-path file-backend/Cargo.toml
cargo build --release --manifest-path chat-server/Cargo.toml
```

## Run

Open 3 terminals:

```bash
# Terminal 1 - File Backend
cd file-backend && cargo run --release

# Terminal 2 - Chat Server
cd chat-server && cargo run --release

# Terminal 3 - Browser
cd web-browser && cargo run --release
```

Browser opens at `fos://home` with links to Drive and Chat apps.

## Architecture

```
┌─────────────────────────────────────────┐
│          fOS Browser (fos://)           │
│  ┌─────────────┐  ┌─────────────────┐   │
│  │ fOS Drive   │  │ fOS Chat        │   │
│  └──────┬──────┘  └────────┬────────┘   │
└─────────┼──────────────────┼────────────┘
          │                  │
          ▼                  ▼
   ┌──────────────┐   ┌───────────────┐
   │ file-backend │   │ chat-server   │
   │ :3000        │   │ :9000         │
   └──────────────┘   └───────────────┘
```

## License

MIT