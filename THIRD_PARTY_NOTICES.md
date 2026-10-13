# Third-party notices

Limo CAD is distributed under the license in [`LICENSE`](LICENSE). It also
uses third-party components that remain under their own licenses. The
lockfiles are the authoritative inventory of exact dependency versions; this
document calls out the primary runtime components and preserves notices that
must accompany redistributed builds.

## Embedded material data

The unified catalog adapts pinned material cards from [FreeCAD](https://github.com/FreeCAD/FreeCAD) and filament profile data from [OrcaSlicer](https://github.com/OrcaSlicer/OrcaSlicer) and [Bambu Studio](https://github.com/bambulab/BambuStudio). Adaptations resolve inheritance, normalize units, and associate profiles with existing material/color records.

FreeCAD cards retain each card's author and declared license. Attribution, exact source paths, commit revisions, and source hashes are preserved per card in `crates/export/presets/catalog.json`, exposed through `material_catalog` and the Bevy material Properties view. Creative Commons licenses: [CC BY 3.0](https://creativecommons.org/licenses/by/3.0/) and [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).

OrcaSlicer and Bambu Studio profile contributions retain their AGPL-3.0 attribution, full pinned commit, file path, and hash in the same catalog. Their license texts are available in the pinned repositories' LICENSE files; the corresponding unmodified source files can be fetched with `cargo xtask materials --fetch`. Source data remains under its stated license.

## Geometry kernels

- **Open CASCADE Technology (OCCT) 7.9.x** is used by native builds under the
  GNU Lesser General Public License 2.1 with the Open CASCADE exception.
  Native bundle generation copies `LICENSE_LGPL_21.txt` and
  `OCCT_LGPL_EXCEPTION.txt` from the selected OCCT SDK into the application
  resources. Source and license information:
  <https://github.com/Open-Cascade-SAS/OCCT>.
  Distributed SDKs retain the 7.9 ABI and apply the checked matrix storage
  changes in [`native/occt-overlay`](native/occt-overlay). The modified sources,
  pinned upstream checksums and portable rebuild instructions are supplied there
  under the same OCCT license and exception.
Native Limo CAD builds make use of and are based on facilities provided by
the Open CASCADE Technology software.

## Application runtime

| Component | Use | License |
|---|---|---|
| Bevy | Shared Bevy interface and viewport | MIT or Apache-2.0 |
| earcutr (Rust Earcut port) | Browser closed-profile triangulation | ISC |
| zip (Rust) | Shared `.limo` archives and 3MF packages | MIT or Apache-2.0 |
| Lucide | General-purpose interface icons | ISC |

Build and test dependencies are listed in `Cargo.lock`,
`desktop/Cargo.lock`, and `mcp-server/Cargo.lock`. Their package archives
contain the corresponding license texts.

## Lucide ISC notice

Copyright (c) for portions of Lucide are held by Cole Bemis 2013-2022 as part
of Feather (MIT). All other copyright (c) for Lucide are held by Lucide
Contributors 2022.

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH
REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY
AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT,
INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM
LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR
OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR
PERFORMANCE OF THIS SOFTWARE.

## earcutr ISC notice

Copyright (c) 2016, Mapbox
Copyright (c) 2018, Tree Cricket

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH
REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY
AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT,
INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM
LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR
OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR
PERFORMANCE OF THIS SOFTWARE.
