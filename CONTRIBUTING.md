# Contributing to rugpui

rugpui supplies shared widgets, grid, editor and application shell for rulogman,
rudbman and rudbgen. Contributions should work across those applications and
keep application-specific behavior in the host. Start with the
[README](README.md) and [documentation index](docs/README.md).

## Reporting a problem or proposing a change

Search the [issues](https://github.com/xcomart/rugpui/issues) first. Include the
revision, affected widget or crate, host application if any, OS, reproduction
steps and expected/actual behavior. For rendering or input bugs, include theme,
display scale, keyboard/IME details and X11 versus Wayland when relevant.

Prefer a small reproduction in the gallery. Screenshots or recordings help for
layout, focus and scrolling problems; remove private text first. Discuss new
public APIs or vendored framework changes before implementing them.

## Development environment

Use Git, stable Rust with `rustfmt` and `clippy` (CI pins the toolchain in
`RUST_TOOLCHAIN` in [the workflow](.github/workflows/ci.yml), currently 1.98.0),
and a native compiler toolchain:
MSVC and the Windows SDK on Windows, Xcode command line tools on macOS, or a C
compiler on Linux. The workspace uses edition 2024. The first build downloads
pinned Git dependencies and compiles gpui from source.

On Debian or Ubuntu, install the native libraries used by CI and the gallery:

```sh
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libfontconfig1-dev
```

From a checkout of `dev`:

```sh
rustup component add rustfmt clippy
cargo build --workspace --locked
cargo run -p rugpui-gallery --locked -- --theme dark
cargo run -p rugpui-gallery --locked -- --theme light
```

Tests use gpui's headless test platform; launching the gallery requires a
graphical session. Windows release builds of the gallery also require the SDK's
`fxc.exe`; `GPUI_FXC_PATH` can select it when discovery fails.

## Where changes belong

| Crate or directory | Responsibility |
| --- | --- |
| `crates/rugpui` | Widgets, themes, text input, focus and shared UI primitives |
| `crates/rugpui-grid` | Virtualized grid over a host-provided data source |
| `crates/rugpui-editor` | Text buffer, syntax, editing, search and optional custom syntax |
| `crates/rugpui-shell` | Window frame, updater, dialogs, palettes and split panes |
| `crates/rugpui-gallery` | Runnable examples and screenshot scenes |
| `docs` | Usage guides and widget documentation |
| `vendor` | Patched gpui crates and the consumer-only unicode-width patch |

The grid, editor and shell each depend on the widget kit, not on one another.
Keep application identities, release settings, storage policy and domain data
in host-supplied adapters. Update public API documentation and examples together
with an API change.

## Validate a change

Run the full set of checks in [CI](.github/workflows/ci.yml), including the
editor feature that is not enabled by default:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy -p rugpui-editor --all-targets --features custom-syntax --locked -- -D warnings
cargo test --workspace --locked
cargo test -p rugpui-editor --features custom-syntax --locked
```

CI also runs `cargo deny --locked check` against [deny.toml](deny.toml)
(advisories are non-blocking there), the macOS relaunch helper's tests (see
[shell](docs/shell.md#the-install)) and a non-blocking `cargo check` of each
consumer application's `dev` branch against the change.

When editing public APIs or documentation, also run:

```sh
cargo doc --workspace --no-deps --locked
```

Add a focused regression test for behavior changes and exercise the gallery
interactively for visual or input changes. Check light/dark themes, keyboard
navigation, Tab/Shift-Tab focus, mouse behavior, disabled states, resizing and
display scaling as applicable. Text changes should cover Unicode, selection,
clipboard, IME and long lines; grid changes should cover null/empty values,
editing and large row counts. Report the platforms actually checked.

Keep [widget documentation](docs/widgets/) and gallery examples current. The
gallery's `--list-shots` and `--shot <name>` modes expose individual screenshot
scenes. [README gallery notes](README.md#gallery) explain `scripts/docshots.sh`
and the optional Pillow-based GIF workflow; inspect the script's environment
requirements before regenerating images.

## Vendored code and consumer checks

The root [Cargo.toml](Cargo.toml) documents why each gpui patch exists. Keep
vendor changes minimal, preserve upstream notices and the existing
`RULOGMAN PATCH` markers, and document the reason for a new hunk. An upstream
revision update must account for all carried patches and platform siblings.
Check the relevant platform; Linux tests do not validate Windows or macOS code.

After any change under `vendor/` or to the patch list, run the vendor check. It
diffs each patched crate against upstream Zed at the revision the root manifest
pins, and fails on a changed hunk without a `RULOGMAN PATCH` marker on or next to
it, or on a patched file the patch list in `Cargo.toml` does not name:

```sh
python3 scripts/vendor_check.py                       # fetches upstream (sparse, shallow)
python3 scripts/vendor_check.py --upstream ../zed     # or uses a checkout at that revision
```

It needs Python 3.11 and Git, plus network access when it fetches. The flattened
`Cargo.toml` of each vendored crate is not compared, and neither is
`vendor/unicode-width`. A follow-on edit away from its patch's main hunk, such as
an import, needs a short marker of its own. The check is not part of CI yet; a
job could run it as a single step after checkout.

`vendor/unicode-width` is not patched into or built by this workspace. Its
consumer is rulogman, so changes there need validation through rulogman too.

For a shared API or behavior change, test affected consumers before they adopt
the new revision. Put sibling checkouts next to each other and add the following
tables to each consumer's uncommitted `.cargo/config.toml`, merging with any
existing configuration. The vendored gpui crates go under the Zed source rather
than the rugpui one: the consumer already patches them to rugpui's Git source,
and Cargo does not patch a patch, so an entry under the rugpui table would be
reported unused and leave them at the pinned revision. The same CI job does the
same thing.

```toml
[patch."https://github.com/xcomart/rugpui"]
rugpui = { path = "../rugpui/crates/rugpui" }
rugpui-grid = { path = "../rugpui/crates/rugpui-grid" }
rugpui-editor = { path = "../rugpui/crates/rugpui-editor" }
rugpui-shell = { path = "../rugpui/crates/rugpui-shell" }

[patch."https://github.com/zed-industries/zed"]
gpui = { path = "../rugpui/vendor/gpui" }
gpui_linux = { path = "../rugpui/vendor/gpui_linux" }
gpui_macos = { path = "../rugpui/vendor/gpui_macos" }
gpui_windows = { path = "../rugpui/vendor/gpui_windows" }
```

For rulogman, omit the unused `rugpui-grid` entry and override its additional
patch, which it takes from crates.io's source, in a third table:

```toml
[patch.crates-io]
unicode-width = { path = "../rugpui/vendor/unicode-width" }
```

Patching the widgets and their vendored gpui together keeps them on the same
framework source. A path patch changes lockfile resolution; run
`cargo check --workspace` without `--locked` once in the consumer to update its
local lockfile, then run its normal checks. The database applications need their
Java bridge built first. Follow each application's guide:
[rulogman](https://github.com/xcomart/rulogman/blob/dev/CONTRIBUTING.md),
[rudbman](https://github.com/xcomart/rudbman/blob/dev/CONTRIBUTING.md) and
[rudbgen](https://github.com/xcomart/rudbgen/blob/dev/CONTRIBUTING.md).

Do not submit local overrides or path-resolved lockfiles. When adopting a
published commit, remove those overrides, move every rugpui Git reference in
the consumer's manifest to the same revision, regenerate its lockfile and
rerun checks. This includes the four gpui patches and rulogman's unicode-width
patch. Submit intended manifest and lockfile updates together.

## Submit your contribution

Follow the surrounding Rust style and run `cargo fmt --all`. Keep a contribution
focused on one behavior or API change, with its tests and documentation.

Add notable changes to the `Unreleased` section of [CHANGELOG.md](CHANGELOG.md)
in the same change, especially public API changes, behavior that a consumer
observes, and vendored patch changes. Name the crate and say what a consumer
must update when it moves its pinned revision. A release moves those entries
under the new version heading. The crates are used only by Git revision and
are not published to crates.io.

`dev` is the integration branch. Fork if needed, branch from the latest upstream
`dev`, and open a pull request targeting `dev`. Maintainers working directly in
this workspace validate and commit on `dev`, then push to `origin/dev`.

Use `type: summary` for new commit subjects, with a concise imperative summary.
Choose `feat`, `fix`, `docs`, `refactor`, `test`, `perf`, `build`, `ci` or `chore`
to describe the change, for example `fix: preserve grid focus after editing`
or `docs: add contribution guide`. Release and merge commits also use a prefix,
such as `chore: release <version>` or `chore: merge dev into main`. This convention
applies across all five repositories; do not rewrite existing history to rename
older commits.

Explain the problem, new behavior, compatibility impact and validation in the
pull request. Include gallery screenshots for visible changes, list consumer
checks and identify anything not tested. Exclude unrelated formatting, build
artifacts and local configuration.

CI runs on pull requests and on pushes to `dev` and `main`, but a direct `dev`
push still needs local validation first, since CI reports only after it lands. Routine work ends at `origin/dev`. Promotion and releases are
separate maintainer actions: merge the `dev` → `main` PR after required checks,
fetch `origin/main`, and tag its verified latest commit. Do not move an existing
release tag.

Contributions are covered by the [MIT license](LICENSE); vendored code retains
its upstream licenses and notices.
