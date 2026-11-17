# Visual changes and evidence

- Inspect an existing reference before choosing model proportions, orientation or
  camera framing. Compare the result beside that reference before presenting it.
  For AI-generated images, document inferred dimensions without claiming exact CAD.
- UI PRs must include representative examples and operational GIFs showing the
  changed native flows working. Add screenshots for important states or layout
  details. Keep evidence current with the implementation, lead with it and keep
  prose concise; put detailed checks or measurements in expandable evidence.
- Distinguish component captures, isolated geometry tests and live interaction
  qualification. Record source identity and do not imply broader coverage than exercised.
- Exercise keyboard navigation in the production control hierarchy. A two-control
  fixture cannot prove traversal through the real ribbon and its neighboring tools.
- Each teaching clip should explain one operation. Reuse the same editable part and
  link prerequisites rather than repeating their construction in every clip.
- Separate cold opening, warm replay and idle costs. Measure work submitted, wakeups,
  memory bounds and release on Close. State build/profile and measurement scope.
- Do not claim zero performance impact from zero warm render jobs. Keep expensive
  work off hover/read-only Help and bound timers, workers and cached assets.
- Plan validation around coherent batches of changes. Do not run the full build/test
  suite after every small edit. Freeze the relevant source, then combine compatible
  targeted checks and native captures in the existing build lane.
- Validation is a required completion gate, not optional. Batch fixes from findings
  and rerun the affected checks. Broaden only when changed risk, failures or unresolved
  concerns justify it; preserve passing evidence and avoid redundant rebuilds.
