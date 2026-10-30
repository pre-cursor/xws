_XWS_SCRIPT_PATH="${BASH_SOURCE[0]}"
_XWS_ROOT="$(cd "$(dirname "$_XWS_SCRIPT_PATH")/.." && pwd)" || return
_xws_var_is_set() { [[ ${!1+x} ]]; }
_xws_var_is_exported() {
  local declaration
  declaration="$(declare -p "$1" 2>/dev/null)" || return 1
  [[ "$declaration" == "declare -x "* || "$declaration" == "declare -rx "* ]]
}
_xws_var_encode() { printf '%s' "${!1}" | base64 | tr -d '\n'; }
_xws_setvar() { export "$1=$2"; }
_xws_unsetvar() { unset "$1"; }
_xws_unexportvar() { export -n "$1"; }
source "$_XWS_ROOT/script/xws-common.sh"
unset _XWS_SCRIPT_PATH
