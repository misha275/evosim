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
    let map = std::env::var("MAP").ok().and_then(|m| evosim::worldgen::MapType::ALL.iter().copied().find(|t| t.name() == m)).unwrap_or(evosim::worldgen::MapType::Continents);
    let mut s = Sim::new_cfg(w, h, prim, seed, map, prim && std::env::var("NO_MICROBES").is_err());
    println!("gen {:.2}s pop {} plants {}", t0.elapsed().as_secs_f32(), s.crs.len(), s.nplants);
    food_check(&s);
    for _ in 0..100 { s.step(8); }
    food_check(&s);
    let t0 = Instant::now();
    let mut last = Instant::now();
    while s.t < ticks {
        s.step(dt);
        if last.elapsed().as_secs_f32() > std::env::var("EVERY").ok().and_then(|v| v.parse().ok()).unwrap_or(4.0) {
            last = Instant::now();
            let gl = (0..s.n).filter(|&i| s.pl[i] >= 2 && s.terr[i] != 1).count();
            let herb = s.crs.iter().filter(|c| c.g.v[evosim::genome::DIET] < 0.45).count();
            let me: f32 = s.crs.iter().map(|c| c.e / c.max_e).sum::<f32>() / s.crs.len().max(1) as f32;
            let omni = s.crs.iter().filter(|c| { let d = c.g.v[evosim::genome::DIET]; d >= 0.45 && d <= 0.65 }).count();
            let carn = s.crs.iter().filter(|c| c.g.v[evosim::genome::DIET] > 0.65).count();
            let ce: f32 = s.crs.iter().filter(|c| c.g.v[evosim::genome::DIET] >= 0.45).map(|c| c.e / c.max_e).sum::<f32>() / (omni + carn).max(1) as f32;
            println!("   grown-land {} herb {} mean-energy {:.2} sick {} omni {} carn {} pred-energy {:.2}", gl, herb, me, s.nsick, omni, carn, ce);
            { let mut ep: Vec<u32> = s.crs.iter().map(|c| c.g.epoch).collect(); ep.sort(); let q = |f: f32| ep.get((ep.len() as f32 * f) as usize).copied().unwrap_or(0);
              let ms = s.crs.iter().map(|c| c.g.v[evosim::genome::SIZE]).fold(0f32, f32::max);
              let avg = |k: usize| s.crs.iter().map(|c| c.g.v[k]).sum::<f32>() / s.crs.len().max(1) as f32;
              println!("   epoch med {} p90 {} max {} | size mean {:.1} max {:.1} | {}", q(0.5), q(0.9), ep.last().copied().unwrap_or(0), avg(evosim::genome::SIZE), ms, evosim::genome::EXTRA_REPORT.iter().map(|&(n,k)| format!("{}={:.2}", n, avg(k))).collect::<Vec<_>>().join(" ")); }
            { use evosim::genome::*;
              let mean = |f: &dyn Fn(&Cr) -> bool, k: usize| { let v: Vec<f32> = s.crs.iter().filter(|c| f(c)).map(|c| c.g.v[k]).collect(); v.iter().sum::<f32>() / v.len().max(1) as f32 };
              let herb = |c: &Cr| c.g.v[DIET] < 0.45 && c.g.v[COMPLEXITY] >= 0.42;
              let carn = |c: &Cr| c.g.v[DIET] > 0.65;
              let hc: f32 = { let v: Vec<f32> = s.crs.iter().filter(|c| herb(c)).map(|c| c.consp).collect(); v.iter().sum::<f32>() / v.len().max(1) as f32 };
              println!("   ARMS prey: armor {:.2} spd {:.2} ears {:.2} size {:.1} consp {:.2} | hunters: jaws {:.2} eyes {:.2} spd {:.2} size {:.1} immune {:.2}",
                mean(&herb, ARMOR), mean(&herb, SPD), mean(&herb, EARS), mean(&herb, SIZE), hc, mean(&carn, JAWS), mean(&carn, EYES), mean(&carn, SPD), mean(&carn, SIZE), mean(&carn, IMMUNE)); }
            { let cons: f32 = s.crs.iter().map(|c| c.consp).sum::<f32>() / s.crs.len().max(1) as f32;
              println!("   conspicuousness {:.2} warning-colour trust {:.2}", cons, s.apo_trust); }
            println!("t {} pop {} plants {} land {} crowd {:.2} species {}/{} lineages {} tps {:.0}", s.t, s.crs.len(), s.nplants, s.land, s.crowd, s.alive_species(), s.species.len(), s.vegetation.lineages.len(), s.t as f32 / t0.elapsed().as_secs_f32());
        }
    }
    let el = t0.elapsed().as_secs_f32();
    println!("parallel: plants {:.2}s creatures {:.2}s | serial {:.2}s", s.prof[5], s.prof[7], el as f64 - s.prof[5] - s.prof[7]);
    println!("time: weather {:.2}s plants {:.2}s (decide {:.2} apply {:.2}) corpses {:.2}s creatures {:.2}s", s.prof[0], s.prof[1], s.prof[5], s.prof[6], s.prof[2], s.prof[4]);
        let mut forms = std::collections::BTreeMap::new(); for i in 0..s.n { if s.pl[i] > 0 { *forms.entry(s.vegetation.lineage(i).form().name()).or_insert(0) += 1; } }
    let mut tall = std::collections::BTreeMap::new(); for (i,l) in s.vegetation.lineages.iter().enumerate() { if s.vegetation.counts[i]>0 { *tall.entry(l.form().name()).or_insert(0u32) += s.vegetation.counts[i]; } }
    println!("living forms {:?}", tall);
    { let v=&s.vegetation; let mut hist=[0u32;30]; for (i,l) in v.lineages.iter().enumerate() { hist[(l.generation as usize).min(29)] += v.counts[i]; } println!("base0 {} base1 {} gen-hist {:?} compactions epoch {} created {}", v.counts[0], v.counts[1], &hist[..20], v.epoch, v.total_created); }
    { let v=&s.vegetation; let mut tot=0f64; let mut acc=[0f64; evosim::vegetation::NPG]; let mut mg=0; for (i,l) in v.lineages.iter().enumerate() { let c=v.counts[i] as f64; if c>0.0 { mg=mg.max(l.generation); tot+=c; for k in 0..evosim::vegetation::NPG { acc[k]+=c*l.g[k] as f64; } } } let names: Vec<String> = evosim::vegetation::TRAITS.iter().map(|(n,k)| format!("{}={:.2}", n, acc[*k]/tot.max(1.0))).collect(); println!("max gen {} mean genes {}", mg, names.join(" ")); }
    println!("forms {:?} land {} cap {} crowd {:.3} poop {}", forms, s.land, s.land as f32*0.1, s.crowd, s.poop.len());
    { let g2 = s.pl.iter().filter(|&&p| p >= 2).count(); let gl = (0..s.n).filter(|&i| s.pl[i] >= 2 && s.terr[i] != 1).count(); let herb_t = s.crs.iter().filter(|c| c.tgt.is_some()).count(); println!("grown {} grown-land {} with-target {}/{}", g2, gl, herb_t, s.crs.len()); }
    { let cb = s.lin.values().filter(|l| l.par != 0 && s.species[l.sp as usize].founder.v[evosim::genome::DIET] > 0.65).count();
      let cf = s.lin.values().filter(|l| l.par == 0 && s.species[l.sp as usize].founder.v[evosim::genome::DIET] > 0.65).count();
      println!("carnivore founders {} births {}", cf, cb); }
    println!("species with peak>=10: {} of {} | maxpop {}", s.species.iter().filter(|p| p.peak >= 10).count(), s.species.len(), s.maxpop);
    println!("DONE t {} in {:.2}s = {:.0} ticks/s | pop {} plants {} species {} trees {} lineages {}", s.t, el, s.t as f32/el, s.crs.len(), s.nplants, s.alive_species(), s.vegetation.trees.len(), s.vegetation.lineages.len());
}
#[allow(dead_code)]
pub fn food_check(s: &evosim::sim::Sim) {
    let mut ok = 0; let mut tot = 0; let mut on_sand = 0; let mut aq = 0;
    for c in &s.crs {
        if c.g.v[evosim::genome::DIET] > 0.45 { continue; }
        tot += 1;
        let r = c.g.v[evosim::genome::VIS] as i64;
        let (cx, cy) = (c.x as i64, c.y as i64);
        let mut found = false;
        'o: for y in (cy - r).max(0)..(cy + r).min(s.h as i64) { for x in (cx - r).max(0)..(cx + r).min(s.w as i64) {
            let i = y as usize * s.w + x as usize; if s.pl[i] >= 2 && s.terr[i] != 1 { found = true; break 'o; } } }
        if found { ok += 1; }
        let i = cy as usize * s.w + cx as usize;
        if s.terr[i] == 3 { on_sand += 1; }
        if s.terr[i] == 1 { aq += 1; }
    }
    let mism: Vec<f32> = s.crs.iter().map(|c| (s.temp_at(c.y as usize * s.w + c.x as usize) - c.g.v[evosim::genome::TOL]).abs()).collect();
    let bad = mism.iter().filter(|&&m| m > 0.15).count();
    let e: f32 = s.crs.iter().map(|c| c.e / c.max_e).sum::<f32>() / s.crs.len().max(1) as f32;
    println!("herbivores with grown land plant in view: {}/{} (on sand {}, in water {}) | climate mismatch>0.15: {} | mean energy {:.2} | d_t {:.3}", ok, tot, on_sand, aq, bad, e, s.d_t);
}
