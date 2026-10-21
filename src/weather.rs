//! Local, world-space water cycle. Cloud liquid becomes falling particles, then soil/pool water or snow.
use crate::sim::Sim;
use rand::Rng as _;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

pub const CELL: usize = 16;

#[derive(Clone, Serialize, Deserialize)]
pub struct Cloud {
    pub x: f32,
    pub y: f32,
    pub water: f32,
    pub radius: f32,
    pub age: i64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Drop {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub water: f32,
    pub snow: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Atmosphere {
    pub width: usize,
    pub height: usize,
    pub cols: usize,
    pub rows: usize,
    pub vapor: Vec<f32>,
    pub clouds: Vec<Cloud>,
    pub drops: Vec<Drop>,
    pub splashes: Vec<(i32, i32, i32)>,
    pub rain: Vec<f32>,
    pub shade: Vec<f32>,
    pub snow: FxHashMap<u32, f32>,
    pub wind: (f32, f32),
    pub evaporated: f32,
    pub deposited: f32,
}

impl Atmosphere {
    pub fn new(width: usize, height: usize) -> Self {
        let cols = width.div_ceil(CELL);
        let rows = height.div_ceil(CELL);
        Atmosphere {
            width, height, cols, rows, vapor: vec![10.0; cols * rows], clouds: vec![], drops: vec![], splashes: vec![],
            rain: vec![0.0; cols * rows], shade: vec![0.0; cols * rows], snow: FxHashMap::default(), wind: (0.06, 0.015),
            evaporated: 0.0, deposited: 0.0,
        }
    }

    #[inline]
    pub fn key(&self, x: f32, y: f32) -> usize {
        let gx = (x.max(0.0) as usize / CELL).min(self.cols - 1);
        let gy = (y.max(0.0) as usize / CELL).min(self.rows - 1);
        gy * self.cols + gx
    }

    #[inline]
    pub fn rain_at(&self, x: f32, y: f32) -> f32 {
        self.rain[self.key(x, y)].min(1.0)
    }

    #[inline]
    pub fn temperature(&self, tmp: &[u8], d_t: f32, w: usize, i: usize) -> f32 {
        let (x, y) = ((i % w) as f32, (i / w) as f32);
        let k = self.key(x, y);
        tmp[i] as f32 / 255.0 + d_t - 0.035 * self.shade[k] - 0.025 * self.rain[k].min(1.0)
    }

    fn footprint(&self, c: &Cloud, out: &mut Vec<usize>) {
        out.clear();
        let gy0 = (c.y - c.radius).max(0.0) as usize / CELL;
        let gy1 = ((c.y + c.radius).max(0.0) as usize / CELL).min(self.rows - 1);
        let gx0 = (c.x - c.radius).max(0.0) as usize / CELL;
        let gx1 = ((c.x + c.radius).max(0.0) as usize / CELL).min(self.cols - 1);
        for gy in gy0..=gy1 {
            for gx in gx0..=gx1 {
                let x = ((gx * CELL) as f32 + CELL as f32 / 2.0).min((self.width - 1) as f32);
                let y = ((gy * CELL) as f32 + CELL as f32 / 2.0).min((self.height - 1) as f32);
                if ((x - c.x).powi(2) + (y - c.y).powi(2)).sqrt() <= c.radius + CELL as f32 / 2.0 {
                    out.push(gy * self.cols + gx);
                }
            }
        }
    }
}

impl Sim {
    fn atmo_temp(&self, i: usize) -> f32 {
        self.atmosphere.temperature(&self.tmp, self.d_t, self.w, i)
    }

    fn soak(&mut self, i: usize, water: f32) {
        let k = i as u32;
        if let Some(p) = self.pools.get_mut(&k) {
            *p = (*p + water * 20.0).min(1800.0);
        } else if self.terr[i] == 0 || self.terr[i] == 3 {
            self.moisture[i] = (self.moisture[i] as i32 + 1.max((water * 6.0) as i32)).min(255) as u8;
            if self.moisture[i] >= 225 && self.neighbors(i).all(|j| self.elevation[i] <= self.elevation[j]) {
                self.make_water(i, true);
            }
        }
        if let Some(f) = self.fire.get_mut(&k) {
            *f = 1.max(*f - (water * 10.0) as i32);
        }
    }

    fn impact(&mut self, d: &Drop) {
        let (x, y) = (d.x as usize, d.y as usize);
        let i = y * self.w + x;
        self.atmosphere.deposited += d.water;
        if d.snow {
            *self.atmosphere.snow.entry(i as u32).or_default() += d.water;
            return;
        }
        let k = self.atmosphere.key(d.x, d.y);
        self.atmosphere.rain[k] = (self.atmosphere.rain[k] + d.water * 0.15).min(1.5);
        if self.atmosphere.splashes.len() < 4000 {
            self.atmosphere.splashes.push((x as i32, y as i32, 5));
        }
        self.soak(i, d.water);
    }

    pub(crate) fn atmosphere_step(&mut self) {
        let dt = self.dt;
        let dtf = dt as f32;
        let kind = self.wx.kind;
        let t = self.t;
        {
            let a = &mut self.atmosphere;
            a.splashes.retain_mut(|s| {
                s.2 -= dt;
                s.2 > 0
            });
            a.wind = (0.065 + 0.035 * (t as f32 / 1500.0).sin(), 0.025 * (t as f32 / 2100.0).sin());
            let decay = 0.985f32.powi(dt);
            for r in a.rain.iter_mut() {
                *r = if *r > 0.015 { *r * decay } else { 0.0 };
            }
        }
        for _ in 0..12 {
            let i = self.rng.gen_range(0..self.n);
            if self.terr[i] == 2 {
                continue;
            }
            let mut amount = if self.terr[i] == 1 { 0.6 } else if self.moisture[i] > 70 { 0.04 } else { 0.0 };
            amount *= dtf * (0.5 + self.atmo_temp(i)).max(0.2);
            if amount == 0.0 {
                continue;
            }
            let k = self.atmosphere.key((i % self.w) as f32, (i / self.w) as f32);
            self.atmosphere.vapor[k] += amount;
            self.atmosphere.evaporated += amount;
            if self.terr[i] != 1 {
                self.moisture[i] = self.moisture[i].saturating_sub(1);
            }
            if let Some(p) = self.pools.get_mut(&(i as u32)) {
                *p -= amount * 20.0;
            }
        }
        if t / 32 != (t - dt as i64) / 32 {
            let a = &mut self.atmosphere;
            let mut tr = a.vapor.clone();
            let dx: i64 = if a.wind.0 >= 0.0 { 1 } else { -1 };
            let cols = a.cols as i64;
            for (k, &water) in a.vapor.iter().enumerate() {
                let row = k as i64 / cols;
                let col = k as i64 % cols;
                let target = (row * cols + (col + dx).rem_euclid(cols)) as usize;
                let moved = water * 0.08;
                tr[k] -= moved;
                tr[target] += moved;
            }
            a.vapor = tr;
        }
        if t / 8 != (t - dt as i64) / 8 {
            let threshold = if matches!(kind, crate::sim::WxKind::Rain | crate::sim::WxKind::Storm | crate::sim::WxKind::Coldsnap | crate::sim::WxKind::Fog) {
                13.0
            } else {
                23.0
            };
            let nv = self.atmosphere.vapor.len();
            let maxc = 12.max(self.n / 3500);
            for _ in 0..32.min(nv) {
                let k = self.rng.gen_range(0..nv);
                let a = &mut self.atmosphere;
                if a.vapor[k] > threshold && a.clouds.len() < maxc {
                    let liquid = a.vapor[k] * 0.8;
                    a.vapor[k] -= liquid;
                    let x = (((k % a.cols) as f32 + 0.5) * CELL as f32).min((a.width - 1) as f32);
                    let y = (((k / a.cols) as f32 + 0.5) * CELL as f32).min((a.height - 1) as f32);
                    let radius = self.rng.gen_range(10.0..20.0);
                    self.atmosphere.clouds.push(Cloud { x, y, water: liquid, radius, age: 0 });
                }
            }
        }
        for s in self.atmosphere.shade.iter_mut() {
            *s = 0.0;
        }
        let mut clouds = std::mem::take(&mut self.atmosphere.clouds);
        let mut fp = Vec::new();
        let mut survivors = Vec::with_capacity(clouds.len());
        let (wd, hd) = (self.w as f32, self.h as f32);
        let mut lightning: Vec<usize> = Vec::new();
        for mut c in clouds.drain(..) {
            c.age += dt as i64;
            let wind = self.atmosphere.wind;
            c.x = (c.x + wind.0 * dtf).rem_euclid(wd);
            c.y = (c.y + wind.1 * dtf).rem_euclid(hd);
            let k = self.atmosphere.key(c.x, c.y);
            let take = self.atmosphere.vapor[k].min(0.09 * dtf);
            self.atmosphere.vapor[k] -= take;
            c.water += take;
            let evap = c.water.min(if kind.dry() { 0.04 } else { 0.004 } * dtf);
            c.water -= evap;
            self.atmosphere.vapor[k] += evap;
            self.atmosphere.footprint(&c, &mut fp);
            for &key in &fp {
                let s = &mut self.atmosphere.shade[key];
                *s = (*s + c.water / 30.0).min(1.0);
            }
            let p = if kind == crate::sim::WxKind::Storm { 0.28 } else { 0.12 };
            if c.water > 12.0 && self.rng.gen::<f32>() < crate::util::pdt(p, dt) {
                for _ in 0..((3 * dt) as usize).min((c.water - 12.0) as usize) {
                    let x = (c.x + self.rng.gen_range(-c.radius..c.radius)).clamp(0.0, wd - 0.01);
                    let y = (c.y + self.rng.gen_range(-c.radius * 0.5..c.radius * 0.5)).clamp(0.0, hd - 0.01);
                    let snow = self.atmo_temp(y as usize * self.w + x as usize) < 0.33;
                    self.atmosphere.drops.push(Drop { x, y, z: 12.0, water: 1.0, snow });
                    c.water -= 1.0;
                }
            }
            if kind == crate::sim::WxKind::Storm && c.water > 20.0 && self.rng.gen::<f32>() < crate::util::pdt(0.0004, dt) {
                lightning.push(c.y as usize * self.w + c.x as usize);
            }
            if c.water > 0.2 {
                survivors.push(c);
            } else {
                self.atmosphere.vapor[k] += c.water;
            }
        }
        self.atmosphere.clouds = survivors;
        let mut drops = std::mem::take(&mut self.atmosphere.drops);
        let wind = self.atmosphere.wind;
        let mut falling = Vec::with_capacity(drops.len());
        for mut d in drops.drain(..) {
            d.x = (d.x + wind.0 * dtf).clamp(0.0, wd - 0.01);
            d.y = (d.y + wind.1 * dtf).clamp(0.0, hd - 0.01);
            d.z -= dtf * if d.snow { 0.35 } else { 1.5 };
            if d.z <= 0.0 {
                self.impact(&d);
            } else {
                falling.push(d);
            }
        }
        self.atmosphere.drops = falling;
        if t / 20 != (t - dt as i64) / 20 {
            let keys: Vec<u32> = self.atmosphere.snow.keys().copied().collect();
            for k in keys {
                if self.atmo_temp(k as usize) <= 0.33 {
                    continue;
                }
                let s = self.atmosphere.snow.get_mut(&k).unwrap();
                let melt = s.min(0.5);
                *s -= melt;
                let gone = *s <= 0.0;
                if gone {
                    self.atmosphere.snow.remove(&k);
                }
                self.soak(k as usize, melt);
            }
        }
        for j in lightning {
            self.lightning(Some(j));
        }
    }
}
