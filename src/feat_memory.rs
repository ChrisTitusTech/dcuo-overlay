//! Pure read-only snapshot of experimentally proven native feat structures.
//! Callers establish build/process identity and impose their own read/time budget.
use crate::feat_memory_codec;
use std::{
    collections::{BTreeMap, HashSet},
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
    fn rows(&mut self, tree: usize, rows: &mut BTreeMap<u32, (bool, bool)>) -> Result<(), String> {
        let count = d(&self.bytes(add(tree, 0x1c)?, 4)?, 0) as usize;
        if count == 0 || count > MAX_ROWS + 1 {
            return Err("Invalid feat row count".into());
        }
        let header = self.bytes(add(tree, 0x24)?, 12)?;
        let nodes = q(&header, 0)?;
        let mut todo = vec![d(&header, 8)];
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
    fn archive(
        &mut self,
        cache: usize,
        token: usize,
        expected_vtable: usize,
    ) -> Result<(Vec<u8>, bool), String> {
        let mut node = q(&self.bytes(add(cache, 0x70)?, 8)?, 0)?;
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
        let flags = self.bytes(add(archive, 0xd4)?, 3)?;
        let big_endian = flags[0] != 0 && flags[2] == 0;
        let mut block = add(archive, 8)?;
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
        Ok((bytes, big_endian))
    }
}
/// Read only through `read`; unsupported individual serialized prefixes stay absent.
/// Absence is never converted into a completion or incompletion observation.
pub fn read(
    base: usize,
    pawn: usize,
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
    let trees = reader.bytes(add(component, 0x5c)?, 16)?;
    let mut rows = BTreeMap::new();
    reader.rows(pointer(q(&trees, 0)?)?, &mut rows)?;
    reader.rows(pointer(q(&trees, 8)?)?, &mut rows)?;
    let mut entries = Vec::new();
    if !rows.is_empty() {
        let cache = add(manager, 0x217c)?;
        let token = add(base, 0x2153450)?;
        let definitions = reader.definitions(cache, token)?;
        let selected: Vec<_> = rows
            .iter()
            .filter_map(|(&id, &status)| {
                definitions
                    .get(&id)
                    .filter(|&&offset| offset != u32::MAX)
                    .map(|&offset| (id, status, offset as usize))
            })
            .collect();
        if !selected.is_empty() {
            let (archive, big_endian) = reader.archive(cache, token, add(base, 0x1d94a68)?)?;
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
            for (id, (completed, eligible), offset) in selected {
                let end = offsets
                    .get(offsets.partition_point(|&next| next <= offset))
                    .copied()
                    .unwrap_or(archive.len());
                if let Ok(definition) = feat_memory_codec::decode(&archive[offset..end], big_endian)
                {
                    if definition.id != id {
                        return Err("Feat serialized ID differs from lookup key".into());
                    }
                    entries.push(Entry {
                        id,
                        title: definition.title,
                        description: definition.description,
                        points: definition.points,
                        completed,
                        eligible,
                    });
                }
            }
        }
    }
    let observed_at = Instant::now();
    reader.recheck()?;
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
    #[derive(Default)]
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
        memory.q(archive, BASE + 0x1d94a68);
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
