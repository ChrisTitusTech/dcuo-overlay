//! Pure read-only snapshot of experimentally proven native feat structures.
//! Callers establish build/process identity and impose their own read/time budget.
use crate::feat_memory_codec;
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
    time::Instant,
};

const MAX_NODES: usize = 20_000;
const MAX_ROWS: usize = 8192;
const MAX_ARCHIVE: usize = 2 * 1024 * 1024;
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub entity: usize,
    /// Native entity identifier, not a Census character identifier.
    pub entity_id: u64,
    pub entries: Vec<Entry>,
    pub observed_at: Instant,
}
#[derive(Clone, Debug)]
pub struct Entry {
    pub id: u32,
    pub title: String,
    pub description: String,
    pub points: u32,
    pub completed: bool,
    pub eligible: bool,
}
/// Session-only cache. The owner must discard this with its PID/build-bound reader.
#[derive(Default)]
pub struct Cache {
    state: Option<CacheState>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Binding {
    base: usize,
    manager: usize,
    entity: usize,
    entity_id: u64,
    component: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct RowSignature {
    tree: usize,
    count: usize,
    nodes: usize,
    root: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct ArchiveSignature {
    type_root: usize,
    record: usize,
    vtable: usize,
    total: usize,
    flags: [u8; 3],
    first_capacity: u32,
    first_len: u32,
    first_links: [u8; 24],
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct DefinitionSignature {
    root: usize,
    archive: ArchiveSignature,
}
#[derive(Clone)]
struct CacheState {
    binding: Binding,
    signature: Option<DefinitionSignature>,
    definitions: BTreeMap<u32, Arc<feat_memory_codec::Definition>>,
    covered: HashSet<u32>,
    completed_signature: Option<RowSignature>,
    completed_rows: BTreeMap<u32, (bool, bool)>,
    proven_completed: BTreeMap<u32, bool>,
}
impl CacheState {
    fn new(binding: Binding) -> Self {
        Self {
            binding,
            signature: None,
            definitions: BTreeMap::new(),
            covered: HashSet::new(),
            completed_signature: None,
            completed_rows: BTreeMap::new(),
            proven_completed: BTreeMap::new(),
        }
    }
}
fn add(address: usize, offset: usize) -> Result<usize, String> {
    address
        .checked_add(offset)
        .ok_or_else(|| "Feat address overflow".into())
}
fn pointer(value: usize) -> Result<usize, String> {
    if !(0x10000..=0x0000_7fff_ffff_ffff).contains(&value) {
        return Err("Invalid feat pointer".into());
    }
    Ok(value)
}
fn d(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("bounded field"))
}
fn q(bytes: &[u8], offset: usize) -> Result<usize, String> {
    usize::try_from(u64::from_le_bytes(
        bytes[offset..offset + 8].try_into().expect("bounded field"),
    ))
    .map_err(|_| "Feat pointer overflow".into())
}
struct Reader<F> {
    read: F,
    watched: BTreeMap<(usize, usize), Vec<u8>>,
    definition_proofs: Vec<(usize, [u8; 0x80])>,
    nodes: usize,
}
impl<F: FnMut(usize, usize) -> Result<Vec<u8>, String>> Reader<F> {
    fn raw(&mut self, address: usize, len: usize) -> Result<Vec<u8>, String> {
        pointer(address)?;
        add(address, len)?;
        if len == 0 || len > 4096 {
            return Err("Feat read exceeds bound".into());
        }
        let bytes = (self.read)(address, len)?;
        if bytes.len() != len {
            return Err("Short feat read".into());
        }
        Ok(bytes)
    }
    fn bytes(&mut self, address: usize, len: usize) -> Result<Vec<u8>, String> {
        let bytes = self.raw(address, len)?;
        if let Some(previous) = self.watched.get(&(address, len)) {
            if previous != &bytes {
                return Err("Feat structure changed during snapshot".into());
            }
        } else {
            self.watched.insert((address, len), bytes.clone());
        }
        Ok(bytes)
    }
    fn ptr(&mut self, address: usize) -> Result<usize, String> {
        pointer(q(&self.bytes(address, 8)?, 0)?)
    }
    fn node(&mut self) -> Result<(), String> {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err("Feat node bound exceeded".into());
        }
        Ok(())
    }
    fn recheck(&mut self) -> Result<(), String> {
        for (&(address, len), previous) in &self.watched {
            if (self.read)(address, len)? != *previous {
                return Err("Feat snapshot changed; discarded".into());
            }
        }
        for (address, previous) in &self.definition_proofs {
            let current = (self.read)(*address, 0x80)?;
            if current.len() != 0x80 {
                return Err("Short feat definition recheck".into());
            }
            for range in [8..16, 0x50..0x54, 0x60..0x70, 0x78..0x7c] {
                if current[range.clone()] != previous[range] {
                    return Err("Feat definition changed; discarded".into());
                }
            }
        }
        Ok(())
    }
    fn component(&mut self, entity: usize) -> Result<usize, String> {
        let data = self.ptr(add(entity, 8)?)?;
        let header = self.bytes(add(data, 0x90)?, 12)?;
        let nodes = pointer(q(&header, 0)?)?;
        let mut index = d(&header, 8);
        let mut seen = HashSet::new();
        for _ in 0..64 {
            if index == 0 {
                return Err("Feat component unavailable".into());
            }
            if index >= 1024 || !seen.insert(index) {
                return Err("Invalid feat component tree".into());
            }
            self.node()?;
            let bytes = self.bytes(add(nodes, index as usize * 28)?, 28)?;
            let key = d(&bytes, 16);
            if key == 0x34 {
                return pointer(q(&bytes, 20)?);
            }
            index = d(&bytes, if key > 0x34 { 4 } else { 8 });
        }
        Err("Feat component ancestry bound exceeded".into())
    }
    fn row_signature(&mut self, tree: usize) -> Result<RowSignature, String> {
        let count = d(&self.bytes(add(tree, 0x1c)?, 4)?, 0) as usize;
        if count == 0 || count > MAX_ROWS + 1 {
            return Err("Invalid feat row count".into());
        }
        let header = self.bytes(add(tree, 0x24)?, 12)?;
        Ok(RowSignature {
            tree,
            count,
            nodes: q(&header, 0)?,
            root: d(&header, 8),
        })
    }
    fn rows(
        &mut self,
        signature: &RowSignature,
        rows: &mut BTreeMap<u32, (bool, bool)>,
    ) -> Result<(), String> {
        let RowSignature {
            count, nodes, root, ..
        } = *signature;
        let mut todo = vec![root];
        let mut seen = HashSet::new();
        while let Some(index) = todo.pop() {
            if index == 0 {
                continue;
            }
            if index as usize >= MAX_NODES || !seen.insert(index) {
                return Err("Invalid feat row tree".into());
            }
            if seen.len() >= count || rows.len() >= MAX_ROWS {
                return Err("Feat row bound exceeded".into());
            }
            self.node()?;
            pointer(nodes)?;
            let node = self.bytes(add(nodes, index as usize * 32)?, 32)?;
            todo.push(d(&node, 4));
            todo.push(d(&node, 8));
            let row = pointer(q(&node, 24)?)?;
            let status = self.bytes(add(row, 0x10)?, 16)?;
            let id = d(&status, 0);
            if id == 0
                || rows
                    .insert(id, (d(&status, 8) != 0, status[13] != 0 && status[12] == 0))
                    .is_some()
            {
                return Err("Duplicate or invalid feat row ID".into());
            }
        }
        if seen.len() != count - 1 {
            return Err("Incomplete feat row tree".into());
        }
        Ok(())
    }
    fn definitions(&mut self, cache: usize, token: usize) -> Result<BTreeMap<u32, u32>, String> {
        let root = q(&self.bytes(add(cache, 0x50)?, 8)?, 0)?;
        let mut todo = vec![root];
        let mut seen = HashSet::new();
        let mut definitions = BTreeMap::new();
        while let Some(address) = todo.pop() {
            if address == 0 {
                continue;
            }
            pointer(address)?;
            if !seen.insert(address) {
                return Err("Cyclic or shared feat definition tree".into());
            }
            self.node()?;
            // Wrapper/LRU state may mutate during ordinary lazy UI population.
            // Only immutable definition identity, tree links and offsets matter.
            let bytes: [u8; 0x80] = self
                .raw(address, 0x80)?
                .try_into()
                .map_err(|_| "Short feat definition")?;
            self.definition_proofs.push((address, bytes));
            let kind = q(&bytes, 8)?;
            todo.push(q(&bytes, 0x60)?);
            todo.push(q(&bytes, 0x68)?);
            if kind == token {
                let id = d(&bytes, 0x50);
                let offset = d(&bytes, 0x78);
                if id == 0 || definitions.insert(id, offset).is_some() {
                    return Err("Duplicate or invalid feat definition ID".into());
                }
            }
        }
        Ok(definitions)
    }
    fn archive_signature(
        &mut self,
        cache: usize,
        token: usize,
        expected_vtable: usize,
    ) -> Result<ArchiveSignature, String> {
        let type_root = q(&self.bytes(add(cache, 0x70)?, 8)?, 0)?;
        let mut node = type_root;
        let mut seen = HashSet::new();
        let record = loop {
            pointer(node)?;
            if !seen.insert(node) || seen.len() > 128 {
                return Err("Invalid feat type tree".into());
            }
            self.node()?;
            let key = q(&self.bytes(add(node, 0x114)?, 8)?, 0)?;
            let children = self.bytes(add(node, 0x124)?, 16)?;
            if key == token {
                break node;
            }
            node = q(&children, if token < key { 0 } else { 8 })?;
        };
        let archive = add(record, 8)?;
        if self.ptr(archive)? != expected_vtable {
            return Err("Unsupported feat archive type".into());
        }
        let total = d(&self.bytes(add(archive, 0xd0)?, 4)?, 0) as usize;
        if total == 0 || total > MAX_ARCHIVE {
            return Err("Feat archive exceeds byte bound".into());
        }
        let flags = self
            .bytes(add(archive, 0xd4)?, 3)?
            .try_into()
            .map_err(|_| "Short archive flags")?;
        let first = add(archive, 8)?;
        Ok(ArchiveSignature {
            type_root,
            record,
            vtable: expected_vtable,
            total,
            flags,
            first_capacity: d(&self.bytes(add(first, 8)?, 4)?, 0),
            first_len: d(&self.bytes(add(first, 0x10)?, 4)?, 0),
            first_links: self
                .bytes(add(first, 0x9c)?, 24)?
                .try_into()
                .map_err(|_| "Short archive links")?,
        })
    }
    fn archive(&mut self, signature: &ArchiveSignature) -> Result<Vec<u8>, String> {
        let total = signature.total;
        let mut block = add(signature.record, 16)?;
        let mut previous = 0;
        let mut seen = HashSet::new();
        let mut bytes = Vec::with_capacity(total);
        loop {
            pointer(block)?;
            if !seen.insert(block) || seen.len() > 1024 {
                return Err("Invalid feat archive block chain".into());
            }
            let capacity = d(&self.bytes(add(block, 8)?, 4)?, 0)
                .checked_add(1)
                .ok_or("Feat block capacity overflow")? as usize;
            let len = d(&self.bytes(add(block, 0x10)?, 4)?, 0) as usize;
            let links = self.bytes(add(block, 0x9c)?, 24)?;
            if q(&links, 8)? != previous
                || capacity > MAX_ARCHIVE
                || len == 0
                || len > capacity
                || bytes.len().checked_add(len).is_none_or(|n| n > total)
            {
                return Err("Invalid feat archive block".into());
            }
            let data = pointer(q(&links, 0)?)?;
            for offset in (0..len).step_by(4096) {
                bytes.extend(self.bytes(add(data, offset)?, (len - offset).min(4096))?);
            }
            let next = q(&links, 16)?;
            if next == 0 {
                break;
            }
            previous = block;
            block = next;
        }
        if bytes.len() != total {
            return Err("Incomplete feat archive".into());
        }
        Ok(bytes)
    }
}
/// Read only through `read`; unsupported individual serialized prefixes stay absent.
/// Absence is never converted into a completion or incompletion observation.
pub fn read(
    base: usize,
    pawn: usize,
    read: impl FnMut(usize, usize) -> Result<Vec<u8>, String>,
) -> Result<Snapshot, String> {
    read_cached(base, pawn, &mut Cache::default(), read)
}
/// Reuse static definitions and monotonic character completions within one native owner.
/// No cache changes are published until every dynamic and cold-source proof rechecks.
pub fn read_cached(
    base: usize,
    pawn: usize,
    cache: &mut Cache,
    read: impl FnMut(usize, usize) -> Result<Vec<u8>, String>,
) -> Result<Snapshot, String> {
    pointer(base)?;
    pointer(pawn)?;
    let mut reader = Reader {
        read,
        watched: BTreeMap::new(),
        definition_proofs: Vec::new(),
        nodes: 0,
    };
    let manager = reader.ptr(add(base, 0x2134130)?)?;
    let entity = reader.ptr(add(manager, 0x94)?)?;
    if reader.ptr(add(pawn, 0x5c)?)? != entity {
        return Err("Feat owner does not match controlled pawn".into());
    }
    let entity_id = u64::from_le_bytes(
        reader
            .bytes(add(pawn, 0x68)?, 8)?
            .try_into()
            .map_err(|_| "Short feat entity ID")?,
    );
    let component = reader.component(entity)?;
    let binding = Binding {
        base,
        manager,
        entity,
        entity_id,
        component,
    };
    let mut staged = cache
        .state
        .as_ref()
        .filter(|state| state.binding == binding)
        .cloned()
        .unwrap_or_else(|| CacheState::new(binding));
    let trees = reader.bytes(add(component, 0x5c)?, 16)?;
    let active = reader.row_signature(pointer(q(&trees, 0)?)?)?;
    let completed = reader.row_signature(pointer(q(&trees, 8)?)?)?;
    if staged
        .completed_signature
        .as_ref()
        .is_some_and(|old| completed.count < old.count)
    {
        return Err("Completed feat count decreased; owner context must be re-established".into());
    }
    let mut rows = BTreeMap::new();
    reader.rows(&active, &mut rows)?;
    if staged.completed_signature.as_ref() != Some(&completed) {
        let mut refreshed = BTreeMap::new();
        reader.rows(&completed, &mut refreshed)?;
        staged.completed_rows = refreshed;
        staged.completed_signature = Some(completed);
    }
    for (&id, &status) in &staged.completed_rows {
        if rows.insert(id, status).is_some() {
            return Err("Duplicate active/completed feat ID".into());
        }
    }
    for (&id, &(done, eligible)) in &rows {
        if done {
            staged.proven_completed.insert(id, eligible);
        }
    }
    for (&id, &eligible) in &staged.proven_completed {
        rows.entry(id)
            .and_modify(|status| status.0 = true)
            .or_insert((true, eligible));
    }
    if rows.len() > MAX_ROWS {
        return Err("Cached feat row bound exceeded".into());
    }
    if !rows.is_empty() {
        let source = add(manager, 0x217c)?;
        let token = add(base, 0x2153450)?;
        let signature = DefinitionSignature {
            root: q(&reader.bytes(add(source, 0x50)?, 8)?, 0)?,
            // Exact October 20 build: archive vtable revalidated on October 22.
            archive: reader.archive_signature(source, token, add(base, 0x1d94a78)?)?,
        };
        if staged.signature.as_ref() != Some(&signature)
            || rows.keys().any(|id| !staged.covered.contains(id))
        {
            let definitions = reader.definitions(source, token)?;
            let archive = reader.archive(&signature.archive)?;
            let mut offsets: Vec<_> = definitions
                .values()
                .filter(|&&n| n != u32::MAX)
                .map(|&n| n as usize)
                .collect();
            offsets.sort_unstable();
            offsets.dedup();
            if offsets.iter().any(|&offset| offset >= archive.len()) {
                return Err("Feat definition offset outside archive".into());
            }
            let big_endian = signature.archive.flags[0] != 0 && signature.archive.flags[2] == 0;
            let mut decoded = BTreeMap::new();
            for (&id, &offset) in &definitions {
                if offset == u32::MAX {
                    continue;
                }
                let offset = offset as usize;
                let end = offsets
                    .get(offsets.partition_point(|&next| next <= offset))
                    .copied()
                    .unwrap_or(archive.len());
                if let Ok(definition) = feat_memory_codec::decode(&archive[offset..end], big_endian)
                {
                    if definition.id != id {
                        return Err("Feat serialized ID differs from lookup key".into());
                    }
                    decoded.insert(id, Arc::new(definition));
                }
            }
            staged.definitions = decoded;
            staged.covered = definitions
                .keys()
                .copied()
                .chain(rows.keys().copied())
                .collect();
            staged.signature = Some(signature);
        }
    }
    let entries = rows
        .iter()
        .filter_map(|(&id, &(completed, eligible))| {
            staged.definitions.get(&id).map(|definition| Entry {
                id,
                title: definition.title.clone(),
                description: definition.description.clone(),
                points: definition.points,
                completed,
                eligible,
            })
        })
        .collect();
    let observed_at = Instant::now();
    reader.recheck()?;
    cache.state = Some(staged);
    Ok(Snapshot {
        entity,
        entity_id,
        entries,
        observed_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    const BASE: usize = 0x1000_0000;
    const PAWN: usize = 0x2000_0000;
    const MANAGER: usize = 0x3000_0000;
    const ENTITY: usize = 0x3100_0000;
    const DATA: usize = 0x3200_0000;
    const COMPONENT_NODES: usize = 0x3300_0000;
    const COMPONENT: usize = 0x3400_0000;
    const ACTIVE: usize = 0x3500_0000;
    const COMPLETED: usize = 0x3600_0000;
    const ROW_NODES: usize = 0x3700_0000;
    const ROW: usize = 0x3800_0000;
    const DEFINITION: usize = 0x3900_0000;
    const TYPE: usize = 0x4000_0000;
    const ARCHIVE_DATA: usize = 0x4100_0000;
    #[derive(Default, Clone)]
    struct Memory(HashMap<usize, u8>);
    impl Memory {
        fn put(&mut self, address: usize, bytes: &[u8]) {
            for (offset, &byte) in bytes.iter().enumerate() {
                self.0.insert(address + offset, byte);
            }
        }
        fn d(&mut self, address: usize, value: u32) {
            self.put(address, &value.to_le_bytes());
        }
        fn q(&mut self, address: usize, value: usize) {
            self.put(address, &(value as u64).to_le_bytes());
        }
        fn read(&self, address: usize, len: usize) -> Result<Vec<u8>, String> {
            assert!((1..=4096).contains(&len));
            Ok((address..address + len)
                .map(|a| self.0.get(&a).copied().unwrap_or(0))
                .collect())
        }
    }
    fn fixture() -> Memory {
        let mut memory = Memory::default();
        memory.q(BASE + 0x2134130, MANAGER);
        memory.q(MANAGER + 0x94, ENTITY);
        memory.q(PAWN + 0x5c, ENTITY);
        memory.q(PAWN + 0x68, 77);
        memory.q(ENTITY + 8, DATA);
        memory.q(DATA + 0x90, COMPONENT_NODES);
        memory.d(DATA + 0x98, 1);
        memory.d(COMPONENT_NODES + 28 + 16, 0x34);
        memory.q(COMPONENT_NODES + 28 + 20, COMPONENT);
        memory.q(COMPONENT + 0x5c, ACTIVE);
        memory.q(COMPONENT + 0x64, COMPLETED);
        memory.d(ACTIVE + 0x1c, 2);
        memory.q(ACTIVE + 0x24, ROW_NODES);
        memory.d(ACTIVE + 0x2c, 1);
        memory.d(COMPLETED + 0x1c, 1);
        memory.q(ROW_NODES + 32 + 24, ROW);
        memory.d(ROW + 0x10, 123);
        memory.d(ROW + 0x18, 1);
        memory.put(ROW + 0x1d, &[1]);
        memory.q(MANAGER + 0x217c + 0x50, DEFINITION);
        memory.q(DEFINITION + 8, BASE + 0x2153450);
        memory.d(DEFINITION + 0x50, 123);
        memory.d(DEFINITION + 0x78, 0);
        memory.q(MANAGER + 0x217c + 0x70, TYPE);
        memory.q(TYPE + 0x114, BASE + 0x2153450);
        let archive = TYPE + 8;
        let block = archive + 8;
        memory.q(archive, BASE + 0x1d94a78);
        let mut prefix = Vec::new();
        for value in [123u32, 12, u32::MAX, 0] {
            prefix.extend(value.to_le_bytes());
        }
        for text in ["Synthetic feat", "Synthetic goal"] {
            prefix.extend([0, 1, 0, 0x10 | text.len() as u8]);
            prefix.extend(text.as_bytes());
            prefix.push(0);
        }
        prefix.extend([0, 0]);
        prefix.extend(25u32.to_le_bytes());
        memory.d(archive + 0xd0, prefix.len() as u32);
        memory.d(block + 8, prefix.len() as u32 - 1);
        memory.d(block + 0x10, prefix.len() as u32);
        memory.q(block + 0x9c, ARCHIVE_DATA);
        memory.put(ARCHIVE_DATA, &prefix);
        memory
    }
    fn two_feats() -> Memory {
        let mut memory = fixture();
        memory.d(ROW + 0x18, 0);
        let completed_nodes = ROW_NODES + 0x1000;
        let completed_row = ROW + 0x1000;
        memory.d(COMPLETED + 0x1c, 2);
        memory.q(COMPLETED + 0x24, completed_nodes);
        memory.d(COMPLETED + 0x2c, 1);
        memory.q(completed_nodes + 32 + 24, completed_row);
        memory.d(completed_row + 0x10, 124);
        memory.d(completed_row + 0x18, 1);
        memory.put(completed_row + 0x1d, &[1]);
        let len = d(&memory.read(TYPE + 8 + 0xd0, 4).unwrap(), 0) as usize;
        let mut prefix = memory.read(ARCHIVE_DATA, len).unwrap();
        prefix[..4].copy_from_slice(&124u32.to_le_bytes());
        memory.put(ARCHIVE_DATA + len, &prefix);
        memory.q(DEFINITION + 0x68, DEFINITION + 0x100);
        memory.q(DEFINITION + 0x100 + 8, BASE + 0x2153450);
        memory.d(DEFINITION + 0x100 + 0x50, 124);
        memory.d(DEFINITION + 0x100 + 0x78, len as u32);
        memory.d(TYPE + 8 + 0xd0, (len * 2) as u32);
        memory.d(TYPE + 16 + 8, (len * 2 - 1) as u32);
        memory.d(TYPE + 16 + 0x10, (len * 2) as u32);
        memory
    }
    #[test]
    fn hot_cache_skips_static_sources_and_completed_rows_but_reads_active_rows() {
        let mut memory = two_feats();
        let mut cache = Cache::default();
        read_cached(BASE, PAWN, &mut cache, |a, n| memory.read(a, n)).unwrap();
        memory.put(ROW + 0x1c, &[1]);
        let mut active_reads = 0;
        let snapshot = read_cached(BASE, PAWN, &mut cache, |a, n| {
            assert!(a != DEFINITION && a != DEFINITION + 0x100);
            assert!(!(ARCHIVE_DATA..ARCHIVE_DATA + 4096).contains(&a));
            assert!(a != ROW_NODES + 0x1000 + 32 && a != ROW + 0x1000 + 0x10);
            if a == ROW + 0x10 {
                active_reads += 1;
            }
            memory.read(a, n)
        })
        .unwrap();
        assert_eq!(active_reads, 2);
        assert!(
            !snapshot
                .entries
                .iter()
                .find(|entry| entry.id == 123)
                .unwrap()
                .eligible
        );
        assert!(
            snapshot
                .entries
                .iter()
                .find(|entry| entry.id == 124)
                .unwrap()
                .completed
        );
    }
    #[test]
    fn new_completions_union_without_downgrade_and_count_decrease_rejects() {
        let mut memory = two_feats();
        let mut cache = Cache::default();
        read_cached(BASE, PAWN, &mut cache, |a, n| memory.read(a, n)).unwrap();
        memory.d(ACTIVE + 0x1c, 1);
        memory.d(ACTIVE + 0x2c, 0);
        memory.d(COMPLETED + 0x1c, 3);
        memory.d(ROW_NODES + 0x1000 + 32 + 8, 2);
        memory.q(ROW_NODES + 0x1000 + 64 + 24, ROW);
        memory.d(ROW + 0x18, 1);
        memory.d(ROW + 0x1000 + 0x18, 0);
        memory.put(ROW + 0x1000 + 0x44, &[1]);
        let snapshot = read_cached(BASE, PAWN, &mut cache, |a, n| memory.read(a, n)).unwrap();
        assert_eq!(snapshot.entries.len(), 2);
        assert!(snapshot.entries.iter().all(|entry| entry.completed));
        memory.d(COMPLETED + 0x1c, 2);
        assert!(read_cached(BASE, PAWN, &mut cache, |a, n| memory.read(a, n)).is_err());
        assert_eq!(
            cache
                .state
                .as_ref()
                .unwrap()
                .completed_signature
                .as_ref()
                .unwrap()
                .count,
            3
        );
    }
    #[test]
    fn unsupported_archive_type_does_not_replace_cached_definitions() {
        let mut memory = fixture();
        let mut cache = Cache::default();
        read_cached(BASE, PAWN, &mut cache, |a, n| memory.read(a, n)).unwrap();
        let previous = cache.state.as_ref().unwrap().definitions[&123].clone();
        memory.q(TYPE + 8, BASE + 0x1d94a68);
        assert_eq!(
            read_cached(BASE, PAWN, &mut cache, |a, n| memory.read(a, n)).unwrap_err(),
            "Unsupported feat archive type"
        );
        assert_eq!(cache.state.as_ref().unwrap().definitions[&123], previous);
        let mut cold = Cache::default();
        assert!(read_cached(BASE, PAWN, &mut cold, |a, n| memory.read(a, n)).is_err());
        assert!(cold.state.is_none());
    }
    #[test]
    fn owner_and_archive_signature_changes_refresh_static_definitions() {
        let mut memory = fixture();
        let mut cache = Cache::default();
        read_cached(BASE, PAWN, &mut cache, |a, n| memory.read(a, n)).unwrap();
        memory.q(PAWN + 0x68, 88);
        memory.d(ROW + 0x18, 0);
        let mut definition_reads = 0;
        let snapshot = read_cached(BASE, PAWN, &mut cache, |a, n| {
            if a == DEFINITION {
                definition_reads += 1;
            }
            memory.read(a, n)
        })
        .unwrap();
        assert_eq!(definition_reads, 2);
        assert_eq!(snapshot.entity_id, 88);
        assert!(!snapshot.entries[0].completed);
        let len = d(&memory.read(TYPE + 8 + 0xd0, 4).unwrap(), 0) as usize;
        let mut prefix = memory.read(ARCHIVE_DATA, len).unwrap();
        prefix[20] = b'X';
        memory.put(ARCHIVE_DATA + 0x1000, &prefix);
        memory.q(TYPE + 16 + 0x9c, ARCHIVE_DATA + 0x1000);
        let snapshot = read_cached(BASE, PAWN, &mut cache, |a, n| memory.read(a, n)).unwrap();
        assert_eq!(snapshot.entries[0].title, "Xynthetic feat");
    }
    #[test]
    fn unsupported_and_absent_ids_are_covered_without_repeated_cold_reads() {
        let mut memory = fixture();
        let mut cache = Cache::default();
        memory.put(ARCHIVE_DATA + 19, &[0xff]);
        assert!(
            read_cached(BASE, PAWN, &mut cache, |a, n| memory.read(a, n))
                .unwrap()
                .entries
                .is_empty()
        );
        read_cached(BASE, PAWN, &mut cache, |a, n| {
            assert!(a != DEFINITION && a != ARCHIVE_DATA);
            memory.read(a, n)
        })
        .unwrap();
        memory.d(ROW + 0x10, 999);
        let mut definition_reads = 0;
        read_cached(BASE, PAWN, &mut cache, |a, n| {
            if a == DEFINITION {
                definition_reads += 1;
            }
            memory.read(a, n)
        })
        .unwrap();
        assert_eq!(definition_reads, 2);
        read_cached(BASE, PAWN, &mut cache, |a, n| {
            assert!(a != DEFINITION && a != ARCHIVE_DATA);
            memory.read(a, n)
        })
        .unwrap();
    }
    #[test]
    fn failed_hot_and_cold_refreshes_never_publish_staged_cache_state() {
        let memory = fixture();
        let mut cache = Cache::default();
        assert!(
            read_cached(BASE, PAWN, &mut cache, |a, n| {
                if a == ARCHIVE_DATA {
                    return Err("Transient".into());
                }
                memory.read(a, n)
            })
            .is_err()
        );
        assert!(cache.state.is_none());
        read_cached(BASE, PAWN, &mut cache, |a, n| memory.read(a, n)).unwrap();
        let old = cache.state.as_ref().unwrap().definitions[&123].clone();
        let mut changed = memory.clone();
        changed.q(PAWN + 0x68, 88);
        assert!(
            read_cached(BASE, PAWN, &mut cache, |a, n| {
                if a == ARCHIVE_DATA {
                    return Err("Transient".into());
                }
                changed.read(a, n)
            })
            .is_err()
        );
        assert_eq!(cache.state.as_ref().unwrap().binding.entity_id, 77);
        assert!(Arc::ptr_eq(
            &old,
            &cache.state.as_ref().unwrap().definitions[&123]
        ));
        let mut reads = 0;
        assert!(
            read_cached(BASE, PAWN, &mut cache, |a, n| {
                let mut bytes = memory.read(a, n)?;
                if a == ROW + 0x10 {
                    reads += 1;
                    if reads > 1 {
                        bytes[8] ^= 1;
                    }
                }
                Ok(bytes)
            })
            .is_err()
        );
        assert!(Arc::ptr_eq(
            &old,
            &cache.state.as_ref().unwrap().definitions[&123]
        ));
    }
    #[test]
    fn failed_new_completion_does_not_poison_monotonic_facts() {
        let mut memory = fixture();
        memory.d(ROW + 0x18, 0);
        let mut cache = Cache::default();
        read_cached(BASE, PAWN, &mut cache, |a, n| memory.read(a, n)).unwrap();
        let mut completed = memory.clone();
        completed.d(ROW + 0x18, 1);
        let mut reads = 0;
        assert!(
            read_cached(BASE, PAWN, &mut cache, |a, n| {
                if a == ROW + 0x10 {
                    reads += 1;
                    if reads == 2 {
                        return Err("Transient status recheck".into());
                    }
                }
                completed.read(a, n)
            })
            .is_err()
        );
        assert!(cache.state.as_ref().unwrap().proven_completed.is_empty());
        let snapshot = read_cached(BASE, PAWN, &mut cache, |a, n| memory.read(a, n)).unwrap();
        assert!(!snapshot.entries[0].completed);
        memory.d(ROW + 0x18, 1);
        read_cached(BASE, PAWN, &mut cache, |a, n| memory.read(a, n)).unwrap();
        memory.d(ROW + 0x18, 0);
        memory.put(ROW + 0x44, &[1]);
        let snapshot = read_cached(BASE, PAWN, &mut cache, |a, n| memory.read(a, n)).unwrap();
        assert!(snapshot.entries[0].completed);
    }
    #[test]
    fn matched_native_snapshot_keeps_character_completion_and_eligibility() {
        let memory = fixture();
        let snapshot = read(BASE, PAWN, |a, n| memory.read(a, n)).unwrap();
        assert_eq!((snapshot.entity, snapshot.entity_id), (ENTITY, 77));
        assert_eq!(snapshot.entries.len(), 1);
        let entry = &snapshot.entries[0];
        assert_eq!((entry.id, entry.points), (123, 25));
        assert_eq!(entry.title, "Synthetic feat");
        assert_eq!(entry.description, "Synthetic goal");
        assert!(entry.completed && entry.eligible);
    }
    #[test]
    fn account_completion_is_ignored_and_eligibility_requires_both_flags() {
        for (blocked, available, eligible) in [(0, 1, true), (1, 1, false), (0, 0, false)] {
            let mut memory = fixture();
            memory.d(ROW + 0x18, 0);
            memory.put(ROW + 0x44, &[1]);
            memory.put(ROW + 0x1c, &[blocked, available]);
            let snapshot = read(BASE, PAWN, |a, n| memory.read(a, n)).unwrap();
            assert!(!snapshot.entries[0].completed);
            assert_eq!(snapshot.entries[0].eligible, eligible);
        }
        let mut memory = fixture();
        memory.q(TYPE + 8, BASE + 0x1d94a70);
        assert!(read(BASE, PAWN, |a, n| memory.read(a, n)).is_err());
    }
    #[test]
    fn definition_recheck_masks_mutable_cache_fields_but_preserves_identity() {
        for offset in [0, 0x10, 0x48, 0x54, 0x70, 0x7c, 8, 0x50, 0x60, 0x68, 0x78] {
            let memory = fixture();
            let mut reads = 0;
            let result = read(BASE, PAWN, |address, len| {
                let mut bytes = memory.read(address, len)?;
                if address == DEFINITION {
                    assert_eq!(len, 0x80);
                    reads += 1;
                    if reads == 2 {
                        bytes[offset] ^= 1;
                    }
                }
                Ok(bytes)
            });
            assert_eq!(reads, 2);
            assert_eq!(
                result.is_ok(),
                [0, 0x10, 0x48, 0x54, 0x70, 0x7c].contains(&offset),
                "offset {offset:x}"
            );
        }
    }
    #[test]
    fn rejects_owner_changes_and_short_reads() {
        let memory = fixture();
        let mut owner_reads = 0;
        assert!(
            read(BASE, PAWN, |a, n| {
                if a == PAWN + 0x5c {
                    owner_reads += 1;
                    if owner_reads > 1 {
                        return Ok((ENTITY + 8).to_le_bytes().to_vec());
                    }
                }
                memory.read(a, n)
            })
            .is_err()
        );
        assert!(
            read(BASE, PAWN, |a, n| {
                let mut bytes = memory.read(a, n)?;
                bytes.pop();
                Ok(bytes)
            })
            .is_err()
        );
    }
    #[test]
    fn rejects_cycles_counts_duplicates_and_archive_corruption() {
        for case in 0..6 {
            let mut memory = fixture();
            match case {
                0 => memory.d(ROW_NODES + 32 + 4, 1),
                1 => memory.d(ACTIVE + 0x1c, 3),
                2 => memory.q(DEFINITION + 0x60, DEFINITION),
                3 => memory.q(TYPE + 8 + 8 + 0xac, TYPE + 8 + 8),
                4 => memory.d(TYPE + 8 + 0xd0, MAX_ARCHIVE as u32 + 1),
                _ => {
                    memory.d(COMPLETED + 0x1c, 2);
                    memory.q(COMPLETED + 0x24, ROW_NODES);
                    memory.d(COMPLETED + 0x2c, 1);
                }
            }
            assert!(
                read(BASE, PAWN, |a, n| memory.read(a, n)).is_err(),
                "case {case}"
            );
        }
    }
    #[test]
    fn rejects_status_archive_and_entity_identifier_changes_but_ignores_cursors() {
        for changed in [ROW + 0x10, ARCHIVE_DATA, PAWN + 0x68] {
            let memory = fixture();
            let mut reads = 0;
            assert!(
                read(BASE, PAWN, |a, n| {
                    let mut bytes = memory.read(a, n)?;
                    if a == changed {
                        reads += 1;
                        if reads > 1 {
                            bytes[0] ^= 1;
                        }
                    }
                    Ok(bytes)
                })
                .is_err()
            );
        }
        let memory = fixture();
        read(BASE, PAWN, |a, n| {
            assert!(a != TYPE + 8 + 8 + 0xc && a != TYPE + 8 + 0xc0 && a != TYPE + 8 + 0xcc);
            memory.read(a, n)
        })
        .unwrap();
    }
    #[test]
    fn unknown_definitions_never_become_incomplete_and_id_mismatch_is_global_error() {
        let mut memory = fixture();
        memory.d(DEFINITION + 0x78, u32::MAX);
        assert!(
            read(BASE, PAWN, |a, n| memory.read(a, n))
                .unwrap()
                .entries
                .is_empty()
        );
        let mut memory = fixture();
        memory.put(ARCHIVE_DATA + 16 + 3, &[0xff]);
        assert!(
            read(BASE, PAWN, |a, n| memory.read(a, n))
                .unwrap()
                .entries
                .is_empty()
        );
        let mut memory = fixture();
        memory.d(ARCHIVE_DATA, 124);
        assert!(read(BASE, PAWN, |a, n| memory.read(a, n)).is_err());
    }
}
