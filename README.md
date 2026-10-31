# xws

**Workspaces that feel native to your shell.**

`xws` allows developers and DevOps engineers to switch project environments seamlessly without opening nested subshells or altering global system runtimes. By pairing a testable Rust workspace manager and interactive picker with lightweight Bash and Zsh shell functions, `xws` manages environment variables, directory contexts, and tool configurations directly inside your active terminal session.

---

### Overview

Navigating multiple software repositories often requires manually setting environment variables, updating `PATH` entries, and navigating directory trees. **xws** simplifies multi-project workflows by providing a **stacked workspace model**. 

Instead of launching nested subshell processes or modifying global shell configurations, `xws` applies project-specific environment variables and working directory changes natively in your current shell process. When you finish working in a workspace, popping it off the stack restores your previous environment state layer by layer.

---

### Key Features

* **Native Shell Activation** — Directory changes (`cd`), exported environment variables, `PATH` updates, and activation scripts apply directly within your active Bash or Zsh terminal session.
* **Stacked Workspace Switching** — Switch between projects using a stack model (`xws push <name>`). When you pop off a workspace (`xws pop`), `xws` returns you to your previous working directory and restores your prior shell environment.
* **Declarative Environment Tracking** — Store project variables in simple `bin/env` configuration files. `xws` tracks the original set or unset state of declared variables and restores them automatically upon deactivation.
* **Interactive Terminal Picker** — Run `xws` without arguments to open an interactive, scrollable terminal UI to view and select workspaces. Supports both `alternate` screen buffers and `inline` terminal output.
* **Reusable Profiles & Features** — Bootstrap workspaces with built-in profiles (such as **Python** virtual environments, **Rust** debugging flags, or **Java** JDK path discovery) and add optional features like **Golang** tooling.
* **User-Owned Workspace Hooks** — Customize workspace initialization and teardown via executable shell scripts (`activate`, `deactivate`, `once`) copied into your workspace at creation time.
* **Isolated Data Footprint** — Workspace templates and files live cleanly under `$HOME/xws/workspaces`, while session stack states and configurations default to `$HOME/.xws`.

---

### Supported Platforms

`xws` supports **macOS** (Intel and Apple silicon) and **Linux** (x86-64) operating systems running **Bash** or **Zsh** terminal shells.

---

### Quick Start

#### 1. Download and Install

Install the lightweight binary distribution using the automated installation script:

```bash
curl -fsSL https://raw.githubusercontent.com/pre-cursor/xws/master/xws-install | sh -s -- install --latest
```

*(By default, files are installed under `$HOME/xws`. Set `XWS_INSTALL_PREFIX` before running the command to select a custom installation directory.)*

#### 2. Initialize Shell Integration

Add `xws` integration to your active shell startup configuration:

```bash
# For Bash
curl -fsSL https://raw.githubusercontent.com/pre-cursor/xws/master/xws-install | sh -s -- init bash

# For Zsh
curl -fsSL https://raw.githubusercontent.com/pre-cursor/xws/master/xws-install | sh -s -- init zsh
```

Restart your terminal or reload your shell startup file (`source ~/.bashrc` or `source ~/.zshrc`) to complete setup.

#### 3. Create and Activate a Workspace

Create workspaces for your repositories and activate one in your active terminal:

```bash
# Create new workspaces
xws create api
xws create web

# Push onto the "api" workspace
xws push api
```

Activating a workspace changes your current working directory to the workspace source path, prepends its `bin/` directory to your `PATH`, exports declared environment variables, and executes activation hooks.

#### 4. Stack Workspaces and Return Context

To jump to another project while retaining your current place, push another workspace:

```bash
# Push onto the "web" workspace while inside "api"
xws push web

# Deactivate "web" and return to "api"
xws pop

# Deactivate "api" and return to where you started
xws pop
```

---

### Command Reference

| Command | Description |
| :--- | :--- |
| `xws` | Launch the interactive terminal workspace picker. |
| `xws create [--profile NAME] [--with-feature NAME] NAME` | Create a new workspace with optional baseline profiles or features. |
| `xws push NAME` *(aliases: `activate`, `use`)* | Activate a workspace in the current shell and push a frame onto the stack. |
| `xws pop` *(alias: `deactivate`)* | Deactivate the top workspace frame and restore the prior environment. |
| `xws ls` *(alias: `list`)* | Print a sorted list of available workspaces (marks active workspace with `*`). |
| `xws profiles` | List available workspace baseline setup profiles. |
| `xws features` | List available optional feature add-ons. |
| `xws which NAME` | Print the filesystem path to a workspace's `bin/` directory. |
| `xws mv SOURCE TARGET` | Rename a workspace (refuses execution on active workspaces). |
| `xws rm NAME...` | Remove one or more workspaces. |
| `xws version` | Display the installed `xws` version. |

---

### Configuration

`xws` behavior can be customized using environment variables or configuration files.

#### Environment Variables

| Variable | Description |
| :--- | :--- |
| `XWS_PREFIX` | Overrides workspace data directory prefix (workspaces stored under its `workspaces/` sub-directory). |
| `XWS_STATE_HOME` | Overrides private session-state and configuration storage location (defaults to `~/.xws`). |
| `XWS_PICKER` | Customizes path to the `xws-picker` binary executable. |
| `XWS_PICKER_DISPLAY` | Temporarily overrides picker display mode (`inline` or `alternate`). |

#### Terminal Picker Display Mode

The interactive terminal picker uses the **alternate screen buffer** by default. To set a persistent display preference, create or edit `~/.xws/config.toml`:

```toml
screen_buffer = "inline" # or "alternate"
```

For temporary per-session or single-invocation overrides, export `XWS_PICKER_DISPLAY` or set it inline:

```bash
# Export for current shell session
export XWS_PICKER_DISPLAY=inline

# Override for a single invocation
XWS_PICKER_DISPLAY=inline xws
```

*(Note: `XWS_PICKER_DISPLAY` must be exported or set inline because `xws-picker` runs as a separate subprocess and cannot observe unexported shell variables.)*

---

### Architecture & Security

`xws` separates execution logic into two distinct components:

1. **Rust CLI Backend (`xws-picker`)**: Handles workspace directory creation, template resolution, session file updates, and rendering the interactive terminal picker UI.
2. **Shell Integration Adapters (`xws.bash` / `xws.zsh`)**: Lightweight shell functions sourced directly into your interactive terminal session.

```
Interactive Bash or Zsh Shell
  └── Sourced xws shell function
        ├── xws-picker CLI (executes filesystem & stack management)
        ├── XWS Shell Protocol v1 (returns machine-readable stdout instructions)
        └── Current Shell Execution (applies cd, export, and hook sourcing)
```

#### Protocol Safety
Communication between the Rust binary and your shell uses **XWS Shell Protocol v1**. The Rust executable outputs line-oriented, tab-separated Base64 records to `stdout`. Shell adapters decode these fields safely as quoted arguments to shell builtins, guaranteeing that protocol data is never evaluated directly as raw shell source (`eval`).

---

### Documentation

For complete specifications, architectural details, and troubleshooting guides:

* [Installation Guide](https://github.com/pre-cursor/xws/blob/master/docs/INSTALLATION.md) — Detailed manual build options and shell configuration.
* [Getting Started Guide](https://github.com/pre-cursor/xws/blob/master/docs/GETTING_STARTED.md) — Step-by-step walkthrough of profile configurations and environment declarations.
* [Design Specification](https://github.com/pre-cursor/xws/blob/master/docs/DESIGN.md) — Full technical specification of stack semantics, IPC protocols, and execution safety contracts.

---

### License

`xws` is open-source software licensed under the **[MIT License](https://github.com/pre-cursor/xws/blob/master/LICENSE)**.
