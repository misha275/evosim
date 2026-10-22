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
/// Mirrored painter: local x grows towards the animal's front, y=0 is the ground row.
struct Pt<'c, 'a> {
    cv: &'c mut Canvas<'a>,
    ox: i32,
    oy: i32,
    f: i32,
}

impl Pt<'_, '_> {
    #[inline]
    fn set(&mut self, dx: i32, dy: i32, c: Col) {
        self.cv.set(self.ox + self.f * dx, self.oy + dy, c);
    }
    #[inline]
    fn x(&self, dx: i32, w: i32) -> i32 {
        if self.f > 0 { self.ox + dx } else { self.ox - dx - w + 1 }
    }
    fn fill(&mut self, dx: i32, dy: i32, w: i32, h: i32, c: Col) {
        if w > 0 && h > 0 {
            let x = self.x(dx, w);
            self.cv.fill(x, self.oy + dy, w, h, c);
        }
    }
    fn blend(&mut self, dx: i32, dy: i32, w: i32, h: i32, c: Col, a: u8) {
        if w > 0 && h > 0 {
            let x = self.x(dx, w);
            self.cv.blend(x, self.oy + dy, w, h, c, a);
        }
    }
}

/// Body measurements in pixels: (length, height, leg length, biped, big).
pub fn body_dims(c: &Cr) -> (i32, i32, i32, bool, bool) {
    let g = &c.g;
    let carn = g.v[DIET] > 0.6;
    let biped = g.v[STANCE] > 0.55;
    let mut l = 4.max((c.s * 0.8).round() as i32) + carn as i32;
    if biped {
        l = 4.max((l as f32 * 0.8).round() as i32);
    }
    let big = l >= 8;
    let h = 3.max((l as f32 * g.v[BODY] * if carn { 0.8 } else { 1.0 } * if big { 0.72 } else { 1.0 }).round() as i32)
        + (g.has(GLUTTON) && l >= 4) as i32;
    let mut ll = 1 + (g.v[LEGS] > 0.7) as i32;
    if big {
        ll = ll.max((l as f32 * (0.1 + 0.18 * g.v[LEGS])).round() as i32);
    }
    if biped {
        ll = (ll as f32 * 1.5).round() as i32 + 1;
    }
    (l, h, ll, biped, big)
}

pub fn draw_cr(cv: &mut Canvas, c: &Cr, t: i64, dark: f32, cx: f32, cy: f32, wet: bool) {
    let g = &c.g;
    if g.v[COMPLEXITY] < 0.42 {
        // microbial colonies: 2x1 (horizontal) when small, 2x2 when grown large
        // drawn half-transparent and desaturated so colonies don't steal attention from animals
        let (x, y) = (cx as i32, cy as i32);
        let tall = c.s >= 0.3;
        let dim = |col: Col| -> Col {
            let l = (col[0] as u32 + col[1] as u32 + col[2] as u32) / 3;
            [((col[0] as u32 + l) / 2) as u8, ((col[1] as u32 + l) / 2) as u8, ((col[2] as u32 + l) / 2) as u8]
        };
        cv.blend(x, y, 2, if tall { 2 } else { 1 }, dim(c.col), 120);
        cv.blend(x, y, 1, 1, dim(c.lt), 90);
        if tall && g.v[COMPLEXITY] >= 0.22 {
            cv.blend(x + 1, y + 1, 1, 1, dim(c.c2), 110); // colonial life: a second cell type
        }
        return;
    }
    let gr = (c.age / mat_age(g)).min(1.0);
    let juv = gr < 1.0;
    let carn = g.v[DIET] > 0.6;
    let (l, h, ll0, biped, big) = body_dims(c);
    let ll = if wet { 0 } else { ll0 };
    let (col, dk, lt, c2) = (c.col, c.dk, c.lt, c.c2);
    let pounce = c.act == 2 && !wet;
    let feeding = c.act == 3 && !wet;
    let moving = c.vx.abs() + c.vy.abs() > 0.08;
    let fast = c.act == 1 || c.act == 4;
    let ph = if moving { ((t / if fast { 2 } else { 4 }) & 1) as i32 } else { 0 };
    // swimmers bob gently on the surface instead of standing on it
    let bob = (wet && ((t / 9 + c.id as i64) & 3) == 0) as i32;
    let oy = (cy - c.z) as i32 + (h + ll) / 2 + bob - pounce as i32;
    let mut p = Pt { cv, ox: cx as i32, oy, f: if c.f >= 0 { 1 } else { -1 } };
    let bx0 = -l / 2;
    let bx1 = bx0 + l - 1;
    let btop = -ll - h;
    let bbot = -ll - 1;
    // column tops of the body outline (big bodies are arched, small ones have cut corners)
    let tops: Vec<i32> = (0..l)
        .map(|i| {
            let ins = if big {
                let u = (i as f32 + 0.5) / l as f32;
                ((1.0 - (u * std::f32::consts::PI).sin()) * h as f32 * 0.4).round() as i32
            } else {
                (i == 0 || i == l - 1) as i32
            };
            btop + ins
        })
        .collect();
    let endcut = |i: i32| (i == 0 || i == l - 1) as i32;

    // ---- shadow, ripples, legs
    let lc = if g.has(SWIMMER) { [60, 200, 220] } else { dk };
    let lw = if big { (l / 7).max(2) } else { 1 };
    if wet {
        let rc = [170, 215, 235];
        p.blend(bx0 - 1, 0, l + 2, 1, rc, 150);
        if moving {
            for k in 1..4 {
                p.blend(bx0 - 1 - k, -(k & 1), 1, 1, rc, (170 - k * 40) as u8);
                p.blend(bx0 - 1 - k, k & 1, 1, 1, rc, (130 - k * 30) as u8);
            }
        }
    } else {
        p.blend(bx0, pounce as i32, l, 1, [0, 0, 0], 80);
        let legs: Vec<(i32, i32)> = if biped {
            // two strong hind legs under the hips, striding
            let hip = bx0 + l / 2 - lw;
            if pounce {
                vec![(hip + 2, ll - 1), (hip - 1, ll)]
            } else {
                vec![(hip + ph, ll), (hip + lw + 1 - ph, ll - (moving && ph == 0) as i32)]
            }
        } else if pounce {
            vec![(bx1 + 1, 1), (bx0 - 1, 1)]
        } else if c.act == 1 && moving {
            // galloping: legs stretched out front and back
            vec![(bx0 + 1 - ph - big as i32, ll), (bx1 - lw + 1 + (1 - ph), ll)]
        } else if big {
            // heavy walk: legs lift in turn
            vec![(bx0 + 1, ll - (ph == 1) as i32), (bx1 - lw, ll - (ph == 0 && moving) as i32)]
        } else {
            vec![(bx0 + 1 + ph, ll), (bx1 - 1 - ph, ll)]
        };
        for &(lx, lh) in &legs {
            if pounce && !biped {
                p.fill(lx, bbot - (lx > 0) as i32, lw, 1, lc);
            } else {
                p.fill(lx, -lh, lw, lh, lc);
                if big {
                    p.fill(lx, -1, lw, 1, scale(dk, 0.7)); // feet
                }
                if carn && l >= 5 {
                    p.set(lx + lw, -1, [235, 230, 210]); // claws
                }
            }
        }
        if biped && l >= 5 {
            // small forearms
            p.set(bx1 - 1, bbot - h / 3 + 1, lc);
            if big {
                p.set(bx1, bbot - h / 3 + 2, lc);
            }
        }
        if c.act == 1 && moving && ph == 1 {
            p.blend(bx0 - 2, -1, 1, 1, [190, 170, 130], 140);
        }
    }

    // ---- tail
    let tl = g.v[TAIL] * (0.4 + 0.6 * gr);
    if tl > 0.2 {
        let mut n = if big { ((tl * (2.0 + l as f32 * 0.6)) as i32).max(2) } else { 1 + (tl * 3.0) as i32 };
        if biped {
            n += n / 2 + 1; // a long counterweight
        }
        let ty0 = if big { btop + (h / 3).max(1) } else { btop + 1 };
        let th0 = if big { (h as f32 * if biped { 0.55 } else { 0.4 }).round() as i32 } else { 1 };
        let mut tip = (bx0 - 1, ty0);
        for i in 0..n {
            let k = i as f32 / n as f32;
            let th = ((th0 as f32 * (1.0 - k)).round() as i32).max(1);
            let droop = if biped { 0 } else if big { (k * k * h as f32 * 0.6) as i32 } else { 0 };
            let wag = (i == n - 1 && n > 1 && (t / 7) & 1 == 1) as i32;
            let up = (!big && tl > 0.8 && i == n - 1) as i32;
            let y = ty0 + droop - wag - up;
            p.fill(bx0 - 1 - i, y, 1, th, if i == n - 1 { c2 } else { col });
            tip = (bx0 - 1 - i, y);
        }
        if g.v[ARMOR] > 0.6 && g.v[TAIL] > 0.6 {
            // tail club
            let cs = if big { 2 } else { 1 };
            p.fill(tip.0 - cs, tip.1 - (cs - 1), cs + 1, cs + 1 - (!big) as i32, dk);
            p.set(tip.0 - cs, tip.1 - (cs - 1), BONE);
        }
    }

    // ---- body
    for i in 0..l {
        let x = bx0 + i;
        let top = tops[i as usize];
        p.fill(x, top, 1, bbot - endcut(i) - top + 1, col);
    }
    if big {
        // outline on the back and a pale belly (countershading)
        for i in 0..l {
            p.blend(bx0 + i, tops[i as usize], 1, 1, dk, 70);
        }
    }
    if h >= 4 {
        p.blend(bx0 + 1, bbot, l - 2, 1, lt, 90);
    }
    match g.pat {
        1 => {
            let step = if big { 3 } else { 2 };
            let mut i = 1;
            while i < l - 1 {
                let top = tops[i as usize];
                p.fill(bx0 + i, top, 1, if big { (bbot - top) * 2 / 3 } else { h }, c2);
                i += step;
            }
        }
        2 => {
            if big {
                let mut i = 1;
                while i < l - 1 {
                    p.set(bx0 + i, tops[i as usize] + 1 + ((i / 3) % 2) * (h / 3), c2);
                    i += 3;
                }
            } else {
                p.set(bx0 + 1, btop + 1, c2);
                p.set(bx0 + l / 2, btop, c2);
                if l >= 5 {
                    p.set(bx1 - 1, bbot - 1, c2);
                }
            }
        }
        3 => p.fill(bx0 + 1, bbot, l - 2, 1, lt),
        4 => {
            for i in (l - 2).max(0)..l {
                let top = tops[i as usize];
                p.fill(bx0 + i, top, 1, bbot - endcut(i) - top + 1, c2);
            }
        }
        5 => {
            for i in 1..l - 1 {
                p.set(bx0 + i, tops[i as usize], c2);
            }
        }
        _ => {}
    }
    if carn && g.pat != 5 {
        // dark dorsal stripe
        for i in 1..l - 1 {
            p.set(bx0 + i, tops[i as usize], dk);
        }
    }
    if g.has(THICK_HIDE) {
        p.fill(bx0 + 1, bbot, l - 2, 1, dk);
    }
    if g.v[ARMOR] > 0.55 && l >= 5 {
        // scutes
        let mut i = 1;
        while i < l - 1 {
            p.set(bx0 + i, tops[i as usize] + 1 + (i % 4 == 1 && h >= 4) as i32, dk);
            i += 2;
        }
        if g.v[ARMOR] > 0.8 && big {
            p.set(bx0 + 1, bbot - h / 2, BONE);
            p.set(bx1 - 1, bbot - h / 2, BONE);
        }
    }
    if g.v[FUR] > 0.6 && l >= 6 && g.v[PLATES] < 0.3 {
        let mut i = 1;
        while i < l - 1 {
            p.blend(bx0 + i, tops[i as usize] - 1, 1, 1, lt, 110);
            i += 2;
        }
    }
    let pl = g.v[PLATES] * (0.5 + 0.5 * gr);
    if pl > 0.3 {
        // back plates, or a continuous sail
        let sail = g.v[PLATES] > 0.8 && g.v[ARMOR] < 0.5;
        let hmax = 1.0 + pl * (1.0 + h as f32 * 0.55);
        for i in 1..l - 1 {
            if sail || i % 2 == 1 {
                let u = (i as f32 + 0.5) / l as f32;
                let hg = ((u * std::f32::consts::PI).sin() * hmax).round().max(1.0) as i32;
                let top = tops[i as usize];
                p.fill(bx0 + i, top - hg, 1, hg, c2);
                p.set(bx0 + i, top - hg, if sail { scale(c2, 0.8) } else { lt });
            }
        }
    }
    if g.has(SPIKY) {
        let mut i = 1;
        while i < l - 1 {
            p.set(bx0 + i, tops[i as usize] - 1, [240, 240, 240]);
            i += 2;
        }
    }

    // ---- neck and head
    let neck = g.v[NECK] * (0.5 + 0.5 * gr);
    let sep = big || biped || neck > 0.3;
    let hw = if big { 2.max((l as f32 * 0.22).round() as i32 + carn as i32) } else { 2 };
    let hh = if big { 2.max((h as f32 * 0.5).round() as i32) } else { 2 };
    // (rear column, top row) of the head
    let (hx0, hy0) = if neck > 0.3 {
        let nl = (neck * (2.0 + l as f32 * 0.8)).round() as i32;
        let ang: f32 = if feeding { -0.5 } else { 0.55 + 0.75 * neck };
        let (ca, sa) = (ang.cos(), ang.sin());
        let nt = if big { 2.max((h as f32 * 0.38).round() as i32) } else { 1 };
        let (sx, sy) = (bx1 - nt + 1, tops[(l - 2).max(0) as usize] + (big as i32));
        for k in 0..=nl {
            let th = ((nt as f32 * (1.0 - 0.35 * k as f32 / nl.max(1) as f32)).round() as i32).max(1);
            let xx = sx + (k as f32 * ca).round() as i32;
            let yy = sy - (k as f32 * sa).round() as i32;
            p.fill(xx, yy, th, th, col);
        }
        let ex = sx + (nl as f32 * ca).round() as i32;
        let ey = sy - (nl as f32 * sa).round() as i32;
        (ex, ey - hh / 2)
    } else if sep {
        let hy = if feeding {
            bbot - hh + 2
        } else if biped {
            btop - hh / 2
        } else {
            btop + (big as i32) - (hh > 2) as i32
        };
        (bx1 - (big as i32), hy)
    } else {
        (bx1 - 1, btop)
    };
    let hxf = hx0 + hw - 1;
    if sep {
        if g.v[HORN] > 0.6 && g.v[ARMOR] > 0.4 && big {
            // bony frill behind the head
            p.fill(hx0 - 1, hy0 - 2, 2, hh + 2, c2);
            p.set(hx0 - 1, hy0 - 2, lt);
        }
        p.fill(hx0, hy0, hw, hh, col);
        if big && hw >= 3 {
            p.set(hxf, hy0, if feeding { col } else { scale(col, 0.92) });
        }
        if carn && big {
            // jaw line with teeth; jaws open while hunting
            let open = (c.act == 1 || c.act == 2) as i32;
            p.fill(hx0 + 1, hy0 + hh - 1 + open, hw - 1, 1, dk);
            let mut k = hx0 + 1;
            while k <= hxf {
                p.set(k, hy0 + hh - 1, WHITE);
                k += 2;
            }
        }
    } else if feeding {
        // small bodies: head lowered to the carcass
        p.set(bx1 + 1, bbot, col);
        p.set(bx1 + 1, bbot + 1, col);
        if ((t / 5) & 1) == 0 {
            p.set(bx1 + 2, bbot + 1, [150, 20, 20]);
        }
    }
    if feeding && sep && ((t / 5) & 1) == 0 && carn {
        p.set(hxf + 1, hy0 + hh - 1, [150, 20, 20]);
    }
    // snout
    if !sep && l >= 5 {
        p.set(bx1 + 1, bbot - 1, col);
    }
    // ears
    let ear = if g.has(SKITTISH) { g.v[EARS].max(0.75) } else if carn { g.v[EARS].max(0.7) } else { g.v[EARS] };
    let ax = if sep { hx0 + (hw > 2) as i32 } else { bx1 - 1 };
    if ear > 0.3 {
        p.set(ax, hy0 - 1, col);
        if ear > 0.65 {
            p.set(ax, hy0 - 2, dk);
        }
        if ear > 0.5 && !sep && l >= 6 {
            p.set(bx1 - 3, btop - 1, col);
        }
    }
    // horns
    let hn = g.v[HORN] * gr;
    if hn > 0.4 && l >= 4 {
        let hl = if big { 1 + (hn * (1.0 + l as f32 / 6.0)) as i32 } else { 1 + (hn > 0.7) as i32 };
        let bx = if sep { hxf - (hw > 2) as i32 } else { bx1 - 1 };
        for k in 0..hl {
            p.set(bx + k / 2 + (k > 0) as i32, hy0 - 1 - k, BONE);
        }
        if hn > 0.9 {
            p.set(if sep { hxf + 1 } else { bx1 - 3 }, if sep { hy0 } else { btop - 1 }, BONE);
        }
    }
    if g.has(SWIMMER) {
        p.set(bx0 + 1, btop - 1, [60, 220, 230]);
    }
    // eyes
    let (ex, ey) = if sep { (hxf - (hw > 2) as i32, hy0 + (hh >= 3) as i32) } else { (bx1 - 1, btop + (h >= 3) as i32) };
    let e = g.v[EYES] + if juv { 0.35 } else { 0.0 };
    if c.rest {
        p.set(ex, ey, dk);
        p.set(bx1 + 1, btop - 3, WHITE);
        p.set(bx1 + 2, btop - 4, WHITE);
    } else {
        let pc = if carn {
            [235, 40, 40]
        } else if g.has(NOCTURNAL) && dark > 0.4 {
            [255, 230, 90]
        } else {
            [15, 15, 25]
        };
        if e > 0.5 && (l >= 4 || sep) {
            p.set(ex - 1, ey, WHITE);
        }
        p.set(ex, ey, pc);
        if e > 0.85 && (h >= 4 || hh >= 3) {
            p.set(ex, ey + 1, pc);
        }
        if g.v[DIET] < 0.3 && l >= 5 {
            let bottom = if sep { hy0 + hh - 1 } else { bbot };
            p.set(ex, bottom.min(ey + 2), [255, 150, 170]);
        }
    }
    if carn && !(sep && big) {
        // fangs (bared while hunting)
        let my = if sep { hy0 + hh - 1 } else { bbot };
        p.set(if sep { hxf } else { bx1 }, my, WHITE);
        if c.act == 1 || c.act == 2 {
            p.set(if sep { hxf + 1 } else { bx1 + 1 }, my, WHITE);
        }
    }
    if g.v[GILLS] > 0.55 {
        p.set(if sep { hx0 } else { bx1 - 2 }, if sep { hy0 + 1 } else { btop + 1 }, dk);
    }

    // ---- marks
    if g.has(VENOMOUS) {
        p.set(bx0 + 1, btop, [110, 230, 90]);
        p.set(bx1 - 1, bbot, [110, 230, 90]);
    }
    if g.has(PROLIFIC) {
        p.set(0, bbot, [255, 170, 200]);
    }
    if c.preg.is_some() {
        p.set(0, bbot + 1, lt);
    }
    p.set(bx0 + 1 + c.mk as i32 % 1.max(l - 2), btop + 1, c.mkc);
    if l >= 5 && (g.has(SCAVENGER) || g.has(HERDER)) {
        let sc = if g.has(SCAVENGER) { [190, 110, 230] } else { [120, 170, 255] };
        p.fill(bx1 - 2, bbot - 1, 1, 2, sc);
    }
    if g.v[WINGS] > 0.35 {
        let wing = 1.max((g.v[WINGS] * 4.0 * (1.0 + l as f32 / 10.0)) as i32);
        let wy = btop - 1 - if c.z > 0.0 { ((t / 4) & 1) as i32 } else { 0 };
        p.fill(-wing, wy, wing, 1, c2);
        p.fill(-wing + 1, wy - 1, 1.max(wing - 1), 1, lt);
    }
    if g.v[FINS] > 0.35 {
        p.fill(bx0 + 1, bbot + 1, 2, 1, c2);
        p.fill(bx1 - 2, bbot + 1, 2, 1, c2);
    }
    if c.act == 4 && moving && !wet {
        // fleeing: speed lines
        p.blend(bx0 - 2, btop + 1, 1, 1, [235, 235, 235], 150);
        p.blend(bx0 - 3, bbot, 1, 1, [235, 235, 235], 110);
    }
    if wet {
        // lower half of the body under the surface
        let sub = 1.max(h / 2);
        p.blend(bx0 - 1, bbot - sub + 1, l + 2, sub, [40, 90, 140], 120);
    }
    let top = btop.min(hy0);
    if c.sick.is_some() {
        p.set(0, top - 4, [145, 205, 90]);
    }
    if !juv && c.cool > 480 {
        for (dx, dy) in [(-1, -6), (1, -6), (-1, -5), (0, -5), (1, -5), (0, -4)] {
            p.set(dx, top + dy, [255, 90, 130]);
        }
    } else if c.thr.is_some() {
        p.set(bx0 - 1, btop - 1, [150, 220, 255]);
    }
}

/// Renders creature portraits (for panels) into an RGBA buffer.
pub fn portrait(c: &Cr, t: i64, dark: f32, pw: i32, ph: i32) -> Vec<[u8; 4]> {
    let mut px = vec![[30, 38, 34, 255]; (pw * ph) as usize];
    let mut cv = Canvas { w: pw, h: ph, px: &mut px };
    draw_cr(&mut cv, c, t, dark, (pw / 2) as f32, (ph * 3 / 5) as f32, false);
    px
}

pub struct WorldRenderer {
    pub px: Vec<[u8; 4]>,
    base: Vec<[u8; 4]>,
    base_key: (u64, usize, u8, i64),
    pub sprites: FxHashMap<(u32, u8, u8), Sprite>,
    sprite_epoch: u32,
    /// leaf hue (whole degrees) of every plant lineage: a compact table the ground pass reads instead of
    /// the full lineage records (far fewer cache misses)
    hue_of: Vec<u16>,
    hue_epoch: u32,
}

impl WorldRenderer {
    pub fn new() -> Self {
        WorldRenderer { px: vec![], base: vec![], base_key: (u64::MAX, 0, 0, 0), sprites: FxHashMap::default(), sprite_epoch: 0, hue_of: Vec::new(), hue_epoch: u32::MAX }
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
        // the ground layer, row by row on all cores (this runs while the simulation waits for the frame)
        let veg = &s.vegetation;
        if self.hue_epoch != veg.epoch || self.hue_of.len() > veg.lineages.len() {
            self.hue_of.clear();
            self.hue_epoch = veg.epoch;
        }
        for l in &veg.lineages[self.hue_of.len()..] {
            self.hue_of.push((l.g[HUE].rem_euclid(360.0) as u16).min(359));
        }
        let hue_of = &self.hue_of;
        let base = &self.base;
        let lut = cover_lut();
        let rows = &mut self.px[(my0 * w) as usize..(my1 * w) as usize];
        rows.par_chunks_mut(w as usize).enumerate().for_each(|(ry, out)| {
            let row = ((my0 + ry as i32) * w) as usize;
            for x in mx0..mx1 {
                let i = row + x as usize;
                let st = s.pl[i];
                out[x as usize] = if st > 0 && mode == MapMode::Terrain {
                    // ground cover tinted by the lineage's own leaf colour
                    let c = lut[hue_of[veg.ids[i] as usize] as usize][(st as usize - 1).min(2)];
                    let b = base[i];
                    // algae stay a subtle tint so lakes and seas keep their colour
                    let k = if s.terr[i] == 1 { 0.18 + 0.06 * st as f32 } else { 0.55 + 0.15 * st as f32 };
                    [mixc(b[0], c[0], k), mixc(b[1], c[1], k), mixc(b[2], c[2], k), 255]
                } else {
                    base[i]
                };
            }
        });
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
        let inside = |x: i32, y: i32| x >= mx0 - 32 && x < mx1 + 32 && y >= my0 - 8 && y < my1 + 40;
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
                let i = c.y as usize * s.w + c.x as usize;
                let wet = c.z <= 0.5 && s.terr[i] == 1 && c.g.v[COMPLEXITY] >= 0.42;
                draw_cr(&mut cv, c, s.t, s.dark, c.x, c.y, wet);
            }
        }
        // short-lived effects: blood at kills, dust of a pounce
        for fx in &s.fx {
            let (x, y) = (fx.x as i32, fx.y as i32);
            if !inside(x, y) {
                continue;
            }
            if fx.kind == 0 {
                let a = (60 + fx.ttl.min(260) * 150 / 260) as u8;
                cv.blend(x, y, 1, 1, [120, 12, 14], a);
                if fx.ttl > 150 {
                    cv.blend(x + 1, y, 1, 1, [150, 20, 20], a / 2);
                }
            } else {
                let a = (fx.ttl.clamp(0, 12) * 18) as u8;
                let up = (12 - fx.ttl.clamp(0, 12)) / 4;
                cv.blend(x - 1, y - up, 1, 1, [200, 180, 140], a);
                cv.blend(x + 1, y - up, 1, 1, [200, 180, 140], a);
                cv.blend(x, y - up - 1, 1, 1, [210, 195, 160], a / 2);
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
