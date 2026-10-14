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
const ENGINE_RVA: usize = 0x2130f00;

#[derive(Clone, Debug)]
pub struct Sample {
    pub position: [f32; 3],
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
fn supported_area(packages: &[String]) -> Option<String> {
    let audio: Vec<_> = packages
        .iter()
        .filter_map(|name| name.strip_suffix("_AUDIO"))
        .collect();
    if audio.len() != 1 || !matches!(audio[0], "MGOT101" | "MPYR0224") {
        return None;
    }
    let key = audio[0];
    let prefix = format!("{key}_");
    let designers: Vec<_> = packages
        .iter()
        .filter(|name| name.ends_with("_DESIGNERDATA"))
        .collect();
    if designers.is_empty() || designers.iter().any(|name| !name.starts_with(&prefix)) {
        return None;
    }
    Some(key.into())
}

pub struct Reader {
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
    area_schemas: RefCell<HashSet<(usize, u32)>>,
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
            area_schemas: RefCell::new(HashSet::new()),
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
                        || u32_at(&bytes, 0x90)? != offset
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
            self.classes = None;
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
        let mut after = self.ownership()?;
        after.area = self.area_context(after.objects[3].0).ok();
        if self.read(self.rva(NAMES_RVA)?, 16)? != name_header {
            return Err("Name table changed during sample; discarded".into());
        }
        self.unchanged()?;
        if before != after {
            return Err("Controlled object changed during sample; discarded".into());
        }
        Ok(Sample {
            position,
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
    fn numbered_unrelated_streams_do_not_hide_valid_area_signals() {
        assert!(!area_signal("MGOT101_BUILDING", 27).unwrap());
        assert!(area_signal("MGOT101_AUDIO", 0).unwrap());
        assert!(area_signal("MGOT101_AUDIO", 1).is_err());
        assert!(area_signal("MGOT101_BL_DESIGNERDATA", 1).is_err());
    }
    #[test]
    fn streaming_area_requires_exact_known_audio_and_matching_designer() {
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
        for names in [
            vec![],
            vec!["MGOT101_AUDIO"],
            vec!["MGOT101_DESIGNERDATA"],
            vec!["MGOT101_AUDIO", "MPYR0224_DESIGNERDATA"],
            vec!["MGOT101_AUDIO", "MPYR0224_AUDIO", "MGOT101_DESIGNERDATA"],
            vec!["MGOT101_AUDIO", "MGOT101_AUDIO", "MGOT101_DESIGNERDATA"],
            vec!["OTHER_AUDIO", "OTHER_DESIGNERDATA"],
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
            area_schemas: RefCell::new(HashSet::new()),
        };
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
