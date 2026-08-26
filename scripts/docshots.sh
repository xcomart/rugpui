#!/usr/bin/env bash
#
# Takes the pictures the documentation is illustrated with.
#
# The gallery binary has a second mode — `--shot <name>` opens one widget in one
# state in a window sized to it, and `--list-shots` writes that registry out —
# and this walks the registry, photographing each window into
# `docs/screenshots/<name>.png`. Nothing here knows what any shot *is*: adding a
# picture is adding an entry to `crates/rugpui-gallery/src/shots/`, and this
# script picks it up on the next run.
#
#   scripts/docshots.sh                 # every shot, in the dark palette
#   scripts/docshots.sh select          # only the shots whose name starts "select"
#   scripts/docshots.sh --theme light   # every shot, in the light palette
#   scripts/docshots.sh --gallery       # the two whole-window gallery pictures
#
# A shot whose widget *moves* — a spinner, an indeterminate progress bar — is a
# GIF rather than a PNG. `--list-shots` says which, and how long one cycle of
# the motion takes; this photographs one such window many times over and hands
# the pile to `scripts/docshots_gif.py`, which puts the frames in order by what
# is in them rather than by when they arrived. See that script for why.
#
# Requirements: a running KDE/Plasma session, `spectacle`, and — for the moving
# shots alone — `python3` with Pillow. `spectacle -a` photographs the window
# that has the *focus*, which is why the shots are taken one at a time and why
# the binary calls `cx.activate(true)` as it opens one. `-e -S` drop the
# decoration and the drop shadow, so the file is exactly the window's content
# and therefore exactly the size the shot asked for — which is what the check at
# the end of every capture verifies.

set -euo pipefail

readonly ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly BIN="$ROOT/target/debug/rugpui-gallery"
readonly OUT="$ROOT/docs/screenshots"

# How long a window is given to open, lay itself out and settle before the
# picture is taken. Generous on purpose: a shot taken too early is a shot of an
# unlaid-out window, and the whole run is a couple of minutes either way.
readonly SETTLE=2
# And how long to leave the file being written before the window is closed.
readonly FLUSH=1
# How many stills a moving shot is photographed from. The captures land at
# roughly 2.7 a second wherever they happen to fall in the cycle, so this is a
# sampling budget rather than a frame count: the assembler keeps whichever of
# them it can place and merges the ones that land on top of each other. Enough
# of them that the widest gap left in the cycle is a frame's worth rather than a
# visible hitch, and no more, since each one costs a third of a second.
readonly FRAMES=72

# Every palette a per-palette shot is taken in, in the order `--theme` names
# them.
readonly PALETTES=(dark light solarized-dark solarized-light gruvbox-dark dracula)

# The whole-window gallery pictures, which are taken *with* their decoration —
# they are what the README opens with, and a window in a page is a window.
readonly GALLERY_PALETTES=(dark light)

theme=dark
gallery=false
prefix=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --theme)
            theme="${2:?--theme needs a palette id}"
            shift 2
            ;;
        --theme=*)
            theme="${1#--theme=}"
            shift
            ;;
        --gallery)
            gallery=true
            shift
            ;;
        -h | --help)
            sed -n '3,/^$/p' "${BASH_SOURCE[0]}" | sed 's|^# \?||'
            exit 0
            ;;
        --*)
            echo "unknown option: $1" >&2
            exit 2
            ;;
        *)
            prefix="$1"
            shift
            ;;
    esac
done

command -v spectacle >/dev/null || {
    echo "spectacle is not installed; it is what takes the pictures" >&2
    exit 1
}

echo "building the gallery…"
cargo build --manifest-path "$ROOT/Cargo.toml" -p rugpui-gallery

mkdir -p "$OUT"

# Names of the shots whose file came out the wrong size, or not at all.
wrong=()

# Photographs one window: `capture <output> <arg>...`, where the arguments are
# the binary's own.
capture() {
    local out="$1"
    shift
    mkdir -p "$(dirname "$out")"
    rm -f "$out"
    "$BIN" "$@" &
    local pid=$!
    sleep "$SETTLE"
    # -a active window, -b no GUI, -n no notification, -e no decoration,
    # -S no shadow.
    spectacle -a -b -n -e -S -o "$out" >/dev/null 2>&1 || true
    sleep "$FLUSH"
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
}

# The `WIDTHxHEIGHT` of a PNG, or the empty string when there is no file.
dimensions() {
    [[ -f "$1" ]] || return 0
    file -b "$1" | sed -n 's/.*, \([0-9]\+\) x \([0-9]\+\),.*/\1x\2/p'
}

# Takes one shot and checks the file is the size the shot asked for.
#
# Tried twice, because a compositor occasionally hands the first window of a run
# a size of its own choosing and gets it right the second time; a picture that
# is the wrong size is a picture of a window that was still being placed.
shoot() {
    local name="$1" width="$2" height="$3" out="$4"
    printf '  %-34s %sx%s\n' "$name" "$width" "$height"
    local attempt got
    for attempt in 1 2; do
        capture "$out" --theme "$theme" --shot "$name"
        got="$(dimensions "$out")"
        [[ "$got" == "${width}x${height}" ]] && return 0
    done
    wrong+=("$name: asked for ${width}x${height}, file is ${got:-missing}")
}

# Photographs a window whose widget moves, and assembles the pile into a GIF.
#
# One window, many captures: the animation runs off the widget's own element id
# and neither stops nor restarts, so every capture is a different instant of the
# same cycle. Which instant is `docshots_gif.py`'s problem, not this script's.
shoot_motion() {
    local name="$1" width="$2" height="$3" out="$4" motion="$5"
    local kind="${motion%%:*}" period="${motion##*:}"
    printf '  %-34s %sx%s  %s\n' "$name" "$width" "$height" "$motion"

    local dir
    dir="$(mktemp -d)"
    "$BIN" --theme "$theme" --shot "$name" &
    local pid=$!
    sleep "$SETTLE"
    local frame
    for ((frame = 1; frame <= FRAMES; frame++)); do
        spectacle -a -b -n -e -S -o "$(printf '%s/f%03d.png' "$dir" "$frame")" \
            >/dev/null 2>&1 || true
    done
    sleep "$FLUSH"
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true

    # The size check is the same one a still shot gets, asked of the first
    # capture: every frame of the pile is the same window.
    local got
    got="$(dimensions "$(printf '%s/f001.png' "$dir")")"
    if [[ "$got" != "${width}x${height}" ]]; then
        wrong+=("$name: asked for ${width}x${height}, first capture is ${got:-missing}")
        rm -rf "$dir"
        return 0
    fi

    mkdir -p "$(dirname "$out")"
    rm -f "$out"
    python3 "$ROOT/scripts/docshots_gif.py" "$kind" "$period" "$dir" "$out" \
        || wrong+=("$name: the captures could not be assembled into a cycle")
    rm -rf "$dir"
}

if [[ "$gallery" == true ]]; then
    echo "the gallery window, with its decoration:"
    for palette in "${GALLERY_PALETTES[@]}"; do
        out="$OUT/gallery-$palette.png"
        printf '  %s\n' "gallery-$palette"
        rm -f "$out"
        "$BIN" --theme "$palette" &
        pid=$!
        sleep "$SETTLE"
        # No -e and no -S: these two keep the decoration and the shadow, which
        # is how they have always been taken.
        spectacle -a -b -n -o "$out" >/dev/null 2>&1 || true
        sleep "$FLUSH"
        kill "$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true
    done
    echo "done."
    exit 0
fi

echo "shots, in the $theme palette:"

# `--list-shots` writes one shot per line: name, width, height, the per-palette
# output template, and the motion. A column a shot has nothing to say in is
# written as `-`, because a tab is whitespace and `read` collapses a run of
# whitespace separators into one — two empty columns in a row would shift every
# column after them along.
while IFS=$'\t' read -r name width height per_theme motion; do
    [[ -z "$name" ]] && continue
    [[ -n "$prefix" && "$name" != "$prefix"* ]] && continue

    if [[ "$motion" != "-" ]]; then
        shoot_motion "$name" "$width" "$height" "$OUT/$name.gif" "$motion"
        continue
    fi

    if [[ "$per_theme" == "-" ]]; then
        shoot "$name" "$width" "$height" "$OUT/$name.png"
        continue
    fi

    # A shot the pages show one of per palette: taken in each, and filed under
    # the template's own name rather than the shot's.
    for palette in "${PALETTES[@]}"; do
        theme_before="$theme"
        theme="$palette"
        shoot "$name" "$width" "$height" "$OUT/${per_theme//%s/$palette}.png"
        theme="$theme_before"
    done
done < <("$BIN" --list-shots)

if [[ ${#wrong[@]} -gt 0 ]]; then
    echo
    echo "these came out the wrong size:" >&2
    printf '  %s\n' "${wrong[@]}" >&2
    exit 1
fi

echo "done."
