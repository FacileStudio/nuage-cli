# nuage-cli — Architecture

How the `nuage` binary is wired: the daemon topology, the bidirectional sync algorithm, the
local state database, and every endpoint it consumes.

## Topology

```
                          ~/.nuage.yml  (server_url, token, sync_dir, poll_interval)
                                  │
       ┌──────────────────────────┴───────────────────────────┐
       ▼                                                      ▼
  nuage daemon (forked)                              nuage <one-shot command>
   ├─ FsWatcher  ──▶ local changes (2s debounce)      ls / upload / download / mv / rm
   ├─ poll timer ──▶ remote changes (poll_interval)   share / shares / search / token
   └─ SyncEngine                                              │
       │                                                      │
       ├── <sync_dir>/.nuage/state.db   SQLite, WAL           │
       └───────────────┬──────────────────────────────────────┘
                       │  reqwest, Authorization: Bearer <token>
                       ▼
              Nuage Go API  (server_url)
                       │
              PostgreSQL + object storage

  ~/.nuage/nuage.pid        PID file
  ~/.nuage/logs/nuage.log   daemon log
```

## Process modes

`main()` splits before the async runtime is created:

- `start`, `stop`, `restart`, `logs` run synchronously. `start` daemonizes the process with
  `daemonize`, redirecting stdout and stderr into `~/.nuage/logs/nuage.log` and writing
  `~/.nuage/nuage.pid`, then builds a runtime inside the forked child.
- Everything else — including the default, no-argument invocation — initializes terminal
  logging, builds a `tokio` runtime, and blocks on the matching handler.

With no subcommand, `nuage` behaves exactly like `nuage watch`: a foreground sync loop.

`stop` reads the PID file, sends `SIGTERM`, polls `kill(pid, 0)` every 100 ms for up to five
seconds, then escalates to `SIGKILL` and removes the PID file. `is_running()` self-heals a
stale PID file: unreadable, unparseable or dead PIDs are deleted and reported as stopped.

## The sync loop

`sync_loop` runs both directions off one loop:

```
loop {
  watcher.try_recv()          non-blocking: any debounced local paths?
    └─▶ process_local_changes

  select! {
    SIGTERM | SIGINT   -> break
    poll_timer.tick()  -> process_remote_changes   every poll_interval seconds
    sleep(100ms)       -> keep the loop responsive
  }
}
```

Local changes are therefore picked up on the next 100 ms turn of the loop, while remote
changes wait for the poll timer. `FsWatcher` wraps `notify-debouncer-mini` with a 2-second
debounce and filters ignored paths inside the callback, before anything reaches the channel.

## Full sync

`SyncEngine::full_sync` is what `sync`, `watch` and the daemon all run first:

1. **Fetch remote changes.** With no cursor, `GET /sync/state` returns the whole tree. With a
   cursor, `GET /sync/changes?since=<cursor>` returns changed and deleted files and folders.
2. **Apply selective sync,** if `selective_sync` is non-empty: folder paths are reconstructed
   from the change set and anything outside the selected prefixes is dropped.
3. **Create folders** locally, topologically sorted so parents exist before children.
4. **Delete locally** anything the server reports as deleted.
5. **Download changed files,** four at a time behind a `tokio::sync::Semaphore`, writing to a
   `.nuage-tmp` sibling and renaming into place (mode `0644` on Unix).
6. **Upload untracked local files** that the state DB has never seen.
7. **Store the server's `server_time`** as the new cursor.

The report counts downloads, uploads, local and remote deletions, conflicts and folders
created.

## Conflict resolution

Before overwriting an existing local file, the engine hashes it and calls
`resolver::resolve_conflict(local_hash, remote_hash, last_known_hash, path)`:

| Situation | Resolution |
|---|---|
| Local and remote hashes match | `UseRemote` — nothing changes |
| Local matches the last known hash, remote does not | `UseRemote` — the server moved on |
| Remote matches the last known hash, local does not | `UseLocal` — skip the download |
| Neither matches, or nothing is known | `KeepBoth` — rename local, then download |

`KeepBoth` renames the local file to `<stem>.conflict.<ext>` (or `<stem>.conflict` when there
is no extension) beside the original, and the remote copy lands under the original name.
Nothing is ever silently discarded.

## Local state

`<sync_dir>/.nuage/state.db` is a SQLite database opened with `journal_mode=WAL` and
`synchronous=NORMAL`, migrated on every open with `CREATE TABLE IF NOT EXISTS`:

| Table | Columns |
|---|---|
| `files` | `id`, `facile_id`, `name`, `local_path` (unique), `hash`, `size`, `folder_id`, `remote_updated_at`, `local_modified_at`, `synced_at` |
| `folders` | `id`, `facile_id`, `name`, `local_path` (unique), `parent_id`, `remote_updated_at`, `synced_at` |
| `sync_cursor` | `key`, `value` — one row, `last_sync` |

`local_path` is relative to the sync directory, so the whole database travels with the folder.
Deleting `.nuage/` resets the client to a first-run full sync. `.nuage/` is force-added to the
ignore rules in `IgnoreRules::new`, so the state DB can never sync itself into the cloud.

Change detection is SHA-256 over a 64 KB buffered reader (`src/hash.rs`), compared against the
`hash` column and the server's hash.

## HTTP client

`ApiClient` builds two reqwest clients:

- the default client, 30-second timeout, for metadata calls
- a `transfer_client`, 300-second timeout, for uploads and downloads

Both set an `Origin` header derived from `server_url` (scheme, host and port), which is what
keeps the server's CSRF check from rejecting multipart uploads. Paths are appended verbatim
to `server_url`, whose trailing slash is trimmed.

## Endpoints used

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/sync/state` | Full tree, and the connection test in `nuage login` |
| `GET` | `/sync/changes?since={cursor}` | Incremental changes and deletions |
| `GET` | `/folders` | Root folders |
| `GET` | `/folders/{id}` | Folder detail with its files and subfolders |
| `POST` | `/folders` | Create a folder |
| `PUT` | `/folders/{id}` | Rename or reparent |
| `DELETE` | `/folders/{id}` | Delete |
| `POST` | `/files` | Multipart upload |
| `GET` | `/files/{id}/download` | Download, buffered or streamed |
| `PUT` | `/files/{id}` | Rename or move |
| `DELETE` | `/files/{id}` | Delete |
| `GET` | `/search?q=&limit=&type=&folder_id=` | Search |
| `POST` | `/shares` | Create a share link |
| `GET` | `/shares/by-me` | List your shares |
| `DELETE` | `/shares/{id}` | Revoke |
| `GET` `POST` | `/users/me/api-token` | List and create API tokens |
| `DELETE` | `/users/me/api-token/{id}` | Revoke a token |
| `GET` | `/auth/config` | Which login flows the instance accepts |
| `GET` | `/auth/oidc?flow=cli&port=&cli_state=` | Start the browser sign-in |
| `POST` | `/auth/oidc/exchange` | Trade the one-time code for a token |

The three `/auth` endpoints are served by [porte](https://github.com/FacileStudio/porte), the
suite's Go auth kit, and are the only ones `nuage` calls without a bearer token. `login.rs`
uses a plain `reqwest::Client` for them rather than `ApiClient`, which exists to attach
credentials it does not yet have.

porte also ships the listener half of that flow, as `porte/loopback`, and every Go CLI in the
suite now links it. This one cannot, so `login.rs` keeps its own listener and `handoff.rs`
keeps a byte-for-byte copy of the page porte renders. The copy is the point: `diff` against
`porte/internal/handoff` or `Mycelium/internal/server/handoff.html.tmpl` is what proves the
pages have not drifted, which a page rewritten in Rust could never have offered.

None of these carry an `/api` prefix, but the deployed Nuage sits behind a Traefik router that
strips one — so `server_url` normally ends in `/api`. See
[configuration.md](configuration.md).

## Path resolution

The remote file commands take human paths like `/Documents/report.pdf`. `resolve_path` walks
them one segment at a time: root folders come from `GET /folders`, then each subsequent
segment is looked up in that folder's detail response. The result is `Root`, `Folder` or
`File`, and commands reject the combinations that make no sense — you cannot `download` a
folder, `mkdir` inside a file, or `rm` the root.

That means a deep path costs one request per level. It is correct, not fast.

## Suite integration

The CLI is a plain REST consumer. It does not use `pool`, `enveloppe` or Journal, and it does
not speak OIDC — authentication is a Nuage API token only.
