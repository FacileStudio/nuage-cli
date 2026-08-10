# nuage-cli

Sync daemon and terminal client for [Nuage](https://github.com/FacileStudio/Nuage), the
self-hosted cloud storage app. The `nuage` binary keeps a local directory bidirectionally in
sync with a Nuage server, and doubles as a remote file manager.

Run it as a background daemon for continuous sync, or use its one-shot subcommands to list,
upload, download, move, share and search remote files.

## What it does

- Bidirectional sync between a local directory and the server, with SHA-256 change detection
- Background daemon with PID file, log file, and `start` / `stop` / `restart` / `logs`
- Filesystem watching with a 2-second debounce, plus a configurable server poll
- Conflict resolution using the last known hash, keeping both copies when it cannot decide
- Glob ignore patterns and optional selective sync of specific paths
- Remote file management: `ls`, `upload`, `download`, `mkdir`, `mv`, `rm`, `search`
- Share links with view or edit permission and an optional expiry
- API token management, and `--json` on every non-daemon command

## Stack

| Layer | Tech |
|---|---|
| CLI | Rust 2021, clap 4 (derive), tokio 1, anyhow 1 |
| Transport | reqwest 0.12 (JSON, multipart, streaming), bearer token auth |
| Sync | notify 7 with notify-debouncer-mini 0.5, sha2 0.10, glob-match 0.2 |
| Storage | rusqlite 0.32 (bundled SQLite) state DB, `~/.nuage.yml` via serde_yaml 0.9 |
| Daemon | daemonize 0.5, libc 0.2, tracing 0.1 with tracing-subscriber |

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/FacileStudio/nuage-cli/main/install.sh | bash
```

Installs to `~/.local/bin` via [facile](https://github.com/FacileStudio/facile), the suite
installer. Pass `--bin-dir <dir>` to change that, `--source` to build from source, `--no-skill`
to skip AI agent skill registration.

Already have `facile`:

```sh
facile install nuage
```

## Usage

```sh
nuage login                        # interactive setup, writes ~/.nuage.yml
nuage start                        # background sync daemon
nuage status                       # daemon state, last sync, file counts
nuage sync                         # one-shot bidirectional sync
nuage ls /Documents -l
nuage upload report.pdf /Documents
nuage share /Documents/report.pdf -e 7d
```

Full command reference and flags: [docs/usage.md](docs/usage.md).

## Configuration

All configuration lives in `~/.nuage.yml`, written by `nuage login` or by hand. There is no
`--config` flag.

```yaml
server_url: https://nuage.facile.studio/api
token: your-api-token
sync_dir: ~/Nuage
poll_interval: 30
ignore_patterns:
  - ".DS_Store"
  - "*.tmp"
  - ".git/"
```

| Key | What it does |
|---|---|
| `server_url` | Base URL prefixed to every request. Must reach the API, `/api` included |
| `token` | Nuage API token, sent as `Authorization: Bearer <token>` |
| `sync_dir` | Local directory to keep in sync. `~` is expanded. Defaults to `~/Nuage` |
| `poll_interval` | Seconds between server polls in the daemon. Defaults to `30` |
| `ignore_patterns` | Globs excluded from sync. `.nuage/` is always added |

Full reference, including `selective_sync` and the on-disk layout:
[docs/configuration.md](docs/configuration.md).

## Structure

```
src/
  main.rs      clap tree and every subcommand handler
  config.rs    ~/.nuage.yml loading, validation, saving
  api.rs       Nuage REST client and response models
  daemon.rs    PID file, log paths, daemon and terminal logging setup
  hash.rs      buffered SHA-256 file hashing
  ignore.rs    glob ignore matching
  sync/        the sync engine: state DB, watcher, conflict resolver, transfers
integrations/  SKILL.md, registered with Claude Code and Codex by install.sh
```

## Documentation

| Doc | What's in it |
|---|---|
| [Architecture](docs/architecture.md) | Topology, the sync algorithm, endpoints, state DB |
| [Configuration](docs/configuration.md) | Every config key, paths, and the files on disk |
| [Development](docs/development.md) | Building, running the daemon locally, source layout |
| [Usage](docs/usage.md) | Every command, flag and output shape |

---

Part of the [Facile Suite](https://facile.studio) — self-hosted tools for creative studios
and freelancers. One login, zero cloud dependency.
