# spotatui — lightweight streaming fork

A stripped-down fork of [LargeModGames/spotatui](https://github.com/LargeModGames/spotatui), focused on being a fast, reliable Spotify terminal UI.

This fork keeps the parts I actually want to use every day:

- native Spotify streaming through librespot
- macOS media keys / Now Playing integration
- cover art in the playbar
- Sonos room selection and local Sonos controls
- fast terminal navigation for playlists, albums, search, queue, library, and settings

It removes or de-emphasizes the original repo's heavier/non-core features:

- Friends / Party / cloud sync
- announcements and global song counter
- Discord Rich Presence
- MPRIS
- audio visualization and analysis views
- self-update code
- wake-lock / keepawake behavior
- high-frequency animation ticks
- extra redraw/allocation work in large lists and playbar rendering

The result is intended to be a smaller, quieter, faster Spotify TUI with fewer background integrations competing with native playback.

## Current status

This is a personal fork and still being manually tested.

Validated by automated checks:

```bash
cargo fmt --all
cargo clippy --no-default-features --features streaming,macos-media,cover-art -- -D warnings
cargo test --no-default-features --features streaming,macos-media,cover-art
cargo clippy --no-default-features --features telemetry -- -D warnings
cargo test --no-default-features --features telemetry
```

Manual verification still matters for Spotify Connect behavior, native playback startup, macOS media keys, and Sonos discovery/control.

## Requirements

- Rust toolchain
- Spotify Premium account
- Spotify Developer app credentials configured for spotatui
- macOS for the macOS media-key feature
- Sonos speakers on the same local network if you want Sonos support

## Run from source

For the intended full lightweight build:

```bash
cargo run --no-default-features --features streaming,macos-media,cover-art
```

For the fastest CI-style build without native streaming/audio extras:

```bash
cargo run --no-default-features --features telemetry
```

## Spotify setup

The app uses the normal spotatui config path:

```text
~/.config/spotatui/config.yml
```

On first run, follow the in-app Spotify authentication instructions. Do not commit Spotify client secrets or tokens to this repository.

## Native streaming

Native streaming is kept as a core feature. The app should appear as a Spotify Connect device and can play without the official Spotify desktop app or `spotifyd`.

This fork includes additional native startup/device-activation hardening:

- stores the librespot session device id immediately
- shows UI-visible activation status such as `Activating native Spotify device: ...`
- confirms the native device against Spotify's device list when possible
- avoids routing raw native URI-list playback through fragile Spotify Web API recovery paths
- keeps an end-of-track recovery heuristic active if native playback stops after one item

To test native playback manually:

1. Start the app with the full build command above.
2. Play a playlist or saved-track list.
3. Confirm the native `spotatui` device appears in Spotify Connect.
4. Confirm the first track starts reliably.
5. Confirm playback continues to the next track.
6. Test play/pause/next/previous from the TUI.
7. Test macOS media keys while native playback is active.

## Sonos

Sonos support is kept, but isolated from normal startup and native streaming.

Sonos discovery should not run in the background just because the app started. It runs when:

- you explicitly open the device picker, or
- a Sonos room is already selected/persisted.

To select Sonos:

1. Open the device picker with the configured **Manage Devices** key.
2. Wait a moment for SSDP/UPnP discovery.
3. Select the Sonos room.
4. Press `Enter`.

Once selected, playback controls route through local Sonos control for that room.

If Sonos does not appear, check that the speaker and computer are on the same network/VLAN and that the local firewall allows SSDP/UPnP discovery.

## macOS media keys

macOS media keys are kept and decoupled from native streaming.

Expected behavior:

- when native playback is active, media keys can use the native player path;
- when native streaming is unavailable, Sonos is selected, or an external Spotify Connect device is selected, media keys should still control playback through the correct API/control path instead of assuming native streaming.

Manual test cases:

- native streaming active
- native streaming unavailable
- Sonos selected
- external Spotify Connect device selected

## Performance work in this fork

This branch removes large non-core modules and reduces redraw/allocation work in common UI paths.

Examples:

- no extra tick emitted after every input event
- lower default tick rate
- optimized cursor visibility updates
- visible-window rendering for large lists/tables
- fewer intermediate `Vec`s in rendering paths
- borrowed playback metadata where possible
- borrowed artist/episode spans in the playbar
- fixed-size table/header row data where practical

## Relationship to upstream spotatui

This fork is based on the original `spotatui`, which itself is a community-maintained fork of [`spotify-tui`](https://github.com/Rigellute/spotify-tui).

Upstream spotatui includes many broader features and integrations. This fork intentionally narrows the scope around reliable native playback, Sonos, cover art, macOS controls, and a fast TUI.

For upstream releases, docs, and the full feature set, see:

- <https://github.com/LargeModGames/spotatui>

## Development

Useful commands:

```bash
cargo fmt --all
cargo clippy --no-default-features --features streaming,macos-media,cover-art -- -D warnings
cargo test --no-default-features --features streaming,macos-media,cover-art
cargo clippy --no-default-features --features telemetry -- -D warnings
cargo test --no-default-features --features telemetry
```

Run a single test:

```bash
cargo test --no-default-features --features telemetry <test_name>
```

## License

This fork keeps the original project's license. See [`LICENSE`](LICENSE).
