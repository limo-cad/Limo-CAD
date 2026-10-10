# Showcase media

The images and loops come from actual native Limo CAD renders. Compact GIFs live
in this directory so the README previews work on GitHub; the longer MP4s are
assets of the companion
[showcase media release](https://github.com/limo-cad/Limo-CAD/releases/tag/showcase-v0.2.0).
No model geometry or camera frames were generated outside CAD.

The three hero PNGs are consistent 800 × 520 pixel crops of the native viewport:
the bench's clean final capture, the vise's framed assembly before its video
banner was added, and the turbine's final isometric view from this recording.
They retain the complete models without interface text or added artwork.

- **Bench:** the 4:04 build retains the first 238 seconds of the native capture
  at its recorded speed, followed by six seconds of the clean completed model.
  Recipe revision `59a27f4c` passed 595 steps and 47 checks and matched its
  deterministic reference.
- **Vise:** the 2:21 detailed montage uses full construction at `53fb4ad5` and
  pickups at `1e36fb0` for opening, screw detail and the continuous final orbit.
  The recorded recipe is unchanged between those revisions; its 1,834 steps
  and 127 checks passed and matched the deterministic reference.
- **Turbine:** the 4:15 detailed montage uses the native construction recorded
  with the packaged desktop at `68c85fd6`. Rust replay passed 3,179 steps and
  11 final checks, then exactly matched the reference model, sketches, assembly
  and geometry. The edit retains representative sketch, solid, assembly and
  part-drawing work, followed by a native eight-second orbit. Repeated hardware,
  export waits, clipped close-ups and the empty live assembly-sheet view are
  shortened or omitted; this is an edited demonstration, not an uncut replay.

Each `*-loop.gif` is 30 seconds, 360 × 225 pixels, 10 frames per second and loops
indefinitely. A stable palette avoids introducing flashing dither. The videos
use H.264/yuv420p with fast-start metadata for ordinary browsers and phones.

The hidden photo comments in the main README reserve places for the project's
future printed prototypes. Add real photographs when supplied.

Recipe sources are in [`examples/scripts`](../../../examples/scripts/README.md).
Digital checks do not replace physical prototype qualification; see
[`docs/flagship-examples.md`](../../flagship-examples.md).

`first-part.png` is an actual Windows desktop capture at `7e298ab`: the 27-step
fillet lesson completed through Scripts with its three editable features. The
12 → 18 mm feature-dialog edit was then saved, closed and reopened successfully;
the reopened dialog retained 18 mm and the 2 mm fillet remained in the model.
