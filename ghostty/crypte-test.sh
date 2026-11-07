#!/usr/bin/env bash
# Focused test for the ghostty/crypte dispatcher: in-place launch on a
# compatible terminal (interactive TTY, truecolor, at least 80x24),
# Ghostty window otherwise. Print-only modes always run in place.
#
#   ghostty/crypte-test.sh
#
# Self-contained (Linux): stubs the release binary (CRYPTE_BIN) and ghostty
# (PATH), fakes TTYs with script(1). No build, no window, no network.
set -u
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
launcher="$here/crypte"
count=0
fails=0

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
printf '#!/bin/sh\necho "PLACE $*"\n' >"$tmp/kiro-horror-tui"
printf '#!/bin/sh\necho "WINDOW $*"\n' >"$tmp/ghostty"
chmod +x "$tmp/kiro-horror-tui" "$tmp/ghostty"
export CRYPTE_BIN="$tmp/kiro-horror-tui"
export PATH="$tmp:$PATH"

# Run the launcher on a sized pty with a controlled environment.
# usage: on_pty <rows> <cols> <env…> -- <launcher args…>
on_pty() {
    local rows=$1 cols=$2
    shift 2
    local cmd="stty rows $rows cols $cols >/dev/null 2>&1; env -u COLORTERM -u TERM_PROGRAM"
    while [ $# -gt 0 ] && [ "$1" != "--" ]; do
        cmd="$cmd $1"
        shift
    done
    shift # --
    cmd="$cmd \"$launcher\""
    while [ $# -gt 0 ]; do
        cmd="$cmd $1"
        shift
    done
    timeout 30 script -qec "$cmd" /dev/null 2>/dev/null | tr -d '\r'
}

# Run the launcher with no TTY anywhere (stdin closed, stdout piped).
# usage: no_tty <env…> -- <launcher args…>
no_tty() {
    local envs=()
    while [ $# -gt 0 ] && [ "$1" != "--" ]; do
        envs+=("$1")
        shift
    done
    shift # --
    timeout 30 env -u COLORTERM -u TERM_PROGRAM "${envs[@]}" "$launcher" "$@" </dev/null 2>/dev/null | cat
}

pass() {
    count=$((count + 1))
    printf 'ok %s\n' "$1"
}

fail() {
    count=$((count + 1))
    fails=$((fails + 1))
    printf 'FAIL %s: %s\n%s\n' "$1" "$2" "$3"
}

# expect <name> <marker-regex-after-^> <output>
expect() {
    if printf '%s' "$3" | grep -qa "^$2"; then
        pass "$1"
    else
        fail "$1" "expected $2" "$3"
    fi
}

# present <name> <fixed-substring> <output>
present() {
    if printf '%s' "$3" | grep -qaF -e "$2"; then
        pass "$1"
    else
        fail "$1" "missing $2" "$3"
    fi
}

# absent <name> <fixed-substring> <output>
absent() {
    if printf '%s' "$3" | grep -qaF -e "$2"; then
        fail "$1" "forbidden $2 present" "$3"
    else
        pass "$1"
    fi
}

out=$(on_pty 30 100 TERM=xterm-256color COLORTERM=truecolor --)
expect "place-compatible" "PLACE" "$out"

out=$(on_pty 30 100 TERM=xterm-ghostty TERM_PROGRAM=ghostty --)
expect "place-ghostty-no-colorterm" "PLACE" "$out"

out=$(on_pty 30 100 TERM=xterm-256color --)
expect "window-no-truecolor" "WINDOW" "$out"

out=$(on_pty 30 100 TERM=dumb COLORTERM=truecolor --)
expect "window-dumb" "WINDOW" "$out"

out=$(on_pty 10 40 TERM=xterm-256color COLORTERM=truecolor --)
expect "window-tiny" "WINDOW" "$out"

out=$(on_pty 24 80 TERM=xterm-256color COLORTERM=truecolor --)
expect "place-exact-minimum" "PLACE" "$out"

out=$(no_tty TERM=xterm-256color COLORTERM=truecolor --)
expect "window-no-tty" "WINDOW" "$out"

out=$(on_pty 30 100 TERM=xterm-256color COLORTERM=truecolor -- --window)
expect "window-forced" "WINDOW" "$out"

out=$(on_pty 30 100 TERM=xterm-256color COLORTERM=truecolor -- --fenetre)
expect "window-forced-alias" "WINDOW" "$out"

out=$(no_tty TERM=dumb -- --once hello)
expect "place-once-no-tty" "PLACE --once hello" "$out"

out=$(no_tty TERM=dumb -- --help)
expect "place-help-no-tty" "PLACE --help" "$out"

out=$(no_tty TERM=dumb -- --muse-auth)
expect "window-tui-args-passthrough" "WINDOW" "$out"
present "window-tui-args-passthrough" "--muse-auth" "$out"

out=$(no_tty TERM=dumb --)
expect "window-default-carries-shader" "WINDOW" "$out"
present "window-default-carries-shader" "crypte.glsl" "$out"

out=$(no_tty TERM=dumb -- --no-shader)
expect "window-no-shader-stripped" "WINDOW" "$out"
absent "window-no-shader-stripped" "--no-shader" "$out"
absent "window-no-shader-stripped" "custom-shader" "$out"

out=$(on_pty 30 100 TERM=xterm-256color COLORTERM=truecolor -- --no-shader --muse-auth)
expect "place-flags-stripped-passthrough" 'PLACE --muse-auth$' "$out"
absent "place-flags-stripped-passthrough" "--no-shader" "$out"

out=$(on_pty 30 100 TERM=xterm-256color COLORTERM=truecolor -- --muse-auth --window)
expect "place-late-window-flag-ignored" 'PLACE --muse-auth --window$' "$out"

printf '%s/%s passed\n' "$((count - fails))" "$count"
if [ "$fails" -eq 0 ]; then
    exit 0
else
    exit 1
fi
