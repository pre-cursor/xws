# Getting started with xws

This guide walks through creating workspaces, switching between them,
managing environment variables, and adding optional hook features. For
installation, see [INSTALLATION.md](INSTALLATION.md).

## 1. Build and connect your shell

From the repository root:

```sh
cargo build --manifest-path picker/Cargo.toml
```

In Bash, source:

```sh
source "$PWD/script/xws.bash"
```

In Zsh, source:

```zsh
source "$PWD/script/xws.zsh"
```

The function is available in this shell. To load it automatically in future
shells, use the hosted installer command in [INSTALLATION.md](INSTALLATION.md).
This explicit command appends one marked block to the selected startup file.

## 2. Create and find workspaces

```sh
xws create api
xws create web
xws ls
```

Workspaces live under `$XWS_PREFIX/workspaces`; by default,
`$XWS_PREFIX` is `$HOME/xws`. Each workspace starts with `src/` and `bin/`.
Use `xws which api` to print its `bin/` directory.

Names may contain spaces:

```sh
xws create "image catalog"
xws ls
```

## 3. Activate in the current shell

```sh
xws push api
pwd
printf '%s\n' "$XWS_WORKSPACE"
```

Activation changes this shell's directory to `api/src`, prepends
`api/bin` to `PATH`, sets `XWS_WORKSPACE` and `XWS_WORKSPACE_PATH`, applies
declared environment variables, and sources the workspace hooks. It does not
launch a child shell.

To pick from the interactive list instead:

```sh
xws
```

Use Up/Down (or `k`/`j`) and Enter. Escape, `q`, or Ctrl-C cancels without
activating a workspace.

### Choose the picker display

By default, the picker uses the alternate screen: it takes over the terminal
while open, then restores the previous screen when it exits. To keep the
picker in the normal terminal history instead, set `screen_buffer` to
`"inline"` in `~/.xws/config.toml`:

```toml
screen_buffer = "inline"
```

Set it to `"alternate"` to explicitly select the default behavior:

```toml
screen_buffer = "alternate"
```

The config file is read from `$XWS_STATE_HOME/config.toml` when
`XWS_STATE_HOME` is set; otherwise, xws uses `~/.xws/config.toml`. Create the
parent directory if needed. For a temporary choice in the current shell,
`XWS_PICKER_DISPLAY` overrides the config:

```sh
XWS_PICKER_DISPLAY=inline xws
XWS_PICKER_DISPLAY=alternate xws
```

## 4. Stack workspaces and return

Start in any directory, then run:

```sh
xws push api
xws push web
```

`web` is now the top activation. Pop it:

```sh
xws pop
```

You return to the directory you were in when you activated `web`, inside
`api`; api's activation hooks run again. Pop once more:

```sh
xws pop
```

Now you return to the directory you were in before activating `api`.
`xws push` of the already-active top workspace is a no-op.

This is a stack of workspace context and declared environment changes, not a
snapshot of every shell feature. Functions, aliases, options, and other
arbitrary hook side effects are not saved automatically.

## 5. Declare tracked environment variables

Create a workspace and edit its `bin/env` file:

```sh
xws create feature-demo
cat > "$(xws which feature-demo)/env" <<'EOF'
APP_MODE=development
API_ENDPOINT=http://localhost:8080
EOF
```

Activate it and inspect the variables:

```sh
xws push feature-demo
printf '%s %s\n' "$APP_MODE" "$API_ENDPOINT"
```

Think of `bin/env` as **configuration** and `activate.d/` as **code**:

| `bin/env` | `bin/activate.d/` |
|---|---|
| Literal `NAME=value` declarations | Bash-compatible scripts sourced into your current shell |
| The Rust CLI reads the declarations; the shell integration applies them | Scripts can run commands and change shell state |
| xws saves the previous state of declared variables in session state and restores it on deactivate | xws does not automatically undo hook side effects; provide cleanup in `deactivate.d/` |

For example, use `bin/env` for a workspace-specific `APP_MODE=development`.
Use an activation hook when setup needs logic, such as finding an installed
JDK or conditionally changing `JAVA_HOME`. Hooks are trusted shell code.

For variables declared in `bin/env`, xws restores each variable's previous
value when you deactivate. If it was unset before activation, it is unset
again. This tracks only variables listed in `bin/env`, not arbitrary changes
made by hooks. Values are literal: `$HOME` is not expanded, and commands are
not executed. Names such as `PATH`, `HOME`, `PWD`, `IFS`, `ZDOTDIR`, and
`XWS_*` are reserved.

If two stacked workspaces declare the same variable, each frame restores the
value from the layer beneath it.

## 6. Choose a Java profile and optional features

Profiles establish a workspace's baseline setup. Features are optional
capabilities you can add to a profile. For example, create a Java workspace:

```sh
xws create --profile java service
```

The Java profile looks for an installed JDK at activation time. On macOS it
uses `/usr/libexec/java_home`; on Linux it resolves `java` with GNU
`readlink -f`. It sets `JAVA_HOME` and adds the JDK's `bin/` to `PATH`. If no
JDK is found, it warns and skips Java setup. xws does not install a JDK.

Use `xws features` to list available add-ons; the `golang` feature is a
non-invasive starter for optional Go-related configuration.

Use `xws profiles` to list the available workspace presets. A profile and its
features are copied into the workspace at creation and can then be edited
there.

Scripts placed directly in `bin/activate.d/` or `bin/deactivate.d/` are
deliberately sourced by xws in lexical filename order. Review scripts before
placing them there; these are trusted shell code. The main `bin/activate` and
`bin/deactivate` hooks also remain editable.

## 7. Try the Python or Rust profile

The Python profile creates a workspace-local `.venv` during creation:

```sh
xws create --profile python python-demo
xws push python-demo
python --version
xws pop
```

It requires `python3` but does not install Python or project dependencies.

The Rust profile adds `RUST_BACKTRACE=1` to the workspace's `bin/env` file:

```sh
xws create --profile rust rust-demo
cat "$(xws which rust-demo)/bin/env"
```

The value is applied when the workspace is activated and restored when it is
deactivated. Edit `bin/env` if you want a different value. The profile does
not change Cargo's target directory or manage Rust toolchains; rustup remains
responsible for toolchain installation and selection.

## 8. Add your own workspace setup

`bin/once` runs in Bash during workspace creation from the workspace root.
Use it for one-time, workspace-local file setup. Its filesystem changes stay;
its environment changes do not affect your interactive shell.

`bin/activate` runs each time the workspace becomes active.
`bin/deactivate` runs before switching away or popping the frame. Pair any
non-declarative changes in these hooks. For example, if an activation hook
starts a tool-specific environment, its deactivation hook should call that
tool's cleanup routine.

Hooks are trusted code. xws reports hook failures but cannot undo arbitrary
external side effects.

## 9. Move and remove workspaces

Deactivate a workspace before moving or removing it:

```sh
xws pop
xws mv api api-v2
xws rm web "image catalog" feature-demo python-demo rust-demo service api-v2
```

Removal is explicit; the picker never deletes entries. Batch removal reports
bad names and continues processing the remaining operands.

## 10. Configure storage

```sh
export XWS_PREFIX="$HOME/projects/xws-data"
export XWS_STATE_HOME="$HOME/.local/state/xws"
```

The state home contains private per-shell stack files under `sessions/`.
Session files may include prior values of variables declared in `bin/env`.
If a shell exits while a workspace is active, its session file may remain;
ordinary successful deactivation removes it when the stack becomes empty.

For the complete behavior and protocol contract, see [DESIGN.md](./DESIGN.md).
