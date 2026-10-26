//! Bounded observations of the current entity's equipped native item instances.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EquippedStat {
    pub name: String,
    pub value: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EquippedItem {
    pub slot: String,
    #[serde(serialize_with = "serialize_id")]
    pub item_id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_level: Option<u32>,
    pub effects: Vec<String>,
    pub base_stats: Vec<EquippedStat>,
}

pub fn capture(
    base: usize,
    requested_ids: &[u32],
    read: impl FnMut(usize, usize) -> Result<Vec<u8>, String>,
) -> Result<Vec<EquippedItem>, String> {
    if requested_ids.len() > 50
        || requested_ids.contains(&0)
        || requested_ids.iter().collect::<HashSet<_>>().len() != requested_ids.len()
    {
        return Err("Invalid equipped item request".into());
    }
    let mut r = Reader {
        read,
        proofs: BTreeMap::new(),
        calls: 0,
        bytes: 0,
    };
    let manager = r.ptr(at(base, 0x2134130)?)?;
    let entity = r.ptr(at(manager, 0x94)?)?;
    let data = r.ptr(at(entity, 8)?)?;
    let component_data = r.ptr(at(data, 0x90)?)?;
    let root = r.u32(at(data, 0x98)?)?;
    let Some(component) = r.lookup(component_data, root, 0x1c, 0x16, false)? else {
        r.verify()?;
        return Ok(Vec::new());
    };
    let array_owner = r.ptr(at(component, 0xac)?)?;
    let count_bytes = r.watch(at(array_owner, 0x14)?, 2)?;
    let count = u16::from_le_bytes([count_bytes[0], count_bytes[1]]) as usize;
    if count > 64 {
        return Err("Equipped slot count exceeds bound".into());
    }
    if count == 0 {
        r.verify()?;
        return Ok(Vec::new());
    }
    let slots = r.ptr(at(array_owner, 0x1c)?)?;
    let overrides = r.u64(at(component, 0xe4)?)? as usize;
    let override_header = if overrides != 0 {
        ptr(overrides)?;
        let root = r.u32(at(overrides, 0x2c)?)?;
        if root == 0 {
            None
        } else {
            Some((r.ptr(at(overrides, 0x24)?)?, root))
        }
    } else {
        None
    };
    let mut items = Vec::new();
    for slot in 0..count {
        let base_entry = r.u64(at(slots, slot * 8)?)? as usize;
        let entry = match override_header {
            Some((nodes, root)) => r
                .lookup(nodes, root, 0x20, slot as u64, true)?
                .unwrap_or(base_entry),
            None => base_entry,
        };
        if entry == 0 {
            continue;
        }
        let instance = at(entry, 0x10)?;
        let item_id = r.u32(at(instance, 0x298)?)?;
        if !requested_ids.contains(&item_id) {
            continue;
        }
        let show_level = r.watch(at(instance, 0x2b5)?, 1)?[0];
        let item_level = if show_level != 0 {
            Some(r.u32(at(instance, 0x64)?)?)
        } else {
            None
        };
        let mut effects = Vec::new();
        let header = r.watch(at(instance, 0x74)?, 16)?;
        let n = u32_at(&header, 0) as usize;
        let capacity = u32_at(&header, 4) as usize;
        if n > 64 || n > capacity || capacity > 256 {
            return Err("Equipped effect vector exceeds bound".into());
        }
        if n != 0 {
            let records = ptr(u64_at(&header, 8) as usize)?;
            for index in 0..n {
                let record = at(records, index * 0x194)?;
                // Native formatter appends these two existing strings. They are effects,
                // not a numeric stat schema or a claim of complete tooltip reproduction.
                for offset in [0x1c, 0x70] {
                    if let Some(text) = r.text(at(record, offset)?)?
                        && !effects.contains(&text)
                    {
                        effects.push(text);
                    }
                }
            }
        }
        let base_stats = r.base_stats(manager, instance)?;
        items.push(EquippedItem {
            slot: slot.to_string(),
            item_id,
            item_level,
            effects,
            base_stats,
        });
    }
    r.verify()?;
    Ok(items)
}
fn at(address: usize, offset: usize) -> Result<usize, String> {
    let value = address
        .checked_add(offset)
        .ok_or("Equipped address overflow")?;
    ptr(value)
}
fn ptr(address: usize) -> Result<usize, String> {
    if !(0x10000..=0x0000_7fff_ffff_ffff).contains(&address) {
        return Err("Invalid equipped pointer".into());
    }
    Ok(address)
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().expect("bounded field"))
}
fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().expect("bounded field"))
}
struct Reader<F> {
    read: F,
    proofs: BTreeMap<(usize, usize), Vec<u8>>,
    calls: usize,
    bytes: usize,
}
impl<F: FnMut(usize, usize) -> Result<Vec<u8>, String>> Reader<F> {
    fn bytes(&mut self, address: usize, len: usize) -> Result<Vec<u8>, String> {
        if len == 0 || len > 512 {
            return Err("Equipped read exceeds bound".into());
        }
        ptr(address)?;
        at(address, len - 1)?;
        self.calls += 1;
        self.bytes += len;
        if self.calls > 20_000 || self.bytes > 4 * 1024 * 1024 {
            return Err("Equipped capture exceeds budget".into());
        }
        let b = (self.read)(address, len)?;
        if b.len() != len {
            return Err("Short equipped read".into());
        }
        Ok(b)
    }
    fn watch(&mut self, a: usize, n: usize) -> Result<Vec<u8>, String> {
        let bytes = self.bytes(a, n)?;
        if let Some(before) = self.proofs.get(&(a, n)) {
            if before != &bytes {
                return Err("Equipped source changed".into());
            }
        } else {
            self.proofs.insert((a, n), bytes.clone());
        }
        Ok(bytes)
    }
    fn u32(&mut self, a: usize) -> Result<u32, String> {
        Ok(u32_at(&self.watch(a, 4)?, 0))
    }
    fn u64(&mut self, a: usize) -> Result<u64, String> {
        Ok(u64_at(&self.watch(a, 8)?, 0))
    }
    fn ptr(&mut self, a: usize) -> Result<usize, String> {
        ptr(self.u64(a)? as usize)
    }
    fn node(
        &mut self,
        data: usize,
        mut index: u32,
        stride: usize,
        key: u64,
        wide: bool,
    ) -> Result<Option<Vec<u8>>, String> {
        let mut seen = HashSet::new();
        for _ in 0..64 {
            if index == 0 {
                return Ok(None);
            }
            if index > 4096 || !seen.insert(index) {
                return Err("Invalid equipped tree index".into());
            }
            let node = self.watch(at(data, index as usize * stride)?, stride)?;
            let found = if wide {
                u64_at(&node, 0x10)
            } else {
                u32_at(&node, 0x10) as u64
            };
            if found == key {
                return Ok(Some(node));
            }
            index = u32_at(&node, if key < found { 4 } else { 8 });
        }
        Err("Equipped tree traversal exceeds bound".into())
    }
    fn lookup(
        &mut self,
        data: usize,
        root: u32,
        stride: usize,
        key: u64,
        wide: bool,
    ) -> Result<Option<usize>, String> {
        let node = self.node(data, root, stride, key, wide)?;
        node.map(|b| {
            let value = u64_at(&b, if wide { 0x18 } else { 0x14 }) as usize;
            if value != 0 {
                ptr(value)?;
            }
            Ok(value)
        })
        .transpose()
    }
    fn base_stats(&mut self, manager: usize, instance: usize) -> Result<Vec<EquippedStat>, String> {
        let root = self.u32(at(instance, 0x168)?)?;
        if root == 0 {
            return Ok(Vec::new());
        }
        let data = self.ptr(at(instance, 0x160)?)?;
        let label_root = self.u32(at(manager, 0x6e38)?)?;
        if label_root == 0 {
            return Ok(Vec::new());
        }
        let labels = self.ptr(at(manager, 0x6e30)?)?;
        let mut stats = Vec::new();
        // Exact native tooltip stat order, image RVA1b9cec0 (formatter efb860).
        // Keep base values distinct from separately displayed effect bonuses.
        for id in [59, 3, 48, 0, 1, 44, 4, 51, 9, 10, 52, 2, 5] {
            let Some(node) = self.node(data, root, 0x1c, id, false)? else {
                continue;
            };
            let value = u32_at(&node, 0x14);
            if value == 0 {
                continue;
            }
            let Some(label) = self.node(labels, label_root, 0x68, id, false)? else {
                continue;
            };
            let label_holder = &label[0x24..0x34];
            if let Some(name) = self.text_header(label_holder)? {
                stats.push(EquippedStat { name, value });
            }
        }
        Ok(stats)
    }
    fn text(&mut self, a: usize) -> Result<Option<String>, String> {
        let header = self.watch(a, 16)?;
        self.text_header(&header)
    }
    fn text_header(&mut self, header: &[u8]) -> Result<Option<String>, String> {
        let n = u32_at(header, 8) as usize;
        let capacity = u32_at(header, 12) as usize;
        if n == 0 {
            return Ok(None);
        }
        if n > 512 || n > capacity || capacity > 4096 {
            return Err("Invalid equipped effect string".into());
        }
        let bytes = self.watch(ptr(u64_at(header, 0) as usize)?, n)?;
        let text = String::from_utf8(bytes).map_err(|_| "Invalid equipped text encoding")?;
        if text.trim().is_empty() || text.chars().any(char::is_control) {
            return Err("Invalid equipped effect text".into());
        }
        Ok(Some(text))
    }
    fn verify(&mut self) -> Result<(), String> {
        for ((a, n), before) in std::mem::take(&mut self.proofs) {
            if self.bytes(a, n)? != before {
                return Err("Equipped source changed; capture discarded".into());
            }
        }
        Ok(())
    }
}

fn serialize_id<S: serde::Serializer>(id: &u32, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    const BASE: usize = 0x10000000;
    const MANAGER: usize = 0x20000000;
    const ENTITY: usize = 0x21000000;
    const DATA: usize = 0x22000000;
    const NODES: usize = 0x23000000;
    const COMPONENT: usize = 0x24000000;
    const ARRAY: usize = 0x25000000;
    const SLOTS: usize = 0x26000000;
    const ENTRY: usize = 0x27000000;
    const OVERRIDE: usize = 0x28000000;
    const TREE: usize = 0x29000000;
    const OV_NODES: usize = 0x30000000;
    const EFFECT: usize = 0x31000000;
    const TEXT: usize = 0x32000000;
    #[derive(Default)]
    struct Memory(HashMap<usize, u8>);
    impl Memory {
        fn put(&mut self, a: usize, b: &[u8]) {
            for (i, v) in b.iter().enumerate() {
                self.0.insert(a + i, *v);
            }
        }
        fn u64(&mut self, a: usize, v: u64) {
            self.put(a, &v.to_le_bytes());
        }
        fn u32(&mut self, a: usize, v: u32) {
            self.put(a, &v.to_le_bytes());
        }
        fn read(&self, a: usize, n: usize) -> Result<Vec<u8>, String> {
            Ok((a..a + n).map(|i| *self.0.get(&i).unwrap_or(&0)).collect())
        }
    }
    fn fixture() -> Memory {
        let mut m = Memory::default();
        for (a, v) in [
            (BASE + 0x2134130, MANAGER),
            (MANAGER + 0x94, ENTITY),
            (ENTITY + 8, DATA),
            (DATA + 0x90, NODES),
            (NODES + 0x1c + 0x14, COMPONENT),
            (COMPONENT + 0xac, ARRAY),
            (ARRAY + 0x1c, SLOTS),
            (SLOTS, ENTRY),
            (SLOTS + 8, ENTRY + 0x1000),
        ] {
            m.u64(a, v as u64);
        }
        m.u32(DATA + 0x98, 1);
        m.u32(NODES + 0x1c + 0x10, 0x16);
        m.put(ARRAY + 0x14, &2u16.to_le_bytes());
        for (entry, level) in [(ENTRY, 381), (ENTRY + 0x1000, 407)] {
            m.u32(entry + 0x10 + 0x298, 123);
            m.put(entry + 0x10 + 0x2b5, &[1]);
            m.u32(entry + 0x10 + 0x64, level);
        }
        m
    }
    #[test]
    fn distinct_slots_preserve_same_definition_different_levels_and_overrides() {
        let mut m = fixture();
        let items = capture(BASE, &[123], |a, n| m.read(a, n)).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].item_level, Some(381));
        assert_eq!(items[1].item_level, Some(407));
        m.u64(COMPONENT + 0xe4, TREE as u64);
        m.u64(TREE + 0x24, OV_NODES as u64);
        m.u32(TREE + 0x2c, 1);
        m.u64(OV_NODES + 0x20 + 0x10, 1);
        m.u64(OV_NODES + 0x20 + 0x18, OVERRIDE as u64);
        m.u32(OVERRIDE + 0x10 + 0x298, 123);
        m.put(OVERRIDE + 0x10 + 0x2b5, &[1]);
        m.u32(OVERRIDE + 0x10 + 0x64, 450);
        m.u32(OVERRIDE + 0x10 + 0x74, 1);
        m.u32(OVERRIDE + 0x10 + 0x78, 1);
        m.u64(OVERRIDE + 0x10 + 0x7c, EFFECT as u64);
        m.u64(EFFECT + 0x1c, TEXT as u64);
        m.u32(EFFECT + 0x24, 20);
        m.u32(EFFECT + 0x28, 32);
        m.put(TEXT, b"Regenerative Shield");
        m.u32(EFFECT + 0x24, 19);
        let items = capture(BASE, &[123], |a, n| m.read(a, n)).unwrap();
        assert_eq!(items[1].slot, "1");
        assert_eq!(items[1].item_level, Some(450));
        assert_eq!(items[1].effects, vec!["Regenerative Shield"]);
        for changed in [
            MANAGER + 0x94,
            DATA + 0x90,
            NODES + 0x1c,
            ARRAY + 0x14,
            SLOTS + 8,
            TREE + 0x2c,
            OV_NODES + 0x20,
            OVERRIDE + 0x10 + 0x64,
            OVERRIDE + 0x10 + 0x74,
            EFFECT + 0x1c,
            TEXT,
        ] {
            let mut calls = 0;
            assert!(
                capture(BASE, &[123], |a, n| {
                    let mut b = m.read(a, n)?;
                    if a == changed {
                        calls += 1;
                        if calls == 2 {
                            b[0] ^= 1;
                        }
                    }
                    Ok(b)
                })
                .is_err(),
                "mutation {changed:x}"
            );
        }
    }
    #[test]
    fn two_strings_per_effect_preserve_the_full_bounded_api_array() {
        let mut m = fixture();
        let instance = ENTRY + 0x10;
        m.u32(instance + 0x74, 64);
        m.u32(instance + 0x78, 64);
        m.u64(instance + 0x7c, EFFECT as u64);
        for index in 0..64 {
            for (part, offset) in [0x1cusize, 0x70].into_iter().enumerate() {
                let value = format!("Effect {index} part {part}");
                let address = TEXT + (index * 2 + part) * 1024;
                let holder = EFFECT + index * 0x194 + offset;
                m.u64(holder, address as u64);
                m.u32(holder + 8, value.len() as u32);
                m.u32(holder + 12, 512);
                m.put(address, value.as_bytes());
            }
        }
        let items = capture(BASE, &[123], |a, n| m.read(a, n)).unwrap();
        assert_eq!(items[0].effects.len(), 128);
        assert_eq!(items[0].effects.last().unwrap(), "Effect 63 part 1");
    }
    #[test]
    fn base_stats_use_native_display_ids_dynamic_labels_and_slot_values() {
        let mut m = fixture();
        let labels = TEXT + 0x10000;
        let label_text = TEXT + 0x20000;
        m.u64(MANAGER + 0x6e30, labels as u64);
        m.u32(MANAGER + 0x6e38, 1);
        m.u32(labels + 0x68 + 0x10, 0);
        m.u64(labels + 0x68 + 0x24, label_text as u64);
        m.u32(labels + 0x68 + 0x2c, 6);
        m.u32(labels + 0x68 + 0x30, 64);
        m.put(label_text, b"Health");
        let mut sources = vec![
            MANAGER + 0x6e30,
            MANAGER + 0x6e38,
            labels + 0x68,
            label_text,
        ];
        for (entry, value) in [(ENTRY, 100), (ENTRY + 0x1000, 200)] {
            let stats = entry + 0x10000;
            m.u64(entry + 0x10 + 0x160, stats as u64);
            m.u32(entry + 0x10 + 0x168, 1);
            m.u32(stats + 0x1c + 0x10, 0);
            m.u32(stats + 0x1c + 0x14, value);
            // Ability Cost is a known internal stat excluded by native tooltip order.
            m.u32(stats + 0x1c + 8, 2);
            m.u32(stats + 0x38 + 0x10, 35);
            m.u32(stats + 0x38 + 0x14, 999);
            sources.extend([entry + 0x10 + 0x160, entry + 0x10 + 0x168, stats + 0x1c]);
        }
        let result = capture(BASE, &[123], |a, n| m.read(a, n)).unwrap();
        assert_eq!(
            result[0].base_stats,
            vec![EquippedStat {
                name: "Health".into(),
                value: 100
            }]
        );
        assert_eq!(result[1].base_stats[0].value, 200);
        for changed in sources {
            let mut calls = 0;
            assert!(
                capture(BASE, &[123], |a, n| {
                    let mut b = m.read(a, n)?;
                    if a == changed {
                        calls += 1;
                        if calls == 2 {
                            b[0] ^= 1;
                        }
                    }
                    Ok(b)
                })
                .is_err(),
                "stats mutation {changed:x}"
            );
        }
        m.u32(labels + 0x68 + 0x2c, 513);
        assert!(capture(BASE, &[123], |a, n| m.read(a, n)).is_err());
    }
    #[test]
    fn empty_override_tree_allows_null_nodes() {
        let mut m = fixture();
        m.u64(COMPONENT + 0xe4, TREE as u64);
        assert_eq!(capture(BASE, &[123], |a, n| m.read(a, n)).unwrap().len(), 2);
    }
    #[test]
    fn rejects_oversize_cycles_short_reads_and_unrequested_ids() {
        let mut m = fixture();
        assert!(
            capture(BASE, &[456], |a, n| m.read(a, n))
                .unwrap()
                .is_empty()
        );
        assert!(capture(BASE, &[0], |_, _| panic!("read")).is_err());
        assert!(capture(BASE, &[123], |_, _| Ok(Vec::new())).is_err());
        m.put(ARRAY + 0x14, &65u16.to_le_bytes());
        assert!(capture(BASE, &[123], |a, n| m.read(a, n)).is_err());
        m.put(ARRAY + 0x14, &2u16.to_le_bytes());
        m.u32(NODES + 0x1c + 0x10, 0x17);
        m.u32(NODES + 0x1c + 4, 1);
        assert!(capture(BASE, &[123], |a, n| m.read(a, n)).is_err());
    }
}
