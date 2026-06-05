#!/usr/bin/env bash
set -euo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")"

release=0
run_app=1
skip_setcap="${D2HELPER_SKIP_SETCAP:-0}"
app_args=()

usage() {
    cat <<'USAGE'
Usage: ./run-d2helper.sh [options] [-- app args...]

Builds d2helper, applies Linux packet-capture capabilities when possible, detects
a local Classic/LoD install for MPQ-backed names, and runs the overlay.

Options:
  --release       Build and run target/release/d2helper
  --build-only    Compile and configure, but do not launch the GUI
  --skip-setcap   Do not apply Linux cap_net_raw/cap_net_admin
  -h, --help      Show this help

Environment:
  D2HELPER_D2_PATH       Preferred Diablo II Classic/LoD install path
  LIBD2_D2_INSTALL       Fallback install path used by libd2 tests/tools
  D2HELPER_SKIP_SETCAP=1 Same as --skip-setcap
USAGE
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --release)
            release=1
            shift
            ;;
        --build-only|--no-run)
            run_app=0
            shift
            ;;
        --skip-setcap)
            skip_setcap=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        --)
            shift
            app_args=("$@")
            break
            ;;
        *)
            app_args=("$@")
            break
            ;;
    esac
done

log() {
    printf '[d2helper] %s\n' "$*"
}

warn() {
    printf '[d2helper] warning: %s\n' "$*" >&2
}

path_contains_legacy_mpqs() {
    local path="$1"
    [[ -d "$path" && -f "$path/patch_d2.mpq" && -f "$path/d2data.mpq" ]]
}

detect_game_path() {
    if [[ -n "${D2HELPER_D2_PATH:-}" ]]; then
        if path_contains_legacy_mpqs "$D2HELPER_D2_PATH"; then
            log "using D2HELPER_D2_PATH=$D2HELPER_D2_PATH"
        else
            warn "D2HELPER_D2_PATH does not contain patch_d2.mpq and d2data.mpq: $D2HELPER_D2_PATH"
        fi
        return
    fi

    if [[ -n "${LIBD2_D2_INSTALL:-}" ]]; then
        export D2HELPER_D2_PATH="$LIBD2_D2_INSTALL"
        log "using LIBD2_D2_INSTALL as D2HELPER_D2_PATH=$D2HELPER_D2_PATH"
        return
    fi

    if [[ -n "${HOME:-}" && -d "$HOME/Games" ]]; then
        local candidate
        for candidate in "$HOME"/Games/Diablo\ II*; do
            [[ -e "$candidate" ]] || continue
            if path_contains_legacy_mpqs "$candidate"; then
                export D2HELPER_D2_PATH="$candidate"
                log "auto-detected D2HELPER_D2_PATH=$D2HELPER_D2_PATH"
                return
            fi
        done
    fi

    warn "no Classic/LoD install path detected; MPQ-backed labels will fall back to raw ids"
}

ensure_linux_capture_caps() {
    local binary="$1"

    if [[ "$(uname -s)" != "Linux" ]]; then
        return
    fi
    if [[ "$skip_setcap" == "1" ]]; then
        log "skipping Linux setcap"
        return
    fi
    if ! command -v getcap >/dev/null 2>&1 || ! command -v setcap >/dev/null 2>&1; then
        warn "getcap/setcap not found; install libcap tools or run with D2HELPER_SKIP_SETCAP=1"
        return
    fi

    local current_caps
    current_caps="$(getcap "$binary" 2>/dev/null || true)"
    if [[ "$current_caps" == *cap_net_raw* && "$current_caps" == *cap_net_admin* ]]; then
        log "packet-capture capabilities already set: $current_caps"
        return
    fi

    log "applying packet-capture capabilities to $binary"
    if [[ "${EUID:-$(id -u)}" -eq 0 ]]; then
        setcap cap_net_raw,cap_net_admin=eip "$binary"
    elif command -v sudo >/dev/null 2>&1; then
        sudo setcap cap_net_raw,cap_net_admin=eip "$binary"
    else
        warn "sudo not found; run: sudo setcap cap_net_raw,cap_net_admin=eip $binary"
        return
    fi

    getcap "$binary" || true
}

build_args=(build)
binary="target/debug/d2helper"
if [[ "$release" == "1" ]]; then
    build_args+=(--release)
    binary="target/release/d2helper"
fi

detect_game_path

log "building with: cargo ${build_args[*]}"
cargo "${build_args[@]}"

ensure_linux_capture_caps "$binary"

if [[ "$run_app" == "0" ]]; then
    log "build-only mode complete"
    exit 0
fi

log "running $binary"
exec "$binary" "${app_args[@]}"
