# Architecture proposals and decisions

The original July proposals discussed a React desktop shell, static MCP tool
disclosure and a future 3MF writer. Those starting assumptions are obsolete.
The [historical proposal](https://github.com/limo-cad/Limo-CAD/blob/0dc4a95b91ac909299fe20983592b57007a43690/docs/proposed-architecture.md)
remains available for context.

Use the maintained decisions and contracts when proposing changes:

- [One Bevy application interface](adr/0003-bevy-interface.md) defines desktop,
  command and rendering ownership.
- [MCP focus and session decisions](adr/0006-dynamic-mcp-focus.md) and the
  [MCP operating guide](agentic/STEERABLE_MCP.md) describe discovery and attachment.
- [3MF and STL export](adr/0003-3mf-export.md) records export scope and validation
  limits; [manufacturing validation](manufacturing/VALIDATION.md) tracks slicer checks.
- [Product directions](goals.md) distinguish goals from shipped behavior.

Record new proposals against the current implementation, with a concrete problem,
tradeoffs and acceptance evidence. An accepted design does not establish that
every platform or workflow has been qualified.

Shared development policy lives in [CONTRIBUTING.md](../CONTRIBUTING.md) and its
linked documents. Personal editor settings and coding-agent instructions stay
local and ignored.
