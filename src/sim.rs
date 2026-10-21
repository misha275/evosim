//! The ecosystem: terrain, plants, creatures, disease, fire, meteors, quakes, water.
use crate::genome::*;
use crate::util::*;
use crate::vegetation::{self as veg, Vegetation};
use crate::weather::Atmosphere;
use rand::seq::SliceRandom;
use rand::{Rng as _, SeedableRng};
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub const DAY: i64 = 2400;
pub const SEASON: i64 = DAY * 6;
pub const CH: usize = 8;
pub const CG: usize = 16;
pub const CELL_CM: f32 = 10.0;
/// nutrient pool size of a fully watery 16x16 grid cell
const NUTRIENT_CAP: f32 = 6.0;
pub const TICK_S: f32 = 0.2;
pub const MK: f32 = 0.0032;
const KREST: f32 = 0.14;
const KMOVE: f32 = 0.12;
const SPEC_T: f32 = 0.26;

pub const WORLDS: [(&str, usize, usize); 9] = [
    ("small", 340, 230), ("medium", 480, 320), ("large", 640, 420), ("huge", 960, 640), ("vast", 1280, 840),
    ("giant", 1600, 1060), ("colossal", 2048, 1360), ("titan", 3072, 2048), ("planet", 4096, 2730),
];

/// Seasonal swing multiplier by row: strong seasons in the cold north, mild near the hot south.
#[inline]
pub fn season_amp(y: usize, h: usize) -> f32 {
    1.6 - 1.2 * (y as f32 / h as f32)
}

#[inline(always)]
fn prefetch<T>(p: &T) {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        std::arch::x86_64::_mm_prefetch(p as *const T as *const i8, std::arch::x86_64::_MM_HINT_T0);
    }
    #[cfg(not(target_arch = "x86_64"))]
    let _ = p;
}

#[derive(Clone, Copy)]
enum PlantAct {
    Clone(u32, u32),
    Sprout(u32),
    Die(u32),
    Grow(u32, u8),
    Seed(u32, u32),
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
pub enum Tgt {
    Plant(u32),
    Herd(f32, f32),
    Prey(u32),
    Mate(u32),
    Corpse(u32),
    Bury(u32),
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Cr {
    pub id: u32,
    pub g: Genome,
    pub hap: Box<(Genome, Genome)>,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub f: i8,
    pub e: f32,
    pub age: f32,
    pub s: f32,
    pub max_e: f32,
    pub m34: f32,
    pub cool: i32,
    pub gen: u32,
    pub sp: u32,
    pub tgt: Option<Tgt>,
    pub thr: Option<u32>,
    pub dead: bool,
    pub gut: VecDeque<(i64, u32)>,
    pub wa: f32,
    pub rest: bool,
    pub col: Col,
    pub dk: Col,
    pub lt: Col,
    pub c2: Col,
    pub male: bool,
    pub preg: Option<(i64, Vec<(Genome, String)>)>,
    pub nut: f32,
    pub pf: f32,
    pub mom: Option<u32>,
    pub nurse: f32,
    pub pers: [f32; 3],
    pub nm: String,
    pub mk: u8,
    pub mkc: Col,
    pub kids: Vec<u32>,
    pub het: f32,
    pub sick: Option<(u32, i64)>,
    pub imm: Vec<(u32, i64)>,
    pub fv: Option<(f32, f32)>,
    pub z: f32,
    pub breath: f32,
}

impl Default for Genome {
    fn default() -> Self {
        Genome { v: [0.0; NG], hue: 0.0, hue2: 0.0, pat: 0, q: 0, epoch: 0 }
    }
}

impl Cr {
    fn placeholder() -> Cr {
        Cr {
            id: 0, g: Genome::default(), hap: Box::new((Genome::default(), Genome::default())), x: 0.0, y: 0.0, vx: 0.0,
            vy: 0.0, f: 1, e: 0.0, age: 0.0, s: 1.0, max_e: 1.0, m34: 0.0, cool: 0, gen: 0, sp: 0, tgt: None, thr: None,
            dead: true, gut: VecDeque::new(), wa: 0.0, rest: false, col: [0; 3], dk: [0; 3], lt: [0; 3], c2: [0; 3],
            male: true, preg: None, nut: 0.6, pf: 1.0, mom: None, nurse: 0.0, pers: [0.5; 3], nm: String::new(), mk: 3,
            mkc: [0; 3], kids: Vec::new(), het: 0.0, sick: None, imm: Vec::new(), fv: None, z: 0.0, breath: 1.0,
        }
    }
    /// A display-only creature built from a genome (phylogeny portraits).
    pub fn dummy(g: &Genome, sp: u32) -> Cr {
        let mut c = Cr::placeholder();
        c.dead = false;
        c.g = g.clone();
        c.sp = sp;
        c.age = g.v[MAT] + 10.0;
        c.s = g.v[SIZE];
        c.max_e = 25.0 + 13.0 * c.s;
        c.e = c.max_e;
        let h = g.hue;
        c.col = hsv(h, 55.0 + 30.0 * g.v[DIET], 88.0 - 8.0 * g.v[DIET]);
        c.dk = hsv(h, 70.0, 50.0);
        c.lt = hsv(h, 30.0, 100.0);
        c.c2 = hsv(g.hue2, 62.0, 92.0);
        c.mkc = c.lt;
        c
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Co {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub e: f32,
    pub ttl: f32,
    pub dead: bool,
    pub inf: Option<u32>,
    pub size: f32,
    pub col: Col,
    pub buried: bool,
    pub burial: f32,
    pub bone: Option<u32>,
    pub has_bones: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Bo {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub ttl: i64,
    pub buried: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Poop {
    pub x: f32,
    pub y: f32,
    pub ttl: i32,
    pub seed: Option<u32>,
    pub inf: Option<u32>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Sp {
    pub id: u32,
    pub parent: Option<u32>,
    pub born: i64,
    pub died: Option<i64>,
    pub founder: Genome,
    pub name: String,
    pub pop: i32,
    pub peak: i32,
    pub note: String,
    pub col: Col,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Strain {
    pub id: u32,
    pub base: String,
    pub var: u32,
    pub trans: f32,
    pub vir: f32,
    pub dur: i64,
    pub incub: i64,
    pub center: f32,
    pub width: f32,
    pub born: i64,
    pub parent: Option<u32>,
    pub cases: u32,
    pub act: u32,
    pub peak: u32,
    pub name: String,
    pub col: Col,
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Debug)]
pub enum WxKind {
    Clear,
    Rain,
    Storm,
    Fog,
    Drought,
    Heatwave,
    Coldsnap,
}
impl WxKind {
    pub fn name(self) -> &'static str {
        match self {
            WxKind::Clear => "clear",
            WxKind::Rain => "rain",
            WxKind::Storm => "storm",
            WxKind::Fog => "fog",
            WxKind::Drought => "drought",
            WxKind::Heatwave => "heatwave",
            WxKind::Coldsnap => "coldsnap",
        }
    }
    pub fn col(self) -> Col {
        match self {
            WxKind::Clear => [200, 210, 225],
            WxKind::Rain => [130, 170, 255],
            WxKind::Storm => [170, 150, 255],
            WxKind::Fog => [190, 195, 205],
            WxKind::Drought => [240, 190, 90],
            WxKind::Heatwave => [255, 130, 70],
            WxKind::Coldsnap => [160, 220, 255],
        }
    }
    pub fn dry(self) -> bool {
        matches!(self, WxKind::Drought | WxKind::Heatwave)
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Weather {
    pub kind: WxKind,
    pub ttl: i64,
    pub t0: i64,
    pub int: f32,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Meteor {
    pub x: i32,
    pub y: i32,
    pub r: i32,
    pub ttl: i32,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Lin {
    pub par: u32,
    pub gen: u32,
    pub sp: u32,
    pub note: String,
    pub born: i64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Sim {
    pub primordial: bool,
    pub map_type: crate::worldgen::MapType,
    pub w: usize,
    pub h: usize,
    pub n: usize,
    pub cwc: usize,
    pub chc: usize,
    pub gwc: usize,
    pub ghc: usize,
    pub ps: usize,
    pub maxpop: usize,
    pub rng: Rng,
    pub dt: i32,
    pub t: i64,
    pub nid: u32,
    pub nkid: u32,
    pub dark: f32,
    pub d_t: f32,
    pub crowd: f32,
    /// crowding of aquatic plants (separate carrying capacity for water)
    pub crowd_w: f32,
    /// plants growing in water cells
    pub nwater_plants: i64,
    pub crs: Vec<Cr>,
    pub cor: Vec<Co>,
    pub bones: Vec<Bo>,
    pub poop: Vec<Poop>,
    pub hist: VecDeque<[u32; 5]>,
    pub species: Vec<Sp>,
    pub lin: FxHashMap<u32, Lin>,
    pub log: VecDeque<(i64, String, Col)>,
    pub quake: Option<(i32, i32, i32, i32)>,
    pub fire: FxHashMap<u32, i32>,
    pub fchunk: FxHashMap<u32, Vec<u32>>,
    pub ash: FxHashMap<u32, i32>,
    pub wx: Weather,
    pub wx_grow: f32,
    pub wx_vis: f32,
    pub wx_dt: f32,
    pub dust: f32,
    pub meteor: Option<Meteor>,
    pub flash: f32,
    pub banner: Option<(String, i32, Col)>,
    pub bolt: Option<(i32, i32, i32)>,
    pub strains: Vec<Strain>,
    /// number of variants per strain family (avoids an O(n) scan per mutation)
    #[serde(default)]
    pub strain_vars: FxHashMap<String, u32>,
    pub nsick: u32,
    pub environment_strain: Option<u32>,
    pub next_outbreak: i64,
    pub vegetation: Vegetation,
    pub milestones: FxHashSet<String>,
    pub elevation: Vec<u8>,
    pub moisture: Vec<u8>,
    pub springs: Vec<u32>,
    /// biome per cell (worldgen::Biome)
    pub biome: Vec<u8>,
    pub pools: FxHashMap<u32, f32>,
    pub terr: Vec<u8>,
    pub fert: Vec<f32>,
    pub tmp: Vec<u8>,
    pub bg: Vec<Col>,
    pub pl: Vec<u8>,
    pub nplants: i64,
    pub land: i64,
    pub atmosphere: Atmosphere,
    /// 1 bit per cell: plant present (fits in L2 cache, used by the plant sampler)
    #[serde(skip)]
    pub occ: Vec<u64>,
    /// number of grown (st>=2) plants per CHxCH chunk
    #[serde(skip)]
    pub pcount: Vec<u16>,
    #[serde(skip)]
    pub idmap: FxHashMap<u32, usize>,
    #[serde(skip)]
    pub kmap: FxHashMap<u32, usize>,
    #[serde(skip)]
    pub cgrid: Vec<Vec<u32>>,
    /// dissolved nutrients per 16x16 grid cell: colonies compete for them (density-dependent growth)
    #[serde(default)]
    pub nutrients: Vec<f32>,
    /// share of water cells per grid cell
    #[serde(skip)]
    pub gwater: Vec<f32>,
    #[serde(skip)]
    pub kgrid: Vec<Vec<u32>>,
    /// bumped whenever the terrain background changes (renderer cache key)
    #[serde(skip)]
    pub bg_version: u64,
    /// phase timings in seconds (profiling)
    #[serde(skip)]
    pub prof: [f64; 8],
}

fn susc(o: &mut Cr, st: &Strain, t: i64) -> f32 {
    if let Some(p) = o.imm.iter().position(|&(id, _)| id == st.id) {
        if o.imm[p].1 > t {
            return 0.0;
        }
        o.imm.swap_remove(p);
    }
    let tr = (1.0 - (o.g.v[DIET] - st.center).abs() / st.width).max(0.0);
    tr * (1.0 - 0.8 * o.g.v[IMMUNE]) * if o.e < 0.3 * o.max_e { 1.6 } else { 1.0 }
}

impl Sim {
    pub fn new(w: usize, h: usize, primordial: bool, seed: u64) -> Sim {
        Self::new_with(w, h, primordial, seed, crate::worldgen::MapType::Continents)
    }

    pub fn new_with(w: usize, h: usize, primordial: bool, seed: u64, map_type: crate::worldgen::MapType) -> Sim {
        let n = w * h;
        let mut s = Sim {
            primordial,
            map_type,
            w,
            h,
            n,
            cwc: w.div_ceil(CH),
            chc: h.div_ceil(CH),
            gwc: w.div_ceil(CG),
            ghc: h.div_ceil(CG),
            ps: 300.max(n / 110),
            maxpop: 120.max((2000f32.min(300.0 * n as f32 / 78200.0)) as usize),
            rng: Rng::seed_from_u64(seed),
            dt: 1,
            t: 0,
            nid: 1,
            nkid: 1,
            dark: 0.0,
            d_t: 0.0,
            crowd: 1.0,
            crowd_w: 1.0,
            nwater_plants: 0,
            crs: Vec::new(),
            cor: Vec::new(),
            bones: Vec::new(),
            poop: Vec::new(),
            hist: VecDeque::new(),
            species: Vec::new(),
            lin: FxHashMap::default(),
            log: VecDeque::new(),
            quake: None,
            fire: FxHashMap::default(),
            fchunk: FxHashMap::default(),
            ash: FxHashMap::default(),
            wx: Weather { kind: WxKind::Clear, ttl: 900, t0: 0, int: 0.0 },
            wx_grow: 1.0,
            wx_vis: 1.0,
            wx_dt: 0.0,
            dust: 0.0,
            meteor: None,
            flash: 0.0,
            banner: None,
            bolt: None,
            strains: Vec::new(),
            strain_vars: FxHashMap::default(),
            nsick: 0,
            environment_strain: None,
            next_outbreak: 0,
            vegetation: Vegetation::new(w, h, primordial),
            milestones: FxHashSet::default(),
            elevation: vec![],
            moisture: vec![],
            springs: vec![],
            biome: vec![],
            pools: FxHashMap::default(),
            terr: vec![0; n],
            fert: vec![0.0; n],
            tmp: vec![0; n],
            bg: vec![[0; 3]; n],
            pl: vec![0; n],
            nplants: 0,
            land: 0,
            atmosphere: Atmosphere::new(w, h),
            pcount: vec![],
            occ: vec![],
            idmap: FxHashMap::default(),
            kmap: FxHashMap::default(),
            cgrid: vec![],
            nutrients: vec![],
            gwater: vec![],
            kgrid: vec![],
            bg_version: 0,
            prof: [0.0; 8],
        };
        s.pcount = vec![0; s.cwc * s.chc];
        s.occ = vec![0; n.div_ceil(64)];
        s.cgrid = vec![Vec::new(); s.gwc * s.ghc];
        s.kgrid = vec![Vec::new(); s.gwc * s.ghc];
        s.next_outbreak = s.rng.gen_range(DAY / 2..=DAY);
        s.mkworld();
        s.rebuild_water_grid();
        s.nutrients = s.gwater.iter().map(|&wf| wf * NUTRIENT_CAP).collect();
        // animal numbers scale with the land, not the whole map (oceans feed no land animals)
        let area = (s.land as f32 / 58000.0).max(0.3);
        s.maxpop = 120.max((1200f32.min(300.0 * s.land as f32 / 58000.0)) as usize);
        let founders: &[(Kind, usize)] =
            if primordial { &[(Kind::Microbe, 90)] } else { &[(Kind::Herb, 70), (Kind::Omni, 6), (Kind::Carn, 3)] };
        for &(kind, cnt) in founders {
            let b = base_genome(&mut s.rng, kind);
            let sp = s.newsp(None, b.clone(), "");
            // founders arrive in herds/colonies: a suitable spot, then companions around it
            let total = cnt.max((cnt as f32 * area.powf(0.8)) as usize);
            let mut herd_at: Option<(f32, f32)> = None;
            for k in 0..total {
                let h1 = mutate(&mut s.rng, &b, 0.8).0;
                let h2 = mutate(&mut s.rng, &b, 0.8).0;
                if k % 12 == 0 {
                    herd_at = None;
                }
                let pos = match herd_at {
                    Some((hx, hy)) => {
                        let (x, y) = (hx + s.rng.gen_range(-8.0..8.0), hy + s.rng.gen_range(-8.0..8.0));
                        let swim = aquatic(&b);
                        let okp = if swim { x >= 1.0 && y >= 1.0 && x < (w - 1) as f32 && y < (h - 1) as f32 && s.terr[y as usize * w + x as usize] == 1 } else { s.ok(x, y, false) };
                        if okp { Some((x, y)) } else { None }
                    }
                    None => s.near_prey(kind),
                };
                if let Some(ci) = s.spawn(h1, h2, sp, None, pos, None, "founder", None, None) {
                    if herd_at.is_none() {
                        herd_at = Some((s.crs[ci].x, s.crs[ci].y));
                    }
                }
            }
        }
        s
    }

    /// Rebuild derived (non-saved) indexes after loading.
    pub fn rebuild_after_load(&mut self) {
        self.pcount = vec![0; self.cwc * self.chc];
        self.occ = vec![0; self.n.div_ceil(64)];
        self.nwater_plants = (0..self.n).filter(|&i| self.pl[i] > 0 && self.terr[i] == 1).count() as i64;
        for i in 0..self.n {
            if self.pl[i] > 0 {
                self.occ[i >> 6] |= 1 << (i & 63);
            }
            if self.pl[i] >= 2 {
                let k = self.chunk(i);
                self.pcount[k] += 1;
            }
        }
        self.cgrid = vec![Vec::new(); self.gwc * self.ghc];
        self.kgrid = vec![Vec::new(); self.gwc * self.ghc];
        self.rebuild_water_grid();
        if self.nutrients.len() != self.gwc * self.ghc {
            self.nutrients = self.gwater.iter().map(|&wf| wf * NUTRIENT_CAP).collect();
        }
        self.idmap = self.crs.iter().enumerate().map(|(i, c)| (c.id, i)).collect();
        self.kmap = self.cor.iter().enumerate().map(|(i, c)| (c.id, i)).collect();
        self.vegetation.rebuild_trees(&self.pl);
        self.bg_version += 1;
    }

    fn rebuild_water_grid(&mut self) {
        let mut g = vec![0f32; self.gwc * self.ghc];
        for i in 0..self.n {
            if self.terr[i] == 1 {
                g[(i / self.w / CG) * self.gwc + (i % self.w) / CG] += 1.0;
            }
        }
        for v in g.iter_mut() {
            *v /= (CG * CG) as f32;
        }
        self.gwater = g;
    }

    /// Nutrient regeneration per grid cell per tick: the waters as a whole feed roughly 60% of
    /// the population limit, so colonies are limited by food (turnover, selection) and not by the cap.
    fn nutrient_regen(&self) -> f32 {
        let water: f32 = self.gwater.iter().sum::<f32>().max(1.0);
        self.maxpop as f32 * 0.6 * 0.012 / water
    }

    #[inline]
    fn chunk(&self, i: usize) -> usize {
        (i / self.w / CH) * self.cwc + (i % self.w) / CH
    }

    #[inline]
    pub fn every(&self, period: i64, offset: i64) -> bool {
        (self.t + offset).div_euclid(period) != (self.t - self.dt as i64 + offset).div_euclid(period)
    }

    #[inline]
    fn rr(&mut self) -> f32 {
        self.rng.gen::<f32>()
    }

    pub fn logmsg(&mut self, txt: String, col: Col) {
        self.log.push_back((self.t, txt, col));
        if self.log.len() > 50 {
            self.log.pop_front();
        }
    }

    // ------------------------------------------------------------ world generation
    fn mkworld(&mut self) {
        let wd = crate::worldgen::generate(self.w, self.h, self.map_type, &mut self.rng);
        self.terr = wd.terr;
        self.elevation = wd.elevation;
        self.moisture = wd.moisture;
        self.tmp = wd.tmp;
        self.fert = wd.fert;
        self.bg = wd.bg;
        self.biome = wd.biome;
        self.springs = wd.springs;
        self.land = wd.land;
        // initial vegetation near carrying capacity, denser on fertile soil
        for i in 0..self.n {
            if self.terr[i] == 0 && self.rng.gen::<f32>() < 0.16 * self.fert[i] {
                self.setp(i, 3, None);
            } else if self.terr[i] == 1 && self.elevation[i] > 70 && self.rng.gen::<f32>() < 0.03 {
                self.setp(i, 3, Some(0));
            }
        }
    }

    /// Local temperature without weather shading: climate + season (seasons are stronger in the cold north).
    #[inline]
    pub fn temp_at(&self, i: usize) -> f32 {
        self.tmp[i] as f32 / 255.0 + self.d_t * season_amp(i / self.w, self.h)
    }

    #[inline]
    pub fn neighbors(&self, i: usize) -> impl Iterator<Item = usize> {
        let (x, y, w, h) = (i % self.w, i / self.w, self.w, self.h);
        [
            if x > 0 { Some(i - 1) } else { None },
            if x < w - 1 { Some(i + 1) } else { None },
            if y > 0 { Some(i - w) } else { None },
            if y < h - 1 { Some(i + w) } else { None },
        ]
        .into_iter()
        .flatten()
    }

    pub fn make_water(&mut self, i: usize, temporary: bool) {
        if self.terr[i] == 2 {
            return;
        }
        if self.terr[i] != 1 {
            self.land -= 1;
            self.setp(i, 0, None);
            self.terr[i] = 1;
            self.bg[i] = [43, 112, 155];
            self.bg_version += 1;
            if temporary {
                self.pools.insert(i as u32, 900.0);
            }
        }
    }

    fn water_step(&mut self) {
        if !self.every(20, 0) {
            return;
        }
        for _ in 0..100 {
            let i = self.rng.gen_range(0..self.n);
            if self.terr[i] == 1 || self.terr[i] == 2 {
                continue;
            }
            let wet = self.atmosphere.rain_at((i % self.w) as f32, (i / self.w) as f32) > 0.05;
            let ns: Vec<usize> = self.neighbors(i).collect();
            let near_water = ns.iter().any(|&j| self.terr[j] == 1);
            let delta: i32 = if wet { 8 } else if near_water { 2 } else { -1 };
            self.moisture[i] = (self.moisture[i] as i32 + delta).clamp(0, 255) as u8;
            let low = *ns.iter().min_by_key(|&&j| self.elevation[j]).unwrap();
            if wet && self.elevation[i] as i32 > self.elevation[low] as i32 + 6 {
                self.elevation[i] -= 1;
                self.elevation[low] = self.elevation[low].saturating_add(1);
                self.fert[low] = (self.fert[low] + 0.003).min(1.0);
            }
            let basin = ns.iter().all(|&j| self.elevation[i] <= self.elevation[j]);
            if wet && self.moisture[i] > 200 && (basin || near_water) {
                self.make_water(i, true);
            }
        }
        let keys: Vec<u32> = self.pools.keys().copied().collect();
        let dry = self.wx.kind.dry();
        for k in keys {
            let i = k as usize;
            let wet = self.atmosphere.rain_at((i % self.w) as f32, (i / self.w) as f32) > 0.05;
            let p = self.pools.get_mut(&k).unwrap();
            *p += if wet { 20.0 } else if dry { -40.0 } else { -10.0 };
            *p = p.min(1800.0);
            if *p <= 0.0 {
                self.pools.remove(&k);
                if self.pl[i] > 0 {
                    self.nwater_plants -= 1;
                }
                self.terr[i] = 0;
                self.land += 1;
                self.fert[i] = self.fert[i].max(0.35);
                self.bg[i] = [80, 92, 56];
                self.bg_version += 1;
            }
        }
        for ci in 0..self.crs.len() {
            let (x, y) = (self.crs[ci].x, self.crs[ci].y);
            let i = y as usize * self.w + x as usize;
            if self.terr[i] != 1 || can_swim(&self.crs[ci].g) || self.crs[ci].z > 0.0 || self.biome.get(i) == Some(&(crate::worldgen::Biome::River as u8)) {
                continue;
            }
            let shores: Vec<usize> = self.neighbors(i).filter(|&j| self.terr[j] == 0 || self.terr[j] == 3).collect();
            if let Some(&j) = shores.choose(&mut self.rng) {
                self.crs[ci].x = (j % self.w) as f32 + 0.5;
                self.crs[ci].y = (j / self.w) as f32 + 0.5;
            } else {
                let c = &mut self.crs[ci];
                c.e -= c.max_e * 0.03;
            }
        }
    }

    fn near_prey(&mut self, kind: Kind) -> Option<(f32, f32)> {
        if matches!(kind, Kind::Herb | Kind::Microbe) {
            return None;
        }
        let herb: Vec<(f32, f32)> = self.crs.iter().filter(|c| c.g.v[DIET] < 0.45).map(|c| (c.x, c.y)).collect();
        if herb.is_empty() {
            return None;
        }
        for _ in 0..30 {
            let (hx, hy) = herb[self.rng.gen_range(0..herb.len())];
            // close enough to find prey, far enough not to wipe a herd out on day one
            let x = hx + self.rng.gen_range(-60.0..60.0);
            let y = hy + self.rng.gen_range(-60.0..60.0);
            if self.ok(x, y, false) {
                return Some((x, y));
            }
        }
        None
    }

    pub fn setp(&mut self, i: usize, st: u8, lineage: Option<u32>) {
        let old = self.pl[i];
        let veg = &mut self.vegetation;
        let prev = veg.ids[i];
        if let Some(l) = lineage {
            veg.ids[i] = l;
        } else if old == 0 && st != 0 {
            veg.ids[i] = if self.terr[i] == 1 { 0 } else { veg.pioneer(self.tmp[i] as f32 / 255.0, self.moisture[i] as f32 / 255.0) };
        }
        let cur = veg.ids[i];
        if old != 0 && (st == 0 || prev != cur) {
            let p = prev as usize;
            veg.counts[p] -= 1;
            if veg.counts[p] == 0 {
                veg.died[p] = Some(self.t);
            }
        }
        if st != 0 && (old == 0 || prev != cur) {
            let c = cur as usize;
            veg.counts[c] += 1;
            veg.peaks[c] = veg.peaks[c].max(veg.counts[c]);
            veg.died[c] = None;
        }
        // pl must be updated before track() reads the growth stage of the lineage
        if old != st {
            self.pl[i] = st;
        }
        self.vegetation.track(i, st);
        if old == st {
            return;
        }
        if st > 0 {
            self.occ[i >> 6] |= 1 << (i & 63);
        } else {
            self.occ[i >> 6] &= !(1 << (i & 63));
        }
        let k = self.chunk(i);
        if st >= 2 && old < 2 {
            self.pcount[k] += 1;
        } else if old >= 2 && st < 2 {
            self.pcount[k] -= 1;
        }
        let d = (st > 0) as i64 - (old > 0) as i64;
        self.nplants += d;
        if self.terr[i] == 1 {
            self.nwater_plants += d;
        }
    }

    pub fn newsp(&mut self, parent: Option<u32>, g: Genome, note: &str) -> u32 {
        let names: FxHashSet<&str> = self.species.iter().map(|p| p.name.as_str()).collect();
        let nm = loop {
            let len = [2, 2, 3][self.rng.gen_range(0..3)];
            let nm = name(&mut self.rng, len);
            if !names.contains(nm.as_str()) {
                break nm;
            }
        };
        let id = self.species.len() as u32;
        let col = hsv(g.hue, 65.0, 95.0);
        self.species.push(Sp { id, parent, born: self.t, died: None, founder: g, name: nm, pop: 0, peak: 0, note: note.into(), col });
        id
    }

    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        &mut self, h1: Genome, h2: Genome, sp: u32, par: Option<(u32, u32)>, pos: Option<(f32, f32)>, e: Option<f32>,
        note: &str, mom: Option<u32>, g: Option<Genome>,
    ) -> Option<usize> {
        let mut g = g.unwrap_or_else(|| phenotype(&mut self.rng, &h1, &h2));
        let male = self.rng.gen::<bool>();
        if !male {
            g.v[HORN] *= 0.35;
            g.v[TAIL] *= 0.7;
        }
        let (x, y) = match pos {
            Some(p) => p,
            None => {
                let mut i: Option<usize> = None;
                let aq = aquatic(&g);
                for _ in 0..400 {
                    let j = self.rng.gen_range(0..self.n);
                    let okk = if aq { self.terr[j] == 1 } else { self.terr[j] == 0 && self.fert[j] > 0.3 };
                    if okk {
                        i = Some(j);
                        if (self.tmp[j] as f32 / 255.0 - g.v[TOL]).abs() < 0.14 {
                            break;
                        }
                    }
                }
                let i = i?;
                ((i % self.w) as f32 + 0.5, (i / self.w) as f32 + 0.5)
            }
        };
        let id = self.nid;
        self.nid += 1;
        let age = if par.is_none() { self.rng.gen_range(0..=(g.v[MAT] as i32)) as f32 } else { 0.0 };
        let nut = 0.6;
        let pf = 0.75 + 0.4 * nut;
        let gr = (age / g.v[MAT]).min(1.0);
        let s = g.v[SIZE] * pf * (0.35 + 0.65 * (1.0 - (1.0 - gr).powi(2)));
        let mm = MK * s.powi(3);
        let m34 = mm.powf(0.75);
        let max_e = 60.0 + 110.0 * mm;
        let pers = [
            (g.v[BOLD] + gauss(&mut self.rng, 0.0, 0.15)).clamp(0.0, 1.0),
            (g.v[SOCIAL] + gauss(&mut self.rng, 0.0, 0.15)).clamp(0.0, 1.0),
            (g.v[CURIO] + gauss(&mut self.rng, 0.0, 0.15)).clamp(0.0, 1.0),
        ];
        let nm = name(&mut self.rng, 2);
        let het = heterozygosity(&h1, &h2);
        let hh = g.hue + gauss(&mut self.rng, 0.0, 3.0);
        let (mut sat, mut val) = (55.0 + 30.0 * g.v[DIET], 88.0 - 8.0 * g.v[DIET] + self.rng.gen_range(-5..=5) as f32);
        if g.has(CAMOUFLAGE) {
            sat *= 0.45;
            val *= 0.75;
        }
        let mk = self.rng.gen_range(1..9u8);
        let mkc = hsv(hh + self.rng.gen_range(-30.0..30.0), 75.0, 100.0);
        let wa = self.rr() * 6.28;
        let c = Cr {
            id, x, y, vx: 0.0, vy: 0.0, f: 1, e: e.unwrap_or(max_e * 0.7), age, s, max_e, m34, cool: 200,
            gen: par.map_or(0, |p| p.1 + 1), sp, tgt: None, thr: None, dead: false, gut: VecDeque::new(), wa, rest: false,
            col: hsv(hh, sat, val), dk: hsv(hh, 70.0, 50.0), lt: hsv(hh, 30.0, 100.0), c2: hsv(g.hue2, 62.0, 92.0), male,
            preg: None, nut, pf, mom, nurse: if mom.is_some() { 150.0 + 500.0 * g.v[CARE] } else { 0.0 }, pers, nm, mk,
            mkc, kids: Vec::new(), het, sick: None, imm: Vec::new(), fv: None, z: 0.0, breath: 1.0,
            hap: Box::new((h1, h2)), g,
        };
        let spp = &mut self.species[sp as usize];
        spp.pop += 1;
        spp.peak = spp.peak.max(spp.pop);
        spp.died = None;
        self.lin.insert(id, Lin { par: par.map_or(0, |p| p.0), gen: c.gen, sp, note: note.into(), born: self.t });
        self.crs.push(c);
        let idx = self.crs.len() - 1;
        self.idmap.insert(id, idx);
        Some(idx)
    }

    fn classify(&mut self, g: &Genome, sp: u32) -> u32 {
        let simple = g.v[COMPLEXITY] < 0.42;
        let founder = &self.species[sp as usize].founder;
        let same_group = simple == (founder.v[COMPLEXITY] < 0.42);
        if same_group && dist(g, founder) <= SPEC_T {
            return sp;
        }
        for o in &self.species {
            if o.pop > 0 && simple == (o.founder.v[COMPLEXITY] < 0.42) && dist(g, &o.founder) < SPEC_T * 0.75 {
                return o.id;
            }
        }
        let note = if !same_group { "animal body plan".to_string() } else { trait_note(g, founder) };
        let nsp = self.newsp(Some(sp), g.clone(), &note);
        let msg = format!("{} splits from {}: {}", self.species[nsp as usize].name, self.species[sp as usize].name, note);
        let col = self.species[nsp as usize].col;
        self.logmsg(msg, col);
        nsp
    }

    fn deliver(&mut self, mom: &mut Cr, sperm: Vec<(Genome, String)>) {
        let e0 = (0.10 + 0.25 * mom.g.v[CARE]) * mom.max_e;
        for (hd, nd) in sperm {
            if self.crs.len() >= self.maxpop {
                break;
            }
            let (hm, nm_) = gamete(&mut self.rng, &mom.hap.0, &mom.hap.1);
            let g = phenotype(&mut self.rng, &hm, &hd);
            let sp = self.classify(&g, mom.sp);
            let note = if nm_.is_empty() { nd } else { nm_ };
            let (mut cx, mut cy) = (mom.x + self.rng.gen_range(-2.0..2.0), mom.y + self.rng.gen_range(-2.0..2.0));
            if !self.ok(cx, cy, can_swim(&g)) {
                cx = mom.x;
                cy = mom.y;
            }
            if let Some(ci) = self.spawn(hm, hd, sp, Some((mom.id, mom.gen)), Some((cx, cy)), Some(e0), &note, Some(mom.id), Some(g)) {
                mom.kids.push(self.crs[ci].id);
            }
            mom.e -= e0;
            if let Some(stripped) = note.strip_prefix('!') {
                let msg = format!("MACRO-MUTANT in {}: {}", self.species[sp as usize].name, stripped);
                self.logmsg(msg, [255, 210, 120]);
            }
        }
        mom.e = mom.e.max(1.0);
    }

    pub fn kill(&mut self, c: &mut Cr, corpse: bool) {
        if c.dead {
            return;
        }
        c.dead = true;
        let t = self.t;
        let sp = &mut self.species[c.sp as usize];
        sp.pop -= 1;
        if sp.pop == 0 {
            sp.died = Some(t);
            if sp.peak >= 8 {
                let msg = format!("{} went extinct (peak {})", sp.name, sp.peak);
                self.logmsg(msg, [150, 150, 160]);
            }
        }
        if corpse {
            let id = self.nkid;
            self.nkid += 1;
            self.cor.push(Co {
                id, x: c.x, y: c.y, e: MK * c.s.powi(3) * 120.0 + 4.0, ttl: 900.0, dead: false,
                inf: c.sick.map(|s| s.0), size: c.s, col: c.col, buried: false, burial: 0.0, bone: None,
                has_bones: c.g.v[COMPLEXITY] >= 0.42,
            });
            self.kmap.insert(id, self.cor.len() - 1);
        }
    }

    fn kill_idx(&mut self, j: usize, corpse: bool) {
        let mut o = std::mem::replace(&mut self.crs[j], Cr::placeholder());
        self.kill(&mut o, corpse);
        self.crs[j] = o;
    }

    pub fn kill_id(&mut self, id: u32) {
        if let Some(&j) = self.idmap.get(&id) {
            self.kill_idx(j, true);
        }
    }

    // ---- disease
    pub fn new_strain(&mut self, parent: Option<u32>) -> u32 {
        const DNAME: [&str; 7] = ["pox", "flu", "rot", "fever", "wilt", "rash", "cough"];
        let id = self.strains.len() as u32;
        let t = self.t;
        let st = match parent {
            None => {
                let base = name(&mut self.rng, 2) + DNAME[self.rng.gen_range(0..7)];
                *self.strain_vars.entry(base.clone()).or_insert(0) += 1;
                let r = &mut self.rng;
                Strain {
                    id, name: base.clone(), base, var: 1, trans: r.gen_range(0.006..0.03), vir: r.gen_range(0.15..0.8),
                    dur: r.gen_range(350..=900), incub: r.gen_range(30..=160), center: r.gen::<f32>(),
                    width: r.gen_range(0.3..0.6), born: t, parent: None, cases: 0, act: 0, peak: 0,
                    col: hsv(r.gen_range(70.0..330.0), 60.0, 95.0),
                }
            }
            Some(p) => {
                let pa = self.strains[p as usize].clone();
                let cnt = self.strain_vars.entry(pa.base.clone()).or_insert(1);
                *cnt += 1;
                let var = *cnt;
                let r = &mut self.rng;
                let st = Strain {
                    id, name: format!("{} v{}", pa.base, var), base: pa.base.clone(), var,
                    trans: (pa.trans * gauss(r, 0.0, 0.25).exp()).clamp(0.003, 0.06),
                    vir: (pa.vir * gauss(r, 0.0, 0.2).exp()).clamp(0.08, 1.0),
                    dur: (pa.dur as f32 * gauss(r, 0.0, 0.15).exp()).clamp(200.0, 1200.0) as i64,
                    incub: (pa.incub as f32 * gauss(r, 0.0, 0.2).exp()).clamp(10.0, 250.0) as i64,
                    center: (pa.center + gauss(r, 0.0, 0.1)).clamp(0.0, 1.0),
                    width: (pa.width + gauss(r, 0.0, 0.05)).clamp(0.2, 0.8), born: t, parent: Some(p), cases: 0,
                    act: 0, peak: 0, col: pa.col,
                };
                let msg = format!("New strain: {} (virulence {:.0}%, spreads {:.1}x)", st.name, st.vir * 100.0, st.trans / pa.trans);
                self.logmsg(msg, st.col);
                st
            }
        };
        self.strains.push(st);
        id
    }

    fn infect(&mut self, c: &mut Cr, st: u32) {
        if c.sick.is_some() || c.dead {
            return;
        }
        c.sick = Some((st, self.t));
        self.strains[st as usize].cases += 1;
    }

    fn infect_idx(&mut self, j: usize, st: u32) {
        if self.crs[j].sick.is_some() || self.crs[j].dead {
            return;
        }
        self.crs[j].sick = Some((st, self.t));
        self.strains[st as usize].cases += 1;
    }

    fn grid_range(&self, x: f32, y: f32, r: f32) -> (usize, usize, usize, usize) {
        let gy0 = ((y - r).max(0.0) as usize) / CG;
        let gy1 = (((y + r).max(0.0) as usize) / CG).min(self.ghc - 1);
        let gx0 = ((x - r).max(0.0) as usize) / CG;
        let gx1 = (((x + r).max(0.0) as usize) / CG).min(self.gwc - 1);
        (gx0, gx1, gy0, gy1)
    }

    fn spread(&mut self, c: &Cr, st: u32) {
        let (gx0, gx1, gy0, gy1) = self.grid_range(c.x, c.y, 5.0);
        let mut near = Vec::new();
        for gy in gy0..=gy1 {
            for gx in gx0..=gx1 {
                for &j in &self.cgrid[gy * self.gwc + gx] {
                    let o = &self.crs[j as usize];
                    if o.id == c.id || o.sick.is_some() || o.dead || (o.x - c.x).powi(2) + (o.y - c.y).powi(2) > 25.0 {
                        continue;
                    }
                    near.push(j as usize);
                }
            }
        }
        for j in near {
            let trans = self.strains[st as usize].trans;
            let sc = susc(&mut self.crs[j], &self.strains[st as usize], self.t);
            if self.rr() < pdt((trans * 3.0 * sc).min(1.0), self.dt) {
                let s2 = if self.rr() > 0.05 { st } else { self.new_strain(Some(st)) };
                self.infect_idx(j, s2);
            }
        }
    }

    pub fn outbreak(&mut self, target: Option<usize>) {
        let healthy: Vec<usize> = (0..self.crs.len()).filter(|&i| !self.crs[i].dead && self.crs[i].sick.is_none()).collect();
        let ci = match target {
            Some(i) => i,
            None => match healthy.choose(&mut self.rng) {
                Some(&i) => i,
                None => return,
            },
        };
        let st = self.new_strain(None);
        let diet = self.crs[ci].g.v[DIET];
        let s = &mut self.strains[st as usize];
        s.center = diet;
        s.width = s.width.max(0.5);
        let incub = s.incub;
        let name = s.name.clone();
        self.crs[ci].sick = Some((st, self.t - incub));
        self.strains[st as usize].cases += 1;
        let spn = self.species[self.crs[ci].sp as usize].name.clone();
        self.logmsg(format!("OUTBREAK: {} appears in {}", name, spn), [150, 255, 120]);
        self.banner = Some((format!("OUTBREAK: {}", name), 160, [150, 255, 120]));
    }

    // ---- weather / fire / meteors
    fn weather_step(&mut self) {
        let t = self.t;
        self.wx.ttl -= self.dt as i64;
        let mut kind = self.wx.kind;
        self.wx.int = if kind == WxKind::Clear {
            0.0
        } else {
            ((t - self.wx.t0) as f32 / 150.0).min((self.wx.ttl as f32 / 150.0).max(0.0)).min(1.0)
        };
        if self.wx.ttl <= 0 {
            let si = (4 * (t % SEASON) / SEASON) as i32;
            let wt = [
                (WxKind::Clear, 5.0),
                (WxKind::Rain, if si == 0 || si == 2 { 3.0 } else { 1.5 }),
                (WxKind::Storm, if si == 1 { 1.5 } else { 0.6 }),
                (WxKind::Fog, if si != 1 { 1.5 } else { 0.4 }),
                (WxKind::Drought, if si == 1 { 1.3 } else { 0.1 }),
                (WxKind::Heatwave, if si == 1 { 1.3 } else { 0.0 }),
                (WxKind::Coldsnap, if si == 3 { 1.5 } else { 0.0 }),
            ];
            let total: f32 = wt.iter().map(|w| w.1).sum();
            let mut pick = self.rr() * total;
            kind = WxKind::Clear;
            for (k, wgt) in wt {
                if pick < wgt {
                    kind = k;
                    break;
                }
                pick -= wgt;
            }
            let ttl = if kind == WxKind::Clear { self.rng.gen_range(800..=2500) } else { self.rng.gen_range(600..=2000) };
            self.wx = Weather { kind, ttl, t0: t, int: 0.0 };
            if kind != WxKind::Clear {
                self.logmsg(format!("Weather: {}", kind.name()), kind.col());
            }
        }
        let i = self.wx.int;
        self.wx_grow = match kind {
            WxKind::Drought => 1.0 - 0.6 * i,
            WxKind::Heatwave => 1.0 - 0.2 * i,
            WxKind::Coldsnap => 1.0 - 0.35 * i,
            _ => 1.0,
        };
        self.wx_vis = if kind == WxKind::Fog { 1.0 - 0.25 * i } else { 1.0 };
        self.wx_dt = match kind {
            WxKind::Heatwave => 0.14 * i,
            WxKind::Coldsnap => -0.14 * i,
            _ => 0.0,
        };
        self.atmosphere_step();
        if kind.dry() && self.rr() < pdt(0.0004 * i, self.dt) {
            for _ in 0..40 {
                let j = self.rng.gen_range(0..self.n);
                if self.pl[j] >= 2 && self.ignite(j, None) {
                    self.logmsg("Wildfire sparked by the heat".into(), [255, 150, 70]);
                    break;
                }
            }
        }
    }

    pub fn lightning(&mut self, target: Option<usize>) {
        let mut j: Option<usize> = target;
        if target.is_none() {
            for _ in 0..30 {
                let k = self.rng.gen_range(0..self.n);
                if self.terr[k] == 0 || self.terr[k] == 3 {
                    j = Some(k);
                    break;
                }
            }
        }
        let Some(j) = j else { return };
        let (x, y) = ((j % self.w) as f32, (j / self.w) as f32);
        self.flash = 1.0;
        self.bolt = Some((x as i32, y as i32, 10));
        for ci in 0..self.crs.len() {
            let c = &self.crs[ci];
            if !c.dead && (c.x - x).powi(2) + (c.y - y).powi(2) < 5.0 {
                self.kill_idx(ci, true);
            }
        }
        if self.rr() < 0.6 && self.ignite(j, None) {
            self.logmsg("Lightning strike starts a fire".into(), [255, 210, 90]);
        }
    }

    pub fn ignite(&mut self, i: usize, ttl: Option<i32>) -> bool {
        let k = i as u32;
        if self.fire.len() > 4000 || self.fire.contains_key(&k) || self.ash.contains_key(&k) || !(self.terr[i] == 0 || self.terr[i] == 3) {
            return false;
        }
        let t = ttl.unwrap_or_else(|| self.rng.gen_range(18..=40));
        self.fire.insert(k, t);
        let ch = self.chunk(i) as u32;
        self.fchunk.entry(ch).or_default().push(k);
        true
    }

    fn fire_step(&mut self) {
        if self.fire.is_empty() {
            return;
        }
        let wi = self.wx.int;
        let base_spread = 0.045 * (1.0 + if self.wx.kind.dry() { 1.2 * wi } else { 0.0 });
        let mut done: Vec<(u32, bool)> = Vec::new();
        let mut entries: Vec<(u32, i32)> = self.fire.iter().map(|(&k, &v)| (k, v)).collect();
        entries.sort_unstable_by_key(|e| e.0);
        let dt = self.dt;
        for (k, ttl) in entries {
            let i = k as usize;
            let wet = self.atmosphere.rain_at((i % self.w) as f32, (i / self.w) as f32);
            let spread = base_spread * (1.0 - 0.92 * wet) * (1.0 - 0.5 * self.moisture[i] as f32 / 255.0);
            if wet > 0.0 && self.rr() < pdt(0.1 * wet, dt) {
                done.push((k, false));
                continue;
            }
            if ttl <= 1 {
                done.push((k, true));
                continue;
            }
            self.fire.insert(k, ttl - dt);
            let x = i % self.w;
            let cand = [
                if x > 0 { Some(i - 1) } else { None },
                if x < self.w - 1 { Some(i + 1) } else { None },
                i.checked_sub(self.w),
                if i + self.w < self.n { Some(i + self.w) } else { None },
            ];
            for j in cand.into_iter().flatten() {
                let jk = j as u32;
                if !self.fire.contains_key(&jk) && (self.terr[j] == 0 || self.terr[j] == 3) && !self.ash.contains_key(&jk) {
                    // succulent (water-filled) plants barely burn
                    let p = spread * if self.pl[j] != 0 { 1.0 - 0.7 * self.vegetation.lineage(j).g[veg::SUCCULENCE] } else { 0.08 };
                    if self.rr() < pdt(p, dt) {
                        self.ignite(j, None);
                    }
                }
            }
        }
        for (k, burnt) in done {
            self.fire.remove(&k);
            let ch = self.chunk(k as usize) as u32;
            if let Some(v) = self.fchunk.get_mut(&ch) {
                v.retain(|&x| x != k);
                if v.is_empty() {
                    self.fchunk.remove(&ch);
                }
            }
            if burnt {
                let i = k as usize;
                let (had, id) = (self.pl[i] > 0, self.vegetation.ids[i]);
                let fr = self.vegetation.lineages[id as usize].g[veg::FIRE_RES];
                if had && self.rr() < fr {
                    // fire-adapted: resprouts from the root and the heat opens its seed cones
                    self.setp(i, 1, Some(id));
                    for _ in 0..2 {
                        let j = i as i64 + self.rng.gen_range(-3..=3) + self.w as i64 * self.rng.gen_range(-3..=3);
                        if j >= 0 && (j as usize) < self.n && self.pl[j as usize] == 0 && self.terr[j as usize] == 0 {
                            self.sow_lineage(j as usize, id);
                        }
                    }
                } else {
                    self.setp(i, 0, None);
                }
                self.ash.insert(k, 1800);
                self.fert[i] = (self.fert[i] + 0.08).min(1.0);
            }
        }
    }

    fn fire_near(&self, x: f32, y: f32, r: f32) -> Option<(f32, f32)> {
        let (cx, cy) = (x as i64 / CH as i64, y as i64 / CH as i64);
        let mut best: Option<(f32, f32)> = None;
        let mut bd = r * r;
        for gy in cy - 3..=cy + 3 {
            for gx in cx - 3..=cx + 3 {
                if gx < 0 || gy < 0 || gx >= self.cwc as i64 || gy >= self.chc as i64 {
                    continue;
                }
                if let Some(v) = self.fchunk.get(&((gy as usize * self.cwc + gx as usize) as u32)) {
                    for &i in v {
                        let dx = (i as usize % self.w) as f32 - x;
                        let dy = (i as usize / self.w) as f32 - y;
                        let d = dx * dx + dy * dy;
                        if d < bd {
                            bd = d;
                            best = Some((dx, dy));
                        }
                    }
                }
            }
        }
        let (bx, by) = best?;
        let d = bd.sqrt() + 1e-6;
        Some((-bx / d, -by / d))
    }

    pub fn meteor_spawn(&mut self, at: Option<(i32, i32)>, big: bool) {
        if self.meteor.is_some() {
            return;
        }
        let (x, y) = match at {
            Some(p) => p,
            None => {
                let mut j = 0;
                for _ in 0..40 {
                    j = self.rng.gen_range(0..self.n);
                    if self.terr[j] == 0 || self.terr[j] == 3 {
                        break;
                    }
                }
                ((j % self.w) as i32, (j / self.w) as i32)
            }
        };
        let r = if big { self.rng.gen_range(18..=26) } else { [4, 5, 5, 6, 7, 9][self.rng.gen_range(0..6)] };
        self.meteor = Some(Meteor { x, y, r, ttl: 100 });
        self.banner = Some(("METEOR INCOMING".into(), 140, [255, 190, 110]));
        self.logmsg(format!("A {} meteor streaks across the sky!", if big { "HUGE" } else { "small" }), [255, 190, 110]);
    }

    fn meteor_hit(&mut self) {
        let Some(m) = self.meteor.take() else { return };
        let (cx, cy, r) = (m.x, m.y, m.r);
        self.flash = 1.6;
        let big = r >= 15;
        self.banner = Some((if big { "EXTINCTION-LEVEL IMPACT!" } else { "IMPACT!" }.into(), 220, [255, 120, 90]));
        let rr_ = (r as f32 * 2.4) as i32;
        let (w, h) = (self.w as i32, self.h as i32);
        for y in (cy - rr_).max(1)..(cy + rr_ + 1).min(h - 1) {
            for x in (cx - rr_).max(1)..(cx + rr_ + 1).min(w - 1) {
                let d = (((x - cx) * (x - cx) + (y - cy) * (y - cy)) as f32).sqrt();
                if d > rr_ as f32 {
                    continue;
                }
                let i = (y * w + x) as usize;
                if self.terr[i] == 1 {
                    continue;
                }
                let nz: i32 = self.rng.gen_range(-8..=8);
                let rf = r as f32;
                if d <= rf {
                    if self.terr[i] != 2 {
                        self.terr[i] = 0;
                    }
                    self.setp(i, 0, None);
                    self.fert[i] = 0.02;
                    self.fire.remove(&(i as u32));
                    self.ash.remove(&(i as u32));
                    self.bg[i] = [(50 + nz) as u8, (40 + nz) as u8, (36 + nz) as u8];
                } else if d <= rf * 1.35 {
                    if self.rr() < 0.55 {
                        self.terr[i] = 2;
                        self.setp(i, 0, None);
                        self.bg[i] = [(112 + nz) as u8, (106 + nz) as u8, (102 + nz) as u8];
                    }
                } else {
                    if d <= rf * 2.0 && self.rr() < 0.6 {
                        self.setp(i, 0, None);
                    }
                    if self.rr() < 0.3 {
                        let ttl = self.rng.gen_range(25..=60);
                        self.ignite(i, Some(ttl));
                    }
                }
            }
        }
        // fires inside the crater were removed from the map; drop stale chunk entries
        let fire = &self.fire;
        self.fchunk.retain(|_, v| {
            v.retain(|k| fire.contains_key(k));
            !v.is_empty()
        });
        self.bg_version += 1;
        for ci in 0..self.crs.len() {
            let c = &mut self.crs[ci];
            if c.dead {
                continue;
            }
            let d = ((c.x - cx as f32).powi(2) + (c.y - cy as f32).powi(2)).sqrt();
            let rf = r as f32;
            if d <= rf * 1.3 {
                self.kill_idx(ci, false);
            } else if d <= rf * 2.4 {
                c.e -= 0.75 * c.max_e;
            } else if d <= rf * 4.0 {
                c.e -= 0.2 * c.max_e;
            }
        }
        self.dust = (self.dust + (r as f32 / 20.0).powi(2)).min(1.0);
        let msg = format!(
            "IMPACT! crater {:.0} cm wide{}",
            r as f32 * 2.0 * CELL_CM,
            if self.dust > 0.4 { " - impact winter begins" } else { "" }
        );
        self.logmsg(msg, [255, 120, 90]);
    }

    fn corpse_step(&mut self) {
        let dt = self.dt;
        for b in self.bones.iter_mut() {
            b.ttl -= dt as i64;
        }
        self.bones.retain(|b| b.ttl > 0);
        let every12 = self.every(12, 0);
        for ki in 0..self.cor.len() {
            {
                let d_t = self.d_t;
                let k = &mut self.cor[ki];
                k.ttl -= dt as f32 * (1.0 + d_t * 2.0).max(0.25) * if k.buried { 0.6 } else { 1.0 };
                k.e *= 0.9995f32.powi(dt);
            }
            let k = &self.cor[ki];
            if k.has_bones && k.bone.is_none() && (k.ttl <= 300.0 || k.dead || k.e <= 0.0) {
                let id = self.nkid;
                self.nkid += 1;
                self.bones.push(Bo { id, x: k.x, y: k.y, size: k.size, ttl: DAY * 8, buried: k.buried });
                self.cor[ki].bone = Some(id);
            }
            if self.cor[ki].inf.is_none() && self.cor[ki].ttl < 650.0 && self.rr() < pdt(0.001, dt) {
                if self.environment_strain.is_none() {
                    let st = self.new_strain(None);
                    self.strains[st as usize].center = 0.5;
                    self.strains[st as usize].width = 0.8;
                    self.environment_strain = Some(st);
                    let msg = format!("Rotting remains harbor {}", self.strains[st as usize].name);
                    self.logmsg(msg, [170, 210, 100]);
                }
                self.cor[ki].inf = self.environment_strain;
            }
            if let Some(inf) = self.cor[ki].inf {
                if every12 {
                    let (kx, ky, buried) = (self.cor[ki].x, self.cor[ki].y, self.cor[ki].buried);
                    let (gx0, gx1, gy0, gy1) = self.grid_range(kx, ky, 5.0);
                    let mut cands = Vec::new();
                    for gy in gy0..=gy1 {
                        for gx in gx0..=gx1 {
                            cands.extend(self.cgrid[gy * self.gwc + gx].iter().map(|&j| j as usize));
                        }
                    }
                    for j in cands {
                        let c = &self.crs[j];
                        if c.sick.is_some() || c.dead || c.z > 0.5 {
                            continue;
                        }
                        let distance = (kx - c.x).powi(2) + (ky - c.y).powi(2);
                        if distance > 25.0 {
                            continue;
                        }
                        let barrier = if buried { 0.01 } else { 1.0 };
                        let trans = self.strains[inf as usize].trans;
                        let sc = susc(&mut self.crs[j], &self.strains[inf as usize], self.t);
                        let risk = (trans * 2.0 * sc * barrier * (1.0 - distance / 25.0)).min(0.8);
                        if risk > 0.0 && self.rr() < 1.0 - (1.0 - risk).powi(12) {
                            self.infect_idx(j, inf);
                        }
                    }
                }
            }
            let k = &self.cor[ki];
            if k.ttl <= 0.0 || k.e <= 0.0 {
                if k.e > 0.0 && k.ttl <= 0.0 {
                    let i = k.y as usize * self.w + k.x as usize;
                    let cells: Vec<usize> = std::iter::once(i).chain(self.neighbors(i)).collect();
                    for j in cells {
                        if self.terr[j] == 0 {
                            self.fert[j] = (self.fert[j] + 0.05).min(1.0);
                            if self.pl[j] == 0 {
                                self.sprout(j);
                            }
                        }
                    }
                }
                self.cor[ki].dead = true;
            }
        }
        self.cor.retain(|k| !k.dead);
        self.kmap = self.cor.iter().enumerate().map(|(i, c)| (c.id, i)).collect();
    }

    fn burial_step(&mut self, c: &mut Cr, kid: u32) {
        let Some(&ki) = self.kmap.get(&kid) else { return };
        let k = &mut self.cor[ki];
        if k.dead || k.buried || c.g.v[BURIAL] < 0.55 {
            return;
        }
        k.burial += self.dt as f32 * (0.5 + c.g.v[BURIAL]);
        c.e -= 0.0015 * c.max_e * self.dt as f32;
        if k.burial >= 60.0 {
            k.buried = true;
            if let Some(b) = k.bone {
                if let Some(bo) = self.bones.iter_mut().find(|bo| bo.id == b) {
                    bo.buried = true;
                }
            }
            if self.milestones.insert("burial".into()) {
                let msg = format!("EVOLUTION: {} buries infectious remains", self.species[c.sp as usize].name);
                self.logmsg(msg, c.col);
            }
        }
    }

    pub fn earthquake(&mut self, at: Option<(i32, i32, i32)>) {
        let (x, y, radius) = at.unwrap_or_else(|| {
            (
                self.rng.gen_range(1..self.w as i32 - 1),
                self.rng.gen_range(1..self.h as i32 - 1),
                self.rng.gen_range(20..=50),
            )
        });
        self.quake = Some((x, y, radius, 100));
        self.banner = Some(("EARTHQUAKE".into(), 160, [235, 185, 120]));
        self.logmsg("Earthquake: landslide and habitat damage".into(), [235, 185, 120]);
        for ci in 0..self.crs.len() {
            let c = &mut self.crs[ci];
            if c.dead {
                continue;
            }
            let d = ((c.x - x as f32).powi(2) + (c.y - y as f32).powi(2)).sqrt();
            if d < radius as f32 {
                c.e -= c.max_e * 0.65 * (1.0 - d / radius as f32) * (1.0 - 0.4 * c.g.v[ARMOR]);
                if c.e <= 0.0 {
                    self.kill_idx(ci, true);
                }
            }
        }
        for _ in 0..radius * 8 {
            let xx = (x as f32 + gauss(&mut self.rng, 0.0, radius as f32 / 2.0)) as i32;
            let yy = (y as f32 + gauss(&mut self.rng, 0.0, radius as f32 / 2.0)) as i32;
            if xx < 0 || yy < 0 || xx >= self.w as i32 || yy >= self.h as i32 {
                continue;
            }
            let i = yy as usize * self.w + xx as usize;
            if !(self.terr[i] == 0 || self.terr[i] == 3) {
                continue;
            }
            self.setp(i, 0, None);
            self.fert[i] *= 0.8;
            self.bg[i] = [100, 87, 65];
        }
        self.bg_version += 1;
    }

    // ---- perception (read-only; runs in parallel)
    fn find_plant(&self, x: f32, y: f32, vis: f32, avoid: Option<(f32, f32)>, swim: bool) -> Option<usize> {
        let (cx, cy) = (x as i64 / CH as i64, y as i64 / CH as i64);
        let mut best: Option<usize> = None;
        let mut bd = f32::MAX;
        let rmax = vis as i64 / CH as i64 + 2;
        for r in 0..rmax {
            for gy in cy - r..=cy + r {
                if gy < 0 || gy >= self.chc as i64 {
                    continue;
                }
                let edge = gy == cy - r || gy == cy + r;
                let step = if edge { 1 } else { (2 * r).max(1) };
                let mut gx = cx - r;
                while gx <= cx + r {
                    if gx >= 0 && gx < self.cwc as i64 {
                        let k = gy as usize * self.cwc + gx as usize;
                        if self.pcount[k] > 0 {
                            let x0 = gx as usize * CH;
                            let y0 = gy as usize * CH;
                            for yy in y0..(y0 + CH).min(self.h) {
                                let row = yy * self.w;
                                for xx in x0..(x0 + CH).min(self.w) {
                                    if self.pl[row + xx] >= 2 && (swim || self.terr[row + xx] != 1) {
                                        let dx = xx as f32 - x;
                                        let dy = yy as f32 - y;
                                        let d = dx * dx + dy * dy;
                                        if d < bd {
                                            if let Some((tox, thorn)) = avoid {
                                                let pg = &self.vegetation.lineage(row + xx).g;
                                                if pg[veg::TOXIN] > tox || pg[veg::THORNS] > thorn {
                                                    continue;
                                                }
                                            }
                                            bd = d;
                                            best = Some(row + xx);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    gx += step;
                }
            }
            if best.is_some() && bd <= ((r * CH as i64) as f32).powi(2) {
                break;
            }
        }
        if bd <= vis * vis {
            best
        } else {
            None
        }
    }

    fn look(&self, ci: usize) -> (Option<u32>, Option<Tgt>) {
        let c = &self.crs[ci];
        let g = &c.g;
        let dk = self.dark;
        let [bold, social, _curio] = c.pers;
        let vis = g.v[VIS]
            * if g.has(NOCTURNAL) { 0.65 + 0.9 * dk } else { 1.0 - 0.35 * dk }
            * self.wx_vis
            * (1.0 - 0.3 * self.atmosphere.rain_at(c.x, c.y));
        let (x, y, v2, cs, sp) = (c.x, c.y, vis * vis, c.s, c.sp);
        let (mut thr, mut prey, mut mate) = (None, None, None);
        let (mut td, mut pd, mut md) = (f32::MAX, f32::MAX, f32::MAX);
        let (mut hx, mut hy, mut hn) = (0.0, 0.0, 0);
        let ready = g.v[COMPLEXITY] >= 0.42 && c.male && c.age >= g.v[MAT] && c.cool <= 0 && c.e >= g.v[THRESH] * c.max_e * 0.8;
        let carn = g.v[DIET] > 0.4;
        let (gx0, gx1, gy0, gy1) = self.grid_range(x, y, vis);
        for gy in gy0..=gy1 {
            for gx in gx0..=gx1 {
                for &oj in &self.cgrid[gy * self.gwc + gx] {
                    let o = &self.crs[oj as usize];
                    if oj as usize == ci || o.dead || (c.z - o.z).abs() > 2.0 {
                        continue;
                    }
                    let (dx, dy) = (o.x - x, o.y - y);
                    let mut d = dx * dx + dy * dy;
                    let og = &o.g;
                    if og.has(CAMOUFLAGE) {
                        d *= 3.0;
                    }
                    if d > v2 {
                        continue;
                    }
                    let kin = o.sp == sp;
                    if kin {
                        hx += o.x;
                        hy += o.y;
                        hn += 1;
                    }
                    if ready
                        && !o.male
                        && o.preg.is_none()
                        && d < md
                        && o.cool <= 0
                        && o.age >= og.v[MAT]
                        && o.e >= og.v[THRESH] * o.max_e * 0.7
                        && (kin || dist(og, g) < 0.3)
                    {
                        mate = Some(o.id);
                        md = d;
                    } else if kin {
                        continue;
                    } else if og.v[DIET] > 0.5 && o.s > cs * 0.85 {
                        if d < td {
                            thr = Some(o.id);
                            td = d;
                        }
                    } else if carn && o.s < cs * 0.9 && d < pd && dist(og, g) > 0.25 {
                        prey = Some(o.id);
                        pd = d;
                    }
                }
            }
        }
        let fr = if g.has(SKITTISH) { 1.0 } else { 0.75 - 0.35 * bold };
        let thr = if thr.is_some() && td <= (vis * fr).powi(2) { thr } else { None };
        let mut tg = None;
        let hunger = c.e / c.max_e;
        let aggr = (g.v[AGGR] + 0.3 * (bold - 0.5)).clamp(0.0, 1.0);
        if thr.is_none() {
            let mut burial_target = None;
            if g.v[BURIAL] > 0.55 && g.v[COMPLEXITY] > 0.6 && hunger > 0.55 {
                let mut bd = f32::MAX;
                self.for_corpses(x, y, vis, |k| {
                    let d = (k.x - x).powi(2) + (k.y - y).powi(2);
                    let cell = self.terr[k.y as usize * self.w + k.x as usize];
                    if !k.dead && !k.buried && (k.inf.is_some() || k.ttl < 650.0) && d < v2 && (cell == 0 || cell == 3) && d < bd {
                        bd = d;
                        burial_target = Some(k.id);
                    }
                });
            }
            if let Some(b) = burial_target {
                tg = Some(Tgt::Bury(b));
            } else if let Some(m) = mate {
                tg = Some(Tgt::Mate(m));
            } else {
                let diet = g.v[DIET];
                if hunger < 0.92 {
                    if let Some(p) = prey {
                        if hunger < 0.3 + 0.6 * aggr {
                            tg = Some(Tgt::Prey(p));
                        }
                    }
                    if tg.is_none() && (diet > 0.45 || g.v[SCAVENGE] > 0.25 || g.has(SCAVENGER)) {
                        let mut bk = None;
                        let mut bd = v2;
                        self.for_corpses(x, y, vis, |k| {
                            let d = (k.x - x).powi(2) + (k.y - y).powi(2);
                            if d < bd && !k.dead && !k.buried {
                                bk = Some(k.id);
                                bd = d;
                            }
                        });
                        if let Some(k) = bk {
                            tg = Some(Tgt::Corpse(k));
                        }
                    }
                    if tg.is_none() && diet < 0.7 {
                        // picky unless starving: skip plants too toxic/thorny for this body
                        let avoid = if hunger < 0.25 { None } else { Some((0.12 + 0.7 * g.v[IMMUNE], 0.12 + 0.7 * g.v[ARMOR])) };
                        if let Some(i) = self.find_plant(x, y, vis, avoid, can_swim(g) || c.z > 0.5) {
                            tg = Some(Tgt::Plant(i as u32));
                        }
                    }
                }
                if tg.is_none() && hn >= 2 && (g.has(HERDER) || social > 0.65) {
                    let (mx, my) = (hx / hn as f32, hy / hn as f32);
                    if (mx - x).powi(2) + (my - y).powi(2) > 36.0 + 100.0 * (1.0 - social) {
                        tg = Some(Tgt::Herd(mx, my));
                    }
                }
            }
        }
        (thr, tg)
    }

    fn for_corpses(&self, x: f32, y: f32, r: f32, mut f: impl FnMut(&Co)) {
        let (gx0, gx1, gy0, gy1) = self.grid_range(x, y, r);
        for gy in gy0..=gy1 {
            for gx in gx0..=gx1 {
                for &k in &self.kgrid[gy * self.gwc + gx] {
                    if let Some(c) = self.cor.get(k as usize) {
                        f(c);
                    }
                }
            }
        }
    }

    #[inline]
    pub fn ok(&self, x: f32, y: f32, sw: bool) -> bool {
        if x < 1.0 || y < 1.0 || x >= (self.w - 1) as f32 || y >= (self.h - 1) as f32 {
            return false;
        }
        let i = y as usize * self.w + x as usize;
        let t = self.terr[i];
        // rivers can be waded across; lakes and the sea need swimmers
        t == 0 || t == 3 || (t == 1 && (sw || self.biome.get(i) == Some(&(crate::worldgen::Biome::River as u8))))
    }

    fn target_pos(&self, tg: Tgt) -> Option<(f32, f32)> {
        match tg {
            Tgt::Plant(i) => {
                if self.pl[i as usize] >= 2 {
                    Some(((i as usize % self.w) as f32 + 0.5, (i as usize / self.w) as f32 + 0.5))
                } else {
                    None
                }
            }
            Tgt::Herd(x, y) => Some((x, y)),
            Tgt::Prey(id) | Tgt::Mate(id) => self.idmap.get(&id).map(|&j| &self.crs[j]).filter(|o| !o.dead).map(|o| (o.x, o.y)),
            Tgt::Corpse(id) | Tgt::Bury(id) => self.kmap.get(&id).map(|&j| &self.cor[j]).filter(|k| !k.dead).map(|k| (k.x, k.y)),
        }
    }

    /// One bite of a plant: energy, poison/thorn damage, the plant's fate and seeds in the gut.
    fn graze(&mut self, c: &mut Cr, o: usize, glut: f32) {
        let t = self.t;
        let plant = *self.vegetation.lineage(o);
        let pg = &plant.g;
        // a meal scales with the eater: a microscopic colony gets only a nibble
        let mouth = if c.g.v[COMPLEXITY] < 0.42 { (c.s / 2.0).clamp(0.05, 1.0) } else { 1.0 };
        c.e += (1.0 - 0.6 * pg[veg::DEFENSE]) * (1.0 - 0.7 * pg[veg::TOXIN]) * self.pl[o] as f32 * 2.2 * (1.0 - c.g.v[DIET] * 1.4).max(0.05) * (0.6 + 0.8 * c.m34) * glut * mouth;
        // poison hurts unless the immune system copes; thorns hurt unless armored
        c.e -= pg[veg::TOXIN] * 10.0 * (1.0 - 0.8 * c.g.v[IMMUNE]);
        c.e -= pg[veg::THORNS] * 6.0 * (1.0 - 0.8 * c.g.v[ARMOR]);
        // woody, resprouting, clonal or defended plants usually lose only a bite and regrow
        let tough = pg[veg::WOOD] > 0.3
            || self.rr() < 0.5 * pg[veg::CLONAL] + 0.3 * pg[veg::FIRE_RES] + 0.9 * pg[veg::TOXIN] + 0.9 * pg[veg::THORNS] + 0.6 * pg[veg::DEFENSE];
        let st = if tough { 1 } else if self.rr() < 0.5 { 0 } else { 1 };
        self.setp(o, st, None);
        if self.rr() < 0.85 {
            let due = (c.gut.back().map_or(0, |l| l.0 + 5)).max(t + self.rng.gen_range(140..=260));
            // fruiting plants (bloom) are far more likely to have viable seeds carried off
            if self.rr() < 0.35 + 0.65 * pg[veg::BLOOM] {
                c.gut.push_back((due, plant.id));
            }
        }
    }

    fn grown_plant_near(&self, x: f32, y: f32, reach: f32) -> Option<usize> {
        let r = reach.ceil() as i64;
        let (cx, cy) = (x as i64, y as i64);
        for yy in (cy - r).max(0)..=(cy + r).min(self.h as i64 - 1) {
            for xx in (cx - r).max(0)..=(cx + r).min(self.w as i64 - 1) {
                let i = yy as usize * self.w + xx as usize;
                if self.pl[i] >= 2 && (xx as f32 + 0.5 - x).powi(2) + (yy as f32 + 0.5 - y).powi(2) < reach * reach {
                    return Some(i);
                }
            }
        }
        None
    }

    fn live(&mut self, ci: usize, look: Option<(Option<u32>, Option<Tgt>)>) {
        let mut c = std::mem::replace(&mut self.crs[ci], Cr::placeholder());
        self.live_inner(&mut c, look);
        self.crs[ci] = c;
    }

    fn live_inner(&mut self, c: &mut Cr, look: Option<(Option<u32>, Option<Tgt>)>) {
        let t = self.t;
        let dt = self.dt;
        let dtf = dt as f32;
        let q = c.g.q;
        c.age += dtf;
        let gr = (c.age / c.g.v[MAT]).min(1.0);
        if gr < 1.0 {
            c.pf = 0.75 + 0.4 * c.nut;
        }
        c.s = c.g.v[SIZE] * c.pf * (0.35 + 0.65 * (1.0 - (1.0 - gr).powi(2)));
        let mm = MK * c.s.powi(3);
        c.m34 = mm.powf(0.75);
        c.max_e = 60.0 + 110.0 * mm;
        if c.cool > 0 {
            c.cool -= dt;
        }
        if c.g.v[COMPLEXITY] < 0.42 && c.age >= c.g.v[MAT] && c.cool <= 0 && c.e > 0.75 * c.max_e && self.crs.len() < self.maxpop {
            let (h1, _) = gamete(&mut self.rng, &c.hap.0, &c.hap.1);
            let (h2, note) = gamete(&mut self.rng, &c.hap.0, &c.hap.1);
            let child_g = phenotype(&mut self.rng, &h1, &h2);
            let sp = self.classify(&child_g, c.sp);
            let energy = c.e * 0.4;
            self.spawn(h1, h2, sp, Some((c.id, c.gen)), Some((c.x, c.y)), Some(energy), &note, None, Some(child_g));
            c.e -= energy;
            c.cool = c.g.v[MAT] as i32;
        }
        c.nut += ((c.e / c.max_e).min(1.0) - c.nut) * (1.0 - 0.997f32.powi(dt));
        if let Some((due, _)) = &c.preg {
            if t >= *due {
                let (_, sperm) = c.preg.take().unwrap();
                self.deliver(c, sperm);
                c.cool = 500;
            }
        }
        if let Some((thr, tg)) = look {
            c.thr = thr;
            c.tgt = tg;
        }
        let (x, y) = (c.x, c.y);
        let idx = y as usize * self.w + x as usize;
        let temp = self.atmosphere.temperature(&self.tmp, self.d_t, self.w, idx);
        if self.fchunk.is_empty() {
            c.fv = None;
        } else if self.every(3, c.id as i64) {
            c.fv = self.fire_near(x, y, 28.0);
        }
        let mut sickf = 0.0;
        if let Some((st, t0)) = c.sick {
            let ag = t - t0;
            let s = &self.strains[st as usize];
            if ag >= s.incub {
                if ag < s.incub + s.dur {
                    sickf = s.vir * (1.0 - 0.55 * c.g.v[IMMUNE]);
                    c.e -= sickf * 0.0006 * c.max_e * dtf;
                    if self.every(3, c.id as i64) {
                        self.spread(c, st);
                    }
                } else {
                    c.imm.push((st, t + 4000));
                    c.sick = None;
                }
            }
        }
        let hib = q & HIBERNATOR != 0 && temp < 0.32;
        let (mut ux, mut uy) = (0f32, 0f32);
        let mut mul = 0.55 + 0.25 * c.pers[2];
        let hunger = c.e / c.max_e;
        let thr_pos = c.thr.and_then(|id| self.idmap.get(&id)).map(|&j| &self.crs[j]).filter(|o| !o.dead).map(|o| (o.x, o.y));
        if let Some((fx, fy)) = c.fv {
            ux = fx;
            uy = fy;
            mul = 1.6;
        } else if let Some((ox, oy)) = thr_pos {
            let (dx, dy) = (x - ox, y - oy);
            let d = (dx * dx + dy * dy).sqrt() + 1e-6;
            ux = dx / d;
            uy = dy / d;
            mul = if q & SKITTISH != 0 { 1.4 } else { 1.2 };
        } else if let Some(tg) = c.tgt {
            if let Some((tx, ty)) = self.target_pos(tg) {
                let (dx, dy) = (tx - x, ty - y);
                let d = (dx * dx + dy * dy).sqrt() + 1e-6;
                ux = dx / d;
                uy = dy / d;
                mul = match tg {
                    Tgt::Plant(_) => 0.8,
                    Tgt::Prey(_) => 1.1,
                    Tgt::Corpse(_) => 0.9,
                    Tgt::Mate(_) => 0.9,
                    Tgt::Herd(..) => 0.6,
                    Tgt::Bury(_) => 0.7,
                };
            } else {
                c.tgt = None;
            }
        }
        if c.thr.is_some() && thr_pos.is_none() {
            c.thr = None;
        }
        if aquatic(&c.g) && self.terr[idx] != 1 {
            let (xi, yi) = (x as i32, y as i32);
            let mut best = None;
            let mut bd = f32::MAX;
            for yy in (yi - 6).max(1)..(yi + 7).min(self.h as i32 - 1) {
                for xx in (xi - 6).max(1)..(xi + 7).min(self.w as i32 - 1) {
                    if self.terr[yy as usize * self.w + xx as usize] == 1 {
                        let d = (xx as f32 - x).powi(2) + (yy as f32 - y).powi(2);
                        if d < bd {
                            bd = d;
                            best = Some((xx as f32, yy as f32));
                        }
                    }
                }
            }
            if let Some((tx, ty)) = best {
                let d = ((tx - x).powi(2) + (ty - y).powi(2)).sqrt() + 1e-6;
                ux = (tx - x) / d;
                uy = (ty - y) / d;
            }
        }
        if ux == 0.0 && uy == 0.0 {
            if let Some(mid) = c.mom {
                if c.age < c.nurse {
                    if let Some(&mj) = self.idmap.get(&mid) {
                        let m = &self.crs[mj];
                        if !m.dead {
                            let (dx, dy) = (m.x - x, m.y - y);
                            let d = (dx * dx + dy * dy).sqrt();
                            if d > 6.0 {
                                ux = dx / d;
                                uy = dy / d;
                                mul = 1.0;
                            }
                        }
                    }
                }
            }
            if ux == 0.0 && uy == 0.0 {
                c.wa += self.rng.gen_range(-0.3..0.3) * dtf.sqrt();
                ux = c.wa.cos();
                uy = c.wa.sin();
            }
        }
        c.rest = false;
        if c.thr.is_none() && c.fv.is_none() {
            let noct = q & NOCTURNAL != 0;
            if hib || (c.tgt.is_none() && hunger > 0.6 && ((noct && self.dark < 0.25) || (!noct && self.dark > 0.75))) {
                c.rest = true;
                mul = 0.0;
            }
        }
        let sen = 1.0 - 0.4 * ((c.age / c.g.v[LIFE] - 0.7) / 0.3).max(0.0);
        let sp = c.g.v[SPD] * mul * (0.6 + 0.4 * gr) * sen * (1.0 - 0.35 * sickf);
        let inertia = 0.6f32.powi(dt);
        c.vx = c.vx * inertia + ux * sp * (1.0 - inertia);
        c.vy = c.vy * inertia + uy * sp * (1.0 - inertia);
        let sw = can_swim(&c.g);
        let target_near = c.tgt.and_then(|tg| self.target_pos(tg)).map_or(false, |(tx, ty)| (tx - x).powi(2) + (ty - y).powi(2) < (c.s + 4.0).powi(2));
        let flying = can_fly(&c.g) && gr >= 0.7 && c.e > 0.35 * c.max_e && !c.rest && !target_near;
        c.z = if flying { (c.z + 0.25 * dtf).min(5.0) } else { (c.z - 0.35 * dtf).max(0.0) };
        if c.z > 0.0 {
            c.vx *= 1.15;
            c.vy *= 1.15;
        }
        let (mut nx, mut ny) = (x, y);
        let air = c.z > 0.5;
        let (wf, hf) = ((self.w - 1) as f32, (self.h - 1) as f32);
        let allowed = |s: &Sim, xx: f32, yy: f32| if air { xx >= 1.0 && xx < wf && yy >= 1.0 && yy < hf } else { s.ok(xx, yy, sw) };
        for _ in 0..dt {
            let (tx, ty) = (nx + c.vx, ny + c.vy);
            if allowed(self, tx, ty) {
                nx = tx;
                ny = ty;
            } else if allowed(self, tx, ny) {
                nx = tx;
            } else if allowed(self, nx, ty) {
                ny = ty;
            } else {
                c.wa = self.rr() * 6.28;
                c.tgt = None;
                c.vx = 0.0;
                c.vy = 0.0;
                break;
            }
        }
        c.x = nx;
        c.y = ny;
        let nidx = ny as usize * self.w + nx as usize;
        let in_water = self.terr[nidx] == 1 && c.z <= 0.5;
        if in_water {
            let drag = 0.55 + 0.4 * c.g.v[FINS];
            c.vx *= drag;
            c.vy *= drag;
        }
        let mut oxygen = if in_water { c.g.v[GILLS] } else { c.g.v[LUNGS] };
        if c.z > 0.5 {
            oxygen = c.g.v[LUNGS];
        }
        c.breath = if oxygen > 0.4 { (c.breath + 0.02 * dtf).min(1.0) } else { (c.breath - 0.005 * dtf).max(0.0) };
        if c.breath <= 0.0 {
            c.e -= c.max_e * 0.015 * dtf;
        }
        if c.g.v[COMPLEXITY] < 0.42 && in_water {
            // photosynthesis / filter feeding draws on the local nutrient pool, shared with neighbours
            let want = 0.035 * dtf * (1.0 - self.dark) * (0.4 + self.moisture[idx] as f32 / 255.0);
            let gi = (c.y as usize / CG).min(self.ghc - 1) * self.gwc + (c.x as usize / CG).min(self.gwc - 1);
            if let Some(pool) = self.nutrients.get_mut(gi) {
                let take = want.min(*pool);
                *pool -= take;
                c.e += take;
            }
        }
        if c.vx > 0.04 {
            c.f = 1;
        } else if c.vx < -0.04 {
            c.f = -1;
        }
        let g = &c.g;
        let v = (c.vx * c.vx + c.vy * c.vy).sqrt();
        let d_t = (temp - g.v[TOL]).abs() - 0.15 - g.v[FUR] * (g.v[TOL] - temp).max(0.0) * 0.5;
        let therm = if d_t > 0.0 { 1.0 + 8.0 * d_t * d_t } else { 1.0 };
        let orn = if c.male { (g.v[HORN] + g.v[TAIL]) * 0.5 } else { 0.0 };
        let mut cost = (KREST * c.m34 * g.v[METAB] + KMOVE * c.m34 * v * v + 0.0002 * g.v[VIS])
            * therm
            * (1.0 + 0.25 * orn)
            * (1.0 + 0.12 * g.v[IMMUNE] + 0.12 * g.v[ARMOR] + 0.08 * g.v[FUR]);
        cost *= 0.2 + 0.8 * g.v[COMPLEXITY];
        if c.z > 0.5 {
            cost *= 1.8 + g.v[SIZE] * 0.08;
        }
        if c.preg.is_some() {
            cost *= 1.2;
        }
        if q & GLUTTON != 0 {
            cost *= 1.15;
        }
        if c.rest {
            cost *= if hib { 0.3 } else { 0.8 };
        }
        c.e -= cost * dtf;
        if c.z <= 0.5 && !self.fire.is_empty() && self.fire.contains_key(&(nidx as u32)) {
            c.e -= c.max_e * 0.08 * dtf;
        }
        // ---- act on target
        if let (Some(tg), true) = (c.tgt, c.z <= 0.5) {
            let glut = if q & GLUTTON != 0 { 1.3 } else { 1.0 };
            let reach = c.s * 0.5 + 1.8;
            match tg {
                Tgt::Plant(o) => {
                    let o = o as usize;
                    let (px, py) = ((o % self.w) as f32 + 0.5, (o / self.w) as f32 + 0.5);
                    if (c.x - px).powi(2) + (c.y - py).powi(2) < reach * reach && self.pl[o] >= 2 {
                        // A coarse step (dt>1) stands for several ticks of grazing: keep eating
                        // neighbouring plants, as the animal would have done tick by tick.
                        let bites = 1 + (dt - 1) / 3;
                        let mut cell = Some(o);
                        let mut n = 0;
                        while let Some(o) = cell {
                            self.graze(c, o, glut);
                            n += 1;
                            if n >= bites || c.e >= c.max_e {
                                break;
                            }
                            cell = self.grown_plant_near(c.x, c.y, reach);
                        }
                        c.tgt = None;
                    }
                }
                Tgt::Bury(kid) => {
                    if let Some(&ki) = self.kmap.get(&kid) {
                        let k = &self.cor[ki];
                        if (c.x - k.x).powi(2) + (c.y - k.y).powi(2) < reach * reach {
                            self.burial_step(c, kid);
                            if self.cor[ki].buried {
                                c.tgt = None;
                            }
                        }
                    }
                }
                Tgt::Corpse(kid) => {
                    if let Some(&ki) = self.kmap.get(&kid) {
                        let k = &self.cor[ki];
                        if !k.dead && !k.buried && (c.x - k.x).powi(2) + (c.y - k.y).powi(2) < reach * reach {
                            let bite = k.e.min(8.0 + 24.0 * c.m34);
                            let eff = if q & SCAVENGER != 0 { 1.0 } else { 0.35 + 0.65 * c.g.v[SCAVENGE] };
                            c.e += bite * eff * glut;
                            let inf = k.inf;
                            self.cor[ki].e -= bite;
                            if let Some(inf) = inf {
                                let sc = susc(c, &self.strains[inf as usize], t);
                                if self.rr() < 0.5 * (1.0 - 0.85 * c.g.v[SCAVENGE]) * sc {
                                    self.infect(c, inf);
                                }
                            }
                            let k = &mut self.cor[ki];
                            k.dead = k.e <= 0.0;
                            c.tgt = None;
                        }
                    }
                }
                Tgt::Prey(pid) => {
                    if let Some(&oj) = self.idmap.get(&pid) {
                        let o = &self.crs[oj];
                        if !o.dead && (c.x - o.x).powi(2) + (c.y - o.y).powi(2) < ((c.s + o.s) * 0.5 + 1.5).powi(2) {
                            let mut alive = true;
                            if o.g.has(SPIKY) {
                                c.e -= 6.0;
                                if self.rr() < 0.4 {
                                    c.tgt = None;
                                    c.thr = None;
                                    alive = false;
                                }
                            }
                            if alive {
                                let o = &self.crs[oj];
                                let mut pk = 0.38 + 0.3 * (c.s / o.s - 0.9);
                                pk *= 1.0 - 0.45 * o.g.v[ARMOR];
                                if o.g.has(THICK_HIDE) {
                                    pk *= 0.55;
                                }
                                if self.rr() < pk {
                                    let o = &self.crs[oj];
                                    c.e += (o.e * 0.6 + o.s * 10.0) * (0.3 + 0.7 * c.g.v[DIET]) * glut; // the whole carcass, not just its fat
                                    let osick = o.sick.map(|s| s.0);
                                    let venom = o.g.has(VENOMOUS);
                                    if let Some(st) = osick {
                                        let sc = susc(c, &self.strains[st as usize], t);
                                        if self.rr() < 0.4 * sc {
                                            self.infect(c, st);
                                        }
                                    }
                                    if venom {
                                        c.e -= 25.0;
                                        if self.rr() < 0.25 {
                                            c.e -= 20.0;
                                        }
                                    }
                                    self.kill_idx(oj, true);
                                    c.tgt = None;
                                }
                            }
                        }
                    }
                }
                Tgt::Mate(mid) => {
                    if let Some(&oj) = self.idmap.get(&mid) {
                        let o = &self.crs[oj];
                        if !o.dead && o.preg.is_none() && (c.x - o.x).powi(2) + (c.y - o.y).powi(2) < ((c.s + o.s) * 0.5 + 2.0).powi(2) {
                            if c.cool <= 0 && o.cool <= 0 {
                                let ornm = (c.g.v[HORN] + c.g.v[TAIL]) * 0.5;
                                let (choosy, pro, litter, care) = (o.g.v[CHOOSY], o.g.has(PROLIFIC), o.g.v[LITTER], o.g.v[CARE]);
                                if self.rr() < 0.25 + 0.75 * (1.0 - choosy * (1.0 - ornm)) {
                                    let gest = (280.0 * if pro { 0.7 } else { 1.0 } * (0.8 + 0.4 * self.rr())) as i64;
                                    let lit = ((litter * (1.25 - 0.55 * care) + if pro { 1.0 } else { 0.0 }).round() as usize).max(1);
                                    let sperm: Vec<(Genome, String)> = (0..lit).map(|_| gamete(&mut self.rng, &c.hap.0, &c.hap.1)).collect();
                                    self.crs[oj].preg = Some((t + gest, sperm));
                                    c.cool = 350;
                                    c.e -= 0.05 * c.max_e;
                                } else {
                                    c.cool = 40;
                                }
                            }
                            c.tgt = None;
                        }
                    }
                }
                Tgt::Herd(..) => {}
            }
        }
        if !c.kids.is_empty() && self.every(10, c.id as i64) {
            let idmap = &self.idmap;
            let crs = &self.crs;
            c.kids.retain(|k| idmap.get(k).map_or(false, |&j| !crs[j].dead));
            for &kid in c.kids.iter() {
                let j = self.idmap[&kid];
                let k = &mut self.crs[j];
                if k.age < k.nurse && k.e < 0.65 * k.max_e && c.e > 0.5 * c.max_e && (k.x - c.x).powi(2) + (k.y - c.y).powi(2) < 64.0 {
                    let a = (c.e * 0.05).min(4.0);
                    c.e -= a;
                    k.e += a * 0.9;
                }
            }
        }
        c.e = c.e.min(c.max_e);
        let inf = c.sick.map(|s| s.0);
        if c.gut.front().map_or(false, |&(d, _)| d <= t) {
            let (_, lin) = c.gut.pop_front().unwrap();
            self.poop.push(Poop { x: c.x, y: c.y, ttl: 500, seed: Some(lin), inf });
        } else if self.every(420, c.id as i64 * 17) {
            self.poop.push(Poop { x: c.x, y: c.y, ttl: 500, seed: None, inf });
        }
        let r = c.age / c.g.v[LIFE];
        let die = c.e <= 0.0
            || r > 1.6
            || (sickf > 0.0 && self.rr() < pdt(sickf * sickf * 0.0004, dt))
            || (r > 0.5 && self.rr() < pdt((1e-4 * (7.0 * (r - 0.7)).exp()).min(1.0), dt));
        if die {
            self.kill(c, true);
        }
    }

    // ---- plants
    fn grow_plants(&mut self) {
        let season = (1.0 + 0.55 * (2.0 * std::f32::consts::PI * (self.t % SEASON) as f32 / SEASON as f32).sin()) * self.wx_grow * (1.0 - 0.6 * self.dust);
        // land and water have separate carrying capacities (algae must not crowd out land plants)
        let land_plants = (self.nplants - self.nwater_plants).max(0) as f32;
        let water_cells = (self.n as i64 - self.land).max(1) as f32;
        self.crowd = (1.0 - land_plants / (self.land as f32 * 0.13).max(1.0)).max(0.0);
        self.crowd_w = (1.0 - self.nwater_plants as f32 / (water_cells * 0.06)).max(0.0);
        let crowd = self.crowd;
        let crowd_w = self.crowd_w;
        // seeds keep replacing dead plants up to ~1.5x the carrying capacity (turnover drives evolution)
        let seed_crowd = crowd.max((1.0 - land_plants / (self.land as f32 * 0.19).max(1.0)).max(0.0) * 0.5);
        let seed_crowd_w = crowd_w.max((1.0 - self.nwater_plants as f32 / (water_cells * 0.09)).max(0.0) * 0.5);
        let dt = self.dt;
        let (w, h, n) = (self.w, self.h, self.n);
        let sprout = pdt((0.0008 * season).min(1.0), dt);
        // Sorted samples turn random cell lookups into a forward sweep; chunks are decided in
        // parallel (read-only, memory-latency bound) and the resulting actions applied in order.
        // Ascending samples from random gaps (mean n/ps): uniform coverage, no sort needed.
        let gap = (2 * n / self.ps).max(2) as u32;
        let mut samples: Vec<u32> = Vec::with_capacity(self.ps + 64);
        let mut pos = self.rng.gen_range(0..gap);
        while (pos as usize) < n {
            samples.push(pos);
            pos += 1 + self.rng.gen_range(0..gap - 1);
        }
        let _ta = std::time::Instant::now();
        const CHUNK: usize = 2048;
        let seeds: Vec<u64> = (0..samples.len().div_ceil(CHUNK)).map(|_| self.rng.gen()).collect();
        let winter = (self.t % SEASON) * 4 / SEASON == 3;
        let drought = if self.wx.kind.dry() { self.wx.int } else { 0.0 };
        let me: &Sim = self;
        let decide = |(ci, chunk): (usize, &[u32])| -> Vec<PlantAct> {
            let mut r = Rng::seed_from_u64(seeds[ci]);
            let mut out = Vec::new();
            for (k, &iu) in chunk.iter().enumerate() {
                let i = iu as usize;
                if let Some(&ahead) = chunk.get(k + 12) {
                    let a = ahead as usize;
                    if me.occ[a >> 6] & (1 << (a & 63)) != 0 {
                        prefetch(&me.pl[a]);
                        prefetch(&me.terr[a]);
                        prefetch(&me.tmp[a]);
                        prefetch(&me.moisture[a]);
                        prefetch(&me.fert[a]);
                        prefetch(&me.vegetation.ids[a]);
                    }
                }
                if me.occ[i >> 6] & (1 << (i & 63)) == 0 {
                    let cr = if me.terr[i] == 1 { crowd_w } else { crowd };
                    if cr > 0.0 && r.gen::<f32>() < sprout && (me.terr[i] == 0 || me.terr[i] == 1) {
                        out.push(PlantAct::Sprout(iu));
                    }
                    continue;
                }
                let st = me.pl[i];
                let terr = me.terr[i];
                let p = me.vegetation.lineage(i);
                let pg = &p.g;
                let temperature = me.temp_at(i);
                let suit = Vegetation::suitability_of(p, me.moisture[i], temperature, terr == 1);
                if suit < 0.1 && r.gen::<f32>() < 1.0 - (0.92 + 0.06 * pg[veg::GIRTH]).powi(dt) {
                    out.push(PlantAct::Die(iu));
                    continue;
                }
                // natural ageing: soft plants turn over in weeks, wood lives for months
                if st == 3 && r.gen::<f32>() < pdt(0.03 * (1.0 + 3.0 * pg[veg::ANNUAL]) / (1.0 + 6.0 * pg[veg::WOOD] + 2.0 * pg[veg::GIRTH]), dt) {
                    out.push(PlantAct::Die(iu));
                    continue;
                }
                // annuals: live fast, die back in winter (their seeds carry on)
                if winter && pg[veg::ANNUAL] > 0.5 && r.gen::<f32>() < pdt(0.004 * pg[veg::ANNUAL], dt) {
                    out.push(PlantAct::Die(iu));
                    continue;
                }
                // deciduous plants sleep through the cold season instead of growing
                let dormant = if winter || temperature < 0.3 { 1.0 - pg[veg::DECIDUOUS] } else { 1.0 };
                // carnivorous plants catch insects: nutrients even on barren soil
                let fertility = if terr == 1 { 0.5 } else { me.fert[i].max(0.15 + 0.5 * pg[veg::CARNIVORY]) };
                let shade = me.atmosphere.shade[me.atmosphere.key((i % w) as f32, (i / w) as f32)];
                // growth is limited by water actually reachable: deep roots and stored water help in dry land
                let water = if terr == 1 { 1.0 } else { (me.moisture[i] as f32 / 255.0 + 0.25 * pg[veg::ROOTS] + 0.4 * pg[veg::SUCCULENCE]).min(1.0) };
                if drought > 0.0 && water < 0.35 && r.gen::<f32>() < pdt(0.01 * drought * (1.0 - water / 0.35), dt) {
                    out.push(PlantAct::Die(iu));
                    continue;
                }
                let rate = fertility * season * suit * dormant * (0.35 + 0.65 * water) * (1.0 - 0.4 * shade) * (1.0 + 0.8 * pg[veg::ANNUAL]) / p.upkeep();
                // Neighbourhood factor = Janzen-Connell crowding x light competition (x climber support),
                // in [0.09, 1]. It needs ~14 scattered memory reads, so it is evaluated lazily: only when
                // the random draw falls in the band where it can change the outcome (exact, just cheaper).
                let pass = |r: &mut Rng, k: f32| -> bool {
                    let u = r.gen::<f32>();
                    if st < 2 {
                        return u < pdt((k * rate).min(1.0), dt);
                    }
                    if u >= pdt((k * rate).min(1.0), dt) {
                        return false;
                    }
                    if u < pdt((k * rate * 0.09).min(1.0), dt) {
                        return true;
                    }
                    let (shadow, tallest) = me.vegetation.overshadow(i, &me.pl, winter);
                    let support = if pg[veg::CLIMBING] > 0.5 && tallest < 0.3 { 0.6 } else { 1.0 };
                    let f = (1.0 - 0.6 * me.vegetation.crowding(i, &me.pl)) * (1.0 - shadow) * support;
                    u < pdt((k * rate * f).min(1.0), dt)
                };
                if st < 3 {
                    if pass(&mut r, 0.65) {
                        out.push(PlantAct::Grow(iu, st + 1));
                    }
                } else if pass(&mut r, 0.35 * if terr == 1 { seed_crowd_w } else { seed_crowd } * (1.0 + 0.6 * pg[veg::ANNUAL])) {
                    // runners/rhizomes: an identical clone next door; otherwise a seed
                    let clone = r.gen::<f32>() < 0.7 * pg[veg::CLONAL];
                    let reach = if clone { 2 } else { 2.max((3.0 + pg[veg::DISPERSAL] * 10.0 + pg[veg::BLOOM] * 6.0) as i32) };
                    let x = (i % w) as i32 + (r.gen::<f32>() * (2 * reach + 1) as f32) as i32 - reach;
                    let y = (i / w) as i32 + (r.gen::<f32>() * (2 * reach + 1) as f32) as i32 - reach;
                    if x >= 0 && x < w as i32 && y >= 0 && y < h as i32 {
                        let j = y as usize * w + x as usize;
                        if me.terr[j] != 2 && me.pl[j] == 0 {
                            out.push(if clone { PlantAct::Clone(iu, j as u32) } else { PlantAct::Seed(iu, j as u32) });
                        }
                    }
                }
            }
            out
        };
        let acts: Vec<Vec<PlantAct>> = samples.par_chunks(CHUNK).enumerate().map(decide).collect();
        self.prof[5] += _ta.elapsed().as_secs_f64();
        let _tb = std::time::Instant::now();
        for act in acts.into_iter().flatten() {
            match act {
                PlantAct::Sprout(i) => {
                    let i = i as usize;
                    if self.pl[i] == 0 {
                        self.sprout(i);
                    }
                }
                PlantAct::Die(i) => self.setp(i as usize, 0, None),
                PlantAct::Clone(i, j) => {
                    let (i, j) = (i as usize, j as usize);
                    if self.pl[i] == 0 || self.pl[j] != 0 {
                        continue;
                    }
                    let id = self.vegetation.ids[i];
                    let tj = self.temp_at(j);
                    if Vegetation::suitability_of(&self.vegetation.lineages[id as usize], self.moisture[j], tj, self.terr[j] == 1) > 0.2 {
                        self.setp(j, 1, Some(id));
                    }
                }
                PlantAct::Grow(i, st) => {
                    if self.pl[i as usize] == st - 1 {
                        self.setp(i as usize, st, None);
                    }
                }
                PlantAct::Seed(i, j) => {
                    let (i, j) = (i as usize, j as usize);
                    if self.pl[i] == 0 || self.pl[j] != 0 {
                        continue;
                    }
                    let t = self.t;
                    let annual = self.vegetation.lineage(i).g[veg::ANNUAL];
                    let child = self.vegetation.seed(&mut self.rng, i, j, t);
                    let tj = self.temp_at(j);
                    if self.vegetation.suitability(j, self.moisture[j], tj, self.terr[j] == 1) > 0.2
                        && self.rr() > 0.7 * self.vegetation.crowding(j, &self.pl)
                    {
                        self.setp(j, 1, Some(child));
                    }
                    // annuals spend themselves on seeding
                    if annual > 0.5 && self.rr() < 0.4 * annual {
                        self.setp(i, 0, None);
                    }
                }
            }
        }
        self.prof[6] += _tb.elapsed().as_secs_f64();
    }

    /// Advance the world by `dt` ticks (dt>1 = coarse integration used by fast modes).
    pub fn step(&mut self, dt: i32) {
        assert!((1..=32).contains(&dt));
        self.dt = dt;
        self.t += dt as i64;
        let t = self.t;
        let dtf = dt as f32;
        self.dark = (1.6 * (0.5 - 0.5 * (2.0 * std::f32::consts::PI * (t % DAY) as f32 / DAY as f32).cos()) - 0.3).clamp(0.0, 1.0);
let __t0 = std::time::Instant::now();
        self.weather_step();
        self.prof[0] += __t0.elapsed().as_secs_f64();
        self.water_step();
        self.dust *= 0.9996f32.powi(dt);
        self.d_t = 0.1 * (2.0 * std::f32::consts::PI * (t % SEASON) as f32 / SEASON as f32).sin() + self.wx_dt - 0.12 * self.dust;
        self.fire_step();
        if self.every(20, 0) && !self.ash.is_empty() {
            self.ash.retain(|_, v| {
                *v -= 20;
                *v > 0
            });
        }
        if let Some(m) = &mut self.meteor {
            m.ttl -= dt;
            if m.ttl <= 0 {
                self.meteor_hit();
            }
        } else if self.rr() < pdt(1.0 / 45000.0, dt) {
            let big = self.rr() < 0.12;
            self.meteor_spawn(None, big);
        }
        if let Some(q) = &mut self.quake {
            q.3 -= dt;
            if q.3 <= 0 {
                self.quake = None;
            }
        }
        if self.rr() < pdt(1.0 / 32000.0, dt) {
            self.earthquake(None);
        }
        if t >= self.next_outbreak && self.crs.len() >= 14 {
            let sick = self.crs.iter().filter(|c| c.sick.is_some()).count();
            if sick < 2.max(self.crs.len() / 10) {
                self.outbreak(None);
            }
            self.next_outbreak = t + self.rng.gen_range(DAY..=DAY * 3);
        }
        self.flash *= 0.88f32.powi(dt);
        if let Some(b) = &mut self.bolt {
            b.2 -= dt;
            if b.2 <= 0 {
                self.bolt = None;
            }
        }
        if let Some(b) = &mut self.banner {
            if b.1 > 1 {
                b.1 -= dt;
            } else {
                self.banner = None;
            }
        }
let __t1 = std::time::Instant::now();
        self.grow_plants();
        let regen = self.nutrient_regen() * dtf;
        for (p, &wf) in self.nutrients.iter_mut().zip(self.gwater.iter()) {
            *p = (*p + regen * wf).min(wf * NUTRIENT_CAP);
        }
        self.prof[1] += __t1.elapsed().as_secs_f64();
        // spatial indexes (rebuild grids before corpse_step so it sees current creatures)
        for v in self.cgrid.iter_mut() {
            v.clear();
        }
        for (j, c) in self.crs.iter().enumerate() {
            let gi = (c.y as usize / CG).min(self.ghc - 1) * self.gwc + (c.x as usize / CG).min(self.gwc - 1);
            self.cgrid[gi].push(j as u32);
        }
        // droppings: fertilise, plant seeds, spread pathogens
        let every30 = self.every(30, 0);
        let mut poop = std::mem::take(&mut self.poop);
        for p in poop.iter_mut() {
            p.ttl -= dt * if self.atmosphere.rain_at(p.x, p.y) > 0.1 { 2 } else { 1 };
            if let (Some(inf), true) = (p.inf, every30) {
                let gi = (p.y as usize / CG) * self.gwc + p.x as usize / CG;
                let cands: Vec<usize> = self.cgrid[gi].iter().map(|&j| j as usize).collect();
                for j in cands {
                    let c = &self.crs[j];
                    if (c.x - p.x).powi(2) + (c.y - p.y).powi(2) < 9.0 {
                        let sc = susc(&mut self.crs[j], &self.strains[inf as usize], t);
                        if self.rr() < 0.08 * sc {
                            self.infect_idx(j, inf);
                        }
                    }
                }
            }
            if p.ttl <= 0 {
                let i = p.y as usize * self.w + p.x as usize;
                if self.terr[i] == 0 || self.terr[i] == 3 {
                    self.fert[i] = (self.fert[i] + 0.04).min(1.0);
                    if let Some(lin) = p.seed {
                        // dung-dispersed seeds still compete for room (up to ~1.5x carrying capacity)
                        let lp = (self.nplants - self.nwater_plants) as f32;
                        let room = if self.crowd > 0.0 { 1.0 } else { (1.0 - lp / (self.land as f32 * 0.19).max(1.0)).max(0.0) };
                        if self.pl[i] == 0 && self.rr() < room {
                            self.sow_lineage(i, lin);
                        }
                        let j = i as i64 + self.rng.gen_range(-1..=1) + self.w as i64 * self.rng.gen_range(-1..=1);
                        if j >= 0 && (j as usize) < self.n {
                            let j = j as usize;
                            if self.terr[j] == 0 && self.pl[j] == 0 && self.rr() < 0.6 * room {
                                self.sow_lineage(j, lin);
                            }
                        }
                    }
                }
            }
        }
        poop.retain(|p| p.ttl > 0);
        self.poop = poop;
let __t2 = std::time::Instant::now();
        self.corpse_step();
        self.prof[2] += __t2.elapsed().as_secs_f64();
        for v in self.kgrid.iter_mut() {
            v.clear();
        }
        for (j, k) in self.cor.iter().enumerate() {
            let gi = (k.y as usize / CG).min(self.ghc - 1) * self.gwc + (k.x as usize / CG).min(self.gwc - 1);
            self.kgrid[gi].push(j as u32);
        }
        // perception in parallel: read-only snapshot, results applied in each creature's turn
let __t3 = std::time::Instant::now();
        let n0 = self.crs.len();
        let lookers: Vec<bool> = (0..n0)
            .map(|i| {
                let c = &self.crs[i];
                !c.dead && self.every(if c.tgt.is_some() { 10 } else { 3 }, c.id as i64)
            })
            .collect();

        self.prof[3] += __t3.elapsed().as_secs_f64();        let me: &Sim = self;
        let looks: Vec<Option<(Option<u32>, Option<Tgt>)>> = if n0 > 64 {
            (0..n0).into_par_iter().with_min_len(16).map(|i| if lookers[i] { Some(me.look(i)) } else { None }).collect()
        } else {
            (0..n0).map(|i| if lookers[i] { Some(me.look(i)) } else { None }).collect()
        };
let __t4 = std::time::Instant::now();
        for (i, lk) in looks.into_iter().enumerate() {
            if !self.crs[i].dead {
                self.live(i, lk);
            }
        }
        self.crs.retain(|c| !c.dead);
        self.prof[4] += __t4.elapsed().as_secs_f64();
        self.idmap = self.crs.iter().enumerate().map(|(i, c)| (c.id, i)).collect();
        let _ = dtf;
        if !self.primordial && self.crs.len() < 14 {
            let b = base_genome(&mut self.rng, Kind::Herb);
            let sp = self.newsp(None, b.clone(), "");
            for _ in 0..10 {
                let h1 = mutate(&mut self.rng, &b, 0.8).0;
                let h2 = mutate(&mut self.rng, &b, 0.8).0;
                self.spawn(h1, h2, sp, None, None, None, "founder", None, None);
            }
            let msg = format!("New founders appear: {}", self.species[sp as usize].name);
            self.logmsg(msg, [120, 220, 160]);
        }
        if !self.primordial && self.every(3000, 0) && t > 0 && !self.crs.iter().any(|c| c.g.v[DIET] > 0.65) && self.crs.len() > 60 {
            let b = base_genome(&mut self.rng, Kind::Carn);
            let sp = self.newsp(None, b.clone(), "");
            for _ in 0..10 {
                let h1 = mutate(&mut self.rng, &b, 0.8).0;
                let h2 = mutate(&mut self.rng, &b, 0.8).0;
                let pos = self.near_prey(Kind::Carn);
                self.spawn(h1, h2, sp, None, pos, None, "founder", None, None);
            }
            let msg = format!("Predators migrate in: {}", self.species[sp as usize].name);
            self.logmsg(msg, [255, 140, 120]);
        }
        if self.every(30, 0) {
            let mut msgs = Vec::new();
            for c in &self.crs {
                for (label, reached) in [
                    ("flight", can_fly(&c.g)),
                    ("swimming limbs", c.g.v[FINS] > 0.5),
                    ("complex animals", c.g.v[COMPLEXITY] > 0.65),
                ] {
                    if reached && !self.milestones.contains(label) && !msgs.iter().any(|(l, _, _)| *l == label) {
                        msgs.push((label, self.species[c.sp as usize].name.clone(), c.col));
                    }
                }
            }
            for (label, spn, col) in msgs {
                self.milestones.insert(label.into());
                self.logmsg(format!("EVOLUTION: {} in {}", label, spn), col);
            }
            let mut cnt: FxHashMap<u32, u32> = FxHashMap::default();
            for c in &self.crs {
                if let Some((s, _)) = c.sick {
                    *cnt.entry(s).or_default() += 1;
                }
            }
            self.nsick = cnt.values().sum();
            let mut died = Vec::new();
            for st in self.strains.iter_mut() {
                let n = cnt.get(&st.id).copied().unwrap_or(0);
                if st.act > 0 && n == 0 && st.peak >= 5 {
                    died.push((format!("{} died out ({} cases total)", st.name, st.cases), [140, 190, 120]));
                }
                st.act = n;
                st.peak = st.peak.max(n);
            }
            for (m, c) in died {
                self.logmsg(m, c);
            }
        }
        if self.every(15, 0) {
            let h = self.crs.iter().filter(|c| c.g.v[DIET] < 0.45).count() as u32;
            let k = self.crs.iter().filter(|c| c.g.v[DIET] > 0.65).count() as u32;
            self.hist.push_back([h, k, self.crs.len() as u32 - h - k, self.nplants as u32, self.nsick]);
            if self.hist.len() > 220 {
                self.hist.pop_front();
            }
        }
        if self.vegetation.needs_compaction() && self.every(DAY / 4, 0) {
            self.compact_plant_lineages();
        }
        if self.lin.len() > 400_000 && self.every(DAY, 0) {
            self.prune_lineage();
        }
    }

    fn compact_plant_lineages(&mut self) {
        let map = self.vegetation.compact(&self.pl);
        for c in self.crs.iter_mut() {
            c.gut.retain_mut(|(_, l)| match map.get(l) {
                Some(&m) => {
                    *l = m;
                    true
                }
                None => false,
            });
        }
        for p in self.poop.iter_mut() {
            p.seed = p.seed.and_then(|l| map.get(&l).copied());
        }
    }

    /// Keep ancestry only for chains that living creatures can still reach (bounded memory).
    fn prune_lineage(&mut self) {
        let mut keep: FxHashSet<u32> = FxHashSet::default();
        for c in &self.crs {
            let mut cur = c.id;
            for _ in 0..30 {
                if !keep.insert(cur) {
                    break;
                }
                match self.lin.get(&cur) {
                    Some(l) if l.par != 0 => cur = l.par,
                    _ => break,
                }
            }
        }
        self.lin.retain(|k, _| keep.contains(k));
    }

    /// Seed rain: a new sprout on empty cell `i` inherits (with mutation) the lineage of a nearby
    /// plant that suits the site. Only bare regions get the ancestral pioneer lineage.
    fn sprout(&mut self, i: usize) {
        let (w, h) = (self.w, self.h);
        let ti = self.temp_at(i);
        let mut seen_any = false;
        for _ in 0..12 {
            let x = (i % w) as i32 + self.rng.gen_range(-4..=4);
            let y = (i / w) as i32 + self.rng.gen_range(-4..=4);
            if x < 0 || y < 0 || x as usize >= w || y as usize >= h {
                continue;
            }
            let j = y as usize * w + x as usize;
            if self.pl[j] >= 2 {
                seen_any = true;
                if Vegetation::suitability_of(self.vegetation.lineage(j), self.moisture[i], ti, self.terr[i] == 1) > 0.15 {
                    let t = self.t;
                    let child = self.vegetation.seed(&mut self.rng, j, i, t);
                    self.setp(i, 1, Some(child));
                    return;
                }
            }
        }
        if !seen_any || self.rr() < 0.1 {
            self.setp(i, 1, None);
        }
    }

    /// A dispersed seed of `lin` lands on empty cell `i`: mutate like any seed, establish if suitable.
    fn sow_lineage(&mut self, i: usize, lin: u32) {
        if (lin as usize) >= self.vegetation.lineages.len() {
            return;
        }
        let prev = self.vegetation.ids[i];
        self.vegetation.ids[i] = lin;
        let t = self.t;
        let child = self.vegetation.seed(&mut self.rng, i, i, t);
        let ti = self.temp_at(i);
        if self.vegetation.suitability(i, self.moisture[i], ti, self.terr[i] == 1) > 0.15 {
            self.setp(i, 1, Some(child));
        } else {
            self.vegetation.ids[i] = prev;
        }
    }

    pub fn alive_species(&self) -> usize {
        self.species.iter().filter(|p| p.pop > 0).count()
    }

    pub fn sow(&mut self, wx: f32, wy: f32) {
        for _ in 0..25 {
            let x = wx as i64 + self.rng.gen_range(-6..=6);
            let y = wy as i64 + self.rng.gen_range(-6..=6);
            if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
                let i = y as usize * self.w + x as usize;
                if self.terr[i] == 0 {
                    self.setp(i, 3, None);
                }
            }
        }
    }

    pub fn place_founders(&mut self, wx: f32, wy: f32, carn: bool) {
        let b = base_genome(&mut self.rng, if carn { Kind::Carn } else { Kind::Herb });
        let sp = self.newsp(None, b.clone(), "founder");
        for _ in 0..6 {
            let cx = wx + self.rng.gen_range(-4.0..4.0);
            let cy = wy + self.rng.gen_range(-4.0..4.0);
            if self.ok(cx, cy, false) {
                let h1 = mutate(&mut self.rng, &b, 0.6).0;
                let h2 = mutate(&mut self.rng, &b, 0.6).0;
                self.spawn(h1, h2, sp, None, Some((cx, cy)), None, "founder", None, None);
            }
        }
        let msg = format!("Founders placed: {}", self.species[sp as usize].name);
        let col = self.species[sp as usize].col;
        self.logmsg(msg, col);
    }
}
