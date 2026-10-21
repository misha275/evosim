//! Renders body plans and hunting poses to critters.png:
//! row 1 - forms (small grazer, hunter, runner, long neck, plated, sail, armored club tail, horned frill, giants),
//! row 2 - poses of a hunter (idle, chase, pounce, feed, flee, swim).
use evosim::genome::*;
use evosim::render::*;
use evosim::sim::*;
use evosim::util::Rng;
use rand::SeedableRng;

fn make(r: &mut Rng, kind: Kind, hue: f32, f: &dyn Fn(&mut Genome)) -> Cr {
    let mut g = base_genome(r, kind);
    g.hue = hue;
    g.hue2 = (hue + 40.0) % 360.0;
    g.v[TAIL] = 0.5;
    g.v[EARS] = 0.4;
    g.v[HORN] = 0.0;
    g.v[ARMOR] = 0.0;
    g.v[PLATES] = 0.0;
    g.v[NECK] = 0.0;
    g.v[STANCE] = 0.0;
    g.v[FUR] = 0.2;
    g.v[WINGS] = 0.0;
    g.v[FINS] = 0.0;
    g.v[GILLS] = 0.0;
    g.v[BODY] = 0.7;
    g.pat = 0;
    f(&mut g);
    let mut c = Cr::dummy(&g, 0);
    c.vx = 0.5;
    c
}

fn main() {
    let mut r = Rng::seed_from_u64(1);
    let forms: Vec<(&str, Cr)> = vec![
        ("grazer", make(&mut r, Kind::Herb, 35.0, &|g| { g.v[SIZE] = 5.0; g.v[EARS] = 0.7; g.pat = 2 })),
        ("hunter", make(&mut r, Kind::Carn, 20.0, &|g| { g.v[SIZE] = 6.5; g.v[TAIL] = 0.7 })),
        ("runner", make(&mut r, Kind::Carn, 90.0, &|g| { g.v[SIZE] = 6.0; g.v[STANCE] = 0.9; g.v[TAIL] = 0.8 })),
        ("long neck", make(&mut r, Kind::Herb, 140.0, &|g| { g.v[SIZE] = 7.0; g.v[NECK] = 0.9 })),
        ("plated", make(&mut r, Kind::Herb, 70.0, &|g| { g.v[SIZE] = 8.0; g.v[PLATES] = 0.8; g.v[ARMOR] = 0.6; g.v[TAIL] = 0.7 })),
        ("sail", make(&mut r, Kind::Carn, 10.0, &|g| { g.v[SIZE] = 9.0; g.v[PLATES] = 0.95; g.v[SAT] = 0.9 })),
        ("club tail", make(&mut r, Kind::Herb, 45.0, &|g| { g.v[SIZE] = 11.0; g.v[ARMOR] = 0.95; g.v[TAIL] = 0.9; g.v[LUM] = 0.3; g.v[BODY] = 0.6 })),
        ("horned frill", make(&mut r, Kind::Herb, 25.0, &|g| { g.v[SIZE] = 14.0; g.v[HORN] = 1.0; g.v[ARMOR] = 0.5; g.v[TAIL] = 0.4; g.pat = 3 })),
        ("titan", make(&mut r, Kind::Herb, 100.0, &|g| { g.v[SIZE] = 24.0; g.v[NECK] = 1.0; g.v[TAIL] = 1.0; g.v[LUM] = 0.45; g.v[SAT] = 0.3; g.pat = 1 })),
        ("tyrant", make(&mut r, Kind::Carn, 5.0, &|g| { g.v[SIZE] = 20.0; g.v[STANCE] = 1.0; g.v[TAIL] = 0.9; g.v[LEGS] = 0.8; g.v[SAT] = 0.6 })),
    ];
    let cell = |c: &Cr| -> (i32, i32) { let (l, h, ll, _, _) = body_dims(c); ((l * 3 + 14).max(24), (h * 3 + ll + 16).max(22)) };
    let k = 4;
    let row1w: i32 = forms.iter().map(|(_, c)| cell(c).0).sum();
    let row1h: i32 = forms.iter().map(|(_, c)| cell(c).1).max().unwrap();
    let hunter = forms[2].1.clone();
    let (pw, ph) = (30, 22);
    let poses: [(u8, bool); 6] = [(0, false), (1, false), (2, false), (3, false), (4, false), (0, true)];
    let (w, h) = (row1w.max(pw * 6), row1h + ph);
    let mut px = vec![[118u8, 150, 84, 255]; (w * h) as usize];
    let mut x = 0;
    for (_, c) in &forms {
        let (cw, ch) = cell(c);
        let mut buf = vec![[118u8, 150, 84, 255]; (cw * ch) as usize];
        let mut cv = Canvas { w: cw, h: ch, px: &mut buf };
        draw_cr(&mut cv, c, 2, 0.0, (cw / 2) as f32, (ch * 2 / 3) as f32, false);
        for yy in 0..ch { for xx in 0..cw { px[((row1h - ch + yy) * w + x + xx) as usize] = buf[(yy * cw + xx) as usize]; } }
        x += cw;
    }
    for (n, &(act, wet)) in poses.iter().enumerate() {
        let mut c = hunter.clone();
        c.act = act;
        let mut buf = vec![if wet { [40u8, 90, 150, 255] } else { [118, 150, 84, 255] }; (pw * ph) as usize];
        let mut cv = Canvas { w: pw, h: ph, px: &mut buf };
        draw_cr(&mut cv, &c, 2, 0.0, (pw / 2) as f32, (ph * 3 / 5) as f32, wet);
        for yy in 0..ph { for xx in 0..pw { px[((row1h + yy) * w + n as i32 * pw + xx) as usize] = buf[(yy * pw + xx) as usize]; } }
    }
    let mut out = macroquad::texture::Image::gen_image_color((w * k) as u16, (h * k) as u16, macroquad::color::BLACK);
    for y in 0..h * k { for x in 0..w * k {
        let p = px[((y / k) * w + x / k) as usize];
        out.set_pixel(x as u32, (h * k - 1 - y) as u32, macroquad::color::Color::from_rgba(p[0], p[1], p[2], 255));
    }}
    out.export_png(&std::env::args().nth(1).unwrap_or("critters.png".into()));
    for (n, c) in &forms { println!("{n:>13}: {}", body_plan(&c.g)); }
}
