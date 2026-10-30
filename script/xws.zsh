_XWS_SCRIPT_PATH="${${(%):-%N}:A}"
_XWS_ROOT="${_XWS_SCRIPT_PATH:h:h}"
_xws_var_is_set() { (( ${+parameters[$1]} )); }
_xws_var_is_exported() { [[ "${parameters[$1]-}" == *export* ]]; }
_xws_var_encode() { printf '%s' "${(P)1}" | base64 | tr -d '\n'; }
_xws_setvar() { export "$1=$2"; }
_xws_unsetvar() { unset "$1"; }
_xws_unexportvar() { typeset +x "$1"; }
source "$_XWS_ROOT/script/xws-common.sh"
unset _XWS_SCRIPT_PATH
