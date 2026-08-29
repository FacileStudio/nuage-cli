# nuage-cli — Configuration

Every key the config file accepts, every path the CLI writes to, and every environment
variable the code reads.

## The config file

`~/.nuage.yml`, resolved as `dirs::home_dir().join(".nuage.yml")`. The path is hardcoded —
there is no `--config` flag and no `XDG_CONFIG_HOME` support. `nuage login` writes it;
`Config::save` always writes to the same place.

```yaml
server_url: https://nuage.facile.studio/api
token: your-api-token
sync_dir: ~/Nuage
poll_interval: 30
ignore_patterns:
  - ".DS_Store"
  - "*.tmp"
  - ".nuage/"
  - "Thumbs.db"
  - ".git/"
selective_sync: []
```

| Key | Required | Default | What it does |
|---|---|---|---|
| `server_url` | yes | — | Base URL for every request. Trailing `/` trimmed. Empty fails validation |
| `token` | yes | — | Bearer token. Empty fails validation |
| `sync_dir` | no | `~/Nuage` | Local sync root. `~` expanded via `shellexpand` |
| `poll_interval` | no | `30` | Seconds between server polls in the sync loop |
| `ignore_patterns` | no | `[]` | Globs excluded from sync |
| `selective_sync` | no | `[]` | Path prefixes to sync. Empty means everything |
| `space` | no | absent | Space id the commands act on. Absent means the personal space. Omitted from the file until you select one, and removed again by `nuage spaces use personal` |

Validation runs at load, after the environment overrides below are applied: malformed YAML, an
empty `server_url` or an empty `token` all fail before anything touches the network. A missing
file is not itself an error — it validates as empty, and fails only if the environment does not
supply what it lacks, which is what lets `NUAGE_TOKEN` and `NUAGE_SERVER_URL` work on a machine
that has never run `nuage login`.

`nuage login`, `nuage logout` and `nuage spaces use` read this file through a path that skips
both validation and the environment overrides. That is what lets them work when the field they
are about to write is the missing one, and it is why a `NUAGE_TOKEN` or `NUAGE_SERVER_URL`
exported for a single run is not written into the file as if it had been typed there.

## The `/api` suffix

`ApiClient` appends paths like `/sync/state` and `/files` directly to `server_url` — no `/api`
prefix of its own. The Nuage server registers those routes at its own root, but the deployed
instance sits behind a Traefik router matching `PathPrefix(/api)` with a `stripprefix`
middleware. So in practice:

```yaml
server_url: https://nuage.facile.studio/api      # deployed instance
server_url: http://localhost:8080                # API reached directly, no proxy
```

Get it wrong and every request returns 404 or the SvelteKit frontend's HTML, not a
configuration error.

One consequence worth knowing: `nuage share` prints its link as `<server_url>/s/<token>`, so
with the `/api` suffix in place the printed URL contains `/api/s/...` while the actual share
page lives at `<host>/s/...`. Strip the `/api` from the printed link before sending it on.

## Ignore patterns

Matching lives in `src/ignore.rs` and uses `glob-match` against the path relative to the sync
directory. A pattern matches if any of these hold:

- it matches the full relative path
- it matches the basename alone — so `.DS_Store` catches the file at any depth
- it ends in `/` and the path is that directory or anything under it

`.nuage/` and `.nuage/**` are appended automatically in `IgnoreRules::new` unless already
present, so the state database can never sync itself.

## Selective sync

`selective_sync` is a list of path prefixes. When it is non-empty, `full_sync` reconstructs
each remote folder's path and keeps only entries matching one of the prefixes; everything else
is skipped for both folders and files. When it is empty, the whole tree syncs. `nuage status`
prints the active list.

## Environment variables

| Variable | Overrides | Notes |
|---|---|---|
| `NUAGE_TOKEN` | `token` | Blank or unset is ignored, so exporting an empty string does not lock you out |
| `NUAGE_SERVER_URL` | `server_url` | Taken verbatim; unlike `--server` it is not given an `/api` suffix |
| `NUAGE_SPACE` | `space` | An id, not a name, and not `personal` either. A non-numeric value is refused with a message naming it, rather than silently leaving you in the personal space. Resolving a name costs a request, and CI has the id to hand |

Precedence is **flag, then environment, then config file, then built-in default**, applied at
load. Both variables are read on every command, and either one is enough on its own — with both
set the CLI works with no config file at all, which is the point: a pipeline cannot run an
interactive login and must not commit a credential.

`nuage logout` warns when `NUAGE_TOKEN` is still set, since clearing the file changes nothing
while the variable outranks it.

The other variable that matters is `RUST_LOG`, consumed by
`tracing_subscriber::EnvFilter::from_default_env()` in both the terminal and daemon logging
setups. `INFO` is added as a directive on top of whatever it parses, so `RUST_LOG=debug`
raises verbosity for the sync engine's `debug!` lines.

```sh
RUST_LOG=debug nuage watch
```

## Files on disk

| Path | What it is |
|---|---|
| `~/.nuage.yml` | The config file, plaintext |
| `~/.nuage/nuage.pid` | Daemon PID file, self-healed when stale |
| `~/.nuage/logs/nuage.log` | Daemon stdout and stderr, appended, read by `nuage logs` |
| `<sync_dir>/.nuage/state.db` | SQLite sync state, WAL mode |
| `<sync_dir>/**/*.nuage-tmp` | In-flight download, renamed into place on completion |
| `<sync_dir>/**/*.conflict.*` | Local copy preserved by an unresolvable conflict |

Note the two different `.nuage` directories: `~/.nuage/` holds daemon runtime files, while
`<sync_dir>/.nuage/` holds the state database. They are unrelated.

## Token storage

The token is stored in plaintext in `~/.nuage.yml`. The CLI does not use the OS keychain, does
not encrypt the file, and does not restrict its mode. On a shared machine:

```sh
chmod 600 ~/.nuage.yml
```

Generate tokens from the Nuage dashboard, or with `nuage token create -n <name>` once you
already have one. `nuage token revoke <id>` invalidates one.

## Error messages you will actually see

| Symptom | Cause |
|---|---|
| `invalid config at ...` | Malformed YAML |
| ``no server_url configured — run `nuage login --server ...` `` | No config file, or the key is blank, and `NUAGE_SERVER_URL` is unset |
| ``not signed in — run `nuage login`, or set NUAGE_TOKEN`` | Signed out, or never signed in |
| ``the sign-in callback did not match this login attempt`` | Something other than your browser hit the loopback port. Run `nuage login` again |
| ``the server refused the login code (400)`` | The one-time code expired. It lasts sixty seconds |
| `GET /sync/state failed (401): ...` | Wrong or revoked token |
| `GET /sync/state failed (404): ...` | `server_url` is missing the `/api` suffix |
| `cannot create sync directory: ...` | `sync_dir` is not writable |
| ``no space named `x` — known: personal, ...`` | `--space` or `spaces use` was given a name no space answers to. `personal` is always one of the names it accepts |
| `[nuage] already running (PID n)` | A daemon is already up; use `restart` |
