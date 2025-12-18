# fOS-WB 🌐

A minimal, keyboard-driven web browser built with Rust, GTK4, and WebKitGTK6. Features a powerful built-in adblocker using Brave's engine. It doesn't work with everything.

![License](https://img.shields.io/badge/license-GPL--3.0-blue.svg)
![Rust](https://img.shields.io/badge/rust-1.75+-orange.svg)
![Platform](https://img.shields.io/badge/platform-Linux-green.svg)

## ✨ Features

- **Built-in Adblocker** - Powered by Brave's adblock-rust engine with 100k+ rules
- **Vertical Tabs** - Clean sidebar layout with tabs on the left
- **Lazy Loading** - Tabs only load content when activated (saves RAM)
- **Session Persistence** - Tabs are saved on close and restored on open
- **Stay Logged In** - Cookies persist across restarts
- **Keyboard-First** - Full keyboard navigation, no mouse required
- **Minimal UI** - URL bar at bottom, no buttons clutter
- **Memory Efficient** - Optimized WebKit settings for low memory usage

## 🛡️ Adblocker

The built-in adblocker uses the same engine as Brave browser with these filter lists:

| Filter List | Description |
|-------------|-------------|
| EasyList | Primary ad blocking rules |
| EasyPrivacy | Privacy protection rules |
| uBlock Origin Filters | Enhanced ad blocking |
| uBlock Origin Privacy | Enhanced privacy rules |
| Peter Lowe's List | Ad server domains |

Filter lists are automatically downloaded on first run and cached in `~/.local/share/fos-wb/filters/`.

## 📊 Performance Metrics

Tested on:
| Component | Specification |
|-----------|---------------|
| **CPU** | AMD Ryzen 5 Pro 7535U (6-core, up to 4.6 GHz) |
| **GPU** | AMD Radeon Graphics (integrated) |
| **RAM** | 15 GB DDR5 |
| **OS** | Manjaro Linux (Kernel 6.12) |

### Benchmarks

| Metric | Value |
|--------|-------|
| **Binary Size** | ~5.4 MB |
| **Startup Time** | <1 second |
| **Idle Memory (1 tab)** | ~60-80 MB* |
| **Memory per Tab** | ~20-40 MB (lazy loaded) |
| **Adblock Rules** | 100,000+ |

*Memory usage is dominated by WebKitGTK. The browser chrome itself adds minimal overhead.

## ⌨️ Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+T` | New tab |
| `Ctrl+W` | Close tab |
| `Ctrl+R` | Reload page |
| `Ctrl+I` | Focus URL bar |
| `Ctrl+O` | Switch to tab above |
| `Ctrl+L` | Switch to tab below |
| `Ctrl+K` | Go back |
| `Ctrl+Ñ` | Go forward |

## 📁 Data Storage

All browser data is stored in `~/.local/share/fos-wb/`:

| File/Directory | Purpose |
|----------------|---------|
| `cookies.sqlite` | Persistent cookies (stay logged in) |
| `session.json` | Saved tabs (restored on open) |
| `cache/` | Web cache |
| `filters/` | Cached adblock filter lists |

## 🚀 Installation

### Requirements

**Arch/Manjaro:**
```bash
sudo pacman -S gtk4 webkitgtk-6.0
```

**Ubuntu/Debian:**
```bash
sudo apt install libgtk-4-dev libwebkitgtk-6.0-dev
```

**Fedora:**
```bash
sudo dnf install gtk4-devel webkitgtk6.0-devel
```

### Build & Install

```bash
# Clone
git clone https://github.com/yourusername/fOS-WB.git
cd fOS-WB

# Build release
cargo build --release

# Install to local bin
cp target/release/fos-wb ~/.local/bin/

# Create desktop entry (optional)
cat > ~/.local/share/applications/fos-wb.desktop << 'EOF'
[Desktop Entry]
Name=fOS-WB
Comment=Minimal Web Browser with Adblocker
Exec=$HOME/.local/bin/fos-wb
Icon=web-browser
Terminal=false
Type=Application
Categories=Network;WebBrowser;
EOF

# Update desktop database
update-desktop-database ~/.local/share/applications/
```

### Set as Default Browser (optional)

```bash
xdg-settings set default-web-browser fos-wb.desktop
```

## 🔧 Development

```bash
# Run with logging
RUST_LOG=info cargo run

# Build release
cargo build --release

# Run release
./target/release/fos-wb
```

## 🏗️ Architecture

```
fOS-WB/
├── crates/
│   ├── fos-wb/        # Main binary (entry point)
│   │   └── src/main.rs
│   └── fos-ui/        # Browser UI + Adblocker
│       └── src/
│           ├── lib.rs
│           ├── webview.rs   # GTK4 + WebKitGTK browser
│           └── adblocker.rs # Brave's adblock engine
├── Cargo.toml         # Workspace configuration
├── LICENSE            # GPL-3.0 License
└── README.md
```

## 🔒 Memory Optimizations

The browser includes several memory-saving features:

- **Lazy tab loading** - New tabs don't load until selected
- **Shared network session** - All tabs share one session for efficiency
- **Disabled WebGL** - Saves GPU memory (re-enable if needed)
- **Disabled page cache** - Trades memory for back/forward speed
- **mimalloc allocator** - More efficient memory allocation

## 📜 License

This project is licensed under the **GNU General Public License v3.0** (GPL-3.0).

You are free to:
- ✅ Use this software for personal use
- ✅ Modify the source code
- ✅ Distribute copies
- ✅ Distribute modified versions

Under these conditions:
- 📖 Source code must remain open source
- 📖 Derivative works must use the same license
- 📖 Changes must be documented

See [LICENSE](LICENSE) for the full license text.

## 🤝 Contributing

Contributions are welcome! Please open an issue or pull request.

---

Made with ❤️ and Rust