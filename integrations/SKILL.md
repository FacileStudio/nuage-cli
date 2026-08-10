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
- `NUAGE_TOKEN` and `NUAGE_SERVER_URL` override `~/.nuage.yml`; prefer them over editing the file
- `login` and `logout` only touch `server_url` and `token`; the user's sync settings survive
- All file/share/search/token commands support `--json`
- Daemon commands do NOT support `--json`
- Confirm before `rm` unless user says `-f`
- Use `--json` when parsing output programmatically
- Run `nuage -h` for exact syntax when unsure
