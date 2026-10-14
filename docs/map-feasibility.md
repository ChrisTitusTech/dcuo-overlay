# Map feasibility research

Research date: 2026-10-13. This is Phase 0 offline source research and an
experiment proposal, not live-game evidence. No supported map, location reader,
asset, floor rule, calibration, or accuracy/latency budget is proved here.
Both open-world and indoor/instance maps remain mandatory for the first release.
The candidate list below is not the agreed launch inventory or universal coverage.

## Primary-source findings

- The [DCUO Census collection index](https://census.daybreakgames.com/get/dcuo:v1/)
  returned eleven collections during this research: `auth_character`,
  `auth_characters_house_item`, `auth_characters_item`, `char_id_mapping`,
  `character`, `characters_active_feat`, `characters_completed_feat`,
  `characters_item`, `guild`, `guild_id_mapping`, and `guild_roster`.
  No map, zone, floor, or live-position collection is advertised by this index.
  This establishes the advertised collection set, not every field or an enduring
  guarantee that no private or future source exists.
- The [official Census documentation](https://census.daybreakgames.com/) identifies
  `dcuo:v1` as a separate namespace and says collections depend on the game.
  Its zone/map-region/map-hex example uses `ps2:v2`, the PlanetSide 2 namespace.
  It is not evidence that those collections work for DCUO. Do not derive DCUO map
  IDs from character world/server IDs or treat Census as a live-location feed.
- The [official world guide](https://www.dcuniverseonline.com/guides/guide-world-of-dcuo-iconic-locations-metropolis-gotham)
  names Gotham and Metropolis, Watchtower and Hall of Doom headquarters, and
  describes the in-game full map/minimap. It establishes candidate locations and
  an in-game visual reference, not a downloadable, licensed map dataset or a
  coordinate transform. Its age means current accessibility must be checked live.
- [Game Update 103](https://www.dcuniverseonline.com/update-notes/dcuo-game-update-103)
  separately identifies the BoP: Metropolis open world and BoP: LexCorp Tower
  solo, plus Volcano Mining Facility, Clock Tower, and Fire & Brimstone instances.
  Historical normal/event/elite distinctions are a reason to preserve variant
  identity, not proof of current availability or shared layout/coordinates.
- The [Daybreak Terms of Service, sections 14.1-14.4](https://www.daybreakgames.com/terms-of-service)
  describes permitted UGC and a limited conditional license, including fan sites
  and mobile apps; it preserves ownership, restrictions, attribution notices,
  and other usage conditions. This research does not establish that bundling a
  particular DCUO map in a native desktop companion falls within those grants.
  The [EULA, sections 2-3](https://www.daybreakgames.com/eula?locale=en_US)
  also distinguishes personal software use from ownership of artwork and other
  game property. Asset-specific permission remains unresolved. These are source
  observations, not a legal conclusion or an authorization to acquire assets.

## Asset routes and unresolved provenance

No reusable DCUO map package with established permission, dimensions, checksum,
and matching context was found in the primary sources inspected. This is a
bounded research result, not a claim that none exists anywhere.

| Candidate source | What is established | Remaining gate |
| --- | --- | --- |
| Official in-game map | Official guide describes its existence | Exact image/layer availability, acquisition method, rights and matching context |
| Official website/manual illustrations | Public primary reference material exists | No calibrated map dataset or asset-specific redistribution permission proved |
| Census | DCUO namespace and current collection list | No advertised map asset/location collection; do not import another game's API |
| User-supplied local image | Possible application design route in SPEC.md | Provenance, permitted use, dimensions/checksum, privacy and calibration still required |
| Original schematic | Possible experiment substrate | Must accurately represent tested landmarks and have established rights; synthetic geometry alone does not prove live support |

Do not commit proprietary maps, client assets, personal screenshots, or dumps.
Local-only status prevents bundling; it does not itself establish permission to
extract or use an asset. A catalog record needs source URL/author, acquisition
date/method, applicable license or written permission reference, notices,
distribution status (`unresolved`, `local-only`, or `bundle-permitted`), and a
rights-review date. Keep unresolved assets out of distribution. Obtain or identify
a permitted usable substrate before claiming the map milestone complete.

## Explicit unsupported coverage inventory

All rows are unsupported today: assets, build-specific location/context readers,
layers, calibration and controlled-route acceptance are unproved. Names are
research candidates grounded in the official sources above. Unknown layers must
stay unknown; do not invent floor counts from appearance or height.

| Candidate area/category | Primary source | Context/coverage still unresolved |
| --- | --- | --- |
| Gotham, open world | World guide | Current zone key, variants, extent and layers |
| Metropolis, open world | World guide | Current zone key, variants, extent and layers |
| Watchtower, indoor headquarters | World guide | Faction access, layout/floor keys, transitions |
| Hall of Doom, indoor headquarters | World guide | Faction access, layout/floor keys, transitions |
| BoP: Metropolis, open world | Update 103 | Distinguish from base Metropolis; current availability |
| BoP: LexCorp Tower, solo instance | Update 103 | Current access, normal/event variants, layout/floors and re-entry |
| BoP: Volcano Mining Facility, alert | Update 103 | Current access, variants, layout/floors and re-entry |
| BoP: Clock Tower, raid | Update 103 | Current access, normal/event/elite variants, layout/floors and re-entry |
| BoP: Fire & Brimstone, raid | Update 103 | Current access, normal/event/elite variants, layout/floors and re-entry |
| All other areas and any unlisted variant/layer | Not inventoried | Unsupported until individually researched and proved |

The lead must declare the actual first-release inventory with both categories.
Selecting a smaller experiment pair does not automatically remove other areas
from an agreed launch inventory or move instances into a later phase.

## Minimal bounded outdoor and indoor experiment

Only the lead's scheduled live operator performs game/desktop actions. Start
with accessible base Gotham or Metropolis outdoors and an accessible solo
instance indoors; BoP: LexCorp Tower is a sourced candidate, not a commitment
that the current character can enter it. A headquarters can be an additional
indoor probe but cannot substitute for instance exit/re-entry acceptance.

1. Record executable fingerprint, profile revision, process/session generation,
   runtime and sanitized character binding. Establish a bounded read-only
   candidate position and context path. Reject unsupported builds, ambiguous
   addresses and mixed context before any projection.
2. At a fixed landmark, keep the character still while rotating the camera and
   changing target. Then move the character along two independent horizontal
   directions and vertically where possible. Record which coordinates change;
   prove controlled-player ownership, horizontal axes, units/orientation, and
   whether coordinates are global or local. Identity matching alone is inadequate.
3. Record outdoor area and indoor instance/layout/floor keys before and after
   loading. Compare entry, exit and re-entry. Test distinct levels with overlapping
   horizontal coordinates if accessible. Establish a floor rule from evidence;
   if no discriminating key/rule is proved, hide the marker for that context.
4. Use an approved local asset or other permitted substrate. Collect at least
   three non-collinear landmark pairs for an affine fit; prefer four or more
   spread across the map. Reserve at least three separate landmarks for validation,
   including distant and edge locations. Record uncertainty in image picking and
   sample timing. Fit once; do not quietly move validation points into training.
5. Report error at each held-out landmark in pixels at native image resolution
   and as a fraction of map width, plus world units only after units are proved.
   Report maximum/RMS error, sample-to-render age and measured movement-to-marker
   delay separately. Set provisional accuracy/latency thresholds from feasibility
   measurements before later acceptance; no thresholds are invented here.
6. Repeat the pair over three launches and instance re-entry. Exercise floor
   change, loading/teleport, death/respawn, character/session switch, restart,
   pause/stale data, and wrong-map browsing. Each invalidation must hide the live
   marker until valid current context and matching calibration return. API-offline
   operation must retain independently valid local location support.

Stop the experiment with a precise blocker when ownership, context, permitted
assets, or calibration cannot be established. Independent shell work may proceed
with fixtures, but an outdoor-only or synthetic result does not complete live maps.

## Proposed calibration/evidence record schema

This is a documentation proposal, not a shared application interface.

| Record | Required fields |
| --- | --- |
| Identity | `schema_version`, `record_id`, `category`, human area label, proven zone/variant/layout/floor match keys and their evidence references |
| Compatibility | executable fingerprint, memory-profile revision, map-catalog revision, supported runtime/build evidence |
| Asset | provenance/permission fields above, asset version/checksum, native width/height, local asset reference, distribution status |
| Projection | coordinate-axis meaning, proven units, coordinate bounds, image pixel origin convention, affine 2x3 matrix, floor/layer rule and ambiguity rejection |
| Landmark | sanitized label, fit/validation role, world coordinates, image pixels, uncertainty, monotonic sample time and coherent session/zone generation |
| Validation | held-out errors, maximum/RMS error, time/latency measurement method, provisional budgets, routes/transitions/launches tested and outcome |
| Support | `unproven`/`supported`/`disabled`, missing gates, approved coverage extent, last verification, invalidation conditions |

Keep precise live coordinates and character/process identifiers in private local
evidence, outside committed diagnostics. Commit synthetic coordinates and a
sanitized aggregate outcome with provenance references. Record timestamp clocks
and transform conventions so later replay tests are reproducible. Version assets
and calibrations separately from memory profiles and reject unmatched versions.

## Research checks and limits

Read AGENTS.md, SPEC.md, ROADMAP.md and TASKS.md and inspected existing changes.
The Census index was fetched directly with successful HTTP completion and its
eleven names inspected; official documentation, guide, update notes and terms
were read through public web sources. No map images, proprietary client files,
personal snapshots, game interaction, memory reads, implementation or commits
were performed by this worker. No live-location/asset/floor/calibration result is
asserted. Required live feasibility and release inventory decisions remain open.
