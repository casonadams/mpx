# Installation Guide

`mpx` is distributed as precompiled native binaries for macOS (Apple Silicon & Intel), Linux (x86_64 & aarch64), and Windows (x86_64), as well as via package managers and Cargo.

---

## Package Managers

### macOS & Linux (Homebrew)

```bash
brew install casonadams/tap/mpx
```

### Ubuntu & Debian (APT Repository)

```bash
# Add repository and trusted keyring
echo "deb [trusted=yes] https://casonadams.github.io/homebrew-tap/apt stable main" | sudo tee /etc/apt/sources.list.d/casonadams.list
sudo apt update
sudo apt install mpx
```

### Fedora, RHEL & CentOS (DNF / RPM Repository)

```bash
curl -fsSL https://casonadams.github.io/homebrew-tap/rpm/casonadams.repo | sudo tee /etc/yum.repos.d/casonadams.repo
sudo dnf install mpx
```

---

## Rust / Cargo

### Prebuilt Binary via `cargo-binstall`

```bash
cargo binstall mpx
```

### Build from Source via `cargo install`

```bash
cargo install mpx
```

---

## GitHub Releases (Direct Downloads)

Pre-built standalone archives (`.tar.gz` and `.zip`) and native packages (`.deb`, `.rpm`) are published with every release on GitHub:

👉 **[https://github.com/casonadams/mpx/releases](https://github.com/casonadams/mpx/releases)**

| Platform | Target Triple | Artifact |
| :--- | :--- | :--- |
| **macOS (Apple Silicon)** | `aarch64-apple-darwin` | `mpx-v*-aarch64-apple-darwin.tar.gz` |
| **macOS (Intel)** | `x86_64-apple-darwin` | `mpx-v*-x86_64-apple-darwin.tar.gz` |
| **Linux (x86_64)** | `x86_64-unknown-linux-gnu` | `mpx-v*-x86_64-unknown-linux-gnu.tar.gz`, `.deb`, `.rpm` |
| **Linux (ARM64)** | `aarch64-unknown-linux-gnu` | `mpx-v*-aarch64-unknown-linux-gnu.tar.gz`, `.deb`, `.rpm` |
| **Windows (x64)** | `x86_64-pc-windows-msvc` | `mpx-v*-x86_64-pc-windows-msvc.zip` |

---

## Building Locally from Git

Requirements: Rust 1.80+ (`cargo` and `rustc`).

```bash
git clone https://github.com/casonadams/mpx.git
cd mpx
cargo build --release
./target/release/mpx --version
```
