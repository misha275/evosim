//! Runs a world for a long time and reports how the planet changes; saves maps at the start and the end:
//! cargo run --release --example planet -- <ticks> <seed> <out-prefix>
use evosim::render::*;
use evosim::sim::*;
use evosim::worldgen::Biome;

fn save(r: &mut WorldRenderer, s: &Sim, mode: MapMode, path: &str) {
    let (w, h) = (s.w as i32, s.h as i32);
    r.compose(s, (0, 0, w, h), None, mode);
    let k = 2; // downscale
    let (ow, oh) = (w / k, h / k);
    let mut out = macroquad::texture::Image::gen_image_color(ow as u16, oh as u16, macroquad::color::BLACK);
    for y in 0..oh { for x in 0..ow {
        let p = r.px[((y * k) * w + x * k) as usize];
        out.set_pixel(x as u32, (oh - 1 - y) as u32, macroquad::color::Color::from_rgba(p[0], p[1], p[2], 255));
    }}
    out.export_png(path);
}

fn stats(s: &Sim) -> String {
    let land = (0..s.n).filter(|&i| s.terr[i] != 1).count();
    let mtn = (0..s.n).filter(|&i| s.biome[i] == Biome::Mountain as u8 || s.biome[i] == Biome::Glacier as u8).count();
    let high = (0..s.n).filter(|&i| s.elevation[i] > 160).count();
    let erupting = s.volcanoes.iter().filter(|v| v.erupt > 0).count();
    format!("t {:>7} land {} mountains {:.1}% of land (>160: {}) volcanoes {} (erupting {}) lava {} basalt {} pools {} dust {:.2} pop {}",
        s.t, land, 100.0 * mtn as f32 / land.max(1) as f32, high, s.volcanoes.len(), erupting, s.lava.len(), s.basalt.len(), s.pools.len(), s.dust, s.crs.len())
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let ticks: i64 = a.get(1).and_then(|v| v.parse().ok()).unwrap_or(200000);
    let seed: u64 = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(5);
    let pre = a.get(3).cloned().unwrap_or("planet".into());
    let (_, w, h) = *WORLDS.iter().find(|x| x.0 == "giant").unwrap();
    let mut s = Sim::new_cfg(w, h, false, seed, evosim::worldgen::MapType::Continents, false);
    let mut r = WorldRenderer::new();
    save(&mut r, &s, MapMode::Terrain, &format!("{pre}_start.png"));
    save(&mut r, &s, MapMode::Tectonics, &format!("{pre}_plates.png"));
    println!("{}", stats(&s));
    let t0 = std::time::Instant::now();
    let mut next = 20000;
    let mut seen = 0;
    while s.t < ticks {
        s.step(8);
        if s.t >= next {
            next += 20000;
            println!("{}", stats(&s));
        }
        if seen == 0 && s.lava.len() > 150 {
            if let Some(v) = s.volcanoes.iter().find(|v| v.erupt > 0) {
                seen = 1;
                let (cw, ch, k) = (160i32, 100i32, 5i32);
                let (x0, y0) = ((v.x - cw / 2).clamp(0, s.w as i32 - cw), (v.y - ch / 2).clamp(0, s.h as i32 - ch));
                r.compose(&s, (x0, y0, x0 + cw, y0 + ch), None, MapMode::Terrain);
                let mut out = macroquad::texture::Image::gen_image_color((cw * k) as u16, (ch * k) as u16, macroquad::color::BLACK);
                for y in 0..ch * k { for x in 0..cw * k {
                    let p = r.px[((y0 + y / k) as usize) * s.w + (x0 + x / k) as usize];
                    out.set_pixel(x as u32, (ch * k - 1 - y) as u32, macroquad::color::Color::from_rgba(p[0], p[1], p[2], 255));
                }}
                out.export_png(&format!("{pre}_eruption.png"));
                println!("eruption close-up at t {} ({})", s.t, v.name);
            }
        }
    }
    for (t, m, _) in s.log.iter() {
        if m.contains("erupt") || m.contains("Flood") || m.contains("volcano") || m.contains("Earthquake") {
            println!("  log {t}: {m}");
        }
    }
    println!("{:.0} ticks/s", s.t as f64 / t0.elapsed().as_secs_f64());
    save(&mut r, &s, MapMode::Terrain, &format!("{pre}_end.png"));
    save(&mut r, &s, MapMode::Tectonics, &format!("{pre}_plates_end.png"));
}
