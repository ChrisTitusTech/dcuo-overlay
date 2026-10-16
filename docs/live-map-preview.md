# Live map preview

The companion now has a working experimental map preview for the observed client
build. This is a vertical slice of Phases 1, 2, 3, 4 and 5, not completion of their
acceptance gates. Current-character identity and the multi-launch memory profile
remain unverified. API references retain their independent source labels.

## Implemented behavior

- `dcuo-companion` opens the normal managed X11 application. The old
  `dcuo-window-probe` entry point remains for existing smoke commands.
- One bounded read-only sampler follows the reflected local-player/controller/pawn
  chain. It requires the exact executable hash, PE metadata and expected reflected
  field schemas. Unsupported builds fail closed. Health, power and position are
  explicitly experimental, with age and unavailable states.
- Area selection uses visible loaded audio and designer streaming packages.
  Persistent parent-world names are not treated as the current indoor area.
  A unique bounded area key with matching designer evidence is required; the
  former two-area allowlist is removed.
- Map artwork is imported from the user's installed client into private local
  files. No proprietary images or package data are bundled or committed. The
  bounded importer verifies package structures, tile grids, decompression sizes,
  cache hashes and image checksums. See [format and provenance](local-map-import.md).
- The Map tab provides local browsing, live experimental markers, zoom, pan,
  fit and follow/recenter. Context, age, session changes and pause state gate the
  marker. UI repaint timing respects sample expiry even with slow idle refresh
  preferences. Current area automatically loads installed main overviews as the
  area changes, even while another tab is open. All 313 installed overviews pass
  import checks. Missing maps and ambiguous context have no marker. Separate
  floors and universal live alignment remain unvalidated.
- Settings, theme, scale, UI refresh interval and map catalog persist in XDG
  configuration. Corrupt preferences recover to defaults with an explanation.
  Optional reconnect is restricted to a unique session of the same exact build.
- API calls use a separate bounded worker, so a delayed reference request cannot
  stall local sampling. Shutdown joins the workers; HTTP has an eight-second
  bound. Sanitized diagnostics exclude names, positions, process IDs and URLs.

## Observed evidence

On Fedora 44, DWM-Titus 0.7.2, X11/Picom and GE-Proton 10-34, the managed companion
was displayed on HDMI-0 while the game ran under Gamescope on DP-0. The real window
retained normal type, stable class and `override_redirect=false`.

Local import decoded all 600 Gotham tiles and all six indoor tiles, assembled
consistent artwork and passed catalog/checksum loading. Both packages specify
100 world units per source pixel. Tile indices require transposition and reversed
row placement, with padding retained. Gotham is downsampled by two to bound GPU
and CPU image memory.

A paired live observation at the Old Gotham Subway entrance in Gotham placed the
companion marker at the same edge and neighboring buildings as the game's marker.
Zoom and Follow were exercised in the real companion window. The user then
confirmed that normal movement is tracked in the correct place. A live Pause
check removed the marker immediately. The game then changed to an unsupported
instance: resources recovered but the marker correctly remained unavailable. Warm native samples
with the outdoor streaming array took approximately 0.22-0.24 seconds; first-sample
schema discovery took about 1.3 seconds. The initial implementation consumed approximately 85% of one CPU core; this is
not acceptable as a polling baseline. After bounded scatter reads, warm samples in a different indoor area took
39-41 ms in the debug build and 9-10 ms in the optimized build. The optimized application used 7.8% of one CPU core in a ten-second check in
that instance. Pause/Resume cleared and restored resources. Outdoor and
extended-session comparison remains pending. The optimization target is warm samples
below 50 ms and average companion CPU below 10% of one core at four samples per
second, with fresh marker age below one second. These are provisional targets,
not passed acceptance or an extended-session performance bound. Indoor artwork matches the in-game layout;
indoor live-marker accuracy still needs an independently paired observation.

Personal screenshots, coordinates, memory observations and imported assets are
kept outside Git. No game memory writes, injection or gameplay automation are used.

## Automated checks

The integrated slice passed `cargo fmt --all -- --check`,
`cargo clippy --locked --workspace --all-targets -- -D warnings`,
`cargo test --locked --workspace` (71 passed, one opt-in local import ignored),
`cargo build --locked --workspace`, the optimized workspace build, and
`cargo test --locked --no-default-features --lib` (58 passed).
`xvfb-run -a python3 scripts/check_x11.py` passed two normal-window launches and
clean exits. Rust 1.95 minimum-version check and strict all-target Clippy also
passed. The opt-in importer was separately exercised twice with installed
assets; private results are not fixtures. Independent review and remote CI are
recorded in the PR rather than inferred from these local checks.

## Remaining acceptance

- Multiple independent landmarks in both areas and indoor movement comparison.
- Indoor/outdoor switching, repeated instance entry, floor/teleport behavior and
  three fresh game launches, including unsupported-build rejection.
- Current-character identity binding and per-field semantic/API comparisons.
- Full monitor, fullscreen, compositor, keyboard and display-recovery matrix.
- Extended session, measured overhead, automatic reconnect and release packaging.
- Project license selection before release distribution. No release is published.

## Reproduce locally

```bash
cargo build --locked --workspace
cargo run --locked --bin dcuo-companion
```

Choose the game session and Connect. In Map, expand the installed-map importer,
confirm the local CookedPC directory, and import. Imported images remain private
local data. The marker is labeled experimental; a matching API name does not
promote it or any other memory field to verified.

## Restarted session and indoor acceptance (2026-10-14)

After the user relaunched the game, the exact supported executable build passed
read-only attachment and fresh controlled-pawn sampling. The current visible
streaming context was MHDT0107 with matching audio/designer signals. Its installed
map is a single-tile indoor floor plan. The companion selected that entry, showed
a fresh experimental marker within the floor plan, and the user explicitly
confirmed: "Yes, the map and marker match" in response to the movement check.

The expanded private catalog also includes MPYR0242's six-tile main overview.
Its live alignment, auxiliary layouts and floor handling remain unvalidated.
The catalog retains experimental calibration confidence for both new entries;
qualitative user confirmation does not replace independent landmark/error budgets.
No game inputs, memory writes, personal snapshots or proprietary assets were
committed. Screenshots and imported artwork remain private.


## Automatic loading acceptance (2026-10-14)

With saved catalog/source cleared, the new release connected to the existing
supported game process, detected its CookedPC installation, and imported MADV155
in the background while Character was selected. Opening Map showed its 2048 x
2304 overview and a fresh experimental marker. This area was outside the former
four-map set and required no manual import or code change. Its precise alignment
has not been independently measured. The earlier MHDT0107 confirmation remains
qualitative evidence for that indoor map, not all instances.

All 313 installed main overviews passed corrected import/cache validation.
Synthetic tests cover rapid area changes, cancellation, stale/late results,
manual browsing, source changes, retry and large-grid downsampling. Real loading
transitions, auxiliary floor selection and quantitative calibration across every
instance remain live acceptance work. Private screenshots were inspected locally
and are intentionally excluded from publication.

The installed release was also checked through companion Pause -> Browse ->
Current area -> Resume. Browse restored the overview while paused without a
marker; resuming restored fresh live tracking. No game input was involved.

## Original nearby blue collectible preview (2026-10-14)

The user identified a nearby blue collectible. Bounded read-only investigation
found a matching magnifying-glass asset on the controller's potential interaction
candidate, with its own world position. The application shows a blue diamond ring
at the object's projected location; the yellow circle remains the player. The
hollow ring keeps both visible at close range. The legend/tooltip explain the
experimental scope.

This displays only the currently exposed nearby blue candidate. It does not
search the whole zone, persist a spawn database, establish collection availability,
or claim support for other colors. Changed, hidden, deleted, wrong-world, distant
or stale observations are omitted. All existing map/area/profile gates apply.
No interaction with the collectible or game-memory writes are performed.

The user explicitly confirmed that the displayed blue diamond matches the spawn.
This acceptance is for the nearby blue candidate, with the limitations above.

## Exposed ground spawn layer

The Map tab's Ground spawns switch enables bounded discovery of recognized live
spawn actors beyond the controller's nearby interaction candidate. The preference
is saved and defaults to off. Progress and any discovery limits appear in a fixed
status row; markers use the same area, freshness and calibration gates as the
player. Discovery supports the installed magnifying-glass color definitions and
question-mark/seasonal variants. Only blue has live user acceptance so far.

This supersedes the nearby-only display described above. It does not show unloaded
objects, establish collected/uncollected status, or persist a location database.
An exact definition match alone is insufficient: objects must also pass current
world, identity, position and lifetime validation.

Installed validation found two exposed blue actors and displayed both diamonds.
Off removed the diamonds without moving or clearing the map or player marker;
the changed map pixels were confined to the marker region. Preference persistence
and the default-off behavior passed. Other colors still need live acceptance.

## Exobyte follow-up

The user identified a nearby exobyte spawn. The current potential interaction
actor resolves to FXSM103_DAMAGEABLEOBJECT in package DFXSM103, with FXSM103 as
its starting mesh. The previous name-only allowlist deliberately excluded this
generic effect name; the new exact definition/package classification recognizes
it as an experimental exobyte marker. Orange diamonds share the existing toggle
and live actor/world/freshness checks. Other packages with the same generic name
remain unsupported. No collection-status or unloaded-spawn claim is made.

The installed build displayed five orange and two blue markers in the current
loaded area. The nearby orange marker matches the potential interaction actor
next to the character. This is lead-observed rendering and memory evidence, not
a new user-confirmed alignment or collected-state acceptance.

## Stable cached spawn locations

User reports disappearing markers during scanning. Retain bounded in-memory
last-seen locations, with expiry after two consecutive completed scans miss a
location. Partial scans, temporary read gaps and interrupted passes do not count.
Detection resets the miss count. Historical markers retain map/context/profile
checks but do not claim fresh availability; player freshness remains unchanged.
Cache clears on confirmed process/controlled-owner/area changes, not on ordinary
streaming or sample generation changes. No locations are persisted to disk.

Live evidence: four completed reader scans were published despite transient
client changes. In MLOC136, seven cached markers remained through a missing
sample and Pause. Toggling off/on while paused restored the exact map-region
pixels without another scan. The two-miss expiry rule is covered with controlled
synthetic scans; no game interactions were performed to force despawns.

## Correction: generic effects are not exo identification

The user reported many non-pickup orange markers. A bounded current-session
inventory found 33 actors sharing FXSM103_DAMAGEABLEOBJECT / DFXSM103, while the
explicit green collectible remained distinct. Replicated mesh pointers did not
provide a distinguishing name. This supersedes the earlier broad exobyte rule:
that definition/package pair is not sufficient evidence for an exo node.
Automatic discovery now accepts only blue/green/gold magnifying-glass definitions
in DDSGPROGEN107 with a Package outer; shared effects and unverified grey/purple/
seasonal variants are rejected. Exo detection is unavailable pending paired
positive/negative validation. Read-only memory and cache expiry rules are unchanged.

## Audio-free alternate Gotham area

A current-world observation found MGOT103_ALT_US with visible
MGOT103_DESIGNERDATA but no audio stream. The prior audio/designer pair rule
could never identify it. The bounded world ownership chain resolves WorldInfo
(instance 1), PersistentLevel, TheWorld and the package (remaining instances 0).
The exact observed package alias now selects the installed MGOT103 map only
when that validated chain and sole matching designer signal agree. Conflicting
or present audio signals retain the original rules; unknown aliases are not
normalized or guessed. Chain identity is resampled and included in the sample
context. Historical cache survives missing chain evidence but rejects a changed
known chain. A retained background now explicitly says Last-known map when its
current context cannot be established.

## Exo-material crafting-effect discriminator

The user provided a new nearby exo-material node. Bounded read-only inspection
confirmed the generic FXSM103 definition again, but also identified an owned
ParticleSystemComponent in AllComponents using ParticleSystem asset
FXGEN109_FX_BAUBLE_CRAFTING in package DFXGEN109. Template and package name
instances are zero, and the package has no outer. Reflected fields are
AllComponents (ArrayProperty, 0xc0, 16), Owner (ObjectProperty, 0x74, 8), and
Template (ObjectProperty, 0x224, 8). A bounded current-area inventory found two
actors with that owned crafting effect, including the user-confirmed nearby node.
This supplies a narrower signal than the shared definition alone; absence of
this effect remains unsupported rather than inferring pickup semantics.
