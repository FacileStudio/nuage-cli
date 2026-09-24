# nuage-cli — Configuration

Every key the config file accepts, every path the CLI writes to, and every environment
variable the code reads.

## The config file

`~/.nuage.yml`, resolved as `dirs::home_dir().join(".nuage.yml")`. The path is hardcoded:
there is no `--config` flag and no `XDG_CONFIG_HOME` support. `nuage login` writes it;
`Config::save` always writes to the same place.

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
  - ".nuage/"
  - "Thumbs.db"
  - ".git/"
```

| Key | Required | Default | What it does |
|---|---|---|---|
| `server_url` | yes | none | Base URL for every request. Trailing `/` trimmed. Empty fails validation |
| `token` | yes | none | Bearer token. Empty fails validation, unless `key_command` or the environment supplies one |
| `key_command` | no | none | A command whose standard output is the token. Outranks `token`, loses to the environment |
| `spaces` | no | empty | Space name to local directory. The daemon syncs every pair. `~` expanded via `shellexpand` |
| `poll_interval` | no | `10` | Seconds between server polls in the sync loop |
| `ignore` | no | `[]` | Globs excluded from sync |

Validation runs at load, after `key_command` and the environment overrides below are applied:
malformed YAML, an empty `server_url` or an empty `token` all fail before anything touches the
network. A missing file is not itself an error. It validates as empty, and fails only if the
environment does not supply what it lacks, which is what lets `NUAGE_TOKEN` and
`NUAGE_SERVER_URL` work on a machine that has never run `nuage login`.

## Credentials

Three places can hold the token, and the order is fixed: the environment, then `key_command`,
then `token`. The last one to speak wins, so an env var exported for a single run beats a
command, and a command beats a plaintext value left in the file.

`key_command` is there to keep the credential out of the file. The command reads it from a
secret manager, a password store or a vault, and the config records only how to ask:

```yaml
server_url: https://nuage.facile.studio/api
key_command: casier get nuage
spaces:
  personal: ~/Brain
```

It runs through `sh -c`, so a pipeline or a subcommand works: `op read
op://vault/nuage/token`, `pass show nuage/api`, `security find-generic-password -w -s nuage`.
Its standard output is trimmed of surrounding whitespace, which is what makes the trailing
newline every one of those commands prints harmless.

A command that exits non-zero, or that prints nothing, is an error naming the command, and the
CLI stops there. It does not fall back to `token` and does not fall back to an empty credential:
either would turn a broken command into a confusing 401 or a confusing "not signed in". Neither
the command's standard error nor its output is echoed, because a command that fails while
holding the credential can put it in either.

It runs on every command that loads the config, which is every command except `login`,
`logout` and `spaces rm`. Those three skip it along with validation and the environment
overrides, which is why `nuage logout` warns when a `key_command` is still set: clearing
`token` changes nothing while a command still supplies one.

`nuage login`, `nuage logout` and `nuage spaces rm` read this file through a path that skips
both validation and the environment overrides. That is what lets them work when the field they
are about to write is the missing one, and it is why a `NUAGE_TOKEN` or `NUAGE_SERVER_URL`
exported for a single run is not written into the file as if it had been typed there.

## Sync directories

`spaces` is the one place a sync target is defined. Each key is a space name (`personal` for
your own files, or the name of a shared space), and each value is the local directory that space
syncs into:

```yaml
spaces:
  personal: ~/Brain
  FacileShared: ~/Nuage
```

The daemon syncs every pair, each into its own directory with its own `.nuage/state.db`. `~` is
expanded, and a missing directory is created at load. Each mapped space must be reachable by the
account; a name the server does not answer to warns and drops that target without stopping the
daemon.

Two names cannot map to one directory, and no directory may sit inside another. The config is
refused at load if either happens, because two engines on one directory would double-sync and
fight over the same state database. That check is the only guard; it is not a warning.

`sync_dir` was the pre-0.8.0 key and is no longer read. A config that has not been moved to
`spaces:` is refused with `no spaces mapped` when `sync` or the daemon runs, not at load:
`status` and `login` still work on it. There is no automatic migration, because folding an old
`sync_dir` onto `personal` would push a directory of shared-space files into the personal space.

## The `/api` suffix

`ApiClient` appends paths like `/sync/state` and `/files` directly to `server_url`, with no
`/api` prefix of its own. The Nuage server registers those routes at its own root, but the
deployed instance sits behind a Traefik router matching `PathPrefix(/api)` with a `stripprefix`
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
- it matches the basename alone, so `.DS_Store` catches the file at any depth
- it ends in `/` and the path is that directory or anything under it

`.nuage/` and `.nuage/**` are appended automatically in `IgnoreRules::new` unless already
present, so the state database can never sync itself. `ignore` stays global and is applied to
every target. The key used to be `ignore_patterns`; that spelling is still read.

## Environment variables

| Variable | Overrides | Notes |
|---|---|---|
| `NUAGE_TOKEN` | `token` | The documented name. Blank or unset is ignored, so exporting an empty string does not lock you out |
| `NUAGE_API_KEY` | `token` | An alias, checked after `NUAGE_TOKEN` |
| `NUAGE_KEY` | `token` | An alias, checked last |
| `NUAGE_SERVER_URL` | `server_url` | Taken verbatim; unlike `--server` it is not given an `/api` suffix |
| `NUAGE_SPACE` | the space the read commands act on | A space name or an id, `personal` included. Both are accepted, so a name costs one request to resolve and an id does not |

Precedence is **flag, then environment, then `key_command`, then config file, then built-in
default**, applied at load. Every variable is read on every command, and the token variable is
enough on its own. With a token variable and `NUAGE_SERVER_URL` both set the CLI works with no
config file at all, which is the point: a pipeline cannot run an interactive login and must not
commit a credential.

The three token names are checked in the order above and the first non-empty one wins, so
setting `NUAGE_KEY` and `NUAGE_TOKEN` in the same environment uses `NUAGE_TOKEN`.

`NUAGE_SPACE` scopes the read commands (`search`, `share`, `shares`) for one run. It does
not change what the daemon syncs: the daemon follows the `spaces:` map and nothing else. Use it
to read a shared space without mapping it.

`nuage logout` warns when `NUAGE_TOKEN` is still set, since clearing the file changes nothing
while the variable outranks it. It warns the same way when a `key_command` is set.

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
| `<dir>/.nuage/state.db` | SQLite sync state for one target, WAL mode |
| `<dir>/**/*.nuage-tmp` | In-flight download, renamed into place on completion |
| `<dir>/**/*.conflict.*` | Local copy preserved by an unresolvable conflict |

`<dir>` is any directory named in `spaces`. Each target has its own state database, so a
problem in one space never leaves another's files stranded.

Note the two different `.nuage` directories: `~/.nuage/` holds daemon runtime files, while
`<dir>/.nuage/` holds the state database. They are unrelated.

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
| `no server_url configured — run \`nuage login --server ...\`` | No config file, or the key is blank, and `NUAGE_SERVER_URL` is unset |
| `not signed in — run \`nuage login\`, or set NUAGE_TOKEN` | Signed out, or never signed in |
| the server refused the login code (400) | The one-time code expired. It lasts sixty seconds |
| `GET /sync/state failed (401): ...` | Wrong or revoked token |
| `GET /sync/state failed (404): ...` | `server_url` is missing the `/api` suffix |
| `cannot create sync directory: ...` | A mapped directory is not writable |
| `no spaces mapped` | The config maps nothing to sync. Add a `spaces:` block; the pre-0.8.0 `sync_dir` key is not read |
| two names map to one directory, or one nests inside another | Overlapping sync targets. The config is refused at load |
| `no space named \`x\` — known: personal, ...` | `NUAGE_SPACE` was given a name no space answers to. `personal` is always one of the names it accepts |
| `[nuage] already running (PID n)` | A daemon is already up; use `restart` |
