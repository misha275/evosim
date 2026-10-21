# Changelog

## 2.6.0
- Giants: gigantism is a late innovation. After ~30 generations of complex animals the size limit starts rising from 8
  up to 26 (reached around generation 120). Big bodies mature later, store far more energy, are safe from small
  predators, run hot in warm climates and need many plants per meal (each plant gives the same food to everyone).
- Tree crowns: small animals only nibble the lower leaves of tall trees; big, long-necked or upright animals browse the
  whole crown and get more from it.
- New body genes: neck length, stance (four-legged / upright two-legged), back plates / sail, colour lightness and
  saturation. Effects: long necks reach crowns and see farther but are easier to bite; upright runners are faster and
  grab prey but carry armor badly; plates and sails shed heat and deter bites but slow down; horns and tail clubs
  wound predators that fail.
- Colours matter: camouflage - the closer the body colour to the ground under it (grass, forest, sand, snow), the
  later it is noticed by predators and by prey; movement and size give animals away, patterns help in cover.
  Warning colours - vivid animals are avoided by predators as often as vivid animals have really been venomous or
  spiny (harmless mimics profit too). Dark bodies warm up in the sun, pale ones stay cool. Bright colours and plates
  help win mates.
- New animal drawing: arched bodies, long necks, two-legged hunters with heavy tails, plates, sails, scutes, frills,
  tail clubs, horns; giants up to ~20 px long. The creature panel shows the body plan (e.g. "giant long-necked
  browser") and how well it is hidden.
- Balance: meat-eaters have bigger stomachs, rest to digest, hunt prey up to 1.25x their size, keep together in packs
  and find mates from farther away; diet changes in small steps; specialists digest fully, mixed diets poorly.
  Predator numbers now cycle with their prey (peaks of 150-380 predators on a giant map).
- Saves from 2.5 and older cannot be loaded (the genome changed).
- `cargo run --release --example critters` renders the body plans; `--example closeup -- <ticks> <seed> <prefix>`
  renders close-ups of a late world.

## 2.5.0
- Microbial colonies have their own population limit (60% of the animal limit), independent of animals, and are drawn
  dimmer and half-transparent. The animal sandbox starts without colonies; primordial worlds keep them as the origin of life.
- Swimming: speed comes from fins, bodies glide on momentum and turn slowly. Swimmers are drawn without legs or shadow,
  half under the surface, with ripples, a wake and a gentle bob.
- Shorelines are barriers: amphibious animals cross between open water and land only with effort (energy cost, failed
  attempts turn them back) and stay out of the deep sea; aquatic animals never leave the water.
- Diet: plant and meat digestion follow steep curves, so omnivores digest both poorly and omnivory is hard to evolve.
- Predators: bigger carcass yields, pack-hunting bonus, kits fed from kills, sated predators rest instead of killing,
  herbivores can't take the slots reserved for predators (they keep ≤80% of the population limit).
- Predator look: long low body, dark dorsal stripe, pointed ears, fangs and claws. Hunting animations: gallop with dust
  while chasing, leap at the pounce, head down while feeding, speed lines while fleeing; blood at kills, dust clouds.
- `cargo run --release --example critters` renders the poses to critters.png.

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
