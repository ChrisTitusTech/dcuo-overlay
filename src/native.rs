//! Experimental build-specific physics observations, not an accepted map profile.
//! Every foreign read uses the bounded read-only memory interface.
use crate::{memory, process};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

pub const PROFILE_ID: &str = "a5a14396497bd05336e0267d14a1c3442fec5c21b4c28b8d298fa0a6e0105cf3";
const NAMES_RVA: usize = 0x2146c40;
const OBJECTS_RVA: usize = 0x2146cc8;
const ENGINE_RVA: usize = 0x2130f00;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroundSpawnKind {
    Blue,
    Gold,
    Green,
    Grey,
    Purple,
    Other,
}
fn ground_spawn_kind(name: &str) -> Option<GroundSpawnKind> {
    use GroundSpawnKind::*;
    match name {
        "DSGPROGEN107_MAGNIFYINGGLASS_BLUE_DO" => Some(Blue),
        "DSGPROGEN107_MAGNIFYINGGLASS_GOLD_DO" => Some(Gold),
        "DSGPROGEN107_MAGNIFYINGGLASS_GREEN_DO" => Some(Green),
        "DSGPROGEN107_MAGNIFYINGGLASS_GREY_DO" => Some(Grey),
        "DSGPROGEN107_MAGNIFYINGGLASS_PURPLE_DO" | "DSGPROGEN107_QUESTIONMARK_PURPLE_DO" => {
            Some(Purple)
        }
        "MAGNIFYINGGLASS_AFOOLS_DO" => Some(Other),
        _ => None,
    }
}
#[derive(Clone, Debug)]
pub struct Collectible {
    pub kind: GroundSpawnKind,
    pub position: [f32; 3],
}

#[derive(Clone, Debug)]
pub struct Sample {
    pub position: [f32; 3],
    /// Legacy nearby recognized spawn, with no collection-status claim.
    pub collectible: Option<Collectible>,
    pub ground_spawns: Vec<Collectible>,
    pub ground_spawns_status: String,
    pub health: f32,
    pub max_health: f32,
    pub power: f32,
    pub max_power: f32,
    /// Object ancestry only. This is NOT a proven current area or floor.
    pub outer_package: String,
    /// Experimental loaded/visible streaming context; no calibrated map or floor claim.
    pub area_key: Option<String>,
    pub area_status: String,
    pub observed_at: Instant,
    owner: Ownership,
}
impl Sample {
    #[cfg(test)]
    pub(crate) fn synthetic(owner_index: u32) -> Self {
        Self {
            position: [1.0, 2.0, 3.0],
            collectible: None,
            ground_spawns: Vec::new(),
            ground_spawns_status: "Ground spawns off".into(),
            health: 50.0,
            max_health: 100.0,
            power: 20.0,
            max_power: 80.0,
            outer_package: "SyntheticPackage".into(),
            area_key: None,
            area_status: "No synthetic area".into(),
            observed_at: Instant::now(),
            owner: Ownership {
                objects: [(0x10000, owner_index); 4],
                classes: [0x20000; 4],
                outers: vec![(0x30000, 1)],
                package: "SyntheticPackage".into(),
                area: None,
                world_pointer: 0,
            },
        }
    }
    pub fn same_owner(&self, other: &Self) -> bool {
        self.owner == other.owner
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Ownership {
    objects: [(usize, u32); 4],
    classes: [usize; 4],
    outers: Vec<(usize, u32)>,
    package: String,
    area: Option<AreaContext>,
    world_pointer: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AreaContext {
    world: (usize, u32),
    array: (usize, u32, u32),
    members: Vec<usize>,
    signals: Vec<(usize, u32, usize, String)>,
    key: Option<String>,
}
fn area_signal(package: &str, number: u32) -> Result<bool, String> {
    let relevant = package.ends_with("_AUDIO") || package.ends_with("_DESIGNERDATA");
    if relevant && number != 0 {
        return Err("Numbered area signal unsupported".into());
    }
    Ok(relevant)
}
pub(crate) fn valid_area_key(key: &str) -> bool {
    (1..=32).contains(&key.len())
        && key.split('_').all(|component| {
            !component.is_empty()
                && component
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        })
}
fn matching_designer(package: &str, key: &str) -> bool {
    if package.len() > 128 {
        return false;
    }
    let Some(stem) = package.strip_suffix("_DESIGNERDATA") else {
        return false;
    };
    if stem == key {
        return true;
    }
    stem.strip_prefix(key)
        .and_then(|suffix| suffix.strip_prefix('_'))
        .is_some_and(|variant| variant.split('_').all(valid_area_key))
}
fn supported_area(packages: &[String]) -> Option<String> {
    if packages.len() > 2048 {
        return None;
    }
    let audio: Vec<_> = packages
        .iter()
        .filter_map(|name| name.strip_suffix("_AUDIO"))
        .collect();
    if audio.len() != 1 || !valid_area_key(audio[0]) {
        return None;
    }
    let key = audio[0];
    let designers: Vec<_> = packages
        .iter()
        .filter(|name| name.ends_with("_DESIGNERDATA"))
        .collect();
    if designers.is_empty() || designers.iter().any(|name| !matching_designer(name, key)) {
        return None;
    }
    Some(key.into())
}

type CollectibleSchemas = HashMap<[(usize, u32); 3], Vec<(usize, u32)>>;

#[derive(Default)]
struct GroundScan {
    owner: Option<Ownership>,
    table: usize,
    count: usize,
    cursor: usize,
    completed: bool,
    candidates: Vec<(usize, u32)>,
    refresh_cursor: usize,
    class_names: HashMap<usize, bool>,
}
fn rotated_indices(start: usize, count: usize) -> impl Iterator<Item = usize> {
    (0..count).map(move |offset| (start % count.max(1) + offset) % count.max(1))
}
fn next_scan_cursor(index: u32, interrupted: bool) -> usize {
    index as usize + usize::from(!interrupted)
}
pub struct Reader {
    ground_spawns_enabled: bool,
    ground_scan: GroundScan,
    candidate: process::Candidate,
    names: usize,
    name_count: u32,
    name_header: Vec<u8>,
    classes: Option<[usize; 4]>,
    cancel: Option<Arc<AtomicBool>>,
    maps: RefCell<Vec<process::Mapping>>,
    names_cache: RefCell<HashMap<u32, String>>,
    metadata_cache: RefCell<HashMap<usize, Vec<u8>>>,
    scope_start: Cell<Instant>,
    scope_reads: Cell<usize>,
    scope_bytes: Cell<usize>,
    collectible_deadline: Cell<Option<Instant>>,
    area_schemas: RefCell<HashSet<(usize, u32)>>,
    collectible_schemas: RefCell<CollectibleSchemas>,
}
fn add(address: usize, offset: usize) -> Result<usize, String> {
    address
        .checked_add(offset)
        .ok_or_else(|| "Address overflow".into())
}
fn pointer(value: u64) -> Result<usize, String> {
    if !(0x10000..=0x0000_7fff_ffff_ffff).contains(&value) {
        return Err("Null or noncanonical object pointer".into());
    }
    usize::try_from(value).map_err(|_| "Pointer exceeds host address size".into())
}
fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let value = bytes.get(offset..offset + 4).ok_or("Short integer field")?;
    Ok(u32::from_le_bytes(
        value.try_into().map_err(|_| "Short integer field")?,
    ))
}
fn u64_at(bytes: &[u8], offset: usize) -> Result<u64, String> {
    let value = bytes.get(offset..offset + 8).ok_or("Short pointer field")?;
    Ok(u64::from_le_bytes(
        value.try_into().map_err(|_| "Short pointer field")?,
    ))
}
fn float(bytes: &[u8], offset: usize) -> Result<f32, String> {
    let value = f32::from_bits(u32_at(bytes, offset)?);
    if !value.is_finite() {
        return Err("Non-finite physics value".into());
    }
    Ok(value)
}
fn validate_values(position: [f32; 3], values: [f32; 4]) -> Result<(), String> {
    if position
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 10_000_000.0)
    {
        return Err("Position outside experimental safety bounds".into());
    }
    let [max_health, health, max_power, power] = values;
    for (current, maximum) in [(health, max_health), (power, max_power)] {
        if !current.is_finite()
            || !maximum.is_finite()
            || maximum <= 0.0
            || maximum > 100_000_000.0
            || current < 0.0
            || current > maximum
        {
            return Err("Resource outside experimental safety bounds".into());
        }
    }
    Ok(())
}

fn validate_collectible_flag(offset: usize, mask: u32) -> Result<(), String> {
    if !(0x54..=0x3bc).contains(&offset) || !offset.is_multiple_of(4) || !mask.is_power_of_two() {
        return Err("Invalid collectible lifetime flag schema".into());
    }
    Ok(())
}
fn collectible_actor_snapshot(bytes: &[u8], flags: &[(usize, u32)]) -> Result<Vec<u8>, String> {
    let mut snapshot = Vec::new();
    for (offset, size) in [(8, 4), (0x3c, 24), (0xd0, 12), (0x164, 8), (0x3b8, 8)] {
        snapshot.extend_from_slice(
            bytes
                .get(offset..offset + size)
                .ok_or("Short collectible snapshot")?,
        );
    }
    for &(offset, mask) in flags {
        validate_collectible_flag(offset, mask)?;
        snapshot.extend_from_slice(&(u32_at(bytes, offset)? & mask).to_le_bytes());
    }
    Ok(snapshot)
}
fn collectible_definition_snapshot(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut snapshot = bytes
        .get(8..12)
        .ok_or("Short definition identity")?
        .to_vec();
    snapshot.extend_from_slice(bytes.get(0x3c..0x54).ok_or("Short definition header")?);
    Ok(snapshot)
}
fn collectible_position(
    bytes: &[u8],
    world: u64,
    player: [f32; 3],
    flags: &[(usize, u32)],
) -> Result<[f32; 3], String> {
    pointer(world)?;
    if u64_at(bytes, 0x164)? != world {
        return Err("Nearby collectible belongs to a different world".into());
    }
    for &(offset, mask) in flags {
        validate_collectible_flag(offset, mask)?;
        if u32_at(bytes, offset)? & mask != 0 {
            return Err("Nearby collectible hidden or deleted".into());
        }
    }
    let position = [
        float(bytes, 0xd0)?,
        float(bytes, 0xd4)?,
        float(bytes, 0xd8)?,
    ];
    if position
        .iter()
        .chain(player.iter())
        .any(|v| !v.is_finite() || v.abs() > 10_000_000.0)
        || position
            .iter()
            .zip(player)
            .map(|(a, b)| (*a as f64 - b as f64).powi(2))
            .sum::<f64>()
            > 2000.0f64.powi(2)
    {
        return Err("Nearby collectible outside bounded proximity".into());
    }
    Ok(position)
}

impl Reader {
    pub fn new(pid: i32) -> Result<Self, String> {
        Self::attach(process::candidate(pid).map_err(|e| e.to_string())?)
    }
    pub fn attach(candidate: process::Candidate) -> Result<Self, String> {
        // Fingerprint before interpreting any build-specific pointers.
        let metadata = process::executable_metadata(&candidate).map_err(|e| e.to_string())?;
        if metadata.sha256 != PROFILE_ID
            || metadata.size != 35_066_880
            || metadata.machine != 0x8664
            || metadata.optional_magic != 0x20b
        {
            return Err("Unsupported executable for experimental physics reader".into());
        }
        let header = memory::read_exact(&candidate.identity, candidate.mapping.start, 64)
            .map_err(|e| e.to_string())?;
        if !metadata.matches_dos_header(&header) {
            return Err("Mapped header mismatch".into());
        }
        let candidate_pid = candidate.identity.pid;
        let mut reader = Self {
            ground_spawns_enabled: false,
            ground_scan: GroundScan::default(),
            candidate,
            names: 0,
            name_count: 0,
            name_header: Vec::new(),
            classes: None,
            cancel: None,
            maps: RefCell::new(process::mappings(candidate_pid).map_err(|e| e.to_string())?),
            names_cache: RefCell::new(HashMap::new()),
            metadata_cache: RefCell::new(HashMap::new()),
            scope_start: Cell::new(Instant::now()),
            scope_reads: Cell::new(0),
            scope_bytes: Cell::new(0),
            collectible_deadline: Cell::new(None),
            area_schemas: RefCell::new(HashSet::new()),
            collectible_schemas: RefCell::new(HashMap::new()),
        };
        reader.unchanged()?;
        let table = reader.read(reader.rva(NAMES_RVA)?, 16)?;
        reader.name_header = table.clone();
        reader.names = pointer(u64_at(&table, 0)?)?;
        reader.name_count = u32_at(&table, 8)?;
        let capacity = u32_at(&table, 12)?;
        if reader.name_count < 3
            || reader.name_count > 250_000
            || capacity < reader.name_count
            || capacity > 250_000
        {
            return Err("Invalid bounded name table".into());
        }
        for (index, expected) in [(0, "None"), (1, "ByteProperty"), (2, "IntProperty")] {
            if reader.name(index)? != expected {
                return Err("Name anchors mismatch".into());
            }
        }
        reader.unchanged()?;
        Ok(reader)
    }
    /// Use the session worker cancellation token to interrupt bounded reflection walks.
    pub fn set_cancel(&mut self, cancel: Arc<AtomicBool>) {
        self.cancel = Some(cancel);
    }
    fn unchanged(&self) -> Result<(), String> {
        let current = process::candidate(self.candidate.identity.pid).map_err(|e| e.to_string())?;
        if current.identity != self.candidate.identity || current.mapping != self.candidate.mapping
        {
            return Err("Process or executable mapping changed".into());
        }
        Ok(())
    }
    fn rva(&self, offset: usize) -> Result<usize, String> {
        add(self.candidate.mapping.start, offset)
    }
    fn read(&self, address: usize, size: usize) -> Result<Vec<u8>, String> {
        if self
            .cancel
            .as_ref()
            .is_some_and(|token| token.load(Ordering::Acquire))
        {
            return Err("Experimental read cancelled".into());
        }
        if size == 0 || size > memory::MAX_READ_BYTES {
            return Err("Experimental read exceeds byte bound".into());
        }
        if self
            .collectible_deadline
            .get()
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err("Optional collectible read exceeded time budget".into());
        }
        if self.scope_start.get().elapsed() > std::time::Duration::from_secs(3)
            || self
                .scope_bytes
                .get()
                .checked_add(size)
                .is_none_or(|bytes| bytes > 2 * 1024 * 1024)
        {
            return Err("Experimental sample exceeded time or byte budget".into());
        }
        let count = self.scope_reads.get();
        if count != 0 && count.is_multiple_of(128) {
            *self.maps.borrow_mut() =
                process::mappings(self.candidate.identity.pid).map_err(|e| e.to_string())?;
        }
        self.scope_reads.set(count + 1);
        self.scope_bytes.set(self.scope_bytes.get() + size);
        let end = add(address, size)?;
        let maps = self.maps.borrow();
        let mapping = maps
            .iter()
            .find(|map| map.readable && address >= map.start && end <= map.end)
            .ok_or("Experimental read outside readable mapping")?;
        memory::read_discovery(&self.candidate.identity, mapping, address, size)
            .map_err(|e| e.to_string())
    }
    fn read_scatter(&self, ranges: &[(usize, usize)]) -> Result<Vec<Vec<u8>>, String> {
        if self
            .cancel
            .as_ref()
            .is_some_and(|token| token.load(Ordering::Acquire))
        {
            return Err("Experimental read cancelled".into());
        }
        let bytes = ranges
            .iter()
            .try_fold(0usize, |sum, (_, len)| sum.checked_add(*len))
            .ok_or("Experimental scatter byte overflow")?;
        if self
            .collectible_deadline
            .get()
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err("Optional collectible read exceeded time budget".into());
        }
        if self.scope_start.get().elapsed() > std::time::Duration::from_secs(3)
            || self
                .scope_bytes
                .get()
                .checked_add(bytes)
                .is_none_or(|total| total > 2 * 1024 * 1024)
        {
            return Err("Experimental sample exceeded time or byte budget".into());
        }
        self.scope_bytes.set(self.scope_bytes.get() + bytes);
        self.scope_reads.set(self.scope_reads.get() + 1);
        // The scatter helper obtains fresh mappings for every batch and checks
        // process identity around the whole syscall, discarding partial batches.
        memory::read_scatter(&self.candidate.identity, ranges).map_err(|error| error.to_string())
    }
    fn ptr(&self, address: usize) -> Result<usize, String> {
        pointer(u64_at(&self.read(address, 8)?, 0)?)
    }
    fn name(&self, index: u32) -> Result<String, String> {
        if let Some(name) = self.names_cache.borrow().get(&index) {
            return Ok(name.clone());
        }
        if index >= self.name_count {
            return Err("Name index outside table".into());
        }
        let entry = self.ptr(add(
            self.names,
            (index as usize).checked_mul(8).ok_or("Name overflow")?,
        )?)?;
        let bytes = self.read(add(entry, 0x14)?, 256)?;
        let end = bytes
            .iter()
            .position(|b| *b == 0)
            .ok_or("Unterminated bounded name")?;
        if end == 0 || !bytes[..end].iter().all(|b| b.is_ascii_graphic()) {
            return Err("Invalid ANSI reflection name".into());
        }
        let name =
            String::from_utf8(bytes[..end].to_vec()).map_err(|_| "Invalid reflection encoding")?;
        if self.names_cache.borrow().len() >= 32_768 {
            return Err("Reflection name cache bound exceeded".into());
        }
        self.names_cache.borrow_mut().insert(index, name.clone());
        Ok(name)
    }
    fn object(&self, address: usize) -> Result<(u32, usize, String), String> {
        let bytes = self.read(address, 0x54)?;
        let class = pointer(u64_at(&bytes, 0x4c)?)?;
        Ok((u32_at(&bytes, 8)?, class, self.name(u32_at(&bytes, 0x44)?)?))
    }
    fn class_name(&self, address: usize) -> Result<String, String> {
        Ok(self.object(address)?.2)
    }
    fn cached_metadata(&self, address: usize, size: usize) -> Result<Vec<u8>, String> {
        if let Some(bytes) = self
            .metadata_cache
            .borrow()
            .get(&address)
            .filter(|bytes| bytes.len() == size)
        {
            return Ok(bytes.clone());
        }
        let bytes = self.read(address, size)?;
        if self.metadata_cache.borrow().len() >= 32_768 {
            return Err("Reflection metadata cache bound exceeded".into());
        }
        self.metadata_cache
            .borrow_mut()
            .insert(address, bytes.clone());
        Ok(bytes)
    }
    fn field(
        &self,
        class: usize,
        name: &str,
        kind: &str,
        offset: u32,
        size: u32,
    ) -> Result<usize, String> {
        let mut base = class;
        let mut visited = HashSet::new();
        let mut found = None;
        for _ in 0..16 {
            if !visited.insert(base) {
                return Err("Cyclic class ancestry".into());
            }
            let metadata = self.cached_metadata(base, 0x84)?;
            let mut next = u64_at(&metadata, 0x7c)?;
            let mut fields = HashSet::new();
            let mut ended = next == 0;
            for _ in 0..2048 {
                if next == 0 {
                    ended = true;
                    break;
                }
                let address = pointer(next)?;
                if !fields.insert(address) {
                    return Err("Cyclic reflection fields".into());
                }
                let bytes = self.cached_metadata(address, 0x94)?;
                if self.name(u32_at(&bytes, 0x44)?)? == name {
                    if found.is_some()
                        || self.class_name(pointer(u64_at(&bytes, 0x4c)?)?)? != kind
                        || u32_at(&bytes, 0x6c)? != 1
                        || u32_at(&bytes, 0x70)? != size
                        || (offset != u32::MAX && u32_at(&bytes, 0x90)? != offset)
                    {
                        return Err(format!("Reflection schema mismatch for {name}"));
                    }
                    found = Some(address);
                }
                next = u64_at(&bytes, 0x64)?;
            }
            if !ended && next != 0 {
                return Err("Reflection field bound exceeded".into());
            }
            let parent = u64_at(&metadata, 0x5c)?;
            if parent == 0 {
                return found.ok_or_else(|| format!("Missing reflected field {name}"));
            }
            base = pointer(parent)?;
        }
        Err("Class ancestry bound exceeded".into())
    }
    fn ownership(&self) -> Result<Ownership, String> {
        let engine = self.ptr(self.rva(ENGINE_RVA)?)?;
        let array = self.read(add(engine, 0x6b4)?, 16)?;
        if u32_at(&array, 8)? != 1 || !(1..=64).contains(&u32_at(&array, 12)?) {
            return Err("Waiting for one controlled local player".into());
        }
        let local = self.ptr(pointer(u64_at(&array, 0)?)?)?;
        let controller = self.ptr(add(local, 0x64)?)?;
        let pawn = self.ptr(add(controller, 0x290)?)?;
        let mut objects = [(0, 0); 4];
        let mut classes = [0; 4];
        for (index, address) in [engine, local, controller, pawn].into_iter().enumerate() {
            let (internal, class, _) = self.object(address)?;
            objects[index] = (address, internal);
            classes[index] = class;
        }
        for (class, expected) in classes.into_iter().zip([
            "GameEngine",
            "DCLocalPlayer",
            "DCPlayerController",
            "DCCharacterPlayer",
        ]) {
            if self.class_name(class)? != expected {
                return Err("Controlled object class mismatch".into());
            }
        }
        let class_class = self.object(classes[0])?.1;
        if self.class_name(class_class)? != "Class" || self.object(class_class)?.1 != class_class {
            return Err("Reflection metaclass anchor mismatch".into());
        }
        let mut current = pawn;
        let mut outers = Vec::new();
        let mut seen = HashSet::new();
        let mut package = String::new();
        for _ in 0..16 {
            if !seen.insert(current) {
                return Err("Cyclic object ownership".into());
            }
            let (internal, _, name) = self.object(current)?;
            outers.push((current, internal));
            let outer = u64_at(&self.read(add(current, 0x3c)?, 8)?, 0)?;
            if outer == 0 {
                package = name;
                break;
            }
            current = pointer(outer)?;
        }
        if package.is_empty() {
            return Err("Object ownership bound exceeded".into());
        }
        Ok(Ownership {
            objects,
            classes,
            outers,
            package,
            area: None,
            world_pointer: u64_at(&self.read(add(pawn, 0x164)?, 8)?, 0)?,
        })
    }
    fn validate_schema(&self, classes: [usize; 4]) -> Result<(), String> {
        for (class, name, kind, offset, size) in [
            (classes[0], "GamePlayers", "ArrayProperty", 0x6b4, 16),
            (classes[1], "Actor", "ObjectProperty", 0x64, 8),
            (classes[2], "Pawn", "ObjectProperty", 0x290, 8),
            (classes[3], "Location", "StructProperty", 0xd0, 12),
            (classes[3], "MaxHealth", "FloatProperty", 0x5f4, 4),
            (classes[3], "CurHealth", "FloatProperty", 0x5f8, 4),
            (classes[3], "MaxPower", "FloatProperty", 0x5fc, 4),
            (classes[3], "CurPower", "FloatProperty", 0x600, 4),
        ] {
            self.field(class, name, kind, offset, size)?;
        }
        Ok(())
    }
    fn has_base(&self, class: usize, expected: &str) -> Result<(), String> {
        let mut base = class;
        let mut seen = HashSet::new();
        for _ in 0..16 {
            if !seen.insert(base) {
                return Err("Cyclic area class ancestry".into());
            }
            if self.class_name(base)? == expected {
                return Ok(());
            }
            let parent = u64_at(&self.cached_metadata(base, 0x84)?, 0x5c)?;
            if parent == 0 {
                break;
            }
            base = pointer(parent)?;
        }
        Err(format!("Unsupported area object class; missing {expected}"))
    }
    fn area_schema(&self, class: usize, world: bool) -> Result<(), String> {
        let identity = (class, self.object(class)?.0);
        if self.area_schemas.borrow().contains(&identity) {
            return Ok(());
        }
        if world {
            self.has_base(class, "WorldInfo")?;
            self.field(class, "StreamingLevels", "ArrayProperty", 0x354, 16)?;
        } else {
            self.has_base(class, "LevelStreaming")?;
            self.field(class, "PackageName", "NameProperty", 0x5c, 8)?;
            self.field(class, "LoadedLevel", "ObjectProperty", 0x64, 8)?;
            let visibility = self.field(class, "bIsVisible", "BoolProperty", 0x84, 4)?;
            if u32_at(&self.read(add(visibility, 0xcc)?, 4)?, 0)? != 1 {
                return Err("Streaming visibility mask mismatch".into());
            }
        }
        if self.area_schemas.borrow().len() >= 64 {
            return Err("Area schema cache bound exceeded".into());
        }
        self.area_schemas.borrow_mut().insert(identity);
        Ok(())
    }
    fn area_context(&self, pawn: usize) -> Result<AreaContext, String> {
        let (_, pawn_class, _) = self.object(pawn)?;
        let pawn_schema = (pawn_class, self.object(pawn_class)?.0);
        if !self.area_schemas.borrow().contains(&pawn_schema) {
            self.field(pawn_class, "WorldInfo", "ObjectProperty", 0x164, 8)?;
            if self.area_schemas.borrow().len() >= 64 {
                return Err("Area schema cache bound exceeded".into());
            }
            self.area_schemas.borrow_mut().insert(pawn_schema);
        }
        let world = self.ptr(add(pawn, 0x164)?)?;
        let (world_index, class, _) = self.object(world)?;
        self.area_schema(class, true)?;
        let header = self.read(add(world, 0x354)?, 16)?;
        let count = u32_at(&header, 8)?;
        let capacity = u32_at(&header, 12)?;
        if count > 2048 || capacity < count || capacity > 2048 {
            return Err("Streaming array exceeds bounds".into());
        }
        let data = if count == 0 {
            0
        } else {
            pointer(u64_at(&header, 0)?)?
        };
        let mut members = Vec::with_capacity(count as usize);
        let bytes = (count as usize)
            .checked_mul(8)
            .ok_or("Streaming byte overflow")?;
        for offset in (0..bytes).step_by(4096) {
            let chunk = self.read(add(data, offset)?, (bytes - offset).min(4096))?;
            for slot in chunk.chunks_exact(8) {
                members.push(pointer(u64_at(slot, 0)?)?);
            }
        }
        let mut signals = Vec::new();
        let mut packages = Vec::new();
        let mut seen = HashSet::new();
        let mut validated_classes = HashSet::new();
        for batch in members.chunks(memory::MAX_SCATTER_RANGES) {
            let ranges: Vec<_> = batch.iter().map(|address| (*address, 0x88)).collect();
            let objects = self.read_scatter(&ranges)?;
            for (address, object) in batch.iter().zip(objects) {
                if !seen.insert(*address) {
                    return Err("Duplicate streaming object membership".into());
                }
                let class = pointer(u64_at(&object, 0x4c)?)?;
                if validated_classes.insert(class) {
                    self.area_schema(class, false)?;
                }
                let loaded = u64_at(&object, 0x64)?;
                if loaded == 0 || u32_at(&object, 0x84)? & 1 == 0 {
                    continue;
                }
                let loaded = pointer(loaded)?;
                let package = self.name(u32_at(&object, 0x5c)?)?;
                if area_signal(&package, u32_at(&object, 0x60)?)? {
                    signals.push((*address, u32_at(&object, 8)?, loaded, package.clone()));
                    packages.push(package);
                }
            }
        }
        // Recheck membership and header after traversal. Loaded/visible fields are
        // checked again by the second context read around the physics observation.
        if self.read(add(world, 0x354)?, 16)? != header {
            return Err("Streaming array changed during context read".into());
        }
        for offset in (0..bytes).step_by(4096) {
            let chunk = self.read(add(data, offset)?, (bytes - offset).min(4096))?;
            for (index, slot) in chunk.chunks_exact(8).enumerate() {
                if pointer(u64_at(slot, 0)?)? != members[offset / 8 + index] {
                    return Err("Streaming membership changed during context read".into());
                }
            }
        }
        Ok(AreaContext {
            world: (world, world_index),
            array: (data, count, capacity),
            members,
            signals,
            key: supported_area(&packages),
        })
    }
    fn collectible_actor(
        &self,
        owner: &Ownership,
        player: [f32; 3],
        actor: usize,
        nearby: bool,
    ) -> Result<Collectible, String> {
        let controller = owner.objects[2].0;
        let target_bytes = self.read(add(controller, 0x49c)?, 8)?;

        let actor_before = self.read(actor, 0x3c0)?;
        let actor_class = pointer(u64_at(&actor_before, 0x4c)?)?;
        let definition = pointer(u64_at(&actor_before, 0x3b8)?)?;
        let definition_before = self.read(definition, 0x54)?;
        let definition_class = pointer(u64_at(&definition_before, 0x4c)?)?;
        let classes = [owner.classes[2], actor_class, definition_class];
        let mut identities = [(0, 0); 3];
        for (index, class) in classes.into_iter().enumerate() {
            identities[index] = (class, self.object(class)?.0);
        }
        if self.class_name(actor_class)? != "DCServerPhysicsObject"
            || self.class_name(definition_class)? != "DCDamageableObject"
            || ground_spawn_kind(&self.name(u32_at(&definition_before, 0x44)?)?).is_none()
            || u32_at(&definition_before, 0x48)? != 0
        {
            return Err("Unsupported nearby collectible definition".into());
        }
        let cached_flags = self.collectible_schemas.borrow().get(&identities).cloned();
        let flags = if let Some(flags) = cached_flags {
            flags
        } else {
            self.field(
                classes[0],
                "potentialInteractable",
                "ObjectProperty",
                0x49c,
                8,
            )?;
            self.field(actor_class, "DamageObj", "ObjectProperty", 0x3b8, 8)?;
            self.field(actor_class, "Location", "StructProperty", 0xd0, 12)?;
            self.field(actor_class, "WorldInfo", "ObjectProperty", 0x164, 8)?;
            let mut flags = Vec::new();
            for name in ["bDeleteMe", "bHidden"] {
                let field = self.field(actor_class, name, "BoolProperty", u32::MAX, 4)?;
                let offset = u32_at(&self.cached_metadata(field, 0x94)?, 0x90)? as usize;
                let mask = u32_at(&self.read(add(field, 0xcc)?, 4)?, 0)?;
                validate_collectible_flag(offset, mask)?;
                flags.push((offset, mask));
            }
            if self.collectible_schemas.borrow().len() >= 64 {
                return Err("Collectible schema cache bound exceeded".into());
            }
            self.collectible_schemas
                .borrow_mut()
                .insert(identities, flags.clone());
            flags
        };
        let position = collectible_position(
            &actor_before,
            owner.world_pointer,
            if nearby {
                player
            } else {
                [
                    float(&actor_before, 0xd0)?,
                    float(&actor_before, 0xd4)?,
                    float(&actor_before, 0xd8)?,
                ]
            },
            &flags,
        )?;
        // Bind relevant fields without rejecting unrelated per-frame actor changes.
        if collectible_actor_snapshot(&self.read(actor, 0x3c0)?, &flags)?
            != collectible_actor_snapshot(&actor_before, &flags)?
            || collectible_definition_snapshot(&self.read(definition, 0x54)?)?
                != collectible_definition_snapshot(&definition_before)?
            || (nearby && self.read(add(controller, 0x49c)?, 8)? != target_bytes)
        {
            return Err("Nearby collectible changed during sample".into());
        }
        for (class, identity) in identities {
            if self.object(class)?.0 != identity {
                return Err("Collectible class identity changed".into());
            }
        }
        Ok(Collectible {
            position,
            kind: ground_spawn_kind(&self.name(u32_at(&definition_before, 0x44)?)?)
                .ok_or("Unsupported spawn")?,
        })
    }
    pub fn set_ground_spawns_enabled(&mut self, enabled: bool) {
        if self.ground_spawns_enabled != enabled {
            self.ground_scan = GroundScan::default();
            self.ground_spawns_enabled = enabled;
        }
    }
    fn optional_interrupted(&self) -> bool {
        self.collectible_deadline
            .get()
            .is_some_and(|d| Instant::now() >= d)
            || self
                .cancel
                .as_ref()
                .is_some_and(|c| c.load(Ordering::Acquire))
    }
    fn ground_spawns(
        &mut self,
        owner: &Ownership,
        player: [f32; 3],
    ) -> Result<(Vec<Collectible>, String), String> {
        let table = self.read(self.rva(OBJECTS_RVA)?, 16)?;
        let base = pointer(u64_at(&table, 0)?)?;
        let count = u32_at(&table, 8)? as usize;
        let capacity = u32_at(&table, 12)? as usize;
        if !(1..=250_000).contains(&count) || capacity < count || capacity > 250_000 {
            self.ground_scan = GroundScan::default();
            return Err("Object table outside bounds".into());
        }
        if self.ground_scan.owner.as_ref() != Some(owner) || self.ground_scan.table != base {
            self.ground_scan = GroundScan {
                owner: Some(owner.clone()),
                table: base,
                ..Default::default()
            };
        }
        if self.ground_scan.count != count {
            self.ground_scan.completed = false;
            self.ground_scan.count = count;
        }
        if self.ground_scan.cursor >= count {
            self.ground_scan.cursor = 0;
        }
        let candidates = self.ground_scan.candidates.clone();
        let mut result = Vec::new();
        let discovery_deadline = self.collectible_deadline.get();
        // Reserve forty milliseconds of the optional budget for discovery.
        self.collectible_deadline
            .set(discovery_deadline.map(|d| d - std::time::Duration::from_millis(40)));
        let mut refreshed = 0;
        for candidate_index in rotated_indices(self.ground_scan.refresh_cursor, candidates.len()) {
            let (address, index) = candidates[candidate_index];
            if self.optional_interrupted() {
                self.ground_scan.refresh_cursor = candidate_index;
                break;
            }
            self.ground_scan.refresh_cursor = (candidate_index + 1) % candidates.len();
            refreshed += 1;
            if index as usize >= count {
                self.ground_scan
                    .candidates
                    .retain(|entry| *entry != (address, index));
                continue;
            }
            let Ok(slot) = self.read(add(base, index as usize * 8)?, 8) else {
                if self.optional_interrupted() {
                    break;
                }
                self.ground_scan
                    .candidates
                    .retain(|entry| *entry != (address, index));
                continue;
            };
            let identity = self.object(address).map(|object| object.0);
            if self.optional_interrupted() {
                break;
            }
            if u64_at(&slot, 0)? != address as u64 || identity != Ok(index) {
                self.ground_scan
                    .candidates
                    .retain(|entry| *entry != (address, index));
                continue;
            }
            match self.collectible_actor(owner, player, address, false) {
                Ok(spawn) => result.push(spawn),
                Err(_) => {
                    if self.optional_interrupted() {
                        break;
                    }
                    self.ground_scan
                        .candidates
                        .retain(|entry| *entry != (address, index));
                }
            }
        }
        let refresh_limited = refreshed < candidates.len() || self.optional_interrupted();
        self.collectible_deadline.set(discovery_deadline);
        let discovery = (|| -> Result<(), String> {
            let start = self.ground_scan.cursor;
            let end = (start + 4096).min(count);
            let ranges: Vec<_> = (start..end)
                .step_by(512)
                .map(|offset| Ok((add(base, offset * 8)?, (end - offset).min(512) * 8)))
                .collect::<Result<_, String>>()?;
            let slots: Vec<u8> = self.read_scatter(&ranges)?.into_iter().flatten().collect();
            let mut actors = Vec::new();
            for index in start..end {
                if let Ok(address) = pointer(u64_at(&slots, (index - start) * 8)?) {
                    let readable = self.maps.borrow().iter().any(|m| {
                        m.readable
                            && address >= m.start
                            && address.checked_add(0x54).is_some_and(|e| e <= m.end)
                    });
                    if readable {
                        actors.push((address, index as u32));
                    }
                }
            }
            for batch in actors.chunks(128) {
                let ranges: Vec<_> = batch.iter().map(|(address, _)| (*address, 0x54)).collect();
                let headers = match self.read_scatter(&ranges) {
                    Ok(headers) => headers.into_iter().map(Some).collect::<Vec<_>>(),
                    Err(error) => {
                        if self.optional_interrupted() {
                            return Err(error);
                        }
                        let mut headers = Vec::new();
                        for &(address, _) in batch {
                            let header = self.read(address, 0x54);
                            if self.optional_interrupted() {
                                self.ground_scan.cursor = next_scan_cursor(batch[0].1, true);
                                return Err("Optional scan budget reached".into());
                            }
                            headers.push(header.ok());
                        }
                        headers
                    }
                };
                for (&(address, index), bytes) in batch.iter().zip(headers) {
                    if self
                        .collectible_deadline
                        .get()
                        .is_some_and(|d| Instant::now() >= d)
                    {
                        return Err("Optional scan budget reached".into());
                    }
                    self.ground_scan.cursor = next_scan_cursor(index, false);
                    let Some(bytes) = bytes else {
                        continue;
                    };
                    if u32_at(&bytes, 8)? != index {
                        continue;
                    }
                    let Ok(class) = pointer(u64_at(&bytes, 0x4c)?) else {
                        continue;
                    };
                    let supported = if let Some(value) = self.ground_scan.class_names.get(&class) {
                        *value
                    } else {
                        let name = self.class_name(class);
                        if self.optional_interrupted() {
                            self.ground_scan.cursor = next_scan_cursor(index, true);
                            return Err("Optional scan budget reached".into());
                        }
                        let Ok(name) = name else {
                            continue;
                        };
                        let value = name == "DCServerPhysicsObject";
                        if self.ground_scan.class_names.len() >= 4096 {
                            return Err("Class cache bound reached".into());
                        }
                        self.ground_scan.class_names.insert(class, value);
                        value
                    };
                    if !supported {
                        continue;
                    }
                    if self.ground_scan.candidates.len() >= 256 {
                        return Err("Ground spawn candidate bound reached".into());
                    }
                    if !self.ground_scan.candidates.contains(&(address, index)) {
                        // Admission requires the full reflected live actor and recognized definition.
                        if self
                            .collectible_actor(owner, player, address, false)
                            .is_ok()
                        {
                            self.ground_scan.candidates.push((address, index));
                        }
                        if self
                            .collectible_deadline
                            .get()
                            .is_some_and(|d| Instant::now() >= d)
                        {
                            self.ground_scan.cursor = next_scan_cursor(index, true);
                            return Err("Optional scan budget reached".into());
                        }
                    }
                }
            }
            self.ground_scan.cursor = end;
            if end == count {
                self.ground_scan.completed = true;
                self.ground_scan.cursor = 0;
                self.ground_scan.class_names.clear();
            }
            Ok(())
        })();
        self.collectible_deadline.set(None);
        if self.read(self.rva(OBJECTS_RVA)?, 16)? != table {
            // Addresses are only discovery hints and are fully revalidated next time.
            // Unrelated object allocation must not restart a complete world scan.
            self.ground_scan.class_names.clear();
            self.ground_scan.completed = false;
            return Err("Object table changed during scan".into());
        }
        let status = if self.ground_scan.completed {
            format!("{} exposed ground spawns; refreshing", result.len())
        } else {
            format!(
                "{} exposed ground spawns; scanning {}/{}",
                result.len(),
                self.ground_scan.cursor,
                count
            )
        };
        let status = match discovery {
            Ok(()) => status,
            Err(error) => format!("{} exposed; scan paused: {error}", result.len()),
        };
        let status = if refresh_limited {
            format!("{status}; refresh partial")
        } else {
            status
        };
        Ok((result, status))
    }
    pub fn sample(&mut self) -> Result<Sample, String> {
        self.scope_start.set(Instant::now());
        self.scope_reads.set(0);
        self.scope_bytes.set(0);
        *self.maps.borrow_mut() =
            process::mappings(self.candidate.identity.pid).map_err(|e| e.to_string())?;
        self.unchanged()?;
        let name_header = self.read(self.rva(NAMES_RVA)?, 16)?;
        if name_header != self.name_header {
            let names = pointer(u64_at(&name_header, 0)?)?;
            let count = u32_at(&name_header, 8)?;
            let capacity = u32_at(&name_header, 12)?;
            if !(3..=250_000).contains(&count) || capacity < count || capacity > 250_000 {
                return Err("Changed name table outside bounds".into());
            }
            self.names = names;
            self.name_count = count;
            self.names_cache.borrow_mut().clear();
            self.metadata_cache.borrow_mut().clear();
            self.area_schemas.borrow_mut().clear();
            self.collectible_schemas.borrow_mut().clear();
            self.classes = None;
            self.ground_scan.class_names.clear();
            for (index, expected) in [(0, "None"), (1, "ByteProperty"), (2, "IntProperty")] {
                if self.name(index)? != expected {
                    return Err("Changed name table anchors mismatch".into());
                }
            }
            self.name_header = name_header.clone();
        }
        let mut before = self.ownership()?;
        if self.classes != Some(before.classes) {
            self.validate_schema(before.classes)?;
            self.classes = Some(before.classes);
        }
        let pawn = before.objects[3].0;
        let area = self.area_context(pawn);
        let area_status = match &area {
            Ok(context) if context.key.is_some() => "Experimental streaming area detected".into(),
            Ok(_) => "No unique supported streaming area".into(),
            Err(error) => error.clone(),
        };
        before.area = area.ok();
        let observed_at = Instant::now();
        let bytes = self.read(add(pawn, 0xd0)?, 12)?;
        let position = [float(&bytes, 0)?, float(&bytes, 4)?, float(&bytes, 8)?];
        let bytes = self.read(add(pawn, 0x5f4)?, 16)?;
        let values = [
            float(&bytes, 0)?,
            float(&bytes, 4)?,
            float(&bytes, 8)?,
            float(&bytes, 12)?,
        ];
        validate_values(position, values)?;
        let collectible = None;
        let (ground_spawns, ground_spawns_status) = if self.ground_spawns_enabled {
            self.collectible_deadline
                .set(Some(Instant::now() + std::time::Duration::from_millis(100)));
            let result = self.ground_spawns(&before, position);
            self.collectible_deadline.set(None);
            result
                .unwrap_or_else(|error| (Vec::new(), format!("Ground spawns unavailable: {error}")))
        } else {
            (Vec::new(), "Ground spawns off".into())
        };
        let mut after = self.ownership()?;
        after.area = self.area_context(after.objects[3].0).ok();
        if self.read(self.rva(NAMES_RVA)?, 16)? != name_header {
            self.ground_scan.class_names.clear();
            return Err("Name table changed during sample; discarded".into());
        }
        self.unchanged()?;
        if before != after {
            self.ground_scan = GroundScan::default();
            return Err("Controlled object changed during sample; discarded".into());
        }
        Ok(Sample {
            position,
            collectible,
            ground_spawns,
            ground_spawns_status,
            max_health: values[0],
            health: values[1],
            max_power: values[2],
            power: values[3],
            outer_package: before.package.clone(),
            area_key: before.area.as_ref().and_then(|area| area.key.clone()),
            area_status,
            observed_at,
            owner: before,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refresh_rotation_services_every_candidate_under_a_small_budget() {
        let mut cursor = 0;
        let mut seen = [false; 256];
        for _ in 0..128 {
            let serviced: Vec<_> = rotated_indices(cursor, 256).take(2).collect();
            for &index in &serviced {
                seen[index] = true;
            }
            cursor = (serviced[1] + 1) % 256;
        }
        assert!(seen.into_iter().all(|value| value));
        // A deadline before the fourth entry preserves it as the next start.
        let first: Vec<_> = rotated_indices(0, 4).take(3).collect();
        let next = (first.last().unwrap() + 1) % 4;
        assert_eq!(rotated_indices(next, 4).next(), Some(3));

        assert_eq!(rotated_indices(400, 0).count(), 0);
        assert_eq!(rotated_indices(5, 3).collect::<Vec<_>>(), vec![2, 0, 1]);
    }
    #[test]
    fn malformed_discovery_entries_advance_but_interrupted_entries_retry() {
        let mut cursor = 50;
        for _ in 0..4 {
            cursor = next_scan_cursor(cursor as u32, false);
        }
        assert_eq!(cursor, 54);
        assert_eq!(next_scan_cursor(cursor as u32, true), 54);
        assert_eq!(next_scan_cursor(249_999, false), 250_000);
    }
    #[test]
    fn ground_spawn_allowlist_rejects_props_and_supports_all_proven_families() {
        for (name, kind) in [
            (
                "DSGPROGEN107_MAGNIFYINGGLASS_BLUE_DO",
                GroundSpawnKind::Blue,
            ),
            (
                "DSGPROGEN107_MAGNIFYINGGLASS_GOLD_DO",
                GroundSpawnKind::Gold,
            ),
            (
                "DSGPROGEN107_MAGNIFYINGGLASS_GREEN_DO",
                GroundSpawnKind::Green,
            ),
            (
                "DSGPROGEN107_MAGNIFYINGGLASS_GREY_DO",
                GroundSpawnKind::Grey,
            ),
            (
                "DSGPROGEN107_MAGNIFYINGGLASS_PURPLE_DO",
                GroundSpawnKind::Purple,
            ),
            (
                "DSGPROGEN107_QUESTIONMARK_PURPLE_DO",
                GroundSpawnKind::Purple,
            ),
            ("MAGNIFYINGGLASS_AFOOLS_DO", GroundSpawnKind::Other),
        ] {
            assert_eq!(ground_spawn_kind(name), Some(kind));
        }
        for name in [
            "FXSM103_DAMAGEABLEOBJECT",
            "MAGNIFYINGGLASS_BLUE",
            "DSGPROGEN107_MAGNIFYINGGLASS_RED_DO",
            "",
        ] {
            assert_eq!(ground_spawn_kind(name), None);
        }
        assert!(Sample::synthetic(1).ground_spawns.is_empty());
    }
    #[test]
    fn collectible_requires_same_world_visible_lifetime_and_bounded_distance() {
        let mut bytes = vec![0; 0x3c0];
        bytes[0x164..0x16c].copy_from_slice(&0x20000u64.to_le_bytes());
        let flags = [(0x100, 8), (0x104, 128)];
        assert_eq!(
            collectible_position(&bytes, 0x20000, [0.0; 3], &flags).unwrap(),
            [0.0; 3]
        );
        assert!(collectible_position(&bytes, 0x30000, [0.0; 3], &flags).is_err());
        assert!(collectible_position(&bytes, 0, [0.0; 3], &flags).is_err());
        for (offset, mask) in flags {
            bytes[offset..offset + 4].copy_from_slice(&mask.to_le_bytes());
            assert!(collectible_position(&bytes, 0x20000, [0.0; 3], &flags).is_err());
            bytes[offset..offset + 4].fill(0);
        }
        for (x, y, accepted) in [
            (2000.0f32, 0.0f32, true),
            (2000.0, 1.0, false),
            (2001.0, 0.0, false),
            (f32::NAN, 0.0, false),
            (10_000_001.0, 0.0, false),
        ] {
            bytes[0xd0..0xd4].copy_from_slice(&x.to_le_bytes());
            bytes[0xd4..0xd8].copy_from_slice(&y.to_le_bytes());
            assert_eq!(
                collectible_position(&bytes, 0x20000, [0.0; 3], &flags).is_ok(),
                accepted
            );
        }
        assert!(collectible_position(&bytes[..100], 0x20000, [0.0; 3], &flags).is_err());
        assert!(collectible_position(&bytes, 0x20000, [f32::NAN; 3], &flags).is_err());
    }
    #[test]
    fn collectible_snapshot_ignores_unrelated_updates_but_tracks_context_and_identity() {
        let bytes = vec![0; 0x3c0];
        let flags = [(0x100, 8)];
        let original = collectible_actor_snapshot(&bytes, &flags).unwrap();
        for offset in [8, 0x3c, 0x44, 0x4c, 0xd0, 0x164, 0x3b8] {
            let mut changed = bytes.clone();
            changed[offset] = 1;
            assert_ne!(
                original,
                collectible_actor_snapshot(&changed, &flags).unwrap()
            );
        }
        for (offset, value) in [(0x180, 1), (0x100, 1)] {
            let mut changed = bytes.clone();
            changed[offset] = value;
            assert_eq!(
                original,
                collectible_actor_snapshot(&changed, &flags).unwrap()
            );
        }
        let mut hidden = bytes.clone();
        hidden[0x100] = 8;
        assert_ne!(
            original,
            collectible_actor_snapshot(&hidden, &flags).unwrap()
        );
        let original = collectible_definition_snapshot(&bytes).unwrap();
        let mut changed = bytes.clone();
        changed[0x20] = 1;
        assert_eq!(original, collectible_definition_snapshot(&changed).unwrap());
        changed[0x44] = 1;
        assert_ne!(original, collectible_definition_snapshot(&changed).unwrap());
    }
    #[test]
    fn collectible_flags_require_bounded_reflected_single_bit_masks() {
        for (offset, mask) in [(0x54, 1), (0x3bc, 0x8000_0000)] {
            assert!(validate_collectible_flag(offset, mask).is_ok());
        }
        for (offset, mask) in [(0x50, 1), (0x55, 1), (0x3c0, 1), (0x100, 0), (0x100, 3)] {
            assert!(validate_collectible_flag(offset, mask).is_err());
        }
        assert!(Sample::synthetic(1).collectible.is_none());
    }
    #[test]
    fn numbered_unrelated_streams_do_not_hide_valid_area_signals() {
        assert!(!area_signal("MGOT101_BUILDING", 27).unwrap());
        assert!(area_signal("MGOT101_AUDIO", 0).unwrap());
        assert!(area_signal("MGOT101_AUDIO", 1).is_err());
        assert!(area_signal("MGOT101_BL_DESIGNERDATA", 1).is_err());
    }
    #[test]
    fn streaming_area_requires_unique_bounded_audio_key_and_matching_designer() {
        let choose = |names: &[&str]| {
            supported_area(
                &names
                    .iter()
                    .map(|name| (*name).into())
                    .collect::<Vec<String>>(),
            )
        };
        assert_eq!(
            choose(&["MGOT101_AUDIO", "MGOT101_BL_DESIGNERDATA"]),
            Some("MGOT101".into())
        );
        assert_eq!(
            choose(&["MPYR0224_AUDIO", "MPYR0224_DESIGNERDATA"]),
            Some("MPYR0224".into())
        );
        for area in ["MHDT0107", "MPYR0242", "NEWINSTANCE999", "OTHER", "A", "1"] {
            assert_eq!(
                choose(&[&format!("{area}_AUDIO"), &format!("{area}_DESIGNERDATA")]),
                Some(area.into())
            );
            assert_eq!(
                choose(&[&format!("{area}_AUDIO"), "MGOT101_DESIGNERDATA"]),
                None
            );
            assert_eq!(
                choose(&[
                    &format!("{area}_AUDIO"),
                    "MPYR0224_AUDIO",
                    &format!("{area}_DESIGNERDATA")
                ]),
                None
            );
        }
        for names in [
            vec![],
            vec!["MGOT101_AUDIO"],
            vec!["MGOT101_DESIGNERDATA"],
            vec!["MGOT101_AUDIO", "MPYR0224_DESIGNERDATA"],
            vec!["MGOT101_AUDIO", "MPYR0224_AUDIO", "MGOT101_DESIGNERDATA"],
            vec!["MGOT101_AUDIO", "MGOT101_AUDIO", "MGOT101_DESIGNERDATA"],
            vec!["MGOT101_AUDIO_EXTRA", "MGOT101_DESIGNERDATA"],
            vec!["MGOT101_AUDIO", "MGOT101X_DESIGNERDATA"],
            vec![
                "MGOT101_AUDIO",
                "MGOT101_DESIGNERDATA",
                "OTHER_DESIGNERDATA",
            ],
        ] {
            assert_eq!(choose(&names), None);
        }
    }
    #[test]
    fn malformed_or_ambiguous_unlisted_area_signals_are_rejected() {
        for key in [
            "",
            "lowercase",
            "MIXed1",
            "BAD-KEY",
            "BAD/KEY",
            "BAD.KEY",
            "_BADKEY",
            "BADKEY_",
            "BAD__KEY",
            "BAD KEY",
            "BAD\nKEY",
            "ÅREA",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        ] {
            let packages = vec![format!("{key}_AUDIO"), format!("{key}_DESIGNERDATA")];
            assert!(supported_area(&packages).is_none(), "malformed key {key:?}");
        }
        for designer in [
            "NEW99X_DESIGNERDATA",
            "NEW99__DESIGNERDATA",
            "NEW99_BL__DESIGNERDATA",
            "NEW99_lower_DESIGNERDATA",
            "NEW99_../BAD_DESIGNERDATA",
            "NEW99_ÅREA_DESIGNERDATA",
        ] {
            assert!(supported_area(&["NEW99_AUDIO".into(), designer.into()]).is_none());
        }
        assert_eq!(
            supported_area(&["NEW99_AUDIO".into(), "NEW99_BL_CT_DESIGNERDATA".into()]),
            Some("NEW99".into())
        );
        assert!(
            supported_area(&[
                "NEW99_AUDIO".into(),
                "OTHER99_AUDIO".into(),
                "NEW99_DESIGNERDATA".into()
            ])
            .is_none()
        );
        assert!(
            supported_area(&[
                "NEW99_AUDIO".into(),
                "NEW99_DESIGNERDATA".into(),
                "OTHER99_DESIGNERDATA".into()
            ])
            .is_none()
        );
        assert!(area_signal("NEW99_AUDIO", 1).is_err());
        assert!(area_signal("NEW99_BL_DESIGNERDATA", 1).is_err());
        let maximum = "A".repeat(32);
        assert_eq!(
            supported_area(&[
                format!("{maximum}_AUDIO"),
                format!("{maximum}_DESIGNERDATA")
            ]),
            Some(maximum)
        );
        assert!(supported_area(&vec!["NEW99_AUDIO".into(); 2049]).is_none());
    }
    #[test]
    fn underscored_area_preserves_full_key_and_exact_designer_boundary() {
        let key = "MLOC139_ART_001";
        assert!(valid_area_key(key));
        for designer in [
            "MLOC139_ART_001_DESIGNERDATA",
            "MLOC139_ART_001_BL_DESIGNERDATA",
        ] {
            assert_eq!(
                supported_area(&[format!("{key}_AUDIO"), designer.into()]),
                Some(key.into())
            );
        }
        for designer in [
            "MLOC139_DESIGNERDATA",
            "MLOC139_ART_DESIGNERDATA",
            "MLOC139_ART_002_DESIGNERDATA",
            "MLOC139_ART_001X_DESIGNERDATA",
            "MLOC139_ART_001__DESIGNERDATA",
            "MLOC139_ART_001_../BAD_DESIGNERDATA",
        ] {
            assert!(supported_area(&[format!("{key}_AUDIO"), designer.into()]).is_none());
        }
        for invalid in [
            "_MLOC139",
            "MLOC139_",
            "MLOC139__ART",
            "MLOC139/ART",
            "MLOC139_.._ART",
            "MLOC139_art",
        ] {
            assert!(!valid_area_key(invalid));
        }
    }
    #[test]
    fn bounded_decoding_rejects_short_and_nonfinite() {
        assert!(float(&[0; 3], 0).is_err());
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(float(&value.to_le_bytes(), 0).is_err());
        }
        assert_eq!(float(&42.5f32.to_le_bytes(), 0).unwrap(), 42.5);
    }
    #[test]
    fn resource_and_position_safety_bounds() {
        assert!(validate_values([1.0, -2.0, 3.0], [100.0, 0.0, 80.0, 80.0]).is_ok());
        for values in [
            [0.0, 0.0, 1.0, 0.0],
            [10.0, 11.0, 1.0, 0.0],
            [10.0, -1.0, 1.0, 0.0],
            [10.0, 1.0, f32::NAN, 0.0],
        ] {
            assert!(validate_values([0.0; 3], values).is_err());
        }
        assert!(validate_values([10_000_001.0, 0.0, 0.0], [1.0; 4]).is_err());
    }
    #[test]
    fn cancellation_stops_before_remote_access() {
        let mapping = process::parse_maps("10000-11000 r--p 0 08:01 42 /synthetic/DCGAME.EXE")
            .unwrap()
            .remove(0);
        let mut reader = Reader {
            ground_spawns_enabled: false,
            ground_scan: GroundScan::default(),
            candidate: process::Candidate {
                identity: process::Identity {
                    pid: -1,
                    start_ticks: 0,
                },
                mapping,
            },
            names: 0,
            name_count: 0,
            name_header: Vec::new(),
            classes: None,
            cancel: None,
            maps: RefCell::new(Vec::new()),
            names_cache: RefCell::new(HashMap::new()),
            metadata_cache: RefCell::new(HashMap::new()),
            scope_start: Cell::new(Instant::now()),
            scope_reads: Cell::new(0),
            scope_bytes: Cell::new(0),
            collectible_deadline: Cell::new(None),
            area_schemas: RefCell::new(HashSet::new()),
            collectible_schemas: RefCell::new(HashMap::new()),
        };
        assert!(!reader.ground_spawns_enabled);
        reader.set_ground_spawns_enabled(true);
        reader.ground_scan.cursor = 42;
        reader.ground_scan.candidates.push((0x10000, 1));
        reader.set_ground_spawns_enabled(true);
        assert_eq!(reader.ground_scan.cursor, 42);
        reader.set_ground_spawns_enabled(false);
        assert_eq!(reader.ground_scan.cursor, 0);
        assert!(reader.ground_scan.candidates.is_empty());
        assert!(!reader.ground_scan.completed);
        reader.set_cancel(Arc::new(AtomicBool::new(true)));
        assert_eq!(
            reader.read(0x10000, 4).unwrap_err(),
            "Experimental read cancelled"
        );
        reader.set_cancel(Arc::new(AtomicBool::new(false)));
        reader
            .scope_start
            .set(Instant::now() - std::time::Duration::from_secs(4));
        assert!(reader.read(0x10000, 4).unwrap_err().contains("budget"));
        reader.scope_start.set(Instant::now());
        reader.scope_bytes.set(2 * 1024 * 1024);
        assert!(reader.read(0x10000, 4).unwrap_err().contains("budget"));
        assert!(
            reader
                .read_scatter(&[(0x10000, 4)])
                .unwrap_err()
                .contains("budget")
        );
        reader.scope_bytes.set(0);
        reader.set_cancel(Arc::new(AtomicBool::new(true)));
        assert_eq!(
            reader.read_scatter(&[(0x10000, 4)]).unwrap_err(),
            "Experimental read cancelled"
        );
    }
    #[test]
    fn pointers_and_arithmetic_fail_closed() {
        for value in [0, 1, u64::MAX, 0x8000_0000_0000] {
            assert!(pointer(value).is_err());
        }
        assert!(pointer(0x10000).is_ok());
        assert!(add(usize::MAX, 1).is_err());
    }
    #[test]
    fn ownership_includes_object_generation_and_package() {
        let original = Ownership {
            objects: [(0x10000, 1); 4],
            classes: [0x20000; 4],
            outers: vec![(0x30000, 4)],
            package: "SyntheticPackage".into(),
            area: None,
            world_pointer: 0,
        };
        let mut replaced = original.clone();
        replaced.objects[3].1 += 1;
        assert_ne!(original, replaced);
        replaced = original.clone();
        replaced.package = "OtherSyntheticPackage".into();
        assert_ne!(original, replaced);
        replaced = original.clone();
        replaced.world_pointer = 0x40000;
        assert_ne!(original, replaced);
        replaced = original.clone();
        replaced.area = Some(AreaContext {
            world: (0x40000, 1),
            array: (0x50000, 1, 1),
            members: vec![0x60000],
            signals: vec![],
            key: Some("MGOT101".into()),
        });
        assert_ne!(original, replaced);
    }
}
