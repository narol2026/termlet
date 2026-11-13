# Patched forks

Termlet builds from five forks: the `iced` family (six crates), `iced_drop`,
`iced_term`, `vte`, and `alacritty_terminal`. Every one is pinned to an exact
revision in [Cargo.toml](Cargo.toml) — the `[patch.crates-io]` table, plus the
`iced_drop` dependency — so a build resolves a revision rather than whatever a
branch points at today.

This file records what those forks actually contain: the upstream revision each
patch branch starts from, what the patch set adds, and what Termlet uses it for.
The goal is that the contents of a build are inspectable without cloning five
repositories by hand, and that rebasing a fork is mechanical rather than
archaeology. Each fork keeps its patch set on a `*-patched` branch; the pinned
revision is the tip of the corresponding patch stack.

## Summary

| Fork | Upstream | Patch branch | Based on | Pinned | Patch set |
| --- | --- | --- | --- | --- | --- |
| `narol2026/iced` | `iced-rs/iced` (`0.14`) | `0.14.0-patched` | `3997291f` Remove `dev` pre-release label | `5f543772` | 10 commits, incl. a vendored `cryoglyph` |
| `narol2026/iced_drop` | `jhannyj/iced_drop` (`master`) | `0.2.2-patched` | `3036537` Merge pull request #16 (touch events) | `1ad7551` | 3 commits |
| `narol2026/iced_term` | `kemokempo/iced_term` (`master`) | `0.8.0-patched` | `e0e90be` 0.8.0 release changes (#66) | `a0964fc` | 1 squashed commit |
| `narol2026/vte` | `alacritty/vte` (`master`) | `v0.15.0-patched` | `3b3da71` Bump version to 0.15.0 | `c5a8ad1` | 1 commit |
| `narol2026/alacritty` | `alacritty/alacritty` (`v0.16`) | `v0.16-patched` | `cc1a1c9d` Alacritty version 0.16.1 | `bdf4728` | 3 commits |

## `narol2026/iced`

Resolves `iced`, `iced_core`, `iced_graphics`, `iced_futures`, `iced_widget`,
and `iced_runtime` — all six to the same revision, so they move together.

**`Ellipsis` text overflow** (`378042af`). Adds an `Ellipsis` enum
(`None`/`Start`/`Middle`/`End`) across the core, widget, and graphics layers and
wires it to `cosmic_text::Buffer::set_ellipsize()`. That needs cosmic-text 0.18,
which is newer than the one iced 0.14 pins, so the fork also vendors
`cryoglyph` — the glyph renderer that talks to cosmic-text — as a subcrate and
points the iced crates at it. The vendored copy is most of the fork's diff size;
its licenses are covered by `THIRD_PARTY_LICENSES.md` like any other dependency.
Termlet uses it to truncate labels with an ellipsis in about twenty views and
widgets.

**Window corner radius** (`5c1b8d56`, `bf8127ba`, `92c1a99b`, `037368aa`). The
platform-specific window settings gain a `corner_radius` field on both macOS and
Linux. On macOS, `NSWindow.setCornerRadius:` is macOS 15+ and does nothing on
borderless windows, so the radius is applied to the content view's backing
`CALayer` instead (with the `NSWindow` call kept behind a `respondsToSelector:`
check so the build still runs on older systems); borderless windows also become
non-opaque so the rounded corners are visible. Termlet passes
`corner_radius: Some(10.0)` when it opens the window.

**`operation::unfocus`** (`bd11a49c`). The counterpart of the existing
`focus()` helper. Termlet calls it to take focus away from the terminal while
the Spotlight popup is open, so the popup becomes the sole writer of the PTY.

**`Button::cursor_interaction`** (`36386983`). Overrides the pointer cursor
shown while hovering an active button, which otherwise always reports a pointer
interaction; the topbar and host forms use it to show a default cursor
where a pointer would be misleading.

**`text_editor` drag selection** (`3dd56435`, `0ab0b276`). Keeps the selection
updating while the pointer is dragged outside the widget's bounds, and recovers
the cursor position when it comes back. The snippet editor, the SFTP preview,
and the right sidebar are built on `text_editor`.

**`pane_grid` split line** (`5f543772`). Insets the split-line highlight from
the pane edges so the divider highlight does not touch the panes.

## `narol2026/iced_drop`

**Double-click and drag overlay** (`3cef217`). `Droppable::on_double_click`
publishes a message when the widget is double-clicked while not dragging (the
click is tracked through a `mouse::Click` in the widget state, so the state
stops being `Copy`). `Droppable::drag_opacity` and `drag_border_radius` control
the dragged preview, which is drawn in a renderer layer and dimmed with a
translucent quad; the drag cursor becomes `Move` instead of `Grabbing`. Termlet
uses the double-click message in the SFTP file table — a folder navigates into
it, a previewable file opens the preview, and anything else starts a download —
and the opacity and radius on the tab strip.

**Manifest and formatting** (`da8cecb`, `1ad7551`). Sorts the imports to match
the downstream copy, and declares the MIT license in the manifest, which
upstream omits.

## `narol2026/iced_term`

One squashed commit holding the changes Termlet builds against. The crate
version stays 0.8.0 so the branch can stand in for the crates.io release in
`[patch.crates-io]`.

- **Shell integration**: forwards the OSC messages reported by
  `alacritty_terminal` as `Action::Osc` / `Event::Osc`, merging a batch of
  actions so only the semantically interesting markers reach the application.
- **Search**: adds `SearchOptions` (case sensitivity, whole word, regex),
  `Terminal::search_jump`, and the backend search / select-and-scroll / clear
  plumbing behind the in-terminal find bar.
- **Links**: detects OSC 8 hyperlinks across wrapped rows as `HyperlinkMatch`,
  with a regex scan as the fallback for bare URLs.
- **Rendering and input**: exposes the visible grid as `RenderableCell`,
  highlights log levels, adds scrollbar geometry with drag and hover scrolling,
  input-method (IME preedit) support, paste handling, and `Terminal` helpers for
  the alt-screen state, the current line, and the cursor position.
- **Diagnostics**: a small rotating file logger that writes to
  `TERMLET_DIAGNOSTIC_LOG` when set and to
  `~/.termlet/iced_term_diagnostics.log` otherwise.
- **Font and bindings**: `FontSettings::scroll_sensitivity`, Source Code Pro as
  the default font, a `Block` key binding action, and the `libc` manifest entry.

## `narol2026/vte`

**`Handler::osc` hook** (`c5a8ad1`). The parser recognises the built-in OSC
codes and drops everything else, so a terminal cannot implement a protocol the
parser does not know — shell integration OSC 633, for instance — even though the
semantics belong to the terminal. The patch hands each unhandled code, with its
full parameter list, to a new `Handler::osc` method that reports whether it
consumed the sequence; the default returns `false`, so handlers that do not care
keep the previous behaviour.

The `alacritty_terminal` patch below depends on this hook and does not build
against a stock `vte` 0.15.0, so the two forks move together.

## `narol2026/alacritty`

Provides `alacritty_terminal`, based on the upstream `v0.16` branch.

**OSC 7/633 shell-integration events** (`c116437`). The VTE parser only splits
an OSC sequence into parameters, so an embedder cannot observe shell-integration
state without re-parsing the PTY stream. This routes OSC 7 and OSC 633 through a
new `term::osc` module and forwards the interpreted messages over the event
channel: `Event::Osc(OscMessage)` reports the working directory, prompt start,
command start, the command line reported by the shell, and command completion
with an optional exit code, while `Event::PtyOutput(Vec<u8>)` exposes each raw
PTY chunk after parsing so a consumer can capture command output between the
semantic markers while preserving byte order. The command the shell reports (OSC
633 ; E) is carried through to the command-start marker (OSC 633 ; C), and
shells that emit only the bare marker fall back to reading the command region
off the grid from the cursor position recorded at the prompt-start marker (OSC
633 ; B). This is what feeds Termlet's command-history sidebar, its
current-directory tracking, and the prompt that offers to `cd` back after a
reconnect.

**Regex search case sensitivity** (`4b530eb`). `RegexSearch::with_case_insensitive`
lets a caller supply the case sensitivity when the DFAs are built, instead of
inferring it from the pattern. `RegexSearch::new` keeps inferring, so the
existing behaviour is unchanged; the point is that a caller exposing a
match-case toggle — like Termlet's find bar — no longer has to simulate one
through the typed pattern.

**Slave PTY echo** (`bdf4728`). Clears `ECHO`, `ECHOE`, `ECHOK`, and `ECHONL` on
the local slave PTY, so pressing Enter does not produce an extra linefeed from
echo output-processing. The shell's line editor handles its own echo in raw mode
and re-enables echo when it switches to cooked mode for a foreground command.
Termlet's SSH bridge clears the same flags on the PTY it runs under — its own
stdin, allocated by the app rather than by `alacritty_terminal`
(`src/ssh/bridge.rs`) — so the raw-mode state does not depend on the platform's
idea of raw mode.

## Upstreaming

Any patch can be offered upstream, and dropping one once it lands is how a fork
gets smaller. The entries that are general-purpose in their current form are the
`vte` `Handler::osc` hook, `RegexSearch` case sensitivity, the `iced` widget
additions (`operation::unfocus`, `Button::cursor_interaction`, the `text_editor`
drag-selection fixes), and the `iced_drop` manifest's missing MIT license
declaration. The rest are downstream by construction: `iced_term`'s diagnostics
logger writes to `~/.termlet/`, and the `iced` fork's vendored `cryoglyph`
exists only because `Ellipsis` needs cosmic-text 0.18.

## Re-deriving this list

Clone a fork, add its upstream as a second remote, and diff the pinned revision
against the branch the patch stack is based on:

```bash
git clone --filter=blob:none https://github.com/narol2026/iced
cd iced
git remote add upstream https://github.com/iced-rs/iced
git fetch --filter=blob:none upstream '+refs/heads/0.14:refs/remotes/upstream/0.14'

rev=5f54377281b37857e0985d2cf6958a9662763d4f   # from Cargo.toml
base=$(git merge-base "$rev" upstream/0.14)
git log --oneline "$base".."$rev"
git diff --stat "$base".."$rev"
```

The revision is the one in `Cargo.toml`. The ref to take the merge base against
is the one the patch branch is named after: `0.14` for `iced`, `v0.16` for
`alacritty`, and `master` for the other three.

## Bumping a fork

1. Rebase the `*-patched` branch onto the new upstream revision.
2. Update the matching `rev` in `Cargo.toml` — the six `iced` entries share one
   revision — and commit the `Cargo.lock` change that the next build produces.
3. Regenerate `THIRD_PARTY_LICENSES.md` if the dependency graph changed shape,
   for example when a vendored subcrate appears or disappears.
4. Update the summary table above: the base revision, the pinned revision, and
   the shape of the patch set.
