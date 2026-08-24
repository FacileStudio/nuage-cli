# Changelog

All notable changes to this project are documented here. The format is
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html). While on
`0.x`, a breaking change bumps the minor.

Every entry below was reconstructed from git history on 2026-08-24, so they
record what shipped rather than what was written down at the time. The first
tag is v0.2.0; everything before it is folded into that entry.

## [Unreleased]

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

[Unreleased]: https://github.com/FacileStudio/nuage-cli/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/FacileStudio/nuage-cli/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/FacileStudio/nuage-cli/releases/tag/v0.2.0
