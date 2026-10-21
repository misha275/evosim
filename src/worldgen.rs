//! Terrain and climate generation.
//!
//! 1. Continents: domain-warped fractal noise, sea level set by quantile (~42% ocean).
//! 2. Mountains: ridged multifractal noise in mountain belts gives sharp ridges and peaks.
//! 3. Hydraulic erosion: water droplets carve gullies and deposit sediment.
//! 4. Drainage: priority-flood finds where every cell drains; closed basins become lakes.
//! 5. Climate: temperature from latitude, altitude lapse rate and the ocean;
//!    rain carried by the west wind, wrung out on windward slopes (rain shadows behind ranges).
//! 6. Rivers: rain accumulated downhill; big flows become rivers that widen and carve valleys.
//! 7. Biomes (Whittaker-style) from temperature and moisture; hill-shaded colours.
use crate::util::{mix, Col, Rng};
use rand::Rng as _;
use rayon::prelude::*;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::atomic::{AtomicU32, Ordering};

/// Generation progress 0..100 (shown on the loading screen).
pub static PROGRESS: AtomicU32 = AtomicU32::new(0);

pub const SEA: u8 = 96; // elevation byte of sea level

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Biome {
    Ocean = 0,
    Lake,
    River,
    Beach,
    Desert,
    Savanna,
    Grassland,
    TemperateForest,
    Rainforest,
    Swamp,
    Taiga,
    Tundra,
    Alpine,
    Mountain,
    Glacier,
}

pub const NBIOMES: usize = 15;

impl Biome {
    pub fn from_u8(v: u8) -> Biome {
        if (v as usize) < NBIOMES {
            // SAFETY: repr(u8) with contiguous discriminants 0..NBIOMES
            unsafe { std::mem::transmute::<u8, Biome>(v) }
        } else {
            Biome::Grassland
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Biome::Ocean => "ocean",
            Biome::Lake => "lake",
            Biome::River => "river",
            Biome::Beach => "beach",
            Biome::Desert => "desert",
            Biome::Savanna => "savanna",
            Biome::Grassland => "grassland",
            Biome::TemperateForest => "temperate forest",
            Biome::Rainforest => "rainforest",
            Biome::Swamp => "swamp",
            Biome::Taiga => "taiga",
            Biome::Tundra => "tundra",
            Biome::Alpine => "alpine meadow",
            Biome::Mountain => "mountain rock",
            Biome::Glacier => "glacier / snow",
        }
    }
    /// Flat colour for the biome map overlay.
    pub fn map_col(self) -> Col {
        match self {
            Biome::Ocean => [30, 60, 120],
            Biome::Lake => [50, 110, 170],
            Biome::River => [70, 140, 200],
            Biome::Beach => [225, 210, 150],
            Biome::Desert => [235, 200, 110],
            Biome::Savanna => [190, 175, 80],
            Biome::Grassland => [140, 190, 90],
            Biome::TemperateForest => [60, 140, 60],
            Biome::Rainforest => [20, 105, 50],
            Biome::Swamp => [80, 110, 90],
            Biome::Taiga => [60, 110, 100],
            Biome::Tundra => [170, 175, 150],
            Biome::Alpine => [150, 160, 120],
            Biome::Mountain => [125, 115, 110],
            Biome::Glacier => [240, 245, 250],
        }
    }
    fn ground(self) -> Col {
        match self {
            Biome::Beach => [210, 196, 140],
            Biome::Desert => [214, 184, 120],
            Biome::Savanna => [168, 152, 86],
            Biome::Grassland => [118, 140, 72],
            Biome::TemperateForest => [76, 104, 56],
            Biome::Rainforest => [46, 88, 46],
            Biome::Swamp => [70, 84, 58],
            Biome::Taiga => [74, 92, 72],
            Biome::Tundra => [138, 144, 104],
            Biome::Alpine => [128, 134, 100],
            Biome::Mountain => [122, 116, 110],
            Biome::Glacier => [232, 238, 244],
            _ => [40, 100, 150],
        }
    }
    /// Base soil fertility.
    pub fn fertility(self) -> f32 {
        match self {
            Biome::Rainforest => 0.95,
            Biome::Swamp => 0.85,
            Biome::TemperateForest => 0.8,
            Biome::Grassland => 0.7,
            Biome::Savanna => 0.5,
            Biome::Taiga => 0.45,
            Biome::Alpine => 0.35,
            Biome::Tundra => 0.2,
            Biome::Desert => 0.1,
            Biome::Beach => 0.08,
            Biome::Mountain => 0.03,
            Biome::Glacier => 0.0,
            _ => 0.5,
        }
    }
}

/// Overall layout of land and sea.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum MapType {
    /// several continents separated by open ocean
    Continents,
    /// one supercontinent
    Pangaea,
    /// many islands of all sizes
    Archipelago,
}

impl MapType {
    pub const ALL: [MapType; 3] = [MapType::Continents, MapType::Pangaea, MapType::Archipelago];
    pub fn name(self) -> &'static str {
        match self {
            MapType::Continents => "continents",
            MapType::Pangaea => "pangaea",
            MapType::Archipelago => "archipelago",
        }
    }
    /// share of the map that is ocean
    fn ocean(self) -> f32 {
        match self {
            MapType::Continents => 0.58,
            MapType::Pangaea => 0.42,
            MapType::Archipelago => 0.68,
        }
    }
}

pub struct World {
    pub terr: Vec<u8>,
    pub elevation: Vec<u8>,
    pub moisture: Vec<u8>,
    pub tmp: Vec<u8>,
    pub fert: Vec<f32>,
    pub bg: Vec<Col>,
    pub biome: Vec<u8>,
    pub springs: Vec<u32>,
    pub land: i64,
}

// ------------------------------------------------------------------ noise
#[inline]
fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x27d4_eb2d) ^ (y as u32).wrapping_mul(0x1656_67b1) ^ seed.wrapping_mul(0x9e37_79b9);
    h = (h ^ (h >> 15)).wrapping_mul(0x85eb_ca6b);
    h = (h ^ (h >> 13)).wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    (h & 0x00ff_ffff) as f32 / 16_777_216.0
}

#[inline]
fn vnoise(x: f32, y: f32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - xi as f32, y - yi as f32);
    let sx = fx * fx * fx * (fx * (fx * 6.0 - 15.0) + 10.0);
    let sy = fy * fy * fy * (fy * (fy * 6.0 - 15.0) + 10.0);
    let a = hash(xi, yi, seed);
    let b = hash(xi + 1, yi, seed);
    let c = hash(xi, yi + 1, seed);
    let d = hash(xi + 1, yi + 1, seed);
    let ab = a + (b - a) * sx;
    let cd = c + (d - c) * sx;
    ab + (cd - ab) * sy
}

fn fbm(x: f32, y: f32, oct: u32, seed: u32) -> f32 {
    let (mut s, mut amp, mut f, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for o in 0..oct {
        s += vnoise(x * f, y * f, seed.wrapping_add(o * 1013)) * amp;
        norm += amp;
        amp *= 0.5;
        f *= 2.03;
    }
    s / norm
}

/// Ridged multifractal: sharp crests where the noise crosses its midline.
fn ridged(x: f32, y: f32, oct: u32, seed: u32) -> f32 {
    let (mut s, mut amp, mut f, mut norm, mut weight) = (0.0, 1.0, 1.0, 0.0, 1.0f32);
    for o in 0..oct {
        let n = 1.0 - (vnoise(x * f, y * f, seed.wrapping_add(o * 7919)) * 2.0 - 1.0).abs();
        let n = n * n * weight;
        weight = (n * 1.6).clamp(0.0, 1.0);
        s += n * amp;
        norm += amp;
        amp *= 0.5;
        f *= 2.1;
    }
    s / norm
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn box_blur(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    if r == 0 {
        return src.to_vec();
    }
    // separable running-sum blur, rows then columns (rows in parallel)
    let mut tmp = vec![0f32; w * h];
    tmp.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let s = &src[y * w..y * w + w];
        let mut acc = 0.0;
        let mut cnt = 0.0;
        for x in 0..r.min(w) {
            acc += s[x];
            cnt += 1.0;
        }
        for x in 0..w {
            if x + r < w {
                acc += s[x + r];
                cnt += 1.0;
            }
            if x > r {
                acc -= s[x - r - 1];
                cnt -= 1.0;
            }
            row[x] = acc / cnt;
        }
    });
    let mut out = vec![0f32; w * h];
    let cols: Vec<Vec<f32>> = (0..w)
        .into_par_iter()
        .map(|x| {
            let mut col = vec![0f32; h];
            let mut acc = 0.0;
            let mut cnt = 0.0;
            for y in 0..r.min(h) {
                acc += tmp[y * w + x];
                cnt += 1.0;
            }
            for y in 0..h {
                if y + r < h {
                    acc += tmp[(y + r) * w + x];
                    cnt += 1.0;
                }
                if y > r {
                    acc -= tmp[(y - r - 1) * w + x];
                    cnt -= 1.0;
                }
                col[y] = acc / cnt;
            }
            col
        })
        .collect();
    for (x, col) in cols.into_iter().enumerate() {
        for y in 0..h {
            out[y * w + x] = col[y];
        }
    }
    out
}

fn quantile(v: &[f32], q: f32) -> f32 {
    let step = (v.len() / 200_000).max(1);
    let mut s: Vec<f32> = v.iter().step_by(step).copied().collect();
    s.sort_by(|a, b| a.total_cmp(b));
    s[((s.len() - 1) as f32 * q) as usize]
}

#[derive(PartialEq, PartialOrd)]
struct Ord32(f32);
impl Eq for Ord32 {}
impl Ord for Ord32 {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&o.0)
    }
}

const N8: [(i32, i32); 8] = [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)];

// ------------------------------------------------------------------ erosion
/// Droplet hydraulic erosion on land (heights in world units, sea level 0).
fn erode(hm: &mut [f32], w: usize, h: usize, droplets: usize, rng: &mut Rng) {
    let (inertia, capacity, min_slope, deposit, erosion, evap, gravity) = (0.06f32, 6.0f32, 0.0005, 0.3, 0.35, 0.02, 4.0);
    let idx = |x: usize, y: usize| y * w + x;
    for d in 0..droplets {
        if d % 20000 == 0 {
            PROGRESS.store(35 + (d * 15 / droplets.max(1)) as u32, Ordering::Relaxed);
        }
        let (mut px, mut py) = (rng.gen::<f32>() * (w - 2) as f32, rng.gen::<f32>() * (h - 2) as f32);
        if hm[idx(px as usize, py as usize)] <= 0.0 {
            continue;
        }
        let (mut dx, mut dy, mut speed, mut water, mut sed) = (0f32, 0f32, 1f32, 1f32, 0f32);
        for _ in 0..48 {
            let (xi, yi) = (px as usize, py as usize);
            if xi + 1 >= w || yi + 1 >= h {
                break;
            }
            let (fx, fy) = (px - xi as f32, py - yi as f32);
            let (h00, h10, h01, h11) = (hm[idx(xi, yi)], hm[idx(xi + 1, yi)], hm[idx(xi, yi + 1)], hm[idx(xi + 1, yi + 1)]);
            let gx = (h10 - h00) * (1.0 - fy) + (h11 - h01) * fy;
            let gy = (h01 - h00) * (1.0 - fx) + (h11 - h10) * fx;
            let hcur = h00 * (1.0 - fx) * (1.0 - fy) + h10 * fx * (1.0 - fy) + h01 * (1.0 - fx) * fy + h11 * fx * fy;
            dx = dx * inertia - gx * (1.0 - inertia);
            dy = dy * inertia - gy * (1.0 - inertia);
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-9 {
                break;
            }
            dx /= len;
            dy /= len;
            let (nx, ny) = (px + dx, py + dy);
            if nx < 0.0 || ny < 0.0 || nx >= (w - 1) as f32 || ny >= (h - 1) as f32 {
                break;
            }
            let (nxi, nyi) = (nx as usize, ny as usize);
            let (nfx, nfy) = (nx - nxi as f32, ny - nyi as f32);
            let hn = hm[idx(nxi, nyi)] * (1.0 - nfx) * (1.0 - nfy)
                + hm[idx(nxi + 1, nyi)] * nfx * (1.0 - nfy)
                + hm[idx(nxi, nyi + 1)] * (1.0 - nfx) * nfy
                + hm[idx(nxi + 1, nyi + 1)] * nfx * nfy;
            let dh = hn - hcur;
            let cap = (-dh).max(min_slope) * speed * water * capacity;
            let put = |amount: f32, hm: &mut [f32]| {
                hm[idx(xi, yi)] += amount * (1.0 - fx) * (1.0 - fy);
                hm[idx(xi + 1, yi)] += amount * fx * (1.0 - fy);
                hm[idx(xi, yi + 1)] += amount * (1.0 - fx) * fy;
                hm[idx(xi + 1, yi + 1)] += amount * fx * fy;
            };
            if sed > cap || dh > 0.0 {
                let amt = if dh > 0.0 { dh.min(sed) } else { (sed - cap) * deposit };
                sed -= amt;
                put(amt, hm);
            } else {
                let amt = ((cap - sed) * erosion).min(-dh);
                sed += amt;
                put(-amt, hm);
            }
            if hn <= 0.0 {
                break; // reached the sea
            }
            speed = (speed * speed + dh.abs() * gravity).sqrt().min(8.0);
            water *= 1.0 - evap;
            px = nx;
            py = ny;
        }
    }
}

// ------------------------------------------------------------------ generation
pub fn generate(w: usize, h: usize, map: MapType, rng: &mut Rng) -> World {
    PROGRESS.store(0, Ordering::Relaxed);
    let n = w * h;
    let seed: u32 = rng.gen();
    let big = w.max(h) as f32;
    // feature scales: ~2-4 continents across the map, mountain ridges a few dozen cells wide
    let cont_f = 2.6 / big;
    let ridge_f = 1.0 / (40.0 + big / 25.0);
    let detail_f = 1.0 / 14.0;
    let wf = w as f32;
    let hf = h as f32;
    // continent cores: spread-out points, each grows a landmass; open ocean between them
    let ncont = (3.0 + big / 1400.0).round() as usize + rng.gen_range(0..2);
    let mut cores: Vec<(f32, f32, f32)> = Vec::new();
    for _ in 0..400 {
        if cores.len() >= ncont {
            break;
        }
        let (cx, cy) = (wf * rng.gen_range(0.14..0.86), hf * rng.gen_range(0.16..0.84));
        let rad = big * rng.gen_range(0.14..0.24) / (ncont as f32 / 3.0).sqrt();
        if cores.iter().all(|&(x, y, r)| ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() > (r + rad) * 0.95) {
            cores.push((cx, cy, rad));
        }
    }
    let cont_f = match map {
        MapType::Archipelago => cont_f * 4.5,
        MapType::Continents => cont_f * 2.2,
        MapType::Pangaea => cont_f,
    };
    // ---- 1+2: raw height (rows in parallel): continental base, mountain-belt mask, ridges, hills
    let parts: Vec<(f32, f32, f32, f32)> = (0..n)
        .into_par_iter()
        .map(|i| {
            let (x, y) = ((i % w) as f32, (i / w) as f32);
            // domain warp makes coastlines and ranges organic
            let wx = x + (fbm(x * cont_f * 1.7, y * cont_f * 1.7, 4, seed ^ 0xa1) - 0.5) * big * 0.18;
            let wy = y + (fbm(x * cont_f * 1.7 + 40.0, y * cont_f * 1.7, 4, seed ^ 0xb2) - 0.5) * big * 0.18;
            let cont = fbm(wx * cont_f, wy * cont_f, 6, seed);
            // gentle pull to ocean at the map border
            let ex = (x / wf).min(1.0 - x / wf);
            let ey = (y / hf).min(1.0 - y / hf);
            let edge = smooth(0.0, 0.12, ex.min(ey));
            let base = match map {
                MapType::Pangaea => cont * (0.55 + 0.45 * edge) - (1.0 - edge) * 0.08,
                MapType::Continents => {
                    // distance to the nearest core shapes each continent; noise roughens the coast
                    let m = cores
                        .iter()
                        .map(|&(cx, cy, r)| 1.0 - ((wx - cx).powi(2) + (wy - cy).powi(2)).sqrt() / r)
                        .fold(-1.0f32, f32::max);
                    cont * 0.8 + smooth(-0.6, 0.7, m) * 0.32 - (1.0 - edge) * 0.2
                }
                MapType::Archipelago => cont * (0.6 + 0.4 * edge) - (1.0 - edge) * 0.15,
            };
            let beltn = fbm(wx * cont_f * 1.3 + 17.0, wy * cont_f * 1.3, 3, seed ^ 0x77);
            let r = ridged(wx * ridge_f, wy * ridge_f, 6, seed ^ 0x33);
            let hills = fbm(x * detail_f, y * detail_f, 3, seed ^ 0x55) - 0.5;
            (base, beltn, r, hills)
        })
        .collect();
    // mountain belts cover only a fixed share of the land (~8%): threshold taken over land cells
    let base_v: Vec<f32> = parts.iter().map(|p| p.0).collect();
    let sea0 = quantile(&base_v, map.ocean());
    let belt_land: Vec<f32> = parts.iter().filter(|p| p.0 > sea0).map(|p| p.1).collect();
    let bq = if belt_land.is_empty() { 1.0 } else { quantile(&belt_land, 0.86) };
    let mut hm: Vec<f32> = parts
        .par_iter()
        .map(|&(base, beltn, r, hills)| {
            let belt = smooth(bq - 0.01, bq + 0.03, beltn);
            // plains stay gentle; rugged relief only inside the belts
            // continental interiors rise only gently above the sea: plains and low plateaus
            let base = if base > sea0 { sea0 + (base - sea0) * 0.35 } else { base };
            base + belt * r.powf(1.5) * 0.6 + (r * 0.01 + hills * 0.01) * (0.2 + 0.8 * belt)
        })
        .collect();
    drop(parts);
    PROGRESS.store(20, Ordering::Relaxed);
    let sea = quantile(&hm, map.ocean());
    let top = quantile(&hm, 0.999);
    for v in hm.iter_mut() {
        // sea level 0; land 0..~1 (highest peaks); ocean negative
        *v = if *v > sea { (*v - sea) / (top - sea) } else { (*v - sea) / (sea + 0.05) * 0.6 };
    }
    // sharpen high ground into peaks (makes mountains rise steeply instead of plateaus)
    for v in hm.iter_mut() {
        if *v > 0.0 {
            *v = v.powf(1.35);
        }
    }
    PROGRESS.store(30, Ordering::Relaxed);
    // ---- 3: erosion (scale heights so slopes are in cell units)
    let relief = 60.0;
    for v in hm.iter_mut() {
        *v *= relief;
    }
    let droplets = (n as f32 * 0.35) as usize;
    erode(&mut hm, w, h, droplets, rng);
    for v in hm.iter_mut() {
        *v /= relief;
    }
    PROGRESS.store(50, Ordering::Relaxed);
    // ---- 4: drainage by priority flood (ocean and the border drain away)
    let mut filled = hm.clone();
    let mut done = vec![false; n];
    let mut recv = vec![u32::MAX; n];
    let mut order: Vec<u32> = Vec::with_capacity(n);
    let mut pq: BinaryHeap<Reverse<(Ord32, u32)>> = BinaryHeap::new();
    for i in 0..n {
        let (x, y) = (i % w, i / w);
        if hm[i] <= 0.0 || x == 0 || y == 0 || x == w - 1 || y == h - 1 {
            done[i] = true;
            pq.push(Reverse((Ord32(filled[i]), i as u32)));
        }
    }
    while let Some(Reverse((Ord32(e), i))) = pq.pop() {
        let i = i as usize;
        order.push(i as u32);
        let (x, y) = ((i % w) as i32, (i / w) as i32);
        for (dx, dy) in N8 {
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                continue;
            }
            let j = ny as usize * w + nx as usize;
            if done[j] {
                continue;
            }
            done[j] = true;
            recv[j] = i as u32;
            // random micro-slope across flats: water meanders instead of running in grid-straight lines
            filled[j] = filled[j].max(e + 2e-6 + 4e-5 * hash(nx, ny, seed ^ 0xf1a7));
            pq.push(Reverse((Ord32(filled[j]), j as u32)));
        }
    }
    PROGRESS.store(60, Ordering::Relaxed);
    // lakes: basins filled noticeably above the ground
    let lake: Vec<bool> = (0..n).map(|i| hm[i] > 0.0 && filled[i] - hm[i] > 0.035).collect();
    // ---- 5: climate
    let ocean: Vec<f32> = (0..n).map(|i| if hm[i] <= 0.0 { 1.0 } else { 0.0 }).collect();
    let near_ocean = box_blur(&ocean, w, h, (big / 40.0) as usize + 2);
    // rain: west wind picks up water over the sea and drops it on the way inland, mostly on windward slopes
    let march = |rev: bool| -> Vec<f32> {
        let rows: Vec<Vec<f32>> = (0..h)
            .into_par_iter()
            .map(|y| {
                let mut row = vec![0f32; w];
                let mut hum: f32 = 0.7;
                let mut prev = 0.0f32;
                for k in 0..w {
                    let x = if rev { w - 1 - k } else { k };
                    let i = y * w + x;
                    let e = hm[i].max(0.0);
                    if hm[i] <= 0.0 || lake[i] {
                        hum = (hum + 0.03).min(1.0);
                        row[x] = hum * 0.5;
                    } else {
                        let up = (e - prev).max(0.0) * big / 60.0;
                        let rain = hum * (0.012 + up * 3.0).min(0.5);
                        hum = (hum - rain * 0.85 + 0.0015).clamp(0.0, 1.0);
                        row[x] = rain * 18.0 + hum * 0.35;
                    }
                    prev = e;
                }
                row
            })
            .collect();
        rows.concat()
    };
    let west = march(false);
    let east = march(true);
    let rain_raw: Vec<f32> = (0..n)
        .into_par_iter()
        .map(|i| {
            let (x, y) = ((i % w) as f32, (i / w) as f32);
            let lat = y / hf;
            // dry subtropical belt in the hot south, wet tropics at the very south, wet mid-latitudes
            let belt = 1.0 - 0.45 * (-((lat - 0.74) / 0.07).powi(2)).exp() + 0.2 * (-((lat - 0.95) / 0.06).powi(2)).exp();
            let nz = fbm(x * cont_f * 3.0, y * cont_f * 3.0, 4, seed ^ 0x99);
            ((0.6 * west[i] + 0.25 * east[i]) * belt + (nz - 0.5) * 0.35).max(0.0)
        })
        .collect();
    let rain = box_blur(&rain_raw, w, h, (big / 120.0) as usize + 1);
    // rank-normalise over land: relative wetness matters (windward vs. rain shadow), so map
    // the land distribution onto 0..1 evenly; then a mild curve leaves room for real deserts
    let step = (n / 200_000).max(1);
    let mut sorted: Vec<f32> = (0..n).step_by(step).filter(|&i| hm[i] > 0.0).map(|i| rain[i]).collect();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let rain: Vec<f32> = rain
        .par_iter()
        .map(|&r| {
            if sorted.is_empty() {
                return 0.5;
            }
            let rank = sorted.partition_point(|&v| v < r) as f32 / sorted.len() as f32;
            rank.powf(0.85)
        })
        .collect();
    PROGRESS.store(70, Ordering::Relaxed);
    // ---- 6: rivers from accumulated rain along the drainage tree
    let mut flow: Vec<f32> = (0..n).map(|i| if hm[i] > 0.0 { 0.2 + rain[i] } else { 0.0 }).collect();
    for &i in order.iter().rev() {
        let r = recv[i as usize];
        if r != u32::MAX {
            let f = flow[i as usize];
            flow[r as usize] += f;
        }
    }
    let land_flows: Vec<f32> = (0..n).filter(|&i| hm[i] > 0.0 && !lake[i]).map(|i| flow[i]).collect();
    let thr = if land_flows.is_empty() { f32::MAX } else { quantile(&land_flows, 0.975).max(big * 0.25) };
    let mut river = vec![0u8; n]; // 0 none, else width class
    let mut springs = vec![];
    for i in 0..n {
        if hm[i] > 0.0 && !lake[i] && flow[i] > thr {
            let wc = if flow[i] > thr * 40.0 { 3 } else if flow[i] > thr * 8.0 { 2 } else { 1 };
            river[i] = wc;
            // a source: no upstream river cell flows into it
            let (x, y) = ((i % w) as i32, (i / w) as i32);
            let fed = N8.iter().any(|&(dx, dy)| {
                let (nx, ny) = (x + dx, y + dy);
                nx >= 0 && ny >= 0 && nx < w as i32 && ny < h as i32 && {
                    let j = ny as usize * w + nx as usize;
                    recv[j] == i as u32 && flow[j] > thr
                }
            });
            if !fed {
                springs.push(i as u32);
            }
        }
    }
    // widen big rivers and carve their valleys
    let mut water_mask = vec![0u8; n]; // 1 ocean, 2 lake, 3 river
    for i in 0..n {
        water_mask[i] = if hm[i] <= 0.0 { 1 } else if lake[i] { 2 } else if river[i] > 0 { 3 } else { 0 };
    }
    for i in 0..n {
        let wc = river[i] as i32;
        if wc >= 2 {
            let (x, y) = ((i % w) as i32, (i / w) as i32);
            let r = wc - 1;
            for dy in -r..=r {
                for dx in -r..=r {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx >= 0 && ny >= 0 && nx < w as i32 && ny < h as i32 && dx * dx + dy * dy <= r * r + 1 {
                        let j = ny as usize * w + nx as usize;
                        if water_mask[j] == 0 {
                            water_mask[j] = 3;
                        }
                    }
                }
            }
        }
    }
    let rv: Vec<f32> = (0..n).map(|i| if water_mask[i] == 3 { 1.0 } else { 0.0 }).collect();
    let valley = box_blur(&rv, w, h, 6);
    for i in 0..n {
        if hm[i] > 0.0 && !lake[i] {
            hm[i] = (hm[i] - valley[i] * 0.05 * hm[i].min(0.6)).max(0.002);
        }
    }
    let near_river = box_blur(&rv, w, h, 4);
    let lk: Vec<f32> = (0..n).map(|i| if water_mask[i] >= 2 { 1.0 } else { 0.0 }).collect();
    let near_fresh = box_blur(&lk, w, h, 8);
    PROGRESS.store(80, Ordering::Relaxed);
    // ---- 7: temperature, moisture, biomes, colours
    let tnoise: Vec<f32> = (0..n).into_par_iter().map(|i| fbm((i % w) as f32 * cont_f * 4.0, (i / w) as f32 * cont_f * 4.0, 3, seed ^ 0x123) - 0.5).collect();
    let mut out = World {
        terr: vec![0; n],
        elevation: vec![0; n],
        moisture: vec![0; n],
        tmp: vec![0; n],
        fert: vec![0.0; n],
        bg: vec![[0; 3]; n],
        biome: vec![0; n],
        springs,
        land: 0,
    };
    let sun = (-0.6f32, -0.75f32, 0.45f32); // light from the north-west, above
    let sl = (sun.0 * sun.0 + sun.1 * sun.1 + sun.2 * sun.2).sqrt();
    for i in 0..n {
        let (x, y) = (i % w, i / w);
        let e = hm[i];
        let lat = y as f32 / hf; // 0 north (cold) .. 1 south (hot)
        let land_h = e.max(0.0);
        // temperature: latitude, lapse rate with altitude, the ocean moderates extremes
        let mut t = 0.04 + 0.92 * lat - 0.62 * land_h + tnoise[i] * 0.08;
        t += (0.5 - t) * 0.22 * near_ocean[i];
        let t = t.clamp(0.02, 1.0);
        out.tmp[i] = (t * 255.0) as u8;
        // elevation byte: sea level SEA
        out.elevation[i] = if e > 0.0 { (SEA as f32 + e.min(1.0) * (255 - SEA) as f32) as u8 } else { (SEA as f32 * (1.0 + e / 0.6).max(0.0)) as u8 };
        let moist = (0.9 * rain[i] + 0.3 * near_river[i] + 0.2 * near_fresh[i]).clamp(0.0, 1.0);
        // slope for hill-shading (height exaggerated for relief)
        let hx = |xx: usize, yy: usize| hm[yy * w + xx].max(0.0);
        let (xl, xr, yu, yd) = (x.saturating_sub(1), (x + 1).min(w - 1), y.saturating_sub(1), (y + 1).min(h - 1));
        let ex = 70.0;
        let gx = (hx(xr, y) - hx(xl, y)) * ex;
        let gy = (hx(x, yd) - hx(x, yu)) * ex;
        let slope = (gx * gx + gy * gy).sqrt();
        let nl = (gx * gx + gy * gy + 1.0).sqrt();
        let lit = ((-gx * sun.0 - gy * sun.1 + sun.2) / (nl * sl)).clamp(0.0, 1.0);
        let shade = 0.55 + 0.75 * (lit - sun.2 / sl);
        let biome = match water_mask[i] {
            1 => Biome::Ocean,
            2 => Biome::Lake,
            3 => Biome::River,
            _ => {
                // wavy, natural borders between biomes: classify on slightly perturbed climate
                let wob = (fbm(x as f32 / 16.0, y as f32 / 16.0, 3, seed ^ 0xb10e) - 0.5) * 0.16;
                let (t, moist) = (t + wob * 0.6, moist + wob);
                let snowline = 0.62 + 0.5 * (t - 0.3);
                if land_h > snowline.max(0.45) || t < 0.07 {
                    Biome::Glacier
                } else if land_h > 0.4 && slope > 0.9 || land_h > 0.6 {
                    Biome::Mountain
                } else if land_h > 0.26 {
                    Biome::Alpine
                } else if land_h < 0.0012 && near_ocean[i] > 0.15 {
                    Biome::Beach
                } else if t < 0.2 {
                    Biome::Tundra
                } else if t < 0.36 {
                    if moist > 0.3 { Biome::Taiga } else { Biome::Tundra }
                } else if t < 0.68 {
                    if moist < 0.17 {
                        Biome::Desert
                    } else if moist < 0.36 {
                        Biome::Grassland
                    } else if moist > 0.82 && land_h < 0.12 {
                        Biome::Swamp
                    } else {
                        Biome::TemperateForest
                    }
                } else if moist < 0.24 {
                    Biome::Desert
                } else if moist < 0.46 {
                    Biome::Savanna
                } else if moist > 0.85 && land_h < 0.1 {
                    Biome::Swamp
                } else {
                    Biome::Rainforest
                }
            }
        };
        out.biome[i] = biome as u8;
        let nz = (hash(x as i32, y as i32, seed ^ 0x5151) - 0.5) * 7.0;
        let jitter = |c: Col, k: f32| -> Col {
            [
                (c[0] as f32 * k + nz).clamp(0.0, 255.0) as u8,
                (c[1] as f32 * k + nz).clamp(0.0, 255.0) as u8,
                (c[2] as f32 * k + nz * 0.8).clamp(0.0, 255.0) as u8,
            ]
        };
        match biome {
            Biome::Ocean => {
                let depth = (-e / 0.6).clamp(0.0, 1.0);
                let mut c = mix([46, 116, 160], [14, 38, 86], depth.sqrt());
                if t < 0.12 {
                    c = mix(c, [205, 225, 238], 0.7); // sea ice
                }
                out.terr[i] = 1;
                out.bg[i] = jitter(c, 1.0);
            }
            Biome::Lake | Biome::River => {
                let c = if biome == Biome::Lake { [40, 98, 148] } else { [52, 118, 166] };
                let c = if t < 0.12 { mix(c, [210, 228, 240], 0.75) } else { c };
                out.terr[i] = 1;
                out.bg[i] = jitter(c, 1.0);
            }
            _ => {
                out.land += 1;
                out.terr[i] = match biome {
                    Biome::Mountain => 2,
                    Biome::Beach | Biome::Desert => 3,
                    _ => 0,
                };
                out.fert[i] = (biome.fertility() * (0.75 + 0.5 * moist) * (0.85 + 0.3 * hash(x as i32, y as i32, seed ^ 0x777))).min(1.0);
                // the land is drawn flat: only mountains rise (relief shading and rock tones fade in with height)
                let relief = smooth(0.16, 0.3, land_h);
                let mut c = biome.ground();
                if biome == Biome::Mountain {
                    c = mix(c, [150, 145, 140], ((land_h - 0.55) * 2.0).clamp(0.0, 1.0));
                } else if biome != Biome::Glacier {
                    c = mix(c, [140, 134, 118], relief * 0.45);
                }
                // plains keep a faint hint of their gentle undulation; mountains get the full relief
                let k = 1.0 + (shade - 1.0) * (0.18 + 0.82 * relief);
                out.bg[i] = jitter(c, k);
            }
        }
        out.moisture[i] = match out.terr[i] {
            1 => 255,
            _ => (moist * 255.0) as u8,
        };
    }
    PROGRESS.store(100, Ordering::Relaxed);
    out
}
