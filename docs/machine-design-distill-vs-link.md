# Distill versus link — machine-design knowledge

Policy for `knowledge/machine-design/`. Session reuse audit updated 2026-10-07;
older catalog rows are not newly verified by that date.
Companion table: [`knowledge/machine-design/SOURCES.md`](../knowledge/machine-design/SOURCES.md).

## Rule of thumb

| We do | We do not |
|-------|-----------|
| Write **our own** thin help pages in our voice | Copy chapters, slides or standards without applicable permission |
| **Copy** selected licensed passages, code or figures with their notices and attribution | Treat a source link or software license as permission for unrelated content |
| **Distill** ideas from PD / CC BY / Apache sources with attribution | Bundle CC BY-**NC** / NC-SA text into the product help corpus |
| **Link** to great NC books, OCW courses, and buy-pages for standards | Scrape MatWeb / MakeItFrom or copy ASME / ISO |
| Point agents at local KB first, then these links | Claim allowables, load ratings, or “ASME-compliant” from help text |

**Distill** = write short OKF concepts and recipes with source attribution.
Rewording a protected passage is not automatically independent authorship or a
way around license terms. An explanation of ideas, methods or formulas written
independently differs from copying their particular expression; see
[U.S. Copyright Office Circular 31](https://www.copyright.gov/circs/circ31.pdf).

**Link** = URL + one-line why it is useful; no substantial quotation.

**Copy** = retain the source's expression, either unchanged or with identified
edits. Copying is supported when the license covers the exact artifact. Preserve
author, title, original URL, version/commit, copyright and license notices;
identify changes and separately credited/excluded content. Keep copies under
their source license and include required license/NOTICE files. CC BY permits
commercial copying with attribution and no extra restrictions on licensed
rights; MIT/Apache code and associated documentation
can be copied subject to their terms. A thin authored page remains preferable to
a bulk manual when it serves the Help task better, but that is a product choice,
not a claim that permitted copying is prohibited.

See the [session knowledge audit](analysis/SESSION-KNOWLEDGE-AUDIT.md) for
artifact-specific decisions, including OCCT release-level licensing evidence
and the limits on vendor datasheets. GPL/AGPL/ShareAlike content is not forbidden
to use: copying or integrating it needs its applicable distribution obligations
and a compatible destination license, rather than relabeling it LGPL.
Those obligations can extend to an adapted or combined work, so preserving
one file's notice is not automatically sufficient for the shipped bundle.

## Why this split

Limo CAD ships under **LGPL-2.1-or-later**. Help that rides with the app must not
quietly include NonCommercial content. ShareAlike can permit commercial copying,
with its attribution and derivative-licensing obligations. Preserve the copied
artifact's license and applicable terms; the project license does not replace
them. Proprietary standards are contractual references, not open textbooks.

---

## DISTILL — write from these

These are the **primary spine**. Prefer them when filling pages.

### GD&T and product definition

| Source | License (verified) | What we take |
|--------|-------------------|--------------|
| [NIST/Berez GD&T Part I](https://zenodo.org/records/7647256) | **CC BY 4.0** (Zenodo API `cc-by-4.0`) | Concept teaching: why tolerances, datums, FCFs, characteristics |
| [NIST/Berez GD&T Part II](https://zenodo.org/records/8237278) | **CC BY 4.0** | Inspection mindset, implementation checklist, limits & fits intro |
| [NIST MBE PMI models / STEP](https://www.nist.gov/ctl/smart-connected-systems-division/smart-connected-manufacturing-systems-group/mbe-pmi-0) | US gov / unrestricted NIST data | Example PMI geometry for demos — not a how-to course |
| Wikimedia Commons GD&T SVGs | **file-by-file** (prefer CC0/PD) | Symbols/diagrams only after checking each file |

### DFM / DFA

| Source | License (verified) | What we take |
|--------|-------------------|--------------|
| [NWTC LibreTexts — Design for Various Manufacturing Methods](https://eng.libretexts.org/Courses/Northeast_Wisconsin_Technical_College/Design_for_Various_Manufacturing_Methods) (Bryan Guns) | **CC BY 4.0** (page footer on DFM chapter) | Process families, DFM principles, AM notes — rewrite, do not paste Boothroyd tables (book already avoids copyrighted tables) |
| [PALNI — Design for Manufacture and Assembly](https://pressbooks.palni.org/designmanufactureassembly/) (Gagnon & Bearman) | **CC BY 4.0** | DFA heuristics (part count, orientation, snap fits) |

### Machine elements

| Source | License (verified) | What we take |
|--------|-------------------|--------------|
| [NASA Fastener Design Manual (NASA-RP-1228)](https://ntrs.nasa.gov/citations/19900009424) | **US Gov — public use permitted** | Preload, locking, washers, torque language, rivets/lockbolts overview |
| NASA rolling-element bearing NTRS reports (e.g. [19830018943](https://ntrs.nasa.gov/citations/19830018943)) | US gov / public use (confirm each record) | Bearing vocabulary and life concepts — not vendor catalogs |

### Materials data patterns

| Source | License (verified) | What we take |
|--------|-------------------|--------------|
| [KittyCAD material-properties](https://github.com/KittyCAD/material-properties) | **Apache-2.0** | JSON schema / common-material seed pattern |
| [Materials Project](https://next-gen.materialsproject.org/) | **CC BY 4.0** default | Computed properties with attribution; **exclude GNoME** (`BY-NC`) |
| [OQMD](https://oqmd.org/) | **CC BY 4.0** | Complement DFT data |
| [COD](https://www.crystallography.net/cod/) | **CC0** | Crystal structures if needed for education |

### Patterns (structure, not content dump)

| Source | License | What we take |
|--------|---------|--------------|
| FreeCAD wiki/docs | **CC BY 3.0** | Article shape / CAD help voice — link heavily; do not fork |

---

## LINK ONLY — point users and agents here

Great pedagogy or tables we **will not** copy into the repo. Link from SOURCES and from “Further reading” on concept pages.

### NonCommercial OERs (excellent — still NC)

| Source | License | Why link |
|--------|---------|----------|
| [UArk Jensen — Intro to Mechanical Design & Manufacturing](https://uark.pressbooks.pub/mechanicaldesign/) | **CC BY-NC** (Open Textbook Library: Attribution-NonCommercial) | Best single open survey of design + DFM chapters |
| [MIT OCW 2.72 Elements of Mechanical Design](https://ocw.mit.edu/courses/2-72-elements-of-mechanical-design-spring-2009/) | **CC BY-NC-SA** | Shafts, bearings, gears, cams, mechanisms |
| [MIT OCW 2.007 / 2.008](https://ocw.mit.edu/courses/2-007-design-and-manufacturing-i-spring-2009/) | **CC BY-NC-SA** | Design practice + manufacturing process physics |
| [VT Strength of Materials](https://engineeringmechanicsoer.github.io/StrengthBook/) | **CC BY-NC-SA** | Stress, fatigue, buckling depth |
| [Ford — Engineering Graphics and Design](https://uw.pressbooks.pub/enggraphics/) | **CC BY-NC-SA** | Drawing + intro GD&T for students |

Agents may **read** these when the user is learning; we still do not **ship** their text as our help.

### Proprietary / contractual (always link-out)

| Source | Why |
|--------|-----|
| ASME Y14.5 / Y14.41 / Y14.46 | Buy/read the standard; never paste |
| ISO 1101 / GPS (ISO/TC 213), ISO 286 fits | Same |
| MatWeb, MakeItFrom | ToS / commercial data; cite or browse, do not scrape |
| Onshape / SolidWorks / Autodesk help | Vendor docs |
| Boothroyd-Dewhurst DFA tables / software | Proprietary timing tables |
| Machinery’s Handbook tables | Proprietary |

### Historical / murky — link with a warning

| Source | Note |
|--------|------|
| NASA GSFC Drawing Standards Manual (1994) | Often PD gov work but **outdated** vs ASME Y14.5-2018 — do not teach as current |
| LibreTexts pages without a clear BY footer | Verify each page; default to link-only |
| [DOE Module 3D](https://www.energy.gov/sites/default/files/2021-07/Module_3D.pdf) | Mixed third-party credits; link-only pending verification of a selected item. Government hosting is not a copying grant for the whole deck |

### ShareAlike — copying needs its own license retained

| Source | Note |
|--------|------|
| Baughman Iowa State ME design (reported CC BY-SA) | Verify the particular artifact before copying; a reported license is not sufficient evidence |
| Mechanics Map (CC BY-SA) | Commercial copying can be permitted; preserve attribution and applicable ShareAlike terms for copies/adaptations |
| Wikipedia GD&T (CC BY-SA) | Check the article/version and separately licensed media; preserve attribution and applicable ShareAlike terms rather than relabeling copied prose LGPL |

---

## How this maps to Limo CAD topics

| Topic | Distill from | Link out to |
|-------|--------------|-------------|
| GD&T intro | NIST Part I/II | ASME Y14.5 buy page; Ford graphics (NC) |
| Fits | NIST Part II + our fit-coupon recipes | ISO 286 / ANSI preferred-fit charts |
| Fasteners | NASA RP-1228 | Vendor catalogs; UArk standard-components chapter (NC) |
| DFM | Guns NWTC + PALNI DFMA | Jensen UArk (NC); MIT 2.008 (NC) |
| Mechanisms / gears | Our recipes (`vertical-axis-turbine`) + thin original notes | MIT 2.72 / 2.007 (NC) |
| Materials vocab | KittyCAD pattern + MP/OQMD/COD (BY/CC0) | MatWeb/MakeItFrom browse |
| Drawings / PMI | NIST PMI models + product drawing docs | Standards; FreeCAD TechDraw patterns |

## Escape hatch

MCP / agent order:

1. Search `knowledge/machine-design/`
2. Open linked sources from SOURCES (including NC links)
3. Web search last — still no pasting standards into the repo

## Maintenance

Keep the source inventory and this policy in sync with the maintained
`knowledge/` corpus. The source inventory resolves stable IDs; this document
explains reuse policy. A policy or source row is not proof that a particular
artifact was copied or that all of its rights were verified.
