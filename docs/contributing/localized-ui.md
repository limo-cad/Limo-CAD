# Localized UI and teaching

- Use the existing language catalog and actual localized operation labels. A demo,
  its captions and its Help instructions should share one teaching source.
- Cover every supported locale when adding user-facing copy. Validate complete,
  nonempty translations and fallback behavior; English-only assertions are insufficient.
- Read and write source, JSON and Markdown explicitly as UTF-8. For shell pipelines,
  preserve UTF-8 or use Unicode-safe escapes. A console display is not an encoding check.
- Shape the complete authored text, including mixed scripts, through the production
  font path and reject missing glyphs. A short sample such as two Han characters
  does not prove that the whole Chinese lesson can render.
- Inspect native captures in each supported language for readable glyphs, clipping,
  wrapping and reachable navigation. Include the supported minimum window and high
  UI scale; verify scrolling reveals the final instruction above fixed controls.
- Keep fallback fonts lazy. Preserve primary Latin glyphs/metrics and the existing
  startup memory budget; avoid eagerly copying whole language or emoji collections.
- Do not substitute an HTML mockup, a successful string lookup or headless geometry
  output for evidence of the rendered native text.
