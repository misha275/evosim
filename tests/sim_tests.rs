use evosim::genome::*;
use evosim::save::*;
use evosim::sim::*;
use evosim::util::Rng;
use rand::SeedableRng;

fn small(primordial: bool, seed: u64) -> Sim {
    Sim::new(340, 230, primordial, seed)
}

/// Derived indexes must always agree with the raw state.
fn check_invariants(s: &Sim) {
    let plants = s.pl.iter().filter(|&&p| p > 0).count() as i64;
    assert_eq!(plants, s.nplants, "nplants");
    for (l, &cnt) in s.vegetation.counts.iter().enumerate() {
        let real = (0..s.n).filter(|&i| s.pl[i] > 0 && s.vegetation.ids[i] as usize == l).count() as u32;
        assert_eq!(real, cnt, "lineage {l} count");
    }
    for i in 0..s.n {
        assert_eq!(s.occ[i >> 6] & (1 << (i & 63)) != 0, s.pl[i] > 0, "occupancy bit {i}");
    }
    let mut pc = vec![0u16; s.pcount.len()];
    for i in 0..s.n {
        if s.pl[i] >= 2 {
            pc[(i / s.w / CH) * s.cwc + (i % s.w) / CH] += 1;
        }
    }
    assert_eq!(pc, s.pcount, "grown-plant chunk counts");
    for (j, c) in s.crs.iter().enumerate() {
        assert_eq!(s.idmap[&c.id], j);
        assert!(!c.dead);
        assert!(c.x >= 0.0 && c.y >= 0.0 && c.x < s.w as f32 && c.y < s.h as f32, "creature in bounds");
        assert!(c.e.is_finite() && c.s.is_finite());
    }
    for sp in &s.species {
        let n = s.crs.iter().filter(|c| c.sp == sp.id).count() as i32;
        assert_eq!(n, sp.pop, "species {} pop", sp.name);
    }
    for &t in &s.vegetation.trees {
        assert!(s.pl[t as usize] >= 2);
    }
}

#[test]
fn runs_and_keeps_invariants() {
    for prim in [false, true] {
        let mut s = small(prim, 7);
        check_invariants(&s);
        for k in 0..3000 {
            s.step(if k % 3 == 0 { 8 } else { 1 });
        }
        check_invariants(&s);
        assert!(s.t > 3000);
    }
}

#[test]
fn deterministic_for_same_seed() {
    let mut a = small(false, 99);
    let mut b = small(false, 99);
    for _ in 0..1500 {
        a.step(1);
        b.step(1);
    }
    assert_eq!(a.crs.len(), b.crs.len());
    assert_eq!(a.nplants, b.nplants);
    assert_eq!(a.pl, b.pl);
    let pa: Vec<(f32, f32)> = a.crs.iter().map(|c| (c.x, c.y)).collect();
    let pb: Vec<(f32, f32)> = b.crs.iter().map(|c| (c.x, c.y)).collect();
    assert_eq!(pa, pb);
}

#[test]
fn save_load_roundtrip_continues_identically() {
    let mut s = small(false, 5);
    for _ in 0..800 {
        s.step(1);
    }
    let dir = std::env::temp_dir().join(format!("evosim-test-{}", std::process::id()));
    let path = dir.join("t.evo2");
    let snap = Snapshot { sim: s.clone(), view: View { z: 2.0, x: 3.0, y: 4.0 }, speed_index: 3, paused: true };
    write_snapshot(&path, &snap).unwrap();
    let back = read_snapshot(&path).unwrap();
    assert_eq!(back.view, View { z: 2.0, x: 3.0, y: 4.0 });
    assert_eq!(back.speed_index, 3);
    let mut l = back.sim;
    check_invariants(&l);
    for _ in 0..400 {
        s.step(1);
        l.step(1);
    }
    assert_eq!(s.pl, l.pl);
    assert_eq!(s.crs.len(), l.crs.len());
    assert_eq!(s.t, l.t);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn rejects_garbage_save() {
    let dir = std::env::temp_dir().join(format!("evosim-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("bad.evo2");
    std::fs::write(&p, b"not a save").unwrap();
    assert!(read_snapshot(&p).is_err());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn genetics_stay_in_range_and_innovations_are_gated() {
    let mut r = Rng::seed_from_u64(3);
    let b = base_genome(&mut r, Kind::Microbe);
    let mut h1 = b.clone();
    let mut h2 = b.clone();
    for _ in 0..500 {
        let (a, _) = gamete(&mut r, &h1, &h2);
        let (c, _) = gamete(&mut r, &h1, &h2);
        let g = phenotype(&mut r, &a, &c);
        for k in 0..NG {
            assert!(g.v[k] >= RANGE[k].0 - 1e-4 && g.v[k] <= RANGE[k].1 + 1e-4, "{} out of range", NAMES[k]);
        }
        h1 = a;
        h2 = c;
    }
    // wings cannot change before generation 12
    let g0 = base_genome(&mut r, Kind::Herb);
    for _ in 0..200 {
        let (m, _) = mutate(&mut r, &g0, 3.0);
        assert_eq!(m.v[WINGS], g0.v[WINGS]);
    }
}

#[test]
fn disasters_do_not_break_state() {
    let mut s = small(false, 11);
    for _ in 0..200 {
        s.step(1);
    }
    s.meteor_spawn(Some((150, 110)), true);
    s.earthquake(Some((100, 100, 40)));
    s.lightning(None);
    s.outbreak(None);
    for _ in 0..400 {
        s.step(1);
    }
    check_invariants(&s);
}

#[test]
fn plants_evolve_new_lineages() {
    let mut s = small(false, 21);
    for _ in 0..2500 {
        s.step(8);
    }
    assert!(s.vegetation.lineages.len() > 50, "plant lineages: {}", s.vegetation.lineages.len());
}
