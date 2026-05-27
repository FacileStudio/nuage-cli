# nuage-cli

Bidirectional file sync daemon and CLI for [Nuage](https://github.com/FacileStudio/Nuage). Keeps a local directory (`~/Nuage` by default) in sync with a Nuage server.

## Tech Stack

- Language: Rust (edition 2021)
- Async runtime: Tokio (full features)
- HTTP client: reqwest (JSON, multipart, streaming)
- CLI framework: clap v4 (derive)
- Local state DB: rusqlite (bundled SQLite)
- File watching: notify + notify-debouncer-mini
- Config format: YAML (`~/.nuage.yml`), parsed with serde_yaml
- Logging: tracing + tracing-subscriber (env-filter)
- Hashing: sha2 (SHA-256 for change detection)

## Key Commands

```sh
cargo build                  # debug build
cargo build --release        # release build (stripped, LTO)
cargo run                    # run the CLI (default: start daemon)
cargo run -- <subcommand>    # run a specific subcommand
cargo clippy                 # lint (no custom clippy config)
cargo test                   # no tests yet
```

The binary is named `nuage` (configured in `[[bin]]` in Cargo.toml).

Install from source via `cargo install --path .` or the remote install script (`install.sh`).

## Project Structure

```
src/
  main.rs          CLI entry point, clap arg parsing, all subcommand handlers
  api.rs           HTTP API client (reqwest) for the Nuage server
  config.rs        Config loading/saving from ~/.nuage.yml
  daemon.rs        Daemonize logic (start/stop/restart, PID file management)
  hash.rs          SHA-256 file hashing for change detection
  ignore.rs        Glob-based ignore pattern matching
  sync/
    mod.rs         Core bidirectional sync engine
    state.rs       SQLite state database (tracks known files, hashes, timestamps)
    remote.rs      Remote file tree fetching
    resolver.rs    Conflict resolution between local and remote changes
    transfer.rs    Upload/download with progress bars
    watcher.rs     Filesystem watcher (notify-based)
```

Config lives at `~/.nuage.yml` -- see `config.example.yaml` for the format.

## CLI Subcommands

`start`, `stop`, `restart` -- daemon management  
`sync` -- one-time bidirectional sync  
`watch` -- foreground watcher (debug mode)  
`status` -- show daemon and sync status  
`logs` -- show daemon logs (`-f` to follow)  
`login` -- interactive setup  
`upgrade` -- self-upgrade from GitHub  
`ls`, `upload`, `download`, `mkdir`, `mv`, `rm` -- remote file operations  
`share`, `unshare`, `shares` -- share link management  
`search` -- search files/folders  
`token create|list|revoke` -- API token management  

All subcommands support `--json` for machine-readable output.

## Conventions

- No inline comments in code.
- No test suite exists yet.
- No rustfmt, clippy, or toolchain config files -- uses Rust defaults.
- Commit messages are lowercase, imperative, and descriptive (e.g., "fix sync always uploading untracked local files").
- `main.rs` is large (~900+ lines) -- contains all subcommand handler logic alongside arg definitions.
- Config validation happens at load time in `config.rs`.
- The release profile enables LTO and binary stripping.

## Gotchas

- The config file path is hardcoded to `~/.nuage.yml` (not XDG-compliant).
- SQLite state DB location is managed in `sync/state.rs` (stored inside the sync directory as `.nuage/state.db`).
- The API client sends an `Origin` header to avoid CSRF 403 errors on multipart uploads.
- Upload supports stdin piping (`-` as source path).
- The daemon writes a PID file for process management; check `daemon.rs` for the PID file path.
