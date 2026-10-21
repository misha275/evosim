//! cargo run --release --example terrain -- <w> <h> <seed> [biome]  -> terrain.png
use evosim::util::Rng;
use evosim::worldgen::*;
use rand::SeedableRng;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let w: usize = a.get(1).and_then(|v| v.parse().ok()).unwrap_or(1600);
    let h: usize = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(1060);
    let seed: u64 = a.get(3).and_then(|v| v.parse().ok()).unwrap_or(1);
    let mode = a.get(4).cloned().unwrap_or_default();
    let t = std::time::Instant::now();
    let map = MapType::ALL.iter().copied().find(|m| Some(m.name()) == a.get(5).map(|s| s.as_str())).unwrap_or(MapType::Continents);
    let wd = generate(w, h, map, &mut Rng::seed_from_u64(seed));
    println!("generated {}x{} in {:.2}s, land {:.0}%, springs {}", w, h, t.elapsed().as_secs_f32(), wd.land as f32 * 100.0 / (w * h) as f32, wd.springs.len());
    let mut counts = [0usize; NBIOMES];
    for &b in &wd.biome { counts[b as usize] += 1; }
    for b in 0..NBIOMES { println!("{:>18} {:5.1}%", Biome::from_u8(b as u8).name(), counts[b] as f32 * 100.0 / (w * h) as f32); }
    let mut ml: Vec<u8> = (0..w*h).filter(|&i| wd.terr[i] != 1).map(|i| wd.moisture[i]).collect(); ml.sort();
    let mut tl: Vec<u8> = (0..w*h).filter(|&i| wd.terr[i] != 1).map(|i| wd.tmp[i]).collect(); tl.sort();
    let q = |v: &Vec<u8>, f: f32| v[((v.len()-1) as f32 * f) as usize];
    println!("land moist q10 {} q50 {} q90 {} | temp q10 {} q50 {} q90 {}", q(&ml,0.1), q(&ml,0.5), q(&ml,0.9), q(&tl,0.1), q(&tl,0.5), q(&tl,0.9));
    let land: Vec<usize> = (0..w*h).filter(|&i| wd.terr[i] != 1).collect();
    let mtn = land.iter().filter(|&&i| evosim::render::metres(wd.elevation[i]) > 900.0).count();
    let mtn_b = land.iter().filter(|&&i| matches!(Biome::from_u8(wd.biome[i]), Biome::Mountain | Biome::Alpine) || (Biome::from_u8(wd.biome[i]) == Biome::Glacier && evosim::render::metres(wd.elevation[i]) > 900.0)).count();
    println!("land above 900 m: {:.1}% | mountain/alpine/glacier biomes: {:.1}% of land", mtn as f32 * 100.0 / land.len() as f32, mtn_b as f32 * 100.0 / land.len() as f32);
    let mut img = macroquad::texture::Image::gen_image_color(w as u16, h as u16, macroquad::color::BLACK);
    for y in 0..h { for x in 0..w {
        let i = y * w + x;
        let c = match mode.as_str() {
            "biome" => Biome::from_u8(wd.biome[i]).map_col(),
            "temp" => { let t = wd.tmp[i] as f32 / 255.0; [(255.0 * t) as u8, 60, (255.0 * (1.0 - t)) as u8] }
            "moist" => { let m = wd.moisture[i]; [255 - m, 200, m] }
            _ => wd.bg[i],
        };
        img.set_pixel(x as u32, (h - 1 - y) as u32, macroquad::color::Color::from_rgba(c[0], c[1], c[2], 255));
    }}
    img.export_png("terrain.png");
}
#[allow(dead_code)]
fn _unused() {}
