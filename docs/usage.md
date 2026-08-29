# nuage-cli — Usage

The complete command reference: daemon control, sync, remote file management, shares, search,
tokens, and the AI agent skill.

## Synopsis

```sh
nuage [--json] [--space NAME_OR_ID] [COMMAND]
```

`--space` is a global flag. It overrides the selected space for one invocation and takes a
name or an id; a name costs one extra request to resolve. See
[Spaces](#spaces) for what a space changes.

`--json` is a global flag. It switches the file, share, search and token commands to
machine-readable output; the daemon commands (`start`, `stop`, `restart`, `logs`) accept it
but ignore it. With no command, `nuage` behaves exactly like `nuage watch`.

Every command except `upgrade`, `login` and `logout` requires a valid `~/.nuage.yml`, or the
`NUAGE_TOKEN` and `NUAGE_SERVER_URL` variables that override it — see
[configuration.md](configuration.md).

## Setup

### `nuage login`

Signs in and writes `server_url` and `token` into `~/.nuage.yml`.

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
   does not match the nonce** — that check is why a nonce is sent at all;
5. exchanges the one-time `code` (single use, sixty seconds) for a token over
   `POST <server_url>/auth/oidc/exchange`.

The token never travels in a URL, so it cannot land in browser history, a `Referer` header or a
proxy log. The wait times out after three minutes.

**The token flow.** Pass `--token` and the CLI prompts for an API token minted in the dashboard
under Settings then API, reading it without echo. This is the path for a headless machine.
Login also falls back to it on its own when a browser cannot be opened, unless the instance
reports `sso_only`, in which case there is nothing to fall back to and it says so.

**What is preserved.** Login is a read-modify-write. Only `server_url` and `token` change;
`sync_dir`, `poll_interval`, `ignore_patterns` and `selective_sync` are read from the existing
file and written back as they were. The sync directory and the default ignore list are only
prompted for and seeded when there is no config file at all.

The connection is tested with `GET /sync/state` before anything is written, so a bad token
aborts rather than replacing a working one. The file is created at mode `0600`.

### `nuage logout`

```sh
nuage logout
```

Blanks `token` and leaves every other key alone, including `server_url` — logging out is not a
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

Fork a background sync daemon. Refuses to start if one is already running, validates the
config first, then writes `~/.nuage/nuage.pid` and appends output to
`~/.nuage/logs/nuage.log`.

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

Foreground equivalent of the daemon: full sync, then the watch-and-poll loop, logging to the
terminal. `Ctrl-C` (SIGINT) or SIGTERM shuts it down cleanly. This is what a bare `nuage` runs.

### `nuage sync`

```sh
nuage sync
```

One-shot bidirectional sync, then exit. Prints `[nuage] sync complete (N changes)` and, when
any occurred, `[nuage] N conflicts resolved (local copies renamed)`.

### `nuage status`

```sh
nuage status
```

```
Daemon: running (PID 41233)
Server: https://nuage.facile.studio/api
Space: personal
Sync dir: /Users/you/Nuage
Last sync: 2026-08-05T14:02:11Z
Files: 318
Folders: 44
```

`Last sync` is the stored cursor, or `never`. `Space` is the selected space id, or `personal`.
With no state database yet it reports zero files and folders. A non-empty `selective_sync` is
listed on an extra line.

## Spaces

A Nuage account has a personal space and, when someone shares one with it, any number of named
spaces. **Every command answers from one space at a time.** Without a selection that is your
personal space, so a folder living only in a shared space is invisible: `nuage ls /Clients`
lists the personal `Clients` and `nuage ls /Clients/DMS` reports `not found` even when the
shared space has it.

### `nuage spaces list`

```sh
nuage spaces list
```

```
* 1    FacileShared             owner
```

The `*` marks the current selection. `--json` gives `id`, `name`, `description` and your
`role`.

### `nuage spaces use`

```sh
nuage spaces use <name-or-id>
nuage spaces use --none
```

Writes `space` to `~/.nuage.yml` and leaves every other key alone. A name is matched
case-insensitively and resolved to an id, so a later rename does not strand the config.
`--none` clears it and goes back to the personal space.

### What a space does not change

**The sync daemon is not scoped by the selection.** It syncs every space you can see into one
`sync_dir`, which is the merged tree `~/Nuage` already holds, and narrowing it would strand
the other spaces' files in a directory the engine stopped tracking. Per-space sync needs its
own sync directory and is not built yet.

## Remote file management

### `nuage ls`

| Argument / flag | Default | What it does |
|---|---|---|
| `[PATH]` | `/` | Remote path to list |
| `-l`, `--long` | off | Show size and date |

```sh
nuage ls
nuage ls /Documents
nuage ls /Documents -l
nuage ls / --json
```

Folders sort before files, then alphabetically, and folder names print with a trailing `/`.
Long format is `<size>  <YYYY-MM-DD>  <name>`. Pointing `ls` at a file lists just that file.

### `nuage upload`

| Argument | Default | What it does |
|---|---|---|
| `<SOURCE>` | — | Local file path, or `-` to read stdin |
| `[DEST]` | `/` | Remote destination path |

```sh
nuage upload report.pdf /Documents
nuage upload report.pdf /Documents/renamed.pdf
cat backup.sql | nuage upload - /Backups/backup.sql
```

The last segment of `DEST` becomes the filename; when `DEST` is a bare folder the local
filename is kept, and a stdin upload with no name becomes `stdin`. The MIME type is inferred
from the extension (`application/octet-stream` for stdin and unknown extensions). Prints
`uploaded <name> (<size>)`. Piping from a terminal with no data bails.

### `nuage download`

| Argument | Default | What it does |
|---|---|---|
| `<REMOTE_PATH>` | — | Remote file to fetch |
| `[LOCAL_DEST]` | `.` | Local file or directory |

```sh
nuage download /Documents/report.pdf
nuage download /Documents/report.pdf ~/Desktop/
nuage download /Documents/report.pdf ./renamed.pdf
```

Streams to a `.nuage-tmp` file and renames on completion, so an interrupted download never
leaves a truncated file under the real name. A progress bar appears on stderr for files over
100 KB when stderr is a terminal and `--json` is off. Downloading a folder or the root is an
error.

### `nuage mkdir`

```sh
nuage mkdir /Documents/2026
```

Creates the final segment inside its already-existing parent — it is not recursive. Prints
`created <path>/`.

### `nuage mv`

```sh
nuage mv /Documents/report.pdf /Archive/report.pdf
nuage mv /Documents/old-name.pdf /Documents/new-name.pdf
nuage mv /Documents /Archive/Documents
```

Works on both files and folders: the destination's parent becomes the new parent and its last
segment the new name. Moving the root is an error.

### `nuage rm`

| Flag | What it does |
|---|---|
| `-f`, `--force` | Skip the confirmation prompt |

```sh
nuage rm /Documents/report.pdf
nuage rm -f /Documents/old-folder
```

Prompts `delete <name>? [y/N]` unless `--force` or `--json` is given — anything other than `y`
cancels. Deleting a folder deletes it server-side; deleting the root is an error.

### `nuage search`

| Flag | Default | What it does |
|---|---|---|
| `<QUERY>` | — | Search string |
| `-t`, `--type <TYPE>` | — | `file` or `folder` |
| `-f`, `--folder <PATH>` | — | Scope to a folder, resolved to its ID |
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
| `<PATH>` | — | Remote file or folder to share |
| `-p`, `--permission <PERM>` | `view` | `view` or `edit` |
| `-e`, `--expires <WHEN>` | — | RFC3339 timestamp, or a duration |

Durations are a number plus `m`, `h`, `d` or `w`; anything containing `T` or `-` is passed
through as an RFC3339 timestamp. An unknown unit is an error.

```sh
nuage share /Documents/report.pdf
nuage share /Documents -p edit -e 7d
nuage share /Documents/report.pdf -e 2026-09-01T00:00:00Z
```

Prints the share URL, and an `expires:` line when there is an expiry. Sharing the root is an
error.

The URL is built as `<server_url>/s/<token>`. If `server_url` ends in `/api` — which it must
for the API calls to work against the deployed instance — the printed link contains an extra
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

Revokes a share by its numeric ID — the `#<id>` from `nuage shares`. Prints
`share 42 revoked`.

## API tokens

### `nuage token create`

| Flag | What it does |
|---|---|
| `-n`, `--name <NAME>` | Token name. Required |

```sh
nuage token create -n laptop
```

Prints the ID, the name and the token value, followed by
`save this token -- it won't be shown again.` The value is only ever returned once.

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

## Machine-readable output

`--json` is accepted anywhere and honored by `ls`, `upload`, `download`, `mkdir`, `mv`, `rm`,
`share`, `shares`, `unshare`, `search` and all three `token` subcommands. It prints compact
JSON on stdout, suppresses the progress bar, and — importantly — makes `rm` skip its
confirmation prompt, since there is no one to answer it.

```sh
nuage --json ls /Documents | jq -r '.[] | select(.type == "file") | .name'
nuage --json search invoice -t file | jq '.[0].path'
```

## AI agent skill

`install.sh` registers `integrations/SKILL.md` with whichever assistants it finds on `PATH`:

- `claude` present — copies the file to `~/.claude/skills/nuage/SKILL.md` and injects its
  contents into `~/.claude/CLAUDE.md`
- `codex` present — injects the same contents into `~/.codex/AGENTS.md`

Injection is idempotent: the block is fenced by `<!-- nuage:start -->` and `<!-- nuage:end -->`
markers, and a rerun strips the old block before appending the new one. Neither file is
created unless the corresponding binary exists. To opt out, install with `cargo install --git`
instead of the script; to remove it later, delete the marked block and the skill directory.

The skill tells an assistant which commands exist and to prefer `--json` when parsing output.
Keep it in step with this page when commands change.
