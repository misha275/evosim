# EVOSIM

A pixel ecosystem with inheritance, evolving body plans and plant lineages, local weather, decomposition and disease.

```sh
python3 -m pip install pygame-ce
python3 evosim.py
```

The welcome screen offers a new primordial world, continuing the autosave, loading the quicksave, or the animal sandbox. Use left/right to choose a map size. Primordial worlds contain microbial colonies and photosynthetic biofilms/algae. Every later animal must descend from those colonies; animal immigration and founder spawning are disabled. Extinction ends that world's living lineages until you restart.

Direct launch options:

```sh
python3 evosim.py --primordial --world large
python3 evosim.py --animals --world small
python3 evosim.py --load saves/quicksave.evo
```

Colonies initially reproduce by fission. Complexity, respiration, swimming limbs, wings, immunity and burial behavior are inherited and mutate over generations. New organs become accessible after lineage generations, and functional flight/swimming require compatible anatomy. Evolution is stochastic: advanced forms are possible, not guaranteed or spawned on a schedule. Microbe icons represent colonies; this is a time-lapsed ecological model, not a microscopic fluid simulator.

Plant seeds inherit height, woodiness, climate preferences, dispersal and defenses. Habitat conditions select which descendants establish; woody shrubs and trees can emerge. Trees also inherit their architecture — crown shape (columnar/conical to umbrella), weeping branches, foliage density, branching, trunk girth and flowers/fruit with their own colour — and are drawn from those genes. Dense stands of one lineage suffer from specialist pests (Janzen–Connell effect), so rare variants establish more easily and forests stay mixed. `T` opens phylogeny; tabs or `Tab` switch between all creatures, microbial life, animals and plants. Hover or pin branches to inspect them, and press Enter to visit a living member.

Wind transports moisture and clouds. Actual falling particles wet the soil, fill pools, suppress fires and accumulate as snow. Corpses can carry pathogens; older animal remains leave bones. Outbreaks, resistance and later burial behavior affect survival.

## Controls

- `1`–`6`: normal speeds; `7`: hyperspeed, using eight-tick integration and reduced rendering. This advances metabolism, aging, weather and reproduction together, with less timing precision. The display reports measured ticks per second.
- `Space`: pause. `R`: restart the current mode. `N`: next map size.
- `F5`: quicksave. `F9`: load quicksave, or autosave if no quicksave exists.
- Autosave every 60 real seconds and on normal exit, to `saves/autosave.evo`. Quicksave is separate. Snapshots include RNG state, organisms, ancestry, terrain, plants and weather. Writes are atomic.
- Left click: inspect; drag: pan; wheel or `+`/`-`: zoom; arrows: pan; `0`: fit map.
- Right click: seeds. `H`/`C`: herbivore/carnivore founders in the animal sandbox.
- `S`: screenshot. `Esc`: close phylogeny or exit.

The default map is 1600×1060 cells (160×106 m); presets range from 340×230 to 2048×1360 (`colossal`). The original creature pixel art is retained; acquired wings and fins use its existing pixel grid.

## Verification

```sh
python3 -m unittest -v
python3 evosim.py --animals --world small --bench 1000 /tmp/evosim.png
```
