# nuage-cli — Development

Building the binary, running the sync engine safely against a real server, and where
everything lives in the source tree.

## Prerequisites

- Rust with edition 2021 support and `cargo` on `PATH` (install via [rustup](https://rustup.rs))
- A reachable Nuage instance and an API token
- A C toolchain, because `rusqlite` is built with the `bundled` feature and compiles SQLite
  from source
- Unix. `daemonize`, `libc::kill` and the `SIGTERM` / `SIGINT` handlers are Unix-only

There is no `mise.toml`, no `Makefile`, no `scripts/check.sh`, no rustfmt or clippy config,
and no CI workflow. Cargo is the entire toolchain.

## Setup

```sh
git clone https://github.com/FacileStudio/nuage-cli.git
cd nuage-cli
cargo build
cargo run -- login
```

`login` writes `~/.nuage.yml`. **Point `sync_dir` at a throwaway directory while developing** —
the sync engine deletes local files the server reports as deleted, and a mistake against your
real folder is not undoable from here.

```yaml
server_url: http://localhost:8080
token: your-api-token
sync_dir: ~/tmp/nuage-dev
poll_interval: 10
```

## Running

```sh
cargo run                       # foreground watcher, same as `watch`
cargo run -- sync               # one-shot sync
cargo run -- status
cargo run -- ls / -l
cargo run -- --json ls /
cargo run -- --help
```

Prefer `cargo run -- watch` over `cargo run -- start` while developing: the daemon forks,
detaches, and sends its output to `~/.nuage/logs/nuage.log`, which makes an iteration loop
needlessly indirect. If you do start one, `cargo run -- stop` before rebuilding — a stale
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

The client's entire memory is one SQLite file:

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

There are none — no `tests/` directory and no `#[cfg(test)]` module anywhere in `src/`. The
available checks are the ones cargo ships:

```sh
cargo check
cargo clippy
cargo fmt --check
```

If you touch sync behavior, add a check for it. The pure functions are the easy wins:
`resolver::resolve_conflict` (a four-case truth table), `IgnoreRules::is_ignored`,
`transfer::mime_from_extension`, `transfer::format_size` and `parse_expiry` in `main.rs` all
test without a server or a filesystem.

## Where things live

| Path | What it holds |
|---|---|
| `src/main.rs` | The clap tree and every subcommand handler, plus path resolution helpers |
| `src/config.rs` | `Config`, its defaults, validation, `sync_dir_expanded`, `save` |
| `src/api.rs` | `ApiClient`, both HTTP clients, response models, one method per endpoint |
| `src/daemon.rs` | PID and log paths, `is_running`, the two logging initializers |
| `src/hash.rs` | Buffered SHA-256 hashing |
| `src/ignore.rs` | `IgnoreRules`, including the forced `.nuage/` entries |
| `src/sync/mod.rs` | `SyncEngine`: full sync, local changes, remote changes, uploads, scans |
| `src/sync/state.rs` | The SQLite schema and every query |
| `src/sync/remote.rs` | Cursor-aware fetch: full state or incremental changes |
| `src/sync/resolver.rs` | Conflict resolution and conflict filenames |
| `src/sync/transfer.rs` | Download to temp then rename, upload, MIME and size formatting |
| `src/sync/watcher.rs` | The debounced filesystem watcher |
| `integrations/SKILL.md` | The AI agent skill the installer registers |
| `install.sh` | Clone, `cargo install --path`, register the skill |

## Adding a command

1. Add a variant to `enum Command` in `src/main.rs`, with a `clap::Args` struct if it takes
   arguments. Doc comments on the variant become the help text.
2. Add its arm to the async match in `main`. Put it in the synchronous match above only if it
   must run before the tokio runtime exists, as the daemon commands do.
3. Write an async `cmd_*` handler taking `json: bool`, and honor it.
4. Add the endpoint to `ApiClient` in `src/api.rs` if it does not exist yet.
5. Document it in [usage.md](usage.md) and, if an assistant should know about it, in
   `integrations/SKILL.md`.

## Gotchas

- **Two different `.nuage` directories.** `~/.nuage/` holds the PID file and daemon logs;
  `<sync_dir>/.nuage/` holds the state database. Neither is the other.
- **The `Origin` header is load-bearing.** `ApiClient` sets it from `server_url` because the
  server rejects multipart uploads without it. Do not drop it while refactoring the client.
- **Two HTTP clients, two timeouts.** Metadata calls get 30 seconds, transfers get 300. A
  large upload on the metadata client will time out.
- **Downloads are concurrent, four at a time,** behind a semaphore in `process_remote_files`.
  Each spawned task builds its own `ApiClient`.
- **`main.rs` is over 1200 lines** and holds both the argument definitions and every handler.
  New handlers keep making it worse; splitting the file is overdue.

## Conventions

- No inline comments. Names and structure carry the meaning.
- Remove dead code as you touch it.
- Commit messages are plain imperative sentence case.
