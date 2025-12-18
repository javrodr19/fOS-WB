# fOS-WB: Zero-Bloat Browser

A privacy-focused, ultra-lightweight web browser written in Rust, designed for minimal memory footprint (<50MB RAM) with integrated userspace VPN.

## Features

- **Ultra-Light**: <50MB RAM with multiple tabs
- **Userspace VPN**: WireGuard-based, no root required
- **6 VPN Regions**: DE, JP, US, KR, RU, UK with auto-switching
- **Kill Switch**: Prevents IP leaks when VPN drops
- **Zero-Copy Content Blocking**: Bloom filter, <1μs per check
- **Tab Hibernation**: Suspends inactive tabs to disk
- **JIT-less JavaScript**: Interpreter-only mode for minimal RAM

## Architecture

```
fOS-WB/
├── crates/
│   ├── fos-wb/        # Main binary
│   ├── fos-memory/    # Memory management + allocators
│   ├── fos-tabs/      # Tab isolation + hibernation
│   ├── fos-network/   # HTTP client + content blocking
│   ├── fos-render/    # wgpu rendering + UI widgets
│   ├── fos-ui/        # Window management (winit)
│   ├── fos-js/        # JavaScript engine config
│   └── fos-vpn/       # WireGuard VPN + multi-region
├── config/
│   ├── vpn_regions.toml    # VPN server config (sample)
│   └── vpn_regions.json    # VPN server config (alternative)
```

## Quick Start

### Prerequisites

- Rust 1.75+ (install via [rustup](https://rustup.rs))
- Linux/macOS/Windows

### Build

```bash
# Clone the repository
git clone https://github.com/yourusername/fOS-WB.git
cd fOS-WB

# Build in release mode
cargo build --release

# Run tests
cargo test
```

### Run

```bash
# Run the browser
cargo run --release

# Or run the built binary
./target/release/fos-wb
```

## VPN Configuration

### 1. Generate WireGuard Keys

```bash
# Generate private key
wg genkey > private.key

# Generate public key (share with VPN provider)
cat private.key | wg pubkey > public.key
```

### 2. Configure VPN Regions

Copy the sample config and add your credentials:

```bash
cp config/vpn_regions.toml config/vpn_regions_local.toml
```

Edit `config/vpn_regions_local.toml`:

```toml
# Your private key (KEEP SECRET!)
private_key = "YOUR_BASE64_PRIVATE_KEY_HERE"

# Your internal VPN IP (provided by VPN service)
client_ip = "10.x.x.x"

# DNS servers
dns = ["1.1.1.1", "9.9.9.9"]

# Regions (update with your VPN provider's info)
[[regions]]
id = "de"
name = "Germany (Frankfurt)"
endpoint_ip = "YOUR_SERVER_IP"
endpoint_port = 51820
public_key = "SERVER_PUBLIC_KEY"
mtu = 1420
enabled = true
```

### 3. MTU Tuning for Problematic Regions

| Region | Recommended MTU | Reason |
|--------|-----------------|--------|
| DE, US, UK | 1420 | Standard |
| JP | 1400 | Trans-Pacific |
| KR | 1320 | ISP filtering |
| RU | 1280 | Aggressive DPI |

## Usage

### VPN Location Picker

Click the region indicator in the status bar `[DE ▼]` to:
- View available regions with latency
- Switch regions (500ms zero-leak pause)
- Disconnect from VPN

### Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| Ctrl+T | New tab |
| Ctrl+W | Close tab |
| Ctrl+L | Focus address bar |
| Ctrl+R | Refresh |
| F11 | Toggle fullscreen |

## Memory Budget

| Component | RAM Usage |
|-----------|-----------|
| Main process | ~2 MB |
| Per active tab | 3-8 MB |
| VPN + 64 connections | ~600 KB |
| Hibernated tab | ~500 KB |
| **Total (5 tabs)** | **~25 MB** |

## Development

### Running Tests

```bash
# All tests
cargo test

# Specific crate
cargo test -p fos-vpn

# With output
cargo test -- --nocapture
```

### Building Documentation

```bash
cargo doc --open
```

## Security

- **Kill Switch**: All traffic blocked when VPN disconnects
- **Zero-Leak Switching**: 500ms network pause during region change
- **No Logging**: No browsing history or VPN logs stored
- **Pointer Compression**: Reduced attack surface in JS heap

⚠️ **Never commit your `private_key` or `*_local.toml` files!**

## License

MIT License - See [LICENSE](LICENSE) for details.

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

---

**109 tests passing** | Built with Rust 🦀