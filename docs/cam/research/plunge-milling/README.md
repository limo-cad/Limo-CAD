# Plunge-milling research draft

Status: research only; bibliography/access refresh 2026-11-17. Author-held Draft.
No CAM implementation, machine program or machining qualification is included. This note replaces the local working
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
2026-10-20 and rechecked on 2026-11-17; R6 metadata was also checked in Crossref
then. R6 abstract evidence is inherited from 2026-10-20 (publisher access
returned HTTP 403 on refresh); C1 public journal metadata/abstract was revisited.
Metadata verification establishes a publication's identity, not correctness, permission
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

## Author decision brief — 2026-11-17

This refresh preserves the research scope and selects no machining algorithm.
The next author review can make three separate decisions:

1. **Source access.** For each proposed method, record the reference ID, exact
   edition/DOI, access route (publisher, institution, author-provided manuscript
   or licensed repository), access date and applicable terms. R1–R6 currently
   have metadata-level identity checks; no full-text method assessment is
   established. R6's earlier abstract assessment does not establish current
   access. C1's public abstract is accessible, but its year/volume remains
   unresolved. A visible PDF link or Crossref text-mining license field alone
   does not establish reuse or redistribution permission. Seek permitted access
   before method study; retain links and original assessment notes here.
2. **Method evidence.** After permitted reading, tie each claimed method
   property to a source location and distinguish the authors' reported results
   from an independently reproduced result. Record assumptions, required inputs,
   failure cases and unavailable details. Apply the synthetic-pocket comparison
   above only after this assessment. Record remaining stock and clearance as
   well as travel; do not rank candidates from titles or abstract performance
   figures.
3. **Implementation-specific patent questions.** Before selecting a method,
   give a qualified patent professional a concrete proposed implementation:
   plunge-location construction, wall-spacing rule, axis/orientation strategy,
   stock update, feed/stability model and linking/entry behavior. Ask which
   claims could concern those steps, what claim limitations distinguish the
   proposal, which related families and jurisdictions require investigation,
   and which current official register records are needed. The bibliography's
   13 leads are unassessed search inputs; no mapping of a lead to an algorithm,
   complete search, current status or clearance is established.

Author decisions still needed: choose the initial evaluation scope (the proposed
2½-axis pocket set or a separately specified multi-axis study), assign lawful
source-access followup, resolve C1's citation, and decide which fully described
candidates warrant method and patent review. Preserve Draft until the author
changes that instruction. No cutting trial, implementation or publication
readiness follows from completing this bibliography.
