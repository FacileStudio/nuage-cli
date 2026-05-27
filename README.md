# nuage

File sync daemon for [Nuage](https://github.com/FacileStudio/Nuage). Keeps a local directory in bidirectional sync with your Nuage server.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/FacileStudio/nuage-cli/main/install.sh | bash
```

### Update

```sh
nuage upgrade
```

## Setup

```sh
nuage login
```

## Usage

```sh
nuage          # start daemon (watch + sync)
nuage sync     # one-time sync
nuage status   # show sync status
```

## AI agent integration

`install.sh` auto-registers nuage as an AI agent skill for Claude Code and Codex.
After installation, AI coding assistants can use nuage commands directly when you ask about file sync, uploads, or shares.
