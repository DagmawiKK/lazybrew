# lazybrew

A [lazygit]-inspired terminal UI for Homebrew, written in Rust with [ratatui].

Browse the full Homebrew catalog, manage installed formulae and casks,
services, taps, and Brewfiles — all without leaving the terminal.

## Features

- Lazygit-style layout: sidebar sections, package table, details pane, output pane
- **Sections**: Installed, Outdated, Casks, Leaves, Catalog, Services, Taps,
  and Brewfile (with `-f`)
- **Catalog**: browse all 15k+ remote formulae and casks (downloaded once a
  day, cached in the XDG cache dir, loaded in the background)
- Live search with `/`, install anything with `i`
- Upgrade / remove / upgrade-all / cleanup / autoremove with confirmation
  dialogs and streamed command output
- **Services**: list and start/stop `brew services`
- **Taps**: list, tap, and untap
- **Brewfile mode**: `lazybrew -f ~/Brewfile` (local path or https URL) with
  install-all / remove-all
- **Vulnerability scan** (`brew vulns`) with cached results shown in details
- Pin/unpin, info, deps via the `x` action menu
- Brewfile export via `brew bundle dump` (`e`)
- **Theme switcher** (`t`): 6 built-in themes — Lazybrew, btop, Dracula,
  Nord, Gruvbox, Solarized — with live preview swatches; the choice is
  persisted to the XDG config dir
- Help overlay (`?`), animated loading and command spinners, pulsing
  busy-state cues, per-line output colorization

## Keybindings

| Key | Action |
|-----|--------|
| `j`/`k`, arrows | navigate |
| `h`/`l`, tab | switch panel |
| `/` | search |
| `esc` | clear search / close |
| `g`/`G` | top / bottom |
| `i` | install the **selected** package (prompt in Taps section; reports if already installed) |
| `u` / `r` | upgrade / remove selected (untap in Taps section) |
| `A` | upgrade all outdated |
| `U` | `brew update` |
| `K` | `brew cleanup` |
| `n` | `brew autoremove` |
| `s` | start/stop service (Services section) |
| `v` | vulnerability scan (formulae) |
| `x` | action menu (info/deps/pin) |
| `t` | theme picker (6 built-in themes, persisted) |
| `I` | install by typed name (install-all in Brewfile section) |
| `R` | remove all (Brewfile section) |
| `e` | export Brewfile to `~/Brewfile` |
| `?` | help |
| `q` | quit |

## Install

```sh
cargo install --path .
```

## Usage

```sh
lazybrew                    # all installed packages + catalog
lazybrew -f ~/Brewfile      # Brewfile mode (local or https URL)
```

## Develop

```sh
cargo build
cargo test
cargo clippy
cargo fmt
# preview the layout as ASCII:
cargo test print_layout -- --nocapture
```

## License

MIT

[lazygit]: https://github.com/jesseduffield/lazygit
[ratatui]: https://ratatui.rs
