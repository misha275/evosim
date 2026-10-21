# Changelog

## 2.4.0
- Microbial colonies are drawn 2x1 (small, horizontal) up to 2x2 (large); colonial life shows a second cell type.
- Fix: in primordial worlds the colonies filled the population limit within ~1000 ticks (free photosynthesis and
  plant meals worth hundreds of ticks), so no one could be born and every new species died as a single individual.
  Colonies now share a regenerating nutrient pool in the water and get meals scaled to their size, so their numbers are
  limited by food: populations fluctuate below the limit and new species can spread (3x more living species).
- Predators get the whole carcass from a kill (previously they starved between kills).
- `cargo run --release --example inspect -- saves/autosave.evo2` prints a summary of a saved world.

## 2.3.1
- Plains show a faint hint of relief (about a fifth of the mountain shading).

## 2.3.0
- Flat land: only mountains are drawn with relief (hill shading and rock tones fade in with height); plains, forests,
  deserts are flat biome colours. Softer, wavy borders between biomes.
- Version shown in the window title, the panel and the welcome screen.

## 2.2.0
- New world generator: continents / pangaea / archipelago, plains with mountain belts (<10% of land), droplet erosion,
  drainage, lakes, branching rivers that can be waded, climate (latitude, altitude, ocean, west-wind rain shadows),
  14 biomes affecting fertility, moisture and temperature.
- Map views (`V`): terrain, temperature, soil moisture, biomes, elevation; cursor info; legends.
- Bigger maps: `titan` 3072x2048, `planet` 4096x2730, generated on a background thread with a progress screen.
- Pioneer plants per climate band; animal numbers scale with land; separate carrying capacity for algae;
  animals graze several plants per coarse step; land animals ignore algae.

## 2.1.0
- 10 new plant genes (toxin, thorns, succulence, deciduous, fire resistance, climbing, runners, roots, annual, carnivory)
  and 7 new forms (conifer, palm, cactus, vine, flytrap, flower, reed); seasonal leaf colours.
- Point mutations, plant ageing, seed rain from neighbours; lineage records compacted on long runs.

## 2.0.0
- Rewrite in Rust: multithreaded simulation, GPU-scaled rendering, speeds x1…x5000 and MAX, adaptive time step,
  background saves.
