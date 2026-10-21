"""Local, world-space water cycle for EVOSIM's time-lapsed ecosystem.

Water units are gameplay quantities, not meteorological SI measurements.
Cloud liquid becomes falling particles, then soil/pool water or snow. Rendering
only reads this state and never advances it.
"""
from dataclasses import dataclass
import math


@dataclass(slots=True)
class Cloud:
    x: float
    y: float
    water: float
    radius: float = 15
    age: int = 0


@dataclass(slots=True)
class Drop:
    x: float
    y: float
    z: float
    water: float
    snow: bool


class Atmosphere:
    CELL = 16

    def __init__(self, width, height, rng):
        self.width, self.height, self.rng = width, height, rng
        self.cols = (width + self.CELL - 1) // self.CELL
        self.rows = (height + self.CELL - 1) // self.CELL
        self.vapor = [10.0] * (self.cols * self.rows)
        self.clouds, self.drops, self.splashes = [], [], []
        self.rain, self.shade, self.snow = {}, {}, {}
        self.wind = (.06, .015)
        self.evaporated = self.deposited = 0.0

    def key(self, x, y):
        return int(y) // self.CELL * self.cols + int(x) // self.CELL

    def rain_at(self, x, y):
        return min(1.0, self.rain.get(self.key(x, y), 0.0))

    def temperature(self, sim, i):
        shadow = self.shade.get(self.key(i % self.width, i // self.width), 0.0)
        return sim.tmp[i] / 255 + sim.dT - .035 * shadow - .025 * self.rain_at(i % self.width, i // self.width)

    def footprint(self, cloud):
        for gy in range(max(0, int(cloud.y-cloud.radius)//self.CELL), min(self.rows-1, int(cloud.y+cloud.radius)//self.CELL)+1):
            for gx in range(max(0, int(cloud.x-cloud.radius)//self.CELL), min(self.cols-1, int(cloud.x+cloud.radius)//self.CELL)+1):
                x = min(self.width-1, gx*self.CELL+self.CELL/2)
                y = min(self.height-1, gy*self.CELL+self.CELL/2)
                if math.hypot(x-cloud.x, y-cloud.y) <= cloud.radius+self.CELL/2:
                    yield gy*self.cols+gx

    def impact(self, sim, drop):
        x, y = int(drop.x), int(drop.y)
        i = y*self.width+x
        self.deposited += drop.water
        if drop.snow:
            self.snow[i] = self.snow.get(i, 0) + drop.water
            return
        self.rain[self.key(x, y)] = min(1.5, self.rain.get(self.key(x, y), 0) + drop.water*.15)
        self.splashes.append((x, y, 5))
        self.soak(sim, i, drop.water)

    def soak(self, sim, i, water):
        if i in sim.pools:
            sim.pools[i] = min(1800, sim.pools[i]+water*20)
        elif sim.terr[i] in (0, 3):
            sim.moisture[i] = min(255, sim.moisture[i]+max(1, int(water*6)))
            ns = list(sim.neighbors(i))
            if sim.moisture[i] >= 225 and all(sim.elevation[i] <= sim.elevation[j] for j in ns):
                sim.make_water(i, temporary=True)
        if i in sim.fire:
            sim.fire[i] = max(1, sim.fire[i]-int(water*10))

    def step(self, sim):
        dt = getattr(sim, 'dt', 1)
        self.splashes = [(x, y, ttl-dt) for x, y, ttl in self.splashes if ttl > 1]
        rng = self.rng
        kind = sim.wx['kind']
        self.wind = (.065+.035*math.sin(sim.t/1500), .025*math.sin(sim.t/2100))
        self.rain = {k: v*.985**dt for k, v in self.rain.items() if v > .015}
        # Sample evaporation instead of walking every terrain cell each tick.
        for _ in range(12):
            i = rng.randrange(self.width*self.height)
            if sim.terr[i] == 2: continue
            amount = .6 if sim.terr[i] == 1 else .04 if sim.moisture[i] > 70 else 0
            amount *= dt*max(.2,.5+self.temperature(sim,i))
            if not amount: continue
            self.vapor[self.key(i%self.width, i//self.width)] += amount
            self.evaporated += amount
            if sim.terr[i] != 1: sim.moisture[i] = max(0, sim.moisture[i]-1)
            if i in sim.pools: sim.pools[i] -= amount*20
        if (sim.t//32) != ((sim.t-dt)//32):
            # Transport vapor conservatively with the prevailing wind.
            transported = self.vapor.copy()
            dx = 1 if self.wind[0] >= 0 else -1
            for k, water in enumerate(self.vapor):
                target = (k // self.cols)*self.cols + (k % self.cols+dx) % self.cols
                moved = water*.08
                transported[k] -= moved
                transported[target] += moved
            self.vapor = transported
        if (sim.t//8) != ((sim.t-dt)//8):
            # Warm/dry air requires more vapor before it condenses.
            threshold = 13 if kind in ('rain', 'storm', 'coldsnap', 'fog') else 23
            for _ in range(min(32, len(self.vapor))):
                k = rng.randrange(len(self.vapor))
                if self.vapor[k] > threshold and len(self.clouds) < max(12, self.width*self.height//3500):
                    liquid = self.vapor[k]*.8
                    self.vapor[k] -= liquid
                    self.clouds.append(Cloud(min(self.width-1, (k%self.cols+.5)*self.CELL), min(self.height-1, (k//self.cols+.5)*self.CELL), liquid, rng.uniform(10, 20)))
        self.shade = {}
        survivors = []
        for cloud in self.clouds:
            cloud.age += dt
            # Closed horizontal boundary retains water in this finite habitat.
            cloud.x = (cloud.x+self.wind[0]*dt) % self.width
            cloud.y = (cloud.y+self.wind[1]*dt) % self.height
            k = self.key(cloud.x, cloud.y)
            take = min(self.vapor[k], .09*dt)
            self.vapor[k] -= take
            cloud.water += take
            dry = kind in ('drought', 'heatwave')
            evaporation = min(cloud.water, (.04 if dry else .004)*dt)
            cloud.water -= evaporation
            self.vapor[k] += evaporation
            for key in self.footprint(cloud):
                self.shade[key] = min(1, self.shade.get(key, 0)+cloud.water/30)
            if cloud.water > 12 and rng.random() < 1-(1-(.28 if kind == 'storm' else .12))**dt:
                for _ in range(min(3*dt, int(cloud.water-12))):
                    x = min(self.width-.01, max(0, cloud.x+rng.uniform(-cloud.radius, cloud.radius)))
                    y = min(self.height-.01, max(0, cloud.y+rng.uniform(-cloud.radius*.5, cloud.radius*.5)))
                    snow = self.temperature(sim, int(y)*self.width+int(x)) < .33
                    self.drops.append(Drop(x, y, 12, 1, snow))
                    cloud.water -= 1
            if kind == 'storm' and cloud.water > 20 and rng.random() < 1-(1-.0004)**dt:
                sim.lightning(int(cloud.y)*self.width+int(cloud.x))
            if cloud.water > .2: survivors.append(cloud)
            else: self.vapor[k] += cloud.water
        self.clouds = survivors
        falling = []
        for drop in self.drops:
            drop.x = min(self.width-.01, max(0, drop.x+self.wind[0]*dt))
            drop.y = min(self.height-.01, max(0, drop.y+self.wind[1]*dt))
            drop.z -= dt*(.35 if drop.snow else 1.5)
            if drop.z <= 0: self.impact(sim, drop)
            else: falling.append(drop)
        self.drops = falling
        if (sim.t//20) != ((sim.t-dt)//20):
            for i in list(self.snow):
                if self.temperature(sim, i) <= .33: continue
                melt = min(self.snow[i], .5)
                self.snow[i] -= melt
                self.soak(sim, i, melt)
                if self.snow[i] <= 0: del self.snow[i]
