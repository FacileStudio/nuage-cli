# nuage-cli — Usage

The complete command reference: daemon control, sync, spaces, remote reads, shares, search,
tokens, and the AI agent skill.

## Synopsis

```sh
nuage [--json] [--no-color] [COMMAND]
```

`--json` is a global flag. It switches the read, space, share, search, token and keys commands
to machine-readable output; the daemon commands (`start`, `stop`, `restart`, `logs`) accept it
but ignore it.

`--no-color` is a global flag. It disables colored output. Color is only used when the target
stream is a terminal and `NO_COLOR` is unset, and `--json` forces it off as well.

With no command, `nuage` prints the help message on stdout and exits `0`. It does not sync.

Every command except `upgrade`, `login` and `logout` requires a valid `~/.nuage.yml`, or the
`NUAGE_TOKEN` and `NUAGE_SERVER_URL` variables that override it. See
[configuration.md](configuration.md).

## The daemon is the only writer

`nuage upload`, `nuage download`, `nuage mkdir`, `nuage mv` and `nuage rm` are gone. To put a
file in a space, drop it in that space's mapped directory and it syncs; to delete one, delete
it there. The `spaces:` block in `~/.nuage.yml` maps each space to a directory, and the daemon
syncs every mapping in parallel.

The remaining remote commands are reads: `search`, `share`, `shares`. Nothing else writes
directly to a space.

## Setup

### `nuage login`

Signs in and writes `server_url` and `token` into `~/.nuage.yml`. On a first run it also seeds
`spaces: { personal: <dir> }` with the directory you choose.

```sh
nuage login
nuage login --server https://nuage.facile.studio
nuage login --token
```

| Flag | What it does |
|---|---|
| `--server <url>` | The instance. The `/api` suffix is appended if you leave it off |
| `--token` | Skip the browser and paste an API token instead |

The server URL is resolved flag first, then `NUAGE_SERVER_URL`, then whatever is already in the
config file, then a prompt.

**The browser flow.** `nuage login` first asks the server what it accepts, with
`GET <server_url>/auth/config`, which answers `{"sso_only":true,"oidc_enabled":true}` on a
Facile deployment. When OIDC is enabled the CLI:

1. binds `127.0.0.1:0` and takes the ephemeral port, so two shells can log in at once;
2. generates a 16-byte nonce from `/dev/urandom`;
3. opens `<server_url>/auth/oidc?flow=cli&port=<port>&cli_state=<nonce>`;
4. serves exactly one callback at `http://127.0.0.1:<port>/`, ignoring stray requests such as
   the browser's unprompted `/favicon.ico`, and **aborts with HTTP 400 if the returned `state`
   does not match the nonce**, which is why a nonce is sent at all;
5. exchanges the one-time `code` (single use, sixty seconds) for a token over
   `POST <server_url>/auth/oidc/exchange`.

The token never travels in a URL, so it cannot land in browser history, a `Referer` header or a
proxy log. The wait times out after three minutes.

**The token flow.** Pass `--token` and the CLI prompts for an API token minted in the dashboard
under Settings then API, reading it without echo. This is the path for a headless machine.
Login also falls back to it on its own when a browser cannot be opened, unless the instance
reports `sso_only`, in which case there is nothing to fall back to and it says so.

**What is preserved.** Login is a read-modify-write. Only `server_url`, `token` and, on a first
run, `spaces` change; `poll_interval` and `ignore` are read from the existing file and written
back as they were. The sync directory and the default ignore list are
only prompted for and seeded when there is no config file at all.

The connection is tested with `GET /sync/state` before anything is written, so a bad token
aborts rather than replacing a working one. The file is created at mode `0600`.

### `nuage logout`

```sh
nuage logout
```

Blanks `token` and leaves every other key alone, including `server_url`. Logging out is not a
reason to make the user retype where their server is. Running it when already signed out is not
an error. If `NUAGE_TOKEN` is set in the environment it warns, because that variable outranks
the file and the user would otherwise still be authenticated.

### `nuage upgrade`

```sh
nuage upgrade
```

Runs `cargo install --git https://github.com/FacileStudio/nuage-cli.git --force`, so `cargo`
must be on `PATH`. This is the only command that does not read the config file.

## Daemon and sync

### `nuage start`

Fork a background sync daemon. Refuses to start if one is already running, validates the config
first, then writes `~/.nuage/nuage.pid` and appends output to
`~/.nuage/logs/nuage.log`.

The daemon runs one sync task per mapped space, so `spaces` with two entries means two engines
in one process, each in its own directory.

```sh
nuage start
```

### `nuage stop`

```sh
nuage stop
```

Sends `SIGTERM`, waits up to five seconds, escalates to `SIGKILL`, and removes the PID file.
Prints `[nuage] not running` when there is nothing to stop, and cleans up a stale PID file
automatically.

### `nuage restart`

```sh
nuage restart
```

`stop` then `start`.

### `nuage logs`

| Flag | What it does |
|---|---|
| `-f`, `--follow` | Follow the log instead of printing and exiting |

```sh
nuage logs
nuage logs -f
```

Shells out to `tail -n 50`, adding `-f` when asked. Prints `[nuage] no logs yet` when the
file does not exist.

### `nuage watch`

```sh
nuage watch
```

Foreground equivalent of the daemon: full sync of every mapped space, then the watch-and-poll
loop, logging to the terminal. `Ctrl-C` (SIGINT) or SIGTERM shuts it down cleanly. A bare
`nuage` no longer runs this; pass `watch` explicitly.

### `nuage sync`

```sh
nuage sync
nuage sync --dry-run
nuage sync --verify
```

One-shot sync of every mapped space, then exit. Each target's report line is prefixed with its
space name, and the command exits `1` if any target failed, even when the others succeeded.

| Flag | What it does |
|---|---|
| `--dry-run` | Show what would change, apply nothing |
| `--allow-bulk-delete` | Allow propagating an unusually large batch of local deletions |
| `--retry-failed` | Clear quarantined files and retry them |
| `--repair-state` | Drop tracking records whose local file is gone, then re-enumerate |
| `--verify` | Re-read the whole space before syncing, to recover anything the incremental feed lost |

A pass prints `[nuage] sync complete (N changes)` and, when any occurred,
`[nuage] N conflicts resolved (local copies renamed)`. `--dry-run` prints one planned-change
line per entry instead, or `Already in sync` when there is nothing to do.

A file the server refuses repeatedly is quarantined and skipped, so it cannot block the rest of
the pass. `nuage status` lists quarantined files; `nuage sync --retry-failed` clears the
quarantine and tries them again. While anything is quarantined, or after any item failed, the
pass holds the sync cursor instead of advancing it, so the item stays in the next window rather
than disappearing from it.

`--repair-state` recovers from tracking that drifted out of agreement with the filesystem. It
drops records whose local file is gone, leaves the server untouched, and forgets the cursor so
the next pass rebuilds tracking from the server's own view.

`--verify` re-reads the whole space rather than the changes since the cursor, and materialises
anything missing locally. A normal pass only sees what the server changed since the cursor, so
an item that was fetched and then skipped would never be offered again. The daemon runs the
same verification once at startup.

### `nuage status`

```sh
nuage status
```

```
Daemon: running (PID 41233)
Server: https://nuage.facile.studio/api

personal  /Users/you/Brain
  Last sync: 2026-08-05T14:02:11Z
  Files: 318
  Folders: 44

FacileShared  /Users/you/Nuage
  Last sync: 2026-08-05T14:02:09Z
  Files: 96
  Folders: 12
```

One block per sync target, each naming its space and its directory. `Last sync` is that
target's stored cursor, or `never`. With no state database yet a target reports zero files and
folders. Quarantined files are listed per target with the reason and the number of failures.

## Spaces

A Nuage account has a personal space and, when someone shares one with it, any number of named
spaces. **The sync daemon covers exactly the spaces mapped in `spaces:`.** A space that is not
mapped is not synced, and a folder living only in an unmapped space is invisible to the read
commands until `NUAGE_SPACE` names it.

**The personal space is named `personal`**, matched case-insensitively. The server never returns
it from `GET /spaces`, because it is the absence of a space rather than one of them, so the CLI
supplies the name itself.

The read commands (`search`, `share`, `shares`) answer from your personal space unless
`NUAGE_SPACE` names another for that run. `NUAGE_SPACE` takes a space name or an id; a name
costs one request to resolve, and `personal` is answered locally.

```sh
NUAGE_SPACE=FacileShared nuage search invoice -f /Clients
NUAGE_SPACE=3 nuage search invoice
```

### `nuage spaces list`

```sh
nuage spaces list
```

```
* -    personal                 ~/Brain
  1    FacileShared             owner
```

`personal` is always the first row, with `-` where a real space prints its id. The `*` marks a
space that has a sync directory, and that directory prints in the last column. An unmapped
space prints its role instead.

`--json` prints an object:

```json
{"spaces":[{"id":1,"name":"FacileShared","description":"","role":"owner","sync_dir":null}]}
```

Each space carries `id`, `name`, `description`, your `role`, and `sync_dir`, which is its
directory as written in the config or `null` when it is not mapped. The personal space is not in
that array, since it has no id to report. There is no `selected` field any more; the daemon
covers every mapped space and the read commands default to personal.

### `nuage spaces create`

```sh
nuage spaces create FacileShared
nuage spaces create FacileShared --description "Client work"
```

| Argument / flag | What it does |
|---|---|
| `<NAME>` | Name of the new space. Required |
| `-d`, `--description <TEXT>` | Optional description |

Creates the space server-side. Prints its row. `--json` prints the created space object.

### `nuage spaces rename`

```sh
nuage spaces rename FacileShared Clients
nuage spaces rename 3 Clients
```

Takes a space name or an id, and the new name. Prints the updated row, or the space object
under `--json`. A rename does not touch a `spaces:` mapping: the config keys on the old name, so
rename the key by hand if you keep syncing that space.

### `nuage spaces rm`

```sh
nuage spaces rm FacileShared
nuage spaces rm 3 --yes
```

| Flag | What it does |
|---|---|
| `-y`, `--yes` | Delete without prompting |

Deletes a space server-side and removes its entry from `spaces:`. Prompts
`delete space <id>? [y/N]` unless `--yes` is given; anything other than `y` cancels. `--json`
never prompts.

`personal` cannot be deleted: it is your own file tree, not a space. The command refuses it
before it resolves anything.

## Remote reads

### `nuage search`

| Flag | Default | What it does |
|---|---|---|
| `<QUERY>` | none | Search string |
| `-t`, `--type <TYPE>` | none | `file` or `folder` |
| `-f`, `--folder <PATH>` | none | Scope to a folder, resolved to its ID |
| `-l`, `--limit <N>` | `50` | Maximum results |

```sh
nuage search invoice
nuage search invoice -t file -l 10
nuage search 2026 -f /Documents --json
```

Human output is `<kind>  <size>  <date>  <path>`, with folders shown as `dir` and a trailing
`/`. Prints `no results` when empty.

## Share links

### `nuage share`

| Flag | Default | What it does |
|---|---|---|
| `<PATH>` | none | Remote file or folder to share |
| `-p`, `--permission <PERM>` | `view` | `view` or `edit` |
| `-e`, `--expires <WHEN>` | none | RFC3339 timestamp, or a duration |

Durations are a number plus `m`, `h`, `d` or `w`; anything containing `T` or `-` is passed
through as an RFC3339 timestamp. An unknown unit is an error.

```sh
nuage share /Documents/report.pdf
nuage share /Documents -p edit -e 7d
nuage share /Documents/report.pdf -e 2026-09-01T00:00:00Z
```

Prints the share URL, and an `expires:` line when there is an expiry. Sharing the root is an
error.

The URL is built as `<server_url>/s/<token>`. If `server_url` ends in `/api`, which it must
for the API calls to work against the deployed instance, the printed link contains an extra
`/api` segment that the real share page does not use. Strip it before sending the link on.

### `nuage shares`

```sh
nuage shares
nuage shares --json
```

Lists your shares as `#<id>  <token>  <file|folder> <id>  perm=<perm>  expires=<when>`, with
`never` for shares that do not expire. Prints `no active shares` when empty.

### `nuage unshare`

```sh
nuage unshare 42
```

Revokes a share by its numeric ID, the `#<id>` from `nuage shares`. Prints `share 42 revoked`.

## API tokens

### `nuage token create`

| Flag | What it does |
|---|---|
| `-n`, `--name <NAME>` | Token name. Required |

```sh
nuage token create -n laptop
```

Prints the ID, the name and the token value, followed by
`save this token, it won't be shown again.` The value is only ever returned once.

### `nuage token list`

```sh
nuage token list
```

Prints `#<id>  <name>  created <YYYY-MM-DD>`, or `no API tokens`.

### `nuage token revoke`

```sh
nuage token revoke 7
```

Revokes a token by ID. Prints `token 7 revoked`.

## API keys

### `nuage keys create`

| Flag | What it does |
|---|---|
| `-a`, `--app <NAME>` | Application name. Required |
| `--public` | Create a public browser key instead of a secret key |
| `--origins <URLS>` | Comma-separated allowed origins (for public keys) |
| `--quota <N>` | Daily event quota limit (for public keys) |

```sh
nuage keys create --app myapp
nuage keys create --app myapp --public --origins https://example.com --quota 1000
```

Prints the created key metadata and the raw token value. In `--json` mode, returns the full JSON
response.

### `nuage keys list`

| Flag | What it does |
|---|---|
| `-a`, `--app <NAME>` | Filter keys by application name |

```sh
nuage keys list
nuage keys list --app myapp
```

Prints `#<id>  <app>  <kind>  <prefix>  <status>  <quota>  <created>`, or `no API keys found`.

### `nuage keys revoke`

| Flag | What it does |
|---|---|
| `-y`, `--yes` | Confirm revocation without prompting |

```sh
nuage keys revoke 42
nuage keys revoke 42 --yes
```

Revokes an API key by ID. Prints `revoked key 42`.

## Machine-readable output

`--json` is accepted anywhere and honored by `search`, `share`, `shares`, `unshare`, the
`spaces` subcommands, `token` and `keys`. It prints compact JSON on stdout and never prompts,
so `spaces rm` skips its confirmation.

```sh
nuage --json search invoice -t file | jq '.[0].path'
```

## AI agent skill

`install.sh` registers `integrations/SKILL.md` with whichever assistants it finds on `PATH`:

- `claude` present: copies the file to `~/.claude/skills/nuage/SKILL.md` and injects its
  contents into `~/.claude/CLAUDE.md`
- `codex` present: injects the same contents into `~/.codex/AGENTS.md`

Injection is idempotent: the block is fenced by `<!-- nuage:start -->` and `<!-- nuage:end -->`
markers, and a rerun strips the old block before appending the new one. Neither file is
created unless the corresponding binary exists. To opt out, install with `cargo install --git`
instead of the script; to remove it later, delete the marked block and the skill directory.

The skill tells an assistant which commands exist and to prefer `--json` when parsing output.
Keep it in step with this page when commands change.
