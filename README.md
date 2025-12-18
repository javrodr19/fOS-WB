# fOS-WB 🌐

A minimal, memory-efficient web browser built with Rust, GTK4, and WebKitGTK6.

![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Rust](https://img.shields.io/badge/rust-1.75+-orange.svg)

## Features

- **Vertical Tabs** - Clean sidebar layout with tabs on the left
- **Lazy Loading** - Tabs only load content when activated (saves memory)
- **Keyboard-First** - Full keyboard navigation support
- **Minimal UI** - URL bar at bottom, no clutter
- **Memory Efficient** - Optimized WebKit settings for low memory usage
- **Fast Startup** - Uses mimalloc allocator

## Screenshot

```
┌──────────┬─────────────────────────────────┐
│ Tab 1    │                                 │
│ Tab 2    │        Web Content              │
│ Tab 3    │                                 │
├──────────┴─────────────────────────────────┤
│ ← →  [________URL________]                 │
└────────────────────────────────────────────┘
```

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+T` | New tab |
| `Ctrl+W` | Close tab |
| `Ctrl+R` | Reload page |
| `Ctrl+L` | Focus URL bar |

## Requirements

### Linux (Arch/Manjaro)
```bash
sudo pacman -S gtk4 webkitgtk-6.0
```

### Linux (Ubuntu/Debian)
```bash
sudo apt install libgtk-4-dev libwebkitgtk-6.0-dev
```

### Linux (Fedora)
```bash
sudo dnf install gtk4-devel webkitgtk6.0-devel
```

## Building

```bash
# Clone
git clone https://github.com/yourusername/fOS-WB.git
cd fOS-WB

# Build (release)
cargo build --release

# Run
./target/release/fos-wb
```

## Development

```bash
# Run with logging
RUST_LOG=info cargo run

# Run release build
cargo run --release
```

## Architecture

```
fOS-WB/
├── crates/
│   ├── fos-wb/      # Main binary
│   └── fos-ui/      # GTK4 + WebKitGTK browser UI
├── Cargo.toml       # Workspace config
└── README.md
```

## Memory Optimizations

The browser includes several memory-saving features:

- **Lazy tab loading** - New tabs don't load until selected
- **Disabled WebGL** - Saves GPU memory (re-enable if needed)
- **Disabled page cache** - Reduces memory at cost of back/forward speed
- **Disabled offline cache** - Less storage/memory usage
- **mimalloc allocator** - More efficient memory allocation

## Tech Stack

- **Rust** - Safe systems programming
- **GTK4** - Modern Linux GUI toolkit
- **WebKitGTK6** - Full web rendering engine
- **mimalloc** - High-performance allocator

## License

MIT License - see [LICENSE](LICENSE) for details.

## Contributing

Contributions welcome! Please open an issue or PR.