# nuage-cli — Development

Building the binary, running the sync engine safely against a real server, and where
everything lives in the source tree.

## Prerequisites

- Rust with edition 2021 support and `cargo` on `PATH` (install via [rustup](https://rustup.rs))
- A reachable Nuage instance and an API token
- A C toolchain, because `rusqlite` is built with the `bundled` feature and compiles SQLite
  from source
- Unix. `daemonize`, `libc::kill` and the `SIGTERM` / `SIGINT` handlers are Unix-only

Cargo is the whole toolchain. `mise.toml` exists only to install lefthook, which runs the git
hooks; it is not a task runner. There is no `Makefile` and no `scripts/check.sh`. The only CI
job is `.github/workflows/release.yml`, which builds the release binaries.

## Setup

```sh
git clone https://github.com/FacileStudio/nuage-cli.git
cd nuage-cli
cargo build
cargo run -- login
```

`login` writes `~/.nuage.yml`. **Point the mapped directory at a throwaway path while
developing.** The sync engine deletes local files the server reports as deleted, and a mistake
against your real folder is not undoable from here.

```yaml
server_url: http://localhost:8080
token: your-api-token
spaces:
  personal: ~/tmp/nuage-dev
poll_interval: 10
```

## Running

```sh
cargo run                       # prints the help message and exits 0
cargo run -- watch              # foreground watcher
cargo run -- sync               # one-shot sync of every mapped space
cargo run -- status
cargo run -- search invoice
cargo run -- --json search invoice
cargo run -- --help
```

Prefer `cargo run -- watch` over `cargo run -- start` while developing: the daemon forks,
detaches, and sends its output to `~/.nuage/logs/nuage.log`, which makes an iteration loop
needlessly indirect. If you do start one, `cargo run -- stop` before rebuilding. A stale
daemon running old code against the same sync directory will fight your foreground process.

## Logging

Both logging setups use `tracing_subscriber::EnvFilter::from_default_env()` with `INFO` added
on top, so `RUST_LOG` controls verbosity:

```sh
RUST_LOG=debug cargo run -- sync
RUST_LOG=nuage=debug cargo run -- watch
```

The daemon writes the same stream to `~/.nuage/logs/nuage.log` with ANSI colors disabled.

## Resetting state

Each target's entire memory is one SQLite file:

```sh
rm -rf ~/tmp/nuage-dev/.nuage
```

The next run has no cursor, so it takes the `GET /sync/state` path and performs a first-run
full sync. This is the fastest way to reproduce first-run behavior, and the first thing to try
when sync state looks wrong.

```sh
sqlite3 ~/tmp/nuage-dev/.nuage/state.db 'select local_path, hash from files limit 10;'
```

## Tests

Unit tests live in `#[cfg(test)]` modules beside the code they cover. There is no `tests/`
directory and no integration harness; a test that needs a server is not written.

```sh
cargo test
cargo clippy
```

The pure functions are the easy wins: `resolver::resolve_conflict` (a four-case truth table),
`IgnoreRules::is_ignored`, `transfer::mime_from_extension`, `transfer::format_size`,
`parse_expiry`, and the config map round-trip. The remote-change filter takes a fixture rather
than a live server.

## Where things live

| Path | What it holds |
|---|---|
| `src/main.rs` | The clap tree and the dispatch from a subcommand to its handler |
| `src/commands/` | One module per command group, plus the daemon supervisor |
| `src/config/` | `Config`, its defaults, validation, `spaces_expanded`, `save` |
| `src/login/` | The browser SSO loopback flow, the API-token fallback, logout |
| `src/api/` | `ApiClient`, both HTTP clients, response models, one method per endpoint |
| `src/daemon/` | PID and log paths, `is_running`, the two logging initializers |
| `src/handoff.rs` | The sign-in page the loopback listener serves, and the template it renders |
| `src/hash.rs` | Buffered SHA-256 hashing |
| `src/ignore/` | `IgnoreRules`, including the forced `.nuage/` entries |
| `src/sync/mod.rs` | `SyncEngine`: full sync, local changes, remote changes, uploads, scans |
| `src/sync/state.rs` | The SQLite schema and every query |
| `src/sync/remote.rs` | Cursor-aware fetch, and the filter that scopes a payload to one space |
| `src/sync/resolver.rs` | Conflict resolution and conflict filenames |
| `src/sync/transfer.rs` | Download to temp then rename, upload, MIME and size formatting |
| `src/sync/watcher.rs` | The debounced filesystem watcher |
| `integrations/SKILL.md` | The AI agent skill the installer registers |
| `install.sh` | Clone, `cargo install --path`, register the skill |

## Adding a command

1. Add a variant to the matching `Command` enum, with a `clap::Args` struct if it takes
   arguments. Doc comments on the variant become the help text: capitalized, imperative, no
   trailing period.
2. Add it to the module under `src/commands/`, or create one beside it.
3. Write an async `cmd_*` handler taking `json: bool`, and honor it.
4. Add the endpoint to `ApiClient` under `src/api/` if it does not exist yet.
5. Document it in [usage.md](usage.md) and, if an assistant should know about it, in
   `integrations/SKILL.md`.

## Gotchas

- **Two different `.nuage` directories.** `~/.nuage/` holds the PID file and daemon logs;
  `<dir>/.nuage/` holds one target's state database. Neither is the other.
- **One engine per mapped space.** Each target has its own directory, its own state database
  and its own space-scoped `ApiClient`. Two names on one directory is a config error, not a
  warning, because the two engines would fight.
- **The server is space-blind on sync.** `GET /sync/state` returns the merged tree whatever
  `space_id` says, so the scoping is a client-side filter in `sync/remote.rs`. Do not "fix" it
  by trusting the query parameter.
- **The `Origin` header is load-bearing.** `ApiClient` sets it from `server_url` because the
  server rejects multipart uploads without it. Do not drop it while refactoring the client.
- **Two HTTP clients, two timeouts.** Metadata calls get 30 seconds, transfers get 300. A
  large upload on the metadata client will time out.
- **Downloads are concurrent, four at a time,** behind a semaphore. Each spawned task builds
  its own `ApiClient`.

## Conventions

- No inline comments. Names and structure carry the meaning.
- Remove dead code as you touch it.
- `filet check .` is the layout and style gate; `cargo clippy` owns anything that needs to
  understand Rust.
- Commit messages are plain imperative sentence case.
