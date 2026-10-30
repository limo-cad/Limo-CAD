# Plunge-milling research draft

Status: research only, 2026-10-20. No CAM implementation, machine program or
machining qualification is included. This note replaces the local working
reports as the material proposed for public review; those reports and their
source downloads are not included.

## Questions for Limo CAD

The local literature search covers plunge-point placement, pocket-wall spacing,
axial layering, tool orientation, linking moves, remaining stock, cutting forces
and stability. Treat these as separate planning questions rather than expecting
one optimization objective to resolve all of them.

| Research question | Reading leads | What a future evaluation should measure |
| --- | --- | --- |
| How should plunge locations cover a pocket containing islands? | R1 | Uncut volume, boundary clearance and number of plunges |
| How should adjacent cuts control material left at a pocket wall? | R2 | Maximum remaining wall material, plunge count and sensitivity to curvature |
| How does planning change for a multi-axis blisk channel? | R3, C1 | Tool/holder clearance, axis motion and reachable stock |
| How do force and vibration constrain the candidate path? | R4, R6 | Predicted load, stability assumptions and measured behavior |
| How should feed vary along a path? | R5 | Cycle time, load variation and machine limits |

These are project-authored evaluation questions, not reproduced algorithms or
claims that the papers establish production readiness. Reference topics below
are based on bibliographic records or the expressly identified public abstract;
full method details have not been revalidated for this draft.

## Proposed investigation

Start by defining a small set of synthetic pockets with known stock and intended
remaining allowance: a rectangle, a pocket with an island, and a concave pocket.
Keep multi-axis impeller work as a separate investigation because axis motion
and holder clearance introduce additional constraints.

For each candidate planning approach, record the input stock, tool envelope,
allowance, numerical tolerance, and machine constraints. Compare cutting travel,
retract/link travel, plunge count and remaining-stock estimates separately. A
shorter connecting path alone does not establish a shorter machining cycle.

Geometry checks should include the complete tool/holder envelope, boundaries,
entry/retract motion and stock remaining after each operation. Any future cutting
trial needs its own setup and validation; this draft supplies no speeds, feeds
or executable toolpaths.

No algorithm is selected here. The next review should establish which sources
can be consulted under documented access terms and which proposed methods need
patent analysis before an implementation is chosen.

## Reading map and evidence

See [References](REFERENCES.md). R1–R5 were checked against Crossref metadata on
2026-10-20. R6 and C1 have publicly accessible publisher abstracts. Metadata
verification establishes a publication's identity, not correctness, permission
to redistribute its contents, or validation of its algorithm.

The earlier local notes included numerical performance claims, detailed
algorithm reconstructions and patent-status assertions. Those have not been
carried into this public draft. No publisher PDFs, extracted full texts,
figures, abstract copies, pseudocode, third-party source code or retrieval
scripts are attached. Bibliographic titles and identifiers are retained for
attribution and discovery.

## IP scope and open questions

This is a freshly written reading map, not a claim of clean-room development or
freedom to operate. The fact that a method is described in a paper or public
patent record does not establish permission to implement it. Writing new code
also does not resolve patent questions.

For copyright background, the [U.S. Copyright Office's Circular 33](https://www.copyright.gov/circs/circ33.pdf)
distinguishes methods and facts from their authored expression. For patent
background, the [USPTO's explanation of patent rights](https://www.uspto.gov/patents/basics/manage)
describes exclusion rights. These U.S. references are orientation, not a legal
assessment of a proposed Limo implementation or other jurisdictions.

Before adapting a specific method, document the source, lawful access and any
relevant reuse terms. Have a qualified patent professional assess the proposed
implementation against relevant claims, families, jurisdictions and current
register records where needed. The candidate records in the bibliography are
search leads only: this draft makes no finding of infringement, validity,
expiration, abandonment or permission to practice. The search is not exhaustive.
