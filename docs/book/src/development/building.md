# Building from source

This page covers the toolchain, the system packages Listen needs, how to run it (including the demo library), how to cross-compile the Windows build from Linux, and the basic rules for contributions.

## Toolchain

- A recent stable Rust toolchain installed with `rustup`. The workspace uses the 2024 edition and Cargo resolver 3, so an old distribution Rust will not do.
- A C compiler and `pkg-config`.
- mpv, which Listen drives for all playback. It is needed at run time and by some tests.
- Optional at run time: `yt-dlp` and Node.js or Deno (YouTube Music), `whipper`, `flac` and `cdrdao` (CD ripping), PipeWire (the visualizer's audio capture on Linux).

Get the code:

```bash
git clone https://github.com/leks2205/Listen.git
cd Listen
```

## Linux packages

The window, the GPU and the keyboard libraries are loaded at run time with `dlopen`, so a build needs only a few development headers. The list below matches what the CI workflow installs.

**Debian and Ubuntu**

```bash
sudo apt-get install --no-install-recommends build-essential pkg-config \
    libxkbcommon-dev libwayland-dev libudev-dev mpv
```

**Fedora**

```bash
sudo dnf install gcc pkgconf-pkg-config libxkbcommon-devel wayland-devel \
    systemd-devel mpv
```

At run time the RPM (`packaging/listen.spec`) requires these libraries, which are good to have on any distribution: `libglvnd-egl`, `libwayland-client`, `libwayland-egl`, `libxkbcommon`, `libxkbcommon-x11`, `libX11`, `libX11-xcb`, `libxcb`, `libXcursor`, `libXi`, `libXrandr`. Install Mesa and Vulkan drivers (`mesa-dri-drivers`, `mesa-vulkan-drivers`, `vulkan-loader` on Fedora) so the GPU renderer starts. A system font with Korean glyphs (Noto Sans CJK KR) is used for Korean titles and lyrics.

## Running

```bash
cargo run --release -p listen                  # normal start
cargo run --release -p listen -- --demo        # built-in demo library, no server needed
```

Everything after `--` goes to Listen. See [Command line](../reference/command-line.md) for all options.

### Demo mode

`--demo` starts with a generated library of albums, artists, playlists and radio stations. No server is contacted. Demo songs play generated audio through mpv. Useful companions:

| Option | Effect |
|---|---|
| `--ephemeral` | Never write settings or credentials. Use it for experiments and screenshots. |
| `--queue` | Open the queue sidebar on start. |
| `--video` | Start with the song/video switch on. |
| `--theme <ID>` | Force a theme, for example `tokyoNight` or `defaultLight`. |
| `--route <PAGE>` | Start on `home`, `albums`, `tracks` or `settings`. |
| `--onboarding` | Show the first-start wizard even in demo or ephemeral runs. |
| `--size <WxH>` | Initial window size. The default is 1600x950. |

Environment variables for development: `LISTEN_MPV_ARGS` adds mpv options (for example `--ao=null` to stay silent), `LISTEN_MPV` and `LISTEN_YTDLP` point to a specific binary, and `LISTEN_LANG` forces the interface language.

### Logs

Debug output uses `tracing`. Set `RUST_LOG`, for example `RUST_LOG=listen=debug`, to see more. The log folder is printed at start-up; see [Backup, logs and diagnostics](../more/backup.md).

## Release profile

`Cargo.toml` defines the release profile: thin LTO, four codegen units and stripped symbols. The dev profile keeps line tables for the workspace crates and compiles dependencies with `opt-level = 1`, which keeps debug builds responsive. Audio analysis is slow in an unoptimised build, so use `--release` when you test the DJ features.

## Windows cross-compile with MinGW

Windows builds are produced on Linux. `.cargo/config.toml` already sets the linker (`x86_64-w64-mingw32-gcc`) and the `CC` and `AR` variables for the target.

1. Install the MinGW toolchain: `sudo dnf install mingw64-gcc` on Fedora, or `sudo apt-get install gcc-mingw-w64-x86-64` on Debian and Ubuntu.
2. Add the Rust target: `rustup target add x86_64-pc-windows-gnu`.
3. Build:

```bash
cargo build --release --target x86_64-pc-windows-gnu -p listen
```

The result is `target/x86_64-pc-windows-gnu/release/listen.exe`. To build the installer, see [Packaging and releases](packaging.md). On Windows the app finds mpv, yt-dlp and Deno through its helper-program settings; the installer fetches them with `winget` on first start.

## Contributing

Bug reports and pull requests are welcome. Before you open a pull request, run the same checks as CI:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI runs the first two plus `cargo test --workspace --lib` on Ubuntu for every push to `main` and every pull request. Tests that need mpv or a live server skip themselves when it is missing; see [Tests](tests.md). Keep a pull request focused on one change, and add or update a test when you fix a bug. New user-visible text must go through the translation system; see [Translations](translations.md).

Listen is licensed under AGPL-3.0-or-later. By contributing you agree that your change is released under that license.
