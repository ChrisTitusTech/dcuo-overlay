# Repository conventions

- Read SPEC.md, ROADMAP.md, and TASKS.md before changing implementation scope.
- Inspect current changes and preserve unrelated work.
- Keep the companion application in Rust; API service changes belong in dcuobot-api.
- Target a normal DWM-managed X11 window on a separate monitor from the game.
- Keep memory access read-only and isolate unsafe operations behind bounded,
  testable interfaces. Do not introduce injection or gameplay automation.
- Reject unsupported game builds and ambiguous profiles rather than guessing.
- Track validity and API comparison per field. Never label all memory verified
  because character identity matched.
- Bind map location to the current session and proven zone/layout/floor context;
  show a live marker only with valid coordinates and a matching calibrated map.
- Do not commit secrets, personal game snapshots, full memory dumps, or proprietary
  game binaries. Prefer synthetic fixtures or minimal sanitized observations.
- Keep documentation and task status aligned with demonstrated capabilities.
- Use simple ASCII punctuation and small, coherent commits after relevant checks.
- Use subagents only when the user or applicable instructions explicitly request them.
- Respect plan-only milestones and explicit user stop points. Publication and
  release actions require explicit authorization.
