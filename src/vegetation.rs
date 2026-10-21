//! Plant lineages inherit and mutate during seed dispersal; the habitat and herbivores select them.
use crate::util::{gauss, Rng};
use rand::Rng as _;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};

/// Number of heritable plant genes (all in 0..1 except the two hues).
pub const NPG: usize = 25;
// gene indices
pub const HEIGHT: usize = 0;
pub const WOOD: usize = 1;
pub const MOISTURE: usize = 2;
pub const TEMPERATURE: usize = 3;
pub const AQUATIC: usize = 4;
pub const DISPERSAL: usize = 5;
pub const DEFENSE: usize = 6;
pub const CROWN: usize = 7;
pub const DROOP: usize = 8;
pub const DENSITY: usize = 9;
pub const BRANCHING: usize = 10;
pub const GIRTH: usize = 11;
pub const BLOOM: usize = 12;
pub const HUE: usize = 13;
pub const BLOOM_HUE: usize = 14;
pub const TOXIN: usize = 15;
pub const THORNS: usize = 16;
pub const SUCCULENCE: usize = 17;
pub const DECIDUOUS: usize = 18;
pub const FIRE_RES: usize = 19;
pub const CLIMBING: usize = 20;
pub const CLONAL: usize = 21;
pub const ROOTS: usize = 22;
pub const ANNUAL: usize = 23;
pub const CARNIVORY: usize = 24;

/// Genes shown as bars in the plant phylogeny (name, index, short meaning).
pub const TRAITS: [(&str, usize); 23] = [
    ("height", HEIGHT), ("wood", WOOD), ("moisture", MOISTURE), ("temperature", TEMPERATURE), ("aquatic", AQUATIC),
    ("dispersal", DISPERSAL), ("defense", DEFENSE), ("toxin", TOXIN), ("thorns", THORNS), ("succulence", SUCCULENCE),
    ("deciduous", DECIDUOUS), ("fire resist", FIRE_RES), ("climbing", CLIMBING), ("runners", CLONAL), ("roots", ROOTS),
    ("annual", ANNUAL), ("carnivory", CARNIVORY), ("crown", CROWN), ("droop", DROOP), ("density", DENSITY),
    ("branching", BRANCHING), ("girth", GIRTH), ("bloom", BLOOM),
];

/// Architecture genes mutate more often in woody plants (visible forest diversity).
const SHAPE: [usize; 9] = [CROWN, DROOP, DENSITY, BRANCHING, GIRTH, BLOOM, BLOOM_HUE, HUE, HEIGHT];

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
pub struct PlantLineage {
    pub id: u32,
    pub parent: Option<u32>,
    pub born: i64,
    pub generation: u32,
    pub g: [f32; NPG],
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Form {
    Algae,
    Kelp,
    Reed,
    Cactus,
    Vine,
    Flytrap,
    Conifer,
    Palm,
    Tree,
    Shrub,
    Flower,
    Biofilm,
    Moss,
    Grass,
}

pub const FORMS: [Form; 14] = [
    Form::Tree, Form::Conifer, Form::Palm, Form::Shrub, Form::Cactus, Form::Vine, Form::Flytrap, Form::Flower,
    Form::Reed, Form::Kelp, Form::Algae, Form::Grass, Form::Moss, Form::Biofilm,
];

impl Form {
    pub fn name(self) -> &'static str {
        match self {
            Form::Algae => "algae",
            Form::Kelp => "kelp",
            Form::Reed => "reed",
            Form::Cactus => "cactus",
            Form::Vine => "vine",
            Form::Flytrap => "flytrap",
            Form::Conifer => "conifer",
            Form::Palm => "palm",
            Form::Tree => "tree",
            Form::Shrub => "shrub",
            Form::Flower => "flower",
            Form::Biofilm => "biofilm",
            Form::Moss => "moss",
            Form::Grass => "grass",
        }
    }
    /// Forms drawn as individual sprites (the rest are ground-cover pixels).
    pub fn sprite(self) -> bool {
        !matches!(self, Form::Algae | Form::Biofilm | Form::Moss | Form::Grass)
    }
    pub fn tree(self) -> bool {
        matches!(self, Form::Tree | Form::Conifer | Form::Palm)
    }
}

impl PlantLineage {
    #[inline]
    pub fn v(&self, k: usize) -> f32 {
        self.g[k]
    }
    pub fn form(&self) -> Form {
        let g = &self.g;
        if g[AQUATIC] > 0.65 {
            return if g[HEIGHT] < 0.5 { Form::Algae } else { Form::Kelp };
        }
        if g[AQUATIC] > 0.45 && g[HEIGHT] > 0.35 {
            return Form::Reed;
        }
        if g[SUCCULENCE] > 0.6 && g[WOOD] < 0.55 && g[HEIGHT] > 0.2 {
            return Form::Cactus;
        }
        if g[CLIMBING] > 0.6 && g[WOOD] < 0.5 && g[HEIGHT] > 0.25 {
            return Form::Vine;
        }
        if g[CARNIVORY] > 0.6 && g[WOOD] < 0.3 {
            return Form::Flytrap;
        }
        if g[WOOD] > 0.55 && g[HEIGHT] > 0.6 {
            if g[CROWN] < 0.35 && g[DECIDUOUS] < 0.4 {
                return Form::Conifer;
            }
            if g[CROWN] > 0.75 && g[BRANCHING] < 0.3 {
                return Form::Palm;
            }
            return Form::Tree;
        }
        if g[WOOD] > 0.3 && g[HEIGHT] > 0.3 {
            return Form::Shrub;
        }
        if g[BLOOM] > 0.6 && g[HEIGHT] >= 0.15 {
            return Form::Flower;
        }
        if g[HEIGHT] < 0.15 && g[WOOD] < 0.1 {
            Form::Biofilm
        } else if g[HEIGHT] < 0.15 {
            Form::Moss
        } else {
            Form::Grass
        }
    }
    /// Height the plant's wood can hold up (climbers borrow support from neighbours).
    #[inline]
    pub fn stature(&self) -> f32 {
        self.g[HEIGHT].min(self.g[WOOD] * 1.2 + 0.15)
    }
    /// Relative growth cost of all the investments (wood, height, defences, adaptations).
    #[inline]
    pub fn upkeep(&self) -> f32 {
        let g = &self.g;
        1.0 + g[WOOD] + 0.3 * g[HEIGHT] + 0.3 * g[BLOOM] + 0.2 * g[GIRTH] + 0.25 * g[TOXIN] + 0.2 * g[THORNS]
            + 0.2 * g[SUCCULENCE] + 0.15 * g[FIRE_RES] + 0.15 * g[ROOTS] + 0.2 * g[CARNIVORY]
    }
}

fn lin(id: u32, height: f32, wood: f32, moisture: f32, temperature: f32, aquatic: f32, dispersal: f32, defense: f32, hue: f32) -> PlantLineage {
    let mut g = [0f32; NPG];
    g[HEIGHT] = height;
    g[WOOD] = wood;
    g[MOISTURE] = moisture;
    g[TEMPERATURE] = temperature;
    g[AQUATIC] = aquatic;
    g[DISPERSAL] = dispersal;
    g[DEFENSE] = defense;
    g[HUE] = hue;
    g[CROWN] = 0.5;
    g[DROOP] = 0.2;
    g[DENSITY] = 0.6;
    g[BRANCHING] = 0.5;
    g[GIRTH] = 0.3;
    g[BLOOM] = 0.1;
    g[BLOOM_HUE] = 330.0;
    g[DECIDUOUS] = 0.2;
    PlantLineage { id, parent: None, born: 0, generation: 0, g }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Vegetation {
    pub width: usize,
    pub height: usize,
    pub ids: Vec<u32>,
    pub lineages: Vec<PlantLineage>,
    pub counts: Vec<u32>,
    pub peaks: Vec<u32>,
    pub died: Vec<Option<i64>>,
    /// Lineage ids are renumbered when the record is compacted; renderers key caches on this.
    pub epoch: u32,
    /// Pioneer lineages adapted to climate bands (temperature x moisture), used for spontaneous sprouts.
    pub pioneers: Vec<u32>,
    /// Total lineages ever created (compaction removes forgotten extinct twigs from `lineages`).
    pub total_created: u64,
    #[serde(skip)]
    pub trees: FxHashSet<u32>,
}

/// Lineage records are compacted beyond this size (keeps memory bounded on very long runs).
const MAX_LINEAGES: usize = 120_000;

impl Vegetation {
    pub fn new(width: usize, height: usize, primordial: bool) -> Self {
        Vegetation {
            width,
            height,
            ids: vec![0; width * height],
            lineages: vec![
                lin(0, 0.08, 0.0, 0.9, 0.5, 1.0, 0.3, 0.0, 145.0),
                lin(1, if primordial { 0.1 } else { 0.22 }, 0.02, 0.55, 0.55, 0.0, 0.35, 0.03, 105.0),
            ],
            counts: vec![0, 0],
            peaks: vec![0, 0],
            died: vec![None, None],
            epoch: 0,
            pioneers: vec![],
            total_created: 2,
            trees: FxHashSet::default(),
        }
        .with_pioneers(primordial)
    }

    const PT: [f32; 6] = [0.12, 0.27, 0.42, 0.57, 0.72, 0.88];
    const PM: [f32; 3] = [0.22, 0.52, 0.82];

    /// The world starts with a simple flora already spread across climates: one pioneer per band,
    /// descended from the ancestral grass (cold ones sleep through winter, dry ones store water).
    fn with_pioneers(mut self, primordial: bool) -> Self {
        for &t in &Self::PT {
            for &m in &Self::PM {
                let mut p = self.lineages[1];
                p.id = self.lineages.len() as u32;
                p.parent = Some(1);
                p.g[TEMPERATURE] = t;
                p.g[MOISTURE] = m;
                p.g[DECIDUOUS] = if t < 0.35 { 0.55 } else { 0.2 };
                p.g[ROOTS] = if m < 0.3 { 0.35 } else { 0.0 };
                p.g[SUCCULENCE] = if m < 0.3 && t > 0.5 { 0.35 } else { 0.0 };
                p.g[HEIGHT] = if primordial { 0.1 } else { 0.18 + 0.12 * m };
                p.g[HUE] = 70.0 + 60.0 * m + 25.0 * (1.0 - t);
                self.pioneers.push(p.id);
                self.lineages.push(p);
                self.counts.push(0);
                self.peaks.push(0);
                self.died.push(None);
            }
        }
        self.total_created = self.lineages.len() as u64;
        self
    }

    /// Pioneer lineage best suited to a site (falls back to the ancestral grass).
    pub fn pioneer(&self, temperature: f32, moisture: f32) -> u32 {
        if self.pioneers.len() != Self::PT.len() * Self::PM.len() {
            return 1;
        }
        let ti = Self::PT.iter().enumerate().min_by(|a, b| (a.1 - temperature).abs().total_cmp(&(b.1 - temperature).abs())).unwrap().0;
        let mi = Self::PM.iter().enumerate().min_by(|a, b| (a.1 - moisture).abs().total_cmp(&(b.1 - moisture).abs())).unwrap().0;
        self.pioneers[ti * Self::PM.len() + mi]
    }

    #[inline]
    pub fn lineage(&self, i: usize) -> &PlantLineage {
        &self.lineages[self.ids[i] as usize]
    }

    /// Seed dispersal with inheritance and mutation. Returns child lineage id.
    pub fn seed(&mut self, r: &mut Rng, source: usize, target: usize, tick: i64) -> u32 {
        let parent = *self.lineage(source);
        let id = self.mutate_from(r, &parent, tick);
        self.ids[target] = id;
        id
    }

    /// With some probability create a mutant child lineage of `parent`; returns the lineage to plant.
    pub fn mutate_from(&mut self, r: &mut Rng, parent: &PlantLineage, tick: i64) -> u32 {
        if r.gen::<f32>() >= 0.6 {
            return parent.id;
        }
        let mut child = *parent;
        let woody = parent.g[WOOD] > 0.3;
        let mut changed = false;
        // point mutations: usually one or two genes, so useful changes can accumulate
        while !changed || r.gen::<f32>() < 0.35 {
            let key = if woody && r.gen::<f32>() < 0.3 { SHAPE[r.gen_range(0..SHAPE.len())] } else { r.gen_range(0..NPG) };
            let hue = key == HUE || key == BLOOM_HUE;
            let sigma = if hue { 14.0 } else if key == MOISTURE || key == TEMPERATURE { 0.07 } else { 0.15 };
            let v = child.g[key] + gauss(r, 0.0, sigma);
            child.g[key] = if hue { v.rem_euclid(360.0) } else { v.clamp(0.0, 1.0) };
            changed = true;
        }
        if !changed {
            return parent.id;
        }
        child.id = self.lineages.len() as u32;
        child.parent = Some(parent.id);
        child.born = tick;
        child.generation = parent.generation + 1;
        self.lineages.push(child);
        self.counts.push(0);
        self.peaks.push(0);
        self.died.push(None);
        self.total_created += 1;
        child.id
    }

    /// Janzen-Connell effect: dense stands of one lineage suffer specialist pests.
    pub fn crowding(&self, i: usize, pl: &[u8]) -> f32 {
        let (w, h) = (self.width as i32, self.height as i32);
        let own = self.ids[i];
        let (x, y) = ((i % self.width) as i32, (i / self.width) as i32);
        let mut same = 0;
        for (dx, dy) in [(-2, 0), (2, 0), (0, -2), (0, 2), (-3, -3), (3, 3), (-3, 3), (3, -3)] {
            let (nx, ny) = (x + dx, y + dy);
            if nx >= 0 && nx < w && ny >= 0 && ny < h {
                let j = (ny * w + nx) as usize;
                if pl[j] != 0 && self.ids[j] == own {
                    same += 1;
                }
            }
        }
        same as f32 / 8.0
    }

    /// Light competition. Returns (shade penalty 0..0.6, stature of the tallest neighbour).
    /// Climbers are not shaded by what they climb on.
    pub fn overshadow(&self, i: usize, pl: &[u8], winter: bool) -> (f32, f32) {
        let (w, h) = (self.width as i32, self.height as i32);
        let p = self.lineage(i);
        let me = p.stature();
        let (x, y) = ((i % self.width) as i32, (i / self.width) as i32);
        let mut tallest = 0f32;
        for (dx, dy) in [(-1, -2), (1, -2), (-2, 0), (2, 0), (-1, 2), (1, 2)] {
            let (nx, ny) = (x + dx, y + dy);
            if nx >= 0 && nx < w && ny >= 0 && ny < h {
                let j = (ny * w + nx) as usize;
                if pl[j] >= 2 {
                    let n = self.lineage(j);
                    // bare deciduous canopies let winter light through
                    let leafy = if winter { 1.0 - 0.8 * n.g[DECIDUOUS] } else { 1.0 };
                    tallest = tallest.max(n.stature() * leafy);
                }
            }
        }
        let shade = ((tallest - me) * 1.5).clamp(0.0, 0.6) * (1.0 - p.g[CLIMBING]);
        (shade, tallest)
    }

    /// How well a lineage fits a site. Roots reach deeper water; succulents tolerate drought;
    /// deciduous plants tolerate cold (they sleep through it).
    pub fn suitability_of(p: &PlantLineage, wet: u8, temperature: f32, water: bool) -> f32 {
        let g = &p.g;
        if water && g[AQUATIC] < 0.5 {
            return 0.0;
        }
        if !water && g[AQUATIC] > 0.75 {
            return 0.0;
        }
        let moisture = if water { 1.0 } else { (wet as f32 / 255.0 + 0.25 * g[ROOTS]).min(1.0) };
        let dm = moisture - g[MOISTURE];
        let mtol = if dm < 0.0 { 1.5 * (1.0 - 0.65 * g[SUCCULENCE]) } else { 1.5 };
        let dtemp = temperature - g[TEMPERATURE];
        let ttol = if dtemp < 0.0 { 2.0 * (1.0 - 0.5 * g[DECIDUOUS]) } else { 2.0 * (1.0 - 0.3 * g[SUCCULENCE]) };
        (1.0 - dm.abs() * mtol).max(0.0) * (1.0 - dtemp.abs() * ttol).max(0.0)
    }

    pub fn suitability(&self, i: usize, wet: u8, temperature: f32, water: bool) -> f32 {
        Self::suitability_of(self.lineage(i), wet, temperature, water)
    }

    pub fn track(&mut self, i: usize, growth: u8) {
        if growth >= 2 && self.lineage(i).form().sprite() {
            self.trees.insert(i as u32);
        } else {
            self.trees.remove(&(i as u32));
        }
    }

    pub fn rebuild_trees(&mut self, pl: &[u8]) {
        self.trees.clear();
        for (i, &st) in pl.iter().enumerate() {
            if st >= 2 && self.lineage(i).form().sprite() {
                self.trees.insert(i as u32);
            }
        }
    }

    pub fn needs_compaction(&self) -> bool {
        self.lineages.len() > MAX_LINEAGES
    }

    /// Forget extinct twigs that left no living descendants and never mattered (small peak),
    /// renumbering the rest. Returns the old->new id map (for seeds carried in guts and dung).
    pub fn compact(&mut self, pl: &[u8]) -> FxHashMap<u32, u32> {
        let n = self.lineages.len();
        let mut keep = vec![false; n];
        keep[0] = true;
        keep[1] = true;
        for &p in &self.pioneers {
            keep[p as usize] = true;
        }
        // living lineages and their whole ancestry
        for i in 0..n {
            if self.counts[i] > 0 {
                let mut cur = Some(i as u32);
                while let Some(c) = cur {
                    if keep[c as usize] && c as usize != i {
                        break;
                    }
                    keep[c as usize] = true;
                    cur = self.lineages[c as usize].parent;
                }
            }
        }
        // notable extinct branches survive in the record, up to a budget
        let mut notable: Vec<usize> = (0..n).filter(|&i| !keep[i] && self.peaks[i] >= 25).collect();
        notable.sort_by_key(|&i| std::cmp::Reverse(self.peaks[i]));
        notable.truncate(MAX_LINEAGES / 6);
        for i in notable {
            let mut cur = Some(i as u32);
            while let Some(c) = cur {
                if keep[c as usize] {
                    break;
                }
                keep[c as usize] = true;
                cur = self.lineages[c as usize].parent;
            }
        }
        let mut map: FxHashMap<u32, u32> = FxHashMap::default();
        let mut lineages = Vec::new();
        let (mut counts, mut peaks, mut died) = (Vec::new(), Vec::new(), Vec::new());
        for i in 0..n {
            if keep[i] {
                map.insert(i as u32, lineages.len() as u32);
                lineages.push(self.lineages[i]);
                counts.push(self.counts[i]);
                peaks.push(self.peaks[i]);
                died.push(self.died[i]);
            }
        }
        for l in lineages.iter_mut() {
            l.id = map[&l.id];
            // a forgotten parent is replaced by its nearest remembered ancestor
            let mut p = l.parent;
            while let Some(pp) = p {
                if let Some(&np) = map.get(&pp) {
                    p = Some(np);
                    break;
                }
                p = self.lineages[pp as usize].parent;
            }
            l.parent = p;
        }
        for (i, id) in self.ids.iter_mut().enumerate() {
            *id = match map.get(id) {
                Some(&m) => m,
                None => {
                    debug_assert!(pl[i] == 0);
                    let _ = i;
                    1
                }
            };
        }
        for p in self.pioneers.iter_mut() {
            *p = map[p];
        }
        self.lineages = lineages;
        self.counts = counts;
        self.peaks = peaks;
        self.died = died;
        self.epoch += 1;
        map
    }
}
