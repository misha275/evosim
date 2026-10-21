//! cargo run --release --example inspect -- <save>   : summary of a saved world
use evosim::genome::*;
fn main() {
    let p = std::env::args().nth(1).expect("path");
    let snap = evosim::save::read_snapshot(std::path::Path::new(&p)).expect("load");
    let s = &snap.sim;
    println!("world {}x{} {:?} primordial {} t {} pop {} plants {} species {}/{}", s.w, s.h, s.map_type, s.primordial, s.t, s.crs.len(), s.nplants, s.alive_species(), s.species.len());
    for sp in &s.species {
        println!("  sp {:>3} {:<10} born {:>6} died {:>8?} peak {:>4} pop {:>4} diet {:.2} cx {:.2} note {}", sp.id, sp.name, sp.born, sp.died, sp.peak, sp.pop, sp.founder.v[DIET], sp.founder.v[COMPLEXITY], sp.note);
    }
    let micro = s.crs.iter().filter(|c| c.g.v[COMPLEXITY] < 0.42).count();
    let swim = s.crs.iter().filter(|c| c.g.v[COMPLEXITY] >= 0.42 && can_swim(&c.g)).count();
    let inwater = s.crs.iter().filter(|c| c.g.v[COMPLEXITY] >= 0.42 && s.terr[c.y as usize * s.w + c.x as usize] == 1).count();
    println!("microbial {} | animals {} (swimmers {}, currently in water {})", micro, s.crs.len() - micro, swim, inwater);
    for (t, m, _) in &s.log { println!("  log {:>6} {}", t, m); }
    println!("hist (herb,carn,omni,plants,sick):");
    for (k, h) in s.hist.iter().enumerate() { if k % 10 == 0 { println!("  {:?}", h); } }
}
#[allow(dead_code)]
fn _x() {}
