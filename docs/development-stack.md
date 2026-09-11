# Build, teach and show through the same CAD interface

Executable design recipes are a product capability. One reviewed source should
build an editable design at maximum rate, explain its decisions while stepping,
and show the same construction with authored captions and camera transitions.
Rendering changes the presentation, not the modeling operations or final checks.

## Review and merge order

The [native GitHub stack shown in PR #115](https://github.com/jackControls/noBS-CAD/pull/115)
is the current source of truth for its order, approvals and CI. Its layers are:

1. PR95: associative drawing dimensions.
2. PR96: solved assembly export and isometric fit.
3. PR97: guarded application exit.
4. PR98: bench joinery, persistent references and interference inspection.
5. PR100: the shared Rust command-script interpreter and editor schema.
6. PR99: MCP/native adapters, source loading and live presentation.
7. PR102: authored parts and assemblies, shared catalog,
   teaching notes, camera directions and independent validation.
8. PR103: driven mechanism coordinates and persistent gear relations.
9. PR104: native associative drawing exports and placed assembly views.
10. PR105: successful-history reuse, patterned joins and mesh-export isolation.
11. PR106: exact authored coordinates, external-thread commands and script preflight.
12. PR107: protected drawing references, exact inspection reuse and source markers.
13. PR108: printable definition exports, rejected-Open recovery and atomic file ownership.
14. PR109: remove unused component placeholders for consumed construction bodies.
15. PR110: the D-screw vise, fit coupon, print layout and manufacturing drawings.
16. PR111: the vertical-axis turbine, four fit coupons and assembly/part drawings.
17. PR112: shared live material presets, retained replay output and camera completion.
18. PR113: one grouped control for retained construction-reference visibility.
19. PR114: truthful final playback counts and stable completed-run feedback.
20. PR115: script and presentation ownership across whole-document replacement.
21. PR116: validation evidence and remaining release work.

Each PR's diff is against the layer below it. GitHub applies main's review rules
to every layer and can merge a reviewed prefix from the bottom. A draft layer
does not prevent a ready prefix below it from landing. Use the native stack
merge control; an ordinary feature-branch merge is not the release workflow.

The recipe layers can be reviewed in parallel against their immediate parents;
merge the approved prefix in order. PR99 remains draft for the interface and
native-preview decisions documented in `script-interface-review.md`. Its status
does not erase the independently reviewable geometry and recipe work above it.
No approvals or repository protection rules are bypassed by using a stack.

On September 10, GitHub rejected the asynchronous PR95–98 prefix merge for a
qualifying write-access approval even though all four exact-head Jack reviews
were approved and his admin permission was confirmed. No merge or branch rewrite
occurred in that attempt; the precise server-side cause is unconfirmed. The
evidence is recorded with issue #14. The subsequent review correction moves the
approved rejected-Open fix from PR115 into PR108, so the requested change is fixed
in its owning layer. PR115 now contains the separately reproduced script/document
ownership corrections. Review history is retained, and changed heads need fresh
approval.

PR88 merged into main as `39eb862`. The stack is rebased onto that revision, with
its session-generation logic retained in the extracted MCP library and its thin
entrypoint preserved. A native regression combines deferred script snapshots with
observational session status: reading status cannot advance the loaded fence or
reconstruct the model; a failed refresh cannot stamp the new fence. The old
PR89 examples are superseded by the native recipes and Rust regressions in the
collection, with their old branch/history preserved.

The [September 10 correction evidence](review-corrections-2026-09-10.md)
records the current rebuild, standalone PR108 checks, full live bench replay,
and the saved-file regression that exposed lost materials during temporary
history rollback. Its shared history prerequisites live in PR108; PR115 adds
the corresponding MCP inbox refresh.

## Keep developing from the current tip

Start independent fixes on current main. Start changes that consume pending
replay capabilities on the current stack tip, and give each cohesive recipe or
capability change its own PR. Reuse existing PRs when updating a layer. Fetch
main and cascade changes through descendants before publishing, preserving
reviewed behavior and resolving conflicts in their actual owner.

Use GitHub's **Rebase stack** control or the official
[stack workflow](https://docs.github.com/en/pull-requests/how-tos/create-pull-requests/managing-stacked-pull-requests)
to update the chain. Verify each parent is an ancestor of its child and no merge
commits remain between adjacent layers. Retain recoverable refs before a rewrite,
use exact branch leases when pushing, and accept renewed approval when required.
Do not change protection rules to get a stack merged.

## Recipes are the learning and demonstration source

The selected reference targets are the garden bench, a printable vertical-axis
turbine with an integrated motor/generator, and a functional screw vise. Their
construction sources,
editing checks, editable drawings and teaching notes belong together. See
[recipe development](recipe-development.md) for actual readiness and the next
work. All three now have runnable native sources. The two new manufacturing
candidates include editable drawings and fit coupons; physical qualification
and the bench's full drawing package remain open.

Both new candidates also completed attached, rendered construction in a rebuilt
desktop at `6f7e5e6`: the vise passed 692 steps and 55 final checks, and the turbine
passed 1,622 steps and 11 final checks. Each matched its independent native model,
scene, sketches and solved assembly exactly before Save As. Separate clean-view
copies changed only retained-reference visibility and the saved document name.
The validation manifests retain the original full headless baseline and this
additional live evidence rather than replacing one with the other.

Single-feature lessons use the same versioned JSONC format and catalog. Prefer a
small, clear part that demonstrates a modeling decision, a meaningful parameter
edit and the result. Extend the supported Rust runner and ordinary product
operations when a capability is missing; keep one execution path.

Use focused geometry and edit/replay/restore checks alongside recipes. Add them
to the existing cargo/CI entry points, without a parallel runner or percentage
coverage gate. Recipe-only changes are desktop build inputs because the app
embeds their sources. Interface layout and native preview work remain draft
until reviewed; geometry checks alone do not validate the teaching experience.
