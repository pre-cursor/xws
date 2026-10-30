# xws: workspaces that feel native to your shell

Switch projects without switching shells. `xws` pairs a testable Rust
workspace manager and picker with small Bash/Zsh functions that activate
projects **in the shell you are already using**.

Stack workspaces and pop back through them. Keep project-specific exported
variables in a simple file. Choose a workspace profile and add optional
features when creating it. Your shell startup remains yours, and xws does not
install system-wide language runtimes.

`xws` is a separate experiment from legacy `ws`; it uses its own data and
state directories.

## Install

Install xws by following the [installation guide](docs/INSTALLATION.md), then
try the [Getting Started guide](docs/GETTING_STARTED.md). For source builds and
validation, see [CONTRIBUTING.md](CONTRIBUTING.md).

The full architecture, stack semantics, protocol, environment file, hooks, and
safety contract are in [DESIGN](docs/DESIGN.md).

## Highlights

- **Native shell activation:** `cd`, exported variables, PATH, and hooks are
  applied in the current Bash/Zsh.
- **Stacked workspace switching:** `xws push bar` while in `foo` lets
  `xws pop` return to foo; pop again to return to where you
  started.
- **Tracked environment variables:** `bin/env` is declarative configuration.
  The Rust CLI reads it, and the shell integration applies the values and
  restores their prior set or unset state as the stack pops.
- **Editable hooks:** workspace hooks are copied at creation, then remain
  yours to change. `activate.d/` scripts are sourced shell code; xws does not
  automatically undo their side effects, so provide matching cleanup in
  `deactivate.d/`.
- **Profiles and features:** choose a workspace baseline such as Python or
  Java, then add optional tooling features.
- **Scrollable picker:** long lists stay navigable; Enter activates, while
  Escape, `q`, or Ctrl-C cancels.
- **Configurable picker display:** choose an inline picker that preserves
  terminal history or an alternate-screen picker that restores the prior
  screen on exit.
- **Isolated data:** workspaces live under `$HOME/xws/workspaces` by default;
  private session state defaults to `$HOME/.xws/sessions`, with picker
  settings in `$HOME/.xws/config.toml`.

## Commands

```text
xws
xws create [--profile NAME]... [--with-feature NAME]... NAME
xws profiles
xws features
xws push|activate|use NAME
xws pop|deactivate
xws ls|list
xws which NAME
xws mv|move SOURCE TARGET
xws rm|remove NAME...
xws version|--version
```

Set `XWS_PREFIX` to change the workspace data prefix (workspaces are stored
under its `workspaces/` directory), `XWS_STATE_HOME` to change private
session-state and configuration storage, or `XWS_PICKER` to select the picker
executable.

The picker uses the alternate screen by default. To choose a display, create
`~/.xws/config.toml`:

```toml
screen_buffer = "inline" # or "alternate"
```

For a temporary override, set `XWS_PICKER_DISPLAY=inline` or
`XWS_PICKER_DISPLAY=alternate`. `xws list` marks the active workspace with
`*`.

## License

This project is licensed under the [MIT license].

[MIT license]: https://github.com/pre-cursor/xws/blob/master/LICENSE

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in Tokio by you shall be licensed as MIT, without any additional
terms or conditions.