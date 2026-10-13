# Edge-Case Hunt — break Limo CAD on purpose

Limo CAD is pre-alpha mechanical CAD. The fastest way to make it reliable is
for people to try **real parts** and **weird geometry** and tell us where it
breaks. Every report becomes a regression test, so the bug stays fixed.

This is the companion guide to the pinned
[Edge-case hunt issue](https://github.com/limo-cad/Limo-CAD/issues/45).

## Why edge cases matter

The kernel (OCCT + our modeling logic) is the hardest part of CAD to get
right. Ordinary rectangles and boxes work; the interesting failures live at
the boundaries: tangent faces, grazing angles, thin walls, coincident
geometry, self-intersecting paths. We cannot find those ourselves — we need
you to try things we never would.

## How to participate

1. **Get a build.** See the README → *Build locally* (macOS/Windows bundles,
   or the browser dev build).
2. **Model something real** — a bracket, a pulley, a gear cover, a fixture,
   whatever you actually need. Or work through the challenge list below.
3. **When something breaks**, report it (see below). If it *works*, that's
   also useful — say so in the
   [Edge-case hunt issue](https://github.com/limo-cad/Limo-CAD/issues/45)
   and we'll mark it as verified.

## Challenge list — where edge cases hide

Each area is known-hard for a young kernel. "It worked" is just as valuable a
report as "it broke".

- **Fillet / chamfer**: edges meeting at T-junctions; several edges selected
  at once; edges meeting a face at a grazing angle; fillet radius larger than
  an edge's neighbors; fillet after shell; fillet on a threaded hole mouth.
- **Threaded holes**: ISO/Unified threads in walls thinner than one pitch;
  thread + fillet on the same hole; countersink + thread; two holes tangent to
  each other; holes at 45° to a face.
- **Combine / split**: two solids sharing a face; tangent faces; coincident
  faces; cutting flush with a face; combine with a body created by a pattern.
- **Shell**: bodies with through-holes; internal features; walls that should
  end up uneven; shelling a body that already has a fillet.
- **Loft / sweep**: non-planar profiles; profiles with different vertex
  counts; sweep along a closed path; self-intersecting paths; loft from a
  sketch to a point.
- **Revolve**: profiles touching the axis; crossing the axis; open profiles;
  revolve with a thin wall.
- **Patterns / mirror**: patterns on curved faces; instances that overlap;
  mirror + pattern combos; patterns of holes through a shell.
- **History editing**: delete, reorder, or drag a feature in the timeline;
  edit a sketch that an extrude depends on; undo across feature boundaries;
  reorder a feature before its sketch.
- **Project files**: save, close, reopen; open a `.limo` after editing it in
  another tool; import odd STEP files; model with mm vs inch units.

## How to report well

A good report takes five minutes and saves us an hour:

- **Steps from a new project** — File → New, then the exact clicks and values.
- **What you expected vs what happened.**
- **A screenshot or short recording** for anything visual.
- **The `.limo` file** (if safe to share) **plus a STEP backup** — STEP
  survives even if the `.limo` format changes in pre-alpha.
- **OS + build info** — e.g. *Windows 11, main @ 5071ec2*.

Use the [bug template](../.github/ISSUE_TEMPLATE/bug_report.yml) (open an
issue and pick *Bug report*), or comment on the
[Edge-case hunt issue](https://github.com/limo-cad/Limo-CAD/issues/45) if
you are not sure it is a bug. Either way, try to say whether it feels like an
edge case — that gets the `edge-case` label and a regression test.

## What happens after you report

1. A maintainer (or an AI on the MCP harness) reproduces it.
2. It becomes a **regression test** — a Playwright e2e (`scripts/e2e-*.mjs`)
   or a cargo test — so it cannot silently come back.
3. Fixes land with release notes.
4. Bug hunters get acknowledged below — say "count me in" if you want credit.

## Hall of Fame

Thank you to everyone who has broken things on purpose:

- *(no entries yet — be the first)*

## FAQ

**Is this a bug or am I doing it wrong?** If something breaks, report it. Even
"user error" reports are useful — they show where the UX misleads people.

**Is my data safe?** Export a STEP copy of anything you care about (the README
recommends this for all pre-alpha projects). `.limo` files are ZIP archives —
you can inspect them with any unzip tool.

**Can I stay anonymous?** Yes. Just don't ask for Hall of Fame credit.

**I can't build the project — can I still help?** Yes! The browser dev build
has the same Rust model via WASM. If even that is too much, you can still help
by reviewing [open issues](https://github.com/limo-cad/Limo-CAD/issues)
and confirming reproductions.
