//! Plant lineages inherit and mutate during seed dispersal; the habitat selects them.
use crate::util::{gauss, Rng};
use rand::Rng as _;
use rustc_hash::FxHashSet;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
pub struct PlantLineage {
    pub id: u32,
    pub parent: Option<u32>,
    pub born: i64,
    pub generation: u32,
    pub height: f32,
    pub wood: f32,
    pub moisture: f32,
    pub temperature: f32,
    pub aquatic: f32,
    pub dispersal: f32,
    pub defense: f32,
    pub hue: f32,
    // heritable architecture
    pub crown: f32,
    pub droop: f32,
    pub density: f32,
    pub branching: f32,
    pub girth: f32,
    pub bloom: f32,
    pub bloom_hue: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    Algae,
    Kelp,
    Tree,
    Shrub,
    Biofilm,
    Moss,
    Grass,
}

impl Form {
    pub fn name(self) -> &'static str {
        match self {
            Form::Algae => "algae",
            Form::Kelp => "kelp",
            Form::Tree => "tree",
            Form::Shrub => "shrub",
            Form::Biofilm => "biofilm",
            Form::Moss => "moss",
            Form::Grass => "grass",
        }
    }
    pub fn tall(self) -> bool {
        matches!(self, Form::Tree | Form::Shrub | Form::Kelp)
    }
}

pub const TRAITS: [&str; 13] =
    ["height", "wood", "moisture", "temperature", "aquatic", "dispersal", "defense", "crown", "droop", "density", "branching", "girth", "bloom"];

impl PlantLineage {
    pub fn form(&self) -> Form {
        if self.aquatic > 0.65 {
            return if self.height < 0.5 { Form::Algae } else { Form::Kelp };
        }
        if self.wood > 0.55 && self.height > 0.6 {
            return Form::Tree;
        }
        if self.wood > 0.3 && self.height > 0.3 {
            return Form::Shrub;
        }
        if self.height < 0.15 && self.wood < 0.1 {
            Form::Biofilm
        } else if self.height < 0.15 {
            Form::Moss
        } else {
            Form::Grass
        }
    }
    pub fn trait_value(&self, i: usize) -> f32 {
        self.field(i)
    }
    fn field(&self, i: usize) -> f32 {
        match i {
            0 => self.height,
            1 => self.wood,
            2 => self.moisture,
            3 => self.temperature,
            4 => self.aquatic,
            5 => self.dispersal,
            6 => self.defense,
            7 => self.crown,
            8 => self.droop,
            9 => self.density,
            10 => self.branching,
            11 => self.girth,
            12 => self.bloom,
            13 => self.hue,
            _ => self.bloom_hue,
        }
    }
    fn field_mut(&mut self, i: usize) -> &mut f32 {
        match i {
            0 => &mut self.height,
            1 => &mut self.wood,
            2 => &mut self.moisture,
            3 => &mut self.temperature,
            4 => &mut self.aquatic,
            5 => &mut self.dispersal,
            6 => &mut self.defense,
            7 => &mut self.crown,
            8 => &mut self.droop,
            9 => &mut self.density,
            10 => &mut self.branching,
            11 => &mut self.girth,
            12 => &mut self.bloom,
            13 => &mut self.hue,
            _ => &mut self.bloom_hue,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn lin(id: u32, height: f32, wood: f32, moisture: f32, temperature: f32, aquatic: f32, dispersal: f32, defense: f32, hue: f32) -> PlantLineage {
    PlantLineage {
        id, parent: None, born: 0, generation: 0, height, wood, moisture, temperature, aquatic, dispersal, defense, hue,
        crown: 0.5, droop: 0.2, density: 0.6, branching: 0.5, girth: 0.3, bloom: 0.1, bloom_hue: 330.0,
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Vegetation {
    pub width: usize,
    pub height: usize,
    pub ids: Vec<u16>,
    pub lineages: Vec<PlantLineage>,
    pub counts: Vec<u32>,
    pub peaks: Vec<u32>,
    pub died: Vec<Option<i64>>,
    #[serde(skip)]
    pub trees: FxHashSet<u32>,
}

const SHAPE: [usize; 9] = [7, 8, 9, 10, 11, 12, 14, 13, 0];

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
            trees: FxHashSet::default(),
        }
    }

    #[inline]
    pub fn lineage(&self, i: usize) -> &PlantLineage {
        &self.lineages[self.ids[i] as usize]
    }

    /// Seed dispersal with inheritance and mutation. Returns child lineage id.
    pub fn seed(&mut self, r: &mut Rng, source: usize, target: usize, tick: i64) -> u16 {
        let parent = *self.lineage(source);
        let mut child_id = parent.id as u16;
        if r.gen::<f32>() < 0.16 && self.lineages.len() < 65535 {
            let mut child = parent;
            let mut changed = false;
            let woody = parent.wood > 0.3;
            for key in 0..15 {
                let p = if woody && SHAPE.contains(&key) { 0.45 } else { 0.35 };
                if r.gen::<f32>() < p {
                    let hue = key >= 13;
                    let sc = if hue { 14.0 } else { 0.13 };
                    let v = child.field(key) + gauss(r, 0.0, sc);
                    *child.field_mut(key) = if hue { v.rem_euclid(360.0) } else { v.clamp(0.0, 1.0) };
                    changed = true;
                }
            }
            if changed {
                child.id = self.lineages.len() as u32;
                child.parent = Some(parent.id);
                child.born = tick;
                child.generation = parent.generation + 1;
                child_id = child.id as u16;
                self.lineages.push(child);
                self.counts.push(0);
                self.peaks.push(0);
                self.died.push(None);
            }
        }
        self.ids[target] = child_id;
        child_id
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

    /// Height that the plant's wood can actually hold up.
    #[inline]
    pub fn stature(p: &PlantLineage) -> f32 {
        p.height.min(p.wood * 1.2 + 0.15)
    }

    /// Light competition: taller neighbours shade this plant (0 = full sun, up to 0.6).
    pub fn overshadow(&self, i: usize, pl: &[u8]) -> f32 {
        let (w, h) = (self.width as i32, self.height as i32);
        let me = Self::stature(self.lineage(i));
        let (x, y) = ((i % self.width) as i32, (i / self.width) as i32);
        let mut tallest = 0f32;
        for (dx, dy) in [(-1, -2), (1, -2), (-2, 0), (2, 0), (-1, 2), (1, 2)] {
            let (nx, ny) = (x + dx, y + dy);
            if nx >= 0 && nx < w && ny >= 0 && ny < h {
                let j = (ny * w + nx) as usize;
                if pl[j] >= 2 {
                    tallest = tallest.max(Self::stature(self.lineage(j)));
                }
            }
        }
        ((tallest - me) * 1.5).clamp(0.0, 0.6)
    }

    pub fn suitability_of(p: &PlantLineage, wet: u8, temperature: f32, water: bool) -> f32 {
        if water && p.aquatic < 0.5 {
            return 0.0;
        }
        if !water && p.aquatic > 0.75 {
            return 0.0;
        }
        let moisture = if water { 1.0 } else { wet as f32 / 255.0 };
        (1.0 - (moisture - p.moisture).abs() * 1.5).max(0.0) * (1.0 - (temperature - p.temperature).abs() * 2.0).max(0.0)
    }

    pub fn suitability(&self, i: usize, wet: u8, temperature: f32, water: bool) -> f32 {
        Self::suitability_of(self.lineage(i), wet, temperature, water)
    }

    pub fn track(&mut self, i: usize, growth: u8) {
        if growth >= 2 && self.lineage(i).form().tall() {
            self.trees.insert(i as u32);
        } else {
            self.trees.remove(&(i as u32));
        }
    }

    pub fn rebuild_trees(&mut self, pl: &[u8]) {
        self.trees.clear();
        for (i, &st) in pl.iter().enumerate() {
            if st >= 2 && self.lineage(i).form().tall() {
                self.trees.insert(i as u32);
            }
        }
    }
}
