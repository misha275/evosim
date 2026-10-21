//! Renders every plant form (and deciduous seasons) to plant_gallery.png
use evosim::render::*;
use evosim::vegetation::*;
fn main() {
    let base = |id: u32, f: &dyn Fn(&mut [f32; NPG])| {
        let mut g = [0f32; NPG];
        g[CROWN] = 0.5; g[DROOP] = 0.2; g[DENSITY] = 0.6; g[BRANCHING] = 0.5; g[GIRTH] = 0.3; g[BLOOM] = 0.1;
        g[BLOOM_HUE] = 330.0; g[HUE] = 105.0; g[MOISTURE] = 0.5; g[TEMPERATURE] = 0.5;
        f(&mut g);
        PlantLineage { id, parent: None, born: 0, generation: 0, g }
    };
    let set: Vec<(&str, PlantLineage, u8)> = vec![
        ("tree", base(1, &|g| { g[WOOD] = 0.8; g[HEIGHT] = 0.8; g[BLOOM] = 0.6 }), 0),
        ("tree autumn", base(2, &|g| { g[WOOD] = 0.8; g[HEIGHT] = 0.8; g[DECIDUOUS] = 0.9 }), 1),
        ("tree winter", base(2, &|g| { g[WOOD] = 0.8; g[HEIGHT] = 0.8; g[DECIDUOUS] = 0.9 }), 2),
        ("weeping", base(3, &|g| { g[WOOD] = 0.7; g[HEIGHT] = 0.7; g[DROOP] = 0.9; g[HUE] = 85.0 }), 0),
        ("toxic+thorny", base(4, &|g| { g[WOOD] = 0.7; g[HEIGHT] = 0.65; g[TOXIN] = 0.8; g[THORNS] = 0.8 }), 0),
        ("conifer", base(5, &|g| { g[WOOD] = 0.8; g[HEIGHT] = 0.9; g[CROWN] = 0.2; g[DECIDUOUS] = 0.1 }), 0),
        ("palm", base(6, &|g| { g[WOOD] = 0.7; g[HEIGHT] = 0.85; g[CROWN] = 0.9; g[BRANCHING] = 0.1; g[DROOP] = 0.6; g[BLOOM] = 0.6 }), 0),
        ("shrub", base(7, &|g| { g[WOOD] = 0.45; g[HEIGHT] = 0.4; g[BLOOM] = 0.7; g[BLOOM_HUE] = 50.0 }), 0),
        ("cactus", base(8, &|g| { g[SUCCULENCE] = 0.8; g[HEIGHT] = 0.6; g[THORNS] = 0.6; g[BLOOM] = 0.5 }), 0),
        ("vine", base(9, &|g| { g[CLIMBING] = 0.8; g[HEIGHT] = 0.6; g[BLOOM] = 0.6; g[BLOOM_HUE] = 280.0 }), 0),
        ("flytrap", base(10, &|g| { g[CARNIVORY] = 0.8; g[HEIGHT] = 0.2 }), 0),
        ("flower", base(11, &|g| { g[BLOOM] = 0.8; g[HEIGHT] = 0.3; g[BLOOM_HUE] = 10.0 }), 0),
        ("reed", base(12, &|g| { g[AQUATIC] = 0.55; g[HEIGHT] = 0.6 }), 0),
        ("kelp", base(13, &|g| { g[AQUATIC] = 0.9; g[HEIGHT] = 0.7; g[HUE] = 150.0 }), 0),
    ];
    let (cell_w, cell_h, k) = (40, 46, 4);
    let (w, h) = (set.len() as i32 * cell_w, cell_h);
    let mut px = vec![[150u8, 140, 100, 255]; (w * h) as usize];
    for (n, (name, l, phase)) in set.iter().enumerate() {
        assert!(l.form().sprite(), "{name} {:?}", l.form());
        let s = plant_sprite(l, 3, *phase);
        let ox = n as i32 * cell_w + cell_w / 2 - s.ax;
        let oy = cell_h - 4 - s.ay;
        for y in 0..s.h { for x in 0..s.w {
            let p = s.px[(y * s.w + x) as usize];
            let (xx, yy) = (ox + x, oy + y);
            if p[3] > 0 && xx >= 0 && yy >= 0 && xx < w && yy < h { px[(yy * w + xx) as usize] = p; }
        }}
        println!("{:>14}: form {:?}", name, l.form());
    }
    let mut out = macroquad::texture::Image::gen_image_color((w * k) as u16, (h * k) as u16, macroquad::color::BLACK);
    for y in 0..h * k { for x in 0..w * k {
        let p = px[((y / k) * w + x / k) as usize];
        out.set_pixel(x as u32, (h * k - 1 - y) as u32, macroquad::color::Color::from_rgba(p[0], p[1], p[2], 255));
    }}
    out.export_png("plant_gallery.png");
}
