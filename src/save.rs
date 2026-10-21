//! Versioned, atomic snapshots: bincode + gzip.
use crate::sim::Sim;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAGIC: &[u8] = b"EVOSIM-RS\x04\n";

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub z: f32,
    pub x: f32,
    pub y: f32,
}

#[derive(Serialize, Deserialize)]
pub struct Snapshot {
    pub sim: Sim,
    pub view: View,
    pub speed_index: usize,
    pub paused: bool,
}

pub fn save_dir() -> PathBuf {
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf()));
    // Prefer ./saves next to the project when run via cargo, else next to the executable.
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if cwd.join("Cargo.toml").exists() || exe_dir.is_none() {
        cwd.join("saves")
    } else {
        exe_dir.unwrap().join("saves")
    }
}

pub fn autosave_path() -> PathBuf {
    save_dir().join("autosave.evo2")
}
pub fn quicksave_path() -> PathBuf {
    save_dir().join("quicksave.evo2")
}

pub fn write_snapshot(path: &Path, snap: &Snapshot) -> Result<(), String> {
    let dir = path.parent().ok_or("bad path")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let raw = bincode::serialize(snap).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    {
        let f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let mut w = std::io::BufWriter::new(f);
        w.write_all(MAGIC).map_err(|e| e.to_string())?;
        let mut gz = flate2::write::GzEncoder::new(w, flate2::Compression::fast());
        gz.write_all(&raw).map_err(|e| e.to_string())?;
        let mut w = gz.finish().map_err(|e| e.to_string())?;
        w.flush().map_err(|e| e.to_string())?;
        w.get_ref().sync_all().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn read_snapshot(path: &Path) -> Result<Snapshot, String> {
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut magic = vec![0u8; MAGIC.len()];
    f.read_exact(&mut magic).map_err(|e| e.to_string())?;
    if magic != MAGIC {
        if magic.starts_with(b"EVOSIM-RS") {
            return Err("save from an older EVOSIM version (world format changed)".into());
        }
        return Err("not an EVOSIM (Rust) save".into());
    }
    let mut raw = Vec::new();
    flate2::read::GzDecoder::new(std::io::BufReader::new(f)).read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let mut snap: Snapshot = bincode::deserialize(&raw).map_err(|e| e.to_string())?;
    let s = &snap.sim;
    if !(120..=4096).contains(&s.w) || !(120..=4096).contains(&s.h) || s.n != s.w * s.h {
        return Err("invalid saved world dimensions".into());
    }
    if s.terr.len() != s.n || s.biome.len() != s.n || s.pl.len() != s.n || s.bg.len() != s.n || s.vegetation.ids.len() != s.n {
        return Err("incomplete saved world".into());
    }
    if s.atmosphere.width != s.w || s.atmosphere.height != s.h {
        return Err("invalid saved atmosphere".into());
    }
    if !(snap.view.z.is_finite() && snap.view.x.is_finite() && snap.view.y.is_finite()) {
        return Err("invalid saved camera".into());
    }
    snap.sim.rebuild_after_load();
    Ok(snap)
}
