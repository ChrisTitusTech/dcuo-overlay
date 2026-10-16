# Repository conventions

- Read SPEC.md, ROADMAP.md, and TASKS.md before changing implementation scope.
- Inspect current changes and preserve unrelated work.
- Keep the companion application in Rust; API service changes belong in dcuobot-api.
- Target a normal DWM-managed X11 window on a separate monitor from the game.
- Keep memory access read-only and isolate unsafe operations behind bounded,
  testable interfaces. The user explicitly authorized an in-process Broker search
  extension on 2026-10-16; implementation requires proven build, call arguments,
  object lifetime and execution-thread contracts. All existing readers remain
  read-only. Purchases, sales, cancellations and other automation remain out of scope.
- Reject unsupported game builds and ambiguous profiles rather than guessing.
- Track validity and API comparison per field. Never label all memory verified
  because character identity matched.
- Bind map location to the current session and proven zone/layout/floor context;
  show a live marker only with valid coordinates and a matching calibrated map.
- Do not commit secrets, personal game snapshots, full memory dumps, or proprietary
  game binaries. Prefer synthetic fixtures or minimal sanitized observations.
- Keep documentation and task status aligned with demonstrated capabilities.
- Use simple ASCII punctuation and small, coherent commits after relevant checks.
- Respect plan-only milestones and explicit user stop points. Publication and
  release actions require explicit authorization.

## Development workers

- The user requests extra workers where useful. Delegate bounded, independent
  research, implementation, tests, or review within the currently authorized
  phase using the assignments in ROADMAP.md. Keep trivial or dependent work serial.
- Explicitly select `gpt-6.1-sol` for each spawned worker. Do not silently replace
  that model if unavailable; report the limitation and continue suitable lead work.
  This does not change the lead agent's model.
- Use at most three extra workers alongside the lead, subject to any lower runtime
  limit. The lead owns spawning and scheduling; workers do not spawn more workers.
- Agree shared interfaces first and assign non-overlapping file ownership, a
  bounded deliverable, required evidence, and a stop point before each assignment.
  Workers request shared-contract changes through the lead.
- The lead owns shared scaffolding/manifests, integration, plan/status updates,
  Git staging/commits, and final checks. Schedule one operator for live game and
  desktop experiments; other workers can analyze sanitized evidence offline.
- Worker completion is input to review, not phase acceptance. Preserve all live
  evidence gates and the user's stop points. These are development agents, not
  permission to create duplicate application readers or game sessions.
