# Installation

The easiest way to install xws on a supported macOS or Linux system is with
the installation script. It downloads the latest matching release and
verifies its SHA-256 checksum before installing:

```sh
curl -fsSL https://raw.githubusercontent.com/pre-cursor/xws/master/xws-install \
  | sh -s -- install --latest
```

Releases currently support Apple silicon and Intel Macs, and x86-64 Linux.
xws is installed under `$HOME/xws` by default. Set `XWS_INSTALL_PREFIX`
before running the command to choose a different location; export it so it is
available to the installer and later shells:

```sh
export XWS_INSTALL_PREFIX="$HOME/.local/xws"
```

The installer does not modify shell startup files automatically. To load xws
in future Bash sessions, run:

```sh
curl -fsSL https://raw.githubusercontent.com/pre-cursor/xws/master/xws-install \
  | sh -s -- init bash
```

For Zsh, run:

```sh
curl -fsSL https://raw.githubusercontent.com/pre-cursor/xws/master/xws-install \
  | sh -s -- init zsh
```

These commands add one marked, idempotent block to `~/.bashrc` or `~/.zshrc`;
use `--file FILE` to choose another startup file. Uninstall with:

```sh
curl -fsSL https://raw.githubusercontent.com/pre-cursor/xws/master/xws-install \
  | sh -s -- uninstall
```

The installer is a root-level distribution script and is not included in the
installed product files. Uninstall leaves the startup block in place; remove
that block manually if you no longer want it.

## Troubleshooting

If `xws` is not available after initialization, open a new terminal and try
again. Or reload the startup file for your current shell:

```sh
# Bash
source ~/.bashrc

# Zsh
source ~/.zshrc
```

Confirm that the shell function is loaded with `type xws`. If you selected a
custom startup file with `--file`, source that file instead. Also make sure
you initialized the shell you are currently using.

For a step-by-step introduction, see [Getting Started](GETTING_STARTED.md).
