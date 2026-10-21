"""Plant lineages inherit and mutate during seed dispersal; the habitat selects them."""
from array import array
from dataclasses import dataclass, replace


@dataclass(slots=True)
class PlantLineage:
    id: int
    parent: object
    born: int
    generation: int
    height: float
    wood: float
    moisture: float
    temperature: float
    aquatic: float
    dispersal: float
    defense: float
    hue: float
    # Heritable architecture (drawn as tree shape, also affects ecology).
    crown: float = .5        # 0 columnar/conical -> 1 broad umbrella
    droop: float = .2        # weeping branches
    density: float = .6      # foliage density (shades neighbours, costs growth)
    branching: float = .5    # number/spread of limbs
    girth: float = .3        # trunk thickness (fire/grazing tolerance)
    bloom: float = .1        # flowers/fruit, attract dispersers
    bloom_hue: float = 330.

    def __getstate__(self):
        return {k: getattr(self, k) for k in self.__dataclass_fields__}

    def __setstate__(self, state):
        if not isinstance(state, dict):
            state = dict(zip(self.__dataclass_fields__, state))
        for k, f in self.__dataclass_fields__.items():
            object.__setattr__(self, k, state.get(k, f.default))

    @property
    def form(self):
        if self.aquatic > .65: return 'algae' if self.height < .5 else 'kelp'
        if self.wood > .55 and self.height > .6: return 'tree'
        if self.wood > .3 and self.height > .3: return 'shrub'
        return 'biofilm' if self.height < .15 and self.wood < .1 else 'moss' if self.height < .15 else 'grass'


MUTABLE = ('height','wood','moisture','temperature','aquatic','dispersal','defense','hue',
           'crown','droop','density','branching','girth','bloom','bloom_hue')
SHAPE = ('crown','droop','density','branching','girth','bloom','bloom_hue','hue','height')


class Vegetation:
    def __init__(self, width, height, rng, primordial=False):
        self.width, self.height, self.rng = width, height, rng
        self.ids = array('H', [0])*(width*height)
        self.lineages = [
            PlantLineage(0,None,0,0,.08,.0,.9,.5,1.,.3,.0,145),
            PlantLineage(1,None,0,0,.1 if primordial else .22,.02,.55,.55,.0,.35,.03,105),
        ]
        self.trees = set()
        self.counts,self.peaks,self.died = [0,0],[0,0],[None,None]

    def lineage(self, i):
        return self.lineages[self.ids[i]]

    def seed(self, source, target, tick):
        parent = self.lineage(source)
        child = parent
        if self.rng.random() < .16 and len(self.lineages) < 65535:
            changes = {}
            # Trees mutate their architecture more often, so forests diversify visibly.
            woody = parent.wood > .3
            for key in MUTABLE:
                if self.rng.random() < (.45 if woody and key in SHAPE else .35):
                    hue = key in ('hue','bloom_hue')
                    scale = 14 if hue else .13
                    value = getattr(parent,key)+self.rng.gauss(0,scale)
                    changes[key] = value % 360 if hue else min(1,max(0,value))
            if changes:
                child = replace(parent, id=len(self.lineages), parent=parent.id, born=tick, generation=parent.generation+1, **changes)
                self.lineages.append(child)
                self.counts.append(0); self.peaks.append(0); self.died.append(None)
        self.ids[target] = child.id
        return child

    def crowding(self, i, pl, w, h):
        """Janzen-Connell effect: specialist pests punish dense stands of one lineage,
        giving rare variants an advantage and keeping the flora diverse."""
        own, x, y, same = self.ids[i], i % w, i // w, 0
        for dx, dy in ((-2,0),(2,0),(0,-2),(0,2),(-3,-3),(3,3),(-3,3),(3,-3)):
            nx, ny = x+dx, y+dy
            if 0 <= nx < w and 0 <= ny < h:
                j = ny*w+nx
                if pl[j] and self.ids[j] == own: same += 1
        return same/8

    def suitability(self, i, wet, temperature, water):
        p = self.lineage(i)
        if water and p.aquatic < .5: return 0
        if not water and p.aquatic > .75: return 0
        moisture = 1 if water else wet/255
        return max(0, 1-abs(moisture-p.moisture)*1.5) * max(0,1-abs(temperature-p.temperature)*2)

    def track(self, i, growth):
        if growth >= 2 and self.lineage(i).form in ('tree','shrub','kelp'): self.trees.add(i)
        else: self.trees.discard(i)
