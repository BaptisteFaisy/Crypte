#!/usr/bin/env bash
# Focused test for the ghostty/crypte dispatcher: in-place launch on a
# compatible terminal (interactive TTY, truecolor, at least 80x24),
# Ghostty window otherwise. Print-only modes always run in place.
# Rebuild mode (first arg) syncs origin/main + forces a release build.
#
#   ghostty/crypte-test.sh
#
# Self-contained (Linux): stubs the release binary (CRYPTE_BIN), ghostty
# and cargo (PATH), fakes TTYs with script(1), fakes the git remote with
# a local-path clone and redirects the managed worktree
# (CRYPTE_LATEST_ROOT). No real build, no window, no network.
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

# zero <name> <rc> / nonzero <name> <rc>
zero() {
    count=$((count + 1))
    if [ "$2" -eq 0 ]; then
        printf 'ok %s\n' "$1"
    else
        fails=$((fails + 1))
        printf 'FAIL %s: expected exit 0, got %s\n' "$1" "$2"
    fi
}

nonzero() {
    count=$((count + 1))
    if [ "$2" -ne 0 ]; then
        printf 'ok %s\n' "$1"
    else
        fails=$((fails + 1))
        printf 'FAIL %s: expected non-zero exit\n' "$1"
    fi
}

# rebuild anywhere but first is app args, like a late --window.
out=$(no_tty TERM=dumb -- --muse-auth rebuild)
expect "window-late-rebuild-passthrough" "WINDOW" "$out"
present "window-late-rebuild-passthrough" "--muse-auth rebuild" "$out"

# Rebuild fixtures: origin repo + clone carrying a copy of the launcher.
printf '#!/bin/sh\necho "CARGO $*"\n' >"$tmp/cargo"
chmod +x "$tmp/cargo"
fx="$tmp/fx"
git_init() {
    git init -q "$1"
    git -C "$1" config user.email rebuild@test
    git -C "$1" config user.name rebuild
    git -C "$1" checkout -q -b main
}
git_init "$fx/origin"
echo v1 >"$fx/origin/version.txt"
mkdir -p "$fx/origin/ghostty"
cp "$launcher" "$fx/origin/ghostty/crypte"
git -C "$fx/origin" add version.txt ghostty/crypte
git -C "$fx/origin" commit -qm v1
git clone -q "$fx/origin" "$fx/root" 2>/dev/null
fxlauncher="$fx/root/ghostty/crypte"

# Origin moved ahead, main checkout dirty: rebuild still works — the
# checkout is never touched, latest goes to the managed worktree.
echo v2 >"$fx/origin/version.txt"
git -C "$fx/origin" commit -qam v2
echo dirty >"$fx/root/dirty.txt"
out=$(CRYPTE_LATEST_ROOT="$fx/latest" "$fxlauncher" rebuild --smoke </dev/null 2>&1)
rc=$?
present "rebuild-builds-release" "CARGO build --release --manifest-path $fx/latest/Cargo.toml" "$out"
present "rebuild-launches-fresh" "PLACE --smoke" "$out"
present "rebuild-announces" "rebuilt " "$out"
zero "rebuild-exit" "$rc"
if [ "$(git -C "$fx/latest" rev-parse HEAD 2>/dev/null)" = "$(git -C "$fx/origin" rev-parse HEAD)" ]; then
    pass "rebuild-syncs-origin-main"
else
    fail "rebuild-syncs-origin-main" "worktree HEAD != origin HEAD" "$(git -C "$fx/latest" log --oneline -2 2>/dev/null)"
fi
if grep -q v2 "$fx/latest/version.txt" 2>/dev/null && grep -q dirty "$fx/root/dirty.txt"; then
    pass "rebuild-leaves-checkout-alone"
else
    fail "rebuild-leaves-checkout-alone" "worktree not latest or checkout touched" "$(cat "$fx/latest/version.txt" 2>/dev/null; ls "$fx/root")"
fi

# Plain launch prefers the managed build when there is one.
mkdir -p "$fx/latest/target/release"
printf '#!/bin/sh\necho "LATEST $*"\n' >"$fx/latest/target/release/kiro-horror-tui"
chmod +x "$fx/latest/target/release/kiro-horror-tui"
out=$(env -u CRYPTE_BIN CRYPTE_LATEST_ROOT="$fx/latest" "$fxlauncher" --smoke </dev/null 2>&1)
rc=$?
present "place-prefers-latest-build" "LATEST --smoke" "$out"
absent "place-prefers-latest-build" "PLACE" "$out"
zero "place-prefers-latest-exit" "$rc"
rm -rf "$fx/latest/target"

# Dirty managed worktree: rebuild refuses, nothing rebuilds or launches.
echo messy >"$fx/latest/messy.txt"
out=$(CRYPTE_LATEST_ROOT="$fx/latest" "$fxlauncher" rebuild </dev/null 2>&1)
rc=$?
present "rebuild-dirty-refused" "dirty" "$out"
absent "rebuild-dirty-refused" "PLACE" "$out"
absent "rebuild-dirty-refused" "WINDOW" "$out"
absent "rebuild-dirty-refused" "CARGO" "$out"
nonzero "rebuild-dirty-exit" "$rc"

printf '%s/%s passed\n' "$((count - fails))" "$count"
if [ "$fails" -eq 0 ]; then
    exit 0
else
    exit 1
fi
