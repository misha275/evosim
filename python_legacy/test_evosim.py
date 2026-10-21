"""Headless integration checks: python3 -m unittest -v."""
import os
os.environ['SDL_VIDEODRIVER'] = 'dummy'
os.environ['SDL_AUDIODRIVER'] = 'dummy'
import time
import unittest
import pygame
import evosim as e


class EcosystemTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        pygame.init()
        e.rnd.seed(42)
        cls.sim = e.Sim(160, 120)

    @classmethod
    def tearDownClass(cls):
        pygame.quit()

    def test_genes_survive_reproduction(self):
        a, b = e.base_genome('herb'), e.base_genome('carn')
        for _ in range(100):
            child, _ = e.gamete(a, b)
            phenotype = e.phenotype(child, b)
            for key, (lo, hi) in e.G.items():
                self.assertTrue(lo <= phenotype[key] <= hi, key)
            self.assertIn('scavenge', child)

    def test_corpse_and_waste_cycle(self):
        sim = self.sim
        c = sim.crs[0]
        sim.kill(c)
        corpse = sim.cor[-1]
        self.assertGreater(corpse.e, 0)
        self.assertEqual(corpse.size, c.s)
        i = next(i for i,t in enumerate(sim.terr) if t == 0)
        sim.setp(i, 0)
        before = sim.fert[i]
        sim.poop.append([i%e.W+.5, i//e.W+.5, 1, False, None])
        corpse.ttl = 1
        sim.step()
        self.assertGreater(sim.fert[i], before)
        self.assertNotIn(corpse, sim.cor)

    def test_pool_drains_and_releases_land(self):
        sim = self.sim
        i = next(i for i,t in enumerate(sim.terr) if t == 0)
        land = sim.land
        sim.make_water(i, temporary=True)
        self.assertEqual(sim.terr[i], 1)
        self.assertEqual(sim.land, land-1)
        sim.pools[i] = 1
        sim.wx.update(kind='drought', **{'int':1.0})
        sim.t = (sim.t//20+1)*20
        sim.water_step()
        self.assertEqual(sim.terr[i], 0)
        self.assertEqual(sim.land, land)
        self.assertNotIn(i, sim.pools)

    def test_render_weather_and_close_detail(self):
        sim = self.sim
        screen = pygame.display.set_mode((e.MAPW+e.PANEL, e.MAPH))
        fonts = (pygame.font.Font(None, 15), pygame.font.Font(None, 11))
        sim.earthquake(sim.crs[0].x, sim.crs[0].y, 20)
        sim.lightning()
        sim.sel = sim.crs[-1]
        for zoom in (2, 8, 14):
            view = e.new_view()
            view.update(z=zoom, x=sim.sel.x-40, y=sim.sel.y-30)
            e.clamp_view(view)
            for kind in ('clear', 'rain', 'storm', 'coldsnap', 'fog'):
                sim.wx.update(kind=kind, **{'int':1.0})
                e.render(sim, screen, pygame.Surface((e.W,e.H)), pygame.Surface((e.W,e.H),pygame.SRCALPHA), fonts, 80, False, view, e.new_tv())
        pygame.image.save(screen, '/tmp/evosim-detail-test.png')

    def test_rain_impacts_are_local(self):
        from weather import Atmosphere, Drop
        sim = self.sim
        atmosphere = Atmosphere(e.W, e.H, e.rnd)
        i = next(i for i,t in enumerate(sim.terr) if t == 0)
        x, y = i%e.W, i//e.W
        sim.moisture[i] = 100
        atmosphere.impact(sim, Drop(x+.5, y+.5, 0, 2, False))
        self.assertEqual(sim.moisture[i], 112)
        self.assertGreater(atmosphere.rain_at(x, y), 0)
        self.assertEqual(atmosphere.rain_at((x+40)%e.W, (y+40)%e.H), 0)
        self.assertEqual(atmosphere.deposited, 2)

    def test_snow_accumulates_then_melts(self):
        from weather import Atmosphere, Drop
        sim = self.sim
        atmosphere = Atmosphere(e.W, e.H, e.rnd)
        i = next(i for i,t in enumerate(sim.terr) if t == 0)
        x, y = i%e.W, i//e.W
        old_temp, old_dt = sim.tmp[i], sim.dT
        sim.tmp[i], sim.dT = 0, 0
        atmosphere.impact(sim, Drop(x+.5, y+.5, 0, 1, True))
        self.assertEqual(atmosphere.snow[i], 1)
        sim.t = (sim.t//20+1)*20
        atmosphere.step(sim)
        self.assertEqual(atmosphere.snow[i], 1)
        sim.tmp[i] = 255
        atmosphere.step(sim)
        self.assertEqual(atmosphere.snow[i], .5)
        sim.tmp[i], sim.dT = old_temp, old_dt

    def test_cloud_water_becomes_rain_and_render_is_read_only(self):
        from weather import Atmosphere, Cloud
        import random
        sim = self.sim
        original = sim.atmosphere
        sim.atmosphere = atmosphere = Atmosphere(e.W, e.H, random.Random(4))
        atmosphere.clouds.append(Cloud(70, 60, 40))
        sim.wx.update(kind='storm')
        start_x = atmosphere.clouds[0].x
        for _ in range(60):
            atmosphere.step(sim)
        self.assertGreater(atmosphere.clouds[0].x, start_x)
        self.assertGreater(atmosphere.deposited, 0)
        self.assertTrue(atmosphere.shade)
        before = repr((atmosphere.clouds, atmosphere.drops, atmosphere.vapor, atmosphere.rain))
        surface = pygame.Surface((e.W,e.H))
        e.draw_pixel_weather(sim, surface)
        e.draw_pixel_weather(sim, surface)
        self.assertEqual(before, repr((atmosphere.clouds, atmosphere.drops, atmosphere.vapor, atmosphere.rain)))
        sim.atmosphere = original

    def test_save_roundtrip_preserves_references_and_future(self):
        import tempfile
        from pathlib import Path
        sim = self.sim
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'world.evo'
            e.save_world(path, sim, e.new_view(), 6, True)
            saved_tick = sim.t
            sim.step()
            expected = [(c.id, c.x, c.y, c.e) for c in sim.crs]
            expected_rng = e.rnd.getstate()
            loaded, view, speed, paused = e.load_world(path)
            self.assertEqual((loaded.t, speed, paused), (saved_tick, 6, True))
            self.assertIs(loaded.atmosphere.rng, e.rnd)
            for c in loaded.crs:
                self.assertIs(c.sp, loaded.species[c.sp.id])
            loaded.step()
            self.assertEqual([(c.id, c.x, c.y, c.e) for c in loaded.crs], expected)
            self.assertEqual(e.rnd.getstate(), expected_rng)
            self.assertEqual(pygame.image.tobytes(loaded.bg, 'RGB'), pygame.image.tobytes(sim.bg, 'RGB'))
            self.sim = loaded

    def test_failed_save_preserves_existing_file(self):
        import tempfile
        from pathlib import Path
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'world.evo'
            e.save_world(path, self.sim, e.new_view())
            before = path.read_bytes()
            with patch('savegame.SnapshotPickler.dump', side_effect=OSError('disk full')):
                with self.assertRaises(OSError): e.save_world(path, self.sim, e.new_view())
            self.assertEqual(path.read_bytes(), before)
            self.assertEqual(list(Path(directory).glob('*.tmp')), [])
            path.write_bytes(b'damaged save')
            with self.assertRaises(ValueError): e.load_world(path)

    def test_hyperspeed_runs_without_new_render_requests(self):
        class Counter:
            def __init__(self): self.t = 0
            def step(self, ticks=1): self.t += ticks
        sim = Counter()
        worker = e.SimulationWorker()
        try:
            worker.submit(sim, 10000)
            deadline = time.monotonic()+2
            while sim.t < 20000 and time.monotonic() < deadline: time.sleep(.005)
            worker.pause()
            self.assertGreater(sim.t, 10000)
            stopped = sim.t
            time.sleep(.03)
            self.assertEqual(sim.t, stopped)
        finally: worker.close()

    def test_worker_advances_and_stops(self):
        worker = e.SimulationWorker()
        try:
            before = self.sim.t
            worker.submit(self.sim, 10)
            deadline = time.monotonic()+5
            while time.monotonic() < deadline:
                with worker.lock:
                    if self.sim.t > before: break
                time.sleep(.01)
            with worker.lock:
                self.assertIsNone(worker.error)
                self.assertGreater(self.sim.t, before)
        finally:
            worker.close()
        self.assertFalse(worker.thread.is_alive())



class EvolutionTests(unittest.TestCase):
    def setUp(self):
        pygame.init()
        e.rnd.seed(81)
        self.sim = e.Sim(160, 120, primordial=True)

    def test_primordial_founders_and_fission(self):
        sim = self.sim
        self.assertTrue(all(e.stage(c.g) == 'microbial colony' for c in sim.crs))
        self.assertTrue(all(sim.terr[int(c.y)*e.W+int(c.x)] == 1 for c in sim.crs))
        c = sim.crs[0]
        c.age, c.cool, c.e = int(c.g['mat'])+1, 0, c.maxE
        before = len(sim.crs)
        sim.t += 1
        sim.live(c)
        self.assertEqual(len(sim.crs), before+1)
        self.assertEqual(sim.crs[-1].gen, c.gen+1)
        self.assertEqual(sim.crs[-1].hap[0]['epoch'], 1)

    def test_innovations_require_generations(self):
        g = e.base_genome('herb')
        for _ in range(80):
            child, note = e.mutate(g, 3)
            self.assertEqual(child['wings'], 0)
            self.assertEqual(child['fins'], 0)
        g['epoch'] = 20
        values = [e.mutate(g, 3)[0]['wings'] for _ in range(80)]
        self.assertGreater(max(values), 0)

    def test_flight_is_functional_and_costs_energy(self):
        sim = self.sim
        g = e.base_genome('herb')
        g.update(wings=1., lungs=1., complexity=1., size=3., armor=0., mat=300.)
        sp = sim.newsp(None, g)
        c = sim.spawn(g, dict(g), sp, None, g=dict(g))
        c.age, c.e = 400, c.maxE
        sim.t += 1
        sim.live(c)
        self.assertTrue(e.can_fly(c.g))
        self.assertGreater(c.z, 0)
        self.assertLess(c.e, c.maxE)

    def test_hyperstep_advances_age_weather_and_decay(self):
        sim = self.sim
        c = sim.crs[0]
        c.cool = 100
        old_age, old_tick, old_weather = c.age, sim.t, sim.wx['ttl']
        sim.step(8)
        self.assertEqual(sim.t, old_tick+8)
        self.assertEqual(c.age, old_age+8)
        self.assertEqual(c.cool, 92)
        self.assertEqual(sim.wx['ttl'], old_weather-8)
        self.assertTrue(sim.every(3))

    def test_plant_descendants_inherit_and_habitats_select(self):
        veg = self.sim.vegetation
        land = next(i for i,t in enumerate(self.sim.terr) if t == 0)
        water = next(i for i,t in enumerate(self.sim.terr) if t == 1)
        veg.ids[land], veg.ids[water] = 1, 0
        self.assertEqual(veg.suitability(land,128,.5,True), 0)
        self.assertGreater(veg.suitability(water,255,.5,True), 0)
        for tick in range(300):
            child = veg.seed(land, land, tick)
        self.assertGreater(child.generation, 0)
        self.assertIsNotNone(child.parent)
        self.assertTrue(0 <= child.height <= 1)
        self.assertTrue(0 <= child.wood <= 1)

if __name__ == '__main__':
    unittest.main()
