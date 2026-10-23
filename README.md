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
Left/right choose the map size (`small` 340x230 … `giant` 1600x1060 (default) … `colossal` 2048x1360, `titan` 3072x2048,
`planet` 4096x2730); `Tab` (or a click) chooses the map type: **continents**, **pangaea** or **archipelago**.
Large maps take a while to generate (a progress screen is shown; `planet` ~10–30 s).

## World generation

1. **Land and sea**: domain-warped fractal noise. *Continents*: several landmasses grown from separated cores with open ocean
   between them; *pangaea*: one supercontinent; *archipelago*: many islands of all sizes.
2. **Mountains**: ridged multifractal noise inside mountain belts that cover a fixed share of the land (mountains, alpine
   meadows and high glaciers stay under ~10% of the land); continental interiors are low plains and plateaus.
3. **Erosion**: hundreds of thousands of water droplets carve gullies and deposit sediment.
4. **Drainage**: a priority-flood finds where every cell drains; closed basins become lakes.
5. **Climate**: temperature from latitude (cold north, hot south), altitude lapse rate and the ocean's moderating effect;
   seasons swing harder in the north. Rain is carried by the west wind and wrung out on windward slopes, leaving rain shadows
   (deserts) behind ranges; plus a dry subtropical belt and wet tropics.
6. **Rivers**: rain accumulated downhill along the drainage tree; big flows become rivers that branch, widen, carve valleys and
   end in lakes or the sea. Rivers can be waded across; lakes and seas need swimmers.
7. **Biomes** from temperature and moisture: ocean, lake, river, beach, desert, savanna, grassland, temperate forest,
   rainforest, swamp, taiga, tundra, alpine meadow, mountain rock, glacier. Each biome sets soil fertility; temperature
   and soil moisture decide which plants can grow and how much animals spend on staying warm or cool.
   The world starts with pioneer plants adapted to each climate band; animal numbers scale with the land area.

The planet keeps changing: tectonic plates drift, mountains rise where plates collide and wear down elsewhere,
earthquakes strike along faults, volcanoes erupt (lava, ash winters, new land, fertile volcanic soil) and heavy rain
floods the land around rivers.

`V` cycles the map view: terrain (hill-shaded relief), **temperature** (live, including the season), soil moisture, biomes,
elevation, tectonic plates — with a legend. Hovering the map shows biome, °C, moisture, elevation and fertility under the cursor.
`cargo run --release --example terrain -- 1600 1060 <seed> [biome|temp|moist] <continents|pangaea|archipelago>` renders a
map to `terrain.png`.

Direct launch options:

```sh
cargo run --release -- --primordial --world giant
cargo run --release -- --animals --world small --map archipelago
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

Measured on a 2-core cloud VM (Python → Rust, ticks/s, before the new world generator): giant map 149 → 5 765,
small map 1 884 → 99 000. With the richer worlds (more land life, up to 1200 animals) the giant map runs ~2 000–2 400
ticks/s (x400–x480) on that VM, medium/large maps several times faster. A desktop CPU with more cores is faster still.

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

Every plant seed inherits 25 genes and mutates (usually one or two genes at a time). Plant form follows from the genes,
and each gene has a real cost and benefit in the ecosystem:

| gene | what it does |
|---|---|
| height, wood, girth | tall plants shade shorter neighbours, but height only counts if wood can hold it up; wood survives grazing and lives long |
| moisture, temperature, aquatic | climate and water niche (aquatic plants live in water, reeds on both sides of the shore) |
| roots | reach deeper water: grow in dry land |
| succulence | stores water: survives drought and heat waves, barely burns → **cactus** |
| deciduous | sleeps through winter instead of freezing; autumn colours, bare in winter, lets light through in winter |
| toxin | herbivores lose energy eating it and learn to avoid it (unless their immune system copes) |
| thorns | hurts herbivores (unless armored); they avoid it |
| defense | less nutritious, often survives a bite |
| fire resist | thick bark: resprouts after fire and the heat releases its seeds |
| climbing | not shaded by what it climbs on, but needs tall neighbours → **vine** |
| runners | spreads clones next to itself (rhizomes), survives grazing |
| annual | grows and seeds fast, dies after seeding and in winter |
| carnivory | catches insects: grows on barren soil → **flytrap** |
| bloom, dispersal | flowers/fruit: animals carry the seeds and drop them in dung far away → **flower** |
| crown, droop, density, branching, hues | the look of the crown: round, conical (**conifer**), umbrella (**palm**), weeping |

Forms: tree, conifer, palm, shrub, cactus, vine, flytrap, flower, reed, kelp, algae, grass, moss, biofilm.
The plant phylogeny (`T`, then `Tab` to Plants) shows a census of living forms, every gene of a lineage, and its sprite
(deciduous ones through the seasons).

Selection on plants:
- **Light**: taller neighbours shade shorter plants; climbers escape shade.
- **Janzen–Connell effect**: dense stands of one lineage suffer from specialist pests, so rare variants establish more easily.
- **Herbivores**: they choose palatable plants, so toxins and thorns pay off; animals evolve immunity/armor in return.
- **Seed rain**: sprouts inherit a nearby plant's lineage; animals carry seeds of what they ate.
- **Turnover**: soft plants live weeks, woody ones months, annuals die after seeding; turnover keeps evolution going.
- Lineage records are compacted on very long runs (forgotten extinct twigs are dropped, living ancestry is kept).

Forests, conifers, palms and cacti need time: run a world for ~1 million ticks (a few minutes at x1000).
`cargo run --release --example gallery` renders all forms to `plant_gallery.png`.

## Controls

- `Space` pause · `R` new world (same size and type) · `N` next map size · `V` map view · `S` screenshot
- `F5` quicksave · `F9` load quicksave (or autosave). Autosave every 60 s and on exit to `saves/autosave.evo2`.
  Saves are written in the background and atomically. Saves from older versions cannot be loaded (an unreadable
  autosave is kept as `autosave.unreadable.evo2` instead of being overwritten).
- Left click inspect · drag pan · wheel or `+`/`-` zoom · arrows pan · `0` fit map
- Right click: seeds · `H`/`C`: herbivore/carnivore founders (animal sandbox)
- `T` or click the panel tree: fullscreen phylogeny; `Tab` switches all creatures / microbial / animals / plants;
  hover/click to pin, `Enter` jumps to a living member, `E` extinct, `M` minor branches, `Esc` close.

Primordial worlds contain microbial colonies and photosynthetic biofilms/algae; every later animal must descend from
those colonies. Colonies have their own population limit (independent of animals) and are drawn dimmed; the animal
sandbox has none.

Predators and herbivores: digestion of plants and meat follows steep curves, so specialists do well and omnivores are rare.
Predators hunt in packs, feed their young from kills and rest when sated; they have their own look (long low body,
dorsal stripe, fangs, claws) and hunting animations (gallop, pounce, feeding, blood). Swimmers float half-submerged with
ripples and a wake; shorelines and deep sea are real barriers.

Body plans and colours evolve and decide survival: long necks browse tree crowns, upright two-legged runners hunt,
plates and sails cool the body, horns and tail clubs fight back. Colours work as camouflage against the ground under
the animal, as warning colours (trusted by predators as far as vivid animals are really venomous or spiny) and as
sun protection. Late in evolution gigantism unlocks: dinosaur-like giants up to size 26.
`cargo run --release --example critters` renders the body plans and hunting poses. Extinction ends that world's living lineages until you restart.

## Verification

```sh
cargo test --release
```
