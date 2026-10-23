# Changelog

## 2.8.0 - hunters and hunted
- Meat is energy-dense: a kill is worth 3x the prey's body energy store (scaled by its mass), far more than a plant.
  The hunter eats part at once; the rest stays on the carcass for the next hours (shared with the pack, the young and
  scavengers). Natural deaths leave twice as much meat as before. Hunting now pays: predators hold about a quarter of
  the animals instead of starving out.
- Predators hold territories: they stop breeding at 30% of the population limit (plant-eaters at 80%), which keeps
  them from wiping out their prey.
- Omnivory is harder: diets at 0.15 or below and 0.85 or above digest fully; in between both fall off steeply
  (a 50/50 diet gets ~6% from each).
- Arms race - every defence has an answer, and both sides keep evolving:
  - armor, thick hide, plates <-> new gene: jaws (bite force; costs energy, shown as a heavy lower jaw);
  - camouflage <-> keen eyes (eye size lets a hunter see through camouflage, prey spot hidden hunters);
  - herds (mates nearby mob the hunter) <-> packs (pack hunters succeed more and take prey up to 2.3x their size);
  - spines <-> the hunter's own armor; venom <-> the hunter's immune system; big ears <-> earlier flight;
  - speed <-> speed. New evolution milestones: armored prey, bone-crushing jaws, keen-eyed hunters, giants.
- A fresh world starts with 5 herbivore species (a grazer, a fast big-eared one, an armored one, a horned herd
  species, a long-necked browser), 2 hunter species and an omnivore, with colours spread around the colour wheel.
  (Before, each kind was one species, so the whole start had one colour; camouflage then pulls colours towards the
  ground - yellow-green grass, tan sand - which is why animals drift to warm hues over time.)
- Saves from 2.7 cannot be loaded.

## 2.7.0 - a living planet
- Tectonic plates (6-14 per world, with curved borders) drift and slowly change course. Where they collide,
  mountain ranges rise; where they part, rift valleys sink; where they slide past each other, earthquakes strike.
  Colliding plates push their borders forward over time.
- Old mountains wear down: high ground weathers (faster in rain), steep slopes slide into the valleys and enrich them.
  Uplift balances erosion over the whole planet, so ranges grow in one place and fade in another while the share of
  mountains and the land area stay about where they started.
- Earthquakes happen on faults and crack the ground (one side of the fault rises); a quake can wake a volcano nearby.
- Volcanoes sit on active faults (new ones are born now and then). They sleep for a long time, then erupt: lava runs
  downhill, burns plants, kills animals that don't flee, and builds land where it reaches the sea; the cone grows;
  big eruptions fill the sky with ash and cool the climate. Lava cools into dark barren rock that weathers into very
  fertile volcanic soil. Animals flee lava like fire.
- Floods: heavy rain makes rivers burst their banks; the low ground around them is under water for a while
  (animals that can't swim get hurt and pushed out), then dries leaving fertile silt.
- Cells whose height changes get their biome, terrain, temperature and colour re-derived (and dried flood water now
  restores the proper ground instead of a generic green).
- New map view `V` -> tectonic plates: plates and faults (red collide, blue part, yellow slide). The cursor info
  shows lava, lava rock, floods, volcanoes and faults.
- `cargo run --release --example planet -- <ticks> <seed> <prefix>` runs a world and saves maps at the start and the
  end (plus an eruption close-up).
- Saves from 2.6 cannot be loaded.

## 2.6.1
- Faster on many-core CPUs: the single-threaded share of a simulation step is roughly halved.
  - Creature movement, growth and the energy bill run in parallel together with perception (each creature has its own
    random stream, so results do not depend on the thread count); only interactions (eating, hunting, mating, births)
    stay sequential.
  - Plant sampling and the search for a parent plant of a sprout run in parallel; droppings age in parallel.
  - Creatures are boxed (a turn moves a pointer instead of ~0.5 KB), no allocation per turn, one neighbourhood scan per
    meal, cheaper shoreline checks, camouflage refreshed every 40 ticks.
  - On 2 cores: +28% ticks/s; the sequential part per 40k ticks went from ~17 s to ~8.6 s.
- The world image is composed on all cores with a colour table instead of per-pixel HSV maths (full giant map
  31 -> 11 ms single-threaded); at x1000 and faster it is refreshed 12 times a second. Composing holds the simulation
  lock, so the simulation waits less.

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
