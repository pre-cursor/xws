# Source in a shell with the legacy ws and installed xws functions available.
# Build the migration tool with:
#   cargo build --manifest-path migrations/workspace/Cargo.toml

xws_migrate() {
  (
    local interactive
    local name exclude bin_path migration_dir binary workspace_list target_dir
    local workspace_root workspaces_prefix
    local -a arguments exclude_names
    interactive=0
    arguments=()
    exclude_names=()

    while (($#)); do
      case "$1" in
        --interactive)
          interactive=1
          shift
          ;;
        --exclude)
          if (($# < 2)) || [[ -z "$2" || "$2" == -* ]]; then
            printf 'usage: xws_migrate [--interactive] [--exclude NAME]...\n' >&2
            return 2
          fi
          exclude_names+=("$2")
          shift 2
          ;;
        -h|--help)
          printf 'Usage: xws_migrate [--interactive] [--exclude NAME]...\n'
          printf 'Batch confirmation is the default; --interactive prompts for each workspace.\n'
          return 0
          ;;
        *)
          printf 'xws_migrate: unknown option: %s\n' "$1" >&2
          return 2
          ;;
      esac
    done

    if ! command -v ws >/dev/null 2>&1; then
      printf 'xws_migrate: legacy ws command is unavailable\n' >&2
      return 1
    fi
    if ! workspace_list="$(ws list)"; then
      printf 'xws_migrate: ws list failed; no workspaces were changed\n' >&2
      return 1
    fi
    name=
    while IFS= read -r name; do
      [[ -n "$name" ]] && break
    done <<< "$workspace_list"
    if [[ -z "$name" ]]; then
      printf 'xws_migrate: ws reports no workspaces to migrate\n'
      return 0
    fi
    if [[ "$name" == *[[:space:]]* || "$name" == */* || "$name" == "." || "$name" == ".." ]]; then
      printf 'xws_migrate: unsupported workspace name from ws list: %s\n' "$name" >&2
      return 1
    fi
    if ! bin_path="$(ws which "$name")"; then
      printf 'xws_migrate: ws which failed while locating the workspaces directory\n' >&2
      return 1
    fi
    if [[ "$bin_path" == *$'\n'* || "$bin_path" != */bin ]]; then
      printf 'xws_migrate: ws which must return a path ending in bin; got %s\n' \
        "$bin_path" >&2
      return 1
    fi
    workspace_root="${bin_path%/bin}"
    workspaces_prefix="${workspace_root%/*}"
    if [[ -z "$workspaces_prefix" || ! -d "$workspaces_prefix" ]]; then
      printf 'xws_migrate: cannot locate the ws workspaces directory from %s\n' \
        "$bin_path" >&2
      return 1
    fi

    if [[ -n "${ZSH_VERSION-}" ]]; then
      migration_dir="${${(%):-%N}:A:h}"
    else
      migration_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)" || return 1
    fi
    target_dir="${CARGO_TARGET_DIR:-$migration_dir/target}"
    if [[ "$target_dir" != /* ]]; then
      target_dir="$PWD/$target_dir"
    fi
    binary="$target_dir/debug/workspace-migrate"
    [[ -x "$binary" ]] || binary="$target_dir/release/workspace-migrate"
    if [[ ! -x "$binary" ]]; then
      printf "xws_migrate: migration binary not found; build it with cargo build --manifest-path '%s/Cargo.toml'\n" \
        "$migration_dir" >&2
      return 127
    fi
    if [[ -z "${_XWS_ROOT-}" || ! -f "$_XWS_ROOT/script/xws.bash" ]]; then
      printf 'xws_migrate: cannot locate the installed xws Bash adapter\n' >&2
      return 1
    fi

    arguments=(--prefix "$workspaces_prefix" --exclude ws)
    ((interactive)) && arguments+=(--interactive)
    for exclude in "${exclude_names[@]}"; do
      arguments+=(--exclude "$exclude")
    done

    XWS_MIGRATE_INIT="$_XWS_ROOT/script/xws.bash" \
      "$binary" "${arguments[@]}"
  )
}
