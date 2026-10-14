//! Bounded local-only decoder for the two investigated DCUO map packages.
//! Package bounds are candidate projection inputs, not accepted calibration.
use crate::maps::{Catalog, Category, Context, MapEntry};
use image::{ImageFormat, Rgba, RgbaImage};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{DirBuilderExt, OpenOptionsExt},
    },
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

const PACKAGE_LIMIT: usize = 16 * 1024 * 1024;
const DECODE_LIMIT: usize = 32 * 1024 * 1024;
const REMAP_LIMIT: usize = 8 * 1024 * 1024;
const TILE_LIMIT: usize = 1024 * 1024;
const MAGIC: u32 = 0x9e2a83c1;

/// Cooperative cancellation shared by the UI and its import worker.
#[derive(Clone, Debug, Default)]
pub struct ImportCancellation(Arc<AtomicBool>);
impl ImportCancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    fn checkpoint(&self) -> Result<(), String> {
        if self.is_cancelled() {
            Err("Map import cancelled".into())
        } else {
            Ok(())
        }
    }
}
struct CancellableFile<'a> {
    file: File,
    cancellation: &'a ImportCancellation,
}
impl Write for CancellableFile<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.cancellation
            .checkpoint()
            .map_err(std::io::Error::other)?;
        self.file.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.cancellation
            .checkpoint()
            .map_err(std::io::Error::other)?;
        self.file.flush()
    }
}
impl Seek for CancellableFile<'_> {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.cancellation
            .checkpoint()
            .map_err(std::io::Error::other)?;
        self.file.seek(position)
    }
}
struct ImportOutput {
    path: PathBuf,
    published: bool,
}
impl Drop for ImportOutput {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
fn with_import_output<T>(
    destination: &Path,
    cancellation: &ImportCancellation,
    work: impl FnOnce(&Path) -> Result<T, String>,
) -> Result<T, String> {
    cancellation.checkpoint()?;
    let mut output = ImportOutput {
        path: new_output(destination)?,
        published: false,
    };
    cancellation.checkpoint()?;
    let result = work(&output.path)?;
    cancellation.checkpoint()?;
    output.published = true;
    Ok(result)
}

#[derive(Debug, Clone)]
pub struct MapMetadata {
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub tile_size: u32,
    pub zoom_value: f64,
    /// Tile keys are row_column, despite the legacy totalX/totalY field names.
    pub rows: u32,
    pub columns: u32,
}
#[derive(Clone)]
struct Tile {
    key: String,
    width: u32,
    height: u32,
    flags: u32,
    count: usize,
    offset: i32,
    disk: usize,
    format: u32,
    cache: String,
}
struct Export {
    class: i32,
    name: String,
    start: usize,
    size: usize,
}
struct Package {
    bytes: Vec<u8>,
    names: Vec<String>,
    imports: Vec<String>,
    exports: Vec<Export>,
}
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    fn at(bytes: &'a [u8], pos: usize) -> Result<Self, String> {
        if pos > bytes.len() {
            return Err("Offset exceeds file".into());
        }
        Ok(Self { bytes, pos })
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.pos.checked_add(n).ok_or("Range overflow")?;
        let value = self.bytes.get(self.pos..end).ok_or_else(|| {
            format!(
                "Truncated map package: field at {} requests {n} bytes from {}",
                self.pos,
                self.bytes.len()
            )
        })?;
        self.pos = end;
        Ok(value)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(self.u32()? as i32)
    }
    fn count(&mut self, limit: usize) -> Result<usize, String> {
        let n = self.u32()? as usize;
        if n > limit {
            return Err("Count exceeds decoder limit".into());
        }
        Ok(n)
    }
    fn fname(&mut self, names: &[String]) -> Result<String, String> {
        let index = self.u32()? as usize;
        let number = self.u32()?;
        if number > 1024 {
            return Err("Invalid name suffix".into());
        }
        let name = names.get(index).ok_or("Invalid package name index")?;
        if number == 0 {
            Ok(name.clone())
        } else {
            Ok(format!("{name}_{}", number - 1))
        }
    }
    fn string(&mut self) -> Result<String, String> {
        let n = self.count(512)?;
        if n == 0 {
            return Ok(String::new());
        }
        let bytes = self.take(n)?;
        if bytes.last() != Some(&0) || bytes[..n - 1].contains(&0) {
            return Err("Invalid map string terminator".into());
        }
        let value =
            std::str::from_utf8(&bytes[..n - 1]).map_err(|_| "Unsupported map string encoding")?;
        if !value.is_ascii() || value.chars().any(char::is_control) {
            return Err("Invalid map text".into());
        }
        Ok(value.into())
    }
}
fn checked_range(bytes: &[u8], start: usize, size: usize) -> Result<&[u8], String> {
    let end = start.checked_add(size).ok_or("Range overflow")?;
    bytes
        .get(start..end)
        .ok_or_else(|| "File range exceeds available bytes".into())
}
fn open_confined(root: &Path, relative: &Path) -> Result<File, String> {
    if relative
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
        || relative.as_os_str().is_empty()
    {
        return Err("Invalid local asset path".into());
    }
    let resolved = root
        .join(relative)
        .canonicalize()
        .map_err(|e| format!("Local game asset unavailable: {e}"))?;
    if !resolved.starts_with(root) {
        return Err("Local asset symlink escapes installation".into());
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(resolved)
        .map_err(|e| e.to_string())?;
    let actual =
        fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd())).map_err(|e| e.to_string())?;
    if !actual.starts_with(root) || !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Opened asset escapes installation or is not regular".into());
    }
    Ok(file)
}
fn read_confined(root: &Path, relative: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let file = open_confined(root, relative)?;
    if file.metadata().map_err(|e| e.to_string())?.len() > limit as u64 {
        return Err("Local file exceeds import limit".into());
    }
    let mut bytes = Vec::new();
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err("Local file grew beyond import limit".into());
    }
    Ok(bytes)
}
fn decode_chunk(data: &[u8], expected: usize, limit: usize) -> Result<Vec<u8>, String> {
    if expected == 0 || expected > limit {
        return Err("Invalid decompressed chunk length".into());
    }
    let mut r = Reader::new(data);
    if r.u32()? != MAGIC {
        return Err("Invalid compressed chunk magic".into());
    }
    let block = r.count(DECODE_LIMIT)?;
    let compressed = r.count(PACKAGE_LIMIT)?;
    let total = r.count(limit)?;
    if block == 0 || total != expected {
        return Err("Invalid compression dimensions".into());
    }
    let n = total.div_ceil(block);
    if n > 256 {
        return Err("Too many compression blocks".into());
    }
    let mut blocks = Vec::with_capacity(n);
    let mut sum_c = 0usize;
    let mut sum_u = 0usize;
    for index in 0..n {
        let c = r.count(PACKAGE_LIMIT)?;
        let u = r.count(block)?;
        let desired = if index + 1 == n {
            total - index * block
        } else {
            block
        };
        if c == 0 || u != desired {
            return Err("Invalid compressed block span".into());
        }
        sum_c = sum_c.checked_add(c).ok_or("Compressed size overflow")?;
        sum_u += u;
        blocks.push((c, u));
    }
    if sum_c != compressed || sum_u != total || r.pos.checked_add(sum_c) != Some(data.len()) {
        return Err("Compressed chunk length mismatch".into());
    }
    let mut out = vec![0; total];
    let mut cursor = 0;
    for (c, u) in blocks {
        let src = r.take(c)?;
        let produced = lzokay::decompress::decompress(src, &mut out[cursor..cursor + u])
            .map_err(|e| format!("Invalid LZO data: {e:?}"))?;
        if produced != u {
            return Err("LZO decoded length mismatch".into());
        }
        cursor += u;
    }
    Ok(out)
}
impl Package {
    fn parse(raw: &[u8]) -> Result<Self, String> {
        if raw.len() > PACKAGE_LIMIT {
            return Err("Package exceeds decoder limit".into());
        }
        let mut r = Reader::new(raw);
        if r.u32()? != MAGIC || r.u32()? != ((6405 << 16) | 648) {
            return Err("Unsupported DCUO package version".into());
        }
        let header_size = r.count(DECODE_LIMIT)?;
        let _group = r.string()?;
        let _flags = r.u32()?;
        let name_count = r.count(4096)?;
        let name_offset = r.count(PACKAGE_LIMIT)?;
        let export_count = r.count(1024)?;
        let export_offset = r.count(PACKAGE_LIMIT)?;
        let import_count = r.count(1024)?;
        let import_offset = r.count(PACKAGE_LIMIT)?;
        r.take(4 + 16 + 16)?;
        let generations = r.count(16)?;
        r.take(generations * 12)?;
        r.take(8)?;
        if r.u32()? != 2 || r.u32()? != 1 {
            return Err("Unsupported package compression layout".into());
        }
        let uncompressed_offset = r.count(PACKAGE_LIMIT)?;
        let uncompressed_size = r.count(DECODE_LIMIT)?;
        let compressed_offset = r.count(PACKAGE_LIMIT)?;
        let compressed_size = r.count(PACKAGE_LIMIT)?;
        if uncompressed_offset > header_size
            || compressed_offset != header_size
            || header_size < r.pos
            || name_offset >= uncompressed_offset
            || export_offset >= uncompressed_offset
            || import_offset >= uncompressed_offset
        {
            return Err("Unsupported package table layout".into());
        }
        let payload = decode_chunk(
            checked_range(raw, compressed_offset, compressed_size)?,
            uncompressed_size,
            DECODE_LIMIT,
        )?;
        if uncompressed_offset
            .checked_add(payload.len())
            .is_none_or(|n| n > DECODE_LIMIT)
        {
            return Err("Decoded package exceeds limit".into());
        }
        let mut bytes = raw[..uncompressed_offset].to_vec();
        bytes.extend(payload);
        let mut r = Reader::at(&raw[..header_size], name_offset)?;
        let mut names = Vec::with_capacity(name_count);
        let mut unique = HashSet::new();
        for _ in 0..name_count {
            let n = r.count(512)?;
            if n == 0 {
                return Err("Empty package name".into());
            }
            let name = std::str::from_utf8(r.take(n)?)
                .map_err(|_| "Invalid package name")?
                .to_owned();
            if !name.is_ascii()
                || name.chars().any(char::is_control)
                || !unique.insert(name.clone())
            {
                return Err("Invalid or duplicate package name".into());
            }
            r.take(8)?;
            names.push(name);
        }
        let mut r = Reader::at(&raw[..header_size], import_offset)?;
        let mut imports = Vec::with_capacity(import_count);
        for _ in 0..import_count {
            r.fname(&names)?;
            r.fname(&names)?;
            r.i32()?;
            imports.push(r.fname(&names)?);
        }
        let mut r = Reader::at(&raw[..header_size], export_offset)?;
        let mut exports = Vec::with_capacity(export_count);
        let mut spans = Vec::new();
        for _ in 0..export_count {
            let class = r.i32()?;
            r.take(8)?;
            let name = r.fname(&names)?;
            r.take(12)?;
            let size = r.count(DECODE_LIMIT)?;
            let start = r.count(DECODE_LIMIT)?;
            r.take(4)?;
            let nets = r.count(1024)?;
            r.take(nets * 4 + 20)?;
            if start < uncompressed_offset || size == 0 {
                return Err("Invalid export data range".into());
            }
            checked_range(&bytes, start, size)?;
            if spans.iter().any(|&(a, b)| start < b && a < start + size) {
                return Err("Overlapping export data".into());
            }
            spans.push((start, start + size));
            exports.push(Export {
                class,
                name,
                start,
                size,
            });
        }
        Ok(Self {
            bytes,
            names,
            imports,
            exports,
        })
    }
    fn class(&self, export: &Export) -> Result<&str, String> {
        let index = export
            .class
            .checked_neg()
            .and_then(|i| i.checked_sub(1))
            .ok_or("Invalid export class")? as usize;
        self.imports
            .get(index)
            .map(String::as_str)
            .ok_or_else(|| "Unsupported export class reference".into())
    }
    fn export(&self, class: &str) -> Result<&Export, String> {
        let mut found = None;
        for e in &self.exports {
            if e.class < 0 && self.class(e)? == class {
                if found.is_some() {
                    return Err("Ambiguous map export class".into());
                }
                found = Some(e);
            }
        }
        found.ok_or_else(|| format!("Missing map export class {class}"))
    }
    fn export_reader(&self, e: &Export) -> Result<Reader<'_>, String> {
        let mut r = Reader::new(checked_range(&self.bytes, e.start, e.size)?);
        r.i32()?;
        Ok(r)
    }
}
struct Property<'a> {
    name: String,
    kind: String,
    structure: Option<String>,
    data: &'a [u8],
}
fn properties<'a>(r: &mut Reader<'a>, names: &[String]) -> Result<Vec<Property<'a>>, String> {
    let mut values = Vec::new();
    let mut unique = HashSet::new();
    for _ in 0..128 {
        let name = r.fname(names)?;
        if name.eq_ignore_ascii_case("none") {
            return Ok(values);
        }
        if !unique.insert(name.clone()) {
            return Err("Duplicate object property".into());
        }
        let kind = r.fname(names)?;
        let n = r.count(DECODE_LIMIT)?;
        if r.u32()? != 0 {
            return Err("Unsupported indexed property".into());
        }
        let structure = if kind == "StructProperty" {
            Some(r.fname(names)?)
        } else {
            None
        };
        let data = if kind == "BoolProperty" {
            if n != 0 {
                return Err("Invalid bool property".into());
            }
            r.take(4)?
        } else {
            r.take(n)?
        };
        values.push(Property {
            name,
            kind,
            structure,
            data,
        });
    }
    Err("Too many object properties".into())
}
fn property<'a, 'b>(
    values: &'b [Property<'a>],
    name: &str,
    kind: &str,
) -> Result<&'b Property<'a>, String> {
    let p = values
        .iter()
        .find(|p| p.name == name)
        .ok_or_else(|| format!("Missing map property {name}"))?;
    if p.kind != kind {
        return Err(format!("Invalid property type for {name}"));
    }
    Ok(p)
}
fn integer(values: &[Property<'_>], name: &str) -> Result<u32, String> {
    let p = property(values, name, "IntProperty")?;
    if p.data.len() != 4 {
        return Err("Invalid map integer length".into());
    }
    Reader::new(p.data).u32()
}
fn vector(values: &[Property<'_>], name: &str) -> Result<[f64; 3], String> {
    let p = property(values, name, "StructProperty")?;
    if p.structure.as_deref() != Some("Vector") || p.data.len() != 12 {
        return Err("Invalid map vector".into());
    }
    let mut r = Reader::new(p.data);
    let mut v = [0.; 3];
    for item in &mut v {
        *item = f32::from_bits(r.u32()?) as f64;
        if !item.is_finite() {
            return Err("Non-finite map bounds".into());
        }
    }
    Ok(v)
}
fn metadata(pkg: &Package) -> Result<MapMetadata, String> {
    let e = pkg.export("DCUIMapData")?;
    if e.name != "MAPDATA" {
        return Err("Unexpected map data export".into());
    }
    let mut r = pkg.export_reader(e)?;
    let values = properties(&mut r, &pkg.names)?;
    if r.pos != r.bytes.len() {
        return Err("Unexpected map data tail".into());
    }
    let allowed = [
        "Min",
        "Max",
        "textureSize",
        "totalXTiles",
        "totalYTiles",
        "hasSubMaps",
        "SubMaps",
        "Textures",
        "zoomValue",
        "MapName",
    ];
    if values.iter().any(|p| !allowed.contains(&p.name.as_str())) {
        return Err("Unsupported map metadata property".into());
    }
    let min = vector(&values, "Min")?;
    let max = vector(&values, "Max")?;
    let m = MapMetadata {
        min,
        max,
        tile_size: integer(&values, "textureSize")?,
        zoom_value: {
            let p = property(&values, "zoomValue", "FloatProperty")?;
            if p.data.len() != 4 {
                return Err("Invalid map zoom length".into());
            }
            f32::from_bits(Reader::new(p.data).u32()?) as f64
        },
        rows: integer(&values, "totalXTiles")?,
        columns: integer(&values, "totalYTiles")?,
    };
    if m.min[0] >= m.max[0]
        || m.min[1] >= m.max[1]
        || !m.zoom_value.is_finite()
        || m.zoom_value <= 0.0
        || m.zoom_value > 100_000.0
        || m.tile_size != 256
        || m.rows == 0
        || m.columns == 0
        || m.rows > 32
        || m.columns > 32
    {
        return Err("Unsupported map bounds or grid".into());
    }
    let textures = property(&values, "Textures", "ObjectProperty")?;
    if textures.data.len() != 4 {
        return Err("Invalid map texture reference".into());
    }
    let index = Reader::new(textures.data).i32()?;
    let linked = index
        .checked_sub(1)
        .and_then(|i| usize::try_from(i).ok())
        .and_then(|i| pkg.exports.get(i))
        .ok_or("Invalid map texture export reference")?;
    if pkg.class(linked)? != "UIStreamingTextures" {
        return Err("Map references a non-streaming texture export".into());
    }
    // Nested metadata is parsed as properties to reject malformed arrays without using
    // submap bounds as the main map calibration.
    if let Some(p) = values.iter().find(|p| p.name == "SubMaps") {
        if p.kind != "ArrayProperty" {
            return Err("Invalid submap type".into());
        }
        let mut nested = Reader::new(p.data);
        let n = nested.count(64)?;
        for _ in 0..n {
            properties(&mut nested, &pkg.names)?;
        }
        if nested.pos != nested.bytes.len() {
            return Err("Invalid submap array tail".into());
        }
    }
    Ok(m)
}
fn tiles(pkg: &Package) -> Result<Vec<Tile>, String> {
    let mut r = pkg.export_reader(pkg.export("UIStreamingTextures")?)?;
    if !properties(&mut r, &pkg.names)?.is_empty() {
        return Err("Unexpected streaming texture properties".into());
    }
    let n = r.count(2048)?;
    let mut out = Vec::with_capacity(n);
    let mut unique = HashSet::new();
    for _ in 0..n {
        let key = r.string()?;
        if !unique.insert(key.clone()) {
            return Err("Duplicate texture key".into());
        }
        let width = r.u32()?;
        let height = r.u32()?;
        let flags = r.u32()?;
        let count = r.count(TILE_LIMIT)?;
        let offset = r.i32()?;
        let disk = r.count(TILE_LIMIT)?;
        let format = r.u32()?;
        if r.take(1)?[0] > 1 {
            return Err("Invalid texture sRGB flag".into());
        }
        let cache = r.fname(&pkg.names)?;
        if !matches!(width, 256 | 512)
            || height != width
            || flags != 17
            || count != (width as usize * height as usize / if format == 5 { 2 } else { 1 })
            || offset != -1
            || disk < 24
            || !matches!(format, 5 | 7)
            || cache != "UI"
        {
            return Err("Unsupported local tile encoding".into());
        }
        out.push(Tile {
            key,
            width,
            height,
            flags,
            count,
            offset,
            disk,
            format,
            cache,
        });
    }
    if r.pos != r.bytes.len() {
        return Err("Unexpected streaming array tail".into());
    }
    Ok(out)
}
fn package_path(area: &str) -> Result<PathBuf, String> {
    if !matches!(area, "MGOT101" | "MPYR0224") {
        return Err("Unsupported local map area".into());
    }
    Ok(PathBuf::from(format!("UI/MAPS/{area}/DUIMAP{area}.UPK")))
}
pub fn load_metadata(cooked_pc: &Path, area: &str) -> Result<MapMetadata, String> {
    let root = cooked_pc.canonicalize().map_err(|e| e.to_string())?;
    let bytes = read_confined(&root, &package_path(area)?, PACKAGE_LIMIT)?;
    metadata(&Package::parse(&bytes)?)
}
fn remap(bytes: &[u8]) -> Result<HashMap<u32, u32>, String> {
    if bytes.len() > REMAP_LIMIT {
        return Err("Remap file exceeds limit".into());
    }
    let mut r = Reader::new(bytes);
    if r.u32()? != 1 {
        return Err("Unsupported TFC remap version".into());
    }
    let n = r.count(500_000)?;
    if r.pos.checked_add(n * 12) != Some(bytes.len()) {
        return Err("Invalid remap file length".into());
    }
    let mut out = HashMap::with_capacity(n);
    for _ in 0..n {
        let packed = r.u32()?;
        let hash = r.u32()?;
        let collision = r.u32()?;
        if collision != 0 || out.insert(hash, packed).is_some() {
            return Err("Ambiguous texture remap hash".into());
        }
    }
    Ok(out)
}
fn texture_hash(key: &str) -> u32 {
    let mut table = [0u32; 256];
    for (i, item) in table.iter_mut().enumerate() {
        let mut v = (i as u32) << 24;
        for _ in 0..8 {
            v = if v & 0x80000000 != 0 {
                (v << 1) ^ 0x04c11db7
            } else {
                v << 1
            };
        }
        *item = v;
    }
    let mut hash = 0u32;
    for b in key
        .to_ascii_uppercase()
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
    {
        hash = (hash >> 8) ^ table[((hash ^ b as u32) & 255) as usize];
    }
    hash
}
fn decode_bc1(bytes: &[u8], width: u32, height: u32) -> Result<RgbaImage, String> {
    if width == 0
        || height == 0
        || width > 1024
        || height > 1024
        || !width.is_multiple_of(4)
        || !height.is_multiple_of(4)
        || bytes.len() != (width as usize * height as usize / 2)
    {
        return Err("Invalid BC1 dimensions or payload".into());
    }
    let mut out = RgbaImage::new(width, height);
    for (index, block) in bytes.chunks_exact(8).enumerate() {
        let a = u16::from_le_bytes(block[..2].try_into().unwrap());
        let b = u16::from_le_bytes(block[2..4].try_into().unwrap());
        let bits = u32::from_le_bytes(block[4..].try_into().unwrap());
        let color = |c: u16| {
            [
                (((c >> 11) & 31) as u32 * 255 / 31) as u8,
                (((c >> 5) & 63) as u32 * 255 / 63) as u8,
                ((c & 31) as u32 * 255 / 31) as u8,
                255,
            ]
        };
        let mut colors = [color(a), color(b), [0; 4], [0; 4]];
        let first = colors[0];
        for (c, component) in first.iter().enumerate().take(3) {
            if a > b {
                colors[2][c] = ((2 * *component as u16 + colors[1][c] as u16) / 3) as u8;
                colors[3][c] = ((*component as u16 + 2 * colors[1][c] as u16) / 3) as u8;
            } else {
                colors[2][c] = ((*component as u16 + colors[1][c] as u16) / 2) as u8;
            }
        }
        colors[2][3] = 255;
        if a > b {
            colors[3][3] = 255;
        }
        let bx = (index as u32 % (width / 4)) * 4;
        let by = (index as u32 / (width / 4)) * 4;
        for y in 0..4 {
            for x in 0..4 {
                let select = ((bits >> (2 * (y * 4 + x))) & 3) as usize;
                out.put_pixel(bx + x, by + y, Rgba(colors[select]));
            }
        }
    }
    Ok(out)
}
fn tile_image(root: &Path, tile: &Tile, mappings: &HashMap<u32, u32>) -> Result<RgbaImage, String> {
    let packed = *mappings
        .get(&texture_hash(&tile.key))
        .ok_or("Missing map texture remap")?;
    let index = packed & 0xfff;
    if index > 64 {
        return Err("Unsupported texture cache shard".into());
    }
    let offset = (packed & !0xfff) as u64;
    let filename = if index == 0 {
        "UI.TFC".to_string()
    } else {
        format!("UI{index}.TFC")
    };
    let mut file = open_confined(root, &PathBuf::from("TEXTUREFILECACHE").join(filename))?;
    if offset
        .checked_add(tile.disk as u64)
        .is_none_or(|n| n > file.metadata().map(|m| m.len()).unwrap_or(0))
    {
        return Err("Texture chunk exceeds cache file".into());
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| e.to_string())?;
    let mut bytes = vec![0; tile.disk];
    file.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    let data = decode_chunk(&bytes, tile.count, TILE_LIMIT)?;
    decode_bc1(&data, tile.width, tile.height)
}
fn tile_position(key: &str, area: &str, m: &MapMetadata) -> Result<Option<(u32, u32)>, String> {
    let prefix = format!("DUIMAP{area}");
    if area == "MGOT101"
        && ["AI", "BL", "CT", "DD", "EE", "OB", "OG", "RP", "TCY"]
            .iter()
            .any(|name| key == format!("{prefix}_{name}.THETILE"))
    {
        return Ok(None);
    }
    for level in 0..=3 {
        if let Some(suffix) = key
            .strip_prefix(&format!("{prefix}LOD{level}_"))
            .and_then(|s| s.strip_suffix(".THETILE"))
        {
            let (row, column) = suffix.split_once('_').ok_or("Invalid LOD tile key")?;
            let row: u32 = row.parse().map_err(|_| "Invalid LOD row")?;
            let column: u32 = column.parse().map_err(|_| "Invalid LOD column")?;
            if row >= m.rows || column >= m.columns {
                return Err("LOD key exceeds grid bounds".into());
            }
            return Ok(None);
        }
    }
    let suffix = key
        .strip_prefix(&format!("{prefix}_"))
        .and_then(|s| s.strip_suffix(".THETILE"))
        .ok_or("Unexpected map texture key")?;
    let (row, column) = suffix.split_once('_').ok_or("Invalid tile index key")?;
    let row: u32 = row.parse().map_err(|_| "Invalid tile row")?;
    let column: u32 = column.parse().map_err(|_| "Invalid tile column")?;
    if row >= m.rows || column >= m.columns {
        return Err("Tile index exceeds map grid".into());
    }
    Ok(Some((row, column)))
}
fn assemble(
    root: &Path,
    area: &str,
    m: &MapMetadata,
    all_tiles: &[Tile],
    mappings: &HashMap<u32, u32>,
    cancellation: &ImportCancellation,
) -> Result<RgbaImage, String> {
    cancellation.checkpoint()?;
    let scale = if area == "MGOT101" { 2 } else { 1 };
    let size = m.tile_size / scale;
    let width = m.columns.checked_mul(size).ok_or("Map width overflow")?;
    let height = m.rows.checked_mul(size).ok_or("Map height overflow")?;
    if width > 4096 || height > 4096 || width as u64 * height as u64 * 4 > 64 * 1024 * 1024 {
        return Err("Assembled image exceeds limits".into());
    }
    let mut out = RgbaImage::new(width, height);
    let mut positions = HashSet::new();
    for tile in all_tiles {
        cancellation.checkpoint()?;
        if tile.flags != 17
            || tile.offset != -1
            || !matches!(tile.format, 5 | 7)
            || tile.cache != "UI"
        {
            return Err("Unsupported tile metadata".into());
        }
        if let Some((row, col)) = tile_position(&tile.key, area, m)? {
            if tile.width != m.tile_size || tile.height != m.tile_size || tile.format != 5 {
                return Err("Unsupported main map tile encoding".into());
            }
            if !positions.insert((row, col)) {
                return Err("Duplicate tile position".into());
            }
            let image = tile_image(root, tile, mappings)?;
            cancellation.checkpoint()?;
            let image = if scale == 2 {
                image::imageops::resize(&image, size, size, image::imageops::FilterType::Triangle)
            } else {
                image
            };
            image::imageops::replace(
                &mut out,
                &image,
                (col * size) as i64,
                ((m.rows - 1 - row) * size) as i64,
            );
        }
    }
    cancellation.checkpoint()?;
    if positions.len() != (m.rows * m.columns) as usize {
        return Err("Incomplete map tile grid".into());
    }
    Ok(out)
}
fn new_output(root: &Path) -> Result<PathBuf, String> {
    if !root.is_absolute()
        || root
            .components()
            .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
    {
        return Err("Import destination must be an absolute path without traversal".into());
    }
    let mut ancestor = PathBuf::new();
    for component in root.components() {
        ancestor.push(component.as_os_str());
        match fs::symlink_metadata(&ancestor) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err("Destination ancestor is a symlink or not a directory".into());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let mut builder = fs::DirBuilder::new();
                builder.mode(0o700);
                builder.create(&ancestor).map_err(|e| e.to_string())?;
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    let canonical = root.canonicalize().map_err(|e| e.to_string())?;
    if canonical != root {
        return Err("Destination path changed during creation".into());
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let output = root.join(format!("import-{}-{stamp}", std::process::id()));
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700);
    builder.create(&output).map_err(|e| e.to_string())?;
    if output.canonicalize().map_err(|e| e.to_string())? != output {
        return Err("Created output escapes destination".into());
    }
    Ok(output)
}
/// Creates a fresh private import directory; never overwrites an existing catalog.
/// Projection remains explicitly unverified until the lead records live landmarks.
pub fn import_maps(cooked_pc: &Path, destination: &Path) -> Result<PathBuf, String> {
    import_maps_cancellable(cooked_pc, destination, &ImportCancellation::default())
}
/// Imports privately and removes its incomplete output on error or cancellation.
pub fn import_maps_cancellable(
    cooked_pc: &Path,
    destination: &Path,
    cancellation: &ImportCancellation,
) -> Result<PathBuf, String> {
    cancellation.checkpoint()?;
    let root = cooked_pc.canonicalize().map_err(|e| e.to_string())?;
    let mappings = remap(&read_confined(
        &root,
        Path::new("TEXTUREFILECACHE/UI_REMAP.BIN"),
        REMAP_LIMIT,
    )?)?;
    cancellation.checkpoint()?;
    with_import_output(destination, cancellation, |output| {
        let mut entries = Vec::new();
        for (area, label, category) in [
            ("MGOT101", "Gotham", Category::OpenWorld),
            ("MPYR0224", "Old Gotham Subway", Category::Instance),
        ] {
            cancellation.checkpoint()?;
            let raw = read_confined(&root, &package_path(area)?, PACKAGE_LIMIT)?;
            cancellation.checkpoint()?;
            let source_sha = format!("{:x}", Sha256::digest(&raw));
            let pkg = Package::parse(&raw)?;
            cancellation.checkpoint()?;
            drop(raw);
            let m = metadata(&pkg)?;
            let expected_grid = if area == "MGOT101" { (24, 25) } else { (2, 3) };
            if (m.rows, m.columns) != expected_grid {
                return Err("Unsupported area grid revision".into());
            }
            cancellation.checkpoint()?;
            let image = assemble(&root, area, &m, &tiles(&pkg)?, &mappings, cancellation)?;
            cancellation.checkpoint()?;
            let filename = format!("{area}.png");
            let path = output.join(&filename);
            let file = OpenOptions::new()
                .write(true)
                .mode(0o600)
                .create_new(true)
                .open(&path)
                .map_err(|e| e.to_string())?;
            let mut writer = CancellableFile { file, cancellation };
            image
                .write_to(&mut writer, ImageFormat::Png)
                .map_err(|e| e.to_string())?;
            cancellation.checkpoint()?;
            writer.file.sync_all().map_err(|e| e.to_string())?;
            let bytes = read_confined(output, Path::new(&filename), PACKAGE_LIMIT)?;
            let width = image.width();
            let height = image.height();
            // UE map horizontal axes appear rotated relative to image axes. This
            // zoom-and-padded-grid candidate deliberately stays labeled unverified.
            let scale = if area == "MGOT101" { 2.0 } else { 1.0 };
            let sx = 1.0 / (m.zoom_value * scale);
            let sy = sx;
            let e = MapEntry {
                id: format!("local-{area}"),
                label: label.into(),
                category,
                context: Context {
                    zone: area.into(),
                    layout: "main".into(),
                    floor: "overview".into(),
                },
                width,
                height,
                asset: filename,
                sha256: format!("{:x}", Sha256::digest(&bytes)),
                provenance: format!(
                    "Local-only installed client import v1; package SHA256 {source_sha}; tile bounds projection UNVERIFIED"
                ),
                credit: "DC Universe Online / Daybreak Games / DC; private local assets".into(),
                affine: [
                    0.,
                    sx,
                    -m.min[1] * sx,
                    -sy,
                    0.,
                    height as f64 + m.min[0] * sy,
                ],
                world_bounds: [m.min[0], m.min[1], m.max[0], m.max[1]],
                calibration: Some(crate::maps::Calibration {
                    version: 1,
                    profile: crate::native::PROFILE_ID.into(),
                    confidence: crate::maps::LocationConfidence::Experimental,
                    landmark_count: 0,
                    independent_landmark_count: 0,
                    max_error_pixels: None,
                    accuracy_budget_pixels: None,
                }),
            };
            e.validate()?;
            entries.push(e);
        }
        let catalog = Catalog {
            schema_version: 1,
            entries,
        };
        cancellation.checkpoint()?;
        let path = output.join("catalog.json");
        let temporary = output.join("catalog.partial");
        let mut file = OpenOptions::new()
            .write(true)
            .mode(0o600)
            .create_new(true)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        cancellation.checkpoint()?;
        file.write_all(&serde_json::to_vec_pretty(&catalog).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        cancellation.checkpoint()?;
        fs::rename(temporary, &path).map_err(|e| e.to_string())?;
        Ok(path)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cancellation_fixture() -> PathBuf {
        std::env::temp_dir().join(format!(
            "dcuo-cancel-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
    #[test]
    fn cancelled_import_stops_before_source_or_output_access() {
        let destination = cancellation_fixture();
        let cancellation = ImportCancellation::default();
        cancellation.cancel();
        let error =
            import_maps_cancellable(Path::new("missing-source"), &destination, &cancellation)
                .unwrap_err();
        assert_eq!(error, "Map import cancelled");
        assert!(!destination.exists());
        let metadata = MapMetadata {
            min: [0.; 3],
            max: [1.; 3],
            tile_size: 256,
            zoom_value: 1.,
            rows: 2,
            columns: 3,
        };
        assert_eq!(
            assemble(
                Path::new("missing-source"),
                "MPYR0224",
                &metadata,
                &[],
                &HashMap::new(),
                &cancellation
            )
            .unwrap_err(),
            "Map import cancelled"
        );
    }
    #[test]
    fn cancellation_before_publication_cleans_only_its_incomplete_output() {
        let destination = cancellation_fixture();
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("keep"), b"unrelated").unwrap();
        let cancellation = ImportCancellation::default();
        let result = with_import_output(&destination, &cancellation, |output| {
            fs::write(output.join("image.png"), b"synthetic partial image").unwrap();
            fs::write(output.join("catalog.partial"), b"synthetic partial catalog").unwrap();
            cancellation.cancel();
            Ok(())
        });
        assert_eq!(result.unwrap_err(), "Map import cancelled");
        assert_eq!(fs::read(destination.join("keep")).unwrap(), b"unrelated");
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 1);
        fs::remove_dir_all(destination).unwrap();
    }
    #[test]
    fn cancellation_interrupts_png_writes_without_write_all_retry() {
        let path = cancellation_fixture();
        let file = File::create(&path).unwrap();
        let cancellation = ImportCancellation::default();
        let mut writer = CancellableFile {
            file,
            cancellation: &cancellation,
        };
        writer.write_all(b"before").unwrap();
        cancellation.cancel();
        let error = writer.write_all(b"after").unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::Other);
        assert!(writer.seek(SeekFrom::Start(0)).is_err());
        drop(writer);
        assert_eq!(fs::read(&path).unwrap(), b"before");
        fs::remove_file(path).unwrap();
    }
    #[test]
    #[ignore = "Requires an explicitly selected local installation and private output directory"]
    fn local_asset_import_opt_in() {
        let source =
            std::env::var_os("DCUO_LOCAL_IMPORT_SOURCE").expect("Set DCUO_LOCAL_IMPORT_SOURCE");
        let output =
            std::env::var_os("DCUO_LOCAL_IMPORT_DEST").expect("Set DCUO_LOCAL_IMPORT_DEST");
        for area in ["MGOT101", "MPYR0224"] {
            let m = load_metadata(Path::new(&source), area).unwrap();
            eprintln!(
                "{area}: zoom={}, rows={}, columns={}",
                m.zoom_value, m.rows, m.columns
            );
        }
        let catalog_path = import_maps(Path::new(&source), Path::new(&output)).unwrap();
        let catalog = crate::maps::load_catalog(&catalog_path).unwrap();
        assert_eq!(catalog.entries.len(), 2);
        for entry in &catalog.entries {
            crate::maps::load_asset(entry, catalog_path.parent().unwrap()).unwrap();
        }
        eprintln!("Local import catalog: {}", catalog_path.display());
    }
    #[test]
    fn bc1_opaque_interpolated_and_transparent_pixels() {
        let red = 0xf800u16;
        let blue = 0x001fu16;
        let mut b = Vec::new();
        b.extend(red.to_le_bytes());
        b.extend(blue.to_le_bytes());
        b.extend(0xe4u32.to_le_bytes());
        let im = decode_bc1(&b, 4, 4).unwrap();
        assert_eq!(im.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(im.get_pixel(1, 0).0, [0, 0, 255, 255]);
        assert_eq!(im.get_pixel(2, 0).0, [170, 0, 85, 255]);
        assert_eq!(im.get_pixel(3, 0).0, [85, 0, 170, 255]);
        let b = [0, 0, 0, 0, 255, 255, 255, 255];
        assert_eq!(decode_bc1(&b, 4, 4).unwrap().get_pixel(0, 0).0[3], 0);
    }
    #[test]
    fn dimensions_ranges_and_corruption_rejected() {
        assert!(decode_bc1(&[], u32::MAX, 4).is_err());
        assert!(decode_bc1(&[0; 8], 3, 4).is_err());
        assert!(decode_bc1(&[0; 7], 4, 4).is_err());
        assert!(checked_range(&[0; 8], usize::MAX, 2).is_err());
        assert!(Package::parse(&[0; 128]).is_err());
        assert!(decode_chunk(&[], 32, TILE_LIMIT).is_err());
        assert!(decode_chunk(&[0; 24], usize::MAX, TILE_LIMIT).is_err());
        let mut bytes = Vec::new();
        for n in [MAGIC, 32, 1, 32, 1, 32] {
            bytes.extend(n.to_le_bytes());
        }
        bytes.push(0);
        assert!(decode_chunk(&bytes, 32, TILE_LIMIT).is_err());
    }
    #[test]
    fn remap_rejects_duplicate_hash_collisions_and_lengths() {
        let mut bytes = Vec::new();
        for n in [1u32, 2, 0x2001, 42, 0, 0x3001, 42, 0] {
            bytes.extend(n.to_le_bytes());
        }
        assert!(remap(&bytes).is_err());
        bytes.truncate(20);
        bytes[4..8].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(remap(&bytes).unwrap().get(&42), Some(&0x2001));
        bytes[16] = 1;
        assert!(remap(&bytes).is_err());
        assert!(remap(&[0; 7]).is_err());
    }
    #[test]
    fn tile_indexes_are_bounded_and_transposed() {
        let m = MapMetadata {
            min: [0.; 3],
            max: [10.; 3],
            tile_size: 256,
            zoom_value: 1.0,
            rows: 2,
            columns: 3,
        };
        assert_eq!(
            tile_position("DUIMAPMPYR0224_1_2.THETILE", "MPYR0224", &m).unwrap(),
            Some((1, 2))
        );
        assert!(tile_position("DUIMAPMPYR0224_2_1.THETILE", "MPYR0224", &m).is_err());
        assert!(package_path("../escape").is_err());
        assert_eq!(texture_hash("DUIMAPMPYR0224_0_0.THETILE"), 0x19b1cc1d);
    }
    #[test]
    fn first_run_destination_is_private_and_preserves_existing_files() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!(
            "dcuo-output-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let destination = base.join("new/data/maps");
        let output = new_output(&destination).unwrap();
        assert!(output.is_dir());
        assert_eq!(
            fs::metadata(&output).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&destination).unwrap().permissions().mode() & 0o777,
            0o700
        );
        fs::write(destination.join("keep"), b"unrelated").unwrap();
        assert_ne!(new_output(&destination).unwrap(), output);
        assert_eq!(fs::read(destination.join("keep")).unwrap(), b"unrelated");
        std::os::unix::fs::symlink("/tmp", base.join("escape")).unwrap();
        assert!(new_output(&base.join("escape/nested")).is_err());
        assert!(new_output(&base.join("../escape")).is_err());
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn source_path_rejects_traversal_and_escaping_symlink() {
        let root = std::env::temp_dir().join(format!("dcuo-import-test-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        assert!(open_confined(&root, Path::new("../escape")).is_err());
        let link = root.join("escape");
        std::os::unix::fs::symlink("/etc/hosts", &link).unwrap();
        assert!(open_confined(&root, Path::new("escape")).is_err());
        fs::remove_file(link).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
