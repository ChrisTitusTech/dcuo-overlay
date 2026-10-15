//! Bounded, session-local last-seen ground spawn locations. Never persisted.
use crate::{
    maps::{Context, Point},
    native::{GroundSpawnKind, Sample},
    session::View,
};
use std::time::{Duration, Instant};

pub const MAX_CACHED_SPAWNS: usize = 1024;

#[derive(Clone, Debug)]
pub struct CachedSpawn {
    pub point: Point,
    pub context: Context,
    pub kind: GroundSpawnKind,
    pub last_seen: Instant,
}

#[derive(Default)]
pub struct SpawnCache {
    session: Option<(i32, u64, String)>,
    owner: Option<Sample>,
    entries: Vec<CachedSpawn>,
    positions: Vec<[f32; 3]>,
    last_processed: Option<Instant>,
    last_sweep: Option<Instant>,
    misses: Vec<u8>,
}

impl SpawnCache {
    pub fn entries(&self) -> &[CachedSpawn] {
        &self.entries
    }

    /// Missing observations and streaming scan gaps cannot prove a location vanished.
    /// Context checks still run while display/scanning is disabled or reads are paused.
    pub fn update(&mut self, view: &View, enabled: bool, now: Instant) {
        let session = view
            .build
            .as_ref()
            .filter(|_| view.connected)
            .map(|build| (build.pid, build.start_ticks, build.sha256.clone()));
        if session != self.session || session.is_none() {
            self.clear();
            self.session = session;
        }
        if self.session.is_none() {
            return;
        }
        let Some(sample) = view.native.as_ref().filter(|sample| {
            now.checked_duration_since(sample.observed_at)
                .is_some_and(|age| age <= Duration::from_secs(1))
        }) else {
            return;
        };

        // An unknown area cannot establish an area transition, but a changed
        // controlled pawn/world still invalidates locations from the old owner.
        let mut context_sample = sample.clone();
        if context_sample.area_key.is_none() {
            context_sample.area_key = self.owner.as_ref().and_then(|old| old.area_key.clone());
        }
        if self
            .owner
            .as_ref()
            .is_some_and(|old| !context_sample.same_spawn_context(old))
        {
            self.clear_locations();
        }
        context_sample.ground_spawns.clear();
        context_sample.collectible = None;
        context_sample.ground_spawn_sweep = None;
        // Preserve the last confirmed world generation through unknown-area gaps.
        if sample.area_key.is_some() || self.owner.is_none() {
            self.owner = Some(context_sample);
        }

        if !enabled || view.paused || self.last_processed == Some(sample.observed_at) {
            return;
        }
        self.last_processed = Some(sample.observed_at);
        let Some(area) = sample
            .area_key
            .as_ref()
            .filter(|key| crate::native::valid_area_key(key))
        else {
            return;
        };
        let context = Context {
            zone: area.clone(),
            layout: "main".into(),
            floor: "overview".into(),
        };
        if let Some(sweep) = sample.ground_spawn_sweep.as_ref().filter(|sweep| {
            sweep.completed_at <= now
                && self.last_sweep.is_none_or(|last| sweep.completed_at > last)
        }) {
            self.last_sweep = Some(sweep.completed_at);
            for index in (0..self.entries.len()).rev() {
                // A completed sweep cannot disprove a later direct observation.
                if self.entries[index].last_seen > sweep.completed_at {
                    continue;
                }
                let present = sweep
                    .spawns
                    .iter()
                    .chain(&sample.ground_spawns)
                    .any(|spawn| {
                        self.entries[index].kind == spawn.kind
                            && near(self.positions[index], spawn.position)
                    });
                self.misses[index] = if present { 0 } else { self.misses[index] + 1 };
                if self.misses[index] >= 2 {
                    self.remove(index);
                }
            }
            for spawn in &sweep.spawns {
                self.observe(spawn, &context, sweep.completed_at);
            }
        }
        for spawn in &sample.ground_spawns {
            self.observe(spawn, &context, sample.observed_at);
        }
    }

    fn observe(
        &mut self,
        spawn: &crate::native::Collectible,
        context: &Context,
        observed_at: Instant,
    ) {
        if !spawn
            .position
            .iter()
            .all(|coordinate| coordinate.is_finite())
        {
            return;
        }
        if let Some(index) = self
            .entries
            .iter()
            .zip(&self.positions)
            .position(|(old, position)| {
                old.kind == spawn.kind && old.context == *context && near(*position, spawn.position)
            })
        {
            self.entries[index].last_seen = self.entries[index].last_seen.max(observed_at);
            self.misses[index] = 0;
            return;
        }
        if self.entries.len() == MAX_CACHED_SPAWNS {
            let oldest = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| entry.last_seen)
                .map(|(index, _)| index)
                .expect("cache is full");
            self.remove(oldest);
        }
        self.entries.push(CachedSpawn {
            point: Point {
                x: f64::from(spawn.position[0]),
                y: f64::from(spawn.position[1]),
            },
            context: context.clone(),
            kind: spawn.kind,
            last_seen: observed_at,
        });
        self.positions.push(spawn.position);
        self.misses.push(0);
    }

    fn remove(&mut self, index: usize) {
        self.entries.remove(index);
        self.positions.remove(index);
        self.misses.remove(index);
    }

    fn clear_locations(&mut self) {
        self.entries.clear();
        self.positions.clear();
        self.misses.clear();
        self.last_sweep = None;
        self.last_processed = None;
    }
    fn clear(&mut self) {
        self.clear_locations();
        self.owner = None;
    }
}

fn near(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter()
        .zip(b)
        .map(|(a, b)| (f64::from(*a) - f64::from(b)).powi(2))
        .sum::<f64>()
        <= 1.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{native::Collectible, session::BuildInfo};

    fn view(now: Instant) -> View {
        let mut sample = Sample::synthetic(1);
        sample.observed_at = now;
        sample.area_key = Some("GOTHAM".into());
        sample.ground_spawns = vec![
            spawn(0.0, GroundSpawnKind::Blue),
            spawn(10.0, GroundSpawnKind::Gold),
        ];
        View {
            connected: true,
            build: Some(BuildInfo {
                pid: 42,
                start_ticks: 100,
                sha256: "synthetic".into(),
                executable_bytes: 1,
                machine: 0,
                header_access: true,
                map_source: None,
            }),
            native: Some(sample),
            ..View::default()
        }
    }
    fn spawn(x: f32, kind: GroundSpawnKind) -> Collectible {
        Collectible {
            position: [x, 2.0, 3.0],
            kind,
        }
    }
    fn advance(view: &mut View, now: Instant) {
        view.native.as_mut().unwrap().observed_at = now;
    }

    #[test]
    fn sweeps_completed_after_sample_start_expire_after_two_misses() {
        let start = Instant::now();
        let mut view = view(start);
        let mut cache = SpawnCache::default();
        cache.update(&view, true, start);
        for pass in 1..=2 {
            let observed_at = start + Duration::from_millis(pass * 20);
            let completed_at = observed_at + Duration::from_millis(5);
            let sample = view.native.as_mut().unwrap();
            sample.observed_at = observed_at;
            sample.ground_spawns.clear();
            sample.ground_spawn_sweep = Some(
                crate::native::GroundSpawnSweep {
                    completed_at,
                    spawns: Vec::new(),
                }
                .into(),
            );
            cache.update(&view, true, completed_at + Duration::from_millis(1));
            if pass == 1 {
                assert_eq!(cache.entries().len(), 2);
            }
        }
        assert!(cache.entries().is_empty());
    }

    #[test]
    fn partial_empty_missing_stale_and_paused_observations_preserve_locations() {
        let now = Instant::now();
        let mut view = view(now);
        let mut cache = SpawnCache::default();
        cache.update(&view, true, now);
        view.native.as_mut().unwrap().ground_spawns.truncate(1);
        advance(&mut view, now + Duration::from_millis(10));
        cache.update(&view, true, now + Duration::from_millis(10));
        assert_eq!(cache.entries().len(), 2);
        view.native.as_mut().unwrap().ground_spawns.clear();
        advance(&mut view, now + Duration::from_millis(20));
        cache.update(&view, true, now + Duration::from_millis(20));
        let native = view.native.take();
        cache.update(&view, true, now + Duration::from_secs(2));
        view.native = native;
        cache.update(&view, true, now + Duration::from_secs(2));
        view.paused = true;
        advance(&mut view, now + Duration::from_secs(3));
        view.native
            .as_mut()
            .unwrap()
            .ground_spawns
            .push(spawn(100.0, GroundSpawnKind::Blue));
        cache.update(&view, true, now + Duration::from_secs(3));
        assert_eq!(cache.entries().len(), 2);
    }

    #[test]
    fn unknown_area_and_toggle_preserve_cache_but_confirmed_context_changes_clear() {
        let now = Instant::now();
        let mut view = view(now);
        let mut cache = SpawnCache::default();
        cache.update(&view, true, now);
        view.native.as_mut().unwrap().area_key = None;
        advance(&mut view, now + Duration::from_millis(10));
        cache.update(&view, false, now + Duration::from_millis(10));
        assert_eq!(cache.entries().len(), 2);
        view.native.as_mut().unwrap().area_key = Some("GOTHAM".into());
        advance(&mut view, now + Duration::from_millis(20));
        cache.update(&view, true, now + Duration::from_millis(20));
        assert_eq!(cache.entries().len(), 2);
        view.native.as_mut().unwrap().area_key = Some("METROPOLIS".into());
        advance(&mut view, now + Duration::from_millis(30));
        cache.update(&view, false, now + Duration::from_millis(30));
        assert!(cache.entries().is_empty());
        cache.update(&view, true, now + Duration::from_millis(30));
        assert_eq!(cache.entries()[0].context.zone, "METROPOLIS");
        view.native = Some(Sample::synthetic(2));
        advance(&mut view, now + Duration::from_millis(40));
        cache.update(&view, false, now + Duration::from_millis(40));
        assert!(cache.entries().is_empty());
    }

    #[test]
    fn two_completed_missing_sweeps_expire_but_repeats_and_interruptions_do_not() {
        let now = Instant::now();
        let mut view = view(now);
        let mut cache = SpawnCache::default();
        cache.update(&view, true, now);
        view.native.as_mut().unwrap().ground_spawns.clear();
        let first = now + Duration::from_millis(10);
        advance(&mut view, first);
        view.native.as_mut().unwrap().ground_spawn_sweep = Some(
            crate::native::GroundSpawnSweep {
                completed_at: first,
                spawns: vec![],
            }
            .into(),
        );
        cache.update(&view, true, first);
        assert_eq!(cache.entries().len(), 2);
        for offset in [20, 30] {
            let time = now + Duration::from_millis(offset);
            advance(&mut view, time);
            cache.update(&view, true, time);
            assert_eq!(cache.entries().len(), 2);
        }
        view.native.as_mut().unwrap().ground_spawn_sweep = None;
        let interrupted = now + Duration::from_millis(40);
        advance(&mut view, interrupted);
        cache.update(&view, true, interrupted);
        assert_eq!(cache.entries().len(), 2);
        let seen = now + Duration::from_millis(50);
        advance(&mut view, seen);
        view.native
            .as_mut()
            .unwrap()
            .ground_spawns
            .push(spawn(0.0, GroundSpawnKind::Blue));
        cache.update(&view, true, seen);
        view.native.as_mut().unwrap().ground_spawns.clear();
        let second = now + Duration::from_millis(60);
        advance(&mut view, second);
        view.native.as_mut().unwrap().ground_spawn_sweep = Some(
            crate::native::GroundSpawnSweep {
                completed_at: second,
                spawns: vec![],
            }
            .into(),
        );
        cache.update(&view, true, second);
        assert_eq!(cache.entries().len(), 1);
        assert_eq!(cache.entries()[0].kind, GroundSpawnKind::Blue);
        let third = now + Duration::from_millis(70);
        advance(&mut view, third);
        view.native.as_mut().unwrap().ground_spawn_sweep = Some(
            crate::native::GroundSpawnSweep {
                completed_at: third,
                spawns: vec![spawn(100.0, GroundSpawnKind::Green)],
            }
            .into(),
        );
        cache.update(&view, true, third);
        assert_eq!(cache.entries().len(), 1);
        assert_eq!(cache.entries()[0].kind, GroundSpawnKind::Green);
    }

    #[test]
    fn old_sweep_cannot_expire_newer_observations_and_live_sighting_overrides_miss() {
        let now = Instant::now();
        let mut view = view(now);
        let mut cache = SpawnCache::default();
        cache.update(&view, true, now);
        let later = now + Duration::from_millis(20);
        advance(&mut view, later);
        cache.update(&view, true, later);
        view.native.as_mut().unwrap().ground_spawns.clear();
        for offset in [5, 10] {
            let time = later + Duration::from_millis(offset);
            advance(&mut view, time);
            view.native.as_mut().unwrap().ground_spawn_sweep = Some(
                crate::native::GroundSpawnSweep {
                    completed_at: now + Duration::from_millis(offset),
                    spawns: vec![],
                }
                .into(),
            );
            cache.update(&view, true, time);
        }
        assert_eq!(cache.entries().len(), 2);
        assert_eq!(cache.misses, vec![0, 0]);
        for offset in [40, 50] {
            let time = now + Duration::from_millis(offset);
            advance(&mut view, time);
            view.native.as_mut().unwrap().ground_spawns = vec![spawn(0.0, GroundSpawnKind::Blue)];
            view.native.as_mut().unwrap().ground_spawn_sweep = Some(
                crate::native::GroundSpawnSweep {
                    completed_at: time,
                    spawns: vec![],
                }
                .into(),
            );
            cache.update(&view, true, time);
        }
        assert_eq!(cache.entries().len(), 1);
        assert_eq!(cache.entries()[0].kind, GroundSpawnKind::Blue);
    }

    #[test]
    fn process_restart_build_change_and_disconnect_clear() {
        let now = Instant::now();
        for change in 0..4 {
            let mut view = view(now);
            let mut cache = SpawnCache::default();
            cache.update(&view, true, now);
            match change {
                0 => view.build.as_mut().unwrap().pid += 1,
                1 => view.build.as_mut().unwrap().start_ticks += 1,
                2 => view.build.as_mut().unwrap().sha256.push('x'),
                _ => view.connected = false,
            }
            view.native = None;
            cache.update(&view, true, now);
            assert!(cache.entries().is_empty());
        }
    }

    #[test]
    fn dedupe_uses_kind_and_three_dimensions_rejects_invalid_and_caps_history() {
        let now = Instant::now();
        let mut view = view(now);
        let sample = view.native.as_mut().unwrap();
        sample.ground_spawns = vec![
            spawn(0.0, GroundSpawnKind::Blue),
            spawn(0.5, GroundSpawnKind::Blue),
            spawn(0.0, GroundSpawnKind::Gold),
            spawn(f32::NAN, GroundSpawnKind::Blue),
            Collectible {
                kind: GroundSpawnKind::Blue,
                position: [0.0, 2.0, 5.0],
            },
        ];
        let mut cache = SpawnCache::default();
        cache.update(&view, true, now);
        assert_eq!(cache.entries().len(), 3);
        let later = now + Duration::from_millis(10);
        let sample = view.native.as_mut().unwrap();
        sample.observed_at = later;
        sample.ground_spawns = (0..1100)
            .map(|index| spawn(100.0 + index as f32 * 2.0, GroundSpawnKind::Blue))
            .collect();
        cache.update(&view, true, later);
        assert_eq!(cache.entries().len(), MAX_CACHED_SPAWNS);
        assert_eq!(cache.positions.len(), MAX_CACHED_SPAWNS);
        assert!(cache.entries().iter().all(|entry| entry.last_seen == later));
    }
}
