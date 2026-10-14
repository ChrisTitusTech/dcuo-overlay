# Local map package import investigation

Offline investigation on 2026-10-13. Installed client packages provide a practical
local-only asset route. Redistribution permission is not a prerequisite to
implementing private local import; it remains required before bundling assets.
No proprietary package, decoded payload, texture, or personal game observation
was copied into this repository. This document does not establish live matching
or calibration.

## Tested package metadata

Files are relative to the user's installed `UNREAL3/DCGAME/COOKEDPC` directory.
The investigator read the four files below and decoded the two map-data packages
in RAM with the installed `liblzo2` safe decompressor. Output was limited to
package metadata and selected map properties; decoded payloads were not saved.

| Package under UI/MAPS | Bytes | Purpose |
| --- | ---: | --- |
| MGOT101/DUIMAPMGOT101.UPK | 6990 | Map metadata and streaming texture references |
| MGOT101/DUIMAPMGOT101_ZONE.UPK | 19822 | Separate zone overview texture exports |
| MPYR0224/DUIMAPMPYR0224.UPK | 1605 | Map metadata and streaming texture references |
| MPYR0224/DUIMAPMPYR0224_ZONE.UPK | 10857 | Separate zone overview texture exports |

All four have the UE3 package magic, version 648, licensee version 6405, and
package flags `0x22880009`. Both map-data packages have compression flags 2
(LZO), one compressed chunk and one LZO block. Their decoded chunk lengths are
48218 and 1092 bytes respectively. Name/import/export tables remain readable
before that chunk. DCUO name entries contain a positive length, that many bytes
without a trailing NUL, and eight flag bytes; treating them as normal UE strings
will misalign parsing.

The map-data name tables contain `DCUIMapData`, `MAPDATA`, `UIStreamingTextures`,
`Min`, `Max`, `SubMaps`, `textureSize`, `totalXTiles`, and `totalYTiles`.
A bounded tagged-property inspection found the following first main-map values:

| Package | Min vector | Max vector | Tile size | Grid | Candidate tiled image |
| --- | --- | --- | ---: | --- | --- |
| MGOT101 | (-340350.03125, -306572.09375, 0) | (283283.625, 340864.375, 0) | 256 | 24 x 25 | 6400 x 6144 |
| MPYR0224 | (-21000, -23000, 0) | (6775.99951171875, 44351.08203125, 0) | 256 | 2 x 3 | 768 x 512 |

MGOT101 also contains nine tagged submap records named `AI`, `BL`, `CT`, `DD`,
`EE`, `OB`, `OG`, `RP`, and `TCY`. MPYR0224 has a submap named `ART`.
These records contain additional Min/Max vectors and zero tile/grid fields.
Use export-table traversal and typed property parsing to distinguish the parent
map from nested submaps. Package bounds and grid dimensions are candidate
projection inputs, not player position, landmark evidence, or proof of a floor.
At the initial offline stage, neither the filename nor abbreviations established
MPYR0224 identity. The lead later confirmed Old Gotham Subway from the actual
streaming-level context and game map title, as recorded below.

## Concrete offline decoder route

[UEViewer's game database](https://github.com/gildor2/UEViewer/blob/master/Unreal/GameDatabase.cpp)
explicitly recognizes the tested version pair as DC Universe Online.
Its [UE3 package reader](https://github.com/gildor2/UEViewer/blob/master/Unreal/UnrealPackage/UnPackage3.cpp)
implements the DCUO name serialization exception, and its
[compression reader](https://github.com/gildor2/UEViewer/blob/master/Unreal/UnCoreCompression.cpp)
supports LZO. A Rust importer can implement this bounded subset rather than
loading unrelated mesh/model data.

1. Validate package magic/version, header counts, all ranges, and compressed
   chunk lengths before allocating. Decode the LZO block into a bounded buffer.
   Resolve export/import class names and select `DCUIMapData` and
   `UIStreamingTextures` explicitly; reject duplicate or ambiguous exports.
2. Decode tagged map properties, including Vector structs and nested SubMaps.
   Preserve array order and hash associations. Runtime structure offsets are
   not serialized-package offsets.
3. Read the current streaming texture array following the UObject property
   terminator. Each item starts with an FString key (i32 positive byte length,
   NUL-terminated ASCII), followed by seven i32 values: width, height, bulk flags,
   element count, bulk offset, on-disk size, pixel format; then sRGB u8 and FName
   (two u32). The fixed body is 37 bytes plus the variable string. This differs
   from the older hash-key schema in the
   [upstream structure definition](https://github.com/gildor2/UEViewer/blob/master/Unreal/UnrealMaterial/UnMaterial3.h).
   MPYR0224 has six 256 x 256 main tiles and one LOD tile. Its seven records end
   exactly at the export boundary. Main tiles use flags 17, pixel format 5 (BC1),
   element count 32768, negative offset -1, and cache name UI.
4. Resolve `TEXTUREFILECACHE/UI_REMAP.BIN`, not the old `_Remap.UPK` source
   implementation. The installed file has version 1, count 22826, and exactly
   `8 + count * 12` bytes. Each row is packed cache position u32, object-name
   hash u32, and collision marker u32. Reject nonzero collision markers and
   duplicate hashes. Hash the uppercase key as UTF-16LE with the historical
   UEViewer CRC routine. Low 12 packed bits select `UI<number>.TFC` (zero means
   UI.TFC); remaining bits are the byte offset. MPYR0224's first tile resolves
   to UI2.TFC offset 0x137c000. A bounded read there contains the correct UE3
   compression header and 853 compressed bytes producing 32768 BC1 bytes; the
   complete 877-byte span matches the tile metadata.
5. Decode only the supported main BC1 tiles. Keys encode row_column, with rows
   counted upward from the bottom; place each at image position
   `(column * tileSize, (rows - 1 - row) * tileSize)`. Do not transpose image
   pixels or normalize world bounds to the padded image dimensions. Candidate
   projection uses map `zoomValue` world units per pixel: image X is
   `(worldY - MinY) / zoomValue`; image Y is
   `rows * tileSize - (worldX - MinX) / zoomValue`. Scale that transform with
   any output downsampling. Both map packages report zoomValue 100. MPYR0224
   produces 768 x 512 pixels. Gotham has 600 main tiles, nine district images,
   and LOD0 through LOD3; import only the complete main grid and explicitly
   recognize/skip the known auxiliary images. Downsample Gotham main tiles
   before assembly to keep the image and decoder bounded.
6. Write derived images/catalog metadata to a private XDG data/cache location,
   with source checksums and decoder version. Keep Git and release bundles free
   of proprietary assets. Bind a catalog entry to proven live zone/layout/floor
   context and validate projection at independent landmarks before a live marker.

## Optional external-tool proof

UEViewer is a useful independent texture-import experiment, not yet a tested
importer for these exact files. Its streaming class header includes an old
"non-functional" comment despite the PostLoad implementation, so successful
export must be observed rather than assumed.

[Upstream Linux build instructions](https://github.com/gildor2/UEViewer/blob/master/README.md)
require gcc and SDL2, zlib, and libpng development packages, then `./build.sh`.
Build a pinned, reviewed checkout outside this repository. Its
[license](https://github.com/gildor2/UEViewer/blob/master/LICENSE.txt) is MIT;
vendored dependencies have their own notices. This licenses the decoder source,
not the game's assets.

The [CLI implementation](https://github.com/gildor2/UEViewer/blob/master/UmodelTool/Main.cpp)
documents `-list`, `-export`, `-png`, `-path=PATH`, and `-out=PATH`.
A private experiment can list `DUIMAPMGOT101`, then export that package with
`-export -png`, `-path` set to the complete CookedPC root and `-out` set to a
private cache directory. Keeping the full installation tree accessible permits
TFC/remap lookup. Export the zone package separately if comparing its overview.
Avoid `-all -dump` and whole-installation exports: they produce unrelated data and
are unnecessary for the bounded map experiment.

UEViewer source commit `a0bfb468d42be831b126632fd8a0ae6b3614f981` built
successfully with `./build.sh` after reviewing the build script, shader generator,
and project rules. Existing gcc/SDL2/zlib/libpng tooling was sufficient. No system
configuration changed. The full CookedPC root scan failed on excessive `.UPK:crc`
sidecars, so the experiments used only the selected map directory.

Exact successful export invocation, from the external tool checkout:

```sh
./umodel -export -png \
  -path="$cooked_pc/UI/MAPS/MPYR0224" \
  -out="$private_cache" DUIMAPMPYR0224_ZONE
```

The same command with MGOT101 and DUIMAPMGOT101_ZONE succeeded after the first
export passed. Each produced THETILE and THETILE_OLD PNGs, 512 x 512 pixels.
Visual inspection confirmed a complete corridor floorplan for MPYR0224 and a
Gotham landmass silhouette. Outputs remain in private local cache, outside Git.

Upstream export of DUIMAPMPYR0224 itself failed with an invalid name index because
of the obsolete hash-key serializer. A local serializer/remap patch built but
failed during generated-texture export. Therefore UEViewer is a verified overview
exporter here, not the production main-tile importer. A separate bounded local
metadata/LZO/BC1 experiment successfully decoded all six MPYR0224 tiles and the
LOD tile from UI2.TFC. The Rust importer implements the investigated current
format directly; it requires no external decoder or proprietary assets in Git.

The lead subsequently matched MPYR0224 to the visible Old Gotham Subway instance
using its AUDIO, DESIGNERDATA, and ART streaming-level names and game map title.
This evidence supersedes the earlier unproven candidate identity. Projection and
live acceptance still require the lead's independent landmark verification.


## Rust import validation

The application now has `src/map_import.rs`, a bounded native decoder for only
MGOT101 and MPYR0224. It parses physical package tables separately from logical
compressed payload offsets (which differ by 16 bytes in these packages), resolves
classes and the MAPDATA texture export link, validates tagged properties and
nested submaps, decodes current string-key streaming entries and BIN remaps, and
reads only bounded TFC chunks. Object BoolProperty values are four bytes here;
streaming-record sRGB remains one byte.

Six synthetic checks pass for BC1 opaque/interpolated/transparent pixels,
malformed compression/range/dimension inputs, duplicate/colliding remap hashes,
tile-index bounds, source confinement, and first-run private destination creation.
Strict library Clippy passes. An ignored opt-in local test accepts explicit source
and destination paths; it does not require game data in CI. The real installed
asset test passed, including catalog loading and PNG SHA-256 checks. It imported
600 Gotham main tiles and six subway main tiles. Outputs are 3200 x 3072 and
768 x 512 pixels respectively; both main maps serialize zoomValue 100. A visual
inspection found continuous geometry after reversing the row index.

New destination ancestors and the unique import directory use mode 700; PNG and
catalog files use mode 600. Existing unrelated destination files are preserved.
A failed import removes only its own new output directory. Returned catalogs
explicitly label the zoom/padded-grid projection unverified until live landmark
acceptance. The files remain local-only and outside repository/release assets.
