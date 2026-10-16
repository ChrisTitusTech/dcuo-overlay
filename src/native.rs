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
    Exobyte,
}
fn exo_definition(name: &str, package: &str) -> bool {
    name == "FXSM103_DAMAGEABLEOBJECT" && package == "DFXSM103"
}
fn exo_effect(
    class: &str,
    owner: u64,
    actor: usize,
    template: &str,
    package: &str,
    template_number: u32,
    package_number: u32,
) -> bool {
    class == "ParticleSystemComponent"
        && owner == actor as u64
        && template == "FXGEN109_FX_BAUBLE_CRAFTING"
        && package == "DFXGEN109"
        && template_number == 0
        && package_number == 0
}
fn component_array(header: &[u8]) -> Result<(usize, usize), String> {
    let count = u32_at(header, 8)? as usize;
    let capacity = u32_at(header, 12)? as usize;
    if count > capacity || capacity > 64 {
        return Err("Exo component array bound exceeded".into());
    }
    let data = u64_at(header, 0)?;
    if data == 0 && count == 0 && capacity == 0 {
        return Ok((0, 0));
    }
    Ok((pointer(data)?, count))
}
// Capture only object identity and linkage, never mutable particle state.
#[derive(Default)]
struct ExoProof(Vec<(usize, Vec<u8>)>);
fn validate_exo_proof(
    proof: &ExoProof,
    mut read: impl FnMut(usize, usize) -> Result<Vec<u8>, String>,
) -> Result<(), String> {
    for (address, before) in &proof.0 {
        if read(*address, before.len())? != *before {
            return Err("Exo effect linkage changed during sample".into());
        }
    }
    Ok(())
}

fn ground_spawn_kind(name: &str, package: &str) -> Option<GroundSpawnKind> {
    // Shared effect/prop definitions do not establish a harvestable exo node.
    // Accept only the explicit investigation, briefing and collection definitions.
    if package != "DDSGPROGEN107" {
        return None;
    }
    match name {
        "DSGPROGEN107_MAGNIFYINGGLASS_BLUE_DO" => Some(GroundSpawnKind::Blue),
        "DSGPROGEN107_MAGNIFYINGGLASS_GOLD_DO" => Some(GroundSpawnKind::Gold),
        "DSGPROGEN107_MAGNIFYINGGLASS_GREEN_DO" => Some(GroundSpawnKind::Green),
        _ => None,
    }
}
#[derive(Clone, Debug)]
pub struct Collectible {
    pub kind: GroundSpawnKind,
    pub position: [f32; 3],
}

#[derive(Clone, Debug)]
pub struct GroundSpawnSweep {
    pub completed_at: Instant,
    pub spawns: Vec<Collectible>,
}
#[derive(Clone, Debug)]
pub struct Sample {
    pub position: [f32; 3],
    pub feats: Option<Arc<crate::feat_memory::Snapshot>>,
    pub feats_status: String,
    /// Legacy nearby recognized spawn, with no collection-status claim.
    pub collectible: Option<Collectible>,
    pub ground_spawns: Vec<Collectible>,
    pub ground_spawns_status: String,
    pub ground_spawn_sweep: Option<Arc<GroundSpawnSweep>>,
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
            feats: None,
            feats_status: "No synthetic feats".into(),
            collectible: None,
            ground_spawns: Vec::new(),
            ground_spawns_status: "Ground spawns off".into(),
            ground_spawn_sweep: None,
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
    /// Stable identity for historical spawn locations; streaming membership is not identity.
    pub fn same_spawn_context(&self, other: &Self) -> bool {
        self.owner.objects == other.owner.objects
            && self.owner.classes == other.owner.classes
            && self.owner.world_pointer == other.owner.world_pointer
            && self.area_key == other.area_key
            && match (&self.owner.area, &other.owner.area) {
                (Some(current), Some(previous)) => {
                    current.world == previous.world
                        && match (&current.world_chain, &previous.world_chain) {
                            (Some(current), Some(previous)) => current == previous,
                            _ => true,
                        }
                }
                _ => true,
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
    area: Option<Box<AreaContext>>,
    world_pointer: u64,
}

impl Ownership {
    fn same_spawn_identity(&self, other: &Self) -> bool {
        self.objects == other.objects
            && self.classes == other.classes
            && self.world_pointer == other.world_pointer
            && self.area.as_ref().and_then(|a| a.key.as_ref())
                == other.area.as_ref().and_then(|a| a.key.as_ref())
            && self.area.as_ref().map(|a| (&a.world, &a.world_chain))
                == other.area.as_ref().map(|a| (&a.world, &a.world_chain))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AreaContext {
    world: (usize, u32),
    array: (usize, u32, u32),
    members: Vec<usize>,
    signals: Vec<(usize, u32, usize, String)>,
    key: Option<String>,
    world_chain: Option<Vec<AreaAncestor>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AreaAncestor {
    address: usize,
    index: u32,
    class: (usize, u32),
    name: String,
    number: u32,
    outer: u64,
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

// FName instances are part of the observed alias contract, not a suffix guess.
fn supported_world_ancestor(depth: usize, name: &str, class: &str, number: u32) -> bool {
    let expected = [
        (Some("WorldInfo"), "WorldInfo", 1),
        (Some("PersistentLevel"), "Level", 0),
        (Some("TheWorld"), "World", 0),
        (Some("MGOT103_ALT_US"), "Package", 0),
    ];
    expected
        .get(depth)
        .is_some_and(|(expected_name, expected_class, expected_number)| {
            class == *expected_class
                && number == *expected_number
                && expected_name.is_none_or(|expected| name == expected)
        })
}

// Only this observed alias has paired world-package and visible designer evidence.
// Any audio signal keeps the existing audio/designer ambiguity rules authoritative.
fn supported_world_alias(packages: &[String], package: &str) -> Option<String> {
    if packages.len() > 2048 || package != "MGOT103_ALT_US" {
        return None;
    }
    if packages.iter().any(|name| name.ends_with("_AUDIO")) {
        return None;
    }
    let designers: Vec<_> = packages
        .iter()
        .filter(|name| name.ends_with("_DESIGNERDATA"))
        .collect();
    (designers.len() == 1 && designers[0] == "MGOT103_DESIGNERDATA").then(|| "MGOT103".into())
}

type CollectibleSchemas = HashMap<[(usize, u32); 3], Vec<(usize, u32)>>;

#[derive(Clone, Default)]
struct GroundScan {
    owner: Option<Ownership>,
    table: usize,
    count: usize,
    cursor: usize,
    completed: bool,
    candidates: Vec<(usize, u32)>,
    refresh_cursor: usize,
    class_names: HashMap<usize, bool>,
    sweep: SweepAccumulator,
    pending_sweep: Option<GroundSpawnSweep>,
    published_sweep: Option<Arc<GroundSpawnSweep>>,
}
impl GroundScan {
    fn accept_pending_sweep(&mut self) {
        if let Some(sweep) = self.pending_sweep.take() {
            self.published_sweep = Some(Arc::new(sweep));
        }
    }
    fn bind_owner(&mut self, owner: &Ownership, table: usize) {
        if !self
            .owner
            .as_ref()
            .is_some_and(|o| o.same_spawn_identity(owner))
            || self.table != table
        {
            *self = Self {
                table,
                ..Default::default()
            };
        }
        self.owner = Some(owner.clone());
    }
    fn restore_for_owner(&mut self, saved: Self, owner: &Ownership) {
        *self = if saved
            .owner
            .as_ref()
            .is_some_and(|o| o.same_spawn_identity(owner))
        {
            saved
        } else {
            Self::default()
        };
        if self.owner.is_some() {
            self.owner = Some(owner.clone());
        }
    }
}
#[derive(Clone, Default)]
struct SweepAccumulator {
    seen: HashMap<(usize, u32), Collectible>,
    uncertain: bool,
}
impl SweepAccumulator {
    fn check_bound(&mut self, count: usize, limit: usize, error: &str) -> Result<(), String> {
        if count >= limit {
            self.uncertain = true;
            Err(error.into())
        } else {
            Ok(())
        }
    }
    fn observe(&mut self, identity: (usize, u32), spawn: Collectible) {
        if self.seen.len() >= 256 && !self.seen.contains_key(&identity) {
            self.uncertain = true;
        } else {
            self.seen.insert(identity, spawn);
        }
    }
    fn finish(&mut self, completed_at: Instant) -> Option<GroundSpawnSweep> {
        let finished = std::mem::take(self);
        (!finished.uncertain).then(|| GroundSpawnSweep {
            completed_at,
            spawns: finished.seen.into_values().collect(),
        })
    }
}
fn known_spawn_absence(error: &str) -> bool {
    matches!(
        error,
        "Unsupported ground spawn definition"
            | "Unsupported nearby collectible definition"
            | "Unsupported pickup definition package"
            | "No owned exo crafting effect"
            | "Nearby collectible hidden or deleted"
            | "Nearby collectible belongs to a different world"
    )
}
fn rotated_indices(start: usize, count: usize) -> impl Iterator<Item = usize> {
    (0..count).map(move |offset| (start % count.max(1) + offset) % count.max(1))
}
fn next_scan_cursor(index: u32, interrupted: bool) -> usize {
    index as usize + usize::from(!interrupted)
}
pub struct Reader {
    feats_enabled: bool,
    feats: Option<Arc<crate::feat_memory::Snapshot>>,
    feats_owner: Option<[(usize, u32); 4]>,
    feats_attempt: Option<Instant>,
    feats_status: String,
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
            feats_enabled: false,
            feats: None,
            feats_owner: None,
            feats_attempt: None,
            feats_status: "Waiting for live feats".into(),
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
            (classes[3], "m_DCClientEntity", "StructProperty", 0x5c, 8),
            (classes[3], "m_esEntityID", "StructProperty", 0x68, 8),
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
    fn world_package_chain(&self, world: usize) -> Result<Vec<AreaAncestor>, String> {
        let mut address = world;
        let mut chain = Vec::with_capacity(4);
        let mut seen = HashSet::new();
        for depth in 0..4 {
            if !seen.insert(address) {
                return Err("Cyclic world package ownership".into());
            }
            let bytes = self.read(address, 0x54)?;
            let class = pointer(u64_at(&bytes, 0x4c)?)?;
            let name = self.name(u32_at(&bytes, 0x44)?)?;
            let number = u32_at(&bytes, 0x48)?;
            let class_object = self.object(class)?;
            if !supported_world_ancestor(depth, &name, &class_object.2, number)
                || self.class_name(class_object.1)? != "Class"
            {
                return Err("Unsupported world package ownership".into());
            }
            let outer = u64_at(&bytes, 0x3c)?;
            chain.push(AreaAncestor {
                address,
                index: u32_at(&bytes, 8)?,
                class: (class, class_object.0),
                name,
                number,
                outer,
            });
            if depth == 3 {
                if outer != 0 {
                    return Err("World package has unexpected outer".into());
                }
            } else {
                address = pointer(outer)?;
            }
        }
        Ok(chain)
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
        let mut key = supported_area(&packages);
        let mut world_chain = None;
        if key.is_none() && supported_world_alias(&packages, "MGOT103_ALT_US").is_some() {
            let chain = self.world_package_chain(world)?;
            key = supported_world_alias(&packages, &chain[3].name);
            if key.is_some() {
                if self.world_package_chain(world)? != chain {
                    return Err("World package ownership changed during context read".into());
                }
                world_chain = Some(chain);
            }
        }
        Ok(AreaContext {
            world: (world, world_index),
            array: (data, count, capacity),
            members,
            signals,
            key,
            world_chain,
        })
    }
    fn exo_capture(
        &self,
        proof: &mut ExoProof,
        address: usize,
        size: usize,
    ) -> Result<Vec<u8>, String> {
        let bytes = self.read(address, size)?;
        proof.0.push((address, bytes.clone()));
        Ok(bytes)
    }
    fn exo_identity(&self, proof: &mut ExoProof, address: usize) -> Result<Vec<u8>, String> {
        let mut bytes = vec![0; 0x54];
        for (offset, size) in [(8, 4), (0x3c, 24)] {
            bytes[offset..offset + size].copy_from_slice(&self.exo_capture(
                proof,
                add(address, offset)?,
                size,
            )?);
        }
        // Bind class generation as well as the object's class pointer.
        let class = pointer(u64_at(&bytes, 0x4c)?)?;
        self.exo_capture(proof, add(class, 8)?, 4)?;
        self.exo_capture(proof, add(class, 0x3c)?, 24)?;
        Ok(bytes)
    }
    fn recheck_exo(&self, proof: &ExoProof) -> Result<(), String> {
        validate_exo_proof(proof, |address, size| self.read(address, size))
    }

    fn owned_exo_effect(&self, actor: usize, actor_class: usize) -> Result<ExoProof, String> {
        let array_property = self.field(actor_class, "AllComponents", "ArrayProperty", 0xc0, 16)?;
        let mut proof = ExoProof::default();
        self.exo_identity(&mut proof, array_property)?;
        let inner = pointer(u64_at(
            &self.exo_capture(&mut proof, add(array_property, 0xcc)?, 8)?,
            0,
        )?)?;
        let inner_identity = self.exo_identity(&mut proof, inner)?;
        let inner_layout = self.exo_capture(&mut proof, add(inner, 0x6c)?, 8)?;
        let component_class = pointer(u64_at(
            &self.exo_capture(&mut proof, add(inner, 0xcc)?, 8)?,
            0,
        )?)?;
        let component_identity = self.exo_identity(&mut proof, component_class)?;
        if self.class_name(pointer(u64_at(&inner_identity, 0x4c)?)?)? != "ComponentProperty"
            || self.name(u32_at(&inner_identity, 0x44)?)? != "AllComponents"
            || u64_at(&inner_identity, 0x3c)? != array_property as u64
            || u32_at(&inner_layout, 0)? != 1
            || u32_at(&inner_layout, 4)? != 8
            || self.name(u32_at(&component_identity, 0x44)?)? != "ActorComponent"
            || self.class_name(pointer(u64_at(&component_identity, 0x4c)?)?)? != "Class"
        {
            return Err("Reflection schema mismatch for AllComponents inner".into());
        }
        self.exo_identity(&mut proof, actor)?;
        let header = self.exo_capture(&mut proof, add(actor, 0xc0)?, 16)?;
        let (data, count) = component_array(&header)?;
        let members = if count == 0 {
            Vec::new()
        } else {
            self.exo_capture(&mut proof, data, count * 8)?
        };
        let mut found = false;
        let mut seen = HashSet::new();
        for slot in members.chunks_exact(8) {
            let value = u64_at(slot, 0)?;
            if value == 0 {
                continue;
            }
            let component = pointer(value)?;
            if !seen.insert(component) {
                return Err("Duplicate exo component membership".into());
            }
            let identity = self.exo_identity(&mut proof, component)?;
            let class = pointer(u64_at(&identity, 0x4c)?)?;
            let class_name = self.class_name(class)?;
            if class_name != "ParticleSystemComponent" {
                continue;
            }
            self.field(class, "Owner", "ObjectProperty", 0x74, 8)?;
            self.field(class, "Template", "ObjectProperty", 0x224, 8)?;
            let owner = u64_at(&self.exo_capture(&mut proof, add(component, 0x74)?, 8)?, 0)?;
            let template = u64_at(&self.exo_capture(&mut proof, add(component, 0x224)?, 8)?, 0)?;
            if owner != actor as u64 || template == 0 {
                continue;
            }
            let template = self.exo_identity(&mut proof, pointer(template)?)?;
            let template_class = pointer(u64_at(&template, 0x4c)?)?;
            if self.class_name(template_class)? != "ParticleSystem" {
                continue;
            }
            let package = self.exo_identity(&mut proof, pointer(u64_at(&template, 0x3c)?)?)?;
            if self.class_name(pointer(u64_at(&package, 0x4c)?)?)? != "Package" {
                continue;
            }
            if u64_at(&package, 0x3c)? != 0 {
                continue;
            }
            found |= exo_effect(
                &class_name,
                owner,
                actor,
                &self.name(u32_at(&template, 0x44)?)?,
                &self.name(u32_at(&package, 0x44)?)?,
                u32_at(&template, 0x48)?,
                u32_at(&package, 0x48)?,
            );
        }
        self.recheck_exo(&proof)?;
        if !found {
            return Err("No owned exo crafting effect".into());
        }
        Ok(proof)
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
        let definition_value = u64_at(&actor_before, 0x3b8)?;
        if definition_value == 0 {
            return Err("Unsupported ground spawn definition".into());
        }
        let definition = pointer(definition_value)?;
        let definition_before = self.read(definition, 0x54)?;
        let definition_class = pointer(u64_at(&definition_before, 0x4c)?)?;
        let classes = [owner.classes[2], actor_class, definition_class];
        let mut identities = [(0, 0); 3];
        for (index, class) in classes.into_iter().enumerate() {
            identities[index] = (class, self.object(class)?.0);
        }
        let definition_name = self.name(u32_at(&definition_before, 0x44)?)?;
        let definition_outer_address = pointer(u64_at(&definition_before, 0x3c)?)?;
        let definition_outer = self.object(definition_outer_address)?;
        let is_exo = exo_definition(&definition_name, &definition_outer.2);
        let kind = if is_exo {
            GroundSpawnKind::Exobyte
        } else {
            ground_spawn_kind(&definition_name, &definition_outer.2)
                .ok_or("Unsupported ground spawn definition")?
        };
        if self.class_name(definition_outer.1)? != "Package" {
            return Err("Unsupported pickup definition package".into());
        }
        if self.class_name(actor_class)? != "DCServerPhysicsObject"
            || self.class_name(definition_class)? != "DCDamageableObject"
            || u32_at(&definition_before, 0x48)? != 0
        {
            return Err("Unsupported nearby collectible definition".into());
        }
        let exo_proof = if is_exo {
            // Asset package names must be unnumbered as well as the definition.
            if u32_at(&self.read(definition_outer_address, 0x54)?, 0x48)? != 0 {
                return Err("Unsupported pickup definition package".into());
            }
            let mut proof = self.owned_exo_effect(actor, actor_class)?;
            let package = self.exo_identity(&mut proof, definition_outer_address)?;
            if u32_at(&package, 0x48)? != 0 {
                return Err("Unsupported pickup definition package".into());
            }
            Some(proof)
        } else {
            None
        };
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
            || self.object(definition_outer_address)? != definition_outer
            || (nearby && self.read(add(controller, 0x49c)?, 8)? != target_bytes)
        {
            return Err("Nearby collectible changed during sample".into());
        }
        for (class, identity) in identities {
            if self.object(class)?.0 != identity {
                return Err("Collectible class identity changed".into());
            }
        }
        if let Some(proof) = exo_proof {
            self.recheck_exo(&proof)?;
        }
        Ok(Collectible { position, kind })
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
        self.ground_scan.bind_owner(owner, base);
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
                Ok(spawn) => {
                    self.ground_scan
                        .sweep
                        .observe((address, index), spawn.clone());
                    result.push(spawn);
                }
                Err(error) => {
                    if self.optional_interrupted() {
                        break;
                    }
                    if !known_spawn_absence(&error) {
                        self.ground_scan.sweep.uncertain = true;
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
                    } else {
                        self.ground_scan.sweep.uncertain = true;
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
                            if header.is_err() {
                                self.ground_scan.sweep.uncertain = true;
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
                        self.ground_scan.sweep.uncertain = true;
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
                            self.ground_scan.sweep.uncertain = true;
                            continue;
                        };
                        let value = name == "DCServerPhysicsObject";
                        self.ground_scan.sweep.check_bound(
                            self.ground_scan.class_names.len(),
                            4096,
                            "Class cache bound reached",
                        )?;
                        self.ground_scan.class_names.insert(class, value);
                        value
                    };
                    if !supported {
                        continue;
                    }
                    self.ground_scan.sweep.check_bound(
                        self.ground_scan.candidates.len(),
                        256,
                        "Ground spawn candidate bound reached",
                    )?;
                    // Every eligible actor contributes to this pass, including cached candidates.
                    match self.collectible_actor(owner, player, address, false) {
                        Ok(spawn) => {
                            self.ground_scan.sweep.observe((address, index), spawn);
                            if !self.ground_scan.candidates.contains(&(address, index)) {
                                self.ground_scan.candidates.push((address, index));
                            }
                        }
                        Err(error) => {
                            if self.optional_interrupted() {
                                self.ground_scan.cursor = next_scan_cursor(index, true);
                                return Err("Optional scan budget reached".into());
                            }
                            if !known_spawn_absence(&error) {
                                self.ground_scan.sweep.uncertain = true;
                            }
                        }
                    }
                }
            }
            self.ground_scan.cursor = end;
            if end == count {
                self.ground_scan.completed = true;
                self.ground_scan.pending_sweep = self.ground_scan.sweep.finish(Instant::now());
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
            self.ground_scan.pending_sweep = None;
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
    pub fn set_feats_enabled(&mut self, enabled: bool) {
        if self.feats_enabled != enabled {
            self.feats_enabled = enabled;
            self.feats = None;
            self.feats_attempt = None;
        }
    }
    fn bind_feats(&mut self, owner: &Ownership) -> bool {
        let pawn = owner.objects[3].0;
        let binding = (|| -> Result<(usize, u64), String> {
            Ok((
                self.ptr(add(pawn, 0x5c)?)?,
                u64_at(&self.read(add(pawn, 0x68)?, 8)?, 0)?,
            ))
        })();
        let changed = self.feats_owner != Some(owner.objects)
            || self.feats.as_ref().is_some_and(|snapshot| {
                binding.as_ref().ok() != Some(&(snapshot.entity, snapshot.entity_id))
            });
        if changed {
            self.feats = None;
            self.feats_attempt = None;
            self.feats_owner = Some(owner.objects);
        }
        if binding.is_err() {
            self.feats = None;
            self.feats_status = "Live feats unavailable while the character is changing.".into();
            return false;
        }
        true
    }
    fn refresh_feats(
        &mut self,
        owner: &Ownership,
        progress: &mut impl FnMut(Result<Sample, String>),
    ) -> bool {
        if !self.bind_feats(owner) {
            return false;
        }
        let pawn = owner.objects[3].0;
        if self
            .feats_attempt
            .is_some_and(|last| last.elapsed() < std::time::Duration::from_secs(8))
        {
            return false;
        }
        self.feats_attempt = Some(Instant::now());
        // Runs serially on the existing session reader, never on the UI thread.
        // A separate bounded budget keeps this optional scan out of the map budget.
        let started = Instant::now();
        let mut bytes = 0usize;
        let mut reads = 0usize;
        let mut maps = self.maps.borrow().clone();
        progress(self.sample_physics(false));
        let mut last_progress = Instant::now();
        let result =
            crate::feat_memory::read(self.candidate.mapping.start, pawn, |address, len| {
                bytes = bytes.checked_add(len).ok_or("Feat read budget overflow")?;
                if bytes > 12 * 1024 * 1024
                    || started.elapsed() > std::time::Duration::from_secs(2)
                    || self
                        .cancel
                        .as_ref()
                        .is_some_and(|token| token.load(Ordering::Acquire))
                {
                    return Err("Live feat scan exceeded its budget or was cancelled".into());
                }
                // Cooperatively publish newly read physics while this larger
                // optional snapshot runs. Never relabel old coordinates as fresh.
                if last_progress.elapsed() >= std::time::Duration::from_millis(200) {
                    progress(self.sample_physics(false));
                    last_progress = Instant::now();
                }
                if reads != 0 && reads.is_multiple_of(128) {
                    maps = process::mappings(self.candidate.identity.pid)
                        .map_err(|e| e.to_string())?;
                }
                reads += 1;
                let end = add(address, len)?;
                let map = maps
                    .iter()
                    .find(|map| map.readable && address >= map.start && end <= map.end)
                    .ok_or("Live feat data is no longer readable")?;
                memory::read_discovery(&self.candidate.identity, map, address, len)
                    .map_err(|e| e.to_string())
            });
        match result {
            Ok(snapshot) => {
                self.feats_status =
                    format!("Live feat definitions matched: {}", snapshot.entries.len());
                self.feats = Some(Arc::new(snapshot));
            }
            Err(error) => {
                self.feats = None;
                self.feats_status = format!("Live feats unavailable: {error}");
            }
        }
        true
    }
    pub fn sample(&mut self) -> Result<Sample, String> {
        self.sample_with_progress(|_| {})
    }
    pub fn sample_with_progress(
        &mut self,
        mut progress: impl FnMut(Result<Sample, String>),
    ) -> Result<Sample, String> {
        let sample = self.sample_physics(true)?;
        if self.feats_enabled && self.refresh_feats(&sample.owner, &mut progress) {
            // The initial coordinates predate the optional scan; read them again.
            return self.sample_physics(false);
        }
        Ok(sample)
    }
    fn sample_physics(&mut self, optional: bool) -> Result<Sample, String> {
        let saved = self.ground_scan.clone();
        let result = self.sample_inner(optional);
        if result.is_err() {
            self.ground_scan = saved;
        }
        result
    }
    fn sample_inner(&mut self, optional: bool) -> Result<Sample, String> {
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
        before.area = area.ok().map(Box::new);
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
        let (ground_spawns, ground_spawns_status) = if self.ground_spawns_enabled && optional {
            self.collectible_deadline
                .set(Some(Instant::now() + std::time::Duration::from_millis(100)));
            let saved = self.ground_scan.clone();
            let result = self.ground_spawns(&before, position);
            if result.is_err() {
                self.ground_scan.restore_for_owner(saved, &before);
            }
            self.collectible_deadline.set(None);
            result
                .unwrap_or_else(|error| (Vec::new(), format!("Ground spawns unavailable: {error}")))
        } else {
            (Vec::new(), "Ground spawns off".into())
        };
        let mut after = self.ownership()?;
        after.area = self.area_context(after.objects[3].0).ok().map(Box::new);
        if self.read(self.rva(NAMES_RVA)?, 16)? != name_header {
            self.ground_scan.class_names.clear();
            return Err("Name table changed during sample; discarded".into());
        }
        self.unchanged()?;
        if before != after {
            self.ground_scan = GroundScan::default();
            return Err("Controlled object changed during sample; discarded".into());
        }
        self.ground_scan.accept_pending_sweep();
        self.bind_feats(&before);
        Ok(Sample {
            ground_spawn_sweep: (self.ground_scan.owner.as_ref() == Some(&before))
                .then(|| self.ground_scan.published_sweep.clone())
                .flatten(),
            position,
            feats: self.feats.clone(),
            feats_status: self.feats_status.clone(),
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
    fn completed_sweep_unions_resumed_chunks_and_rejects_uncertain_absence() {
        let mut pass = SweepAccumulator::default();
        let spawn = |x| Collectible {
            position: [x, 2.0, 3.0],
            kind: GroundSpawnKind::Blue,
        };
        pass.observe((0x10000, 1), spawn(1.0));
        // Interruption does not finish or clear accumulated observations.
        pass.observe((0x20000, 2), spawn(2.0));
        pass.observe((0x10000, 1), spawn(4.0));
        let timestamp = Instant::now();
        let sweep = pass.finish(timestamp).unwrap();
        assert_eq!(sweep.completed_at, timestamp);
        assert_eq!(sweep.spawns.len(), 2);
        assert!(sweep.spawns.iter().any(|s| s.position[0] == 4.0));
        assert!(pass.seen.is_empty());
        pass.observe((0x30000, 3), spawn(3.0));
        pass.uncertain = true;
        assert!(pass.finish(Instant::now()).is_none());
        assert!(pass.finish(Instant::now()).unwrap().spawns.is_empty());
        assert!(known_spawn_absence("Nearby collectible hidden or deleted"));
        assert!(known_spawn_absence("Unsupported pickup definition package"));
        assert!(!known_spawn_absence(
            "Experimental read outside readable mapping"
        ));
    }
    #[test]
    fn discovery_bounds_cannot_certify_missing_spawns() {
        for limit in [256, 4096] {
            let mut pass = SweepAccumulator::default();
            pass.observe(
                (0x10000, 1),
                Collectible {
                    position: [1., 2., 3.],
                    kind: GroundSpawnKind::Blue,
                },
            );
            assert!(pass.check_bound(limit - 1, limit, "bound").is_ok());
            assert!(pass.check_bound(limit, limit, "bound").is_err());
            // Later chunks cannot turn a skipped actor into a certified absence.
            pass.observe(
                (0x20000, 2),
                Collectible {
                    position: [4., 5., 6.],
                    kind: GroundSpawnKind::Gold,
                },
            );
            assert!(pass.finish(Instant::now()).is_none());
        }
    }
    #[test]
    fn completed_sweep_publishes_only_after_valid_sample_and_repeats_same_timestamp() {
        let mut scan = GroundScan::default();
        let first = Instant::now();
        scan.pending_sweep = Some(GroundSpawnSweep {
            completed_at: first,
            spawns: Vec::new(),
        });
        assert!(scan.published_sweep.is_none());
        scan.accept_pending_sweep();
        assert_eq!(scan.published_sweep.as_ref().unwrap().completed_at, first);
        scan.accept_pending_sweep();
        assert_eq!(scan.published_sweep.as_ref().unwrap().completed_at, first);
        scan.pending_sweep = Some(GroundSpawnSweep {
            completed_at: first + std::time::Duration::from_secs(1),
            spawns: Vec::new(),
        });
        let saved = GroundScan {
            published_sweep: scan.published_sweep.clone(),
            ..Default::default()
        };
        scan = saved;
        scan.accept_pending_sweep();
        assert_eq!(scan.published_sweep.as_ref().unwrap().completed_at, first);
        assert!(!scan.sweep.uncertain);
    }
    #[test]
    fn streaming_membership_does_not_restart_scan_but_zone_or_player_changes_do() {
        let mut owner = Sample::synthetic(1).owner;
        owner.area = Some(Box::new(AreaContext {
            world: (0x50000, 7),
            array: (0x60000, 1, 2),
            members: vec![0x70000],
            signals: Vec::new(),
            key: Some("TEST01".into()),
            world_chain: None,
        }));
        let mut scan = GroundScan::default();
        scan.bind_owner(&owner, 0x80000);
        scan.cursor = 500;
        let mut changed = owner.clone();
        changed.area.as_mut().unwrap().members.push(0x90000);
        changed.area.as_mut().unwrap().array.1 += 1;
        scan.bind_owner(&changed, 0x80000);
        assert_eq!(scan.cursor, 500);
        assert_eq!(scan.owner, Some(changed.clone()));
        changed.area.as_mut().unwrap().key = Some("TEST02".into());
        scan.bind_owner(&changed, 0x80000);
        assert_eq!(scan.cursor, 0);
        scan.cursor = 42;
        changed.objects[3].1 += 1;
        scan.bind_owner(&changed, 0x80000);
        assert_eq!(scan.cursor, 0);
    }
    #[test]
    fn failed_chunk_rolls_back_progress_without_poisoning_completed_pass() {
        let owner = Sample::synthetic(1).owner;
        let mut scan = GroundScan {
            owner: Some(owner.clone()),
            cursor: 100,
            ..Default::default()
        };
        scan.sweep.observe(
            (0x10000, 1),
            Collectible {
                position: [1.0, 2.0, 3.0],
                kind: GroundSpawnKind::Blue,
            },
        );
        let saved = scan.clone();
        scan.cursor = 200;
        scan.sweep.uncertain = true;
        scan.pending_sweep = Some(GroundSpawnSweep {
            completed_at: Instant::now(),
            spawns: Vec::new(),
        });
        scan.restore_for_owner(saved.clone(), &owner);
        assert_eq!(scan.cursor, 100);
        assert!(scan.pending_sweep.is_none());
        assert_eq!(scan.sweep.finish(Instant::now()).unwrap().spawns.len(), 1);
        scan.restore_for_owner(saved, &Sample::synthetic(2).owner);
        assert!(scan.owner.is_none());
        assert!(scan.published_sweep.is_none());
        assert_eq!(scan.cursor, 0);
    }
    #[test]
    fn sweep_bound_cannot_certify_missing_objects() {
        let mut pass = SweepAccumulator::default();
        for index in 0..257 {
            pass.observe(
                (0x10000 + index as usize, index),
                Collectible {
                    position: [index as f32, 0.0, 0.0],
                    kind: GroundSpawnKind::Blue,
                },
            );
        }
        assert_eq!(pass.seen.len(), 256);
        assert!(pass.finish(Instant::now()).is_none());
    }
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
    fn exo_requires_exact_owned_effect_assets() {
        assert!(exo_definition("FXSM103_DAMAGEABLEOBJECT", "DFXSM103"));
        assert!(!exo_definition("FXSM103_DAMAGEABLEOBJECT", "OTHER"));
        let matches = |class, owner, template, package, tn, pn| {
            exo_effect(class, owner, 0x10000, template, package, tn, pn)
        };
        assert!(matches(
            "ParticleSystemComponent",
            0x10000,
            "FXGEN109_FX_BAUBLE_CRAFTING",
            "DFXGEN109",
            0,
            0
        ));
        for (class, owner, template, package, tn, pn) in [
            (
                "Other",
                0x10000,
                "FXGEN109_FX_BAUBLE_CRAFTING",
                "DFXGEN109",
                0,
                0,
            ),
            (
                "ParticleSystemComponent",
                0x20000,
                "FXGEN109_FX_BAUBLE_CRAFTING",
                "DFXGEN109",
                0,
                0,
            ),
            (
                "ParticleSystemComponent",
                0x10000,
                "OTHER",
                "DFXGEN109",
                0,
                0,
            ),
            (
                "ParticleSystemComponent",
                0x10000,
                "FXGEN109_FX_BAUBLE_CRAFTING",
                "OTHER",
                0,
                0,
            ),
            (
                "ParticleSystemComponent",
                0x10000,
                "FXGEN109_FX_BAUBLE_CRAFTING",
                "DFXGEN109",
                1,
                0,
            ),
            (
                "ParticleSystemComponent",
                0x10000,
                "FXGEN109_FX_BAUBLE_CRAFTING",
                "DFXGEN109",
                0,
                1,
            ),
        ] {
            assert!(!matches(class, owner, template, package, tn, pn));
        }
        assert!(known_spawn_absence("No owned exo crafting effect"));
        assert!(!known_spawn_absence(
            "Exo effect linkage changed during sample"
        ));
        assert!(!known_spawn_absence("Exo component array bound exceeded"));
    }
    #[test]
    fn exo_proof_detects_identity_membership_and_linkage_changes() {
        // Minimal captured identity, array header, membership, owner and template.
        let proof = ExoProof(vec![
            (0x10008, vec![1; 4]),
            (0x1003c, vec![2; 24]),
            (0x100c0, vec![3; 16]),
            (0x20000, vec![4; 32]),
            (0x30074, vec![5; 8]),
            (0x30224, vec![6; 8]),
        ]);
        let read = |address, size| -> Result<Vec<u8>, String> {
            let value = &proof.0.iter().find(|(a, _)| *a == address).unwrap().1;
            assert_eq!(size, value.len());
            Ok(value.clone())
        };
        assert!(validate_exo_proof(&proof, read).is_ok());
        for (changed, _) in &proof.0 {
            assert!(
                validate_exo_proof(&proof, |address, size| {
                    let mut value = read(address, size)?;
                    if address == *changed {
                        value[0] ^= 1;
                    }
                    Ok(value)
                })
                .is_err()
            );
        }
        assert!(validate_exo_proof(&proof, |_, _| Err("unreadable".into())).is_err());
    }
    #[test]
    fn exo_component_membership_is_bounded() {
        let mut header = vec![0; 16];
        assert_eq!(component_array(&header).unwrap(), (0, 0));
        header[..8].copy_from_slice(&0x10000u64.to_le_bytes());
        header[8..12].copy_from_slice(&4u32.to_le_bytes());
        header[12..16].copy_from_slice(&64u32.to_le_bytes());
        assert_eq!(component_array(&header).unwrap(), (0x10000, 4));
        header[12..16].copy_from_slice(&65u32.to_le_bytes());
        assert!(component_array(&header).is_err());
        header[12..16].copy_from_slice(&3u32.to_le_bytes());
        assert!(component_array(&header).is_err());
        header[..8].fill(0);
        header[12..16].copy_from_slice(&4u32.to_le_bytes());
        assert!(component_array(&header).is_err());
    }
    #[test]
    fn pickup_filter_rejects_shared_effects_props_and_unproven_variants() {
        for (name, package) in [
            ("FXSM103_DAMAGEABLEOBJECT", "DFXSM103"),
            ("FXSM103_DAMAGEABLEOBJECT", "DDSGPROGEN107"),
            ("DSGPROGEN107_MAGNIFYINGGLASS_BLUE_DO", "OTHER_PACKAGE"),
            ("DSGPROGEN107_MAGNIFYINGGLASS_GREY_DO", "DDSGPROGEN107"),
            ("DSGPROGEN107_MAGNIFYINGGLASS_PURPLE_DO", "DDSGPROGEN107"),
            ("DSGPROGEN107_QUESTIONMARK_PURPLE_DO", "DDSGPROGEN107"),
            ("MAGNIFYINGGLASS_AFOOLS_DO", "DDSGPROGEN107"),
            ("MAGNIFYINGGLASS_BLUE", "DDSGPROGEN107"),
            ("", "DDSGPROGEN107"),
        ] {
            assert_eq!(ground_spawn_kind(name, package), None, "{package}.{name}");
        }
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
        ] {
            assert_eq!(ground_spawn_kind(name, "DDSGPROGEN107"), Some(kind));
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
    fn fallback_world_ancestor_instances_match_observed_chain_only() {
        for (depth, name, class, number) in [
            (0, "WorldInfo", "WorldInfo", 1),
            (1, "PersistentLevel", "Level", 0),
            (2, "TheWorld", "World", 0),
            (3, "MGOT103_ALT_US", "Package", 0),
        ] {
            assert!(supported_world_ancestor(depth, name, class, number));
            assert!(!supported_world_ancestor(depth, name, class, number + 1));
            assert!(!supported_world_ancestor(depth, "Other", class, number));
            assert!(!supported_world_ancestor(depth, name, "Other", number));
        }
        assert!(!supported_world_ancestor(0, "WorldInfo", "WorldInfo", 0));
        assert!(!supported_world_ancestor(4, "MGOT103_ALT_US", "Package", 0));
    }
    #[test]
    fn observed_world_alias_requires_exact_single_designer_and_no_audio() {
        let choose = |names: &[&str], package| {
            supported_world_alias(
                &names.iter().map(|name| (*name).into()).collect::<Vec<_>>(),
                package,
            )
        };
        assert_eq!(
            choose(&["MGOT103_DESIGNERDATA"], "MGOT103_ALT_US"),
            Some("MGOT103".into())
        );
        for names in [
            vec![],
            vec!["MGOT103_BL_DESIGNERDATA"],
            vec!["MGOT103_DESIGNERDATA", "OTHER_DESIGNERDATA"],
            vec!["MGOT103_DESIGNERDATA", "MGOT103_DESIGNERDATA"],
            vec!["MGOT103_DESIGNERDATA", "MGOT103_AUDIO"],
            vec!["MGOT103_DESIGNERDATA", "MGOT103_AUDIO", "OTHER_AUDIO"],
        ] {
            assert_eq!(choose(&names, "MGOT103_ALT_US"), None);
        }
        for package in [
            "MGOT103",
            "MGOT103_ALT_EU",
            "OTHER_ALT_US",
            "MGOT103_ALT_US_1",
        ] {
            assert_eq!(choose(&["MGOT103_DESIGNERDATA"], package), None);
        }
        assert!(
            supported_world_alias(&vec!["MGOT103_DESIGNERDATA".into(); 2049], "MGOT103_ALT_US")
                .is_none()
        );
    }
    #[test]
    fn fallback_chain_identity_invalidates_area_and_spawn_context() {
        let mut original = Sample::synthetic(1);
        original.area_key = Some("MGOT103".into());
        original.owner.area = Some(Box::new(AreaContext {
            world: (0x40000, 1),
            array: (0x50000, 1, 1),
            members: vec![0x60000],
            signals: vec![],
            key: Some("MGOT103".into()),
            world_chain: Some(vec![AreaAncestor {
                address: 0x70000,
                index: 5,
                class: (0x80000, 6),
                name: "MGOT103_ALT_US".into(),
                number: 0,
                outer: 0,
            }]),
        }));
        let mut changed = original.clone();
        changed.owner.area.as_mut().unwrap().members.push(0x90000);
        assert!(original.same_spawn_context(&changed));
        assert!(original.owner.same_spawn_identity(&changed.owner));
        let mut unknown = original.clone();
        unknown.owner.area.as_mut().unwrap().world_chain = None;
        unknown.owner.area.as_mut().unwrap().key = None;
        // The public cache retains its last known area key across an unknown gap.
        assert!(original.same_spawn_context(&unknown));
        assert!(unknown.same_spawn_context(&original));
        assert!(!original.owner.same_spawn_identity(&unknown.owner));
        changed
            .owner
            .area
            .as_mut()
            .unwrap()
            .world_chain
            .as_mut()
            .unwrap()[0]
            .index += 1;
        assert_ne!(original.owner.area, changed.owner.area);
        assert!(!original.same_spawn_context(&changed));
        assert!(!original.owner.same_spawn_identity(&changed.owner));
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
            feats_enabled: false,
            feats: None,
            feats_owner: None,
            feats_attempt: None,
            feats_status: String::new(),
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
    fn cached_spawn_context_ignores_streaming_but_rejects_reused_world() {
        let mut original = Sample::synthetic(1);
        original.area_key = Some("MGOT101".into());
        original.owner.area = Some(Box::new(AreaContext {
            world: (0x40000, 1),
            array: (0x50000, 1, 1),
            members: vec![0x60000],
            signals: vec![],
            key: Some("MGOT101".into()),
            world_chain: None,
        }));
        let mut streamed = original.clone();
        streamed.owner.area.as_mut().unwrap().members.push(0x70000);
        assert!(original.same_spawn_context(&streamed));
        streamed.owner.area.as_mut().unwrap().world.1 += 1;
        assert!(!original.same_spawn_context(&streamed));
        let mut unknown = original.clone();
        unknown.owner.area = None;
        assert!(original.same_spawn_context(&unknown));
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
        replaced.area = Some(Box::new(AreaContext {
            world: (0x40000, 1),
            array: (0x50000, 1, 1),
            members: vec![0x60000],
            signals: vec![],
            key: Some("MGOT101".into()),
            world_chain: None,
        }));
        assert_ne!(original, replaced);
    }
}
