//! A living planet: drifting tectonic plates raise mountains where they collide and open rifts where they part,
//! old mountains wear down, earthquakes strike along faults, volcanoes erupt and pour lava that cools into new
//! (eventually very fertile) land, and heavy rain makes rivers burst their banks.
use super::*;
use crate::worldgen::{base_fertility, land_biome, land_color, land_terr, sea_color, Biome, SEA};

/// Plate map resolution: one plate id per PB x PB cells.
pub const PB: usize = 16;
/// Geology is updated every GEO ticks (lava every LAVA ticks).
const GEO: i64 = 40;
const LAVA: i64 = 4;
const MAX_LAVA: usize = 3000;
/// Elevation byte above which ground counts as high (mountains, plateaus) for the planet's balance.
const HIGH: u8 = 160;

#[derive(Clone, Serialize, Deserialize)]
pub struct Plate {
    pub vx: f32,
    pub vy: f32,
    pub col: Col,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// plates collide: mountains, volcanic arcs
    Converging = 1,
    /// plates part: rift valleys, volcanoes
    Spreading = 2,
    /// plates slide past each other: earthquakes
    Sliding = 3,
}

#[derive(Clone, Copy)]
pub struct Boundary {
    /// plate-map block on this side and its neighbour block
    pub a: u32,
    pub b: u32,
    pub kind: Fault,
    pub rate: f32,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Volcano {
    pub x: i32,
    pub y: i32,
    /// ticks of eruption left (0 = dormant)
    pub erupt: i32,
    /// eruption strength (lava per vent pulse)
    pub power: f32,
    /// no new eruption before this tick
    pub next: i64,
    pub name: String,
}

impl Sim {
    pub(crate) fn pbw(&self) -> usize {
        self.w.div_ceil(PB)
    }
    pub(crate) fn pbh(&self) -> usize {
        self.h.div_ceil(PB)
    }
    /// Plate-map block under a cell, looked up through a smooth warp so plate borders curve naturally.
    #[inline]
    pub fn plate_block(&self, i: usize) -> usize {
        let (mut x, mut y) = ((i % self.w) as i32, (i / self.w) as i32);
        if let Some(&(dx, dy)) = self.warp.get(i) {
            x = (x + dx as i32).clamp(0, self.w as i32 - 1);
            y = (y + dy as i32).clamp(0, self.h as i32 - 1);
        }
        (y as usize / PB) * self.pbw() + x as usize / PB
    }

    /// Is there a plate border within `r` cells of cell i? Returns the fault kind there (0 = none).
    pub fn fault_near(&self, i: usize, r: usize) -> u8 {
        if self.plate_map.is_empty() {
            return 0;
        }
        let b = self.plate_block(i);
        let p = self.plate_map[b];
        let (x, y) = (i % self.w, i / self.w);
        for (dx, dy) in [(r as i32, 0i32), (-(r as i32), 0), (0, r as i32), (0, -(r as i32))] {
            let (nx, ny) = (x as i32 + dx, y as i32 + dy);
            if nx < 0 || ny < 0 || nx >= self.w as i32 || ny >= self.h as i32 {
                continue;
            }
            let nb = self.plate_block(ny as usize * self.w + nx as usize);
            if self.plate_map[nb] != p {
                let k = self.fault_of[b].max(self.fault_of[nb]);
                return if k == 0 { 3 } else { k };
            }
        }
        0
    }

    /// The warp field (per cell offsets, in cells) bending the block grid of the plate map.
    pub(crate) fn build_warp(&mut self) {
        let (w, seed) = (self.w, self.wseed ^ 0x3a7);
        self.warp = (0..self.n)
            .into_par_iter()
            .map(|i| {
                let (x, y) = ((i % w) as f32, (i / w) as f32);
                let dx = (crate::worldgen::fbm(x / 48.0, y / 48.0, 3, seed) - 0.5) * 2.0 * 26.0;
                let dy = (crate::worldgen::fbm(x / 48.0, y / 48.0, 3, seed ^ 0x99) - 0.5) * 2.0 * 26.0;
                (dx.clamp(-120.0, 120.0) as i8, dy.clamp(-120.0, 120.0) as i8)
            })
            .collect();
    }

    /// Undo the warp approximately: the cell whose warped position is (x, y).
    fn unwarp(&self, x: i32, y: i32) -> (i32, i32) {
        let xc = x.clamp(0, self.w as i32 - 1) as usize;
        let yc = y.clamp(0, self.h as i32 - 1) as usize;
        match self.warp.get(yc * self.w + xc) {
            Some(&(dx, dy)) => (x - dx as i32, y - dy as i32),
            None => (x, y),
        }
    }

    /// Plates for a fresh world: warped Voronoi regions with random drift; a few dormant volcanoes on the faults.
    pub(crate) fn init_geology(&mut self) {
        self.build_warp();
        let (bw, bh) = (self.pbw(), self.pbh());
        let np = ((bw * bh) / 600).clamp(6, 14);
        let seed = self.wseed ^ 0x7ec7;
        let centres: Vec<(f32, f32)> = (0..np).map(|_| (self.rng.gen_range(0.0..bw as f32), self.rng.gen_range(0.0..bh as f32))).collect();
        self.plates = (0..np)
            .map(|k| {
                let a = self.rng.gen_range(0.0..std::f32::consts::TAU);
                let v = self.rng.gen_range(0.35..1.0);
                Plate { vx: a.cos() * v, vy: a.sin() * v, col: hsv(k as f32 * 360.0 / np as f32, 35.0, 70.0) }
            })
            .collect();
        self.plate_map = (0..bw * bh)
            .map(|b| {
                let (x, y) = ((b % bw) as f32, (b / bw) as f32);
                let wx = x + (crate::worldgen::fbm(x / 9.0, y / 9.0, 3, seed) - 0.5) * 14.0;
                let wy = y + (crate::worldgen::fbm(x / 9.0, y / 9.0, 3, seed ^ 0x55) - 0.5) * 14.0;
                let mut best = (f32::MAX, 0u8);
                for (k, &(cx, cy)) in centres.iter().enumerate() {
                    let d = (wx - cx).powi(2) + (wy - cy).powi(2);
                    if d < best.0 {
                        best = (d, k as u8);
                    }
                }
                best.1
            })
            .collect();
        self.rebuild_boundaries();
        self.geo_land0 = self.land;
        self.geo_high0 = self.high_ground();
        self.uplift = 1.0;
        for _ in 0..np / 2 {
            self.new_volcano(false);
        }
    }

    /// Faults between neighbouring blocks of different plates, typed by how the plates move against each other.
    pub(crate) fn rebuild_boundaries(&mut self) {
        let (bw, bh) = (self.pbw(), self.pbh());
        let mut out = Vec::new();
        let mut kind = vec![0u8; bw * bh];
        if self.plate_map.len() != bw * bh {
            self.boundaries = out;
            self.fault_of = kind;
            return;
        }
        for b in 0..bw * bh {
            let (x, y) = ((b % bw) as i32, (b / bw) as i32);
            for (dx, dy) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= bw as i32 || ny >= bh as i32 {
                    continue;
                }
                let nb = ny as usize * bw + nx as usize;
                let (pa, pb) = (self.plate_map[b] as usize, self.plate_map[nb] as usize);
                if pa == pb {
                    continue;
                }
                let (a, o) = (&self.plates[pa], &self.plates[pb]);
                // closing speed along the boundary normal, and shear across it
                let (rvx, rvy) = (a.vx - o.vx, a.vy - o.vy);
                let close = rvx * dx as f32 + rvy * dy as f32;
                let shear = (rvx * dy as f32 - rvy * dx as f32).abs();
                let (k, rate) = if close > 0.3 {
                    (Fault::Converging, close)
                } else if close < -0.3 {
                    (Fault::Spreading, -close)
                } else {
                    (Fault::Sliding, shear.max(0.2))
                };
                kind[b] = kind[b].max(k as u8 * (k != Fault::Sliding || kind[b] == 0) as u8);
                out.push(Boundary { a: b as u32, b: nb as u32, kind: k, rate });
            }
        }
        self.boundaries = out;
        self.fault_of = kind;
    }

    /// A cell near a fault of the given kind (or any kind), chosen in proportion to how active the fault is.
    fn fault_cell(&mut self, want: Option<Fault>, spread: f32) -> Option<usize> {
        let cands: Vec<usize> = (0..self.boundaries.len()).filter(|&k| want.map_or(true, |f| self.boundaries[k].kind == f)).collect();
        if cands.is_empty() {
            return None;
        }
        let total: f32 = cands.iter().map(|&k| self.boundaries[k].rate).sum();
        let mut pick = self.rr() * total;
        let mut chosen = cands[0];
        for &k in &cands {
            pick -= self.boundaries[k].rate;
            if pick <= 0.0 {
                chosen = k;
                break;
            }
        }
        let bd = self.boundaries[chosen];
        let bw = self.pbw();
        // on the line between the two blocks, scattered across the fault zone
        let (ax, ay) = ((bd.a as usize % bw) as f32, (bd.a as usize / bw) as f32);
        let (bx, by) = ((bd.b as usize % bw) as f32, (bd.b as usize / bw) as f32);
        let u = self.rr();
        let x = ((ax + (bx - ax) * 0.5 + 0.5) * PB as f32 + gauss(&mut self.rng, 0.0, spread) + (u - 0.5) * PB as f32 * (by - ay).abs()) as i32;
        let y = ((ay + (by - ay) * 0.5 + 0.5) * PB as f32 + gauss(&mut self.rng, 0.0, spread) + (u - 0.5) * PB as f32 * (bx - ax).abs()) as i32;
        let (x, y) = self.unwarp(x, y);
        if x < 2 || y < 2 || x >= self.w as i32 - 2 || y >= self.h as i32 - 2 {
            return None;
        }
        Some(y as usize * self.w + x as usize)
    }

    /// Raise or lower the ground; temperature follows the altitude. Rivers and lakes keep their beds.
    pub(crate) fn shift_height(&mut self, i: usize, d: i32) {
        if self.terr[i] == 1 && matches!(Biome::from_u8(self.biome[i]), Biome::Lake | Biome::River) {
            return;
        }
        let old = self.elevation[i] as i32;
        let new = (old + d).clamp(2, 254);
        if new == old {
            return;
        }
        self.elevation[i] = new as u8;
        let sea = SEA as i32;
        let lapse = (new.max(sea) - old.max(sea)) as f32 * 0.99;
        self.tmp[i] = (self.tmp[i] as f32 - lapse).clamp(5.0, 255.0) as u8;
        self.geo_dirty.push(i as u32);
    }

    /// Re-derive terrain, biome, colour (and fertility if the biome changed) of a cell from its height.
    pub(crate) fn refresh_cell(&mut self, i: usize) {
        let k = i as u32;
        if self.pools.contains_key(&k) || self.lava.contains_key(&k) || self.basalt.contains_key(&k) {
            return;
        }
        let old_b = Biome::from_u8(self.biome[i]);
        if self.terr[i] == 1 && matches!(old_b, Biome::Lake | Biome::River) {
            return;
        }
        let (w, h) = (self.w, self.h);
        let (x, y) = (i % w, i / w);
        let seed = self.wseed;
        let e = self.elevation[i];
        let ocean = |s: &Sim, j: usize| s.terr[j] == 1 && s.biome[j] == Biome::Ocean as u8;
        let coastal = self.neighbors(i).any(|j| ocean(self, j));
        if e < SEA && (old_b == Biome::Ocean || coastal) {
            if self.terr[i] != 1 {
                // the coast sinks below the sea
                self.setp(i, 0, None);
                self.land -= 1;
                self.terr[i] = 1;
                self.water_changed = true;
            }
            self.biome[i] = Biome::Ocean as u8;
            self.moisture[i] = 255;
            self.fert[i] = 0.5;
            self.bg[i] = sea_color(seed, x, y, 1.0 - e as f32 / SEA as f32, self.tmp[i] < 31);
            return;
        }
        if self.terr[i] == 1 {
            // the sea floor rises above the waves: new land
            self.setp(i, 0, None);
            self.terr[i] = 0;
            self.land += 1;
            self.moisture[i] = 170;
            self.water_changed = true;
        }
        let lh = |s: &Sim, xx: usize, yy: usize| (s.elevation[yy * w + xx].max(SEA) - SEA) as f32 / (255 - SEA) as f32;
        let land_h = lh(self, x, y);
        let (xl, xr, yu, yd) = (x.saturating_sub(1), (x + 1).min(w - 1), y.saturating_sub(1), (y + 1).min(h - 1));
        let gx = (lh(self, xr, y) - lh(self, xl, y)) * 70.0;
        let gy = (lh(self, x, yd) - lh(self, x, yu)) * 70.0;
        let slope = (gx * gx + gy * gy).sqrt();
        let sun = (-0.6f32, -0.75f32, 0.45f32);
        let sl = (sun.0 * sun.0 + sun.1 * sun.1 + sun.2 * sun.2).sqrt();
        let nl = (gx * gx + gy * gy + 1.0).sqrt();
        let lit = ((-gx * sun.0 - gy * sun.1 + sun.2) / (nl * sl)).clamp(0.0, 1.0);
        let shade = 0.55 + 0.75 * (lit - sun.2 / sl);
        let moist = self.moisture[i] as f32 / 255.0;
        // beaches stay beaches, and new land rising from the sea starts as one; other low coast keeps its biome
        let beach = coastal && matches!(old_b, Biome::Beach | Biome::Ocean);
        let b = land_biome(seed, x, y, land_h, self.tmp[i] as f32 / 255.0, moist, slope, beach);
        let terr = land_terr(b);
        if b != old_b {
            self.fert[i] = base_fertility(seed, x, y, b, moist);
        }
        if terr == 2 && self.terr[i] != 2 {
            self.setp(i, 0, None); // bare rock
        }
        self.terr[i] = terr;
        self.biome[i] = b as u8;
        self.bg[i] = land_color(seed, x, y, b, land_h, shade);
    }

    /// Re-derive all cells whose height changed (and their neighbours, whose shading changed).
    fn flush_geo(&mut self) {
        if self.geo_dirty.is_empty() {
            return;
        }
        let mut cells = std::mem::take(&mut self.geo_dirty);
        let w = self.w as u32;
        let n = self.n as u32;
        let base = cells.len();
        for k in 0..base {
            let i = cells[k];
            for j in [i.wrapping_sub(1), i + 1, i.wrapping_sub(w), i + w] {
                if j < n {
                    cells.push(j);
                }
            }
        }
        cells.sort_unstable();
        cells.dedup();
        for i in cells {
            self.refresh_cell(i as usize);
        }
        self.bg_version += 1;
    }

    pub(crate) fn geology_step(&mut self) {
        if self.plates.is_empty() {
            return;
        }
        if self.every(LAVA, 1) {
            self.volcano_pulse();
            self.lava_step();
        }
        if self.every(GEO, 3) {
            self.tectonics();
            self.erosion();
            self.floods();
            if self.every(200, 5) {
                self.weathering();
            }
            if self.every(2000, 11) {
                self.drift_plates();
            }
            self.flush_geo();
            if self.water_changed && self.every(1000, 13) {
                self.water_changed = false;
                self.rebuild_water_grid();
                if self.nutrients.len() != self.gwater.len() {
                    self.nutrients = self.gwater.iter().map(|&wf| wf * NUTRIENT_CAP).collect();
                }
            }
        }
    }

    /// Cells of high ground (mountains and high plateaus).
    fn high_ground(&self) -> i64 {
        self.elevation.iter().filter(|&&e| e > HIGH).count() as i64
    }

    /// Mountain building where plates collide, rift valleys where they part. Uplift is balanced against erosion
    /// over the whole planet: mountains rise in one place while old ones wear down elsewhere, but the share of
    /// high ground and the land area stay about where they started.
    fn tectonics(&mut self) {
        if self.every(2000, 17) {
            let high = self.high_ground() as f32;
            let target = self.geo_high0.max(1) as f32;
            self.uplift = (self.uplift * if high > target * 1.05 { 0.8 } else if high < target * 0.95 { 1.15 } else { 1.0 }).clamp(0.05, 1.5);
        }
        let sea_floor_rises = self.land < self.geo_land0;
        let nb = self.boundaries.len();
        for k in 0..nb {
            let bd = self.boundaries[k];
            match bd.kind {
                Fault::Converging => {
                    if self.rr() < (0.3 * bd.rate * self.uplift).min(0.9) {
                        let Some(i) = self.fault_cell_of(k, 5.0) else { continue };
                        if self.elevation[i] < SEA && !sea_floor_rises {
                            continue;
                        }
                        // a broad swell: the centre rises most
                        self.shift_height(i, 1);
                        for j in [i - 1, i + 1, i - self.w, i + self.w] {
                            if self.rr() < 0.6 {
                                self.shift_height(j, 1);
                            }
                        }
                    }
                }
                Fault::Spreading => {
                    if self.rr() < (0.2 * bd.rate).min(0.6) {
                        let Some(i) = self.fault_cell_of(k, 3.0) else { continue };
                        self.shift_height(i, -1);
                    }
                }
                Fault::Sliding => {}
            }
        }
        // new volcanoes are born on active faults now and then
        if self.volcanoes.len() < self.plates.len() + 2 && self.rr() < 0.0015 {
            self.new_volcano(true);
        }
    }

    fn fault_cell_of(&mut self, k: usize, spread: f32) -> Option<usize> {
        let bd = self.boundaries[k];
        let bw = self.pbw();
        let (ax, ay) = ((bd.a as usize % bw) as f32 + 0.5, (bd.a as usize / bw) as f32 + 0.5);
        let (bx, by) = ((bd.b as usize % bw) as f32 + 0.5, (bd.b as usize / bw) as f32 + 0.5);
        let u = self.rr() - 0.5;
        let x = ((ax + bx) * 0.5 * PB as f32 + u * PB as f32 * (by - ay).abs() + gauss(&mut self.rng, 0.0, spread)) as i32;
        let y = ((ay + by) * 0.5 * PB as f32 + u * PB as f32 * (bx - ax).abs() + gauss(&mut self.rng, 0.0, spread)) as i32;
        let (x, y) = self.unwarp(x, y);
        if x < 2 || y < 2 || x >= self.w as i32 - 2 || y >= self.h as i32 - 2 {
            return None;
        }
        Some(y as usize * self.w + x as usize)
    }

    /// Old mountains wear down: steep ground slides into the valleys, high peaks weather away (faster in rain).
    fn erosion(&mut self) {
        let samples = (self.n / 800).max(200);
        for _ in 0..samples {
            let i = self.rng.gen_range(0..self.n);
            if self.terr[i] == 1 || self.lava.contains_key(&(i as u32)) {
                continue;
            }
            let e = self.elevation[i];
            if e <= SEA + 2 {
                continue;
            }
            let x = i % self.w;
            let (x0, x1) = (x > 0, x + 1 < self.w);
            let mut low = i;
            for j in [if x0 { Some(i - 1) } else { None }, if x1 { Some(i + 1) } else { None }, i.checked_sub(self.w), if i + self.w < self.n { Some(i + self.w) } else { None }]
                .into_iter()
                .flatten()
            {
                if self.elevation[j] < self.elevation[low] {
                    low = j;
                }
            }
            let wet = self.atmosphere.rain_at(x as f32, (i / self.w) as f32) > 0.2;
            let land_h = (e - SEA) as f32 / (255 - SEA) as f32;
            let steep = e as i32 - self.elevation[low] as i32 > 2;
            // high ground weathers fast, lowlands hardly at all
            let weather = land_h > 0.08 && self.rr() < 0.9 * ((land_h - 0.08) / 0.5).clamp(0.0, 1.0) * if wet { 1.6 } else { 1.0 };
            if steep || weather {
                self.shift_height(i, -1);
                if steep && low != i && self.terr[low] != 1 {
                    self.shift_height(low, 1);
                    self.fert[low] = (self.fert[low] + 0.01).min(1.0);
                }
            }
        }
    }

    /// Plates slowly change course, and colliding plates push their boundary forward.
    fn drift_plates(&mut self) {
        for k in 0..self.plates.len() {
            let a = gauss(&mut self.rng, 0.0, 0.12);
            let (c, s) = (a.cos(), a.sin());
            let p = &mut self.plates[k];
            let (vx, vy) = (p.vx * c - p.vy * s, p.vx * s + p.vy * c);
            p.vx = vx;
            p.vy = vy;
        }
        let mut moved = 0;
        for k in 0..self.boundaries.len() {
            let bd = self.boundaries[k];
            if bd.kind == Fault::Converging && self.rr() < 0.04 * bd.rate {
                // the advancing plate overrides the neighbour block
                self.plate_map[bd.b as usize] = self.plate_map[bd.a as usize];
                moved += 1;
            }
        }
        self.rebuild_boundaries();
        if moved > 0 {
            self.bg_version += 1;
        }
    }

    fn volcano_name(&mut self) -> String {
        format!("Mt {}", crate::util::name(&mut self.rng, 2))
    }

    /// A volcano on an active fault (colliding or parting plates).
    pub(crate) fn new_volcano(&mut self, announce: bool) {
        let want = if self.rr() < 0.7 { Fault::Converging } else { Fault::Spreading };
        for _ in 0..20 {
            let Some(i) = self.fault_cell(Some(want), 6.0) else { return };
            if matches!(Biome::from_u8(self.biome[i]), Biome::Lake | Biome::River) {
                continue;
            }
            let (x, y) = ((i % self.w) as i32, (i / self.w) as i32);
            if self.volcanoes.iter().any(|v| (v.x - x).pow(2) + (v.y - y).pow(2) < 60 * 60) {
                continue;
            }
            // a cone
            for dy in -5i32..=5 {
                for dx in -5i32..=5 {
                    let d = ((dx * dx + dy * dy) as f32).sqrt();
                    let (cx, cy) = (x + dx, y + dy);
                    if d <= 5.0 && cx >= 1 && cy >= 1 && cx < self.w as i32 - 1 && cy < self.h as i32 - 1 {
                        let j = cy as usize * self.w + cx as usize;
                        self.shift_height(j, (6.0 - d) as i32 * 3);
                    }
                }
            }
            let name = self.volcano_name();
            let next = self.t + self.rng.gen_range(2000..30000);
            if announce {
                self.logmsg(format!("A new volcano rises: {name}"), [255, 150, 80]);
            }
            self.volcanoes.push(Volcano { x, y, erupt: 0, power: 1.0, next, name });
            return;
        }
    }

    /// Volcano life: dormant volcanoes wake up now and then; erupting ones pour out lava, build their cone and
    /// throw ash into the sky.
    fn volcano_pulse(&mut self) {
        let dt = LAVA as i32;
        for k in 0..self.volcanoes.len() {
            let (x, y) = (self.volcanoes[k].x, self.volcanoes[k].y);
            if self.volcanoes[k].erupt <= 0 {
                if self.t >= self.volcanoes[k].next && self.rr() < 0.004 {
                    let big = self.rr() < 0.12;
                    let v = &mut self.volcanoes[k];
                    v.erupt = if big { 2600 } else { 900 + (x as i32 * 7 % 900) };
                    v.power = if big { 3.0 } else { 1.0 };
                    let name = v.name.clone();
                    self.banner = Some((if big { "MAJOR ERUPTION!" } else { "VOLCANIC ERUPTION" }.into(), 200, [255, 120, 60]));
                    self.logmsg(format!("{name} erupts{}", if big { " - ash darkens the sky" } else { "" }), [255, 120, 60]);
                    self.earthquake(Some((x, y, if big { 40 } else { 22 })));
                }
                continue;
            }
            let power = self.volcanoes[k].power;
            self.volcanoes[k].erupt -= dt;
            if self.volcanoes[k].erupt <= 0 {
                let rest = self.rng.gen_range(15000..60000);
                self.volcanoes[k].next = self.t + rest;
                continue;
            }
            // lava from the crater
            for _ in 0..(power * 1.5).ceil() as i32 {
                if self.lava.len() >= MAX_LAVA {
                    break;
                }
                let (cx, cy) = (x + self.rng.gen_range(-1..=1), y + self.rng.gen_range(-1..=1));
                if cx < 1 || cy < 1 || cx >= self.w as i32 - 1 || cy >= self.h as i32 - 1 {
                    continue;
                }
                let i = cy as usize * self.w + cx as usize;
                if self.terr[i] != 1 {
                    self.lava_at(i, 1.0);
                }
            }
            // the cone grows
            if self.rr() < 0.12 * power {
                let (dx, dy) = (self.rng.gen_range(-3..=3), self.rng.gen_range(-3..=3));
                let i = ((y + dy).clamp(1, self.h as i32 - 2)) as usize * self.w + (x + dx).clamp(1, self.w as i32 - 2) as usize;
                self.shift_height(i, 1);
            }
            // ash: a big eruption cools the world like a small impact winter
            self.dust = (self.dust + 0.00004 * power * power).min(0.35);
        }
    }

    fn lava_at(&mut self, i: usize, heat: f32) {
        let k = i as u32;
        self.setp(i, 0, None);
        self.lava.insert(k, heat);
        self.basalt.remove(&k);
        let ch = self.chunk(i) as u32;
        self.lchunk.entry(ch).or_default().push(k);
    }

    /// Lava runs downhill, sets plants on fire, cools into dark rock, and builds new land where it meets the sea.
    fn lava_step(&mut self) {
        if self.lava.is_empty() {
            return;
        }
        let mut cells: Vec<(u32, f32)> = self.lava.iter().map(|(&k, &v)| (k, v)).collect();
        cells.sort_unstable_by_key(|c| c.0);
        let w = self.w;
        for (k, heat) in cells {
            let i = k as usize;
            let heat = heat - 0.012;
            if heat <= 0.0 {
                self.lava.remove(&k);
                self.cool_lava(i);
                continue;
            }
            self.lava.insert(k, heat);
            if heat > 0.3 && self.rr() < 0.55 && self.lava.len() < MAX_LAVA {
                let (x, y) = ((i % w) as i32, (i / w) as i32);
                let mut best: Option<usize> = None;
                for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 1 || ny < 1 || nx >= w as i32 - 1 || ny >= self.h as i32 - 1 {
                        continue;
                    }
                    let j = ny as usize * w + nx as usize;
                    // lava runs downhill in tongues; on level ground it creeps only now and then
                    if self.lava.contains_key(&(j as u32)) || self.elevation[j] > self.elevation[i] || (self.elevation[j] == self.elevation[i] && self.rr() > 0.2) {
                        continue;
                    }
                    if best.map_or(true, |b| self.elevation[j] < self.elevation[b] || (self.elevation[j] == self.elevation[b] && self.rr() < 0.5)) {
                        best = Some(j);
                    }
                }
                if let Some(j) = best {
                    if self.terr[j] == 1 {
                        if self.biome[j] == Biome::Ocean as u8 {
                            // quenched in the sea: new land
                            let up = SEA as i32 - self.elevation[j] as i32;
                            self.shift_height(j, up.max(0));
                            self.flush_one(j);
                            if self.terr[j] != 1 {
                                self.cool_lava(j);
                            }
                        }
                    } else {
                        self.lava_at(j, heat * 0.93);
                        for f in [j.wrapping_sub(1), j + 1, j.wrapping_sub(w), j + w] {
                            if f < self.n && self.pl[f] > 0 && self.rr() < 0.3 {
                                self.ignite(f, None);
                            }
                        }
                    }
                }
            }
        }
        // chunk index for creatures fleeing lava
        let mut lc: FxHashMap<u32, Vec<u32>> = FxHashMap::default();
        for &k in self.lava.keys() {
            lc.entry(self.chunk(k as usize) as u32).or_default().push(k);
        }
        self.lchunk = lc;
    }

    fn flush_one(&mut self, i: usize) {
        self.geo_dirty.retain(|&k| k as usize != i);
        self.refresh_cell(i);
        self.bg_version += 1;
    }

    /// Fresh lava rock: dark, barren, slightly raised; it weathers into rich volcanic soil.
    fn cool_lava(&mut self, i: usize) {
        if self.rr() < 0.5 {
            self.shift_height(i, 1);
        }
        let (x, y) = (i % self.w, i / self.w);
        let nz = ((x * 7 + y * 13) % 9) as u8;
        self.basalt.insert(i as u32, 9000);
        if self.terr[i] == 3 {
            self.terr[i] = 0;
        }
        self.fert[i] = 0.05;
        self.bg[i] = [52 + nz, 47 + nz, 45 + nz];
        self.bg_version += 1;
    }

    fn weathering(&mut self) {
        if self.basalt.is_empty() {
            return;
        }
        let mut done = Vec::new();
        for (k, v) in self.basalt.iter_mut() {
            *v -= 200;
            if *v <= 0 {
                done.push(*k);
            }
        }
        for k in done {
            self.basalt.remove(&k);
            let i = k as usize;
            self.refresh_cell(i);
            if self.terr[i] != 1 {
                self.fert[i] = (self.fert[i] + 0.35).min(1.0);
            }
        }
        self.bg_version += 1;
    }

    /// Heavy rain makes rivers burst their banks: the low ground around them floods for a while.
    fn floods(&mut self) {
        if !matches!(self.wx.kind, WxKind::Rain | WxKind::Storm) || self.wx.int < 0.5 || self.t < self.flood_cool {
            return;
        }
        for _ in 0..60 {
            let i = self.rng.gen_range(0..self.n);
            if !(self.terr[i] == 1 && self.biome[i] == Biome::River as u8) {
                continue;
            }
            let rain = self.atmosphere.rain_at((i % self.w) as f32, (i / self.w) as f32);
            if rain < 0.45 {
                continue;
            }
            let crest = self.elevation[i].saturating_add(2 + (rain * 3.0) as u8);
            let limit = (150.0 + 500.0 * rain) as usize;
            let (x0, y0) = ((i % self.w) as i32, (i / self.w) as i32);
            let mut queue = std::collections::VecDeque::from([i]);
            let mut seen: FxHashSet<usize> = FxHashSet::default();
            seen.insert(i);
            let mut flooded = 0;
            while let Some(c) = queue.pop_front() {
                if flooded >= limit {
                    break;
                }
                for j in [c.wrapping_sub(1), c + 1, c.wrapping_sub(self.w), c + self.w] {
                    if j >= self.n || !seen.insert(j) {
                        continue;
                    }
                    let (x, y) = ((j % self.w) as i32, (j / self.w) as i32);
                    if (x - x0).pow(2) + (y - y0).pow(2) > 40 * 40 || self.elevation[j] > crest || self.terr[j] == 2 {
                        continue;
                    }
                    if self.terr[j] != 1 {
                        self.make_water(j, true);
                        if let Some(p) = self.pools.get_mut(&(j as u32)) {
                            *p = 500.0 + 900.0 * rain;
                        }
                        flooded += 1;
                    }
                    queue.push_back(j);
                }
            }
            if flooded > 20 {
                self.flood_cool = self.t + 3000;
                // animals caught by the water
                for c in self.crs.iter_mut() {
                    if !c.dead && (c.x as i32 - x0).pow(2) + (c.y as i32 - y0).pow(2) < 40 * 40 && !can_swim(&c.g) {
                        let ci = c.y as usize * self.w + c.x as usize;
                        if self.pools.contains_key(&(ci as u32)) {
                            c.e -= 0.25 * c.max_e;
                        }
                    }
                }
                self.banner = Some(("FLOOD".into(), 150, [120, 180, 255]));
                self.logmsg(format!("Flood: a river bursts its banks ({} cells under water)", flooded), [120, 180, 255]);
            }
            return;
        }
    }

    /// An earthquake at a fault: the ground cracks (a scarp: one side rises), slopes slide.
    pub(crate) fn fault_quake_site(&mut self) -> Option<(i32, i32, i32)> {
        let want = if self.rr() < 0.5 { Some(Fault::Sliding) } else { None };
        let i = self.fault_cell(want, 4.0).or_else(|| self.fault_cell(None, 4.0))?;
        Some(((i % self.w) as i32, (i / self.w) as i32, self.rng.gen_range(20..=50)))
    }

    pub(crate) fn fault_scarp(&mut self, x: i32, y: i32, radius: i32) {
        let a = self.rr() * std::f32::consts::PI;
        let (c, s) = (a.cos(), a.sin());
        for k in -radius..=radius {
            for off in 1..=3 {
                let px = x as f32 + c * k as f32 - s * off as f32;
                let py = y as f32 + s * k as f32 + c * off as f32;
                if px < 1.0 || py < 1.0 || px >= (self.w - 1) as f32 || py >= (self.h - 1) as f32 {
                    continue;
                }
                let i = py as usize * self.w + px as usize;
                if self.terr[i] != 1 {
                    self.shift_height(i, 1);
                }
            }
        }
        self.flush_geo();
        // a quake may wake a volcano nearby
        for v in self.volcanoes.iter_mut() {
            if (v.x - x).pow(2) + (v.y - y).pow(2) < 80 * 80 && v.erupt <= 0 {
                v.next = v.next.min(self.t + 200);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refresh_matches_generator() {
        let mut s = Sim::new_cfg(400, 260, false, 3, crate::worldgen::MapType::Continents, false);
        let before = s.biome.clone();
        let terr = s.terr.clone();
        let mut changed = 0;
        let mut land = 0;
        let mut pairs = std::collections::BTreeMap::new();
        for i in 0..s.n {
            if s.terr[i] != 1 {
                land += 1;
                s.refresh_cell(i);
                if s.biome[i] != before[i] || s.terr[i] != terr[i] {
                    changed += 1;
                    *pairs.entry((before[i], s.biome[i])).or_insert(0) += 1;
                }
            }
        }
        eprintln!("changed {changed} of {land}: {:?}", pairs.iter().map(|((a, b), n)| format!("{}->{} {n}", Biome::from_u8(*a).name(), Biome::from_u8(*b).name())).collect::<Vec<_>>());
        assert!(changed * 100 < land * 3, "changed {changed} of {land}");
    }

    #[test]
    fn eruptions_floods_keep_counts_consistent() {
        let mut s = Sim::new_cfg(400, 260, false, 9, crate::worldgen::MapType::Continents, false);
        assert!(!s.volcanoes.is_empty());
        for v in s.volcanoes.iter_mut() {
            v.next = 0;
        }
        let mut erupted = false;
        for k in 0..4000 {
            if k % 50 == 0 {
                s.wx = Weather { kind: WxKind::Storm, ttl: 5000, t0: s.t - 1000, int: 1.0 };
            }
            s.step(8);
            erupted |= !s.lava.is_empty();
        }
        assert!(erupted, "no eruption");
        let land = (0..s.n).filter(|&i| s.terr[i] != 1).count() as i64;
        assert_eq!(land, s.land, "land count");
        let wp = (0..s.n).filter(|&i| s.pl[i] > 0 && s.terr[i] == 1).count() as i64;
        assert_eq!(wp, s.nwater_plants, "water plants");
        let np = s.pl.iter().filter(|&&p| p > 0).count() as i64;
        assert_eq!(np, s.nplants, "plants");
    }
}
