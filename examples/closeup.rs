//! Runs a world headless and renders close-ups around the biggest animal and a predator:
//! cargo run --release --example closeup -- <ticks> <seed> <out-prefix>
use evosim::genome::*;
use evosim::render::*;
use evosim::sim::*;

fn shoot(r: &mut WorldRenderer, s: &Sim, cx: i32, cy: i32, path: &str) {
    let (cw, ch, k) = (200i32, 120i32, 5i32);
    let (x0, y0) = ((cx - cw / 2).clamp(0, s.w as i32 - cw), (cy - ch / 2).clamp(0, s.h as i32 - ch));
    r.compose(s, (x0, y0, x0 + cw, y0 + ch), None, MapMode::Terrain);
    let mut out = macroquad::texture::Image::gen_image_color((cw * k) as u16, (ch * k) as u16, macroquad::color::BLACK);
    for y in 0..ch * k { for x in 0..cw * k {
        let p = r.px[((y0 + y / k) as usize) * s.w + (x0 + x / k) as usize];
        out.set_pixel(x as u32, (ch * k - 1 - y) as u32, macroquad::color::Color::from_rgba(p[0], p[1], p[2], 255));
    }}
    out.export_png(path);
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let ticks: i64 = a.get(1).and_then(|v| v.parse().ok()).unwrap_or(100000);
    let seed: u64 = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(5);
    let pre = a.get(3).cloned().unwrap_or("closeup".into());
    let (_, w, h) = *WORLDS.iter().find(|x| x.0 == "giant").unwrap();
    let mut s = Sim::new_cfg(w, h, false, seed, evosim::worldgen::MapType::Continents, false);
    while s.t < ticks { s.step(8); }
    for _ in 0..30 { s.step(1); }
    let mut r = WorldRenderer::new();
    let big = s.crs.iter().filter(|c| c.g.v[COMPLEXITY] >= 0.42).max_by(|a, b| a.s.partial_cmp(&b.s).unwrap()).unwrap();
    println!("biggest: size {:.1} {} at {:.0},{:.0}", big.s, body_plan(&big.g), big.x, big.y);
    shoot(&mut r, &s, big.x as i32, big.y as i32, &format!("{pre}_giant.png"));
    // the densest spot of predators
    if let Some(p) = s.crs.iter().filter(|c| c.g.v[DIET] > 0.65).max_by_key(|c| s.crs.iter().filter(|o| (o.x - c.x).abs() < 60.0 && (o.y - c.y).abs() < 40.0).count()) {
        println!("predator: size {:.1} {}", p.s, body_plan(&p.g));
        shoot(&mut r, &s, p.x as i32, p.y as i32, &format!("{pre}_hunt.png"));
    }
    let mut plans = std::collections::BTreeMap::new();
    for c in &s.crs { *plans.entry(body_plan(&c.g)).or_insert(0) += 1; }
    println!("{plans:#?}");
}
