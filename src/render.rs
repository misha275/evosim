//! Software pixel renderer at world resolution (one pixel per cell); the GPU then scales it.
use crate::genome::*;
use crate::sim::*;
use crate::util::*;
use crate::vegetation::*;
use rand::{Rng as _, SeedableRng};
use rayon::prelude::*;
use rustc_hash::FxHashMap;

pub const BONE: Col = [232, 226, 196];
pub const WHITE: Col = [245, 245, 245];

/// What the base map layer shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapMode {
    Terrain,
    Temperature,
    Moisture,
    Biome,
    Elevation,
}

impl MapMode {
    pub const ALL: [MapMode; 5] = [MapMode::Terrain, MapMode::Temperature, MapMode::Moisture, MapMode::Biome, MapMode::Elevation];
    pub fn name(self) -> &'static str {
        match self {
            MapMode::Terrain => "terrain",
            MapMode::Temperature => "temperature",
            MapMode::Moisture => "soil moisture",
            MapMode::Biome => "biomes",
            MapMode::Elevation => "elevation",
        }
    }
    pub fn next(self) -> MapMode {
        let i = Self::ALL.iter().position(|&m| m == self).unwrap();
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

#[inline]
fn mixc(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t) as u8
}

/// Multi-stop colour ramp, t in 0..1.
pub fn ramp(stops: &[Col], t: f32) -> Col {
    let t = t.clamp(0.0, 1.0) * (stops.len() - 1) as f32;
    let k = (t as usize).min(stops.len() - 2);
    mix(stops[k], stops[k + 1], t - k as f32)
}

pub const TEMP_RAMP: [Col; 6] = [[40, 60, 170], [70, 160, 230], [120, 210, 160], [235, 225, 90], [240, 140, 50], [200, 40, 40]];
pub const MOIST_RAMP: [Col; 5] = [[170, 120, 60], [215, 195, 110], [120, 180, 90], [40, 130, 110], [30, 70, 170]];
pub const ELEV_RAMP: [Col; 6] = [[70, 140, 80], [150, 180, 95], [215, 200, 120], [165, 120, 80], [140, 130, 125], [250, 250, 250]];

/// Temperature in degrees C shown for a 0..1 climate value.
pub fn celsius(t: f32) -> f32 {
    t * 60.0 - 20.0
}

/// Elevation in metres for the elevation byte (sea level = worldgen::SEA).
pub fn metres(e: u8) -> f32 {
    let sea = crate::worldgen::SEA as f32;
    if e as f32 >= sea {
        (e as f32 - sea) * 22.0
    } else {
        (e as f32 - sea) * 45.0
    }
}

pub fn overlay_color(s: &Sim, i: usize, mode: MapMode) -> Col {
    let water = s.terr[i] == 1;
    // a little of the terrain shading keeps relief readable under the overlays
    let lum = {
        let b = s.bg[i];
        (b[0] as f32 * 0.3 + b[1] as f32 * 0.55 + b[2] as f32 * 0.15) / 140.0
    };
    let shade = |c: Col| -> Col { if water { c } else { crate::util::scale(c, (0.75 + 0.35 * lum).min(1.15)) } };
    match mode {
        MapMode::Terrain => s.bg[i],
        MapMode::Temperature => {
            let c = ramp(&TEMP_RAMP, (celsius(s.temp_at(i)) + 20.0) / 60.0);
            if water { mix(c, [20, 40, 90], 0.35) } else { shade(c) }
        }
        MapMode::Moisture => {
            if water {
                [25, 55, 140]
            } else {
                shade(ramp(&MOIST_RAMP, s.moisture[i] as f32 / 255.0))
            }
        }
        MapMode::Biome => {
            let b = crate::worldgen::Biome::from_u8(s.biome.get(i).copied().unwrap_or(6));
            shade(b.map_col())
        }
        MapMode::Elevation => {
            let m = metres(s.elevation[i]);
            if m < 0.0 {
                mix([60, 120, 190], [10, 25, 70], (-m / 4000.0).min(1.0))
            } else {
                shade(ramp(&ELEV_RAMP, m / 3500.0))
            }
        }
    }
}

pub struct Canvas<'a> {
    pub w: i32,
    pub h: i32,
    pub px: &'a mut [[u8; 4]],
}

impl Canvas<'_> {
    #[inline]
    pub fn set(&mut self, x: i32, y: i32, c: Col) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            self.px[(y * self.w + x) as usize] = [c[0], c[1], c[2], 255];
        }
    }
    #[inline]
    pub fn set4(&mut self, x: i32, y: i32, c: [u8; 4]) {
        if c[3] == 0 {
            return;
        }
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            self.px[(y * self.w + x) as usize] = c;
        }
    }
    pub fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, c: Col) {
        let (x0, y0, x1, y1) = (x.max(0), y.max(0), (x + w).min(self.w), (y + h).min(self.h));
        for yy in y0..y1 {
            let row = (yy * self.w) as usize;
            for xx in x0..x1 {
                self.px[row + xx as usize] = [c[0], c[1], c[2], 255];
            }
        }
    }
    pub fn blend(&mut self, x: i32, y: i32, w: i32, h: i32, c: Col, a: u8) {
        let (x0, y0, x1, y1) = (x.max(0), y.max(0), (x + w).min(self.w), (y + h).min(self.h));
        let af = a as u32;
        let ia = 255 - af;
        for yy in y0..y1 {
            let row = (yy * self.w) as usize;
            for xx in x0..x1 {
                let p = &mut self.px[row + xx as usize];
                for k in 0..3 {
                    p[k] = ((p[k] as u32 * ia + c[k] as u32 * af) / 255) as u8;
                }
            }
        }
    }
    pub fn circle(&mut self, cx: i32, cy: i32, r: i32, c: Col) {
        // pygame.draw.circle-like filled disc
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy <= r * r + r / 2 {
                    self.set(cx + dx, cy + dy, c);
                }
            }
        }
    }
}

// ---------------------------------------------------------------- plant sprites
pub struct Sprite {
    pub w: i32,
    pub h: i32,
    pub ax: i32,
    pub ay: i32,
    pub px: Vec<[u8; 4]>,
}

/// Seasonal look of a lineage: 0 = in leaf, 1 = autumn colours, 2 = bare (deciduous plants only).
pub fn leaf_phase(p: &PlantLineage, t: i64) -> u8 {
    if p.g[DECIDUOUS] < 0.5 {
        return 0;
    }
    match (t % SEASON) * 8 / SEASON {
        5 => 1,     // late autumn
        6 | 7 => 2, // winter
        _ => 0,
    }
}

fn new_sprite(w: i32, h: i32) -> Vec<[u8; 4]> {
    vec![[0u8; 4]; (w * h) as usize]
}

pub fn plant_sprite(p: &PlantLineage, growth: u8, phase: u8) -> Sprite {
    let mut rng = Rng::seed_from_u64(p.id as u64 * 7919 + growth as u64);
    let g = &p.g;
    let form = p.form();
    let age = if growth < 3 { 0.7 } else { 1.0 };
    let rf = |r: &mut Rng| r.gen::<f32>();
    let leafc = |h: f32, s: f32, v: f32| -> Col {
        match phase {
            1 => hsv(18.0 + (g[HUE] * 7.0) % 30.0, 75.0, v + 15.0),
            _ => hsv(h, s, v),
        }
    };
    let bloom = hsv(g[BLOOM_HUE], 70.0, 95.0);
    let toxic = g[TOXIN] > 0.5;
    let berry = hsv(270.0 + g[TOXIN] * 40.0, 80.0, 75.0);
    match form {
        Form::Kelp => {
            let h = ((5.0 + g[HEIGHT] * 12.0) * age) as i32;
            let (sw, sh) = (7, h + 1);
            let mut px = new_sprite(sw, sh);
            let mut cv = Canvas { w: sw, h: sh, px: &mut px };
            let (col, dark) = (hsv(g[HUE], 60.0, 50.0), hsv(g[HUE], 70.0, 35.0));
            for strand in 0..(1 + (g[BRANCHING] * 2.0) as i32) {
                let x = 3 + strand - 1;
                for y in 0..h {
                    let xx = x + ((y as f32 * 0.6 + strand as f32).sin() * (1.0 + g[DROOP])).round() as i32;
                    cv.set(xx, sh - 1 - y, if y % 3 != 0 { col } else { dark });
                }
            }
            return Sprite { w: sw, h: sh, ax: 3, ay: sh - 1, px };
        }
        Form::Reed => {
            // a clump of stalks with brown seed heads, at the waterline
            let h = ((4.0 + g[HEIGHT] * 8.0) * age) as i32;
            let (sw, sh) = (7, h + 2);
            let mut px = new_sprite(sw, sh);
            let mut cv = Canvas { w: sw, h: sh, px: &mut px };
            let stalk = leafc(g[HUE] * 0.3 + 70.0, 45.0, 55.0);
            let n = 2 + (g[BRANCHING] * 3.0) as i32;
            for k in 0..n {
                let x = 1 + (k * 5) / n.max(1);
                let hh = h - (rf(&mut rng) * 3.0) as i32;
                for y in 0..hh {
                    cv.set(x + (y > hh - 3 && k % 2 == 0) as i32, sh - 1 - y, stalk);
                }
                cv.fill(x, sh - hh - 1, 1, 2, [120, 78, 45]);
            }
            return Sprite { w: sw, h: sh, ax: 3, ay: sh - 1, px };
        }
        Form::Cactus => {
            let h = ((4.0 + g[HEIGHT] * 9.0) * age) as i32;
            let (sw, sh) = (9, h + 1);
            let mut px = new_sprite(sw, sh);
            let mut cv = Canvas { w: sw, h: sh, px: &mut px };
            let body = hsv(95.0 + (g[HUE] - 105.0) * 0.3, 45.0, 52.0);
            let dark = hsv(100.0, 50.0, 36.0);
            let base = sh - 1;
            cv.fill(3, base - h + 1, 3, h, body);
            cv.fill(5, base - h + 1, 1, h, dark);
            let arms = (g[BRANCHING] * 3.0) as i32;
            for a in 0..arms.min(2) {
                let side = if a == 0 { -1 } else { 1 };
                let ay = base - h / 2 + (rf(&mut rng) * 3.0) as i32 - 1;
                let ax = if side < 0 { 1 } else { 6 };
                cv.fill(ax, ay, 2, 1, body);
                let up = 2 + (rf(&mut rng) * (h as f32 / 3.0)) as i32;
                cv.fill(if side < 0 { 1 } else { 7 }, ay - up, 1, up, body);
            }
            if g[THORNS] > 0.3 {
                for y in (base - h + 1..base).step_by(2) {
                    cv.set(2, y, [230, 225, 200]);
                    cv.set(6, y + 1, [230, 225, 200]);
                }
            }
            if g[BLOOM] > 0.35 && growth >= 3 {
                cv.fill(3, base - h, 3, 1, bloom);
            }
            return Sprite { w: sw, h: sh, ax: 4, ay: base, px };
        }
        Form::Vine => {
            // twisting climber with leaves
            let h = ((5.0 + g[HEIGHT] * 12.0) * age) as i32;
            let (sw, sh) = (7, h + 1);
            let mut px = new_sprite(sw, sh);
            let mut cv = Canvas { w: sw, h: sh, px: &mut px };
            let stem = hsv(80.0, 40.0, 35.0);
            let leaf = leafc(g[HUE], 60.0, 50.0);
            for y in 0..h {
                let x = 3 + ((y as f32 * 0.7).sin() * 1.6).round() as i32;
                cv.set(x, sh - 1 - y, stem);
                if y % 3 == 1 && phase != 2 {
                    cv.set(x + if y % 6 == 1 { 1 } else { -1 }, sh - 1 - y, leaf);
                }
                if g[BLOOM] > 0.4 && growth >= 3 && y % 7 == 5 {
                    cv.set(x, sh - 2 - y, bloom);
                }
            }
            return Sprite { w: sw, h: sh, ax: 3, ay: sh - 1, px };
        }
        Form::Flytrap => {
            let (sw, sh) = (7, 5);
            let mut px = new_sprite(sw, sh);
            let mut cv = Canvas { w: sw, h: sh, px: &mut px };
            let leaf = hsv(g[HUE], 55.0, 45.0);
            cv.fill(1, 4, 5, 1, leaf);
            for (x, y) in [(0, 2), (3, 1), (6, 2)] {
                cv.set(x, y + 1, leaf);
                cv.set(x, y, [200, 40, 50]);
                cv.set(x + if x < 3 { 1 } else if x > 3 { -1 } else { 0 }, y - 1 + (x == 3) as i32, [235, 230, 200]);
            }
            return Sprite { w: sw, h: sh, ax: 3, ay: 4, px };
        }
        Form::Flower => {
            let h = ((2.0 + g[HEIGHT] * 6.0) * age) as i32;
            let (sw, sh) = (5, h + 3);
            let mut px = new_sprite(sw, sh);
            let mut cv = Canvas { w: sw, h: sh, px: &mut px };
            let stem = hsv(g[HUE], 50.0, 45.0);
            cv.fill(2, sh - h, 1, h, stem);
            cv.set(1, sh - 2, stem);
            if growth >= 3 && phase != 2 {
                let top = sh - h - 1;
                for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    cv.set(2 + dx, top + dy, bloom);
                }
                cv.set(2, top, [250, 220, 90]);
            }
            return Sprite { w: sw, h: sh, ax: 2, ay: sh - 1, px };
        }
        Form::Conifer => {
            let h = ((8.0 + g[HEIGHT] * 14.0) * age) as i32;
            let half = ((2.0 + g[BRANCHING] * 4.0 + g[DENSITY] * 2.0) * age) as i32;
            let (sw, sh) = (2 * half + 3, h + 2);
            let mut px = new_sprite(sw, sh);
            let mut cv = Canvas { w: sw, h: sh, px: &mut px };
            let (cx, base) = (sw / 2, sh - 1);
            let needle = hsv(g[HUE] * 0.4 + 90.0, 55.0, 30.0 + g[DENSITY] * 12.0);
            let light = hsv(g[HUE] * 0.4 + 95.0, 45.0, 45.0 + g[DENSITY] * 10.0);
            cv.fill(cx, base - 2, 1, 3, [92, 64, 40]);
            let tiers = 3 + (g[BRANCHING] * 3.0) as i32;
            for y in 0..h - 2 {
                let fy = y as f32 / (h - 2) as f32; // 0 bottom .. 1 top
                let tier_pos = (fy * tiers as f32).fract();
                let wdt = ((1.0 - fy) * half as f32 * (0.55 + 0.45 * (1.0 - tier_pos))).round() as i32;
                let yy = base - 2 - y;
                cv.fill(cx - wdt, yy, 2 * wdt + 1, 1, needle);
                if wdt > 0 {
                    cv.set(cx - wdt, yy, light);
                }
            }
            cv.set(cx, base - h + 1, light);
            if toxic {
                cv.set(cx + 1, base - h / 2, berry);
            }
            return Sprite { w: sw, h: sh, ax: cx, ay: base, px };
        }
        Form::Palm => {
            let h = ((8.0 + g[HEIGHT] * 12.0) * age) as i32;
            let fr = ((4.0 + g[CROWN] * 5.0) * age) as i32;
            let (sw, sh) = (2 * fr + 5, h + fr / 2 + 3);
            let mut px = new_sprite(sw, sh);
            let mut cv = Canvas { w: sw, h: sh, px: &mut px };
            let (cx, base) = (sw / 2, sh - 1);
            let bend = (g[DROOP] * 3.0) as i32;
            let mut topx = cx;
            for y in 0..h {
                let x = cx + (bend as f32 * (y as f32 / h as f32).powi(2)) as i32;
                cv.set(x, base - y, if y % 2 == 0 { [120, 92, 60] } else { [96, 72, 46] });
                topx = x;
            }
            let top = base - h;
            let frond = leafc(g[HUE], 60.0, 42.0);
            for k in 0..(4 + (g[DENSITY] * 3.0) as i32) {
                let a = std::f32::consts::PI * (0.05 + 0.9 * k as f32 / (3.0 + g[DENSITY] * 3.0));
                for t in 0..fr {
                    let x = topx as f32 - a.cos() * t as f32;
                    let y = top as f32 - a.sin() * t as f32 * 0.6 + (t * t) as f32 * 0.08;
                    cv.set(x as i32, y as i32, frond);
                }
            }
            if g[BLOOM] > 0.4 && growth >= 3 {
                cv.fill(topx - 1, top + 1, 3, 1, hsv(30.0, 70.0, 55.0)); // coconuts
            }
            return Sprite { w: sw, h: sh, ax: cx, ay: base, px };
        }
        _ => {}
    }
    // broadleaf tree / shrub
    let tree = form == Form::Tree;
    let trunk_h = ((if tree { 3.0 + g[HEIGHT] * 9.0 } else { 1.0 + g[HEIGHT] * 2.0 }) * age) as i32;
    let cw: f32 = ((2.0 + g[CROWN] * 6.0 + g[BRANCHING] * 2.0) * if tree { 1.0 } else { 0.7 } * age).max(1.5);
    let ch: f32 = ((3.0 + (1.0 - g[CROWN]) * 7.0 + g[HEIGHT] * 3.0) * if tree { 1.0 } else { 0.55 } * age).max(2.0);
    let sw = (2.0 * cw + 5.0 + g[DROOP] * 4.0) as i32;
    let sh = (trunk_h as f32 + ch + 3.0 + g[DROOP] * ch) as i32;
    let mut px = new_sprite(sw, sh);
    let mut cv = Canvas { w: sw, h: sh, px: &mut px };
    let (cx, base) = (sw / 2, sh - 1);
    let bark_v = if g[FIRE_RES] > 0.5 { 30.0 } else { 38.0 }; // thick dark bark
    let (bark, bark2) = (hsv(25.0 + g[HUE] * 0.05, 50.0, bark_v), hsv(25.0, 45.0, 26.0));
    let girth = 1 + (g[GIRTH] > 0.45) as i32 + (g[GIRTH] > 0.8 && tree) as i32;
    for y in 0..(trunk_h + (ch * 0.4) as i32) {
        for k in 0..girth {
            cv.set(cx - girth / 2 + k, base - y, if k == 0 { bark2 } else { bark });
        }
    }
    if girth > 1 {
        cv.set(cx - girth / 2 - 1, base, bark2);
        cv.set(cx + girth - girth / 2, base, bark2);
    }
    let top = base - trunk_h;
    let limbs = 1 + (g[BRANCHING] * 4.0) as i32 + if phase == 2 { 2 } else { 0 };
    for b in 0..limbs {
        let side = if b % 2 == 1 { -1 } else { 1 };
        let by = top - (rf(&mut rng) * ch * 0.6) as i32;
        let ln = (cw * (0.4 + rf(&mut rng) * 0.6)) as i32;
        for t in 0..ln {
            let x = cx + side * t;
            let y = by - (t as f32 * (1.0 - g[CROWN]) * 0.8) as i32 + (t as f32 * g[DROOP] * 0.5) as i32;
            cv.set(x, y, bark);
        }
    }
    if phase == 2 {
        // bare winter crown: a fan of branches with twigs, dusted with snow
        let n = 5 + (g[BRANCHING] * 4.0) as i32;
        for b in 0..n {
            let a = std::f32::consts::PI * (0.12 + 0.76 * b as f32 / (n - 1) as f32);
            let len = ch * 0.95 * (0.7 + 0.3 * rf(&mut rng));
            let (mut ex, mut ey) = (cx as f32, top as f32);
            for t in 0..len as i32 {
                ex = cx as f32 - a.cos() * t as f32 * (cw / len.max(1.0)).max(0.6);
                ey = top as f32 - a.sin() * t as f32 + (t as f32 * g[DROOP] * 0.4);
                cv.set(ex as i32, ey as i32, bark);
            }
            cv.set(ex as i32 + 1, ey as i32 - 1, bark2);
            cv.set(ex as i32, ey as i32 - 1, [225, 232, 238]);
        }
    }
    let leaf = leafc(g[HUE], 50.0 + g[DENSITY] * 25.0, 40.0 + g[DENSITY] * 15.0);
    let light = leafc(g[HUE] + 8.0, 40.0 + g[DENSITY] * 20.0, 62.0 + g[DENSITY] * 15.0);
    let shadow = leafc(g[HUE] - 6.0, 65.0, 26.0 + g[DENSITY] * 8.0);
    let ccy = top as f32 - ch * 0.55;
    let clusters = (6.0 + g[DENSITY] * 14.0 + g[BRANCHING] * 6.0) as i32;
    for _ in 0..clusters {
        let a = rf(&mut rng) * std::f32::consts::TAU;
        let r = rf(&mut rng).powf(0.6);
        let mut dx = a.cos() * r * cw;
        let vy = a.sin() * r * ch * 0.5;
        if g[CROWN] < 0.4 && vy < 0.0 {
            dx *= (0.5 + vy / ch + (1.0 - g[CROWN]) * 0.2).max(0.15);
        }
        let (x, y) = ((cx as f32 + dx) as i32, (ccy + vy) as i32);
        let rad = 1.max((1.0 + g[DENSITY] * 2.0 * rf(&mut rng) + if tree { 1.0 } else { 0.0 }) as i32);
        let roll = rf(&mut rng);
        if phase == 2 {
            continue; // bare winter branches
        }
        cv.circle(x, y + 1, rad, shadow);
        cv.circle(x, y, rad, leaf);
        if (y as f32) < ccy && roll < 0.6 {
            cv.set(x - rad / 2, y - rad / 2, light);
        }
        if g[DROOP] > 0.45 && rf(&mut rng) < g[DROOP] {
            for t in 0..(g[DROOP] * ch * 0.9) as i32 {
                let yy = y + rad + t;
                if yy < base - 1 {
                    cv.set(x, yy, if t % 2 == 1 { leaf } else { shadow });
                }
            }
        }
        if g[THORNS] > 0.5 && rf(&mut rng) < g[THORNS] {
            cv.set(x + rad, y, [225, 220, 195]); // thorn tips at the crown edge
        }
    }
    if phase != 2 {
        if g[BLOOM] > 0.35 && growth >= 3 {
            for _ in 0..(g[BLOOM] * 10.0) as i32 {
                let a = rf(&mut rng) * std::f32::consts::TAU;
                let r = rf(&mut rng).sqrt();
                cv.set((cx as f32 + a.cos() * r * cw) as i32, (ccy + a.sin() * r * ch * 0.45) as i32, bloom);
            }
        }
        if toxic && growth >= 3 {
            for _ in 0..(g[TOXIN] * 5.0) as i32 {
                let a = rf(&mut rng) * std::f32::consts::TAU;
                let r = rf(&mut rng).sqrt() * 0.8;
                cv.set((cx as f32 + a.cos() * r * cw) as i32, (ccy + a.sin() * r * ch * 0.4) as i32, berry);
            }
        }
    }
    Sprite { w: sw, h: sh, ax: cx, ay: base, px }
}

// ---------------------------------------------------------------- creature sprites
pub fn draw_cr(cv: &mut Canvas, c: &Cr, t: i64, dark: f32, cx: f32, cy: f32) {
    let g = &c.g;
    let f = c.f as i32;
    if g.v[COMPLEXITY] < 0.22 {
        let (x, y) = (cx as i32, cy as i32);
        cv.fill(x, y, 2, 2, c.col);
        cv.set(x, y, c.lt);
        return;
    }
    let cy = cy - c.z;
    let gr = (c.age / g.v[MAT]).min(1.0);
    let juv = gr < 1.0;
    let w = 4.max((c.s * 0.8).round() as i32);
    let h = 3.max((w as f32 * g.v[BODY]).round() as i32) + (g.has(GLUTTON) && w >= 4) as i32;
    let legl = 1 + (g.v[LEGS] > 0.7) as i32;
    let x0 = cx as i32 - w / 2;
    let y0 = cy as i32 - (h + legl) / 2;
    let (col, dk, lt, c2) = (c.col, c.dk, c.lt, c.c2);
    let moving = c.vx.abs() + c.vy.abs() > 0.08;
    let ph = if moving { ((t / 4) & 1) as i32 } else { 0 };
    cv.blend(x0, y0 + h + legl, w, 1, [0, 0, 0], 80);
    let lc = if g.has(SWIMMER) { [60, 200, 220] } else { dk };
    cv.fill(x0 + 1 + ph, y0 + h, 1, legl, lc);
    cv.fill(x0 + w - 2 - ph, y0 + h, 1, legl, lc);
    let tl = g.v[TAIL] * (0.4 + 0.6 * gr);
    if tl > 0.2 {
        let n = 1 + (tl * 3.0) as i32;
        let tx = if f > 0 { x0 - 1 } else { x0 + w };
        for i in 0..n {
            let wag = (i == n - 1 && n > 1 && (t / 7) & 1 == 1) as i32;
            let up = (tl > 0.8 && i == n - 1) as i32;
            cv.set(tx - f * i, y0 + 1 - wag - up, if i == n - 1 { c2 } else { col });
        }
    }
    if w >= 4 {
        cv.fill(x0 + 1, y0, w - 2, h, col);
        cv.fill(x0, y0 + 1, w, h - 2, col);
    } else {
        cv.fill(x0, y0, w, h, col);
    }
    match g.pat {
        1 => {
            let mut xx = x0 + 1;
            while xx < x0 + w - 1 {
                cv.fill(xx, y0, 1, h, c2);
                xx += 2;
            }
        }
        2 => {
            cv.set(x0 + 1, y0 + 1, c2);
            cv.set(x0 + w / 2, y0, c2);
            if w >= 5 {
                cv.set(x0 + w - 2, y0 + h - 2, c2);
            }
        }
        3 => cv.fill(x0 + 1, y0 + h - 1, w - 2, 1, lt),
        4 => cv.fill(if f > 0 { x0 + w - 2 } else { x0 }, y0, 2, h, c2),
        5 => cv.fill(x0 + 1, y0, w - 2, 1, c2),
        _ => {}
    }
    if g.has(THICK_HIDE) {
        cv.fill(x0 + 1, y0 + h - 1, w - 2, 1, dk);
    }
    if g.has(VENOMOUS) {
        cv.set(x0 + 1, y0, [110, 230, 90]);
        cv.set(x0 + w - 2, y0 + h - 1, [110, 230, 90]);
    }
    if g.has(PROLIFIC) {
        cv.set(x0 + w / 2, y0 + h - 1, [255, 170, 200]);
    }
    if c.preg.is_some() {
        cv.set(x0 + w / 2, y0 + h, lt);
    }
    cv.set(x0 + 1 + c.mk as i32 % 1.max(w - 2), y0 + 1, c.mkc);
    let hx = if f > 0 { x0 + w - 1 } else { x0 };
    let ex = if f > 0 { x0 + w - 2 } else { x0 + 1 };
    let ey = y0 + (h >= 3) as i32;
    if w >= 5 {
        cv.set(if f > 0 { x0 + w } else { x0 - 1 }, y0 + h - 2, col);
    }
    let ear = if g.has(SKITTISH) { g.v[EARS].max(0.75) } else { g.v[EARS] };
    let ax = if f > 0 { x0 + w - 2 } else { x0 + 1 };
    if ear > 0.3 {
        cv.set(ax, y0 - 1, col);
        if ear > 0.65 {
            cv.set(ax, y0 - 2, dk);
        }
        if ear > 0.5 && w >= 6 {
            cv.set(if f > 0 { x0 + w - 4 } else { x0 + 3 }, y0 - 1, col);
        }
    }
    let hn = g.v[HORN] * gr;
    if hn > 0.4 && w >= 4 {
        let bx = if f > 0 { x0 + w - 3 } else { x0 + 2 };
        cv.set(bx, y0 - 1, BONE);
        if hn > 0.7 {
            cv.set(bx + f, y0 - 2, BONE);
        }
        if hn > 0.9 && w >= 6 {
            cv.set(bx - 2 * f, y0 - 1, BONE);
        }
    }
    if g.has(SPIKY) {
        let mut xx = x0 + 1;
        while xx < x0 + w - 1 {
            cv.set(xx, y0 - 1, [240, 240, 240]);
            xx += 2;
        }
    }
    if g.has(SWIMMER) {
        cv.set(x0 + if f > 0 { 1 } else { w - 2 }, y0 - 1, [60, 220, 230]);
    }
    let e = g.v[EYES] + if juv { 0.35 } else { 0.0 };
    if c.rest {
        cv.set(ex, ey, dk);
        cv.set(x0 + w, y0 - 3, WHITE);
        cv.set(x0 + w + 1, y0 - 4, WHITE);
    } else {
        let pc = if g.v[DIET] > 0.55 {
            [235, 40, 40]
        } else if g.has(NOCTURNAL) && dark > 0.4 {
            [255, 230, 90]
        } else {
            [15, 15, 25]
        };
        if e > 0.5 && w >= 4 {
            cv.set(ex - f, ey, WHITE);
        }
        cv.set(ex, ey, pc);
        if e > 0.85 && h >= 4 {
            cv.set(ex, ey + 1, pc);
        }
        if g.v[DIET] < 0.3 && w >= 5 {
            cv.set(ex, (y0 + h - 1).min(ey + 2), [255, 150, 170]);
        }
    }
    if g.v[DIET] > 0.6 && w >= 5 {
        cv.set(hx, y0 + h - 1, WHITE);
    }
    if w >= 5 && (g.has(SCAVENGER) || g.has(HERDER)) {
        let sc = if g.has(SCAVENGER) { [190, 110, 230] } else { [120, 170, 255] };
        cv.fill(if f > 0 { x0 + w - 3 } else { x0 + 2 }, y0 + h - 2, 1, 2, sc);
    }
    if g.v[WINGS] > 0.35 {
        let wing = 1.max((g.v[WINGS] * 4.0) as i32);
        let wy = y0 - 1 - if c.z > 0.0 { ((t / 4) & 1) as i32 } else { 0 };
        cv.fill(x0 + w / 2 - wing, wy, wing, 1, c.c2);
        cv.fill(x0 + w / 2 - wing + 1, wy - 1, 1.max(wing - 1), 1, c.lt);
    }
    if g.v[FINS] > 0.35 {
        cv.fill(x0 + 1, y0 + h, 2, 1, c.c2);
        cv.fill(x0 + w - 3, y0 + h, 2, 1, c.c2);
    }
    if g.v[GILLS] > 0.55 {
        cv.set(if f > 0 { x0 + w - 3 } else { x0 + 2 }, y0 + 1, c.dk);
    }
    if c.sick.is_some() {
        cv.set(x0 + w / 2, y0 - 4, [145, 205, 90]);
    }
    let mx = x0 + w / 2;
    if !juv && c.cool > 480 {
        for (dx, dy) in [(-1, -6), (1, -6), (-1, -5), (0, -5), (1, -5), (0, -4)] {
            cv.set(mx + dx, y0 + dy, [255, 90, 130]);
        }
    } else if c.thr.is_some() {
        cv.set(if f > 0 { x0 - 1 } else { x0 + w }, y0 - 1, [150, 220, 255]);
    }
}

/// Renders creature portraits (for panels) into an RGBA buffer.
pub fn portrait(c: &Cr, t: i64, dark: f32, pw: i32, ph: i32) -> Vec<[u8; 4]> {
    let mut px = vec![[30, 38, 34, 255]; (pw * ph) as usize];
    let mut cv = Canvas { w: pw, h: ph, px: &mut px };
    draw_cr(&mut cv, c, t, dark, (pw / 2) as f32, (ph / 2 + 1) as f32);
    px
}

pub struct WorldRenderer {
    pub px: Vec<[u8; 4]>,
    base: Vec<[u8; 4]>,
    base_key: (u64, usize, u8, i64),
    pub sprites: FxHashMap<(u32, u8, u8), Sprite>,
    sprite_epoch: u32,
}

impl WorldRenderer {
    pub fn new() -> Self {
        WorldRenderer { px: vec![], base: vec![], base_key: (u64::MAX, 0, 0, 0), sprites: FxHashMap::default(), sprite_epoch: 0 }
    }

    pub fn reset(&mut self) {
        self.sprites.clear();
        self.base_key = (u64::MAX, 0, 0, 0);
    }

    /// Compose the world at native resolution. `rect` = visible region (x0,y0,x1,y1) to limit work.
    pub fn compose(&mut self, s: &Sim, rect: (i32, i32, i32, i32), sel: Option<u32>, mode: MapMode) {
        let (w, h) = (s.w as i32, s.h as i32);
        let n = s.n;
        if self.px.len() != n {
            self.px = vec![[0, 0, 0, 255]; n];
            self.base = vec![[0, 0, 0, 255]; n];
            self.base_key = (u64::MAX, 0, 0, 0);
        }
        // terrain base layer with sparse land details (rebuilt only when the terrain changes)
        // base layer: terrain colours or a data overlay (rebuilt only when something it shows changes)
        let tbucket = if mode == MapMode::Temperature { (s.d_t * 200.0) as i64 } else { 0 };
        let key = (s.bg_version, s.n, mode as u8, tbucket);
        if self.base_key != key {
            self.base_key = key;
            let base: Vec<[u8; 4]> = (0..n)
                .into_par_iter()
                .map(|i| {
                    let c = overlay_color(s, i, mode);
                    [c[0], c[1], c[2], 255]
                })
                .collect();
            self.base = base;
        }
        let (rx0, ry0, rx1, ry1) = (rect.0.max(0), rect.1.max(0), rect.2.min(w), rect.3.min(h));
        // margins so sprites partially outside the view still draw
        let (mx0, my0, mx1, my1) = ((rx0 - 16).max(0), (ry0 - 4).max(0), (rx1 + 16).min(w), (ry1 + 40).min(h));
        for y in my0..my1 {
            let row = (y * w) as usize;
            for x in mx0..mx1 {
                let i = row + x as usize;
                let st = s.pl[i];
                self.px[i] = if st > 0 && mode == MapMode::Terrain {
                    // ground cover tinted by the lineage's own leaf colour
                    let hue = s.vegetation.lineage(i).g[HUE];
                    let c = hsv(hue, 42.0 + st as f32 * 8.0, 26.0 + st as f32 * 13.0);
                    let b = self.base[i];
                    // algae stay a subtle tint so lakes and seas keep their colour
                    let k = if s.terr[i] == 1 { 0.18 + 0.06 * st as f32 } else { 0.55 + 0.15 * st as f32 };
                    [mixc(b[0], c[0], k), mixc(b[1], c[1], k), mixc(b[2], c[2], k), 255]
                } else {
                    self.base[i]
                };
            }
        }
        // animated water glints
        let mut i = if mode == MapMode::Terrain { (my0 * w + mx0) as usize } else { usize::MAX - 64 };
        let end = if mode == MapMode::Terrain { (my1 * w) as usize } else { 0 };
        i += (43 - i % 43) % 43;
        while i < end {
            let x = (i % s.w) as i32;
            if s.terr[i] == 1 && (s.t / 16 + x as i64) % 7 == 0 && x >= mx0 && x < mx1 {
                self.px[i] = [65, 133, 171, 255];
                if x + 1 < w {
                    self.px[i + 1] = [65, 133, 171, 255];
                }
            }
            i += 43;
        }
        let px = &mut self.px;
        let mut cv = Canvas { w, h, px };
        let a = &s.atmosphere;
        for gy in (my0 as usize / 16)..=((my1 - 1).max(0) as usize / 16).min(a.rows - 1) {
            for gx in (mx0 as usize / 16)..=((mx1 - 1).max(0) as usize / 16).min(a.cols - 1) {
                let amount = a.shade[gy * a.cols + gx];
                if amount > 0.0 {
                    cv.blend(gx as i32 * 16, gy as i32 * 16, 16, 16, [30, 40, 63], (40.0 * amount) as u8);
                }
            }
        }
        let inside = |x: i32, y: i32| x >= mx0 - 8 && x < mx1 + 8 && y >= my0 - 8 && y < my1 + 40;
        for p in &s.poop {
            cv.set(p.x as i32, p.y as i32, [110, 70, 40]);
        }
        for k in &s.cor {
            if !inside(k.x as i32, k.y as i32) {
                continue;
            }
            if k.buried {
                cv.fill(k.x as i32 - 1, k.y as i32, 3, 1, [116, 92, 61]);
                continue;
            }
            let color = mix([90, 65, 55], k.col, (k.ttl / 900.0).max(0.0) * 0.55);
            let (x, y, ww) = (k.x as i32 - 2, k.y as i32 - 1, 3.max(k.size as i32));
            cv.fill(x + 1, y, ww - 2, 3, color);
            cv.fill(x, y + 1, ww, 1, color);
            cv.set(k.x as i32, k.y as i32, BONE);
        }
        for b in &s.bones {
            if b.buried || !inside(b.x as i32, b.y as i32) {
                continue;
            }
            let (x, y) = (b.x as i32, b.y as i32);
            let len = 2.max((b.size * 0.7) as i32);
            cv.fill(x - len / 2, y, len, 1, BONE);
            cv.fill(x - len / 2, y - 1, 1, 3, BONE);
            cv.fill(x + len / 2 - 1, y - 1, 1, 3, BONE);
        }
        for &i in s.ash.keys() {
            cv.set((i as usize % s.w) as i32, (i as usize / s.w) as i32, [48, 43, 38]);
        }
        for &i in s.fire.keys() {
            cv.set((i as usize % s.w) as i32, (i as usize / s.w) as i32, [255, (120 + (s.t + i as i64) % 120) as u8, 35]);
        }
        // trees: visible ones, back to front
        let mut trees: Vec<u32> = if mode != MapMode::Terrain { Vec::new() } else { s
            .vegetation
            .trees
            .iter()
            .copied()
            .filter(|&i| {
                let (x, y) = ((i as usize % s.w) as i32, (i as usize / s.w) as i32);
                x >= mx0 - 12 && x < mx1 + 12 && y >= my0 && y < my1 + 40
            })
            .collect() };
        trees.sort_unstable();
        if self.sprites.len() > 12000 || self.sprite_epoch != s.vegetation.epoch {
            self.sprites.clear();
            self.sprite_epoch = s.vegetation.epoch;
        }
        for i in trees {
            let iu = i as usize;
            let p = s.vegetation.lineage(iu);
            let phase = leaf_phase(p, s.t);
            let key = (p.id, s.pl[iu], phase);
            let spr = self.sprites.entry(key).or_insert_with(|| plant_sprite(p, s.pl[iu], phase));
            let (ox, oy) = ((iu % s.w) as i32 - spr.ax, (iu / s.w) as i32 - spr.ay);
            for yy in 0..spr.h {
                for xx in 0..spr.w {
                    cv.set4(ox + xx, oy + yy, spr.px[(yy * spr.w + xx) as usize]);
                }
            }
        }
        for c in &s.crs {
            if !c.dead && inside(c.x as i32, c.y as i32) {
                draw_cr(&mut cv, c, s.t, s.dark, c.x, c.y);
            }
        }
        if let Some(id) = sel {
            if let Some(&j) = s.idmap.get(&id) {
                let c = &s.crs[j];
                if !c.dead {
                    let r = c.s as i32 + 3;
                    let (x, y) = (c.x as i32, c.y as i32);
                    for k in -r..r {
                        cv.set(x + k, y - r, [255, 255, 80]);
                        cv.set(x + k, y + r - 1, [255, 255, 80]);
                        cv.set(x - r, y + k, [255, 255, 80]);
                        cv.set(x + r - 1, y + k, [255, 255, 80]);
                    }
                    let vr = c.g.v[VIS];
                    let steps = (vr * 6.0) as i32;
                    for k in 0..steps {
                        let a = k as f32 / steps as f32 * std::f32::consts::TAU;
                        cv.set((c.x + a.cos() * vr) as i32, (c.y + a.sin() * vr) as i32, [255, 255, 255]);
                    }
                }
            }
        }
        // weather at native resolution
        for &i in a.snow.keys() {
            cv.set((i as usize % s.w) as i32, (i as usize / s.w) as i32, [207, 224, 230]);
        }
        for cl in &a.clouds {
            let (x, y) = (cl.x as i32, cl.y as i32 - 6);
            let ww = 6.max((cl.radius * 1.5) as i32);
            let dark = cl.water > 18.0;
            let edge = if dark { [94, 112, 128] } else { [147, 166, 175] };
            let body = if dark { [133, 151, 162] } else { [188, 204, 207] };
            let light = if dark { [153, 171, 180] } else { [211, 220, 216] };
            cv.fill(x - ww / 2 + 2, y + 3, ww - 4, 2, edge);
            cv.fill(x - ww / 2, y, ww, 3, body);
            cv.fill(x - ww / 3, y - 2, ww * 2 / 3, 2, body);
            cv.fill(x - ww / 4, y - 3, 2.max(ww / 3), 1, light);
        }
        for d in &a.drops {
            let (x, y) = (d.x as i32, (d.y - d.z) as i32);
            if d.snow {
                cv.set(x, y, [211, 229, 235]);
            } else {
                cv.fill(x, y, 1, 2, [103, 162, 201]);
            }
        }
        for &(x, y, ttl) in &a.splashes {
            cv.fill(x, y, if ttl > 2 { 2 } else { 1 }, 1, [87, 145, 178]);
        }
        if let Some((x, y, _)) = s.bolt {
            for (dx, dy, hh) in [(-2, -19, 6), (-1, -13, 5), (-2, -8, 4), (-1, -4, 4)] {
                cv.fill(x + dx, y + dy, 1, hh, [242, 233, 174]);
            }
        }
        if let Some((x, y, r, _)) = s.quake {
            for n in (1..=20.min(r)).rev() {
                cv.fill(x + n - r / 3, y + n / 3 + (n / 4) % 2, 2, 1, [63, 52, 42]);
            }
        }
        if let Some(m) = &s.meteor {
            // falling meteor marker: a bright streak above the target
            let k = m.ttl.max(0);
            cv.fill(m.x - 1 + k / 4, m.y - 2 - k / 2, 3, 3, [255, 200, 120]);
            cv.fill(m.x + k / 4, m.y - 4 - k / 2, 2, 2, [255, 240, 200]);
        }
    }
}

impl Default for WorldRenderer {
    fn default() -> Self {
        Self::new()
    }
}
