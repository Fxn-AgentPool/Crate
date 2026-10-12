//! Bake exclusion: low-poly triangles that mesh map baking leaves out, and the bake sources of
//! stacked UV islands. See docs/reference/bake-exclusion.md.

use std::collections::{BTreeMap, BTreeSet};

use crate::geo::Geo;
use crate::mesh::Mesh;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BakeMask {
    /// Excluded flag per triangle; empty while nothing is excluded.
    pub excluded: Vec<bool>,
    /// One triangle of each bake source's island.
    pub sources: BTreeSet<u32>,
}

impl BakeMask {
    pub fn is_excluded(&self, tri: u32) -> bool {
        self.excluded.get(tri as usize).copied().unwrap_or(false)
    }

    pub fn count(&self) -> usize {
        self.excluded.iter().filter(|e| **e).count()
    }

    /// Excluded triangles as `[first, count]` ranges, for saving.
    pub fn ranges(&self) -> Vec<[u32; 2]> {
        let mut out: Vec<[u32; 2]> = Vec::new();
        for (t, _) in self.excluded.iter().enumerate().filter(|(_, e)| **e) {
            match out.last_mut() {
                Some(r) if r[0] + r[1] == t as u32 => r[1] += 1,
                _ => out.push([t as u32, 1]),
            }
        }
        out
    }

    /// From saved ranges; ranges beyond `tris` triangles are dropped.
    pub fn from_ranges(ranges: &[[u32; 2]], sources: &[u32], tris: usize) -> Self {
        let mut excluded = vec![false; tris];
        for r in ranges {
            for t in r[0]..r[0].saturating_add(r[1]) {
                if let Some(e) = excluded.get_mut(t as usize) {
                    *e = true;
                }
            }
        }
        let mut m = Self { excluded, sources: sources.iter().copied().filter(|t| (*t as usize) < tris).collect() };
        m.compact();
        m
    }

    /// Excludes (`on`) or includes triangles. A source whose island gets excluded stops being one.
    pub fn set(&mut self, mesh: &Mesh, geo: &Geo, tris: &[u32], on: bool) {
        if self.excluded.len() != mesh.indices.len() / 3 {
            self.excluded = vec![false; mesh.indices.len() / 3];
        }
        for &t in tris {
            self.excluded[t as usize] = on;
        }
        if on {
            let hit: BTreeSet<u32> = tris.iter().map(|t| geo.island(*t)).collect();
            self.sources.retain(|s| !hit.contains(&geo.island(*s)));
        }
        self.compact();
    }

    /// Make bake source (docs/reference/bake-exclusion.md#make-bake-source): for each stack with a
    /// selected island, the first selected island of it becomes its source, and the stack's other
    /// islands are excluded. Returns the number of stacks changed.
    pub fn make_sources(&mut self, mesh: &Mesh, geo: &Geo, selected: &BTreeSet<u32>) -> usize {
        let stacks = geo.stacks(mesh);
        // Stack -> the first selected island in it, in triangle order.
        let mut chosen: BTreeMap<u32, (u32, u32)> = BTreeMap::new();
        for &t in selected {
            let island = geo.island(t);
            if let Some(&s) = stacks.of_island.get(&island) {
                chosen.entry(s).or_insert((island, t));
            }
        }
        if chosen.is_empty() {
            return 0;
        }
        let tris = mesh.indices.len() / 3;
        if self.excluded.len() != tris {
            self.excluded = vec![false; tris];
        }
        for (&stack, &(island, tri)) in &chosen {
            // The stack's previous source goes.
            self.sources.retain(|s| stacks.of_island.get(&geo.island(*s)) != Some(&stack));
            self.sources.insert(tri);
            for t in 0..tris as u32 {
                let i = geo.island(t);
                if stacks.of_island.get(&i) == Some(&stack) {
                    self.excluded[t as usize] = i != island;
                }
            }
        }
        self.compact();
        chosen.len()
    }

    /// Empties `excluded` when nothing is excluded, so an untouched mesh has no mask.
    fn compact(&mut self) {
        if !self.excluded.iter().any(|e| *e) {
            self.excluded.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_round_trip() {
        let mut m = BakeMask { excluded: vec![false, true, true, false, true], sources: [3].into() };
        assert_eq!(m.ranges(), vec![[1, 2], [4, 1]]);
        assert_eq!(BakeMask::from_ranges(&m.ranges(), &[3], 5), m);
        // Out of range parts are dropped; nothing excluded is no mask.
        assert_eq!(BakeMask::from_ranges(&[[4, 9]], &[7], 5).excluded, vec![false, false, false, false, true]);
        m.excluded = vec![false; 5];
        m.compact();
        assert!(m.excluded.is_empty() && m.count() == 0);
    }
}
