//! Diploid genetics: genes, mutation, meiosis, phenotype, species distance, body-plan checks.
use crate::util::{gauss, Rng};
use rand::Rng as _;
use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;

pub const NG: usize = 38;
// gene indices
pub const SPD: usize = 0;
pub const SIZE: usize = 1;
pub const VIS: usize = 2;
pub const DIET: usize = 3;
pub const METAB: usize = 4;
pub const THRESH: usize = 5;
pub const LIFE: usize = 6;
pub const AGGR: usize = 7;
pub const MUT: usize = 8;
pub const MAT: usize = 9;
pub const LITTER: usize = 10;
pub const TOL: usize = 11;
pub const CARE: usize = 12;
pub const CHOOSY: usize = 13;
pub const BOLD: usize = 14;
pub const SOCIAL: usize = 15;
pub const CURIO: usize = 16;
pub const IMMUNE: usize = 17;
pub const BODY: usize = 18;
pub const EARS: usize = 19;
pub const HORN: usize = 20;
pub const TAIL: usize = 21;
pub const LEGS: usize = 22;
pub const EYES: usize = 23;
pub const SCAVENGE: usize = 24;
pub const FUR: usize = 25;
pub const ARMOR: usize = 26;
pub const COMPLEXITY: usize = 27;
pub const WINGS: usize = 28;
pub const GILLS: usize = 29;
pub const FINS: usize = 30;
pub const LUNGS: usize = 31;
pub const BURIAL: usize = 32;
/// neck length: long necks reach tree crowns and see farther
pub const NECK: usize = 33;
/// 0 = four-legged, 1 = upright two-legged runner
pub const STANCE: usize = 34;
/// dorsal plates / sail: shed heat, deter bites, cost speed
pub const PLATES: usize = 35;
/// body lightness (dark bodies warm up in the sun, light ones stay cool)
pub const LUM: usize = 36;
/// colour saturation (vivid = warning colours, dull = camouflage)
pub const SAT: usize = 37;

pub const RANGE: [(f32, f32); NG] = [
    (0.35, 1.3), (0.1, 26.0), (10.0, 50.0), (0.0, 1.0), (0.6, 1.5), (0.55, 0.9), (2500.0, 7000.0), (0.3, 1.0),
    (0.03, 0.22), (300.0, 1100.0), (1.0, 3.0), (0.1, 0.9), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0),
    (0.0, 1.0), (0.0, 1.0), (0.5, 0.95), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0),
    (0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0),
    (0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0),
];
pub const NAMES: [&str; NG] = [
    "spd", "size", "vis", "diet", "metab", "thresh", "life", "aggr", "mut", "mat", "litter", "tol", "care", "choosy",
    "bold", "social", "curio", "immune", "body", "ears", "horn", "tail", "legs", "eyes", "scavenge", "fur", "armor",
    "complexity", "wings", "gills", "fins", "lungs", "burial", "neck", "stance", "plates", "lum", "sat",
];
pub const DK: [usize; 15] = [SPD, SIZE, VIS, DIET, METAB, THRESH, LIFE, AGGR, TOL, COMPLEXITY, WINGS, GILLS, FINS, LUNGS, BURIAL];
pub const VK: [usize; 13] = [BODY, EARS, HORN, TAIL, LEGS, EYES, FUR, ARMOR, NECK, STANCE, PLATES, LUM, SAT];
/// genes summarised by the headless benchmark
pub const EXTRA_REPORT: [(&str, usize); 7] = [("neck", NECK), ("stance", STANCE), ("plates", PLATES), ("lum", LUM), ("sat", SAT), ("armor", ARMOR), ("horn", HORN)];
const LG: [&[usize]; 5] = [
    &[SPD, SIZE, METAB, LIFE, MAT],
    &[VIS, DIET, AGGR, THRESH, TOL, BOLD, SOCIAL, CURIO],
    &[LITTER, CARE, CHOOSY, MUT, IMMUNE, SCAVENGE],
    &VK,
    &[COMPLEXITY, WINGS, GILLS, FINS, LUNGS, BURIAL],
];

pub fn innovation_generation(k: usize) -> Option<u32> {
    match k {
        WINGS => Some(12),
        FINS => Some(3),
        GILLS => Some(4),
        LUNGS => Some(6),
        BURIAL => Some(8),
        PLATES => Some(6),
        STANCE => Some(8),
        NECK => Some(10),
        _ => None,
    }
}

fn up(k: usize) -> &'static str {
    match k {
        SPD => "faster", SIZE => "bigger", VIS => "keener eyes", DIET => "meat-eater", METAB => "hot-blooded",
        THRESH => "late breeder", LIFE => "long-lived", AGGR => "bolder", TOL => "warm-adapted",
        COMPLEXITY => "complex body", WINGS => "wing growth", GILLS => "gill breathing", FINS => "swimming limbs",
        LUNGS => "air breathing", BURIAL => "burial behavior", BODY => "rounder", EARS => "bigger ears", HORN => "horns",
        TAIL => "longer tail", LEGS => "longer legs", EYES => "bigger eyes", FUR => "thicker fur", ARMOR => "armored",
        SCAVENGE => "corpse-eater", NECK => "longer neck", STANCE => "upright stance", PLATES => "back plates",
        LUM => "paler", SAT => "vivid colours", _ => "drift",
    }
}
fn down(k: usize) -> &'static str {
    match k {
        SPD => "slower", SIZE => "smaller", VIS => "dim eyes", DIET => "leaf-eater", METAB => "cold-blooded",
        THRESH => "early breeder", LIFE => "short-lived", AGGR => "meeker", TOL => "cold-adapted",
        COMPLEXITY => "simpler body", WINGS => "reduced wings", GILLS => "reduced gills", FINS => "reduced fins",
        LUNGS => "reduced lungs", BURIAL => "reduced burial behavior", BODY => "sleeker", EARS => "smaller ears",
        HORN => "hornless", TAIL => "shorter tail", LEGS => "stubby legs", EYES => "smaller eyes", FUR => "thinner fur",
        ARMOR => "softer hide", SCAVENGE => "less scavenging", NECK => "shorter neck", STANCE => "four-legged",
        PLATES => "smooth back", LUM => "darker", SAT => "dull colours", _ => "drift",
    }
}

// quirks (bitmask)
pub const NOCTURNAL: u16 = 1 << 0;
pub const HERDER: u16 = 1 << 1;
pub const SWIMMER: u16 = 1 << 2;
pub const SPIKY: u16 = 1 << 3;
pub const SKITTISH: u16 = 1 << 4;
pub const SCAVENGER: u16 = 1 << 5;
pub const GLUTTON: u16 = 1 << 6;
pub const CAMOUFLAGE: u16 = 1 << 7;
pub const VENOMOUS: u16 = 1 << 8;
pub const THICK_HIDE: u16 = 1 << 9;
pub const HIBERNATOR: u16 = 1 << 10;
pub const PROLIFIC: u16 = 1 << 11;
pub const NQ: usize = 12;
pub const QNAMES: [&str; NQ] = [
    "Nocturnal", "Herder", "Swimmer", "Spiky", "Skittish", "Scavenger", "Glutton", "Camouflage", "Venomous",
    "Thick-hide", "Hibernator", "Prolific",
];
pub const QCOL: [[u8; 3]; NQ] = [
    [255, 225, 90], [120, 170, 255], [60, 220, 230], [240, 240, 240], [255, 130, 190], [190, 110, 230],
    [255, 160, 60], [130, 190, 120], [110, 230, 90], [170, 140, 110], [170, 200, 255], [255, 170, 200],
];
const QDOM: u16 = !(NOCTURNAL | HIBERNATOR | PROLIFIC);

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Genome {
    #[serde(with = "BigArray")]
    pub v: [f32; NG],
    pub hue: f32,
    pub hue2: f32,
    pub pat: u8,
    pub q: u16,
    pub epoch: u32,
}

impl Genome {
    #[inline]
    pub fn has(&self, q: u16) -> bool {
        self.q & q != 0
    }
    #[inline]
    pub fn get(&self, k: usize) -> f32 {
        self.v[k]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Microbe,
    Herb,
    Omni,
    Carn,
}

fn clampg(k: usize, v: f32) -> f32 {
    let (lo, hi) = RANGE[k];
    v.max(lo).min(hi)
}

fn uni(r: &mut Rng, a: f32, b: f32) -> f32 {
    a + (b - a) * r.gen::<f32>()
}

pub fn base_genome(r: &mut Rng, kind: Kind) -> Genome {
    let mut v = [0f32; NG];
    for k in 0..NG {
        v[k] = uni(r, RANGE[k].0, RANGE[k].1);
    }
    let hue = uni(r, 0.0, 360.0);
    v[LITTER] = uni(r, 1.2, 2.6);
    v[MUT] = uni(r, 0.07, 0.14);
    v[MAT] = uni(r, 400.0, 800.0);
    v[LIFE] = uni(r, 3500.0, 6000.0);
    v[TOL] = uni(r, 0.3, 0.8);
    v[CARE] = uni(r, 0.1, 0.7);
    v[CHOOSY] = uni(r, 0.0, 0.6);
    v[BOLD] = uni(r, 0.25, 0.75);
    v[SOCIAL] = uni(r, 0.2, 0.8);
    v[CURIO] = uni(r, 0.2, 0.8);
    v[IMMUNE] = uni(r, 0.15, 0.5);
    v[COMPLEXITY] = 0.8;
    v[WINGS] = 0.0;
    v[GILLS] = 0.0;
    v[FINS] = 0.0;
    v[LUNGS] = 1.0;
    v[BURIAL] = 0.0;
    v[SCAVENGE] = uni(r, 0.0, 0.12);
    v[NECK] = uni(r, 0.0, 0.15);
    v[STANCE] = uni(r, 0.0, 0.2);
    v[PLATES] = uni(r, 0.0, 0.1);
    v[LUM] = uni(r, 0.35, 0.75);
    v[SAT] = uni(r, 0.35, 0.75);
    let pat = r.gen_range(0..6u8);
    let hue2 = (hue + uni(r, -90.0, 90.0)).rem_euclid(360.0);
    match kind {
        Kind::Microbe => {
            v[COMPLEXITY] = 0.08;
            v[GILLS] = 0.9;
            v[LUNGS] = 0.0;
            v[SIZE] = 0.16;
            v[DIET] = 0.03;
            v[MAT] = 300.0;
            v[LIFE] = 3500.0;
            v[CARE] = 0.0;
            v[ARMOR] = 0.0;
            v[FUR] = 0.0;
            v[SPD] = 0.35;
        }
        Kind::Herb => {
            v[DIET] = uni(r, 0.0, 0.15);
            v[SIZE] = uni(r, 3.0, 5.5);
        }
        Kind::Omni => {
            v[DIET] = uni(r, 0.45, 0.6);
            v[SIZE] = uni(r, 4.0, 6.0);
        }
        Kind::Carn => {
            v[DIET] = uni(r, 0.85, 1.0);
            v[SIZE] = uni(r, 4.8, 7.0);
            v[SPD] = uni(r, 0.8, 1.2);
            v[VIS] = uni(r, 28.0, 50.0);
            v[AGGR] = uni(r, 0.6, 1.0);
        }
    }
    Genome { v, hue, hue2, pat, q: 0, epoch: 0 }
}

/// Mutate ONE haplotype (a gamete). Point mutations, rare macro-mutations, quirk gain/loss, color drift.
pub fn mutate(r: &mut Rng, g: &Genome, boost: f32) -> (Genome, String) {
    let mut c = g.clone();
    let m = g.v[MUT] * boost;
    let (mut best, mut bk, mut vbest, mut vk) = (0f32, usize::MAX, 0f32, usize::MAX);
    let simple = g.v[COMPLEXITY] < 0.42;
    for k in 0..NG {
        if let Some(ig) = innovation_generation(k) {
            if g.epoch < ig {
                continue;
            }
        }
        if r.gen::<f32>() < 0.3 {
            let old = c.v[k];
            let (lo, hi) = RANGE[k];
            let span = if k == SIZE && simple { (old * 0.8).max(0.1) } else { hi - lo };
            c.v[k] = clampg(k, old + gauss(r, 0.0, m * if k == MUT { 0.5 } else if k == DIET { 0.4 } else { 1.0 }) * span);
            let rel = (c.v[k] - old) / (hi - lo);
            if DK.contains(&k) && rel.abs() > best.abs() {
                best = rel;
                bk = k;
            } else if VK.contains(&k) && rel.abs() > vbest.abs() {
                vbest = rel;
                vk = k;
            }
        }
    }
    let mut note: String = if bk != usize::MAX && best.abs() > 0.07 {
        (if best > 0.0 { up(bk) } else { down(bk) }).to_string()
    } else {
        String::new()
    };
    if note.is_empty() && vk != usize::MAX && vbest.abs() > 0.12 {
        note = (if vbest > 0.0 { up(vk) } else { down(vk) }).to_string();
    }
    if r.gen::<f32>() < 0.008 * boost {
        let eligible: Vec<usize> = DK
            .iter()
            .copied()
            .filter(|&k| innovation_generation(k).map_or(true, |ig| g.epoch >= ig) && !(simple && k == SIZE))
            .collect();
        let k = eligible[r.gen_range(0..eligible.len())];
        let (lo, hi) = RANGE[k];
        let sign = if r.gen::<bool>() { 1.0 } else { -1.0 };
        let d = sign * uni(r, 0.3, 0.5) * (hi - lo);
        c.v[k] = clampg(k, c.v[k] + d);
        note = format!("!{}", if d > 0.0 { up(k) } else { down(k) });
    }
    c.v[SIZE] = c.v[SIZE].min(size_cap(&c));
    c.hue = (g.hue + gauss(r, 0.0, 6.0)).rem_euclid(360.0);
    c.hue2 = (g.hue2 + gauss(r, 0.0, 6.0)).rem_euclid(360.0);
    if r.gen::<f32>() < 0.05 {
        c.pat = r.gen_range(0..6);
    }
    if r.gen::<f32>() < 0.02 * m / 0.1 {
        let nq = r.gen_range(0..NQ);
        if c.q & (1 << nq) == 0 {
            c.q |= 1 << nq;
            note = format!("+{}", QNAMES[nq]);
        }
    }
    for qi in 0..NQ {
        if c.q & (1 << qi) != 0 && r.gen::<f32>() < 0.006 {
            c.q &= !(1 << qi);
            note = format!("-{}", QNAMES[qi]);
        }
    }
    (c, note)
}

/// Meiosis: recombine two haplotypes (linkage groups travel together), then mutate.
pub fn gamete(r: &mut Rng, h1: &Genome, h2: &Genome) -> (Genome, String) {
    let mut g = h1.clone();
    for grp in LG.iter() {
        let (a, b) = if r.gen::<f32>() < 0.5 { (h1, h2) } else { (h2, h1) };
        for &k in grp.iter() {
            g.v[k] = if r.gen::<f32>() > 0.06 { a.v[k] } else { b.v[k] };
        }
    }
    // colour group
    let (a, b) = if r.gen::<f32>() < 0.5 { (h1, h2) } else { (h2, h1) };
    g.hue = if r.gen::<f32>() > 0.06 { a.hue } else { b.hue };
    g.hue2 = if r.gen::<f32>() > 0.06 { a.hue2 } else { b.hue2 };
    g.pat = if r.gen::<f32>() > 0.06 { a.pat } else { b.pat };
    g.epoch = h1.epoch.max(h2.epoch) + 1;
    let mut q = 0u16;
    for qi in 0..NQ {
        let bit = 1u16 << qi;
        let (in1, in2) = (h1.q & bit != 0, h2.q & bit != 0);
        if (in1 && in2) || ((in1 || in2) && r.gen::<f32>() < 0.5) {
            q |= bit;
        }
    }
    g.q = q;
    mutate(r, &g, 1.0)
}

/// Alleles -> body: additive + developmental noise; quirks dominant or recessive.
pub fn phenotype(r: &mut Rng, h1: &Genome, h2: &Genome) -> Genome {
    let mut g = h1.clone();
    for k in 0..NG {
        g.v[k] = clampg(k, (h1.v[k] + h2.v[k]) / 2.0 * (1.0 + gauss(r, 0.0, 0.025)));
    }
    let d = ((h2.hue - h1.hue + 180.0).rem_euclid(360.0)) - 180.0;
    g.hue = (h1.hue + d / 2.0).rem_euclid(360.0);
    let d2 = ((h2.hue2 - h1.hue2 + 180.0).rem_euclid(360.0)) - 180.0;
    g.hue2 = (h1.hue2 + d2 / 2.0).rem_euclid(360.0);
    g.epoch = h1.epoch.max(h2.epoch);
    g.pat = if r.gen::<f32>() < 0.5 { h1.pat } else { h2.pat };
    let mut q = 0u16;
    for qi in 0..NQ {
        let bit = 1u16 << qi;
        let (in1, in2) = (h1.q & bit != 0, h2.q & bit != 0);
        if (in1 && in2) || ((in1 || in2) && (QDOM & bit != 0 || r.gen::<f32>() < 0.25)) {
            q |= bit;
        }
    }
    g.q = q;
    g
}

pub fn dist(a: &Genome, b: &Genome) -> f32 {
    let mut d = 0.0;
    for &k in DK.iter() {
        d += (a.v[k] - b.v[k]).abs() / (RANGE[k].1 - RANGE[k].0);
    }
    d /= DK.len() as f32;
    let mut v = 0.0;
    for &k in VK.iter() {
        v += (a.v[k] - b.v[k]).abs() / (RANGE[k].1 - RANGE[k].0);
    }
    d + 0.3 * v / VK.len() as f32 + 0.12 * (a.q ^ b.q).count_ones() as f32
}

pub fn trait_note(g: &Genome, rf: &Genome) -> String {
    let (mut best, mut bk) = (0f32, usize::MAX);
    for &k in DK.iter().chain(VK.iter()) {
        let d = (g.v[k] - rf.v[k]) / (RANGE[k].1 - RANGE[k].0);
        if d.abs() > best.abs() {
            best = d;
            bk = k;
        }
    }
    let gained = g.q & !rf.q;
    if gained != 0 && best.abs() < 0.25 {
        return format!("+{}", QNAMES[gained.trailing_zeros() as usize]);
    }
    if bk == usize::MAX {
        "drift".into()
    } else if best > 0.0 {
        up(bk).into()
    } else {
        down(bk).into()
    }
}

/// How well a diet extracts energy from plants / from meat. Specialists do well; a middle diet
/// (omnivory) gets little from either, so it is a fitness valley that is hard to cross.
/// Specialists (diet <= 0.2 or >= 0.8) digest their food fully; in between both fall off steeply.
#[inline]
pub fn plant_eff(diet: f32) -> f32 {
    meat_eff(1.0 - diet)
}
#[inline]
pub fn meat_eff(diet: f32) -> f32 {
    let u = ((diet - 0.2) / 0.6).clamp(0.0, 1.0);
    (u * u * (3.0 - 2.0 * u)).powf(2.5)
}

pub fn heterozygosity(h1: &Genome, h2: &Genome) -> f32 {
    DK.iter().map(|&k| (h1.v[k] - h2.v[k]).abs() / (RANGE[k].1 - RANGE[k].0)).sum::<f32>() / DK.len() as f32
}

// ---- body plans (abilities need compatible anatomy)
pub fn stage(g: &Genome) -> &'static str {
    let c = g.v[COMPLEXITY];
    if c < 0.22 {
        "microbial colony"
    } else if c < 0.42 {
        "colonial life"
    } else if c < 0.65 {
        "simple animal"
    } else {
        "complex animal"
    }
}
pub fn can_swim(g: &Genome) -> bool {
    g.has(SWIMMER) || g.v[GILLS] > 0.45 || g.v[FINS] > 0.35
}
pub fn aquatic(g: &Genome) -> bool {
    g.v[GILLS] > 0.55 && g.v[LUNGS] < 0.4
}
pub fn can_fly(g: &Genome) -> bool {
    let lift = g.v[WINGS] - 0.025 * g.v[SIZE] - 0.16 * g.v[ARMOR];
    g.v[COMPLEXITY] > 0.6 && g.v[LUNGS] > 0.5 && lift > 0.48
}
pub fn habitat(g: &Genome) -> &'static str {
    if can_fly(g) {
        "winged / aerial"
    } else if aquatic(g) {
        "aquatic / gill breathing"
    } else if can_swim(g) {
        "amphibious"
    } else {
        "terrestrial"
    }
}

/// Gigantism is a late innovation: bodies can grow past the ancestral limit only after many
/// generations of complex animal life, and the limit rises slowly from there.
pub fn size_cap(g: &Genome) -> f32 {
    if g.v[COMPLEXITY] < 0.65 {
        return 8.0;
    }
    8.0 + 18.0 * ((g.epoch as f32 - 30.0) / 90.0).clamp(0.0, 1.0)
}

/// Age of maturity: big bodies take longer to grow up.
#[inline]
pub fn mat_age(g: &Genome) -> f32 {
    g.v[MAT] * (1.0 + (g.v[SIZE] - 8.0).max(0.0) / 10.0)
}

/// Body colours from the genes: hue, saturation (SAT) and lightness (LUM). `jit` is a per-individual hue jitter.
/// Returns (body, dark, light, secondary).
pub fn body_colors(g: &Genome, jit: f32) -> ([u8; 3], [u8; 3], [u8; 3], [u8; 3]) {
    use crate::util::hsv;
    let h = g.hue + jit;
    let (mut sat, mut val) = (12.0 + 83.0 * g.v[SAT], 28.0 + 70.0 * g.v[LUM]);
    if g.has(CAMOUFLAGE) {
        sat *= 0.6;
        val *= 0.85;
    }
    (
        hsv(h, sat, val),
        hsv(h, (sat + 15.0).min(100.0), val * 0.55),
        hsv(h, sat * 0.45, (val + 25.0).min(100.0)),
        hsv(g.hue2, 25.0 + 55.0 * g.v[SAT], 40.0 + 55.0 * g.v[LUM]),
    )
}

/// Warning colouration: how vivid (bright and saturated) a body is, 0..1.
pub fn vividness(g: &Genome) -> f32 {
    ((g.v[SAT] - 0.5) * 2.0).clamp(0.0, 1.0) * (0.5 + 0.5 * g.v[LUM])
}

/// A short description of the body plan, e.g. "giant long-necked browser".
pub fn body_plan(g: &Genome) -> String {
    if g.v[COMPLEXITY] < 0.42 {
        return stage(g).into();
    }
    let mut s = String::new();
    if g.v[SIZE] > 16.0 {
        s.push_str("colossal ");
    } else if g.v[SIZE] > 10.0 {
        s.push_str("giant ");
    }
    if g.v[ARMOR] > 0.7 {
        s.push_str("armored ");
    }
    if g.v[PLATES] > 0.5 {
        s.push_str("plated ");
    }
    if g.v[HORN] > 0.75 {
        s.push_str("horned ");
    }
    let carn = g.v[DIET] > 0.6;
    let core = if can_fly(g) {
        "flyer"
    } else if aquatic(g) {
        "swimmer"
    } else if g.v[NECK] > 0.6 {
        "long-necked browser"
    } else if g.v[STANCE] > 0.6 {
        if carn { "two-legged hunter" } else { "two-legged runner" }
    } else if carn {
        "four-legged hunter"
    } else {
        "four-legged grazer"
    };
    s.push_str(core);
    s
}
