# Build, teach and show through the same CAD interface

Executable design recipes are a product capability. One reviewed source should
build an editable design at maximum rate, explain its decisions while stepping,
and show the same construction with authored captions and camera transitions.
Rendering changes the presentation, not the modeling operations or final checks.

## Review and merge order

The [native GitHub stack shown in PR #111](https://github.com/jackControls/noBS-CAD/pull/111)
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
13. PR108: printable definition exports with atomic document ownership.
14. PR109: remove unused component placeholders for consumed construction bodies.
15. PR110: the D-screw vise, fit coupon, print layout and manufacturing drawings.
16. PR111: the vertical-axis turbine, four fit coupons and assembly/part drawings.

Each PR's diff is against the layer below it. GitHub applies main's review rules
to every layer and can merge a reviewed prefix from the bottom. A draft layer
does not prevent a ready prefix below it from landing. Use the native stack
merge control; an ordinary feature-branch merge is not the release workflow.

The recipe layers can be reviewed in parallel against their immediate parents;
merge the approved prefix in order. PR99 remains draft for the interface and
native-preview decisions documented in `script-interface-review.md`. Its status
does not erase the independently reviewable geometry and recipe work above it.
No approvals or repository protection rules are bypassed by using a stack.

PR88 remains an independent diagnostics change against main. When it lands,
carry its session-generation logic into the extracted MCP library while updating
this stack; do not restore an older entire entrypoint over those fixes. The old
PR89 examples are superseded by the native recipes and Rust regressions in the
collection, with their old branch/history preserved.

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

Single-feature lessons use the same versioned JSONC format and catalog. Prefer a
small, clear part that demonstrates a modeling decision, a meaningful parameter
edit and the result. Extend the supported Rust runner and ordinary product
operations when a capability is missing; keep one execution path.

Use focused geometry and edit/replay/restore checks alongside recipes. Add them
to the existing cargo/CI entry points, without a parallel runner or percentage
coverage gate. Recipe-only changes are desktop build inputs because the app
embeds their sources. Interface layout and native preview work remain draft
until reviewed; geometry checks alone do not validate the teaching experience.
