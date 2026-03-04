# 🖨️ epson-watchdog

**Automatic CUPS queue recovery for Epson L3150 printers on macOS**

[![License](https://img.shields.io/badge/License-MIT-blue)]()
[![Rust](https://img.shields.io/badge/Rust-1.75+-DEA584?logo=rust&logoColor=white)]()
[![macOS](https://img.shields.io/badge/macOS-11+-000000?logo=apple&logoColor=white)]()

## Problem

Your Epson L3150 printer on Wi-Fi periodically:
- Changes IP address (DHCP)
- Drops from the network
- Stops responding to requests

When this happens, macOS CUPS marks the queue as `disabled`, and print jobs fail or hang indefinitely.

## Solution

**epson-watchdog** is a lightweight launchd agent that:
- Monitors your printer via mDNS every 30 seconds
- Automatically re-enables the CUPS queue when the printer reappears
- Queues print jobs locally while the printer is offline
- Works seamlessly with standard macOS print dialogs

## Features

- **Automatic Recovery** — Detects printer reappearance via Bonjour/mDNS and instantly re-enables the queue
- **Zero Configuration** — Works out of the box if your printer is already in CUPS
- **Background Agent** — Runs silently via launchd, survives system restarts
- **Job Safety** — Print jobs never get lost, they queue locally until the printer comes back
- **Minimal Resource Usage** — Single-threaded async daemon with 30-second poll interval
- **Pure macOS** — Uses built-in CUPS, no additional drivers or dependencies needed

## Installation

### Prerequisites

- macOS 11+
- [Rust](https://rustup.rs/) 1.75+ (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- Epson L3150 printer already added to **System Settings → Printers & Scanners**

### Install from GitHub

```bash
# 1. Install the binary directly from the repository
cargo install --git https://github.com/svishniakov/epson-watchdog

# 2. Set up the watchdog (printer must already be in CUPS)
epson-watchdog install --use-existing

# 3. Verify
epson-watchdog status
```

`cargo install` builds the binary and places it in `~/.cargo/bin/`, which is already in your `PATH` after Rust installation.

### What Gets Installed

```
~/.cargo/bin/epson-watchdog                               # Binary
~/Library/LaunchAgents/com.epson.l3150.watchdog.plist    # launchd agent
~/.config/epson-watchdog/config.toml                      # Watchdog settings
~/Library/Logs/epson-watchdog/epson-watchdog.log          # Runtime logs
```

The watchdog starts immediately and runs on every login automatically.

### Update

```bash
cargo install --git https://github.com/svishniakov/epson-watchdog --force
```

### Uninstall binary

```bash
epson-watchdog uninstall
cargo uninstall epson-watchdog
```

## Usage

### Check Status

```bash
epson-watchdog status
```

Shows printer state, queue status, and watchdog configuration.

### View Logs

```bash
tail -f ~/Library/Logs/epson-watchdog/epson-watchdog.log
```

Monitor the watchdog in real-time.

### Uninstall

```bash
epson-watchdog uninstall

# Or remove the printer from CUPS too:
epson-watchdog uninstall --remove-printer
```

## How It Works

### State Machine

```
Online (printer found on mDNS)
   ↓
   ├─→ Queue is enabled? → Stay Online
   │
   └─→ Queue disabled? → Issue cupsenable + cupsaccept → Recovering

Offline (printer not found on mDNS)
   ↓
   └─→ Print jobs queue in CUPS, wait for printer to return
```

### mDNS Discovery

The watchdog searches for your printer via Bonjour (mDNS) on the `_pdl-datastream._tcp.local.` service type, which is the native Epson printer service.

If your printer isn't found, you can manually discover it:

```bash
# Browse for all Epson printers
dns-sd -B _pdl-datastream._tcp local.
```

## Configuration

Config file: `~/.config/epson-watchdog/config.toml`

```toml
printer_name = "EPSON_L3150_Series"
mdns_instance_name = "EPSON L3150 Series"
mdns_hostname = "EPSON-L3150-Series.local."
poll_interval_secs = 30
enable_delay_secs = 3
plist_path = "~/Library/LaunchAgents/com.epson.l3150.watchdog.plist"
```

**Tweakable settings:**
- `poll_interval_secs` — How often to check the printer (default: 30s)
- `enable_delay_secs` — Pause after printer found before re-enabling (default: 3s)

## Building from Source

```bash
git clone https://github.com/svishniakov/epson-watchdog.git
cd epson-watchdog
cargo build --release
# Binary: target/release/epson-watchdog
```

### Run Tests

```bash
cargo test
```

## Troubleshooting

### Printer not found during install

Ensure your printer is:
1. Powered on and connected to Wi-Fi
2. Visible in System Settings > Printers & Scanners
3. On the same network as your Mac

Try:
```bash
dns-sd -B _pdl-datastream._tcp local.
```

### Queue stays disabled

Check logs:
```bash
tail -f ~/Library/Logs/epson-watchdog/epson-watchdog.log
```

Look for "Printer online" messages. If not appearing, the printer isn't responding to mDNS. Restart the printer and try again.

### Watchdog not running

```bash
launchctl list | grep epson
```

Should show `com.epson.l3150.watchdog` with a PID. If not:
```bash
launchctl load -w ~/Library/LaunchAgents/com.epson.l3150.watchdog.plist
```

### Manual queue enable (if watchdog fails)

```bash
# Check queue status
lpstat -p EPSON_L3150_Series

# Manually enable
cupsenable EPSON_L3150_Series
cupsaccept EPSON_L3150_Series
```

## Architecture

### Project Structure

```
epson/
├── Cargo.toml                          # Dependencies
├── src/
│   ├── main.rs                         # CLI entry point
│   ├── config.rs                       # Config load/save (toml)
│   ├── cups.rs                         # CUPS system commands
│   ├── discovery.rs                    # mDNS/Bonjour discovery (mdns-sd crate)
│   ├── watchdog.rs                     # Async monitoring loop (tokio)
│   └── installer.rs                    # Install/uninstall + launchd plist
├── launchd/
│   └── com.epson.l3150.watchdog.plist.template
└── README.md
```

### Tech Stack

| Layer | Technology |
|-------|------------|
| Runtime | Rust 1.75+ |
| Async | Tokio 1.x |
| mDNS Discovery | mdns-sd 0.18 |
| CLI | Clap 4.5 |
| Config | TOML |
| Logging | env_logger |
| OS Integration | launchd (native macOS) |

## Limitations

- **macOS only** — Requires launchd and CUPS (not portable to Windows/Linux)
- **Epson L3150 only** — Tested on L3150; should work with other Epson Wi-Fi printers
- **Local network only** — Watches mDNS on the local network, not remote printers
- **Poll-based** — Checks every 30 seconds, not event-driven (lightweight but not real-time)

## Performance

- **Memory**: ~5 MB resident
- **CPU**: <1% (sleeps between polls)
- **Network**: Minimal mDNS queries every 30s

## Contributing

Contributions welcome! Areas of interest:

- Support for other printer brands (Brother, Canon, etc.)
- Cross-platform support (Windows via Task Scheduler, Linux via systemd)
- Web UI for configuration
- Improved error messages

## License

MIT License — see [LICENSE](LICENSE) file

## Related

- [CUPS Documentation](https://www.cups.org/doc/admin-guide.html)
- [mDNS/Bonjour](https://developer.apple.com/bonjour/)
- [Epson L3150 Specs](https://www.epson.com/cgi-bin/Store/support/supDetail.jsp?intsalesmodel=EPSON%20Expression%20Series|L3150&useanymodel=true)

---

**Status**: Working in production on macOS Sonoma (14.x) ✅
