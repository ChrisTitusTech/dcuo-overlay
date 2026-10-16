# Feat matching corrections, 2026-10-16

The supported client's read-only definitions were compared with the existing
solo catalog. No personal completion data, memory addresses or serialized records
are included here. These are minimal catalog corrections, not a replacement
client-definition dump.

- Trailing whitespace in titles is formatting, not a different feat.
- Superman and Wonder Woman School of Hard Knocks titles also occur in duos.
  Exact mission requirements distinguish the solo entries; title alone cannot.
- Fun Times has a client ID differing from the public archived mapping used by
  this catalog. Removed the conflicting global ID constraint instead of assuming
  one faction's ID applies to all characters. Live title/reward matching remains
  required. API-only completion for this row stays unknown without a trusted ID.
- The client spells the Mausoleum feat Constantine Shmonstantine. The catalog
  used Schmonstantine; the mission requirement and reward agree.
- At Arm's Length in Kryb's Lab awards 10 points in the supported client. The
  catalog incorrectly specified 25; its title and exact requirement agree.
- Ain't Going Down and The Croc Hunter are Hero-only, as explicitly listed by
  [DCUO Bloguide](https://dcuobloguide.com/zones/gotham-sewer-challenge/).
  They remain in the catalog for Heroes and are filtered for Villains.

Requirements and rewards still constrain matches. Unknown, conflicting, or
multiply matching records are never guessed. The native reader continues to
validate ownership and completion separately from definition matching.
