: "${XWS_PREFIX:=$HOME/xws}"
export XWS_PREFIX

_xws_cli() {
  local _xws_local_picker="${XWS_PICKER-}"
  if [[ -z "$_xws_local_picker" ]]; then
    local _xws_local_candidate
    if [[ -f "$_XWS_ROOT/picker/Cargo.toml" ]]; then
      for _xws_local_candidate in \
        "$_XWS_ROOT/picker/target/debug/xws-picker" \
        "$_XWS_ROOT/picker/target/release/xws-picker"; do
        if [[ -x "$_xws_local_candidate" ]]; then
          _xws_local_picker="$_xws_local_candidate"
          break
        fi
      done
    else
      _xws_local_candidate="$_XWS_ROOT/../../bin/xws-picker"
      if [[ -x "$_xws_local_candidate" ]]; then
        _xws_local_picker="$_xws_local_candidate"
      fi
    fi
  fi
  if [[ ! -x "$_xws_local_picker" ]]; then
    if [[ -f "$_XWS_ROOT/picker/Cargo.toml" ]]; then
      printf "xws: picker executable not found; build it with cargo build --manifest-path '%s/picker/Cargo.toml'\n" "$_XWS_ROOT" >&2
    else
      printf 'xws: picker executable not found; reinstall xws or set XWS_PICKER\n' >&2
    fi
    return 127
  fi
  "$_xws_local_picker" "$@"
}

_xws_b64decode() {
  if [[ "$1" == "-" ]]; then
    return 0
  fi
  if printf '' | base64 --decode >/dev/null 2>&1; then
    printf '%s' "$1" | base64 --decode
  else
    printf '%s' "$1" | base64 -D
  fi
}

_xws_b64decode_capture() {
  _XWS_DECODED="$(_xws_b64decode "$1" && printf '\001')" || return $?
  _XWS_DECODED="${_XWS_DECODED%$'\001'}"
}

_xws_path_prepend() {
  local _xws_local_entry="$1"
  case ":${PATH-}:" in
    *":${_xws_local_entry}:"*) ;;
    *) PATH="${_xws_local_entry}${PATH:+:${PATH}}"; export PATH ;;
  esac
}

_xws_path_remove() {
  local _xws_local_entry="$1"
  local _xws_local_remaining="${PATH-}"
  local _xws_local_result=""
  local _xws_local_component
  local _xws_local_first=1
  while [[ "$_xws_local_remaining" == *:* ]]; do
    _xws_local_component="${_xws_local_remaining%%:*}"
    _xws_local_remaining="${_xws_local_remaining#*:}"
    if [[ "$_xws_local_component" != "$_xws_local_entry" ]]; then
      if [[ "$_xws_local_first" == 1 ]]; then
        _xws_local_result="$_xws_local_component"
        _xws_local_first=0
      else
        _xws_local_result="${_xws_local_result}:${_xws_local_component}"
      fi
    fi
  done
  _xws_local_component="$_xws_local_remaining"
  if [[ "$_xws_local_component" != "$_xws_local_entry" ]]; then
    if [[ "$_xws_local_first" == 1 ]]; then
      _xws_local_result="$_xws_local_component"
    else
      _xws_local_result="${_xws_local_result}:${_xws_local_component}"
    fi
  fi
  PATH="$_xws_local_result"
  export PATH
}

_xws_ensure_session() {
  [[ -n "${_XWS_SESSION_ID-}" ]] && return 0
  local _xws_local_response _xws_local_version _xws_local_kind _xws_local_session_id
  _xws_local_response="$(_xws_cli session new)" || return $?
  IFS=$'\t' read -r _xws_local_version _xws_local_kind _xws_local_session_id <<< "$_xws_local_response"
  if [[ "$_xws_local_version" != "XWS/1" || "$_xws_local_kind" != "session" || -z "$_xws_local_session_id" ]]; then
    printf 'xws: invalid session response from picker\n' >&2
    return 1
  fi
  _XWS_SESSION_ID="$_xws_local_session_id"
}

_xws_run_activate_hooks() {
  local _xws_local_workspace_path="$1"
  local _xws_local_hook
  if [[ -n "${ZSH_VERSION-}" ]]; then
    setopt local_options nonomatch
  fi
  source "$_xws_local_workspace_path/bin/activate" || return 1
  for _xws_local_hook in "$_xws_local_workspace_path"/bin/activate.d/*; do
    [[ -f "$_xws_local_hook" ]] || continue
    source "$_xws_local_hook" || {
      printf 'xws: activation hook failed: %s\n' "$_xws_local_hook" >&2
      return 1
    }
  done
}

_xws_run_deactivate_hooks() {
  local _xws_local_workspace_path="$1"
  local _xws_local_hook
  if [[ -n "${ZSH_VERSION-}" ]]; then
    setopt local_options nonomatch
  fi
  for _xws_local_hook in "$_xws_local_workspace_path"/bin/deactivate.d/*; do
    [[ -f "$_xws_local_hook" ]] || continue
    source "$_xws_local_hook" || {
      printf 'xws: deactivation hook failed: %s\n' "$_xws_local_hook" >&2
      return 1
    }
  done
  if [[ -f "$_xws_local_workspace_path/bin/deactivate" ]]; then
    source "$_xws_local_workspace_path/bin/deactivate" || {
      printf 'xws: deactivation hook failed for %s\n' "${XWS_WORKSPACE-}" >&2
      return 1
    }
  fi
}

_xws_clear_active() {
  local _xws_local_workspace_path="$1"
  _xws_path_remove "$_xws_local_workspace_path/bin"
  unset XWS_WORKSPACE XWS_WORKSPACE_PATH
}

_xws_load_environment() {
  local _xws_local_workspace="$1"
  local _xws_local_response _xws_local_version _xws_local_record _xws_local_name
  local _xws_local_encoded _xws_local_value _xws_local_previous _xws_local_state
  _xws_local_response="$(_xws_cli protocol env "$_xws_local_workspace")" || return $?
  _XWS_ENV_SNAPSHOTS=""
  _XWS_ENV_VALUES=""
  while IFS=$'\t' read -r _xws_local_version _xws_local_record _xws_local_name _xws_local_encoded; do
    [[ -z "$_xws_local_version" && -z "$_xws_local_record" ]] && continue
    if [[ "$_xws_local_version" != "XWS/1" || "$_xws_local_record" != "var" || -z "$_xws_local_name" ]]; then
      printf 'xws: invalid environment response from picker\n' >&2
      return 1
    fi
    _xws_b64decode_capture "$_xws_local_encoded" || return $?
    _xws_local_value="$_XWS_DECODED"
    if _xws_var_is_set "$_xws_local_name"; then
      _xws_local_previous="$(_xws_var_encode "$_xws_local_name")"
      if _xws_var_is_exported "$_xws_local_name"; then
        _xws_local_state="set-exported"
      else
        _xws_local_state="set-local"
      fi
    else
      _xws_local_previous="-"
      _xws_local_state="unset"
    fi
    _XWS_ENV_SNAPSHOTS="${_XWS_ENV_SNAPSHOTS}XWS/1${_XWS_TAB}var${_XWS_TAB}${_xws_local_name}${_XWS_TAB}${_xws_local_state}${_XWS_TAB}${_xws_local_previous}
"
    _XWS_ENV_VALUES="${_XWS_ENV_VALUES}${_xws_local_name}${_XWS_TAB}${_xws_local_value}
"
  done <<< "$_xws_local_response"
}

_xws_apply_environment() {
  local _xws_local_name _xws_local_value
  while IFS=$'\t' read -r _xws_local_name _xws_local_value; do
    [[ -z "$_xws_local_name" ]] && continue
    _xws_setvar "$_xws_local_name" "$_xws_local_value" || return $?
  done <<< "${_XWS_ENV_VALUES-}"
}

_xws_restore_environment() {
  local _xws_local_name _xws_local_state _xws_local_encoded _xws_local_value
  while IFS=$'\t' read -r _xws_local_name _xws_local_state _xws_local_encoded; do
    [[ -z "$_xws_local_name" ]] && continue
    if [[ "$_xws_local_state" == "set-exported" || "$_xws_local_state" == "set-local" ]]; then
      _xws_b64decode_capture "$_xws_local_encoded" || return $?
      _xws_local_value="$_XWS_DECODED"
      _xws_setvar "$_xws_local_name" "$_xws_local_value" || return $?
      if [[ "$_xws_local_state" == "set-local" ]]; then
        _xws_unexportvar "$_xws_local_name" || return $?
      fi
    elif [[ "$_xws_local_state" == "unset" ]]; then
      _xws_unsetvar "$_xws_local_name" || return $?
    else
      printf 'xws: invalid saved environment state for %s\n' "$_xws_local_name" >&2
      return 1
    fi
  done <<< "${_XWS_POP_VARS-}"
}

_xws_pop_frame() {
  local _xws_local_response _xws_local_version _xws_local_kind _xws_local_a
  local _xws_local_b _xws_local_c
  _xws_local_response="$(_xws_cli session pop "$_XWS_SESSION_ID")" || return $?
  _XWS_POP_VARS=""
  _XWS_POP_PARENT=""
  _XWS_POP_FOUND=0
  _XWS_POP_FINISHED=0
  while IFS=$'\t' read -r _xws_local_version _xws_local_kind _xws_local_a _xws_local_b _xws_local_c; do
    [[ -z "$_xws_local_version" && -z "$_xws_local_kind" && -z "$_xws_local_a" &&
      -z "$_xws_local_b" && -z "$_xws_local_c" ]] && continue
    [[ "$_xws_local_version" == "XWS/1" && "$_XWS_POP_FINISHED" == 0 ]] || {
      printf 'xws: invalid session response from picker\n' >&2
      return 1
    }
    case "$_xws_local_kind" in
      popped)
        [[ "$_XWS_POP_FOUND" == 0 && -n "$_xws_local_a" &&
          -n "$_xws_local_b" && -n "$_xws_local_c" ]] || return 1
        _xws_b64decode_capture "$_xws_local_a" || return $?
        [[ -n "$_XWS_DECODED" ]] || return 1
        _xws_b64decode_capture "$_xws_local_b" || return $?
        [[ -n "$_XWS_DECODED" ]] || return 1
        _xws_b64decode_capture "$_xws_local_c" || return $?
        [[ -n "$_XWS_DECODED" ]] || return 1
        _XWS_RETURN_DIR="$_XWS_DECODED"
        _XWS_POP_FOUND=1
        ;;
      var)
        [[ "$_XWS_POP_FOUND" == 1 ]] || return 1
        [[ -n "$_xws_local_a" && -n "$_xws_local_b" && -n "$_xws_local_c" ]] || return 1
        case "$_xws_local_b" in
          set-exported|set-local)
            _xws_b64decode_capture "$_xws_local_c" || return $?
            ;;
          unset)
            [[ "$_xws_local_c" == "-" ]] || return 1
            ;;
          *)
            return 1
            ;;
        esac
        _XWS_POP_VARS="${_XWS_POP_VARS}${_xws_local_a}${_XWS_TAB}${_xws_local_b}${_XWS_TAB}${_xws_local_c}
"
        ;;
      parent)
        [[ "$_XWS_POP_FOUND" == 1 && -n "$_xws_local_a" &&
          -n "$_xws_local_b" && -z "$_xws_local_c" ]] || return 1
        _xws_b64decode_capture "$_xws_local_a" || return $?
        [[ -n "$_XWS_DECODED" ]] || return 1
        _XWS_POP_PARENT="$_XWS_DECODED"
        _xws_b64decode_capture "$_xws_local_b" || return $?
        [[ -n "$_XWS_DECODED" ]] || return 1
        _XWS_POP_PARENT_PATH="$_XWS_DECODED"
        _XWS_POP_FINISHED=1
        ;;
      empty)
        [[ "$_XWS_POP_FOUND" == 1 && -z "$_xws_local_a" &&
          -z "$_xws_local_b" && -z "$_xws_local_c" ]] || return 1
        _XWS_POP_PARENT=""
        unset _XWS_SESSION_ID
        _XWS_POP_FINISHED=1
        ;;
      *)
        printf 'xws: invalid session response from picker\n' >&2
        return 1
        ;;
    esac
  done <<< "$_xws_local_response"
  [[ "$_XWS_POP_FOUND" == 1 && "$_XWS_POP_FINISHED" == 1 ]] || {
    printf 'xws: incomplete session response from picker\n' >&2
    return 1
  }
  _xws_restore_environment || return $?
}

_xws_restore_parent() {
  local _xws_local_workspace="$1"
  local _xws_local_workspace_path="$2"
  if [[ -n "$_xws_local_workspace" ]]; then
    cd "$_XWS_RETURN_DIR" || {
      printf 'xws: saved return directory no longer exists: %s\n' "$_XWS_RETURN_DIR" >&2
      return 1
    }
    XWS_WORKSPACE="$_xws_local_workspace"
    XWS_WORKSPACE_PATH="$_xws_local_workspace_path"
    export XWS_WORKSPACE XWS_WORKSPACE_PATH
    _xws_path_prepend "$_xws_local_workspace_path/bin"
    _xws_run_activate_hooks "$_xws_local_workspace_path" || return 1
  else
    cd "$_XWS_RETURN_DIR" || {
      printf 'xws: saved return directory no longer exists: %s\n' "$_XWS_RETURN_DIR" >&2
      return 1
    }
  fi
}

_xws_protocol_path() {
  local _xws_local_response _xws_local_version _xws_local_kind _xws_local_encoded
  _xws_local_response="$(_xws_cli protocol path "$1")" || return $?
  IFS=$'\t' read -r _xws_local_version _xws_local_kind _xws_local_encoded <<< "$_xws_local_response"
  if [[ "$_xws_local_version" != "XWS/1" || "$_xws_local_kind" != "path" || -z "$_xws_local_encoded" ]]; then
    printf 'xws: invalid path response from picker\n' >&2
    return 1
  fi
  _xws_b64decode_capture "$_xws_local_encoded" || return $?
}

_xws_protocol_pick() {
  local _xws_local_response _xws_local_version _xws_local_kind _xws_local_encoded
  _xws_local_response="$(_xws_cli protocol pick "${XWS_WORKSPACE-}")" || return $?
  IFS=$'\t' read -r _xws_local_version _xws_local_kind _xws_local_encoded <<< "$_xws_local_response"
  if [[ "$_xws_local_version" != "XWS/1" ]]; then
    printf 'xws: invalid picker response\n' >&2
    return 1
  fi
  case "$_xws_local_kind" in
    selected) _xws_b64decode "$_xws_local_encoded" ;;
    cancel) return 130 ;;
    *) printf 'xws: invalid picker response\n' >&2; return 1 ;;
  esac
}

_xws_session_return_dir() {
  local _xws_local_response _xws_local_version _xws_local_kind
  local _xws_local_name _xws_local_path _xws_local_dir
  _xws_local_response="$(_xws_cli session peek "$_XWS_SESSION_ID")" || return $?
  IFS=$'\t' read -r _xws_local_version _xws_local_kind _xws_local_name _xws_local_path _xws_local_dir <<< "$_xws_local_response"
  if [[ "$_xws_local_version" != "XWS/1" || "$_xws_local_kind" != "top" ]]; then
    printf 'xws: invalid session response from picker\n' >&2
    return 1
  fi
  _xws_b64decode_capture "$_xws_local_dir" || return $?
  _XWS_RETURN_DIR="$_XWS_DECODED"
}

_xws_deactivate() {
  if [[ -z "${XWS_WORKSPACE_PATH-}" ]]; then
    return 0
  fi
  local _xws_local_active_path="$XWS_WORKSPACE_PATH"
  local _xws_local_active_name="$XWS_WORKSPACE"
  local _xws_local_active_dir="$PWD"
  _xws_session_return_dir || return $?
  if [[ ! -d "$_XWS_RETURN_DIR" ]]; then
    printf 'xws: saved return directory no longer exists: %s\n' "$_XWS_RETURN_DIR" >&2
    return 1
  fi
  _xws_run_deactivate_hooks "$_xws_local_active_path" || return 1
  _xws_clear_active "$_xws_local_active_path"
  if ! _xws_pop_frame; then
    cd "$_xws_local_active_dir" || return 1
    XWS_WORKSPACE="$_xws_local_active_name"
    XWS_WORKSPACE_PATH="$_xws_local_active_path"
    export XWS_WORKSPACE XWS_WORKSPACE_PATH
    _xws_path_prepend "$_xws_local_active_path/bin"
    _xws_run_activate_hooks "$_xws_local_active_path" || return 1
    return 1
  fi
  _xws_restore_parent "$_XWS_POP_PARENT" "${_XWS_POP_PARENT_PATH-}"
}

_xws_activate() {
  local _xws_local_name="$1"
  local _xws_local_workspace_path _xws_local_return_dir
  local _xws_local_previous_name="${XWS_WORKSPACE-}"
  local _xws_local_previous_path="${XWS_WORKSPACE_PATH-}"
  local _xws_local_status
  _xws_protocol_path "$_xws_local_name" || return $?
  _xws_local_workspace_path="$_XWS_DECODED"
  if [[ ! -d "$_xws_local_workspace_path/src" || ! -f "$_xws_local_workspace_path/bin/activate" ]]; then
    printf "xws: %s: workspace is incomplete (expected src/ and bin/activate)\n" "$_xws_local_name" >&2
    return 1
  fi
  if [[ "${XWS_WORKSPACE_PATH-}" == "$_xws_local_workspace_path" ]]; then
    return 0
  fi
  _xws_load_environment "$_xws_local_name" || return $?
  _xws_ensure_session || return $?
  _xws_local_return_dir="$PWD"

  if [[ -n "${XWS_WORKSPACE_PATH-}" ]]; then
    _xws_run_deactivate_hooks "$XWS_WORKSPACE_PATH" || return 1
    _xws_clear_active "$XWS_WORKSPACE_PATH"
  fi

  if printf '%s' "${_XWS_ENV_SNAPSHOTS-}" |
    _xws_cli session push "$_XWS_SESSION_ID" "$_xws_local_name" "$_xws_local_workspace_path" "$_xws_local_return_dir"; then
    :
  else
    _xws_local_status=$?
    if [[ -n "$_xws_local_previous_path" ]]; then
      cd "$_xws_local_return_dir" || return 1
      XWS_WORKSPACE="$_xws_local_previous_name"
      XWS_WORKSPACE_PATH="$_xws_local_previous_path"
      export XWS_WORKSPACE XWS_WORKSPACE_PATH
      _xws_path_prepend "$_xws_local_previous_path/bin"
      _xws_run_activate_hooks "$_xws_local_previous_path" || return 1
    fi
    return "$_xws_local_status"
  fi

  if ! _xws_apply_environment; then
    _xws_pop_frame || return $?
    _xws_restore_parent "${_XWS_POP_PARENT-}" "${_XWS_POP_PARENT_PATH-}"
    return 1
  fi
  if ! cd "$_xws_local_workspace_path/src"; then
    printf 'xws: cannot enter workspace source directory: %s\n' "$_xws_local_workspace_path/src" >&2
    _xws_pop_frame
    _xws_restore_parent "${_XWS_POP_PARENT-}" "${_XWS_POP_PARENT_PATH-}"
    return 1
  fi
  XWS_WORKSPACE="$_xws_local_name"
  XWS_WORKSPACE_PATH="$_xws_local_workspace_path"
  export XWS_WORKSPACE XWS_WORKSPACE_PATH
  _xws_path_prepend "$_xws_local_workspace_path/bin"

  if _xws_run_activate_hooks "$_xws_local_workspace_path"; then
    return 0
  fi
  printf 'xws: activation failed for %s\n' "$_xws_local_name" >&2
  _xws_run_deactivate_hooks "$_xws_local_workspace_path"
  _xws_clear_active "$_xws_local_workspace_path"
  _xws_pop_frame || return $?
  _xws_restore_parent "${_XWS_POP_PARENT-}" "${_XWS_POP_PARENT_PATH-}"
  return 1
}

_xws_require_inactive() {
  local _xws_local_verb="$1"
  shift
  local _xws_local_name
  local _xws_local_response _xws_local_version _xws_local_kind
  for _xws_local_name in "$@"; do
    if [[ -n "${_XWS_SESSION_ID-}" ]]; then
      _xws_local_response="$(_xws_cli session contains "$_XWS_SESSION_ID" "$_xws_local_name")" || return $?
      IFS=$'\t' read -r _xws_local_version _xws_local_kind <<< "$_xws_local_response"
      if [[ "$_xws_local_version" != "XWS/1" ]]; then
        printf 'xws: invalid session response from picker\n' >&2
        return 1
      fi
      if [[ "$_xws_local_kind" == contains ]]; then
        printf "xws: deactivate '%s' from the session stack before %s\n" "$_xws_local_name" "$_xws_local_verb" >&2
        return 1
      elif [[ "$_xws_local_kind" != absent ]]; then
        printf 'xws: invalid session response from picker\n' >&2
        return 1
      fi
    fi
    if [[ "$_xws_local_name" == "${XWS_WORKSPACE-}" && -n "${XWS_WORKSPACE-}" ]]; then
      printf "xws: deactivate '%s' before %s\n" "$_xws_local_name" "$_xws_local_verb" >&2
      return 1
    fi
  done
}

_XWS_TAB=$'\t'

xws() {
  local _xws_local_command="${1-}"
  local _xws_local_selected
  local _xws_local_status
  case "$_xws_local_command" in
    "")
      if _xws_local_selected="$(_xws_protocol_pick)"; then
        :
      else
        _xws_local_status=$?
        [[ "$_xws_local_status" -eq 130 ]] && return 0
        return "$_xws_local_status"
      fi
      [[ -n "$_xws_local_selected" ]] || return 0
      _xws_activate "$_xws_local_selected"
      ;;
    activate|push|use)
      shift
      if [[ $# -ne 1 ]]; then
        printf "usage: xws %s <workspace>\n" "$_xws_local_command" >&2
        return 2
      fi
      _xws_activate "$1"
      ;;
    deactivate|pop)
      shift
      [[ $# -eq 0 ]] || { printf "usage: xws deactivate|pop\n" >&2; return 2; }
      _xws_deactivate
      ;;
    create)
      shift
      _xws_cli create "$@"
      ;;
    rm|remove)
      shift
      _xws_require_inactive remove "$@" || return $?
      _xws_cli remove "$@"
      ;;
    mv|move)
      shift
      _xws_require_inactive move "$@" || return $?
      _xws_cli move "$@"
      ;;
    ls|list)
      shift
      if [[ $# -ne 0 ]]; then
        printf 'usage: xws %s\n' "$_xws_local_command" >&2
        return 2
      fi
      _xws_cli "$_xws_local_command"
      ;;
    which|profiles|features|version|--version|help|-h|--help)
      _xws_cli "$@"
      ;;
    *)
      printf "xws: unknown command '%s'; run 'xws help'\n" "$_xws_local_command" >&2
      return 2
      ;;
  esac
}
