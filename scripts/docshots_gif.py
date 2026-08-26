#!/usr/bin/env python3
"""Reassemble a handful of stills of one animated widget into a GIF.

    docshots_gif.py <spin|sweep> <period_ms> <frames-dir> <out.gif>

Why this exists: the capture tool cannot record a window, only photograph it,
and the photographs arrive at roughly 2.7 a second with hundreds of
milliseconds of jitter between asking for one and getting it — so the moment a
frame was taken says nothing useful about where in the cycle it caught the
widget. The frames' *content* does. Both widgets here move monotonically
through one cycle, so every frame can be placed by looking at it: a spinner by
the angle of its moving pixels about the arc's centre, a sweep by their
horizontal centroid. Sorting by that recovers the order the frames would have
been drawn in, and the gaps between neighbouring positions say how long each
frame should be held for so the cycle plays back at the speed the widget really
moves at.

"Moving pixels" are the ones that differ from the per-pixel median of every
frame. The median is whatever a pixel shows for most of the cycle, so the
window's background and a progress bar's static track fall out of it and only
the animated part is left. A spinner's arc covers three quarters of its ring,
so at a ring pixel the median is the *arc* and what the mask picks out is the
quarter-turn gap in it — which turns with the arc, at the same rate, so the
ordering is unaffected.

Requires Pillow. No numpy.
"""

import sys
from collections import Counter
from math import atan2, pi, sqrt
from pathlib import Path

from PIL import Image

TAU = 2 * pi

# How far a channel has to move for a pixel to count as animated. Well below
# the distance from any of these palettes' backgrounds to their accents, and
# well above the wobble of a re-rendered antialiased edge.
THRESHOLD = 24

# A frame showing fewer moving pixels than this is one whose position cannot be
# read off it at all. Deliberately low: the sweep is eased and therefore lingers
# at both ends of its track, where all that shows is a sliver of the segment a
# pixel or two wide — those frames are most of what the ends of the cycle are
# made of, and dropping them would leave the loop jumping across them.
MIN_MOVING = 4

# Shortest frame delay a GIF can carry and be played at face value: browsers
# read anything under two centiseconds as "unspecified" and substitute a tenth
# of a second, which would play the whole cycle six times too slowly. Frames
# closer together than this are merged into their predecessor.
MIN_DELAY_MS = 20

# What the progress bar's sweeping segment covers, as a fraction of its track.
# `rugpui::progress::SWEEP_FRACTION`.
SWEEP_FRACTION = 0.3

# Columns with no moving pixel in any frame this wide or wider separate one
# spinner from the next.
CLUSTER_GAP = 4


def load(directory):
    """Every capture in `directory`, flattened onto its own background."""
    paths = sorted(directory.glob("*.png"))
    if not paths:
        sys.exit(f"no frames in {directory}")
    frames = []
    for path in paths:
        image = Image.open(path).convert("RGBA")
        # The compositor rounds the window's corners, so the captures arrive
        # with transparent ones. A GIF has one transparent index and spending
        # it on four corners would cost the palette an entry and leave the
        # corners showing whatever is behind the page; filling them with the
        # window's own background is what the eye expects anyway.
        pixels = image.load()
        opaque = Counter(
            pixels[x, y][:3]
            for y in range(image.height)
            for x in range(image.width)
            if pixels[x, y][3] == 255
        )
        if not opaque:
            sys.exit(f"{path} is entirely transparent")
        background = Image.new("RGBA", image.size, opaque.most_common(1)[0][0] + (255,))
        frames.append(Image.alpha_composite(background, image).convert("RGB"))
    sizes = {frame.size for frame in frames}
    if len(sizes) != 1:
        sys.exit(f"the captures are not all one size: {sorted(sizes)}")
    return frames


def median_frame(frames):
    """The per-pixel median of every frame: the widget standing still."""
    width, height = frames[0].size
    loaded = [frame.load() for frame in frames]
    middle = len(frames) // 2
    out = Image.new("RGB", (width, height))
    target = out.load()
    for y in range(height):
        for x in range(width):
            channels = []
            for band in range(3):
                values = sorted(pixels[x, y][band] for pixels in loaded)
                channels.append(values[middle])
            target[x, y] = tuple(channels)
    return out


def moving(frame, median):
    """The coordinates of `frame`'s pixels that are not what the median shows."""
    width, height = frame.size
    here, there = frame.load(), median.load()
    points = []
    for y in range(height):
        for x in range(width):
            a, b = here[x, y], there[x, y]
            if max(abs(a[0] - b[0]), abs(a[1] - b[1]), abs(a[2] - b[2])) > THRESHOLD:
                points.append((x, y))
    return points


def clusters(columns, width):
    """Runs of columns that hold a moving pixel, split on gaps of `CLUSTER_GAP`."""
    spans, start, gap = [], None, 0
    for x in range(width):
        if x in columns:
            if start is None:
                start = x
            gap = 0
        elif start is not None:
            gap += 1
            if gap >= CLUSTER_GAP:
                spans.append((start, x - gap))
                start = None
    if start is not None:
        spans.append((start, width - 1))
    return spans


def spin_phases(frames, median):
    """Each frame's place in the turn, as a fraction of one revolution.

    Measured on the widest of the spinners in the picture, since a large arc
    resolves its own angle far better than a twelve-pixel one does. Angles are
    clockwise from twelve o'clock — `atan2(dx, -dy)`, because the screen's y
    grows downwards — which is the direction `rugpui::Spinner` turns, so a
    frame with a larger angle is a later frame.
    """
    masks = [moving(frame, median) for frame in frames]
    width, height = frames[0].size
    everywhere = {point for mask in masks for point in mask}
    if not everywhere:
        sys.exit("nothing moved between the captures")

    spans = clusters({x for x, _ in everywhere}, width)
    def area(span):
        rows = [y for x, y in everywhere if span[0] <= x <= span[1]]
        return (span[1] - span[0] + 1) * (max(rows) - min(rows) + 1)

    left, right = max(spans, key=area)
    rows = [y for x, y in everywhere if left <= x <= right]
    centre = ((left + right) / 2, (min(rows) + max(rows)) / 2)

    phases = []
    for mask in masks:
        inside = [(x, y) for x, y in mask if left <= x <= right]
        if len(inside) < MIN_MOVING:
            phases.append(None)
            continue
        mean_x = sum(x for x, _ in inside) / len(inside)
        mean_y = sum(y for _, y in inside) / len(inside)
        angle = atan2(mean_x - centre[0], -(mean_y - centre[1])) % TAU
        phases.append(angle / TAU)
    return phases


def unease(value):
    """The animation delta that `gpui::ease_in_out` maps to `value`."""
    value = min(max(value, 0.0), 1.0)
    if value < 0.5:
        return sqrt(value / 2)
    return 1 - sqrt(2 * (1 - value)) / 2


def sweep_phases(frames, median):
    """Each frame's place in the pass, as the animation's own delta.

    The segment's centroid is what the picture shows, and the widget's own
    arithmetic turns that back into a delta: the segment spans `left` to
    `left + SWEEP_FRACTION` of the track and is clipped by it, so a centroid
    inside the track's middle is `left + SWEEP_FRACTION / 2` and one near
    either end is the midpoint of whatever is still showing. `sweep_left` and
    `ease_in_out` invert from there — which matters, because the sweep is eased
    and a GIF laid out by centroid alone would play it at a constant speed the
    widget never moves at.
    """
    masks = [moving(frame, median) for frame in frames]
    everywhere = {x for mask in masks for x, _ in mask}
    if not everywhere:
        sys.exit("nothing moved between the captures")
    track_left, track_right = min(everywhere), max(everywhere)
    track = track_right - track_left + 1

    phases = []
    for mask in masks:
        if len(mask) < MIN_MOVING:
            phases.append(None)
            continue
        centre = (sum(x for x, _ in mask) / len(mask) - track_left) / track
        if centre < SWEEP_FRACTION / 2:
            left = 2 * centre - SWEEP_FRACTION
        elif centre > 1 - SWEEP_FRACTION / 2:
            left = 2 * centre - 1
        else:
            left = centre - SWEEP_FRACTION / 2
        eased = (left + SWEEP_FRACTION) / (1 + SWEEP_FRACTION)
        phases.append(unease(eased))
    return phases


def lay_out(order, period_ms):
    """Frame indices and delays, from frames already sorted by phase.

    A frame is held until the next one's turn comes round, so its delay is the
    gap to its successor — and the last frame's is the gap round to the first,
    which is what makes the loop seamless. Delays are then quantised to the
    centiseconds a GIF actually stores, with the rounding error handed to the
    longest frames so the cycle still adds up to exactly one period.
    """
    kept, gaps = [], []
    for index, (position, phase) in enumerate(order):
        nxt = order[(index + 1) % len(order)][1]
        gap = (nxt - phase) % 1.0
        if index == len(order) - 1 and gap == 0.0:
            gap = 1.0
        kept.append(position)
        gaps.append(gap)

    # Merge anything too brief to be stored honestly into the frame before it.
    merged_frames, merged_gaps = [], []
    for position, gap in zip(kept, gaps):
        if merged_gaps and merged_gaps[-1] * period_ms < MIN_DELAY_MS:
            merged_gaps[-1] += gap
        else:
            merged_frames.append(position)
            merged_gaps.append(gap)
    while len(merged_gaps) > 1 and merged_gaps[-1] * period_ms < MIN_DELAY_MS:
        merged_gaps[-2] += merged_gaps.pop()
        merged_frames.pop()

    total = sum(merged_gaps)
    exact = [gap / total * period_ms for gap in merged_gaps]
    delays = [max(MIN_DELAY_MS, int(round(value / 10)) * 10) for value in exact]
    # Hand the rounding error to the longest frames, where a centisecond shows
    # least — whether it has to be added or taken away.
    drift = int(round(period_ms / 10)) * 10 - sum(delays)
    step = 10 if drift > 0 else -10
    for _ in range(abs(drift) // 10):
        index = max(range(len(delays)), key=lambda i: delays[i])
        if delays[index] + step < MIN_DELAY_MS:
            break
        delays[index] += step
    return merged_frames, delays


def write_gif(frames, positions, delays, out):
    """Saves the chosen frames under one palette, so nothing flickers."""
    chosen = [frames[position] for position in positions]
    width, height = chosen[0].size
    # One palette for the whole animation, taken from every frame at once: a
    # palette per frame would have each of them quantise its antialiasing
    # differently and the background would crawl.
    strip = Image.new("RGB", (width, height * len(chosen)))
    for index, frame in enumerate(chosen):
        strip.paste(frame, (0, index * height))
    palette = strip.quantize(colors=255, method=Image.Quantize.MEDIANCUT)
    quantised = [frame.quantize(palette=palette, dither=Image.Dither.NONE) for frame in chosen]
    quantised[0].save(
        out,
        save_all=True,
        append_images=quantised[1:],
        duration=delays,
        loop=0,
        optimize=False,
        disposal=1,
    )


def main():
    if len(sys.argv) != 5:
        sys.exit(__doc__.strip().splitlines()[2].strip())
    kind, period_ms, directory, out = sys.argv[1], int(sys.argv[2]), Path(sys.argv[3]), Path(sys.argv[4])
    if kind not in ("spin", "sweep"):
        sys.exit(f"unknown motion {kind!r}; expected spin or sweep")

    frames = load(directory)
    median = median_frame(frames)
    phases = spin_phases(frames, median) if kind == "spin" else sweep_phases(frames, median)

    order = sorted(
        ((index, phase) for index, phase in enumerate(phases) if phase is not None),
        key=lambda pair: pair[1],
    )
    if len(order) < 2:
        sys.exit(f"only {len(order)} of {len(frames)} captures could be placed in the cycle")

    positions, delays = lay_out(order, period_ms)
    write_gif(frames, positions, delays, out)
    print(
        f"{out}: {len(positions)} frames of {len(frames)} captures, "
        f"{sum(delays)} ms a cycle ({len(frames) - len(order)} unplaceable)"
    )


if __name__ == "__main__":
    main()
