# Broker observations

The Broker tab reads the supported client's active Broker response without game
calls or input. Open the companion Broker tab, then visit My Wares in game. Browse
or search in game for competing offers. The companion retains observed rows for
15 minutes within the connected character/session. A fresh request after the
reader first observes a session is required to exclude leftover responses.

Prices are total asking prices, not completed sales or vendor values. Comparisons
exclude the current seller and require the same item ID, displayed name and
nonzero client quantity value. Stack-size semantics and other item variants are
not independently verified; confirm those details in game. Missing observations
are unknown, not evidence that an item sold. Neither the active response nor its
count field establishes full market or My Wares coverage. No automatic searches,
buying, selling or cancellation are performed.

The cache is in memory only, capped at 2,000 observed listing IDs. Re-reading the
same response does not renew observation ages. Disconnect or character binding
changes clear the view. Search request tokens, list links, rows, seller strings,
character context and successful name lookups are rechecked before publication.
The existing build-gated worker performs these optional reads at most every three
seconds while Broker is selected, with a 250 ms budget and fresh physics afterward.

## API evidence (2026-10-16)

[Official Census documentation](https://census.daybreakgames.com/) describes
collection discovery and access limits. The
[DCUO collection listing](https://census.daybreakgames.com/s:example/get/dcuo:v1/)
returned 11 collections with no Broker or auction collection. Bounded broker and
auction probes returned `No data found`. `characters_item` supports equipped item
references; `auth_characters_item` returned `login_required`. Neither establishes
public inventory or own-listing access. The sibling dcuobot-api exposes equipped
artifacts/allies and their maintained metadata, with no Broker endpoint.

## Validation evidence

The lead compared a My Wares page and a user-driven item search against bounded
read-only observations. All ten My Wares prices and seven search prices matched;
item names, seller identity and the owned-row flag agreed with the visible UI.
The production parser read the seven-result page in approximately 1.4 ms and
22 KiB in this local observation. This is not a general performance guarantee.
Private screenshots and probe output remain ignored, outside committed fixtures.
Synthetic tests cover malformed/mutating data, bounds, ownership exclusion,
response-token age retention, owner changes, partial pages and cache eviction.
