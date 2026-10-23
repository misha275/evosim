//! Loads a save and keeps simulating it (finding crashes): cargo run --example replay -- <save> <steps> <dt>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let snap = evosim::save::read_snapshot(std::path::Path::new(&a[1])).expect("load");
    let steps: usize = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(2000);
    let dt: i32 = a.get(3).and_then(|v| v.parse().ok()).unwrap_or(8);
    let mut s = snap.sim;
    println!("loaded t {} pop {} volcanoes {} lava {}", s.t, s.crs.len(), s.volcanoes.len(), s.lava.len());
    let t0 = std::time::Instant::now();
    for k in 0..steps {
        let st = std::time::Instant::now();
        s.step(dt);
        let ms = st.elapsed().as_secs_f64() * 1000.0;
        if ms > 500.0 || k % 200 == 0 {
            println!("step {k} t {} took {ms:.0} ms pop {} lava {} pools {}", s.t, s.crs.len(), s.lava.len(), s.pools.len());
        }
    }
    println!("done in {:.1}s", t0.elapsed().as_secs_f64());
}
