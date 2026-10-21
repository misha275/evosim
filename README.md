# EVOSIM

A pixel ecosystem with inheritance, evolving body plans and plant lineages, local weather, decomposition and disease.
Rewritten in **Rust**: about 35–50x faster than the original Python version, multithreaded, with GPU scaling of the world image.
The original Python version is kept in `python_legacy/`.

## Run

Windows: just start `evosim.exe` (prebuilt) — or build it yourself:

```sh
cargo run --release
```

Requires Rust (rustup). The first build downloads dependencies (~1 min).

The welcome screen offers a new primordial world, continuing the autosave, loading the quicksave, or the animal sandbox.
Use left/right to choose a map size (`small` 340x230 … `giant` 1600x1060 (default) … `colossal` 2048x1360).

Direct launch options:

```sh
cargo run --release -- --primordial --world giant
cargo run --release -- --animals --world small
cargo run --release -- --world 2000x1200
cargo run --release -- --load saves/quicksave.evo2
cargo run --release -- --animals --warmup 200000      # simulate 200k ticks before showing the world
cargo run --release --bin bench -- giant 20000 8      # headless benchmark: world, ticks, dt
```

## Speed

1 tick = 0.2 s of simulated time, so **x1 = real time = 5 ticks/s**.

| key | speed | ticks/s |
|---|---|---|
| `1`…`9` | x1, x5, x20, x50, x100, x200, x500, x1000, x5000 | 5 … 25 000 |
| `M` | MAX — as fast as the CPU allows | |
| `[` / `]` | one step slower / faster | |

The panel shows the requested speed and the **actual** speed reached. The step is adaptive: the simulation uses exact
1-tick steps whenever the computer keeps up, and only switches to coarser 2/4/8-tick steps (like the old “hyperspeed”) when it
would otherwise fall behind. Steps never exceed 8 ticks, because creatures can eat at most once per step.

Measured on the same 2-core cloud VM (Python → Rust, ticks/s): giant map 149 → 5 765 (x1150 real time),
small map 1 884 → 99 000. A desktop CPU with more cores is faster still.

## How it is fast

- Native code with data laid out for the cache: plant growth samples are generated in ascending order,
  an occupancy bit-set (fits in L2) skips empty cells, memory is prefetched ahead of use.
- Multithreading: the simulation runs on its own thread; creature perception (the most expensive query) and the plant
  sweep run in parallel with rayon. Results are deterministic for a given seed.
- GPU: the world is composed once at native resolution (one pixel per cell), uploaded as a texture and scaled by the GPU.
  Only the visible region is recomposed; in very fast modes the map is redrawn at 20 fps to leave the CPU to the simulation.
- The ecosystem itself (agent AI with branching, random point updates) does not map well onto GPU compute, so the GPU
  is used for drawing, not for the model.

## Plants

Plant seeds inherit height, woodiness, climate preferences, dispersal, defenses and tree architecture — crown shape,
weeping branches, foliage density, branching, trunk girth and flowers/fruit with their own colour. Trees are drawn from those genes.

Selection on plants:
- **Light**: taller neighbours shade shorter plants. Height only counts if the wood can hold it up, so trees emerge as height
  and wood co-evolve. Growing tall and woody costs growth.
- **Janzen–Connell effect**: dense stands of one lineage suffer from specialist pests, so rare variants establish more easily.
- **Seed rain**: new sprouts usually inherit a nearby plant's lineage; animals carry seeds of the plants they ate and drop them
  in dung (fruiting plants are dispersed much more often).
- Turnover continues up to ~1.5x the carrying capacity, so evolution does not freeze when the land is full.

## Controls

- `Space` pause · `R` restart · `N` next map size · `S` screenshot
- `F5` quicksave · `F9` load quicksave (or autosave). Autosave every 60 s and on exit to `saves/autosave.evo2`.
  Saves are written in the background and atomically. Old Python `.evo` saves cannot be loaded by the Rust version.
- Left click inspect · drag pan · wheel or `+`/`-` zoom · arrows pan · `0` fit map
- Right click: seeds · `H`/`C`: herbivore/carnivore founders (animal sandbox)
- `T` or click the panel tree: fullscreen phylogeny; `Tab` switches all creatures / microbial / animals / plants;
  hover/click to pin, `Enter` jumps to a living member, `E` extinct, `M` minor branches, `Esc` close.

Primordial worlds contain microbial colonies and photosynthetic biofilms/algae; every later animal must descend from
those colonies. Extinction ends that world's living lineages until you restart.

## Verification

```sh
cargo test --release
```
