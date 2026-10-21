//! Headless benchmark:  cargo run --release --bin bench -- <world> <ticks> <dt> [--primordial]
use evosim::sim::*;
use std::time::Instant;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let world = a.get(1).cloned().unwrap_or("giant".into());
    let ticks: i64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(20000);
    let dt: i32 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(1);
    let prim = a.iter().any(|s| s == "--primordial");
    let (_, w, h) = *WORLDS.iter().find(|x| x.0 == world).unwrap();
    let t0 = Instant::now();
    let seed: u64 = std::env::var("SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(42);
    let mut s = Sim::new(w, h, prim, seed);
    println!("gen {:.2}s pop {} plants {}", t0.elapsed().as_secs_f32(), s.crs.len(), s.nplants);
    let t0 = Instant::now();
    let mut last = Instant::now();
    while s.t < ticks {
        s.step(dt);
        if last.elapsed().as_secs_f32() > std::env::var("EVERY").ok().and_then(|v| v.parse().ok()).unwrap_or(4.0) {
            last = Instant::now();
            println!("t {} pop {} plants {} land {} crowd {:.2} species {}/{} lineages {} tps {:.0}", s.t, s.crs.len(), s.nplants, s.land, s.crowd, s.alive_species(), s.species.len(), s.vegetation.lineages.len(), s.t as f32 / t0.elapsed().as_secs_f32());
        }
    }
    let el = t0.elapsed().as_secs_f32();
    println!("time: weather {:.2}s plants {:.2}s corpses {:.2}s creatures {:.2}s", s.prof[0], s.prof[1], s.prof[2], s.prof[4]);
        let mut forms = std::collections::BTreeMap::new(); for i in 0..s.n { if s.pl[i] > 0 { *forms.entry(s.vegetation.lineage(i).form().name()).or_insert(0) += 1; } }
    println!("forms {:?} land {} cap {} crowd {:.3} poop {}", forms, s.land, s.land as f32*0.1, s.crowd, s.poop.len());
    println!("DONE t {} in {:.2}s = {:.0} ticks/s | pop {} plants {} species {} trees {} lineages {}", s.t, el, s.t as f32/el, s.crs.len(), s.nplants, s.alive_species(), s.vegetation.trees.len(), s.vegetation.lineages.len());
}
