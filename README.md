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
