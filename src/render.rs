//! Software pixel renderer at world resolution (one pixel per cell); the GPU then scales it.
use crate::genome::*;
use crate::sim::*;
use crate::util::*;
use crate::vegetation::{Form, PlantLineage};
use rand::{Rng as _, SeedableRng};
use rustc_hash::FxHashMap;

pub const BONE: Col = [232, 226, 196];
pub const WHITE: Col = [245, 245, 245];
const PCOL: [Col; 4] = [[0, 0, 0], [52, 96, 50], [60, 138, 58], [112, 196, 70]];

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

pub fn plant_sprite(p: &PlantLineage, growth: u8) -> Sprite {
    let mut rng = Rng::seed_from_u64(p.id as u64 * 7919 + growth as u64);
    let form = p.form();
    let age = if growth < 3 { 0.7 } else { 1.0 };
    let rf = |r: &mut Rng| r.gen::<f32>();
    if form == Form::Kelp {
        let h = ((5.0 + p.height * 12.0) * age) as i32;
        let (sw, sh) = (7, h + 1);
        let mut px = vec![[0u8; 4]; (sw * sh) as usize];
        let mut cv = Canvas { w: sw, h: sh, px: &mut px };
        let (col, dark) = (hsv(p.hue, 60.0, 50.0), hsv(p.hue, 70.0, 35.0));
        for strand in 0..(1 + (p.branching * 2.0) as i32) {
            let x = 3 + strand - 1;
            for y in 0..h {
                let xx = x + ((y as f32 * 0.6 + strand as f32).sin() * (1.0 + p.droop)).round() as i32;
                cv.set(xx, sh - 1 - y, if y % 3 != 0 { col } else { dark });
            }
        }
        return Sprite { w: sw, h: sh, ax: 3, ay: sh - 1, px };
    }
    let tree = form == Form::Tree;
    let trunk_h = ((if tree { 3.0 + p.height * 9.0 } else { 1.0 + p.height * 2.0 }) * age) as i32;
    let cw = ((2.0 + p.crown * 6.0 + p.branching * 2.0) * if tree { 1.0 } else { 0.7 } * age).max(1.5);
    let ch = ((3.0 + (1.0 - p.crown) * 7.0 + p.height * 3.0) * if tree { 1.0 } else { 0.55 } * age).max(2.0);
    let sw = (2.0 * cw + 5.0 + p.droop * 4.0) as i32;
    let sh = (trunk_h as f32 + ch + 3.0 + p.droop * ch) as i32;
    let mut px = vec![[0u8; 4]; (sw * sh) as usize];
    let mut cv = Canvas { w: sw, h: sh, px: &mut px };
    let (cx, base) = (sw / 2, sh - 1);
    let (bark, bark2) = (hsv(25.0 + p.hue * 0.05, 50.0, 38.0), hsv(25.0, 45.0, 26.0));
    let girth = 1 + (p.girth > 0.45) as i32 + (p.girth > 0.8 && tree) as i32;
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
    for b in 0..(1 + (p.branching * 4.0) as i32) {
        let side = if b % 2 == 1 { -1 } else { 1 };
        let by = top - (rf(&mut rng) * ch * 0.6) as i32;
        let ln = (cw * (0.4 + rf(&mut rng) * 0.6)) as i32;
        for t in 0..ln {
            let x = cx + side * t;
            let y = by - (t as f32 * (1.0 - p.crown) * 0.8) as i32 + (t as f32 * p.droop * 0.5) as i32;
            cv.set(x, y, bark);
        }
    }
    let leaf = hsv(p.hue, 50.0 + p.density * 25.0, 40.0 + p.density * 15.0);
    let light = hsv(p.hue + 8.0, 40.0 + p.density * 20.0, 62.0 + p.density * 15.0);
    let shadow = hsv(p.hue - 6.0, 65.0, 26.0 + p.density * 8.0);
    let ccy = top as f32 - ch * 0.55;
    let clusters = (6.0 + p.density * 14.0 + p.branching * 6.0) as i32;
    for _ in 0..clusters {
        let a = rf(&mut rng) * std::f32::consts::TAU;
        let r = rf(&mut rng).powf(0.6);
        let mut dx = a.cos() * r * cw;
        let vy = a.sin() * r * ch * 0.5;
        if p.crown < 0.4 && vy < 0.0 {
            dx *= (0.5 + vy / ch + (1.0 - p.crown) * 0.2).max(0.15);
        }
        let (x, y) = ((cx as f32 + dx) as i32, (ccy + vy) as i32);
        let rad = 1.max((1.0 + p.density * 2.0 * rf(&mut rng) + if tree { 1.0 } else { 0.0 }) as i32);
        cv.circle(x, y + 1, rad, shadow);
        cv.circle(x, y, rad, leaf);
        if (y as f32) < ccy && rf(&mut rng) < 0.6 {
            cv.set(x - rad / 2, y - rad / 2, light);
        }
        if p.droop > 0.45 && rf(&mut rng) < p.droop {
            for t in 0..(p.droop * ch * 0.9) as i32 {
                let yy = y + rad + t;
                if yy < base - 1 {
                    cv.set(x, yy, if t % 2 == 1 { leaf } else { shadow });
                }
            }
        }
    }
    if p.bloom > 0.35 && growth >= 3 {
        let bc = hsv(p.bloom_hue, 70.0, 95.0);
        for _ in 0..(p.bloom * 10.0) as i32 {
            let a = rf(&mut rng) * std::f32::consts::TAU;
            let r = rf(&mut rng).sqrt();
            cv.set((cx as f32 + a.cos() * r * cw) as i32, (ccy + a.sin() * r * ch * 0.45) as i32, bc);
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
    base_key: (u64, usize),
    pub sprites: FxHashMap<(u32, u8), Sprite>,
}

impl WorldRenderer {
    pub fn new() -> Self {
        WorldRenderer { px: vec![], base: vec![], base_key: (u64::MAX, 0), sprites: FxHashMap::default() }
    }

    pub fn reset(&mut self) {
        self.sprites.clear();
        self.base_key = (u64::MAX, 0);
    }

    /// Compose the world at native resolution. `rect` = visible region (x0,y0,x1,y1) to limit work.
    pub fn compose(&mut self, s: &Sim, rect: (i32, i32, i32, i32), sel: Option<u32>) {
        let (w, h) = (s.w as i32, s.h as i32);
        let n = s.n;
        if self.px.len() != n {
            self.px = vec![[0, 0, 0, 255]; n];
            self.base = vec![[0, 0, 0, 255]; n];
            self.base_key = (u64::MAX, 0);
        }
        // terrain base layer with sparse land details (rebuilt only when the terrain changes)
        let key = (s.bg_version, s.n);
        if self.base_key != key {
            self.base_key = key;
            for i in 0..n {
                let c = s.bg[i];
                self.base[i] = [c[0], c[1], c[2], 255];
            }
            let mut i = 0;
            while i < n {
                let (x, y) = ((i % s.w) as i32, (i / s.w) as i32);
                if s.terr[i] == 2 {
                    self.base[i] = [132, 132, 142, 255];
                } else if s.terr[i] != 1 && (i * 17) % 11 == 0 {
                    self.base[i] = [119, 105, 73, 255];
                }
                let _ = (x, y);
                i += 43;
            }
            for &sp in &s.springs {
                let i = sp as usize;
                let (x, y) = ((i % s.w) as i32, (i / s.w) as i32);
                for dx in -1..=1 {
                    if x + dx >= 0 && x + dx < w {
                        self.base[(y * w + x + dx) as usize] = [89, 165, 190, 255];
                    }
                }
                if y > 0 {
                    self.base[((y - 1) * w + x) as usize] = [125, 188, 205, 255];
                }
            }
        }
        let (rx0, ry0, rx1, ry1) = (rect.0.max(0), rect.1.max(0), rect.2.min(w), rect.3.min(h));
        // margins so sprites partially outside the view still draw
        let (mx0, my0, mx1, my1) = ((rx0 - 16).max(0), (ry0 - 4).max(0), (rx1 + 16).min(w), (ry1 + 40).min(h));
        for y in my0..my1 {
            let row = (y * w) as usize;
            for x in mx0..mx1 {
                let i = row + x as usize;
                let st = s.pl[i];
                self.px[i] = if st > 0 {
                    let c = PCOL[st as usize];
                    [c[0], c[1], c[2], 255]
                } else {
                    self.base[i]
                };
            }
        }
        // animated water glints
        let mut i = (my0 * w + mx0) as usize;
        let end = (my1 * w) as usize;
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
        let mut trees: Vec<u32> = s
            .vegetation
            .trees
            .iter()
            .copied()
            .filter(|&i| {
                let (x, y) = ((i as usize % s.w) as i32, (i as usize / s.w) as i32);
                x >= mx0 - 12 && x < mx1 + 12 && y >= my0 && y < my1 + 40
            })
            .collect();
        trees.sort_unstable();
        if self.sprites.len() > 8000 {
            self.sprites.clear();
        }
        for i in trees {
            let iu = i as usize;
            let p = s.vegetation.lineage(iu);
            let key = (p.id, s.pl[iu]);
            let spr = self.sprites.entry(key).or_insert_with(|| plant_sprite(p, s.pl[iu]));
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
