//! Renders herbivore / predator poses (idle, chase, pounce, feed, flee, swim) to critters.png
use evosim::render::*;
use evosim::sim::*;
fn main() {
    let mut s = Sim::new(340, 230, false, 3);
    for _ in 0..300 { s.step(1); }
    let herb = s.crs.iter().find(|c| c.g.v[evosim::genome::DIET] < 0.3 && c.age > c.g.v[evosim::genome::MAT]).cloned();
    let carn = s.crs.iter().find(|c| c.g.v[evosim::genome::DIET] > 0.6).cloned();
    let (cw, ch, k) = (24i32, 20i32, 6i32);
    let poses: [(u8, bool); 6] = [(0, false), (1, false), (2, false), (3, false), (4, false), (0, true)];
    let rows: Vec<_> = [herb, carn].into_iter().flatten().collect();
    let (w, h) = (cw * poses.len() as i32, ch * rows.len() as i32);
    let mut px = vec![[110u8, 150, 80, 255]; (w * h) as usize];
    for (r, c0) in rows.iter().enumerate() {
        for (n, &(act, wet)) in poses.iter().enumerate() {
            let mut c = c0.clone();
            c.act = act; c.vx = 0.5; c.vy = 0.0; c.f = 1; c.rest = false; c.z = 0.0;
            let mut cell = vec![if wet { [40u8, 90, 150, 255] } else { [110, 150, 80, 255] }; (cw * ch) as usize];
            let mut cv = Canvas { w: cw, h: ch, px: &mut cell };
            draw_cr(&mut cv, &c, 2, 0.0, (cw / 2) as f32, (ch / 2) as f32, wet);
            for y in 0..ch { for x in 0..cw { px[((r as i32 * ch + y) * w + n as i32 * cw + x) as usize] = cell[(y * cw + x) as usize]; } }
        }
    }
    let mut out = macroquad::texture::Image::gen_image_color((w * k) as u16, (h * k) as u16, macroquad::color::BLACK);
    for y in 0..h * k { for x in 0..w * k {
        let p = px[((y / k) * w + x / k) as usize];
        out.set_pixel(x as u32, (h * k - 1 - y) as u32, macroquad::color::Color::from_rgba(p[0], p[1], p[2], 255));
    }}
    out.export_png(&std::env::args().nth(1).unwrap_or("critters.png".into()));
}
