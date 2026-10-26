//! Exact, bounded local game icon resource decoding. No item/name inference.
use crate::map_import::{
    Package, Reader, decode_chunk, open_confined, properties, property, read_confined, remap,
    texture_hash,
};
use image::{ImageFormat, Rgba, RgbaImage};
use std::{
    collections::HashMap,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};
const LIMIT: usize = 1024 * 1024;

fn resource_parts(resource: &str) -> Result<(&str, &str, &str), String> {
    let (package, rest) = resource.split_once('/').ok_or("Invalid icon resource")?;
    let (movie, symbol) = rest.split_once('#').ok_or("Invalid icon symbol")?;
    let suffix = package
        .strip_prefix("DUIICONSLIBS")
        .ok_or("Unsupported icon package")?;
    if suffix.len() != 3
        || !suffix.bytes().all(|b| b.is_ascii_digit())
        || movie != package.strip_prefix('D').unwrap_or("")
        || symbol.is_empty()
        || symbol.len() > 32
        || !symbol
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err("Invalid icon resource path or symbol".into());
    }
    Ok((package, movie, symbol))
}
fn u16le(r: &mut Reader<'_>) -> Result<u16, String> {
    Ok(u16::from_le_bytes(r.take(2)?.try_into().unwrap()))
}
fn text<'a>(r: &mut Reader<'a>, length: usize) -> Result<&'a str, String> {
    let bytes = r.take(length)?;
    let s = std::str::from_utf8(bytes).map_err(|_| "Invalid GFX text")?;
    if !s.is_ascii() || s.bytes().any(|b| b.is_ascii_control()) {
        return Err("Invalid GFX text".into());
    }
    Ok(s)
}
fn ctext<'a>(r: &mut Reader<'a>) -> Result<&'a str, String> {
    let n = r.bytes[r.pos..]
        .iter()
        .position(|b| *b == 0)
        .filter(|n| *n <= 128)
        .ok_or("Invalid GFX terminated text")?;
    let s = text(r, n)?;
    r.take(1)?;
    Ok(s)
}
/// ExportAssets (56) joins a named symbol to a character ID. DefineExternalImage2
/// (1009) joins that ID to an exact image label, not a positional texture index.
fn gfx_image(raw: &[u8], symbol: &str) -> Result<(String, u32, u32), String> {
    if raw.len() > LIMIT
        || raw.len() < 13
        || &raw[..4] != b"GFX\x09"
        || u32::from_le_bytes(raw[4..8].try_into().unwrap()) as usize != raw.len()
    {
        return Err("Unsupported GFX header".into());
    }
    let bits = (raw[8] >> 3) as usize;
    let mut r = Reader::new(raw);
    r.take(8 + (5 + bits * 4).div_ceil(8) + 4)?;
    let mut images = HashMap::new();
    let mut exports = HashMap::new();
    let mut ended = false;
    for _ in 0..16384 {
        let h = u16le(&mut r)?;
        let tag = h >> 6;
        let n = if h & 63 == 63 {
            r.count(LIMIT)?
        } else {
            (h & 63) as usize
        };
        let data = r.take(n)?;
        let mut p = Reader::new(data);
        match tag {
            0 => {
                if n != 0 {
                    return Err("Invalid GFX end tag".into());
                }
                ended = true;
                break;
            }
            56 => {
                let count = u16le(&mut p)?;
                if count > 4096 {
                    return Err("Too many GFX exports".into());
                }
                for _ in 0..count {
                    let id = u16le(&mut p)?;
                    let name = ctext(&mut p)?.to_owned();
                    if exports.insert(name, id).is_some() {
                        return Err("Duplicate GFX symbol".into());
                    }
                }
                if p.pos != data.len() {
                    return Err("Invalid GFX export tail".into());
                }
            }
            1009 => {
                let id = p.u32()?;
                // The observed layout uses u16 format, u16 width, u16 height.
                let format = u16le(&mut p)?;
                let width = u16le(&mut p)? as u32;
                let height = u16le(&mut p)? as u32;
                if format != 14 {
                    return Err("Unsupported GFX image format".into());
                }
                let n = p.take(1)?[0] as usize;
                let label = text(&mut p, n)?.to_owned();
                let n = p.take(1)?[0] as usize;
                let file = text(&mut p, n)?;
                if (!label.is_empty() && file != format!("{label}.dds"))
                    || p.pos != data.len()
                    || !label
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                {
                    return Err("Unsupported GFX image label".into());
                }
                if images.insert(id, (label, width, height)).is_some() {
                    return Err("Duplicate GFX image ID".into());
                }
            }
            _ => {}
        }
    }
    if !ended || raw[r.pos..].iter().any(|b| *b != 0) {
        return Err("Missing GFX end or invalid tail".into());
    }
    let id = *exports
        .get(symbol)
        .ok_or("Exact GFX icon symbol unavailable")? as u32;
    let image = images
        .remove(&id)
        .ok_or("GFX symbol does not reference an external image")?;
    if image.0.is_empty() {
        return Err("GFX symbol references unsupported atlas image".into());
    }
    Ok(image)
}

fn decode_bc3(bytes: &[u8], width: u32, height: u32) -> Result<RgbaImage, String> {
    if width == 0
        || height == 0
        || width > 256
        || height > 256
        || !width.is_multiple_of(4)
        || !height.is_multiple_of(4)
        || bytes.len() != width as usize * height as usize
    {
        return Err("Invalid BC3 dimensions or payload".into());
    }
    let mut image = RgbaImage::new(width, height);
    for (i, b) in bytes.chunks_exact(16).enumerate() {
        let mut alpha = [b[0], b[1], 0, 0, 0, 0, 0, 0];
        if b[0] > b[1] {
            for j in 1..7 {
                alpha[j + 1] = (((7 - j) as u16 * b[0] as u16 + j as u16 * b[1] as u16) / 7) as u8;
            }
        } else {
            for j in 1..5 {
                alpha[j + 1] = (((5 - j) as u16 * b[0] as u16 + j as u16 * b[1] as u16) / 5) as u8;
            }
            alpha[7] = 255;
        }
        let mut ab = [0; 8];
        ab[..6].copy_from_slice(&b[2..8]);
        let ab = u64::from_le_bytes(ab);
        let color = |v: u16| {
            [
                (u32::from((v >> 11) & 31) * 255 / 31) as u8,
                (u32::from((v >> 5) & 63) * 255 / 63) as u8,
                (u32::from(v & 31) * 255 / 31) as u8,
            ]
        };
        let a = color(u16::from_le_bytes(b[8..10].try_into().unwrap()));
        let z = color(u16::from_le_bytes(b[10..12].try_into().unwrap()));
        let mut colors = [a, z, [0; 3], [0; 3]];
        for c in 0..3 {
            colors[2][c] = ((2 * a[c] as u16 + z[c] as u16) / 3) as u8;
            colors[3][c] = ((a[c] as u16 + 2 * z[c] as u16) / 3) as u8;
        }
        let cb = u32::from_le_bytes(b[12..16].try_into().unwrap());
        for j in 0..16 {
            let c = colors[((cb >> (2 * j)) & 3) as usize];
            image.put_pixel(
                (i as u32 % (width / 4)) * 4 + j % 4,
                (i as u32 / (width / 4)) * 4 + j / 4,
                Rgba([c[0], c[1], c[2], alpha[((ab >> (3 * j)) & 7) as usize]]),
            );
        }
    }
    Ok(image)
}

fn encode_png(image: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut png = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut png, ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    let bytes = png.into_inner();
    if bytes.len() > 64 * 1024 {
        return Err("Icon PNG exceeds publication limit".into());
    }
    Ok(bytes)
}

// Startup libraries are embedded in a multi-chunk package. Read only its bounded
// tables and chunks intersecting explicitly selected exports.
struct StartupExport {
    class: String,
    name: String,
    outer: i32,
    start: usize,
    size: usize,
}
struct Startup {
    file: std::fs::File,
    names: Vec<String>,
    exports: Vec<StartupExport>,
    chunks: Vec<[usize; 4]>,
}
impl Startup {
    fn open(root: &Path) -> Result<Self, String> {
        let mut file = open_confined(root, Path::new("STARTUP/STARTUPUI.UPK"))?;
        let mut first = [0; 12];
        file.read_exact(&mut first).map_err(|e| e.to_string())?;
        let header = u32::from_le_bytes(first[8..12].try_into().unwrap()) as usize;
        if !(12..=2 * LIMIT).contains(&header) {
            return Err("Unsupported startup header length".into());
        }
        file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        let mut bytes = vec![0; header];
        file.read_exact(&mut bytes).map_err(|e| e.to_string())?;
        let mut r = Reader::new(&bytes);
        if r.u32()? != 0x9e2a83c1 || r.u32()? != ((6405 << 16) | 648) {
            return Err("Unsupported startup package version".into());
        }
        r.u32()?;
        r.string()?;
        r.u32()?;
        let nc = r.count(20000)?;
        let no = r.count(header)?;
        let ec = r.count(20000)?;
        let eo = r.count(header)?;
        let ic = r.count(10000)?;
        let io = r.count(header)?;
        r.take(36)?;
        let generations = r.count(16)?;
        r.take(generations * 12 + 8)?;
        if r.u32()? != 2 {
            return Err("Unsupported startup compression".into());
        }
        let count = r.count(512)?;
        let mut chunks = Vec::new();
        let mut previous = None;
        let length = file.metadata().map_err(|e| e.to_string())?.len();
        for _ in 0..count {
            let a = r.count(512 * LIMIT)?;
            let n = r.count(8 * LIMIT)?;
            let c = r.count(128 * LIMIT)?;
            let m = r.count(16 * LIMIT)?;
            if n == 0
                || m < 24
                || c < header
                || c.checked_add(m).is_none_or(|end| end as u64 > length)
                || previous.is_some_and(|p| p != a)
            {
                return Err("Invalid startup chunk range".into());
            }
            previous = Some(a.checked_add(n).ok_or("Startup chunk overflow")?);
            chunks.push([a, n, c, m]);
        }
        let mut r = Reader::new(bytes.get(no..).ok_or("Invalid startup name offset")?);
        let mut names = Vec::new();
        for _ in 0..nc {
            let n = r.count(512)?;
            let name = text(&mut r, n)?.to_owned();
            r.take(8)?;
            if name.is_empty() {
                return Err(format!("Invalid startup name {name:?}"));
            }
            names.push(name);
        }
        let mut r = Reader::new(bytes.get(io..).ok_or("Invalid startup import offset")?);
        let mut imports = Vec::new();
        for _ in 0..ic {
            r.fname(&names)?;
            r.fname(&names)?;
            r.i32()?;
            imports.push(r.fname(&names)?);
        }
        let mut r = Reader::new(bytes.get(eo..).ok_or("Invalid startup export offset")?);
        let mut exports = Vec::new();
        for _ in 0..ec {
            let class = r.i32()?;
            r.i32()?;
            let outer = r.i32()?;
            let name = r.fname(&names)?;
            r.take(12)?;
            let size = r.count(512 * LIMIT)?;
            let start = r.count(512 * LIMIT)?;
            r.take(4)?;
            let nets = r.count(1024)?;
            r.take(nets * 4 + 20)?;
            let class = if class < 0 {
                let idx = class
                    .checked_neg()
                    .and_then(|c| c.checked_sub(1))
                    .ok_or("Invalid startup class")? as usize;
                imports
                    .get(idx)
                    .ok_or("Invalid startup imported class")?
                    .clone()
            } else {
                String::new()
            };
            exports.push(StartupExport {
                class,
                name,
                outer,
                start,
                size,
            });
        }
        Ok(Self {
            file,
            names,
            exports,
            chunks,
        })
    }
    fn exact(&self, class: &str, name: &str, outer: Option<i32>) -> Result<usize, String> {
        let mut matches = self.exports.iter().enumerate().filter(|(_, e)| {
            e.class == class && e.name == name && outer.is_none_or(|o| e.outer == o)
        });
        let i = matches
            .next()
            .ok_or("Exact startup icon export unavailable")?
            .0;
        if matches.next().is_some() {
            return Err("Ambiguous startup icon export".into());
        }
        Ok(i)
    }
    fn data(&mut self, index: usize) -> Result<Vec<u8>, String> {
        let e = self
            .exports
            .get(index)
            .ok_or("Invalid startup export index")?;
        let end = e
            .start
            .checked_add(e.size)
            .ok_or("Startup export overflow")?;
        if e.size == 0 || e.size > LIMIT {
            return Err("Startup icon export exceeds limit".into());
        }
        let mut out = vec![0; e.size];
        let mut copied = 0;
        for &[a, n, c, m] in &self.chunks {
            let z = a.checked_add(n).ok_or("Startup chunk overflow")?;
            if a >= end || z <= e.start {
                continue;
            }
            self.file
                .seek(SeekFrom::Start(c as u64))
                .map_err(|e| e.to_string())?;
            let mut chunk = vec![0; m];
            self.file
                .read_exact(&mut chunk)
                .map_err(|e| e.to_string())?;
            let decoded = decode_chunk(&chunk, n, 8 * LIMIT)?;
            let first = a.max(e.start);
            let last = z.min(end);
            out[first - e.start..last - e.start].copy_from_slice(&decoded[first - a..last - a]);
            copied += last - first;
        }
        if copied != out.len() {
            return Err("Startup icon export has missing chunks".into());
        }
        Ok(out)
    }
}
fn startup_icon(
    p: &mut Startup,
    package: &str,
    movie: &str,
    symbol: &str,
) -> Result<Vec<u8>, String> {
    let outer = p.exact("Package", package, Some(0))? as i32 + 1;
    let movie_index = p.exact("GFxMovieInfo", movie, Some(outer))?;
    let movie = p.data(movie_index)?;
    let mut r = Reader::new(&movie);
    r.i32()?;
    let props = properties(&mut r, &p.names)?;
    let raw = property(&props, "RawData", "ArrayProperty")?;
    let mut r = Reader::new(raw.data);
    let n = r.count(LIMIT)?;
    let raw = r.take(n)?;
    if r.pos != r.bytes.len() {
        return Err("Invalid startup GFX array".into());
    }
    let (label, w, h) = gfx_image(raw, symbol)?;
    let index = p.exact("Texture2D", &label, Some(outer))?;
    let texture = p.data(index)?;
    startup_texture(&texture, &p.names, p.exports[index].start, w, h)
}
fn startup_texture(
    bytes: &[u8],
    names: &[String],
    export_start: usize,
    w: u32,
    h: u32,
) -> Result<Vec<u8>, String> {
    let mut r = Reader::new(bytes);
    r.i32()?;
    let mut values = HashMap::new();
    let mut ended = false;
    for _ in 0..128 {
        let name = r.fname(names)?;
        if name.eq_ignore_ascii_case("none") {
            ended = true;
            break;
        }
        let kind = r.fname(names)?;
        let n = r.count(LIMIT)?;
        if r.u32()? != 0 {
            return Err("Unsupported startup texture property index".into());
        }
        if kind == "ByteProperty" {
            r.fname(names)?;
        }
        if kind == "StructProperty" {
            r.fname(names)?;
        }
        let n = if kind == "BoolProperty" { 4 } else { n };
        let data = r.take(n)?.to_vec();
        if values.insert(name, (kind, data)).is_some() {
            return Err("Duplicate startup texture property".into());
        }
    }
    if !ended {
        return Err("Startup texture property limit exceeded".into());
    }
    let integer = |name: &str| -> Result<u32, String> {
        let (kind, bytes) = values
            .get(name)
            .ok_or_else(|| format!("Missing startup texture {name}"))?;
        if kind != "IntProperty" || bytes.len() != 4 {
            return Err("Invalid startup texture integer".into());
        }
        Ok(u32::from_le_bytes(bytes[..].try_into().unwrap()))
    };
    if integer("SizeX")? != w
        || integer("SizeY")? != h
        || w == 0
        || h == 0
        || w > 256
        || h > 256
        || !w.is_multiple_of(4)
        || !h.is_multiple_of(4)
    {
        return Err("Unsupported startup icon dimensions".into());
    }
    let (kind, format) = values
        .get("Format")
        .ok_or("Missing startup texture format")?;
    let mut f = Reader::new(format);
    if kind != "ByteProperty" || f.fname(names)? != "PF_DXT5" || f.pos != f.bytes.len() {
        return Err("Unsupported startup texture format".into());
    }
    // SourceArt is empty. One inline mip follows as UE bulk-data flags,
    // element count, disk size, absolute payload offset, payload, width, height.
    if r.u32()? != 0 || r.u32()? != 0 || r.u32()? != 0 {
        return Err("Unsupported startup source art".into());
    }
    let source_offset = r.u32()? as usize;
    if source_offset != export_start + r.pos || r.u32()? != 1 {
        return Err("Unsupported startup mip count".into());
    }
    let flags = r.u32()?;
    let count = r.count(LIMIT)?;
    let disk = r.count(LIMIT)?;
    let offset = r.u32()? as usize;
    if flags != 0
        || count != w as usize * h as usize
        || disk != count
        || offset != export_start + r.pos
    {
        return Err("Invalid startup inline texture range".into());
    }
    let payload = r.take(count)?;
    if r.u32()? != w || r.u32()? != h || r.pos != r.bytes.len() {
        return Err("Invalid startup mip dimensions or tail".into());
    }
    let image = decode_bc3(payload, w, h)?;
    encode_png(&image)
}
/// Deduplicate exact references and reuse startup metadata within one publication.
/// No installed assets or decoded bytes persist beyond this bounded call.
pub fn decode_icons(
    root: &Path,
    resources: &[String],
) -> std::collections::BTreeMap<String, Result<Vec<u8>, String>> {
    let mut output = std::collections::BTreeMap::new();
    if resources.len() > 64 {
        for resource in resources.iter() {
            output.insert(resource.clone(), Err("Icon batch exceeds limit".into()));
        }
        return output;
    }
    let root = root.canonicalize().map_err(|e| e.to_string());
    let mut startup: Option<Result<Startup, String>> = None;
    for resource in resources {
        if output.contains_key(resource) {
            continue;
        }
        let result = (|| {
            let root = root.as_ref().map_err(Clone::clone)?;
            let (package, movie, symbol) = resource_parts(resource)?;
            if matches!(
                package,
                "DUIICONSLIBS101" | "DUIICONSLIBS102" | "DUIICONSLIBS103"
            ) {
                let startup = startup.get_or_insert_with(|| Startup::open(root));
                return match startup {
                    Ok(p) => startup_icon(p, package, movie, symbol),
                    Err(e) => Err(e.clone()),
                };
            }
            decode_icon(root, resource)
        })();
        output.insert(resource.clone(), result);
    }
    output
}
pub fn decode_icon(root: &Path, resource: &str) -> Result<Vec<u8>, String> {
    let (package, movie, symbol) = resource_parts(resource)?;
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    if matches!(
        package,
        "DUIICONSLIBS101" | "DUIICONSLIBS102" | "DUIICONSLIBS103"
    ) {
        return startup_icon(&mut Startup::open(&root)?, package, movie, symbol);
    }
    let raw = read_confined(
        &root,
        &PathBuf::from(format!("UI/ICONS/LIBRARIES/{package}.UPK")),
        16 * LIMIT,
    )?;
    let pkg = Package::parse(&raw)?;
    let mut matches = pkg
        .exports
        .iter()
        .filter(|e| e.name == movie && pkg.class(e).is_ok_and(|c| c == "GFxMovieInfo"));
    let e = matches.next().ok_or("Exact icon movie unavailable")?;
    if matches.next().is_some() {
        return Err("Ambiguous exact icon movie".into());
    }
    if e.name != movie {
        return Err("Exact icon movie unavailable".into());
    }
    let mut r = pkg.export_reader(e)?;
    let props = properties(&mut r, &pkg.names)?;
    let raw = property(&props, "RawData", "ArrayProperty")?;
    let mut p = Reader::new(raw.data);
    let n = p.count(LIMIT)?;
    let gfx = p.take(n)?;
    if p.pos != p.bytes.len() {
        return Err("Invalid GFX byte array".into());
    }
    let (label, width, height) = gfx_image(gfx, symbol)?;
    let key = format!("{package}.{label}");
    let mut r = pkg.export_reader(pkg.export("UIStreamingTextures")?)?;
    if !properties(&mut r, &pkg.names)?.is_empty() {
        return Err("Unsupported icon streaming properties".into());
    }
    let n = r.count(4096)?;
    let mut found = None;
    for _ in 0..n {
        let name = r.string()?;
        let w = r.u32()?;
        let h = r.u32()?;
        let flags = r.u32()?;
        let count = r.count(LIMIT)?;
        let offset = r.i32()?;
        let disk = r.count(LIMIT)?;
        let format = r.u32()?;
        let srgb = r.take(1)?[0];
        let cache = r.fname(&pkg.names)?;
        if name == key {
            if found.is_some() {
                return Err("Ambiguous exact icon texture".into());
            }
            found = Some((w, h, flags, count, offset, disk, format, srgb, cache));
        }
    }
    if r.pos != r.bytes.len() {
        return Err("Invalid icon streaming tail".into());
    }
    let (w, h, flags, count, offset, disk, format, srgb, cache) =
        found.ok_or("Exact icon texture unavailable")?;
    if w != width
        || h != height
        || w == 0
        || h == 0
        || w > 256
        || h > 256
        || !w.is_multiple_of(4)
        || !h.is_multiple_of(4)
        || flags != 17
        || count != w as usize * h as usize
        || offset != -1
        || disk < 24
        || format != 7
        || srgb > 1
        || cache != "UI"
    {
        return Err("Unsupported exact icon texture encoding".into());
    }
    let mappings = remap(&read_confined(
        &root,
        Path::new("TEXTUREFILECACHE/UI_REMAP.BIN"),
        8 * LIMIT,
    )?)?;
    let packed = *mappings
        .get(&texture_hash(&key))
        .ok_or("Missing exact icon cache remap")?;
    let shard = packed & 0xfff;
    if shard > 64 {
        return Err("Unsupported icon cache shard".into());
    }
    let filename = if shard == 0 {
        "UI.TFC".into()
    } else {
        format!("UI{shard}.TFC")
    };
    let mut f = open_confined(&root, &Path::new("TEXTUREFILECACHE").join(filename))?;
    let start = (packed & !0xfff) as u64;
    if start
        .checked_add(disk as u64)
        .is_none_or(|end| end > f.metadata().map(|m| m.len()).unwrap_or(0))
    {
        return Err("Icon chunk exceeds cache".into());
    }
    f.seek(SeekFrom::Start(start)).map_err(|e| e.to_string())?;
    let mut chunk = vec![0; disk];
    f.read_exact(&mut chunk).map_err(|e| e.to_string())?;
    let decoded = decode_chunk(&chunk, count, LIMIT)?;
    let image = decode_bc3(&decoded, w, h)?;
    encode_png(&image)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_resources() {
        for s in [
            "../X/Y#1",
            "DUIICONSLIBS101/UIICONSLIBS102#046",
            "DUIICONSLIBS101/UIICONSLIBS101#../046",
            "DUIICONSLIBS101/UIICONSLIBS101#",
            "DUIICONSLIBS101/UIICONSLIBS101#x/y",
        ] {
            assert!(resource_parts(s).is_err());
        }
        assert!(resource_parts("DUIICONSLIBS101/UIICONSLIBS101#046").is_ok());
    }
    #[test]
    fn bc3_colors_and_alpha() {
        let mut b = [0u8; 16];
        b[0] = 255;
        b[8..10].copy_from_slice(&0xf800u16.to_le_bytes());
        let im = decode_bc3(&b, 4, 4).unwrap();
        assert_eq!(im.get_pixel(0, 0).0, [255, 0, 0, 255]);
        b[2] = 1;
        assert_eq!(decode_bc3(&b, 4, 4).unwrap().get_pixel(0, 0).0[3], 0);
        assert!(decode_bc3(&[], u32::MAX, 4).is_err());
        assert!(decode_bc3(&b, 3, 4).is_err());
        assert!(decode_bc3(&b[..15], 4, 4).is_err());
    }
    #[test]
    fn gfx_joins_ids_instead_of_symbol_order() {
        fn tag(out: &mut Vec<u8>, code: u16, data: &[u8]) {
            out.extend(((code << 6) | data.len() as u16).to_le_bytes());
            out.extend(data);
        }
        let mut raw = b"GFX\x09\0\0\0\0\x08\0\0\0\0\0".to_vec();
        let mut image = 42u32.to_le_bytes().to_vec();
        for n in [14u16, 64, 64] {
            image.extend(n.to_le_bytes());
        }
        image.extend(b"\x03abc\x07abc.dds");
        tag(&mut raw, 1009, &image);
        let mut export = 1u16.to_le_bytes().to_vec();
        export.extend(42u16.to_le_bytes());
        export.extend(b"046\0");
        tag(&mut raw, 56, &export);
        tag(&mut raw, 0, &[]);
        let len = raw.len() as u32;
        raw[4..8].copy_from_slice(&len.to_le_bytes());
        assert_eq!(gfx_image(&raw, "046").unwrap(), ("abc".into(), 64, 64));
        assert!(gfx_image(&raw, "abc").is_err());
        raw.pop();
        assert!(gfx_image(&raw, "046").is_err());
    }
    #[test]
    fn source_confines_symlinks_and_requires_exact_package() {
        let path = std::env::temp_dir().join(format!("dcuo-icon-test-{}", std::process::id()));
        std::fs::create_dir_all(path.join("UI/ICONS/LIBRARIES")).unwrap();
        std::os::unix::fs::symlink(
            "/etc/hosts",
            path.join("UI/ICONS/LIBRARIES/DUIICONSLIBS123.UPK"),
        )
        .unwrap();
        assert!(
            decode_icon(&path, "DUIICONSLIBS123/UIICONSLIBS123#321")
                .unwrap_err()
                .contains("escapes")
        );
        assert!(decode_icon(&path, "DUIICONSLIBS102/UIICONSLIBS102#071").is_err());
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn startup_inline_mip_checks_format_offsets_and_bounds() {
        let names: Vec<String> = [
            "NONE",
            "SizeX",
            "SizeY",
            "IntProperty",
            "Format",
            "ByteProperty",
            "EPixelFormat",
            "PF_DXT5",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        fn fname(b: &mut Vec<u8>, i: u32) {
            b.extend(i.to_le_bytes());
            b.extend(0u32.to_le_bytes());
        }
        let mut b = vec![255; 4];
        for i in [1, 2] {
            fname(&mut b, i);
            fname(&mut b, 3);
            b.extend(4u32.to_le_bytes());
            b.extend(0u32.to_le_bytes());
            b.extend(4u32.to_le_bytes());
        }
        fname(&mut b, 4);
        fname(&mut b, 5);
        b.extend(8u32.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        fname(&mut b, 6);
        fname(&mut b, 7);
        fname(&mut b, 0);
        b.extend([0; 12]);
        let source = b.len() + 4;
        b.extend((source as u32).to_le_bytes());
        b.extend(1u32.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b.extend(16u32.to_le_bytes());
        b.extend(16u32.to_le_bytes());
        let offset = b.len() + 4;
        b.extend((offset as u32).to_le_bytes());
        let mut pixels = [0; 16];
        pixels[0] = 255;
        pixels[8..10].copy_from_slice(&0xf800u16.to_le_bytes());
        b.extend(pixels);
        b.extend(4u32.to_le_bytes());
        b.extend(4u32.to_le_bytes());
        let png = startup_texture(&b, &names, 0, 4, 4).unwrap();
        assert_eq!(
            image::load_from_memory(&png)
                .unwrap()
                .to_rgba8()
                .get_pixel(0, 0)
                .0,
            [255, 0, 0, 255]
        );
        assert!(startup_texture(&b, &names, 1, 4, 4).is_err());
        assert!(startup_texture(&b, &names, 0, 512, 512).is_err());
        b.pop();
        assert!(startup_texture(&b, &names, 0, 4, 4).is_err());
    }
    #[test]
    fn batch_deduplicates_and_limits_work() {
        let bad = "invalid".to_string();
        let map = decode_icons(Path::new("/"), &[bad.clone(), bad.clone()]);
        assert_eq!(map.len(), 1);
        assert!(map[&bad].is_err());
        let resources = vec![bad.clone(); 65];
        assert_eq!(
            decode_icons(Path::new("/"), &resources)[&bad]
                .as_ref()
                .unwrap_err(),
            "Icon batch exceeds limit"
        );
    }
    #[test]
    fn malformed_gfx() {
        assert!(gfx_image(b"GFX\x09", "001").is_err());
        assert!(gfx_image(&vec![0; LIMIT + 1], "001").is_err());
    }
    #[test]
    #[ignore = "Explicit installed startup library assets only"]
    fn installed_exact_startup_103() {
        let root = std::env::var_os("DCUO_LOCAL_IMPORT_SOURCE").unwrap();
        let resources: Vec<String> = ["05", "03", "30", "21", "191", "355", "061", "158"]
            .into_iter()
            .map(|label| format!("DUIICONSLIBS103/UIICONSLIBS103#{label}"))
            .collect();
        let result = decode_icons(Path::new(&root), &resources);
        assert_eq!(result.len(), 8);
        for (resource, result) in result {
            let png = result.unwrap_or_else(|error| panic!("{resource}: {error}"));
            let im = image::load_from_memory(&png).unwrap();
            assert_eq!((im.width(), im.height()), (64, 64));
            assert!(png.len() <= 64 * 1024);
        }
    }
    #[test]
    #[ignore = "Explicit installed assets only; local output optional"]
    fn installed_exact_icon() {
        let root = std::env::var_os("DCUO_LOCAL_IMPORT_SOURCE").unwrap();
        let resource = std::env::var("DCUO_ICON_RESOURCE")
            .unwrap_or_else(|_| "DUIICONSLIBS123/UIICONSLIBS123#321".into());
        let png = decode_icon(Path::new(&root), &resource).unwrap();
        let im = image::load_from_memory(&png).unwrap();
        assert!(im.width() <= 256);
        if let Some(path) = std::env::var_os("DCUO_ICON_OUTPUT") {
            std::fs::write(path, png).unwrap();
        }
    }
}
