# Changelog

All notable changes to this project are documented here. The format is
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html). While on
`0.x`, a breaking change bumps the minor.

Every entry below was reconstructed from git history on 2026-08-24, so they
record what shipped rather than what was written down at the time. The first
tag is v0.2.0; everything before it is folded into that entry.

## [Unreleased]

### Changed

- **The browser lands on the suite's sign-in page.** `src/handoff.html.tmpl` is
  a byte-for-byte copy of the template every other Facile tool renders, so a
  `nuage` login and a `courrier` login now end on the same page. It replaces
  eight lines of inline styles with an unclosed `<body>`.
- **A refused callback renders that same page.** Success and failure used to
  agree only by accident, having been written minutes apart. Both refusals, a
  callback with no code and one carrying the wrong nonce, now render the page
  with the warning colour and a line saying the login is still waiting.

### Fixed

- **A callback carrying the wrong nonce no longer ends the login.** It is
  refused and the listener keeps waiting. Ending it let any page the user has
  open close a login it did not start, simply by guessing the ephemeral port,
  which is the same class of problem as the `/favicon.ico` a browser asks for
  unprompted. Refusing the callback is what keeps a session that is not the
  user's out of this CLI; leaving the login open is the other half of it.
- The ``the sign-in callback did not match this login attempt`` error is gone,
  along with its row in `docs/configuration.md`. Nothing raises it any more.

## [0.5.0] — 2026-08-29

### Added

- `personal` names the account's own files wherever a space can be named:
  `nuage spaces use personal` and `nuage --space personal <command>`, matched
  case-insensitively. `--none` is kept as an alias for it.
- `nuage spaces list` always prints a `personal` row, with `-` where a real
  space prints its id, and the `*` selection marker sits on that row when no
  space is selected.

### Changed

- **Breaking:** `--space` is refused on the commands that never consulted it,
  rather than accepted and discarded. `nuage --space Shared sync` reads as
  scoping the sync and never could: the daemon syncs every space into one
  directory. It still applies to the file, share and search commands.
- **Breaking:** a non-numeric `NUAGE_SPACE` is refused with a message naming the
  value, where it used to parse to nothing and leave the caller in the personal
  space with no indication why. `--space` takes a name, so assuming the variable
  does too was the obvious mistake to make.
- **Breaking:** `nuage spaces list --json` prints
  `{"selected": <id|null>, "spaces": [...]}` instead of a bare array of spaces.
  `selected` answers the question the `*` answers in the human output, which the
  array had no room for. A script reading the old output reads `.spaces` now.
- **Breaking:** `nuage spaces use --json` prints `{"selected": <id|null>}`
  instead of `{"space": <id|null>}`, so both `spaces` subcommands name the field
  the same way.
- Selecting a space now points at `nuage spaces use personal` for the way back.
  The standing note about the sync daemon syncing every space is documented
  rather than printed after every selection.

### Fixed

- The personal space was invisible and unnameable. `nuage spaces list` showed
  only the shared spaces, so someone who had switched away had nothing telling
  them a personal one existed; `nuage spaces use personal` failed with
  ``no space named `personal` — known: FacileShared``; and `--none`, mentioned
  nowhere but `--help`, was the only way back. It is now a listed row, an
  accepted name, and one of the names that error prints.
- `nuage spaces use` no longer writes a `NUAGE_TOKEN` or `NUAGE_SERVER_URL` set
  for a single run into `~/.nuage.yml` as if it had been typed there. It reads
  the file the way `login` already did, unvalidated and without the environment
  overrides applied, so the command changes `space` and nothing else.
- The `spaces use` positional argument no longer shares the clap id `space` with
  the global `--space` flag, which is a collision waiting to be tripped over.
  Nothing user-visible moved except the help text, now `<NAME_OR_ID>`.

## [0.4.0] — 2026-08-29

### Added

- `nuage spaces list` and `nuage spaces use <name-or-id>`, plus a global
  `--space` flag and a `NUAGE_SPACE` variable, so commands can act on a shared
  space instead of your personal one. `spaces use --none` goes back.
- `space` in `~/.nuage.yml`, written by `spaces use` and absent until you
  select one.
- `nuage status` reports the selected space.

### Fixed

- Commands no longer answer from the personal space alone. Every request now
  carries the selected `space_id`, which is what made a folder living only in a
  shared space report `not found` — and made uploading into one fail with
  `POST /files (404): folder not found` on every sync pass, visible only in
  `nuage logs`.

### Changed

- `ApiFile` and `ApiFolder` keep the `space_id` the server already sent and the
  client used to discard, so it now appears in `--json` output.

## [0.3.0] — 2026-08-10

### Added

- Sign in through the browser, and a `logout` to undo it.

### Changed

- `install.sh` delegates to the `facile` CLI and bootstraps it from
  `get.facile.studio`, so installing nuage no longer means a second set of
  install steps to keep in sync.

## [0.2.0] — 2026-08-08

### Added

- First tagged release. A file sync daemon and CLI for Nuage: `start`, `stop`,
  `restart`, `logs` and `status`, with selective sync and progress bars.
- File management, shares, tokens, JSON output and pipe support.
- `search`, with a type filter, a folder scope and a limit.
- Rename and move detection, which uses the update API instead of deleting and
  re-uploading the file.
- Download integrity verification, a standardized installer, and a `ui` module
  so output goes through one place.
- AI agent skill registration.
- Documentation harmonized against the suite standard.

### Changed

- TLS is built with rustls instead of the system OpenSSL.
- Sync is durable, isolated and safe to leave running.
- `upgrade` shows cargo's output instead of hiding it behind `--quiet`.

### Fixed

- Recursive sync no longer misses new folder contents. Remote folders are
  topologically sorted and new local folders are walked.
- Sync no longer re-uploads every untracked local file on each run instead of
  only on the first.
- Path resolution walks the folder tree with `get_folder` rather than reading a
  flat list.
- Multipart uploads send an `Origin` header, which was the CSRF 403.
- Uploading no longer creates every folder up front before putting any file in
  them.
- The daemon guards against PID 0 and stops creating a directory twice.

[Unreleased]: https://github.com/FacileStudio/nuage-cli/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/FacileStudio/nuage-cli/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/FacileStudio/nuage-cli/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/FacileStudio/nuage-cli/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/FacileStudio/nuage-cli/releases/tag/v0.2.0
