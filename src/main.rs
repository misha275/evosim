//! EVOSIM (Rust) - pixel ecosystem with evolving creatures and plants.
//! The simulation runs on its own thread (with rayon for perception/plant sweeps);
//! the GPU scales the native-resolution world image and draws the UI.
use evosim::genome::*;
use evosim::render::{plant_sprite, portrait, WorldRenderer};
use evosim::save::*;
use evosim::sim::*;
use evosim::util::{hsv, scale, Col};
use evosim::vegetation::TRAITS as PLANT_TRAITS;
use macroquad::prelude::*;
use parking_lot::{Mutex, MutexGuard};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const PANEL: f32 = 400.0;
const ZMAX: f32 = 14.0;
/// One tick = 0.2 s of simulated time, so x1 = 5 ticks per real second.
const REALTIME_TPS: f64 = 5.0;
const SPEEDS: [f64; 10] = [1.0, 5.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 5000.0, f64::INFINITY];
const AUTOSAVE_SECS: f32 = 60.0;
const HDR: f32 = 72.0;

const BG: Col = [14, 16, 24];
const FG: Col = [205, 214, 228];
const DIM: Col = [115, 124, 140];

fn c(c: Col) -> Color {
    Color::from_rgba(c[0], c[1], c[2], 255)
}

const FS: f32 = 15.0;
const FB: f32 = 21.0;

fn txt(s: &str, x: f32, y: f32, size: f32, col: Col) {
    draw_text(s, x.round(), (y + size * 0.72).round(), size, c(col));
}
fn tw(s: &str, size: f32) -> f32 {
    measure_text(s, None, size as u16, 1.0).width
}
fn bar(x: f32, y: f32, w: f32, frac: f32, col: Col, h: f32) {
    draw_rectangle(x, y, w, h, c([36, 40, 54]));
    draw_rectangle(x, y, w * frac.clamp(0.0, 1.0), h, c(col));
}
fn speed_label(i: usize) -> String {
    if SPEEDS[i].is_infinite() {
        "MAX".into()
    } else {
        format!("x{}", SPEEDS[i])
    }
}

// ------------------------------------------------------------------------------- sim thread
struct Shared {
    sim: Mutex<Sim>,
    target: AtomicU64, // f64 bits, ticks per second (inf = as fast as possible)
    paused: AtomicBool,
    quit: AtomicBool,
    tps: AtomicU64,
    dt: AtomicU64,
}

fn sim_thread(sh: Arc<Shared>) {
    let mut debt = 0.0f64;
    let mut last = Instant::now();
    let mut meas_t = Instant::now();
    let mut meas_ticks = 0i64;
    let mut busy = Duration::ZERO;
    // Adaptive integration step: exact dt=1 whenever the machine keeps up; only when it falls
    // behind does dt grow (2, 4, 8). Creatures eat at most once per step, so dt never exceeds 8.
    let mut dt: i32 = 1;
    loop {
        if sh.quit.load(Ordering::Relaxed) {
            return;
        }
        let now = Instant::now();
        let el = (now - last).as_secs_f64();
        last = now;
        let target = f64::from_bits(sh.target.load(Ordering::Relaxed));
        let window = meas_t.elapsed().as_secs_f64();
        if window >= 0.5 {
            sh.tps.store((meas_ticks as f64 / window).to_bits(), Ordering::Relaxed);
            let load = busy.as_secs_f64() / window;
            if target.is_infinite() {
                dt = 8;
            } else if load > 0.85 && debt > dt as f64 * 4.0 && dt < 8 {
                dt *= 2;
            } else if load < 0.3 && dt > 1 {
                dt /= 2;
            }
            meas_t = Instant::now();
            meas_ticks = 0;
            busy = Duration::ZERO;
        }
        sh.dt.store(dt as u64, Ordering::Relaxed);
        if sh.paused.load(Ordering::Relaxed) {
            debt = 0.0;
            std::thread::sleep(Duration::from_millis(4));
            continue;
        }
        if target.is_finite() {
            debt = (debt + target * el).min(target * 0.25 + 8.0);
        }
        let mut g = sh.sim.lock();
        let start = Instant::now();
        while (target.is_infinite() || debt >= dt as f64) && start.elapsed() < Duration::from_millis(12) {
            g.step(dt);
            debt -= dt as f64;
            meas_ticks += dt as i64;
        }
        busy += start.elapsed();
        MutexGuard::unlock_fair(g);
        if target.is_finite() && debt < dt as f64 {
            std::thread::sleep(Duration::from_micros(800));
        } else {
            std::thread::yield_now();
        }
    }
}

// ------------------------------------------------------------------------------- UI state
struct TreeView {
    fs: bool,
    zx: f32,
    zy: f32,
    px: f32,
    py: f32,
    pin: Option<usize>,
    ex: bool,
    mn: bool,
    kind: usize, // 0 all, 1 microbial, 2 animal, 3 plant
    drag: Option<(Vec2, bool)>,
    geo: Option<(f32, f32, f32, f32, f32)>,
    hits: Vec<(usize, Rect)>,
    tabs: Vec<(usize, Rect)>,
    close: Rect,
}
impl TreeView {
    fn new() -> Self {
        TreeView {
            fs: false, zx: 1.0, zy: 1.0, px: 0.0, py: 0.0, pin: None, ex: true, mn: false, kind: 0, drag: None, geo: None,
            hits: vec![], tabs: vec![], close: Rect::new(0.0, 0.0, 0.0, 0.0),
        }
    }
    fn switch(&mut self, kind: usize) {
        *self = TreeView { fs: true, ex: self.ex, mn: self.mn, kind, ..TreeView::new() };
    }
}

struct App {
    sh: Arc<Shared>,
    view: View,
    drag: Option<(Vec2, bool)>,
    si: usize,
    paused: bool,
    sel: Option<u32>,
    sel_snap: Option<Cr>,
    tv: TreeView,
    rend: WorldRenderer,
    tex: Option<Texture2D>,
    wname: String,
    last_save: Instant,
    saving: Arc<AtomicBool>,
    mouse_d: Vec2,
}

fn mapw() -> f32 {
    (screen_width() - PANEL).max(200.0)
}
fn maph() -> f32 {
    screen_height()
}

impl App {
    fn fit(&self, s: &Sim) -> f32 {
        (mapw() / s.w as f32).min(maph() / s.h as f32)
    }
    fn new_view(&mut self, s: &Sim) {
        self.view = View { z: self.fit(s), x: 0.0, y: 0.0 };
        self.clamp_view(s);
    }
    fn clamp_view(&mut self, s: &Sim) {
        let v = &mut self.view;
        let zmin = (mapw() / s.w as f32).min(maph() / s.h as f32).min(0.8);
        v.z = v.z.clamp(zmin, ZMAX);
        let (vw, vh) = (mapw() / v.z, maph() / v.z);
        let (w, h) = (s.w as f32, s.h as f32);
        v.x = if vw >= w { (w - vw) / 2.0 } else { v.x.clamp(0.0, w - vw) };
        v.y = if vh >= h { (h - vh) / 2.0 } else { v.y.clamp(0.0, h - vh) };
    }
    fn zoom_at(&mut self, s: &Sim, mx: f32, my: f32, f: f32) {
        let (wx, wy) = (self.view.x + mx / self.view.z, self.view.y + my / self.view.z);
        self.view.z *= f;
        self.clamp_view(s);
        self.view.x = wx - mx / self.view.z;
        self.view.y = wy - my / self.view.z;
        self.clamp_view(s);
    }
    fn to_world(&self, mx: f32, my: f32) -> (f32, f32) {
        (self.view.x + mx / self.view.z, self.view.y + my / self.view.z)
    }
    fn set_speed(&mut self) {
        self.sh.target.store((SPEEDS[self.si] * REALTIME_TPS).to_bits(), Ordering::Relaxed);
        self.sh.paused.store(self.paused, Ordering::Relaxed);
    }

    fn snapshot(&self, s: &Sim) -> Snapshot {
        Snapshot { sim: s.clone(), view: self.view, speed_index: self.si, paused: self.paused }
    }

    /// Clone under the lock (fast), compress & write on a background thread.
    fn save_async(&self, s: &Sim, path: std::path::PathBuf) {
        if self.saving.swap(true, Ordering::SeqCst) {
            return;
        }
        let snap = self.snapshot(s);
        let flag = self.saving.clone();
        std::thread::spawn(move || {
            if let Err(e) = write_snapshot(&path, &snap) {
                eprintln!("[evosim] save failed: {e}");
            }
            flag.store(false, Ordering::SeqCst);
        });
    }
}

// ------------------------------------------------------------------------------- panel widgets
fn draw_graph(x: f32, y: f32, w: f32, h: f32, hist: &std::collections::VecDeque<[u32; 5]>) {
    draw_rectangle(x, y, w, h, c([20, 23, 34]));
    if hist.len() < 2 {
        return;
    }
    let mx = hist.iter().map(|r| r[0].max(r[1]).max(r[2])).max().unwrap_or(0).max(10) as f32;
    let pm = hist.iter().map(|r| r[3]).max().unwrap_or(1).max(1) as f32;
    let cols: [Col; 4] = [[90, 200, 110], [235, 80, 80], [240, 200, 90], [80, 130, 70]];
    for (j, col) in cols.iter().enumerate() {
        let pt = |i: usize, r: &[u32; 5]| -> Vec2 {
            let v = if j < 3 { r[j] as f32 / mx } else { r[j] as f32 / pm * 0.6 };
            vec2(x + i as f32 * (w - 1.0) / 219.0, y + h - 2.0 - v * (h - 4.0))
        };
        for i in 1..hist.len() {
            let (a, b) = (pt(i - 1, &hist[i - 1]), pt(i, &hist[i]));
            draw_line(a.x, a.y, b.x, b.y, 1.0, c(*col));
        }
    }
}

fn draw_mini_tree(s: &Sim, x0: f32, y0: f32, w: f32, h: f32) {
    draw_rectangle(x0, y0, w, h, c([20, 23, 34]));
    let sps = &s.species;
    let mut vis: std::collections::BTreeSet<usize> = sps.iter().filter(|p| p.peak >= 6).map(|p| p.id as usize).collect();
    for i in vis.clone() {
        let mut p = sps[i].parent;
        while let Some(pp) = p {
            if !vis.insert(pp as usize) {
                break;
            }
            p = sps[pp as usize].parent;
        }
    }
    let kids_of = |vis: &std::collections::BTreeSet<usize>| {
        let mut kids: std::collections::HashMap<Option<u32>, Vec<usize>> = Default::default();
        for &i in vis {
            kids.entry(sps[i].parent).or_default().push(i);
        }
        kids
    };
    let mut kids = kids_of(&vis);
    while vis.len() > 12 {
        let leaves: Vec<usize> = vis.iter().copied().filter(|&i| sps[i].pop == 0 && !kids.contains_key(&Some(i as u32))).collect();
        let Some(&d) = leaves.iter().min_by_key(|&&i| (sps[i].peak, sps[i].born)) else { break };
        vis.remove(&d);
        kids = kids_of(&vis);
    }
    let mut order = vec![];
    let mut stack: Vec<usize> = kids.get(&None).cloned().unwrap_or_default();
    stack.sort_by_key(|&i| -sps[i].born);
    while let Some(i) = stack.pop() {
        order.push(i);
        let mut k = kids.get(&Some(i as u32)).cloned().unwrap_or_default();
        k.sort_by_key(|&j| -sps[j].born);
        stack.extend(k);
    }
    if order.is_empty() {
        return;
    }
    let rh = 17.0f32.min((h - 8.0) / order.len() as f32);
    let row = |i: usize| -> f32 { y0 + 6.0 + (order.iter().position(|&o| o == i).unwrap() as f32 + 0.5) * rh };
    let now = s.t.max(1) as f32;
    let xf = |t: i64| x0 + 6.0 + (w - 14.0) * t as f32 / now;
    for &i in &order {
        let p = &sps[i];
        let a = p.pop > 0;
        let col = if a { p.col } else { scale(p.col, 0.45) };
        let (xs, xe, y) = (xf(p.born), xf(p.died.unwrap_or(s.t)), row(i));
        if let Some(par) = p.parent {
            if order.contains(&(par as usize)) {
                draw_line(xs, row(par as usize), xs, y, 1.0, c(col));
            }
        }
        draw_line(xs, y, xe, y, if a { 2.0 } else { 1.0 }, c(col));
        if !a {
            draw_line(xe - 2.0, y - 2.0, xe + 2.0, y + 2.0, 1.0, c([200, 80, 80]));
            draw_line(xe - 2.0, y + 2.0, xe + 2.0, y - 2.0, 1.0, c([200, 80, 80]));
        }
        let label = format!("{}{}{}", p.name, if p.note.is_empty() { String::new() } else { format!(" {}", p.note) }, if a { format!("  n={}", p.pop) } else { String::new() });
        let lw = tw(&label, FS - 2.0);
        txt(&label, (xs + 4.0).min(x0 + w - lw - 3.0), y - 12.0, FS - 2.0, if a { col } else { DIM });
    }
}

fn persona(c: &Cr) -> String {
    let [b, so, cu] = c.pers;
    let w: Vec<&str> = [
        if b > 0.65 { Some("bold") } else if b < 0.35 { Some("shy") } else { None },
        if so > 0.65 { Some("social") } else if so < 0.35 { Some("loner") } else { None },
        if cu > 0.65 { Some("curious") } else if cu < 0.35 { Some("cautious") } else { None },
    ]
    .into_iter()
    .flatten()
    .collect();
    if w.is_empty() {
        "easy-going".into()
    } else {
        w.join(", ")
    }
}

fn draw_pixels(px: &[[u8; 4]], pw: i32, ph: i32, x: f32, y: f32, k: f32) {
    for yy in 0..ph {
        for xx in 0..pw {
            let p = px[(yy * pw + xx) as usize];
            if p[3] > 0 {
                draw_rectangle(x + xx as f32 * k, y + yy as f32 * k, k, k, Color::from_rgba(p[0], p[1], p[2], p[3]));
            }
        }
    }
}

fn draw_portrait(cr: &Cr, t: i64, dark: f32, x: f32, y: f32, k: f32) {
    let px = portrait(cr, t, dark, 20, 16);
    draw_pixels(&px, 20, 16, x, y, k);
}

fn quirk_list(q: u16, x0: f32, mut y: f32, w: f32) -> f32 {
    txt("quirks", x0, y, FS, DIM);
    let mut x = x0 + 50.0;
    if q == 0 {
        txt("none", x, y, FS, DIM);
    }
    for i in 0..NQ {
        if q & (1 << i) != 0 {
            let n = QNAMES[i];
            let ww = tw(n, FS);
            if x + ww > x0 + w {
                x = x0 + 50.0;
                y += 13.0;
            }
            txt(n, x, y, FS, QCOL[i]);
            x += ww + 8.0;
        }
    }
    y
}

fn draw_inspector(s: &Sim, cr: Option<&Cr>, dead: bool, x: f32, y: f32, w: f32) {
    let Some(cr) = cr else {
        for (i, l) in [
            "click creature = inspect | drag = pan | wheel/+/- = zoom",
            "arrows pan | 0 reset view | RMB seeds | H/C founders",
            "T or click tree = fullscreen tree | N world size",
            "1-9 speed, M = MAX, [ ] step | SPACE pause | F5/F9",
        ]
        .iter()
        .enumerate()
        {
            txt(l, x, y + 4.0 + i as f32 * 15.0, FS - 1.0, DIM);
        }
        return;
    };
    let g = &cr.g;
    let sp = &s.species[cr.sp as usize];
    draw_portrait(cr, s.t, s.dark, x, y, 5.0);
    draw_rectangle_lines(x, y, 100.0, 80.0, 1.0, c([50, 56, 76]));
    let tx = x + 108.0;
    let mass_g = MK * cr.s.powi(3) * 1000.0;
    txt(&cr.nm, tx, y, FB, sp.col);
    txt(&format!("{}  gen {}  {}", sp.name, cr.gen, if dead { "[dead]" } else { "" }), tx, y + 18.0, FS, DIM);
    let preg = cr.preg.as_ref().map_or(String::new(), |p| format!("  PREGNANT x{}", p.1.len()));
    txt(&format!("{} {}{}", if cr.male { "male" } else { "female" }, if cr.age >= g.v[MAT] { "adult" } else { "juvenile" }, preg), tx, y + 31.0, FS, FG);
    let mass = if mass_g < 1000.0 { format!("{:.0} g", mass_g) } else { format!("{:.2} kg", mass_g / 1000.0) };
    txt(&format!("{:.1} cm  {}  {:.0} cm/s", cr.s * CELL_CM, mass, g.v[SPD] * CELL_CM / TICK_S), tx, y + 44.0, FS, FG);
    txt(&format!("age {:.1}/{:.1} d  hetero {:.2}", cr.age / DAY as f32, g.v[LIFE] / DAY as f32, cr.het), tx, y + 57.0, FS, DIM);
    txt("energy", tx, y + 70.0, FS, DIM);
    bar(tx + 52.0, y + 72.0, 130.0, cr.e / cr.max_e, [240, 190, 60], 7.0);
    txt(&persona(cr), tx, y + 83.0, FS, [160, 230, 160]);
    let qy = quirk_list(g.q, x, y + 99.0, w);
    let gy = qy + 18.0;
    let rows: [(&str, i32); 18] = [
        ("speed", SPD as i32), ("size", SIZE as i32), ("vision", VIS as i32), ("diet", DIET as i32), ("metab", METAB as i32),
        ("aggr", AGGR as i32), ("climate", TOL as i32), ("care", CARE as i32), ("choosy", CHOOSY as i32), ("breed", THRESH as i32),
        ("bold", -1), ("social", -2), ("curious", -3), ("horns", HORN as i32), ("tail", TAIL as i32), ("ears", EARS as i32),
        ("legs", LEGS as i32), ("eyes", EYES as i32),
    ];
    for (i, (lab, k)) in rows.iter().enumerate() {
        let (cx, cy) = (x + (i % 2) as f32 * 195.0, gy + (i / 2) as f32 * 13.0);
        txt(lab, cx, cy - 2.0, FS - 1.0, DIM);
        let (frac, col) = if *k < 0 {
            (cr.pers[(-k - 1) as usize], [140, 220, 150])
        } else {
            let k = *k as usize;
            let (lo, hi) = RANGE[k];
            ((g.v[k] - lo) / (hi - lo), if k == DIET { [220, 80, 80] } else if VK.contains(&k) { [200, 140, 240] } else { [110, 170, 240] })
        };
        bar(cx + 56.0, cy + 1.0, 114.0, frac, col, 6.0);
    }
    let ay = gy + 9.0 * 13.0 + 2.0;
    txt("ancestry (newest first):", x, ay, FS, DIM);
    let mut parts = vec![];
    let mut cur = cr.id;
    while let Some(l) = s.lin.get(&cur) {
        parts.push((l.gen, l.note.clone()));
        if l.par == 0 || parts.len() >= 24 {
            break;
        }
        cur = l.par;
    }
    let (mut cx, mut cy) = (x, ay + 14.0);
    for (gen, note) in parts.iter().take(14) {
        let s_ = format!("g{} {} ", gen, if note.is_empty() { "-" } else { note });
        let col = if note.starts_with('!') {
            [255, 210, 120]
        } else if note.starts_with('+') || note.starts_with('-') {
            [160, 230, 160]
        } else {
            FG
        };
        let wd = tw(&s_, FS - 1.0);
        if cx + wd > x + w {
            cx = x;
            cy += 13.0;
            if cy > ay + 40.0 {
                break;
            }
        }
        txt(&s_, cx, cy, FS - 1.0, col);
        cx += wd + 5.0;
    }
}

// ------------------------------------------------------------------------------- phylogeny
fn tree_layout(s: &Sim, show_ex: bool, minpeak: i32) -> (Vec<usize>, std::collections::HashMap<Option<u32>, Vec<usize>>) {
    let sps = &s.species;
    let mut vis: std::collections::BTreeSet<usize> =
        sps.iter().filter(|p| p.pop > 0 || (show_ex && p.peak >= minpeak)).map(|p| p.id as usize).collect();
    for i in vis.clone() {
        let mut p = sps[i].parent;
        while let Some(pp) = p {
            if !vis.insert(pp as usize) {
                break;
            }
            p = sps[pp as usize].parent;
        }
    }
    let mut kids: std::collections::HashMap<Option<u32>, Vec<usize>> = Default::default();
    for &i in &vis {
        kids.entry(sps[i].parent).or_default().push(i);
    }
    let mut order = vec![];
    let mut stack: Vec<usize> = kids.get(&None).cloned().unwrap_or_default();
    stack.sort_by_key(|&i| -sps[i].born);
    while let Some(i) = stack.pop() {
        order.push(i);
        let mut k = kids.get(&Some(i as u32)).cloned().unwrap_or_default();
        k.sort_by_key(|&j| -sps[j].born);
        stack.extend(k);
    }
    (order, kids)
}

fn species_card(s: &Sim, sp: &Sp, x: f32, y: f32, w: f32) {
    let sps = &s.species;
    txt(&sp.name, x, y, FB, sp.col);
    let parent = sp.parent.map_or("root species (starting population)".to_string(), |p| format!("branched from {}", sps[p as usize].name));
    txt(&parent, x, y + 20.0, FS, DIM);
    draw_rectangle(x, y + 36.0, 120.0, 96.0, c([30, 38, 34]));
    draw_portrait(&Cr::dummy(&sp.founder, sp.id), 0, 0.0, x, y + 36.0, 6.0);
    draw_rectangle_lines(x, y + 36.0, 120.0, 96.0, 1.0, c([50, 56, 76]));
    txt("founder", x + 4.0, y + 38.0, FS - 2.0, DIM);
    let end = sp.died.unwrap_or(s.t);
    let info = [
        ("born", format!("day {}  t={}", sp.born / DAY + 1, sp.born)),
        ("status", if sp.pop > 0 { format!("alive  n={}", sp.pop) } else { format!("extinct t={}", sp.died.unwrap_or(0)) }),
        ("peak", sp.peak.to_string()),
        ("lasted", format!("{:.1} days", (end - sp.born) as f32 / DAY as f32)),
        ("trait", if sp.note.is_empty() { "founder".into() } else { sp.note.clone() }),
        ("kids", format!("{} species", sps.iter().filter(|o| o.parent == Some(sp.id)).count())),
    ];
    for (i, (k, v)) in info.iter().enumerate() {
        txt(k, x + 130.0, y + 38.0 + i as f32 * 15.0, FS, DIM);
        txt(v, x + 176.0, y + 38.0 + i as f32 * 15.0, FS, FG);
    }
    let members: Vec<&Cr> = s.crs.iter().filter(|c| c.sp == sp.id).collect();
    let mut my = y + 142.0;
    if !members.is_empty() {
        txt(&format!("living members ({})", members.len()), x, my, FS, DIM);
        let step = (members.len() / 3).max(1);
        for (j, m) in members.iter().step_by(step).take(3).enumerate() {
            draw_rectangle(x + j as f32 * 90.0, my + 14.0, 80.0, 64.0, c([30, 38, 34]));
            draw_portrait(m, s.t, s.dark, x + j as f32 * 90.0, my + 14.0, 4.0);
        }
        my += 86.0;
    } else {
        txt("no living members", x, my, FS, [200, 90, 90]);
        my += 22.0;
    }
    let g = &sp.founder;
    txt("founder genome", x, my, FS, DIM);
    let rows = [
        ("speed", SPD), ("size", SIZE), ("vision", VIS), ("diet", DIET), ("metab", METAB), ("aggr", AGGR), ("breed", THRESH),
        ("body", BODY), ("ears", EARS), ("horns", HORN), ("tail", TAIL), ("legs", LEGS), ("eyes", EYES), ("mutab", MUT),
    ];
    for (i, (lab, k)) in rows.iter().enumerate() {
        let (cx, cy) = (x + (i % 2) as f32 * 150.0, my + 16.0 + (i / 2) as f32 * 13.0);
        txt(lab, cx, cy - 2.0, FS - 1.0, DIM);
        let (lo, hi) = RANGE[*k];
        let col = if *k == DIET { [220, 80, 80] } else if VK.contains(k) { [200, 140, 240] } else { [110, 170, 240] };
        bar(cx + 44.0, cy + 1.0, 90.0, (g.v[*k] - lo) / (hi - lo), col, 6.0);
    }
    my += 16.0 + 7.0 * 13.0 + 6.0;
    my = quirk_list(g.q, x, my, w);
    let mut chain = vec![];
    let mut q = Some(sp.id);
    while let Some(id) = q {
        chain.push(sps[id as usize].name.clone());
        q = sps[id as usize].parent;
    }
    my += 20.0;
    txt("lineage", x, my, FS, DIM);
    let mut line = String::new();
    let mut ly = my + 14.0;
    for nm in chain.iter().rev() {
        let piece = format!("{} > ", nm);
        if tw(&(line.clone() + &piece), FS) > w {
            txt(&line, x, ly, FS, FG);
            ly += 13.0;
            line.clear();
        }
        line += &piece;
    }
    txt(line.trim_end_matches(" > "), x, ly, FS, FG);
    if sp.pop > 0 {
        txt("ENTER: jump to a living member", x, ly + 22.0, FS, [120, 220, 160]);
    }
}

fn draw_tree_fs(s: &Sim, tv: &mut TreeView) {
    let (sw, shh) = (screen_width(), screen_height());
    let aw = sw - 320.0;
    clear_background(c(BG));
    let (mut order, kids) = tree_layout(s, tv.ex, if tv.mn { 1 } else { 6 });
    let sps = &s.species;
    if tv.kind == 1 || tv.kind == 2 {
        let microbial = tv.kind == 1;
        order.retain(|&i| (sps[i].founder.v[COMPLEXITY] < 0.42) == microbial);
    }
    let n = order.len();
    let rh0 = ((shh - HDR - 40.0) / n.max(1) as f32).clamp(14.0, 26.0);
    let (tw_, x0, y0) = (aw - 120.0, 50.0, HDR + 12.0);
    let now = s.t.max(1) as f32;
    tv.geo = Some((x0, tw_, y0, rh0, aw));
    tv.px = tv.px.min(tw_ * tv.zx - aw + 170.0).max(-60.0);
    tv.py = tv.py.min((n as f32 * rh0 * tv.zy + HDR + 50.0 - shh).max(0.0)).max(-30.0);
    let (px, py, zy) = (tv.px, tv.py, tv.zy);
    let ppt = tw_ * tv.zx / now;
    let xf = |t: i64| x0 + t as f32 * ppt - px;
    let row: std::collections::HashMap<usize, f32> = order.iter().enumerate().map(|(k, &i)| (i, y0 + (k as f32 + 0.5) * rh0 * zy - py)).collect();
    let mp = mouse_position();
    let mut hover = None;
    let mut hd = 1e9;
    tv.hits.clear();
    let mut items = std::collections::HashMap::new();
    for &i in &order {
        let p = &sps[i];
        let (xs, xe, y) = (xf(p.born), xf(p.died.unwrap_or(s.t)), row[&i]);
        let label = format!("{}{}{}", p.name, if p.note.is_empty() { String::new() } else { format!(" {}", p.note) }, if p.pop > 0 { format!("  n={}", p.pop) } else { "  x".into() });
        let lw = tw(&label, FS);
        let lx = (xs + 6.0).max(6.0).min(aw - lw - 6.0);
        let lo_x = xs.min(lx) - 4.0;
        let r = Rect::new(lo_x.max(-5000.0), y - 8.0, (xe.max(lx + lw) + 4.0).min(aw + 5000.0) - lo_x.max(-5000.0), 16.0);
        tv.hits.push((i, r));
        if mp.0 < aw && mp.1 > HDR && mp.1 < shh - 18.0 && r.contains(vec2(mp.0, mp.1)) && (mp.1 - y).abs() < hd {
            hover = Some(i);
            hd = (mp.1 - y).abs();
        }
        items.insert(i, (xs, xe, y, label, lx, lw));
    }
    let focus = tv.pin.or(hover);
    let mut chain = std::collections::HashSet::new();
    if let Some(f) = focus {
        let mut q = Some(f as u32);
        while let Some(id) = q {
            chain.insert(id as usize);
            q = sps[id as usize].parent;
        }
        let mut st = vec![f];
        while let Some(j) = st.pop() {
            for &k in kids.get(&Some(j as u32)).map(|v| v.as_slice()).unwrap_or(&[]) {
                chain.insert(k);
                st.push(k);
            }
        }
    }
    // time grid
    let mut st_t = DAY / 4;
    for cand in [DAY / 4, DAY / 2, DAY, 2 * DAY, 5 * DAY, 10 * DAY, 25 * DAY, 50 * DAY, 100 * DAY, 250 * DAY, 1000 * DAY, 5000 * DAY] {
        st_t = cand;
        if cand as f32 * ppt >= 80.0 {
            break;
        }
    }
    let k0 = (((px - x0) / ppt / st_t as f32) as i64).max(0);
    let k1 = ((px + aw - x0) / ppt / st_t as f32) as i64 + 1;
    for k in k0..=k1 {
        if k * st_t > s.t {
            break;
        }
        let gx = xf(k * st_t);
        draw_line(gx, HDR, gx, shh, 1.0, c([26, 30, 44]));
        txt(&format!("day {}", (k * st_t) as f32 / DAY as f32), gx + 3.0, HDR + 1.0, FS - 2.0, [70, 78, 98]);
    }
    draw_line(xf(s.t), HDR, xf(s.t), shh, 1.0, c([60, 90, 70]));
    let dim = |col: Col, m: f32| scale(col, m);
    for &i in &order {
        let p = &sps[i];
        let a = p.pop > 0;
        let base = if a { p.col } else { dim(p.col, 0.55) };
        let col = if focus.is_none() || chain.contains(&i) { base } else { dim(base, 0.3) };
        let (xs, xe, y, ..) = items[&i];
        if y < HDR - 30.0 || y > shh + 30.0 {
            continue;
        }
        if let Some(par) = p.parent {
            if let Some(&py_) = row.get(&(par as usize)) {
                draw_line(xs, py_, xs, y, 1.0, c(col));
            }
        }
        let th = if a { (2.0 + (p.pop as f32).sqrt() / 3.0).min(6.0) } else { 1.0 };
        draw_line(xs.max(-5000.0), y, xe.min(aw + 5000.0), y, th, c(col));
        draw_circle(xs, y, 3.0, c(col));
        if !a {
            draw_line(xe - 3.0, y - 3.0, xe + 3.0, y + 3.0, 1.0, c([200, 80, 80]));
            draw_line(xe - 3.0, y + 3.0, xe + 3.0, y - 3.0, 1.0, c([200, 80, 80]));
        }
    }
    for &i in &order {
        let p = &sps[i];
        let (xs, xe, y, label, lx, lw) = &items[&i];
        if *xe < 0.0 || *xs > aw || *y < HDR || *y > shh - 18.0 {
            continue;
        }
        let a = p.pop > 0;
        let base = if a { p.col } else { dim(p.col, 0.7) };
        let col = if focus.is_none() || chain.contains(&i) { base } else { dim(base, 0.35) };
        txt(label, *lx, y - 12.0, FS, col);
        if Some(i) == focus {
            draw_rectangle_lines(lx - 3.0, y - 14.0, lw + 6.0, 16.0, 1.0, macroquad::color::WHITE);
        }
    }
    draw_rectangle(0.0, 0.0, sw, HDR, c(BG));
    draw_line(0.0, HDR, aw, HDR, 1.0, c([50, 56, 76]));
    txt("PHYLOGENY", 10.0, 6.0, FB, [120, 220, 160]);
    txt(&format!("{} shown / {} alive / {} total     zoom x{:.1} y{:.1}", n, s.alive_species(), sps.len(), tv.zx, tv.zy), 140.0, 9.0, FS, DIM);
    txt("time ->  (thick line = big population, x = extinct, dots = branch points)", 10.0, 27.0, FS, DIM);
    draw_rectangle(0.0, shh - 18.0, aw, 18.0, c(BG));
    txt(&format!("wheel zoom (shift: time only, ctrl: rows only) | drag pan | click pin | 0 fit | E extinct:{} | M minor:{} | ESC close", if tv.ex { "on" } else { "off" }, if tv.mn { "on" } else { "off" }), 8.0, shh - 16.0, FS - 1.0, DIM);
    draw_rectangle(aw, 0.0, sw - aw, shh, c(BG));
    draw_line(aw, 0.0, aw, shh, 1.0, c([50, 56, 76]));
    if let Some(f) = focus {
        species_card(s, &sps[f], aw + 14.0, 12.0, 296.0);
    } else {
        for (i, l) in ["hover a species to preview it", "click to pin it", "", "highlighted = its ancestors and", "all species that descended from it"].iter().enumerate() {
            txt(l, aw + 14.0, 14.0 + i as f32 * 15.0, FS, DIM);
        }
    }
}

fn draw_plant_phylogeny(s: &Sim, tv: &mut TreeView, sprites: &mut WorldRenderer) {
    let veg = &s.vegetation;
    let (sw, shh) = (screen_width(), screen_height());
    let aw = sw - 320.0;
    clear_background(c(BG));
    let minpeak = if tv.mn { 1 } else { 3 };
    let mut visible: std::collections::BTreeSet<usize> = veg
        .lineages
        .iter()
        .filter(|p| veg.counts[p.id as usize] > 0 || (tv.ex && veg.peaks[p.id as usize] >= minpeak))
        .map(|p| p.id as usize)
        .collect();
    // very large plant trees: keep the most populous living lineages + their ancestry
    if visible.len() > 3000 {
        let mut liv: Vec<usize> = visible.iter().copied().filter(|&i| veg.counts[i] > 0).collect();
        liv.sort_by_key(|&i| std::cmp::Reverse(veg.counts[i]));
        liv.truncate(1500);
        visible = liv.into_iter().collect();
    }
    for i in visible.clone() {
        let mut p = veg.lineages[i].parent;
        while let Some(pp) = p {
            if !visible.insert(pp as usize) {
                break;
            }
            p = veg.lineages[pp as usize].parent;
        }
    }
    let mut kids: std::collections::HashMap<Option<u32>, Vec<usize>> = Default::default();
    for &i in &visible {
        kids.entry(veg.lineages[i].parent).or_default().push(i);
    }
    let mut order = vec![];
    let mut stack: Vec<usize> = kids.get(&None).cloned().unwrap_or_default();
    stack.reverse();
    while let Some(i) = stack.pop() {
        order.push(i);
        let mut k = kids.get(&Some(i as u32)).cloned().unwrap_or_default();
        k.reverse();
        stack.extend(k);
    }
    let rh = ((shh - HDR - 40.0) / order.len().max(1) as f32).clamp(14.0, 26.0);
    let (tw_, x0, y0) = (aw - 120.0, 50.0, HDR + 12.0);
    tv.py = tv.py.min((order.len() as f32 * rh * tv.zy - shh + HDR + 40.0).max(0.0)).max(0.0);
    tv.geo = Some((x0, tw_, y0, rh, aw));
    let now = s.t.max(1) as f32;
    let xf = |t: i64| x0 + t as f32 / now * tw_ * tv.zx - tv.px;
    let rows: std::collections::HashMap<usize, f32> = order.iter().enumerate().map(|(n, &i)| (i, y0 + (n as f32 + 0.5) * rh * tv.zy - tv.py)).collect();
    tv.hits.clear();
    let mp = mouse_position();
    let mut hover = None;
    for &i in &order {
        let p = &veg.lineages[i];
        let y = rows[&i];
        if y < HDR - 25.0 || y > shh + 25.0 {
            continue;
        }
        let live = veg.counts[i];
        let end = if live > 0 { s.t } else { veg.died[i].unwrap_or(p.born) };
        let col = hsv(p.hue, 55.0, if live > 0 { 85.0 } else { 45.0 });
        let (xs, xe) = (xf(p.born), xf(end));
        if let Some(par) = p.parent {
            if let Some(&py_) = rows.get(&(par as usize)) {
                draw_line(xs, py_, xs, y, 1.0, c(col));
            }
        }
        draw_line(xs, y, (xs + 2.0).max(xe), y, (1.0 + (live as f32).sqrt() / 3.0).min(5.0), c(col));
        draw_rectangle(xs - 1.0, y - 1.0, 3.0, 3.0, c(col));
        let label = format!("{} #{} | gen {} | n={}", p.form().name(), i, p.generation, live);
        let lw = tw(&label, FS);
        let lx = (xs + 5.0).max(5.0).min(aw - lw - 5.0);
        if y > HDR && y < shh - 20.0 {
            txt(&label, lx, y - 13.0, FS, col);
        }
        let r = Rect::new(xs.min(lx) - 3.0, y - 15.0, xe.max(lx + lw) - xs.min(lx) + 6.0, 20.0);
        tv.hits.push((i, r));
        if r.contains(vec2(mp.0, mp.1)) && mp.0 < aw && mp.1 > HDR {
            hover = Some(i);
        }
    }
    draw_rectangle(0.0, 0.0, aw, HDR, c(BG));
    let focus = tv.pin.or(hover);
    txt("PLANT PHYLOGENY", 10.0, 8.0, FB, [130, 218, 143]);
    txt(&format!("{} lineages shown / {} living | seeds inherit and mutate", order.len(), veg.counts.iter().filter(|&&n| n > 0).count()), 220.0, 11.0, FS, DIM);
    if order.is_empty() {
        txt("Plant lineages have not established yet.", 20.0, HDR + 30.0, FS, DIM);
    }
    draw_rectangle(aw, 0.0, sw - aw, shh, c(BG));
    draw_line(aw, 0.0, aw, shh, 1.0, c([50, 56, 76]));
    if let Some(f) = focus.filter(|&f| f < veg.lineages.len()) {
        let p = &veg.lineages[f];
        let (x, y) = (aw + 14.0, 18.0);
        txt(&format!("{} #{}", p.form().name(), p.id), x, y, FB, hsv(p.hue, 55.0, 90.0));
        let lines = [
            format!("generation {}", p.generation),
            format!("parent {}", p.parent.map_or("none".into(), |v| v.to_string())),
            format!("born day {:.2}", p.born as f32 / DAY as f32 + 1.0),
            format!("population {} / peak {}", veg.counts[f], veg.peaks[f]),
        ];
        for (n, l) in lines.iter().enumerate() {
            txt(l, x, y + 28.0 + n as f32 * 17.0, FS, DIM);
        }
        for (n, key) in PLANT_TRAITS.iter().enumerate() {
            let yy = y + 110.0 + n as f32 * 17.0;
            txt(key, x, yy, FS, DIM);
            bar(x + 88.0, yy + 2.0, 160.0, p.trait_value(n), [108, 184, 106], 7.0);
        }
        txt("ENTER: jump to a living plant", x, y + 340.0, FS, [120, 220, 160]);
        if p.form().tall() {
            let spr = sprites.sprites.entry((p.id, 3)).or_insert_with(|| plant_sprite(p, 3));
            let k = (240.0 / spr.w as f32).min(200.0 / spr.h as f32).clamp(1.0, 6.0).floor();
            draw_pixels(&spr.px, spr.w, spr.h, x, y + 362.0, k);
        }
    } else {
        txt("Hover or pin a plant lineage.", aw + 14.0, 18.0, FS, DIM);
    }
    draw_rectangle(0.0, shh - 18.0, aw, 18.0, c(BG));
    txt("wheel zoom | drag pan | click pin | E extinct | M minor | 0 fit | ESC close", 8.0, shh - 16.0, FS - 1.0, DIM);
}

fn draw_tabs(tv: &mut TreeView) {
    tv.tabs.clear();
    for (j, label) in ["All creatures", "Microbial", "Animals", "Plants"].iter().enumerate() {
        let r = Rect::new(10.0 + j as f32 * 148.0, 46.0, 140.0, 21.0);
        draw_rectangle(r.x, r.y, r.w, r.h, c(if tv.kind == j { [44, 70, 57] } else { [29, 34, 46] }));
        txt(label, r.x + 8.0, r.y + 4.0, FS, if tv.kind == j { [143, 223, 167] } else { DIM });
        tv.tabs.push((j, r));
    }
    txt("TAB: switch life form", 610.0, 50.0, FS, DIM);
    let sw = screen_width();
    tv.close = Rect::new(sw - 34.0, 6.0, 26.0, 22.0);
    draw_rectangle(tv.close.x, tv.close.y, tv.close.w, tv.close.h, c([40, 46, 64]));
    txt("X", sw - 27.0, 8.0, FB, FG);
}

fn tree_input(app: &mut App, s: &Sim) {
    let md = app.mouse_d;
    let tv = &mut app.tv;
    let mp = vec2(mouse_position().0, mouse_position().1);
    if is_key_pressed(KeyCode::Tab) {
        let k = (tv.kind + 1) % 4;
        tv.switch(k);
        return;
    }
    if is_key_pressed(KeyCode::E) {
        tv.ex = !tv.ex;
    }
    if is_key_pressed(KeyCode::M) {
        tv.mn = !tv.mn;
    }
    if is_key_pressed(KeyCode::Key0) {
        tv.zx = 1.0;
        tv.zy = 1.0;
        tv.px = 0.0;
        tv.py = 0.0;
    }
    if (is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter)) && tv.pin.is_some() {
        let pin = tv.pin.unwrap();
        if tv.kind == 3 {
            if let Some(i) = (0..s.n).find(|&i| s.pl[i] != 0 && s.vegetation.ids[i] as usize == pin) {
                app.view.z = 8.0;
                app.view.x = (i % s.w) as f32 - mapw() / 16.0;
                app.view.y = (i / s.w) as f32 - maph() / 16.0;
                app.tv.fs = false;
                app.clamp_view(s);
            }
            return;
        }
        if let Some(cr) = s.crs.iter().filter(|c| c.sp as usize == pin).max_by(|a, b| a.age.total_cmp(&b.age)) {
            app.sel = Some(cr.id);
            app.view.z = 8.0;
            app.view.x = cr.x - mapw() / 16.0;
            app.view.y = cr.y - maph() / 16.0;
            app.tv.fs = false;
            app.clamp_view(s);
        }
        return;
    }
    if is_mouse_button_pressed(MouseButton::Left) {
        for (kind, r) in tv.tabs.clone() {
            if r.contains(mp) {
                tv.switch(kind);
                return;
            }
        }
        if tv.close.contains(mp) {
            tv.fs = false;
            return;
        }
    }
    let Some((x0, tw_, y0, rh0, aw)) = tv.geo else { return };
    let wheel = mouse_wheel().1;
    if wheel != 0.0 && mp.x < aw && mp.y > HDR {
        let fct = 1.2f32.powf(wheel.signum());
        let ctrl = is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl);
        let shift = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
        let (zx, zy) = (tv.zx, tv.zy);
        let nzx = if ctrl { zx } else { (zx * fct).clamp(0.4, 400.0) };
        let nzy = if shift { zy } else { (zy * fct).clamp(0.4, 10.0) };
        let u = (mp.x + tv.px - x0) / (tw_ * zx);
        let v = (mp.y + tv.py - y0) / (rh0 * zy);
        tv.zx = nzx;
        tv.zy = nzy;
        tv.px = x0 + u * tw_ * nzx - mp.x;
        tv.py = y0 + v * rh0 * nzy - mp.y;
    }
    if is_mouse_button_pressed(MouseButton::Left) && mp.x < aw && mp.y > HDR {
        tv.drag = Some((mp, false));
    }
    if let Some((start, moved)) = &mut tv.drag {
        if is_mouse_button_down(MouseButton::Left) {
            if *moved || (mp - *start).length() > 5.0 {
                *moved = true;
                tv.px -= md.x;
                tv.py -= md.y;
            }
        }
        if is_mouse_button_released(MouseButton::Left) {
            if !*moved {
                tv.pin = tv.hits.iter().find(|(_, r)| r.contains(mp)).map(|(i, _)| *i);
            }
            tv.drag = None;
        }
    }
}

// ------------------------------------------------------------------------------- main view
fn draw_main(app: &mut App, s: &Sim, recompose: bool) {
    let (mw, mh) = (mapw(), maph());
    let v = app.view;
    let need_new = app.tex.as_ref().map_or(true, |t| t.width() as usize != s.w || t.height() as usize != s.h);
    if recompose || need_new {
        let rect = (v.x as i32 - 1, v.y as i32 - 1, (v.x + mw / v.z) as i32 + 2, (v.y + mh / v.z) as i32 + 2);
        app.rend.compose(s, rect, app.sel);
        // SAFETY: [u8; 4] has the same layout as 4 consecutive u8
        let bytes: &[u8] = unsafe { std::slice::from_raw_parts(app.rend.px.as_ptr() as *const u8, app.rend.px.len() * 4) };
        if need_new {
            let t = Texture2D::from_rgba8(s.w as u16, s.h as u16, bytes);
            t.set_filter(FilterMode::Nearest);
            app.tex = Some(t);
        } else {
            app.tex.as_ref().unwrap().update_from_bytes(s.w as u32, s.h as u32, bytes);
        }
    }
    clear_background(c([8, 10, 16]));
    let tex = app.tex.as_ref().unwrap();
    draw_texture_ex(
        tex,
        (-v.x * v.z).round(),
        (-v.y * v.z).round(),
        macroquad::color::WHITE,
        DrawTextureParams { dest_size: Some(vec2(s.w as f32 * v.z, s.h as f32 * v.z)), ..Default::default() },
    );
    // night and impact flash over the map
    draw_rectangle(0.0, 0.0, mw, mh, Color::from_rgba(8, 12, 50, (135.0 * s.dark) as u8));
    if s.flash > 0.05 {
        draw_rectangle(0.0, 0.0, mw, mh, Color::from_rgba(255, 245, 210, (s.flash * 95.0).min(130.0) as u8));
    }
    // status labels (on a translucent backing so they stay readable over snow and clouds)
    let lines_h = if app.sel_snap.is_some() { 104.0 } else { 40.0 };
    draw_rectangle(8.0, 6.0, 560.0f32.min(mw - 16.0), lines_h, Color::from_rgba(10, 12, 20, 150));
    if let Some(b) = &s.banner {
        txt(&b.0, 16.0, 42.0, FB, b.2);
    }
    if let Some(cr) = app.sel_snap.as_ref() {
        let temp = s.atmosphere.temperature(&s.tmp, s.d_t, s.w, (cr.y as usize).min(s.h - 1) * s.w + (cr.x as usize).min(s.w - 1)) * 60.0 - 20.0;
        let status = cr.sick.map_or("healthy".to_string(), |(st, _)| s.strains[st as usize].name.clone());
        txt(&format!("{} | {:.1} C | {} | corpse digestion {:.0}%", cr.nm, temp, status, cr.g.v[SCAVENGE] * 100.0), 16.0, 62.0, FS, WHITE_C);
        txt(&format!("{} | {} | oxygen {:.0}%", stage(&cr.g), habitat(&cr.g), cr.breath * 100.0), 16.0, 77.0, FS, WHITE_C);
        txt(&format!("immune {:.0}% | burial {:.0}% | wings {:.0}% | fins {:.0}%", cr.g.v[IMMUNE] * 100.0, cr.g.v[BURIAL] * 100.0, cr.g.v[WINGS] * 100.0, cr.g.v[FINS] * 100.0), 16.0, 92.0, FS, DIM);
    }
    txt(
        &format!("{} | regional {:.1} C | sick {} | corpses {} | poop {}", s.wx.kind.name(), (0.5 + s.d_t) * 60.0 - 20.0, s.nsick, s.cor.len(), s.poop.len()),
        16.0, 10.0, FS, WHITE_C,
    );
    txt(
        &format!("clouds {} | drops {} | bones {} | plant lineages {}", s.atmosphere.clouds.len(), s.atmosphere.drops.len(), s.bones.len(), s.vegetation.lineages.len()),
        16.0, 25.0, FS, DIM,
    );
    // scale bar
    let cm_px = CELL_CM / v.z;
    let mut l = 1.0;
    for cand in [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 20000.0] {
        l = cand;
        if cand / cm_px >= 80.0 {
            break;
        }
    }
    let wpx = l / cm_px;
    let (bx, by) = (16.0, mh - 22.0);
    for (off, col) in [(1.0, [0, 0, 0]), (0.0, [240, 240, 240])] {
        draw_line(bx + off, by + off, bx + wpx + off, by + off, 2.0, c(col));
        draw_line(bx + off, by - 4.0 + off, bx + off, by + 4.0 + off, 2.0, c(col));
        draw_line(bx + wpx + off, by - 4.0 + off, bx + wpx + off, by + 4.0 + off, 2.0, c(col));
        let lab = if l < 100.0 { format!("{} cm", l) } else { format!("{} m", l / 100.0) };
        txt(&lab, bx + off, by - 18.0 + off, FS, col);
    }
    // ---- side panel
    let px = mw;
    draw_rectangle(px, 0.0, PANEL, mh, c(BG));
    draw_line(px, 0.0, px, mh, 1.0, c([50, 56, 76]));
    let (x, w) = (px + 10.0, PANEL - 20.0);
    let seas = ["spring", "summer", "autumn", "winter"][(4 * (s.t % SEASON) / SEASON) as usize];
    txt("EVOSIM", x, 5.0, FB, [120, 220, 160]);
    txt(
        &format!("day {}  {}  {}  {}{}", s.t / DAY + 1, seas, if s.dark > 0.5 { "night" } else { "day" }, speed_label(app.si), if app.paused { "  PAUSED" } else { "" }),
        x + 82.0, 9.0, FS, DIM,
    );
    let mg = s.crs.iter().map(|c| c.gen).max().unwrap_or(0);
    txt(&format!("pop {}   species {} alive / {} total   max gen {}", s.crs.len(), s.alive_species(), s.species.len(), mg), x, 27.0, FS, FG);
    let h = s.hist.back().copied().unwrap_or([0; 5]);
    txt(&format!("plants {}   herb {}   omni {}   carn {}", s.nplants, h[0], h[2], h[1]), x, 41.0, FS, FG);
    txt(&format!("world {}x{} cells = {:.1} x {:.1} m", s.w, s.h, s.w as f32 * CELL_CM / 100.0, s.h as f32 * CELL_CM / 100.0), x, 55.0, FS - 1.0, [90, 98, 118]);
    draw_graph(x, 70.0, w, 40.0, &s.hist);
    txt("PHYLOGENY  (click to expand)", x, 114.0, FS, DIM);
    draw_mini_tree(s, x, 128.0, w, 168.0);
    let dead = app.sel.map_or(false, |id| !s.idmap.contains_key(&id));
    draw_inspector(s, app.sel_snap.as_ref(), dead, x, 302.0, w);
    let ly = mh - 7.0 * 13.0 - 22.0;
    draw_line(x, ly - 4.0, x + w, ly - 4.0, 1.0, c([50, 56, 76]));
    let n = s.log.len();
    for (i, (t, m, col)) in s.log.iter().skip(n.saturating_sub(7)).enumerate() {
        txt(&format!("{:>7} {}", t, m), x, ly + i as f32 * 13.0, FS - 2.0, *col);
    }
}
const WHITE_C: Col = [245, 245, 245];

fn speed_footer(app: &App) {
    let tps = f64::from_bits(app.sh.tps.load(Ordering::Relaxed));
    let dt = app.sh.dt.load(Ordering::Relaxed);
    let actual = tps / REALTIME_TPS;
    let target = speed_label(app.si);
    let msg = if app.paused {
        "PAUSED | F5 save | F9 load".to_string()
    } else {
        format!("speed {} -> actual x{:.0} ({:.0} ticks/s, dt {}) | F5/F9", target, actual, tps, dt)
    };
    txt(&msg, mapw() + 10.0, maph() - 17.0, FS - 1.0, if !app.paused && !SPEEDS[app.si].is_infinite() && actual < SPEEDS[app.si] * 0.8 { [240, 190, 90] } else { DIM });
}

// ------------------------------------------------------------------------------- welcome
async fn welcome(mut index: usize) -> Option<(&'static str, usize)> {
    let mut selected = 0usize;
    let buttons = [("primordial", "NEW PRIMORDIAL WORLD"), ("resume", "CONTINUE AUTOSAVE"), ("load", "LOAD QUICKSAVE"), ("animals", "ANIMAL SANDBOX")];
    loop {
        let (sw, _) = (screen_width(), screen_height());
        let enabled = [true, autosave_path().exists(), quicksave_path().exists(), true];
        let rects: Vec<Rect> = (0..4).map(|j| Rect::new(sw / 2.0 - 210.0, 242.0 + j as f32 * 57.0, 420.0, 44.0)).collect();
        if is_key_pressed(KeyCode::Escape) || is_quit_requested() {
            return None;
        }
        if is_key_pressed(KeyCode::Left) {
            index = (index + WORLDS.len() - 1) % WORLDS.len();
        }
        if is_key_pressed(KeyCode::Right) {
            index = (index + 1) % WORLDS.len();
        }
        if is_key_pressed(KeyCode::Up) {
            selected = (selected + 3) % 4;
        }
        if is_key_pressed(KeyCode::Down) {
            selected = (selected + 1) % 4;
        }
        if (is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter)) && enabled[selected] {
            return Some((buttons[selected].0, index));
        }
        if is_key_pressed(KeyCode::P) {
            return Some(("primordial", index));
        }
        if is_key_pressed(KeyCode::C) && enabled[1] {
            return Some(("resume", index));
        }
        if is_key_pressed(KeyCode::L) && enabled[2] {
            return Some(("load", index));
        }
        let mp = vec2(mouse_position().0, mouse_position().1);
        if is_mouse_button_pressed(MouseButton::Left) {
            for (j, r) in rects.iter().enumerate() {
                if r.contains(mp) && enabled[j] {
                    return Some((buttons[j].0, index));
                }
            }
        }
        clear_background(c(BG));
        let title = "EVOSIM";
        let ts = 84.0;
        txt(title, sw / 2.0 - tw(title, ts) / 2.0, 70.0, ts, [120, 220, 160]);
        let line = "From microbial colonies to branching ecosystems";
        txt(line, sw / 2.0 - tw(line, FS) / 2.0, 164.0, FS, DIM);
        let (nm, w, h) = WORLDS[index];
        let line = format!("<  {}  {}x{} cells  >    LEFT / RIGHT: world size", nm, w, h);
        txt(&line, sw / 2.0 - tw(&line, FS) / 2.0, 205.0, FS, DIM);
        for (j, (_, label)) in buttons.iter().enumerate() {
            let r = rects[j];
            let active = enabled[j] && (r.contains(mp) || selected == j);
            draw_rectangle(r.x, r.y, r.w, r.h, c(if active { [39, 68, 49] } else { [24, 29, 38] }));
            draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, c(if active { [98, 168, 119] } else { [48, 55, 67] }));
            txt(label, r.x + 18.0, r.y + 12.0, FB, if enabled[j] { [153, 229, 170] } else { [62, 68, 77] });
        }
        for (j, line) in [
            "Primordial: all animals descend from the starting colonies.",
            "Traits spread through inheritance, survival and reproduction.",
            "Speeds x1..x5000 and MAX (1 tick = 0.2 s). F5 save / F9 load / autosave every 60 s",
            "ENTER choose / arrows select / ESC quit",
        ]
        .iter()
        .enumerate()
        {
            txt(line, sw / 2.0 - tw(line, FS) / 2.0, 500.0 + j as f32 * 23.0, FS, DIM);
        }
        next_frame().await;
    }
}

// ------------------------------------------------------------------------------- main loop
fn seed() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1)
}

fn world_index(name: &str) -> Option<usize> {
    WORLDS.iter().position(|w| w.0 == name)
}

async fn amain() {
    prevent_quit();
    let args: Vec<String> = std::env::args().collect();
    let mut windex = world_index("giant").unwrap();
    let mut custom: Option<(usize, usize)> = None;
    if let Some(p) = args.iter().position(|a| a == "--world") {
        if let Some(a) = args.get(p + 1) {
            if let Some(i) = world_index(&a.to_lowercase()) {
                windex = i;
            } else if let Some((w, h)) = a.split_once('x') {
                custom = Some((w.parse::<usize>().unwrap_or(480).clamp(120, 4096), h.parse::<usize>().unwrap_or(320).clamp(120, 4096)));
            }
        }
    }
    let flags = ["--primordial", "--animals", "--world", "--load", "--new"];
    let mut choice = None;
    if !args.iter().any(|a| flags.contains(&a.as_str())) {
        match welcome(windex).await {
            None => return,
            Some((c, i)) => {
                choice = Some(c);
                windex = i;
            }
        }
    }
    let mut load_path = match choice {
        Some("resume") => Some(autosave_path()),
        Some("load") => Some(quicksave_path()),
        _ => None,
    };
    if let Some(p) = args.iter().position(|a| a == "--load") {
        load_path = args.get(p + 1).map(std::path::PathBuf::from);
    }
    let primordial = choice.map_or(!args.iter().any(|a| a == "--animals"), |c| c != "animals");
    let (ww, wh) = custom.unwrap_or((WORLDS[windex].1, WORLDS[windex].2));
    let mut load_err = None;
    let mut snap = None;
    if let Some(p) = &load_path {
        match read_snapshot(p) {
            Ok(s) => snap = Some(s),
            Err(e) => load_err = Some(e),
        }
    }
    let (mut sim, view, si, paused) = match snap {
        Some(s) => (s.sim, Some(s.view), s.speed_index.min(SPEEDS.len() - 1), s.paused),
        None => (Sim::new(ww, wh, primordial, seed()), None, 2, false),
    };
    if let Some(n) = args.iter().position(|a| a == "--warmup").and_then(|p| args.get(p + 1)).and_then(|v| v.parse::<i64>().ok()) {
        let end = sim.t + n;
        while sim.t < end {
            sim.step(8);
        }
    }
    if let Some(e) = load_err {
        sim.logmsg(format!("Save could not load: {e}"), [255, 150, 100]);
    }
    let sh = Arc::new(Shared {
        sim: Mutex::new(sim),
        target: AtomicU64::new(0f64.to_bits()),
        paused: AtomicBool::new(true),
        quit: AtomicBool::new(false),
        tps: AtomicU64::new(0f64.to_bits()),
        dt: AtomicU64::new(1),
    });
    let th = {
        let sh = sh.clone();
        std::thread::Builder::new().name("evosim-sim".into()).spawn(move || sim_thread(sh)).unwrap()
    };
    let mut app = App {
        sh: sh.clone(),
        view: View { z: 1.0, x: 0.0, y: 0.0 },
        drag: None,
        si,
        paused,
        sel: None,
        sel_snap: None,
        tv: TreeView::new(),
        rend: WorldRenderer::new(),
        tex: None,
        wname: WORLDS[windex].0.to_string(),
        last_save: Instant::now(),
        saving: Arc::new(AtomicBool::new(false)),
        mouse_d: Vec2::ZERO,
    };
    {
        let s = sh.sim.lock();
        match view {
            Some(v) => {
                app.view = v;
                app.clamp_view(&s);
            }
            None => app.new_view(&s),
        }
    }
    if let Some(p) = args.iter().position(|a| a == "--speed") {
        app.si = args.get(p + 1).and_then(|v| v.parse::<usize>().ok()).unwrap_or(2).min(SPEEDS.len() - 1);
    }
    if let Some(p) = args.iter().position(|a| a == "--view") {
        match args.get(p + 1).map(|s| s.as_str()) {
            Some("tree") => app.tv.fs = true,
            Some("plants") => {
                app.tv.switch(3);
            }
            _ => {}
        }
    }
    // --shot FILE FRAMES: render, save a screenshot after FRAMES frames and exit (testing aid)
    let shot = args.iter().position(|a| a == "--shot").map(|p| {
        (args.get(p + 1).cloned().unwrap_or("evosim.png".into()), args.get(p + 2).and_then(|v| v.parse::<u32>().ok()).unwrap_or(120))
    });
    let mut frame_no = 0u32;
    app.set_speed();
    let mut last_frame = Instant::now();
    let mut last_mouse = vec2(mouse_position().0, mouse_position().1);
    loop {
        let mpos = vec2(mouse_position().0, mouse_position().1);
        app.mouse_d = mpos - last_mouse;
        last_mouse = mpos;
        // ---- input
        let mut quit = is_quit_requested();
        let fast = !app.paused && SPEEDS[app.si] >= 500.0;
        {
            let mut s = sh.sim.lock();
            if is_key_pressed(KeyCode::F5) {
                app.save_async(&s, quicksave_path());
                s.logmsg("Saved: quicksave.evo2".into(), [120, 220, 160]);
            }
            if is_key_pressed(KeyCode::F9) {
                let path = if quicksave_path().exists() { quicksave_path() } else { autosave_path() };
                match read_snapshot(&path) {
                    Ok(snap) => {
                        *s = snap.sim;
                        app.view = snap.view;
                        app.si = snap.speed_index.min(SPEEDS.len() - 1);
                        app.paused = snap.paused;
                        app.tv = TreeView::new();
                        app.rend.reset();
                        app.sel = None;
                        app.sel_snap = None;
                        app.clamp_view(&s);
                        let name = path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
                        s.logmsg(format!("Loaded: {name}"), [120, 220, 160]);
                    }
                    Err(e) => s.logmsg(format!("Load failed: {e}"), [255, 150, 100]),
                }
            }
            for (k, key) in [KeyCode::Key1, KeyCode::Key2, KeyCode::Key3, KeyCode::Key4, KeyCode::Key5, KeyCode::Key6, KeyCode::Key7, KeyCode::Key8, KeyCode::Key9].iter().enumerate() {
                if is_key_pressed(*key) {
                    app.si = k;
                }
            }
            if is_key_pressed(KeyCode::RightBracket) {
                app.si = (app.si + 1).min(SPEEDS.len() - 1);
            }
            if is_key_pressed(KeyCode::LeftBracket) {
                app.si = app.si.saturating_sub(1);
            }
            if is_key_pressed(KeyCode::Space) {
                app.paused = !app.paused;
            }
            if app.tv.fs {
                if is_key_pressed(KeyCode::Escape) || is_key_pressed(KeyCode::T) {
                    app.tv.fs = false;
                } else {
                    tree_input(&mut app, &s);
                }
            } else {
                if is_key_pressed(KeyCode::M) {
                    app.si = SPEEDS.len() - 1;
                }
                let mp = vec2(mouse_position().0, mouse_position().1);
                let (mw, mh) = (mapw(), maph());
                if is_mouse_button_pressed(MouseButton::Left) && mp.x >= mw + 10.0 && mp.x <= mw + PANEL - 10.0 && mp.y >= 128.0 && mp.y < 296.0 {
                    app.tv.fs = true;
                } else {
                    if is_key_pressed(KeyCode::Escape) {
                        quit = true;
                    }
                    if is_key_pressed(KeyCode::T) {
                        app.tv.fs = true;
                    }
                    if is_key_pressed(KeyCode::R) || is_key_pressed(KeyCode::N) {
                        let (w, h) = if is_key_pressed(KeyCode::N) {
                            windex = (world_index(&app.wname).map_or(0, |i| i + 1)) % WORLDS.len();
                            app.wname = WORLDS[windex].0.to_string();
                            (WORLDS[windex].1, WORLDS[windex].2)
                        } else {
                            (s.w, s.h)
                        };
                        let prim = s.primordial;
                        *s = Sim::new(w, h, prim, seed());
                        app.tv = TreeView::new();
                        app.rend.reset();
                        app.sel = None;
                        app.sel_snap = None;
                        app.new_view(&s);
                    }
                    if is_key_pressed(KeyCode::S) {
                        get_screen_data().export_png("evosim.png");
                        s.logmsg("Screenshot: evosim.png".into(), [120, 220, 160]);
                    }
                    if is_key_pressed(KeyCode::Equal) || is_key_pressed(KeyCode::KpAdd) {
                        app.zoom_at(&s, mw / 2.0, mh / 2.0, 1.25);
                    }
                    if is_key_pressed(KeyCode::Minus) || is_key_pressed(KeyCode::KpSubtract) {
                        app.zoom_at(&s, mw / 2.0, mh / 2.0, 1.0 / 1.25);
                    }
                    if is_key_pressed(KeyCode::Key0) {
                        app.new_view(&s);
                    }
                    if (is_key_pressed(KeyCode::H) || is_key_pressed(KeyCode::C)) && !s.primordial && mp.x < mw {
                        let (wx, wy) = app.to_world(mp.x, mp.y);
                        s.place_founders(wx, wy, is_key_pressed(KeyCode::C));
                    }
                    let wheel = mouse_wheel().1;
                    if wheel != 0.0 && mp.x < mw {
                        app.zoom_at(&s, mp.x, mp.y, 1.2f32.powf(wheel.signum()));
                    }
                    if mp.x < mw {
                        if is_mouse_button_pressed(MouseButton::Left) {
                            app.drag = Some((mp, false));
                        }
                        if is_mouse_button_pressed(MouseButton::Right) {
                            let (wx, wy) = app.to_world(mp.x, mp.y);
                            s.sow(wx, wy);
                        }
                    }
                    if let Some((start, moved)) = app.drag {
                        if is_mouse_button_down(MouseButton::Left) {
                            if moved || (mp - start).length() > 5.0 {
                                app.drag = Some((start, true));
                                app.view.x -= app.mouse_d.x / app.view.z;
                                app.view.y -= app.mouse_d.y / app.view.z;
                                app.clamp_view(&s);
                            }
                        }
                        if is_mouse_button_released(MouseButton::Left) {
                            if !moved {
                                let (wx, wy) = app.to_world(mp.x, mp.y);
                                let mut bd = (14.0 / app.view.z).max(3.0).powi(2);
                                let mut best = None;
                                for c in &s.crs {
                                    let d = (c.x - wx).powi(2) + (c.y - wy).powi(2);
                                    if d < bd {
                                        bd = d;
                                        best = Some(c.id);
                                    }
                                }
                                app.sel = best;
                                app.sel_snap = None;
                            }
                            app.drag = None;
                        }
                    }
                    let (l, r, u, d) = (is_key_down(KeyCode::Left), is_key_down(KeyCode::Right), is_key_down(KeyCode::Up), is_key_down(KeyCode::Down));
                    if l || r || u || d {
                        app.view.x += (r as i32 - l as i32) as f32 * 9.0 / app.view.z;
                        app.view.y += (d as i32 - u as i32) as f32 * 9.0 / app.view.z;
                        app.clamp_view(&s);
                    }
                }
            }
            app.set_speed();
            if app.last_save.elapsed().as_secs_f32() >= AUTOSAVE_SECS {
                app.save_async(&s, autosave_path());
                app.last_save = Instant::now();
            }
            // ---- draw (holding the lock: the world is a consistent snapshot)
            if let Some(id) = app.sel {
                if let Some(&j) = s.idmap.get(&id) {
                    app.sel_snap = Some(s.crs[j].clone());
                }
            }
            let render_now = !fast || last_frame.elapsed().as_secs_f32() >= 1.0 / 20.0;
            if render_now {
                last_frame = Instant::now();
            }
            {
                if app.tv.fs {
                    if app.tv.kind == 3 {
                        draw_plant_phylogeny(&s, &mut app.tv, &mut app.rend);
                    } else {
                        draw_tree_fs(&s, &mut app.tv);
                    }
                    draw_tabs(&mut app.tv);
                } else {
                    draw_main(&mut app, &s, render_now);
                    speed_footer(&app);
                }
            }
            frame_no += 1;
            if let Some((file, n)) = &shot {
                if frame_no == *n {
                    get_screen_data().export_png(file);
                    quit = true;
                }
                if frame_no == *n / 2 && app.sel.is_none() {
                    app.sel = s.crs.iter().find(|c| c.g.v[COMPLEXITY] >= 0.42).or(s.crs.first()).map(|c| c.id);
                    if let Some(z) = args.iter().position(|a| a == "--zoom").and_then(|p| args.get(p + 1)).and_then(|v| v.parse::<f32>().ok()) {
                        // centre on the densest stand of trees if any, else the selected creature
                        let target = s.vegetation.trees.iter().next().map(|&i| ((i as usize % s.w) as f32, (i as usize / s.w) as f32))
                            .or_else(|| app.sel.and_then(|id| s.idmap.get(&id)).map(|&j| (s.crs[j].x, s.crs[j].y)));
                        if let Some((x, y)) = target {
                            app.view.z = z;
                            app.view.x = x - mapw() / z / 2.0;
                            app.view.y = y - maph() / z / 2.0;
                            app.clamp_view(&s);
                        }
                    }
                }
            }
            if quit {
                // final autosave (synchronous)
                let snap = app.snapshot(&s);
                drop(s);
                sh.quit.store(true, Ordering::Relaxed);
                while app.saving.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(20));
                }
                if let Err(e) = write_snapshot(&autosave_path(), &snap) {
                    eprintln!("[evosim] exit autosave failed: {e}");
                }
                break;
            }
        }
        next_frame().await;
    }
    sh.quit.store(true, Ordering::Relaxed);
    let _ = th.join();
}

fn conf() -> Conf {
    Conf {
        window_title: "EVOSIM".to_owned(),
        window_width: 1420,
        window_height: 760,
        window_resizable: true,
        high_dpi: false,
        ..Default::default()
    }
}

fn main() {
    macroquad::Window::from_config(conf(), amain());
}
