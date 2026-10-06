# lazybrew

A [lazygit]-inspired terminal UI for Homebrew, written in Rust with [ratatui].

Browse installed formulae and casks, see what's outdated, search, and manage
packages — all without leaving the terminal.

## Features

- Lazygit-style layout: sidebar sections, package list, details pane, live output pane
- Sections: Installed, Outdated, Casks, Leaves
- Live search with `/`
- Install / upgrade / remove / `brew update` with confirmation dialogs and streamed output
- Action menu (`x`): info, deps, pin/unpin
- Brewfile export via `brew bundle dump`
- Help overlay (`?`), animated loading + command spinners

## Keybindings

| Key | Action |
|-----|--------|
| `j`/`k`, arrows | navigate |
| `h`/`l`, tab | switch panel |
| `/` | search |
| `esc` | clear search / close |
| `g`/`G` | top / bottom |
| `i` | install package |
| `u` / `r` | upgrade / remove selected |
| `U` | `brew update` |
| `x` | action menu |
| `e` | export Brewfile to `~/Brewfile` |
| `?` | help |
| `q` | quit |

## Install

```sh
cargo install --path .
```

## Develop

```sh
cargo build
cargo test
cargo clippy
cargo fmt
```

## License

MIT

[lazygit]: https://github.com/jesseduffield/lazygit
[ratatui]: https://ratatui.rs
