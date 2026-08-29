---
name: nuage
description: >
  Facile cloud storage CLI and sync daemon. Use when the user asks to upload,
  download, sync, search, or share files with Nuage.
---

# nuage — Facile cloud storage

Binary: `nuage`
Config: `~/.nuage.yml`

## When to apply

Use when the user mentions file sync, cloud storage, uploading, downloading, sharing files, or Nuage.
Triggers: "upload", "download", "sync", "share", "cloud", "nuage", "share link", "remote files"

## Commands

### Daemon
```
nuage start                    Start background sync daemon
nuage stop                     Stop daemon
nuage restart                  Restart daemon
nuage status                   Show sync/daemon status
nuage logs [-f]                Show/follow daemon logs
```

### File operations
```
nuage ls [path] [-l]           List remote files
nuage upload <src> [dest]      Upload file (src="-" for stdin)
nuage download <path> [dest]   Download file
nuage mkdir <path>             Create remote folder
nuage mv <src> <dest>          Move/rename
nuage rm <path> [-f]           Delete (-f skips confirmation)
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

### Spaces
```
nuage spaces list              List spaces, personal first
nuage spaces use <name-or-id>  Select the space every command acts on
nuage spaces use personal      Go back to your own files (--none is an alias)
```

### Setup
```
nuage login [--server <url>]   Sign in through the browser (SSO)
nuage login --token            Sign in by pasting an API token (headless)
nuage logout                   Clear the stored token
nuage upgrade                  Self-upgrade
```

## Rules
- `nuage login` opens a browser. Never run it unattended — suggest `nuage login --token`, or
  `NUAGE_TOKEN`, on a machine with no display
- `NUAGE_TOKEN`, `NUAGE_SERVER_URL` and `NUAGE_SPACE` override `~/.nuage.yml`; prefer them over
  editing the file
- Every command answers from **one space**, the personal one unless a space is selected. A path
  that exists only in a shared space reports `not found` until you pass `--space <name-or-id>`
  or run `nuage spaces use`. `--space` is global and accepts a name or an id
- `personal` names the account's own files wherever a space is named, case-insensitively:
  `nuage spaces use personal` and `nuage --space personal <cmd>`. It is the only name the server
  does not know, so it never appears in `GET /spaces`
- `nuage spaces list --json` prints `{"selected": <id|null>, "spaces": [...]}`, where `selected`
  is `null` for the personal space. Before 0.5.0 it printed the bare array now under `spaces`
- The sync daemon is deliberately not scoped by the selection: it syncs every visible space into
  one `sync_dir`
- `login` and `logout` only touch `server_url` and `token`; the user's sync settings survive
- All file/share/search/token commands support `--json`
- Daemon commands do NOT support `--json`
- Confirm before `rm` unless user says `-f`
- Use `--json` when parsing output programmatically
- Run `nuage -h` for exact syntax when unsure
