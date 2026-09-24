---
name: nuage
description: >
  Facile cloud storage CLI and sync daemon. Use when the user asks to sync,
  search, or share files with Nuage.
---

# nuage — Facile cloud storage

Binary: `nuage`
Config: `~/.nuage.yml`

## When to apply

Use when the user mentions file sync, cloud storage, sharing files, spaces, or Nuage.
Triggers: "sync", "share", "cloud", "nuage", "share link", "remote files", "space"

## Commands

### Daemon
```
nuage start                    Start background sync daemon (one task per mapped space)
nuage stop                     Stop daemon
nuage restart                  Restart daemon
nuage status                   Show sync/daemon status, one block per space
nuage logs [-f]                Show/follow daemon logs
nuage sync                     One-shot sync of every mapped space
nuage sync --verify            Re-read the whole space first, to recover lost drift
nuage watch                    Foreground watcher
```

### Reads
```
nuage search <query>           Search files
  -t file|folder              Filter by type
  -f <folder>                 Scope to folder
  -l <n>                      Max results (default 50)
```

### Share links
```
nuage share <path> [-p view|edit] [-e <duration>]
nuage unshare <id>
nuage shares
```

### Tokens
```
nuage token create -n <name>
nuage token list
nuage token revoke <id>
```

### Keys
```
nuage keys create --app <name> [--public] [--origins <urls>] [--quota <n>]
nuage keys list [--app <name>]
nuage keys revoke <id> [--yes]
```

### Spaces
```
nuage spaces list                          List spaces, with sync directory when mapped
nuage spaces create <name> [-d <text>]     Create a space
nuage spaces rename <name-or-id> <new>     Rename a space
nuage spaces rm <name-or-id> [--yes]       Delete a space (prompts; refuses personal)
```

### Setup
```
nuage login [--server <url>]   Sign in through the browser (SSO)
nuage login --token            Sign in by pasting an API token (headless)
nuage logout                   Clear the stored token
nuage upgrade                  Self-upgrade
```

## Rules
- **The daemon is the only writer.** `nuage upload`, `download`, `mkdir`, `mv` and `rm` are
  removed. To put a file in a space, drop it in that space's mapped directory and it syncs; to
  delete one, delete it there
- `nuage` with no arguments prints help and exits 0. It does not sync; use `nuage sync` or
  `nuage watch`
- A pass advances the sync cursor only when it applied everything it read. While an item is
  failed or quarantined the cursor holds, so the same item is retried next pass instead of
  dropping out of the change window
- `nuage sync --verify` re-reads the whole space and materialises anything missing locally. Use
  it when the server shows something the synced directory does not have; the daemon also runs it
  once at startup
- `NUAGE_TOKEN`, `NUAGE_SERVER_URL` and `NUAGE_SPACE` override `~/.nuage.yml`; prefer them over
  editing the file
- The read commands (`search`, `share`, `shares`) answer from the personal space unless
  `NUAGE_SPACE` names another for that run. `NUAGE_SPACE` takes a space name or an id
- `~/.nuage.yml` maps each space to a directory under `spaces:`, and the daemon syncs every
  mapping in parallel, each into its own directory. A config with no `spaces:` block is refused
  with `no spaces mapped`; the pre-0.8.0 `sync_dir` key is not read
- `personal` names the account's own files wherever a space is named, case-insensitively. It is
  the only name the server does not know, so it never appears in `GET /spaces`
- `nuage spaces list --json` prints `{"spaces":[{...,"sync_dir": ...}]}`. `sync_dir` is the
  mapped directory or `null`; there is no `selected` field
- `login` and `logout` only touch `server_url` and `token` (and `spaces` on a first run); the
  user's sync settings survive
- All read/share/search/token/keys/spaces commands support `--json`
- Daemon commands do NOT support `--json`
- Confirm before `spaces rm` unless the user passes `--yes`
- Use `--json` when parsing output programmatically
- Run `nuage -h` for exact syntax when unsure
