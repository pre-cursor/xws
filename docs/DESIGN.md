# xws Design Specification

## Status and scope

`xws` is an experimental workspace manager, separate from legacy `ws`. It
defaults to `$HOME/xws` and neither reads nor migrates legacy workspace data.
The first version supports Bash and Zsh on Unix-like systems.

The design combines:

- A Rust CLI for workspace files, session stacks, and the interactive picker.
- Small sourceable Bash/Zsh functions for actions that must affect the current
  shell (`cd`, exports, and sourcing activation hooks).
- Explicit workspace-local environment declarations and opt-in hook features.

It does not install system-wide language runtimes, rewrite shell startup files,
or snapshot/restore arbitrary shell state.

## Goals

1. Activation changes the caller's shell; it never creates a nested shell.
2. Workspace operations and session persistence are testable Rust code.
3. Switching workspaces is stack-based: deactivating the top returns to and
   reactivates the previous workspace; deactivating the last returns to the
   directory from before the first activation.
4. xws restores declared exported variables it tracks, but does not claim to
   snapshot functions, aliases, options, traps, jobs, or side effects.
5. Workspace-local hooks are user-owned and never regenerated after creation.
6. Executable feature hooks are installed only when explicitly selected or
   deliberately added to the workspace hook directories.
7. Protocol data is parsed as data; the shell integration never `eval`s CLI
   output.

## Architecture

```text
interactive Bash or Zsh
  └── sourced xws function
        ├── xws-picker filesystem and session commands
        ├── xws-picker picker (protocol response on stdout, UI on stderr)
        └── caller-shell cd/export/source actions
```

The Rust process cannot change the parent shell's working directory or
variables. It returns paths, environment records, and stack frames; the shell
adapter validates those records and performs the changes in the current
process.

## Filesystem layout

```text
$XWS_PREFIX/
  workspaces/
    <name>/
      src/
      bin/
        activate
        deactivate
        once
        env
        activate.d/
        deactivate.d/
  templates/
    profiles/<profile>/...
    features/<feature>/{manifest.toml,activate,deactivate,once,env}

$XWS_STATE_HOME/
  sessions/
    <session-id>.json
```

`XWS_PREFIX` defaults to `$HOME/xws`, so workspaces live under
`$HOME/xws/workspaces`; `XWS_STATE_HOME` defaults to `$HOME/.xws`.
Session files live in the `sessions/` subdirectory, use private permissions,
and are removed when the shell pops its last activation frame. A session ID is
kept in an unexported shell variable and passed to the Rust CLI explicitly.
Each session file records the PID of the owning shell (`session new
--shell-pid`, which the wrapper fills with `$$`). If a shell exits while a
workspace is still active, its file remains until the next `xws session new`,
which deletes session files that are stale: created before the last boot
(`kern.boottime` on macOS, `btime` in `/proc/stat` on Linux), or owned by a
shell PID that no longer exists. Cleanup is best-effort and errors are ignored.
Files from older versions have no recorded PID and are removed only by the boot
rule. `xws remove` and `xws move` also scan live sessions and refuse to touch a
workspace that is active in another shell, with a message that names the shell
and how to resolve it.

An installed xws distribution uses this layout, where `XWS_INSTALL_PREFIX`
defaults to `$HOME/xws`:

```text
$XWS_INSTALL_PREFIX/
  bin/xws-picker
  share/xws/
    script/
      xws-common.sh
      xws.bash
      xws.zsh
    templates/
      profiles/<profile>/...
      features/<feature>/{manifest.toml,activate,deactivate,once,env}
```

The CLI locates packaged templates relative to its executable in an installed
tree and falls back to the source checkout's templates during development.
Shell adapters similarly locate the installed CLI relative to their shared
script directory, while retaining the source-tree build paths for development.
`XWS_INSTALL_PREFIX` locates installed program files; `XWS_PREFIX` continues
to identify workspace data and user template overrides. By default both use
`$HOME/xws`, but their contents do not overlap: installed files are under
`bin/` and `share/xws/`, while workspaces and user templates are under
`workspaces/` and `templates/`.

The root-level `xws-install` distribution script accepts
`XWS_INSTALL_PREFIX` or `--prefix DIR` (the flag wins), and can install a
published release, local build, or prepared artifact tree. The script itself
is not part of the installed product. The install manifest tracks product
files so upgrades replace stale managed files and uninstall preserves
unrelated files. Startup integration is opt-in: `xws-install init bash|zsh`
appends one marked, idempotent block to the selected startup file.

Workspace names may contain spaces and punctuation, but cannot be empty, `.` or
`..`, start with `-` or `.`, contain `/` or `:`, or contain control characters.
A leading `-` would be read as an option, `:` would corrupt `PATH` when the
workspace `bin` directory is prepended, and newline is excluded because
workspace listing and picker output are line-oriented. Directories that already
have such names are skipped by `xws ls` with a warning; rename them by hand.

## Activation stack and tracked environment

Each frame records the workspace name/path, the directory to return to, and
the prior values (including unset-vs-set state) for variables declared by that
workspace. The session file stores only those declared variable values, not a
snapshot of the complete shell.

Example:

1. Starting in `/work`, `xws push foo` pushes a frame that returns to `/work`.
2. `xws push bar` deactivates foo's hooks, pushes a frame that returns to the
   current directory in foo, and activates bar.
3. `xws pop` deactivates bar, restores the variables bar overrode,
   returns to foo's saved directory, and runs foo's activation hooks again.
4. A second `xws pop` deactivates foo, restores its prior variables,
   and returns to `/work`.

This is layered activation, not a snapshot of arbitrary shell state. Declared
variables from foo remain as the base environment while bar is active. Hooks
must pair their own non-declarative side effects. On a pop, the prior
workspace's activation hooks run again, so those effects are reconstructed
according to the hooks.

### `bin/env`

`bin/env` is a literal, line-oriented `NAME=value` file. Blank lines and lines
beginning with `#` are ignored. Values are exported when the workspace is
activated; no shell expansion, command substitution, or quoting syntax is
performed. Empty values are supported. Values cannot contain control
characters or span lines. Duplicate names are errors.

On each activation, xws saves the current value and set/exported status for
every declared name in the session frame. Deactivation restores that prior
state, including whether the variable was exported, or unsets it if it was
previously unset. The same variable can be declared by multiple stacked
workspaces and is restored layer by layer.

Variable names use the portable shell identifier syntax. `PATH`, `XWS_*`, and
shell-control variables such as `IFS`, `HOME`, `PWD`, and `ZDOTDIR` are
reserved, as are variables that run code at prompt time or alter the dynamic
loader: `PROMPT_COMMAND`, `PS0`, `PS1`, `PS2`, `PS4`, `PYTHONSTARTUP`,
`HISTFILE`, and the `LD_*`, `DYLD_*`, and `BASH_FUNC_*` families. The `_XWS_*`
and `_xws_*` prefixes are reserved for integration internals. xws manages each
workspace's `bin/` PATH entry itself. Toolchain and language variables such as
`CC`, `PYTHONPATH`, or `GOPATH` remain allowed. The same check is applied to
names read back from session files. It restricts only the declarative `bin/env`
channel; `activate` and `deactivate` hooks are shell scripts and are not
covered, so it is hardening and not a sandbox.

Session state may contain sensitive prior environment values. Keep
`XWS_STATE_HOME` in a private location and do not share session files.

## Shell/CLI protocol

Shell-facing machine output uses **XWS Shell Protocol v1**:

- UTF-8, one record per line, tab-separated fields.
- Every record starts with `XWS/1`.
- Arbitrary string fields are standard Base64 encoded (`-` represents an
  empty field); record tags and validated variable names are plain text.
- CLI diagnostics go to stderr. Protocol output goes to stdout.
- Exit status indicates success/failure; picker cancellation is a successful
  `cancel` record.
- Unknown versions, malformed records, invalid variable names, and unknown
  action/record types are errors.
- Shell adapters decode fields and pass them to builtins as quoted arguments.
  They never evaluate protocol data as shell source.

Examples:

```text
XWS/1<TAB>path<TAB><base64-workspace-path>
XWS/1<TAB>var<TAB>TOOL_MODE<TAB><base64-value>
XWS/1<TAB>session<TAB><session-id>
XWS/1<TAB>selected<TAB><base64-workspace-name>
XWS/1<TAB>cancel
```

Session pushes accept `var` records on stdin, recording prior values. Pops
return the popped frame, prior variable records, and the parent frame (if any).
This allows future shell adapters to implement the same semantics without
requiring identical parsing code. Bash and Zsh are the only adapters currently
supported; adding another shell requires its own parser and conformance tests.

User-facing commands such as `xws ls` retain readable output rather than
forcing all terminal output into the wire format.

## Hooks and features

`bin/activate` and `bin/deactivate` are trusted Bash-compatible shell scripts
sourced into the current Bash/Zsh. They can change arbitrary shell state.
`bin/once` runs in a non-interactive Bash subprocess from the workspace root;
its filesystem changes persist, but its process environment does not leak into
the current shell.

Activation order is the main `bin/activate` followed by regular files in
`bin/activate.d/` in lexical order. Deactivation runs regular files in
`bin/deactivate.d/` in lexical order, then `bin/deactivate`. The directories
are explicitly documented execution points: placing a script there means it
will be sourced in the current shell. xws does not scan other files or
filename patterns.

Profiles are workspace presets: each establishes a baseline environment.
Features are optional capabilities that can be added to a profile. For
example, a language profile can be combined with optional tooling features.
This distinction is user-facing and is reflected in the template layout:
profiles are in `templates/profiles/<name>/`, and features are in
`templates/features/<name>/`.

The CLI discovers available profiles and features by listing those directories.
`xws profiles` and `xws features` show the discovered names. Packaged templates
are installed under `share/xws/templates/`. The packaged location is resolved
from `XWS_TEMPLATES_DIR` (used as-is, and an invalid value is an error), then
`<exe>/../share/xws/templates`, then, in debug builds only, the checkout's
`templates/` directory. Release binaries never embed a source path. Because
packaged templates are copied into workspaces and executed, `XWS_TEMPLATES_DIR`
has the same trust level as `XWS_PREFIX`. User templates under
`$XWS_PREFIX/templates/profiles/<name>/` or
`$XWS_PREFIX/templates/features/<name>/` override matching packaged files.
Overrides are resolved per file, so a user bundle can customize one file and
inherit the other files from the packaged bundle.

Bundles can provide `activate`, `deactivate`, `once`, and `env` files.
Activation/deactivation hooks must be supplied as a pair; `once` scripts run
during workspace creation, and `env` lines are added to the generated
`bin/env`. Each file is optional except the default profile's main
`activate`, `deactivate`, and `once` templates. Selected profiles contribute
environment defaults first, then selected features; duplicate environment
variable declarations are errors. Profile activation hooks run in
profile-name order, followed by feature hooks in feature-name order.
Deactivation runs feature hooks before profiles, with profiles in reverse
order. `once` setup runs the default profile, selected profiles in
profile-name order, then selected features in feature-name order.
Templates are copied at creation; later changes do not rewrite existing
workspaces.

Every feature has a `manifest.toml`. Its optional `with-profiles` array limits
the feature to workspaces that explicitly select at least one named profile;
the listed profiles are alternatives, not requirements to select all of them.
The always-applied `default` profile does not satisfy this restriction. An
omitted `with-profiles` means the feature is generally applicable.

### Built-in profiles and features

- **default profile**: base hooks applied to every workspace.
- **java profile**: locates an installed JDK at activation time, sets
  `JAVA_HOME`, and adds the JDK's `bin/` to `PATH`. If no JDK is found it
  warns and skips setup; it does not install one.
- **python profile**: `once` creates a local `.venv` with `python3 -m venv`;
  activation sources it when present. It does not install Python or project
  dependencies.
- **rust profile**: adds `RUST_BACKTRACE=1` to `bin/env`; it does not change
  Cargo's target directory or manage Rust toolchains.
- **golang feature**: a non-invasive starter hook; configure values such as
  `GOPATH` in `bin/env` or edit the generated hooks.

Language managers remain responsible for installing and selecting toolchains.
Profiles/features may configure existing tools or local project environments,
but do not silently install machine-wide products.

## Commands

| Command | Behavior |
|---|---|
| `xws` | Pick and activate; cancel leaves shell state unchanged |
| `xws create [--profile PROFILE]... [--with-feature NAME]... NAME` | Create without activating |
| `xws profiles` | List available workspace profiles |
| `xws features` | List available optional features |
| `xws push NAME` | Activate in the current shell and push a frame |
| `xws activate NAME`, `xws use NAME` | Aliases for `push` |
| `xws pop` | Deactivate and pop one frame |
| `xws deactivate` | Alias for `pop` |
| `xws ls`, `xws list` | Print sorted names |
| `xws which NAME` | Print workspace `bin/` path |
| `xws mv SOURCE TARGET` | Rename; refuses the active workspace |
| `xws rm NAME...` | Remove; refuses active workspace and continues after operand errors |
| `xws version`, `xws --version` | Print the xws version |
| `xws help` | Print help |

`xws push` of the already-active top workspace is a no-op. Pushing runs the
outgoing deactivation hooks before the incoming activation hooks. Popping
reactivates the parent workspace. `xws pop` does not pop if a
deactivation hook fails.

## Picker

- Highlights the active workspace when present; otherwise starts unselected.
- First Up selects the first entry; first Down selects the last.
- Arrow keys and `k`/`j` move one row, clamp at endpoints, and do not wrap.
- Enter selects; Escape, `q`, or Ctrl-C cancels.
- Uses a scrollable viewport with selection-aware one-third/two-thirds
  scrolling and reserves two extra terminal rows to avoid drawing at the
  reported lower edge.
- Terminal UI is written to stderr; protocol selection/cancel goes to stdout.
- Terminal raw mode and cursor state are restored through an RAII guard.

Removal is explicit; the picker never deletes a workspace.

## Safety and errors

- Workspace names are validated; symlink workspace entries are not accepted.
- `rm`/`mv` refuse the active workspace through the shell integration.
- Hook files are executable behavior and are trusted user code. Explicit
  feature selection and documented hook directories avoid accidental
  filename-based execution.
- CLI errors are explicit on stderr and nonzero status. Hook failures stop the
  transition and are reported; arbitrary hook side effects cannot be rolled
  back automatically.
- Session writes use private permissions and atomic replacement.
- Session stack state and hook side effects cannot be committed atomically
  together. xws rolls back its own stack/environment changes when activation
  fails where possible, but cannot reverse external hook side effects.

## Validation

From the repository root:

```sh
cargo fmt --manifest-path picker/Cargo.toml --check
cargo test --manifest-path picker/Cargo.toml
bash -n script/xws.bash script/xws-common.sh xws-install
```

Manual Bash/Zsh tests should cover stacked activation, restoration of set and
unset variables, custom hooks/features, picker cancel/Ctrl-C, paths with
spaces, and terminal resizing. See [GETTING_STARTED.md](./GETTING_STARTED.md)
for a guided workflow.
