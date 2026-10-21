use rand::Rng as _;
use rand_distr::{Distribution, StandardNormal};

pub type Rng = rand_xoshiro::Xoshiro256PlusPlus;
pub type Col = [u8; 3];

#[inline]
pub fn gauss(r: &mut Rng, mu: f32, sigma: f32) -> f32 {
    let z: f32 = StandardNormal.sample(r);
    mu + sigma * z
}

#[inline]
pub fn chance(r: &mut Rng, p: f32) -> bool {
    r.gen::<f32>() < p
}

/// Per-tick probability scaled to a coarse time step: 1-(1-p)^dt.
#[inline]
pub fn pdt(p: f32, dt: i32) -> f32 {
    if dt == 1 {
        p
    } else {
        1.0 - (1.0 - p.clamp(0.0, 1.0)).powi(dt)
    }
}

/// pygame-compatible HSV (h 0..360, s/v 0..100).
pub fn hsv(h: f32, s: f32, v: f32) -> Col {
    let h = h.rem_euclid(360.0);
    let s = (s / 100.0).clamp(0.0, 1.0);
    let v = (v / 100.0).clamp(0.0, 1.0);
    let c = v * s;
    let hp = h / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [((r + m) * 255.0) as u8, ((g + m) * 255.0) as u8, ((b + m) * 255.0) as u8]
}

pub fn mix(a: Col, b: Col, t: f32) -> Col {
    [
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t) as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t) as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t) as u8,
    ]
}

pub fn scale(c: Col, m: f32) -> Col {
    [(c[0] as f32 * m) as u8, (c[1] as f32 * m) as u8, (c[2] as f32 * m) as u8]
}

const SYL: [&str; 18] = ["ka", "zo", "mi", "ru", "ta", "ve", "lo", "an", "ix", "po", "su", "dre", "gal", "nem", "quo", "fen", "bri", "yu"];

pub fn name(r: &mut Rng, n: usize) -> String {
    let mut s = String::new();
    for _ in 0..n {
        s.push_str(SYL[r.gen_range(0..SYL.len())]);
    }
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => s,
    }
}

/// Smooth value-noise fractal (fbm), one value per cell.
pub fn fbm(r: &mut Rng, w: usize, h: usize, scales: &[usize], weights: &[f32]) -> Vec<f32> {
    let mut out = vec![0f32; w * h];
    for (&sc, &wt) in scales.iter().zip(weights) {
        let gw = w / sc + 2;
        let gh = h / sc + 2;
        let g: Vec<f32> = (0..gw * gh).map(|_| r.gen::<f32>()).collect();
        for y in 0..h {
            let fy = y as f32 / sc as f32;
            let y0 = fy as usize;
            let mut ty = fy - y0 as f32;
            ty = ty * ty * (3.0 - 2.0 * ty);
            for x in 0..w {
                let fx = x as f32 / sc as f32;
                let x0 = fx as usize;
                let mut tx = fx - x0 as f32;
                tx = tx * tx * (3.0 - 2.0 * tx);
                let r0 = y0 * gw;
                let r1 = (y0 + 1) * gw;
                let a = g[r0 + x0] + (g[r0 + x0 + 1] - g[r0 + x0]) * tx;
                let b = g[r1 + x0] + (g[r1 + x0 + 1] - g[r1 + x0]) * tx;
                out[y * w + x] += (a + (b - a) * ty) * wt;
            }
        }
    }
    out
}
