# Migrating source workspaces to `xws`

## What a migration means

A migration is an explicit, relatively rare action in an environment. It
changes existing workspace data to fit a different workspace system or a
newer workspace format. It is not routine startup behavior.

Examples include:

- Migrating legacy `ws` workspaces to `xws`.
- Adding Java or Golang hook starters to an existing `xws` workspace.
- Adopting the Python or Rust profile in an existing `xws` workspace.
- Updating an `xws` workspace to use features introduced in a newer xws
  release.
- Migrating workspaces after a major xws release changes compatibility.

This tool migrates source trees into newly-created xws workspaces. It supports
legacy `ws` workspaces through a thin shell wrapper, and can also migrate
unmanaged project directories such as extracted archives.

## Source-prefix contract

The Rust migration engine is generic: it accepts a source prefix and
discovers each immediate child directory whose `src/` is a directory. The
required layout is:

```text
SOURCE_PREFIX/
  project-one/
    src/
      ...
  project-two/
    src/
      ...
```

The directory name becomes the xws workspace name. Child directories without
`src/` are ignored. `--exclude <name>` can be repeated to exclude selected
children. There is no generic special case for a directory named `ws`.

For legacy `ws`, `migrate.sh` is a thin adapter: it runs `ws list`, calls
`ws which` for one listed workspace to locate the legacy `ws/workspaces`
directory, then passes that directory as the source prefix to the generic
Rust engine. It does not require legacy `bin/` or `bin/activate` files, and
does not call `ws use` to validate or activate projects. The adapter
explicitly adds `--exclude ws`, so the legacy `ws` implementation workspace
is not migrated. Other source prefixes can be passed directly to the Rust
engine without loading or invoking `ws`.

The engine inspects each candidate's `src/`, detects project types at
immediate checkout roots, creates an xws workspace with matching profiles and
features, and copies source entries. A `src/` directory may contain multiple
checkouts and direct files.

The complete inventory and planned destination/options are shown before
modification. Existing xws workspaces and source-entry collisions are not
silently overwritten. Only contents under the source `src/` are transferred;
other source-directory metadata and state are left alone.

The preview reports the estimated source size for each workspace and the
combined total. A workspace estimated at 10 GiB or more is listed under
`Large migration(s) detected` before the normal batch confirmation. The
estimate sums file and symbolic-link lengths from filesystem metadata; it
does not read file contents. It represents logical source bytes, not a
guarantee of exact destination disk allocation, which can differ due to sparse
files, filesystem allocation, or metadata overhead.

## Initial project-type indicators

Detection is based on files in a workspace's source tree. The first version
should use these explicit indicators:

| Project type | Indicator | xws creation setting |
|---|---|---|
| Java | `pom.xml` (Maven), or a `Makefile` that invokes `javac` | `--profile java` |
| Golang | `go.mod`, or a `Makefile` that invokes `go` | `--with-feature golang` |
| Python | `pyproject.toml`, `requirements.txt`, or `setup.cfg` | `--profile python` |
| Rust | `Cargo.toml` | `--profile rust` |

The markers are checked at each checkout root, not recursively through all of
its descendants. Use the Makefile checks only when a project has a `Makefile`;
a Makefile by itself is not enough to classify a project as Java or Golang.
Detection should be explainable: the inventory should show which checkout and
indicator caused each classification. Additional indicators can be added
later with examples and tests to avoid guessing based on ambiguous files.

All detected languages contribute their matching Java/Python/Rust profiles
and Golang feature starter. The create interface accepts repeatable
`--profile` options, allowing detected language profiles to compose. Profile
activation hooks run in profile-name order and
deactivation hooks in reverse order; creation-time `once` setup runs for every
selected profile. Profile hooks coexist with the default workspace hooks.
Project-type detection must not depend on whether a matching runtime happens
to be installed on the migration host.

The Python profile currently creates `.venv` during workspace creation and
therefore requires `python3` at migration time. The Java profile checks for a
JDK at activation time; when none is available, it warns that Java environment
setup was skipped and allows workspace activation to succeed. Missing Java
does not cause the migration tool to misclassify a Java project as non-Java.

## Copy semantics and safety

The operation copies immediate `src/` entries and never removes or modifies
the source entries. This works even when source and destination are on
different filesystems. `rsync` is required; the tool checks for it before
asking for confirmation or changing any workspace. It copies recursively,
preserves symlinks, permissions, and modification times, and does not copy
ownership or special files. Unsupported special files cause the migration to
stop with an error.

Rsync copies the contents of each source `src/` into a migration-owned
staging directory inside the new xws workspace. The transfer uses partial,
in-place updates in staging so rsync can reuse incomplete data on retry. Its
`--delete` is scoped to the migration's staging payload, so that payload
reflects the current source; it cannot delete source files or xws entries
outside staging. Once rsync succeeds, staged entries are published to the new
workspace. If interrupted during transfer, retry rsyncs from the source into
the retained staging data. If interrupted while publishing, already-published
entries are accepted only when they still match the source, and remaining
entries can be published on retry.

Destination collisions are never overwritten. A batch of multiple workspaces
is not one atomic transaction, so an interruption can leave some xws
destinations completed and others pending; the legacy workspaces remain
intact throughout. Before copying, verify that the destination xws workspace
was created successfully, its `src/` exists, and destination names do not
collide with source entries.

On later runs, an existing xws workspace without a migration marker is
reported as already migrated and skipped only if its complete `src/` entry
set matches the source exactly. An unmarked destination that is partial,
different, or empty is treated as a collision and is not modified. A
destination with the migration marker is considered an interrupted migration
and can be resumed. In the completed-migration comparison only, Git repository
`.git/config` files may differ by `branch.*.vscode-merge-base` entries written
by VS Code. Other differences in those files still count as collisions; the
exception does not alter either config file or change copy/resume comparisons.

## Migration interface

Build and test the migration-local Rust tool:

```sh
cargo build --manifest-path migrations/workspace/Cargo.toml
cargo test --manifest-path migrations/workspace/Cargo.toml
```

To migrate a generic source prefix directly:

```sh
cargo run --manifest-path migrations/workspace/Cargo.toml -- \
  --prefix /path/to/source-prefix [--exclude <name>]...
```

In a shell where the legacy `ws` command and installed `xws` shell function
are available, source `migrations/workspace/migrate.sh` and run the
legacy-ws adapter:

```sh
source migrations/workspace/migrate.sh
xws_migrate [--interactive] [--exclude <name>]...
```

Batch mode is the default: the complete inventory is previewed, then one
confirmation applies to all included workspaces. `--interactive` previews the
same inventory and prompts for each workspace separately. `--exclude <name>`
is repeatable in both modes. The legacy `ws` adapter excludes the workspace
named `ws`; generic prefix migration does not.

## Not included in the first migration

Migration does not convert arbitrary source hooks, workspace environment
variables, activation stacks, or old `bin/` contents. Those are
behavior-specific and need an explicit compatibility policy. Likewise,
detecting a project type does not install its language runtime or dependencies;
xws profiles and features only establish workspace-local starter behavior.
