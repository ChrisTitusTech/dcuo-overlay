//! Bounded read-only Broker response observations, without ownership or stack claims.
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    time::Instant,
};
const MAX_ROWS: usize = 100;
const MAX_SELLER: usize = 128;
const MAX_READS: usize = 16_384;
const MAX_BYTES: usize = 4 * 1024 * 1024;
const ROW_BYTES: usize = 0x50c;
const HEADER_BYTES: usize = 0xd4;

#[derive(Clone, Debug)]
pub struct Listing {
    pub id: u64,
    pub item_id: u32,
    pub name: Option<String>,
    pub seller: String,
    pub total_price: u64,
    /// Observed server flag; callers must also match seller to the proven player name.
    pub is_own_candidate: bool,
    /// Observed field; its relationship to stack size remains unproven.
    pub quantity_candidate: u32,
}
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub entity: usize,
    pub player_name: String,
    pub listings: Vec<Listing>,
    pub request_token: u64,
    /// Observed separately from linked-list length; total/page semantics unproven.
    pub response_count: u32,
    pub observed_at: Instant,
}
fn add(address: usize, offset: usize) -> Result<usize, String> {
    address
        .checked_add(offset)
        .ok_or_else(|| "Broker address overflow".into())
}
fn pointer(address: usize) -> Result<usize, String> {
    if !(0x10000..=0x0000_7fff_ffff_ffff).contains(&address) {
        return Err("Invalid Broker pointer".into());
    }
    Ok(address)
}
fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("bounded Broker field"),
    )
}
fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("bounded Broker field"),
    )
}
fn address_at(bytes: &[u8], offset: usize) -> Result<usize, String> {
    usize::try_from(u64_at(bytes, offset)).map_err(|_| "Broker pointer overflow".into())
}
struct Reader<F> {
    read: F,
    calls: usize,
    bytes: usize,
    name_proofs: BTreeMap<(usize, usize), Vec<u8>>,
    name_nodes: BTreeMap<usize, [u8; 0x80]>,
    named_wrappers: HashSet<usize>,
}
impl<F: FnMut(usize, usize) -> Result<Vec<u8>, String>> Reader<F> {
    fn remember(&mut self, address: usize, bytes: &[u8]) -> Result<(), String> {
        if let Some(before) = self.name_proofs.get(&(address, bytes.len())) {
            if before != bytes {
                return Err("Broker item name source changed".into());
            }
        } else {
            self.name_proofs
                .insert((address, bytes.len()), bytes.to_vec());
        }
        Ok(())
    }
    fn watched(&mut self, address: usize, len: usize) -> Result<Vec<u8>, String> {
        let bytes = self.bytes(address, len)?;
        if let Some(before) = self.name_proofs.get(&(address, len)) {
            if before != &bytes {
                return Err("Broker item name source changed".into());
            }
        } else {
            self.name_proofs.insert((address, len), bytes.clone());
        }
        Ok(bytes)
    }
    fn item_name(
        &mut self,
        base: usize,
        manager: usize,
        item_id: u32,
    ) -> Result<Option<String>, String> {
        if item_id == 0 {
            return Ok(None);
        }
        let root = self.watched(add(manager, 0x217c + 0x50)?, 8)?;
        let mut address = address_at(&root, 0)?;
        let mut seen = HashSet::new();
        for _ in 0..64 {
            if address == 0 {
                return Ok(None);
            }
            pointer(address)?;
            if !seen.insert(address) {
                return Err("Cyclic Broker item definition path".into());
            }
            let node = if let Some(node) = self.name_nodes.get(&address) {
                *node
            } else {
                let node: [u8; 0x80] = self
                    .bytes(address, 0x80)?
                    .try_into()
                    .map_err(|_| "Short Broker item node")?;
                self.name_nodes.insert(address, node);
                node
            };
            let key = u32_at(&node, 0x50);
            if key == item_id {
                if address_at(&node, 8)? != add(base, 0x2153402)? {
                    return Ok(None);
                }
                let wrapper = address_at(&node, 0)?;
                if wrapper == 0 {
                    return Ok(None);
                }
                pointer(wrapper)?;
                let definition = address_at(&self.watched(add(wrapper, 8)?, 8)?, 0)?;
                if definition == 0 {
                    return Ok(None);
                }
                pointer(definition)?;
                let holder = self.watched(add(definition, 0x250)?, 16)?;
                if u32_at(&holder, 8) == 0 {
                    return Ok(None);
                }
                let (text_address, bytes, text) = self.item_string(&holder)?;
                self.remember(text_address, &bytes)?;
                self.named_wrappers.insert(address);
                return Ok(Some(text));
            }
            address = address_at(&node, if item_id < key { 0x60 } else { 0x68 })?;
        }
        Err("Broker item name path exceeds bound".into())
    }
    fn item_string(&mut self, holder: &[u8]) -> Result<(usize, Vec<u8>, String), String> {
        self.bounded_string(holder, 512)
    }
    fn string(&mut self, holder: &[u8]) -> Result<(usize, Vec<u8>, String), String> {
        self.bounded_string(holder, MAX_SELLER)
    }
    fn bounded_string(
        &mut self,
        holder: &[u8],
        max: usize,
    ) -> Result<(usize, Vec<u8>, String), String> {
        let address = pointer(address_at(holder, 0)?)?;
        let len = u32_at(holder, 8) as usize;
        let capacity = u32_at(holder, 12) as usize;
        if len == 0 || len > max || len > capacity || capacity > 4096 {
            return Err("Invalid bounded Broker name string".into());
        }
        let bytes = self.bytes(address, len)?;
        let text =
            String::from_utf8(bytes.clone()).map_err(|_| "Unsupported Broker name encoding")?;
        if text.trim().is_empty() || text.chars().any(char::is_control) {
            return Err("Invalid Broker name text".into());
        }
        Ok((address, bytes, text))
    }
    fn bytes(&mut self, address: usize, len: usize) -> Result<Vec<u8>, String> {
        if len == 0 || len > 4096 {
            return Err("Broker read exceeds request bound".into());
        }
        pointer(address)?;
        pointer(add(address, len - 1)?)?;
        self.calls = self
            .calls
            .checked_add(1)
            .ok_or("Broker read count overflow")?;
        self.bytes = self
            .bytes
            .checked_add(len)
            .ok_or("Broker byte count overflow")?;
        if self.calls > MAX_READS || self.bytes > MAX_BYTES {
            return Err("Broker snapshot exceeds read budget".into());
        }
        let bytes = (self.read)(address, len)?;
        if bytes.len() != len {
            return Err("Short Broker read; snapshot discarded".into());
        }
        Ok(bytes)
    }
}
struct Proof {
    address: usize,
    row: Vec<u8>,
    seller_address: usize,
    seller: Vec<u8>,
}
fn same_header(before: &[u8], after: &[u8]) -> bool {
    // Ignore unrelated mutable UI state outside the proven response fields.
    [8..16, 0xac..0xc0, 0xc8..0xd4]
        .into_iter()
        .all(|range| before[range.clone()] == after[range])
}
/// Caller establishes process/build identity. Every observation is rechecked;
/// this parser does not infer that the response is My Wares or owned by a player.
pub fn snapshot(
    base: usize,
    read: impl FnMut(usize, usize) -> Result<Vec<u8>, String>,
) -> Result<Snapshot, String> {
    pointer(base)?;
    let mut reader = Reader {
        read,
        calls: 0,
        bytes: 0,
        name_proofs: BTreeMap::new(),
        name_nodes: BTreeMap::new(),
        named_wrappers: HashSet::new(),
    };
    let manager_address = add(base, 0x2134130)?;
    let manager_bytes = reader.bytes(manager_address, 8)?;
    let manager = pointer(address_at(&manager_bytes, 0)?)?;
    let entity_address = add(manager, 0x94)?;
    let entity_bytes = reader.bytes(entity_address, 8)?;
    let entity = pointer(address_at(&entity_bytes, 0)?)?;
    let data_address = add(entity, 8)?;
    let data_bytes = reader.bytes(data_address, 8)?;
    let data = pointer(address_at(&data_bytes, 0)?)?;
    let gate_address = add(data, 0x50)?;
    let gate = reader.bytes(gate_address, 4)?;
    if u32_at(&gate, 0) != 0 {
        return Err("Unsupported Broker player-name context".into());
    }
    let name_holder_address = add(entity, 0x264)?;
    let name_holder = reader.bytes(name_holder_address, 16)?;
    let (name_address, name_bytes, player_name) = reader.string(&name_holder)?;
    let context_proofs = [
        (entity_address, entity_bytes),
        (data_address, data_bytes),
        (gate_address, gate),
        (name_holder_address, name_holder),
    ];
    let broker = add(manager, 0x1e00)?;
    let header = reader.bytes(broker, HEADER_BYTES)?;
    let request_token = u64_at(&header, 8);
    if request_token == 0 || request_token != u64_at(&header, 0xcc) {
        return Err("Broker request/response tokens do not match".into());
    }
    let count = u32_at(&header, 0xac) as usize;
    if count > MAX_ROWS {
        return Err("Broker row count exceeds bound".into());
    }
    let head = address_at(&header, 0xb0)?;
    let tail = address_at(&header, 0xb8)?;
    if (count == 0 && (head != 0 || tail != 0)) || (count != 0 && (head == 0 || tail == 0)) {
        return Err("Invalid Broker list endpoints".into());
    }
    let mut current = head;
    let mut previous = 0;
    let mut addresses = HashSet::new();
    let mut ids = HashSet::new();
    let mut proofs = Vec::with_capacity(count);
    let mut listings = Vec::with_capacity(count);
    let mut item_names: HashMap<u32, Option<String>> = HashMap::new();
    for _ in 0..count {
        pointer(current)?;
        if !addresses.insert(current) {
            return Err("Cyclic or duplicate Broker list node".into());
        }
        let row = reader.bytes(current, ROW_BYTES)?;
        if address_at(&row, 0x504)? != previous {
            return Err("Broken Broker previous link".into());
        }
        let id = u64_at(&row, 0);
        if !ids.insert(id) {
            return Err("Duplicate Broker listing ID".into());
        }
        let (seller_address, seller_bytes, seller) = reader.string(&row[0x10..0x20])?;
        let own = row[0x24];
        if own > 1 {
            return Err("Unsupported Broker ownership flag".into());
        }
        let item_id = u32_at(&row, 0x2d0);
        let name = if u32_at(&row, 0x58) != 0 {
            let (address, bytes, text) = reader.item_string(&row[0x50..0x60])?;
            reader.remember(address, &bytes)?;
            Some(text)
        } else if let Some(name) = item_names.get(&item_id) {
            name.clone()
        } else {
            let name = reader.item_name(base, manager, item_id)?;
            item_names.insert(item_id, name.clone());
            name
        };
        listings.push(Listing {
            id,
            item_id,
            name,
            seller,
            total_price: u64_at(&row, 0x28),
            is_own_candidate: own == 1,
            quantity_candidate: u32_at(&row, 0x2d8),
        });
        let next = address_at(&row, 0x4fc)?;
        proofs.push(Proof {
            address: current,
            row,
            seller_address,
            seller: seller_bytes,
        });
        previous = current;
        current = next;
    }
    for (address, before) in context_proofs {
        if reader.bytes(address, before.len())? != before {
            return Err("Broker player context changed; snapshot discarded".into());
        }
    }
    let name_proofs = std::mem::take(&mut reader.name_proofs);
    for ((address, len), before) in name_proofs {
        if reader.bytes(address, len)? != before {
            return Err("Broker item name changed; snapshot discarded".into());
        }
    }
    let name_nodes = std::mem::take(&mut reader.name_nodes);
    for (address, before) in name_nodes {
        let after = reader.bytes(address, 0x80)?;
        for range in [8..16, 0x50..0x54, 0x60..0x70] {
            if before[range.clone()] != after[range] {
                return Err("Broker item definition changed; snapshot discarded".into());
            }
        }
        if reader.named_wrappers.contains(&address) && before[..8] != after[..8] {
            return Err("Broker item wrapper changed; snapshot discarded".into());
        }
    }
    if reader.bytes(name_address, name_bytes.len())? != name_bytes {
        return Err("Broker player name changed; snapshot discarded".into());
    }
    if current != 0 || previous != tail {
        return Err("Broker list count/tail mismatch".into());
    }
    let observed_at = Instant::now();
    for proof in proofs {
        if reader.bytes(proof.address, ROW_BYTES)? != proof.row
            || reader.bytes(proof.seller_address, proof.seller.len())? != proof.seller
        {
            return Err("Broker row or seller changed; snapshot discarded".into());
        }
    }
    if !same_header(&header, &reader.bytes(broker, HEADER_BYTES)?)
        || reader.bytes(manager_address, 8)? != manager_bytes
    {
        return Err("Broker response or manager changed; snapshot discarded".into());
    }
    Ok(Snapshot {
        entity,
        player_name,
        listings,
        request_token,
        response_count: u32_at(&header, 0xc8),
        observed_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    const BASE: usize = 0x1000_0000;
    const MANAGER: usize = 0x2000_0000;
    const BROKER: usize = MANAGER + 0x1e00;
    const FIRST: usize = 0x3000_0000;
    const SECOND: usize = 0x3000_1000;
    const SELLER: usize = 0x4000_0000;
    const ENTITY: usize = 0x5000_0000;
    const DATA: usize = 0x5100_0000;
    const PLAYER_NAME: usize = 0x5200_0000;
    const NODE: usize = 0x6000_0000;
    const WRAPPER: usize = 0x6100_0000;
    const DEFINITION: usize = 0x6200_0000;
    const ITEM_NAME: usize = 0x6300_0000;
    #[derive(Clone, Default)]
    struct Memory(HashMap<usize, u8>);
    impl Memory {
        fn put(&mut self, address: usize, bytes: &[u8]) {
            for (i, &b) in bytes.iter().enumerate() {
                self.0.insert(address + i, b);
            }
        }
        fn u32(&mut self, address: usize, n: u32) {
            self.put(address, &n.to_le_bytes());
        }
        fn u64(&mut self, address: usize, n: u64) {
            self.put(address, &n.to_le_bytes());
        }
        fn read(&self, address: usize, len: usize) -> Result<Vec<u8>, String> {
            assert!((1..=4096).contains(&len));
            Ok((address..address + len)
                .map(|a| self.0.get(&a).copied().unwrap_or(0))
                .collect())
        }
    }
    fn fixture() -> Memory {
        let mut m = Memory::default();
        m.u64(BASE + 0x2134130, MANAGER as u64);
        m.u64(MANAGER + 0x94, ENTITY as u64);
        m.u64(ENTITY + 8, DATA as u64);
        m.u64(ENTITY + 0x264, PLAYER_NAME as u64);
        m.u32(ENTITY + 0x26c, 6);
        m.u32(ENTITY + 0x270, 64);
        m.put(PLAYER_NAME, b"Player");
        m.u64(BROKER + 8, 7);
        m.u64(BROKER + 0xcc, 7);
        m.u32(BROKER + 0xac, 2);
        m.u64(BROKER + 0xb0, FIRST as u64);
        m.u64(BROKER + 0xb8, SECOND as u64);
        m.u32(BROKER + 0xc8, 30);
        for (address, id, previous, next, offset) in
            [(FIRST, 11, 0, SECOND, 0), (SECOND, 12, FIRST, 0, 32)]
        {
            m.u64(address, id);
            m.u64(address + 0x10, (SELLER + offset) as u64);
            m.u32(address + 0x18, 6);
            m.u32(address + 0x1c, 256);
            m.put(SELLER + offset, b"Seller");
            m.u64(address + 0x28, 1_234_567);
            m.u32(address + 0x2d0, 123);
            m.u32(address + 0x2d8, 1);
            m.u64(address + 0x4fc, next as u64);
            m.u64(address + 0x504, previous as u64);
        }
        m
    }
    fn named_fixture() -> Memory {
        let mut m = fixture();
        m.u64(MANAGER + 0x217c + 0x50, NODE as u64);
        m.u32(NODE + 0x50, 123);
        m.u64(NODE + 8, (BASE + 0x2153402) as u64);
        m.u64(NODE, WRAPPER as u64);
        m.u64(WRAPPER + 8, DEFINITION as u64);
        m.u64(DEFINITION + 0x250, ITEM_NAME as u64);
        m.u32(DEFINITION + 0x258, 14);
        m.u32(DEFINITION + 0x25c, 64);
        m.put(ITEM_NAME, b"Synthetic item");
        m
    }
    #[test]
    fn resolves_shared_item_name_and_preserves_player_and_own_candidate_separately() {
        let mut m = named_fixture();
        m.put(FIRST + 0x24, &[1, 0xff, 0xff, 0xff]);
        let mut node_reads = 0;
        let result = snapshot(BASE, |a, n| {
            if a == NODE {
                node_reads += 1;
            }
            m.read(a, n)
        })
        .unwrap();
        assert_eq!(node_reads, 2);
        assert!(
            result
                .listings
                .iter()
                .all(|row| row.name.as_deref() == Some("Synthetic item"))
        );
        assert_eq!(result.entity, ENTITY);
        assert_eq!(result.player_name, "Player");
        assert!(result.listings[0].is_own_candidate);
        assert_ne!(result.listings[0].seller, result.player_name);
        m.put(FIRST + 0x24, &[2]);
        assert!(snapshot(BASE, |a, n| m.read(a, n)).is_err());
    }
    #[test]
    fn row_override_precedes_definition_and_unloaded_or_wrong_type_stays_unknown() {
        let mut m = named_fixture();
        m.u64(FIRST + 0x50, (ITEM_NAME + 256) as u64);
        m.u32(FIRST + 0x58, 8);
        m.u32(FIRST + 0x5c, 64);
        m.put(ITEM_NAME + 256, b"Override");
        let result = snapshot(BASE, |a, n| m.read(a, n)).unwrap();
        assert_eq!(result.listings[0].name.as_deref(), Some("Override"));
        assert_eq!(result.listings[1].name.as_deref(), Some("Synthetic item"));
        for case in 0..3 {
            let mut m = named_fixture();
            match case {
                0 => m.u64(NODE, 0),
                1 => m.u64(NODE + 8, (BASE + 0x2153403) as u64),
                _ => m.u32(DEFINITION + 0x258, 0),
            }
            assert!(
                snapshot(BASE, |a, n| m.read(a, n))
                    .unwrap()
                    .listings
                    .iter()
                    .all(|row| row.name.is_none())
            );
        }
    }
    #[test]
    fn item_name_bounds_paths_and_successful_identity_proofs_are_rechecked() {
        for case in 0..3 {
            let mut m = named_fixture();
            match case {
                0 => m.u32(DEFINITION + 0x258, 513),
                1 => m.put(ITEM_NAME, &[0xff; 14]),
                _ => {
                    m.u32(NODE + 0x50, 124);
                    m.u64(NODE + 0x60, NODE as u64);
                }
            }
            assert!(snapshot(BASE, |a, n| m.read(a, n)).is_err());
        }
        for (changed, offset) in [
            (NODE, 0),
            (NODE, 8),
            (NODE, 0x50),
            (NODE, 0x60),
            (WRAPPER + 8, 0),
            (DEFINITION + 0x250, 0),
            (ITEM_NAME, 0),
        ] {
            let m = named_fixture();
            let mut reads = 0;
            assert!(
                snapshot(BASE, |a, n| {
                    let mut bytes = m.read(a, n)?;
                    if a == changed {
                        reads += 1;
                        if reads == 2 {
                            bytes[offset] ^= 1;
                        }
                    }
                    Ok(bytes)
                })
                .is_err(),
                "{changed:x}/{offset:x}"
            );
        }
        let m = named_fixture();
        let mut reads = 0;
        snapshot(BASE, |a, n| {
            let mut bytes = m.read(a, n)?;
            if a == NODE {
                reads += 1;
                if reads == 2 {
                    bytes[0x18] ^= 1;
                }
            }
            Ok(bytes)
        })
        .unwrap();
    }
    #[test]
    fn player_context_and_name_are_bounded_and_rechecked() {
        for changed in [
            MANAGER + 0x94,
            ENTITY + 8,
            DATA + 0x50,
            ENTITY + 0x264,
            PLAYER_NAME,
        ] {
            let m = fixture();
            let mut reads = 0;
            assert!(
                snapshot(BASE, |a, n| {
                    let mut bytes = m.read(a, n)?;
                    if a == changed {
                        reads += 1;
                        if reads == 2 {
                            bytes[0] ^= 1;
                        }
                    }
                    Ok(bytes)
                })
                .is_err()
            );
        }
        let mut m = fixture();
        m.u32(DATA + 0x50, 1);
        assert!(snapshot(BASE, |a, n| m.read(a, n)).is_err());
    }
    #[test]
    fn preserves_observed_fields_without_count_or_quantity_assumptions() {
        let mut m = fixture();
        m.u32(FIRST + 0x2d8, 0);
        let result = snapshot(BASE, |a, n| m.read(a, n)).unwrap();
        assert_eq!(result.listings.len(), 2);
        assert_eq!(result.response_count, 30);
        assert_eq!(result.request_token, 7);
        let row = &result.listings[0];
        assert_eq!(
            (row.id, row.item_id, row.total_price, row.quantity_candidate),
            (11, 123, 1_234_567, 0)
        );
        assert_eq!(row.seller, "Seller");
        let mut m = fixture();
        m.u32(BROKER + 0xac, 0);
        m.u64(BROKER + 0xb0, 0);
        m.u64(BROKER + 0xb8, 0);
        assert!(
            snapshot(BASE, |a, n| m.read(a, n))
                .unwrap()
                .listings
                .is_empty()
        );
    }
    #[test]
    fn rejects_bad_tokens_endpoints_counts_links_cycles_and_duplicate_ids() {
        for case in 0..9 {
            let mut m = fixture();
            match case {
                0 => m.u64(BROKER + 8, 0),
                1 => m.u64(BROKER + 0xcc, 8),
                2 => m.u32(BROKER + 0xac, 101),
                3 => m.u64(BROKER + 0xb0, 0),
                4 => m.u64(BROKER + 0xb8, FIRST as u64),
                5 => m.u64(SECOND + 0x504, 0),
                6 => m.u64(FIRST + 0x4fc, FIRST as u64),
                7 => m.u64(SECOND, 11),
                _ => m.u32(BROKER + 0xac, 3),
            }
            assert!(snapshot(BASE, |a, n| m.read(a, n)).is_err(), "case {case}");
        }
    }
    #[test]
    fn validates_utf8_lengths_capacity_and_control_characters() {
        let mut m = fixture();
        m.put(SELLER, "Héros".as_bytes());
        assert_eq!(
            snapshot(BASE, |a, n| m.read(a, n)).unwrap().listings[0].seller,
            "Héros"
        );
        for case in 0..6 {
            let mut m = fixture();
            match case {
                0 => m.u32(FIRST + 0x18, 129),
                1 => m.u32(FIRST + 0x1c, 5),
                2 => m.u32(FIRST + 0x1c, 4097),
                3 => m.put(SELLER, b"Sell\0r"),
                4 => m.put(SELLER, &[0xff; 6]),
                _ => m.u32(FIRST + 0x18, 0),
            }
            assert!(snapshot(BASE, |a, n| m.read(a, n)).is_err(), "case {case}");
        }
    }
    #[test]
    fn rejects_short_reads_bad_pointers_and_arithmetic_overflow() {
        let m = fixture();
        assert!(
            snapshot(BASE, |a, n| {
                let mut b = m.read(a, n)?;
                b.pop();
                Ok(b)
            })
            .is_err()
        );
        assert!(snapshot(usize::MAX, |_, _| panic!("invalid base must not read")).is_err());
        let mut m = fixture();
        m.u64(BROKER + 0xb0, u64::MAX);
        assert!(snapshot(BASE, |a, n| m.read(a, n)).is_err());
        assert!(add(usize::MAX, 1).is_err());
    }
    #[test]
    fn mutations_in_rows_strings_manager_and_response_fields_discard_snapshot() {
        for changed in [FIRST, SELLER, BROKER, BASE + 0x2134130] {
            let m = fixture();
            let mut seen = 0;
            assert!(
                snapshot(BASE, |a, n| {
                    let mut b = m.read(a, n)?;
                    if a == changed {
                        seen += 1;
                        if seen == 2 {
                            let offset = if a == BROKER { 8 } else { 0 };
                            b[offset] ^= 1;
                        }
                    }
                    Ok(b)
                })
                .is_err()
            );
        }
        let m = fixture();
        let mut seen = 0;
        snapshot(BASE, |a, n| {
            let mut b = m.read(a, n)?;
            if a == BROKER {
                seen += 1;
                if seen == 2 {
                    b[0x20] ^= 1;
                }
            }
            Ok(b)
        })
        .unwrap();
    }
    #[test]
    fn maximal_page_stays_within_overall_byte_and_call_budget() {
        let mut m = fixture();
        m.u32(BROKER + 0xac, 100);
        m.u64(BROKER + 0xb8, (FIRST + 99 * 0x1000) as u64);
        for i in 0..100 {
            let address = FIRST + i * 0x1000;
            m.u64(address, i as u64);
            m.u64(address + 0x10, SELLER as u64);
            m.u32(address + 0x18, 128);
            m.u32(address + 0x1c, 4096);
            m.u64(
                address + 0x504,
                if i == 0 { 0 } else { (address - 0x1000) as u64 },
            );
            m.u64(
                address + 0x4fc,
                if i == 99 {
                    0
                } else {
                    (address + 0x1000) as u64
                },
            );
        }
        m.put(SELLER, &[b'A'; 128]);
        let mut calls = 0;
        let mut bytes = 0;
        assert_eq!(
            snapshot(BASE, |a, n| {
                calls += 1;
                bytes += n;
                m.read(a, n)
            })
            .unwrap()
            .listings
            .len(),
            100
        );
        assert!((414..=MAX_READS).contains(&calls));
        assert!(bytes <= MAX_BYTES);
    }
}
