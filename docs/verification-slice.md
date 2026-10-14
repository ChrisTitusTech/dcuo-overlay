# User-verifiable character and process information

Date: 2026-10-13. This slice follows the user's request to continue beyond an
empty prototype until application and character information can be checked.
It combines bounded discovery, session UI and exact API reference work without
claiming complete Phase 0, Phase 1 or Phase 3 acceptance.

## Visible capabilities

- The managed window discovers candidate DCGAME.EXE sessions and provides explicit
  selection, Connect, Disconnect, Pause and Resume controls.
- Diagnostics displays the selected process, executable size/architecture/hash,
  successful bounded header-access evidence, application version and X11 backend.
- Character displays an exact name/world API reference: identity, power, movement,
  alignment, combat rating, skill points, reference maximum health/power and league
  where present. Unknown/null fields stay unavailable. Receipt age is shown while
  upstream cache freshness remains unknown. All fields are explicitly API sourced.
- A separate experimental section displays bounded memory-search progress and
  decoded matching name strings, labeled unverified occurrences. It does not
  equate a cached string or entered reference with active-character identity.
- Map explicitly reports missing verified location, context and calibrated assets;
  no fabricated marker or map is displayed. Settings and About are usable, with
  settings clearly limited to the current run.

## Live evidence and user acceptance

The selected game session's executable/header inspection succeeded. A resumable
search found 16 unique UTF-16 name occurrences after 14,446,592 bytes and stopped
at the candidate cap. Both the optimized CLI and interactive window displayed
the expected bounded counts; the UI decoded the observed name from remote bytes.
No addresses, raw data, personal reference payloads or screenshots were committed.

The production dcuobot-api request succeeded using the companion's application
User-Agent. The exact name and world response passed typed identity checks.
The populated Character tab was inspected visually, and the user answered
"The displayed details match" when asked to compare the displayed name, power,
combat rating and skill points with the in-game character sheet.

That confirms the first user-visible reference milestone. It does not independently
validate memory-backed stats, active-character ownership, freshness across character
switches, or location. A memory occurrence is a past bounded observation, not a
continuously refreshed live field. The current reference can be refreshed manually.

The live window retained its exact X11 instance/class/title, normal type and
managed behavior on the companion monitor. A separate nested-X smoke test checks
two startups and timed shutdown without interrupting the user's verification
window. Nested results do not replace the remaining live desktop/game matrix.

## Boundaries and lifecycle

One background worker handles discovery, inspection, reference fetching and search.
The UI stays responsive; tab switches do not create readers. Bounded command/event
queues and generation checks prevent previous-session results from populating a
new session. Event backpressure respects cancellation/shutdown instead of dropping
completion events. Quit cancels pending work and joins the worker; an active
network operation can finish within its bounded timeout before shutdown completes.

Interactive process selection preserves both PID and start time. Attach rejects
a replacement process rather than following a reused PID. Disconnect/process loss
clears build, reference and search evidence; Pause clears memory observations and
updates any cancelled pending reference status. Completed API references remain
visibly separate from memory evidence during pause.

Name discovery reads only private anonymous/heap writable mappings in chunks up
to 64 KiB. Every batch is bounded by 64 MiB and a soft one-second deadline, with
a retained cursor/overlap, 2 GiB lifetime scope and 16-candidate cap. Scope changes
invalidate rather than silently retarget the cursor. These are discovery limits,
not production polling/performance budgets.

API transport refuses redirects, bounds decoded responses to 1 MiB and total
request time to eight seconds, encodes query parameters, and rejects wrong or
ambiguous identity. Missing values do not become zero. Error messages exclude
server bodies and personal request URLs. This slice uses explicit refresh, not
automatic retry/polling or persistent response caches.

## Validation and review

- Formatting, strict all-target Clippy, workspace tests and build passed.
- 34 tests cover typed/null API fields, identity mismatches, size/transport bounds,
  redirects, memory read bounds, cursor overlap and decoding, cancellation,
  stale sessions, pause/lookup cancellation, stale process selection and shutdown
  with a full event queue.
- The optimized memory probe built and completed the bounded live search above.
- Live UI inspection demonstrated real reference values, source labels, observed
  name bytes and process/build information. User comparison passed for the fields
  named above. Personal captures remain only in ignored local storage.
- Independent `gpt-6.1-sol` review identified a stuck loading message after pause
  and loss of process start-time identity during selection. Both were fixed,
  regression-tested, and confirmed resolved by the reviewer.

## Remaining work

Prove which memory object belongs to the controlled character across switches
and three launches before accepting an active-character profile. Independently
discover and validate resources/stats, position and zone/instance/floor context.
Do not use the sixteen matching strings as a pointer profile or memory validation
of the API values. Map assets, calibration and both outdoor/indoor coverage remain
unproven. Persistent settings, automatic reattachment/retries, release packaging,
the complete desktop matrix and performance budgets remain unfinished.
