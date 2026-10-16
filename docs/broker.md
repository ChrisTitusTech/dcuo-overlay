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

## Search then fill with Ctrl+Alt+P

1. Visit My Wares or Browse to observe the item. Choose its exact name and client
   quantity in the companion. Check its stack against the item you intend to sell.
2. Open the game's **Browse** tab: All categories, Price Ascending, **Usable by me
   off**. Confirm this setup in the companion and arm the search.
3. Click the **search item field**, then press and release **Ctrl+Alt+P**. The
   shortcut replaces its text with the chosen name and presses Enter to search.
4. Wait for the companion's **Copied** status. Only a fresh matching response
   supplies the lowest observed total minus $1. This includes your own offers,
   compares the selected client quantity, and does not prove whole-market coverage.
5. On **My Wares**, add the item and click its **price field**. The next press of
   **Ctrl+Alt+P** replaces that field with the copied amount. The $1 is subtracted
   only once. This step does not press Enter or submit the listing. Review and
   confirm the listing yourself.

Each hotkey stage is armed for 60 seconds; the response wait lasts 30 seconds.
Watch the status to know which stage is armed: the app validates game-window focus
but cannot detect which Broker widget has focus. It never clicks or moves focus.
Cancel or changing the companion tab, selection, session, or pause stops the flow.
If it expires, arm a fresh search. A failed/empty/mismatched search never copies an
old cached price or arms price entry. Existing clipboard contents are not cleared.

Turn off Caps Lock, Num Lock and Sticky Keys modifiers. Printable ASCII names on
verified keyboard mappings are supported; unsupported input fails before typing.
If typing stops partway, inspect the field before trying again.

Before submitting the search, the shortcut briefly waits for the existing reader
to confirm a fresh settled Broker response. If another query intervenes, it stops
instead of attributing that query's prices to this search.
