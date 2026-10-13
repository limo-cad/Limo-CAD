# ADR 0001 — Rust for the shared engine and platform-neutral features

- Status: Historical proposal; interface ownership superseded by [ADR 0003](0003-bevy-interface.md)
- Date: 2026-07-27

## Decision (proposed)

1. Implement shared engine, export, MCP, and project-I/O behavior in Rust when
   it must serve more than one host, then expose it through the existing
   bridges.
2. Treat Windows and Linux as first-class native targets alongside macOS;
   track remaining packaging gaps in issues.
3. Keep OCCT as the B-rep backend; Rust owns planning/history around it.

## Consequences

- Contributors default to Rust for shared semantic model changes.
