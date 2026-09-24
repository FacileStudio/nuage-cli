# nuage-cli

Sync daemon and terminal client for [Nuage](https://github.com/FacileStudio/Nuage), the
self-hosted cloud storage app. The `nuage` binary keeps one local directory per space
bidirectionally in sync with a Nuage server, and searches and shares what is there.

Run it as a background daemon for continuous sync, or use its one-shot subcommands to search
and share remote files. The daemon is the only writer: to put a file in a space, drop
it in that space's mapped directory and it syncs.

## What it does

- Bidirectional sync between each mapped directory and its space, with SHA-256 change detection
- One directory and one state database per space, synced in parallel
- Background daemon with PID file, log file, and `start` / `stop` / `restart` / `logs`
- Filesystem watching with a 2-second debounce, plus a configurable server poll
- Conflict resolution using the last known hash, keeping both copies when it cannot decide
- Glob ignore patterns
- The token from `~/.nuage.yml`, an env var, or a command you name, so it can stay in a secret manager
- Remote reads with `search`, and share links with view or edit permission and an expiry
- API token and API key management, and `--json` on every non-daemon command

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
nuage login                        # sign in through the browser, writes ~/.nuage.yml
nuage logout                       # clear the stored token, keep everything else
nuage start                        # background sync daemon, one task per mapped space
nuage status                       # daemon state, last sync, file counts, per space
nuage sync                         # one-shot sync of every mapped space
nuage spaces list                  # every space you can act in, with its sync directory
nuage spaces create FacileShared   # create a space
nuage search report
NUAGE_SPACE=FacileShared nuage search report # read a shared space for one command
nuage share /Documents/report.pdf -e 7d
nuage keys list                    # list registered API keys
nuage keys create --app myapp      # create an API key
```

Full command reference and flags: [docs/usage.md](docs/usage.md).

## Signing in

`nuage login` asks the server which flows it accepts (`GET /auth/config`) and then opens your
browser. The CLI listens on an ephemeral loopback port, the server sends the browser back with
a single-use code valid for sixty seconds, and the CLI exchanges that code for a token. No
credential is ever typed into the terminal or left in a URL.

The page the browser lands on at the end is the suite's, not this repo's: `src/handoff.html.tmpl`
is a byte-for-byte copy of the template every Facile tool renders, so a `nuage` login and a
`courrier` one end on the same page. A callback the listener refuses, one carrying no code or the
wrong nonce, gets that page too, colored as a warning and saying the login is still waiting.

```sh
nuage login --server https://nuage.facile.studio   # the /api suffix is added for you
nuage login --token                                # paste an API token instead
nuage logout                                       # clear the token, keep your sync settings
```

Use `--token` on a machine with no browser: mint a token in the dashboard under Settings then
API and paste it at the prompt. Login also falls back to it on its own if a browser cannot be
opened and the instance permits it.

Both commands rewrite only `server_url` and `token`, plus `spaces` on a first run. Your
`poll_interval`, `ignore` and `key_command` are read, kept and written back untouched.

## Configuration

All configuration lives in `~/.nuage.yml`, written by `nuage login` or by hand. There is no
`--config` flag.

```yaml
server_url: https://nuage.facile.studio/api
token: your-api-token
# key_command: casier get nuage
spaces:
  personal: ~/Brain
  FacileShared: ~/Nuage
poll_interval: 10
ignore:
  - ".DS_Store"
  - "*.tmp"
  - ".git/"
```

| Key | What it does |
|---|---|
| `server_url` | Base URL prefixed to every request. Must reach the API, `/api` included |
| `token` | Nuage API token, sent as `Authorization: Bearer <token>` |
| `key_command` | A command whose standard output is the token, so the credential can stay in a secret manager. Outranks `token`; an env var outranks it |
| `spaces` | Space name to local directory. The daemon syncs every pair, each into its own directory with its own state database. `~` is expanded |
| `poll_interval` | Seconds between server polls in the daemon. Defaults to `10` |
| `ignore` | Globs excluded from sync. `.nuage/` is always added |

A config with no `spaces:` block is refused when `sync` or the daemon runs, with
`no spaces mapped`. The pre-0.8.0 `sync_dir` key is no longer read: move its value under
`spaces:` yourself, because one directory cannot be folded onto a space without moving the
wrong files into it.

Three environment variables override the file, for CI and for one-off runs against another
instance. Precedence is flag, then environment, then file, then built-in default.

| Variable | Overrides |
|---|---|
| `NUAGE_TOKEN` | `token` |
| `NUAGE_API_KEY`, `NUAGE_KEY` | `token`, aliases of the above |
| `NUAGE_SERVER_URL` | `server_url` |
| `NUAGE_SPACE` | The space the read commands act on, for one run. Takes a space name or an id |

Full reference, including the on-disk layout:
[docs/configuration.md](docs/configuration.md).

## Structure

```
src/
  main.rs      clap tree and dispatch
  commands/    one module per command group, plus the daemon supervisor
  config/      ~/.nuage.yml model, validation, env overrides, saving
  login/       browser SSO loopback flow, API-token fallback, logout
  api/         Nuage REST client and response models
  daemon/      PID file, log paths, daemon and terminal logging setup
  handoff.rs   the sign-in page the loopback listener serves, shared with the suite
  hash.rs      buffered SHA-256 file hashing
  ignore/      glob ignore matching
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
