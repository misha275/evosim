#!/usr/bin/env python3
"""
EVOSIM - pixel ecosystem: plants, growing creatures, sexual reproduction, predation,
seed-pooping, mutation, quirks, speciation + live phylogenetic tree.

    pip install pygame
    python evosim.py

Controls: LMB click inspect / drag pan | wheel or +/- zoom | arrows pan | 0 reset view | RMB drop seeds | H/C spawn herbivore/carnivore founders at cursor
          F5 save | F9 load | autosave every 60s and on exit
          T or click tree = fullscreen phylogeny | 1-7 speed (7 = megaspeed) | SPACE pause | R new world | N next world size | S screenshot | ESC quit
    python evosim.py --world small|medium|large|480x320
"""
import math, random, sys, os, time, threading
from collections import deque
from weather import Atmosphere
from evolution import TRAITS, DEFAULTS, INNOVATION_GENERATION, stage, can_swim, aquatic, can_fly, habitat
from vegetation import Vegetation
from pathlib import Path
from savegame import write_snapshot, read_snapshot

def _ensure_pygame():
    """Auto-fix: get a working pygame no matter what (venv, Windows, brand-new Python, no compiler)."""
    try:
        import pygame  # noqa
        return
    except ImportError:
        pass
    import subprocess, shutil, re

    def pip(py, pkg, extra=()):
        # binary wheels only: never try to compile pygame from source (needs SDL headers / MSVC)
        r = subprocess.run([py, '-m', 'pip', 'install', '--disable-pip-version-check', '--only-binary=:all:', pkg, *extra],
                           stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        return r.returncode == 0, r.stdout

    def try_install(py):
        invenv = getattr(sys, 'base_prefix', sys.prefix) != sys.prefix if py == sys.executable else False
        scopes = [[], ['--break-system-packages']] if invenv else [[], ['--user'], ['--break-system-packages'], ['--user', '--break-system-packages']]
        for pkg, more in (('pygame-ce', []), ('pygame-ce', ['--pre']), ('pygame', [])):
            for sc in scopes:
                print('[evosim] %s: pip install %s %s' % (os.path.basename(os.path.dirname(py)) or py, pkg, ' '.join(sc + more)))
                ok, out = pip(py, pkg, sc + more)
                if ok: return True
        return False

    if try_install(sys.executable):
        import importlib
        importlib.invalidate_caches()
        try:
            import pygame  # noqa
            return
        except ImportError:
            pass
    print('[evosim] pygame will not install on this Python (%d.%d). Looking for another Python...' % sys.version_info[:2])

    def other_pythons():
        found = []
        if os.name == 'nt' and shutil.which('py'):
            out = subprocess.run(['py', '-0p'], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True).stdout
            found += re.findall(r'([A-Za-z]:\\[^\r\n]*?python[w]?\.exe)', out)
        for v in ('3.13', '3.12', '3.11', '3.10'):
            w = shutil.which('python' + v)
            if w: found.append(w)
        seen, res = set(), []
        for p_ in found:
            k = os.path.normcase(os.path.abspath(p_))
            if k not in seen and k != os.path.normcase(os.path.abspath(sys.executable)) and 'WindowsApps' not in p_:
                seen.add(k); res.append(p_)
        return res

    def relaunch(py):
        print('[evosim] restarting under %s' % py)
        sys.exit(subprocess.call([py, os.path.abspath(__file__)] + sys.argv[1:]))

    def has_pygame(py):
        return subprocess.run([py, '-c', 'import pygame'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0

    for py in other_pythons():
        if has_pygame(py) or try_install(py): relaunch(py)
    if os.name == 'nt' and shutil.which('winget'):
        try:
            ans = input('[evosim] Install Python 3.13 (has ready-made pygame) with winget now? [Y/n] ').strip().lower()
        except EOFError:
            ans = 'n'
        if ans in ('', 'y', 'yes'):
            subprocess.call(['winget', 'install', '-e', '--id', 'Python.Python.3.13', '--silent',
                             '--accept-package-agreements', '--accept-source-agreements'])
            for py in other_pythons():
                if '313' in py.replace('.', '').replace('-', '') or '3.13' in py:
                    if try_install(py): relaunch(py)
            sys.exit('[evosim] Python 3.13 installed - open a NEW terminal and run:  py -3.13 evosim.py')
    sys.exit('[evosim] Could not get pygame. Use Python 3.10-3.14 with:  python -m pip install pygame-ce   (or: py -3.13 evosim.py)')


_ensure_pygame()
import pygame  # noqa: E402

PANEL = 400
MAPW, MAPH = 1020, 690          # map viewport (screen pixels); the world can be bigger than this - zoom/pan
DAY = 2400
SEASON = DAY * 6
CH, CG = 8, 16
CSCALE = 0.8                    # sprite size (1.0 = chunky); logic unchanged
ZMIN, ZMAX = 0.8, 14.0
# ---- physical scale -------------------------------------------------------------------------
CELL_CM = 10.0                   # one cell = 10 cm; adults ~25-80 cm, default habitat = 96 x 64 m
TICK_S = 0.2                    # movement clock: 1 tick = 0.2 s  (life cycles are time-lapsed so evolution is watchable)
MK = 0.0032                     # mass[kg] = MK * length_in_cells**3 (a 50 cm creature ~0.4 kg)
KREST, KMOVE = 0.14, 0.12       # Kleiber's law: resting energy cost ~ mass**0.75 ; locomotion ~ mass**0.75 * v**2
SPEC_T = 0.26                   # genetic distance that starts a new species
WORLDS = {'small': (340, 230), 'medium': (480, 320), 'large': (640, 420), 'huge': (960, 640), 'vast': (1280, 840), 'giant': (1600, 1060), 'colossal': (2048, 1360)}
W = H = N = CWc = CHc = GWc = GHc = PS = MAXPOP = 0


def set_world(w, h):
    global W, H, N, CWc, CHc, GWc, GHc, PS, MAXPOP
    W, H = int(w), int(h)
    N = W * H
    CWc, CHc = (W + CH - 1) // CH, (H + CH - 1) // CH
    GWc, GHc = (W + CG - 1) // CG, (H + CG - 1) // CG
    PS = max(300, N // 110)                          # plant growth samples per tick
    MAXPOP = max(120, int(min(800, 300 * N / 78200)))


set_world(*WORLDS['giant'])
rnd = random.Random()
rr = rnd.random

# ---------------------------------------------------------------- genetics (diploid)
G = {'spd': (0.35, 1.3), 'size': (.1, 8.0), 'vis': (10, 50), 'diet': (0, 1), 'metab': (0.6, 1.5),
     'thresh': (0.55, 0.9), 'life': (2500, 7000), 'aggr': (0.3, 1.0), 'mut': (0.03, 0.22),
     'mat': (300, 1100), 'litter': (1, 3), 'tol': (.1, .9), 'care': (0, 1), 'choosy': (0, 1),
     'bold': (0, 1), 'social': (0, 1), 'curio': (0, 1), 'immune': (0, 1),
     'body': (.5, .95), 'ears': (0, 1), 'horn': (0, 1), 'tail': (0, 1), 'legs': (0, 1), 'eyes': (0, 1), 'scavenge': (0, 1), 'fur': (0, 1), 'armor': (0, 1)}
G.update(TRAITS)
DK = ['spd', 'size', 'vis', 'diet', 'metab', 'thresh', 'life', 'aggr', 'tol', 'complexity', 'wings', 'gills', 'fins', 'lungs', 'burial']
VK = ['body', 'ears', 'horn', 'tail', 'legs', 'eyes', 'fur', 'armor']
LG = [['spd', 'size', 'metab', 'life', 'mat'], ['vis', 'diet', 'aggr', 'thresh', 'tol', 'bold', 'social', 'curio'],
      ['litter', 'care', 'choosy', 'mut', 'immune', 'scavenge'], VK, ['hue', 'hue2', 'pat'], list(TRAITS)]       # linkage groups (inherited together)
QUIRKS = {'Nocturnal': (255, 225, 90), 'Herder': (120, 170, 255), 'Swimmer': (60, 220, 230),
          'Spiky': (240, 240, 240), 'Skittish': (255, 130, 190), 'Scavenger': (190, 110, 230),
          'Glutton': (255, 160, 60), 'Camouflage': (130, 190, 120), 'Venomous': (110, 230, 90),
          'Thick-hide': (170, 140, 110), 'Hibernator': (170, 200, 255), 'Prolific': (255, 170, 200)}
QN = list(QUIRKS)
QDOM = set(QN) - {'Nocturnal', 'Hibernator', 'Prolific'}      # dominant alleles; the rest need two copies to show
UP = {'spd': 'faster', 'size': 'bigger', 'vis': 'keener eyes', 'diet': 'meat-eater', 'metab': 'hot-blooded',
      'thresh': 'late breeder', 'life': 'long-lived', 'aggr': 'bolder', 'tol': 'warm-adapted'}
DN = {'spd': 'slower', 'size': 'smaller', 'vis': 'dim eyes', 'diet': 'leaf-eater', 'metab': 'cold-blooded',
      'thresh': 'early breeder', 'life': 'short-lived', 'aggr': 'meeker', 'tol': 'cold-adapted'}
UP.update(complexity='complex body', wings='wing growth', gills='gill breathing', fins='swimming limbs', lungs='air breathing', burial='burial behavior')
DN.update(complexity='simpler body', wings='reduced wings', gills='reduced gills', fins='reduced fins', lungs='reduced lungs', burial='reduced burial behavior')
UP.update(body='rounder', ears='bigger ears', horn='horns', tail='longer tail', legs='longer legs', eyes='bigger eyes')
UP.update(fur='thicker fur', armor='armored', scavenge='corpse-eater')
DN.update(fur='thinner fur', armor='softer hide', scavenge='less scavenging')
DN.update(body='sleeker', ears='smaller ears', horn='hornless', tail='shorter tail', legs='stubby legs', eyes='smaller eyes')
SYL = ['ka', 'zo', 'mi', 'ru', 'ta', 've', 'lo', 'an', 'ix', 'po', 'su', 'dre', 'gal', 'nem', 'quo', 'fen', 'bri', 'yu']


def base_genome(kind):
    g = {k: rnd.uniform(lo, hi) for k, (lo, hi) in G.items()}
    g.update(litter=rnd.uniform(1.2, 2.6), mut=rnd.uniform(.07, .14), mat=rnd.uniform(400, 800),
             life=rnd.uniform(3500, 6000), hue=rnd.uniform(0, 360), pat=rnd.randrange(6), q=set(),
             tol=rnd.uniform(.3, .8), care=rnd.uniform(.1, .7), choosy=rnd.uniform(0, .6),
             bold=rnd.uniform(.25, .75), social=rnd.uniform(.2, .8), curio=rnd.uniform(.2, .8), immune=rnd.uniform(.15, .5))
    g.update(DEFAULTS, epoch=0)
    g['scavenge'] = rnd.uniform(0, .12)  # corpse specialists must evolve
    g['hue2'] = (g['hue'] + rnd.uniform(-90, 90)) % 360
    if kind == 'microbe':
        g.update(complexity=.08, wings=0., gills=.9, fins=0., lungs=0., size=.16,
                 diet=.03, mat=300., life=3500., care=0., armor=0., fur=0., spd=.35)
    elif kind == 'herb':
        g.update(diet=rnd.uniform(0, .15), size=rnd.uniform(3, 5.5))
    elif kind == 'omni':
        g.update(diet=rnd.uniform(.45, .6), size=rnd.uniform(4, 6))
    else:
        g.update(diet=rnd.uniform(.85, 1), size=rnd.uniform(4.8, 7), spd=rnd.uniform(.8, 1.2),
                 vis=rnd.uniform(28, 50), aggr=rnd.uniform(.6, 1))
    return g


def clampg(k, v):
    lo, hi = G[k]
    return min(hi, max(lo, v))


def mutate(g, boost=1.0):
    """Mutate ONE haplotype (a gamete). Point mutations, rare macro-mutations, quirk gain/loss, color drift."""
    c = dict(g)
    c['q'] = set(g['q'])
    m = g['mut'] * boost
    best, bk, vbest, vk = 0, None, 0, None
    for k, (lo, hi) in G.items():
        if k in INNOVATION_GENERATION and g.get('epoch', 0) < INNOVATION_GENERATION[k]: continue
        if rr() < .3:
            old = c[k]
            span = max(.1, old * .8) if k == 'size' and g.get('complexity', .8) < .42 else hi-lo
            c[k] = clampg(k, old + rnd.gauss(0, m * (.5 if k == 'mut' else 1)) * span)
            rel = (c[k] - old) / (hi - lo)
            if k in DK and abs(rel) > abs(best): best, bk = rel, k
            elif k in VK and abs(rel) > abs(vbest): vbest, vk = rel, k
    note = (UP if best > 0 else DN)[bk] if bk and abs(best) > .07 else ''
    if not note and vk and abs(vbest) > .12: note = (UP if vbest > 0 else DN)[vk]
    if rr() < .008 * boost:                                   # macro-mutation
        eligible = [k for k in DK if k not in INNOVATION_GENERATION or g.get('epoch',0) >= INNOVATION_GENERATION[k]]
        if g.get('complexity', .8) < .42: eligible.remove('size')
        k = rnd.choice(eligible)
        lo, hi = G[k]
        d = rnd.choice((-1, 1)) * rnd.uniform(.3, .5) * (hi - lo)
        c[k] = clampg(k, c[k] + d)
        note = '!' + (UP if d > 0 else DN)[k]
    c['hue'] = (g['hue'] + rnd.gauss(0, 4)) % 360
    c['hue2'] = (g['hue2'] + rnd.gauss(0, 6)) % 360
    if rr() < .05: c['pat'] = rnd.randrange(6)
    if rr() < .02 * m / .1:
        nq = rnd.choice(QN)
        if nq not in c['q']:
            c['q'].add(nq)
            note = '+' + nq
    for q in list(c['q']):
        if rr() < .006:
            c['q'].discard(q)
            note = '-' + q
    return c, note


def gamete(h1, h2):
    """Meiosis: recombine the two parental haplotypes (linkage groups travel together), then mutate."""
    g = {}
    for grp in LG:
        a, b = (h1, h2) if rr() < .5 else (h2, h1)
        for k in grp: g[k] = a[k] if rr() > .06 else b[k]
    g['epoch'] = max(h1.get('epoch',0), h2.get('epoch',0))+1
    g['q'] = {qn for qn in QN if (qn in h1['q'] and qn in h2['q']) or ((qn in h1['q'] or qn in h2['q']) and rr() < .5)}
    return mutate(g)


def phenotype(h1, h2):
    """Alleles -> body. Continuous traits are additive + developmental noise; quirks are dominant or recessive."""
    g = {}
    for k, (lo, hi) in G.items():
        g[k] = min(hi, max(lo, (h1[k] + h2[k]) / 2 * (1 + rnd.gauss(0, .025))))
    for k in ('hue', 'hue2'):
        d = ((h2[k] - h1[k] + 180) % 360) - 180
        g[k] = (h1[k] + d / 2) % 360
    g['epoch'] = max(h1.get('epoch',0), h2.get('epoch',0))
    g['pat'] = h1['pat'] if rr() < .5 else h2['pat']
    g['q'] = {qn for qn in QN if (qn in h1['q'] and qn in h2['q']) or ((qn in h1['q'] or qn in h2['q']) and (qn in QDOM or rr() < .25))}   # recessives: incomplete penetrance
    return g


def dist(a, b):
    d = sum(abs(a[k] - b[k]) / (G[k][1] - G[k][0]) for k in DK) / len(DK)
    d += .3 * sum(abs(a[k] - b[k]) / (G[k][1] - G[k][0]) for k in VK) / len(VK)
    return d + .12 * len(a['q'] ^ b['q'])


def trait_note(g, ref):
    best, bk = 0, None
    for k in DK + VK:
        d = (g[k] - ref[k]) / (G[k][1] - G[k][0])
        if abs(d) > abs(best): best, bk = d, k
    gained = g['q'] - ref['q']
    if gained and abs(best) < .25: return '+' + next(iter(gained))
    return (UP if best > 0 else DN)[bk] if bk else 'drift'


def hsv(h, s, v):
    c = pygame.Color(0, 0, 0)
    c.hsva = (h % 360, max(0, min(100, s)), max(0, min(100, v)), 100)
    return (c.r, c.g, c.b)


def mix(a, b, t):
    return (int(a[0] + (b[0] - a[0]) * t), int(a[1] + (b[1] - a[1]) * t), int(a[2] + (b[2] - a[2]) * t))


def pname():
    return ''.join(rnd.choice(SYL) for _ in range(2)).capitalize()


class Sp:
    def __init__(s, id, parent, born, g, name):
        s.id, s.parent, s.born, s.died = id, parent, born, None
        s.founder, s.name, s.pop, s.peak, s.note = g, name, 0, 0, ''
        s.col = hsv(g['hue'], 65, 95)


WXCOL = {'rain': (130, 170, 255), 'storm': (170, 150, 255), 'fog': (190, 195, 205), 'drought': (240, 190, 90),
         'heatwave': (255, 130, 70), 'coldsnap': (160, 220, 255)}
DNAME = ('pox', 'flu', 'rot', 'fever', 'wilt', 'rash', 'cough')


class Strain:
    """A pathogen. Evolves as it spreads: transmissibility, virulence and host range (diet tropism) all drift."""
    def __init__(s, id, base, var, trans, vir, dur, incub, center, width, born, parent=None):
        s.id, s.base, s.var, s.trans, s.vir, s.dur, s.incub = id, base, var, trans, vir, dur, incub
        s.center, s.width, s.born, s.parent = center, width, born, parent
        s.cases, s.act, s.peak = 0, 0, 0
        s.name = base + (' v%d' % var if var > 1 else '')
        s.col = hsv(rnd.uniform(70, 330), 60, 95)


class Cr:
    __slots__ = ('sick', 'imm', 'fv', 'id', 'g', 'x', 'y', 'vx', 'vy', 'f', 'e', 'age', 's', 'maxE', 'cool', 'gen', 'sp', 'tgt',
                 'thr', 'dead', 'gut', 'wa', 'rest', 'col', 'dk', 'lt', 'c2', 'sex', 'preg', 'nut', 'pf', 'mom',
                 'nurse', 'hap', 'pers', 'nm', 'mk', 'mkc', 'kids', 'het', 'm34', 'z', 'breath')


class Co:
    __slots__ = ('x', 'y', 'e', 'ttl', 'dead', 'inf', 'size', 'col', 'buried', 'burial', 'bone', 'has_bones')


class Bo:
    __slots__ = ('x', 'y', 'size', 'ttl', 'buried')


# ---------------------------------------------------------------- noise / world
def fbm(w, h, scales, weights):
    out = [[0.0] * w for _ in range(h)]
    for sc, wt in zip(scales, weights):
        gw, gh = w // sc + 2, h // sc + 2
        g = [[rr() for _ in range(gw)] for _ in range(gh)]
        for y in range(h):
            fy = y / sc; y0 = int(fy); ty = fy - y0; ty = ty * ty * (3 - 2 * ty)
            r0, r1, row = g[y0], g[y0 + 1], out[y]
            for x in range(w):
                fx = x / sc; x0 = int(fx); tx = fx - x0; tx = tx * tx * (3 - 2 * tx)
                a = r0[x0] + (r0[x0 + 1] - r0[x0]) * tx
                b = r1[x0] + (r1[x0 + 1] - r1[x0]) * tx
                row[x] += (a + (b - a) * ty) * wt
    return out


class Sim:
    def __init__(s, w=None, h=None, primordial=False):
        s.primordial = primordial
        if w: set_world(w, h)
        s.reset()

    # ---- setup
    def reset(s):
        s.dt = 1
        s.t, s.nid, s.dark, s.dT, s.crowd = 0, 1, 0.0, 0.0, 1.0
        s.crs, s.cor, s.poop, s.hist, s.species, s.lin = [], [], [], [], [], {}
        s.log = deque(maxlen=50)
        s.sel = None
        s.cgrid = {}
        s.kgrid = {}
        s.quake = None
        s.fire, s.fchunk, s.ash = {}, {}, {}                      # burning cells (idx -> ticks left), chunk index, burnt ground
        s.wx = {'kind': 'clear', 'ttl': 900, 't0': 0, 'int': 0.0}
        s.wx_grow, s.wx_vis, s.wx_dT = 1.0, 1.0, 0.0
        s.dust, s.meteor, s.flash, s.banner, s.bolt = 0.0, None, 0.0, None, None
        s.strains, s.nsick = [], 0
        s.bones, s.environment_strain = [], None
        s.next_outbreak = rnd.randint(DAY//2,DAY)
        s.vegetation = Vegetation(W, H, rnd, s.primordial)
        s.milestones = set()
        s.mkworld()
        s.atmosphere = Atmosphere(W, H, rnd)
        area = N / 78200.0
        founders = (('microbe', 90),) if s.primordial else (('herb', 70), ('omni', 14), ('carn', 6))
        for kind, n in founders:
            b = base_genome(kind)
            sp = s.newsp(None, b)
            for _ in range(max(n, int(n * area ** .8))):
                s.spawn(mutate(b, .8)[0], mutate(b, .8)[0], sp, None, *s.near_prey(kind))

    def mkworld(s):
        sf = (W / 340) ** .7
        sc = lambda *a: tuple(max(3, int(v * sf)) for v in a)
        el = fbm(W, H, sc(48, 24, 12, 6), (.5, .3, .15, .05))
        mo = fbm(W, H, sc(40, 20, 8), (.55, .3, .15))
        tn = fbm(W, H, sc(60, 25), (.65, .35))
        s.elevation = bytearray(int(min(1, max(0, v))*255) for row in el for v in row)
        s.moisture = bytearray(int(min(1, max(0, v))*255) for row in mo for v in row)
        s.springs, s.pools = [], {}
        s.terr, s.fert, s.tmp = bytearray(N), [0.0] * N, bytearray(N)     # terr: 0 land 1 water 2 rock 3 sand
        s.bg = pygame.Surface((W, H))
        s.psurf = pygame.Surface((W, H), pygame.SRCALPHA)
        s.pl, s.pchunk, s.nplants, s.land = bytearray(N), {}, 0, 0
        for y in range(H):
            lat = y / H                                            # north (top) is cold, south is hot
            for x in range(W):
                i = y * W + x
                e, m, nz = el[y][x], mo[y][x], rnd.randint(-5, 5)
                T = min(1.0, max(.02, .12 + .74 * lat - .3 * max(0.0, e - .45) + (tn[y][x] - .5) * .2))
                s.tmp[i] = int(T * 255)
                suit = min(1.0, max(.05, 1.15 - 2.2 * abs(T - .55)))
                if e < .38:
                    t = e / .38
                    s.terr[i] = 1
                    col = (int(24 + 20 * t), int(70 + 60 * t), int(130 + 60 * t))
                    if T < .14: col = mix(col, (200, 225, 240), .75)
                elif e > .8:
                    s.terr[i] = 2
                    col = (104 + nz * 2, 104 + nz * 2, 116 + nz * 2)
                elif e < .42:
                    s.terr[i] = 3
                    s.fert[i] = .12 * suit
                    col = (196 + nz, 182 + nz, 130 + nz)
                    s.land += 1
                else:
                    f = min(1.0, max(.05, m * 1.7 - .35))
                    s.fert[i] = f * suit
                    s.land += 1
                    col = (int(128 - 82 * f) + nz, int(108 - 30 * f) + nz, int(72 - 16 * f) + nz)
                    if T < .3: col = mix(col, (226, 234, 244), min(1.0, (.3 - T) / .18) * .85)
                    elif T > .78: col = mix(col, (206, 170, 100), min(1.0, (T - .78) / .15) * .7)
                s.bg.set_at((x, y), tuple(max(0, min(255, v)) for v in col))
        s.form_sources()
        for _ in range(int(s.land * .04)):
            i = rnd.randrange(N)
            if s.terr[i] == 0 and s.fert[i] > .2: s.setp(i, 3)
            elif s.terr[i] == 1: s.setp(i, 3, lineage=0)

    def neighbors(s, i):
        x, y = i % W, i // W
        if x: yield i-1
        if x < W-1: yield i+1
        if y: yield i-W
        if y < H-1: yield i+W

    def make_water(s, i, temporary=False):
        if s.terr[i] == 2: return
        if s.terr[i] != 1:
            s.land -= 1
            s.setp(i, 0)
            s.terr[i] = 1
            s.bg.set_at((i % W, i // W), (43, 112, 155))
            if temporary: s.pools[i] = 900

    def form_sources(s):
        # Wet uplands feed streams. Descending channels erode until reaching a
        # lake, a local basin, or the edge; depressions retain spring water.
        for _ in range(max(3, N // 50000)):
            candidates = [rnd.randrange(N) for _ in range(120)]
            candidates = [i for i in candidates if s.terr[i] == 0 and s.moisture[i] > 110]
            if not candidates: continue
            i = max(candidates, key=lambda j: s.elevation[j])
            s.springs.append(i)
            seen = set()
            for _ in range(250):
                if i in seen: break
                seen.add(i)
                was_water = s.terr[i] == 1
                s.make_water(i)
                if was_water: break
                ns = [j for j in s.neighbors(i) if j not in seen and s.terr[j] != 2]
                if not ns: break
                j = min(ns, key=lambda j: s.elevation[j])
                if s.elevation[j] > s.elevation[i] + 2: break
                s.elevation[i] = max(0, s.elevation[i]-1)
                i = j

    def water_step(s):
        if not s.every(20): return
        # Sampled local moisture/erosion work scales independently of map area.
        for _ in range(100):
            i = rnd.randrange(N)
            if s.terr[i] in (1, 2): continue
            wet = s.atmosphere.rain_at(i % W, i // W) > .05
            near_water = any(s.terr[j] == 1 for j in s.neighbors(i))
            delta = 8 if wet else (2 if near_water else -1)
            s.moisture[i] = min(255, max(0, s.moisture[i]+delta))
            ns = list(s.neighbors(i))
            low = min(ns, key=lambda j: s.elevation[j])
            if wet and s.elevation[i] > s.elevation[low] + 6:
                s.elevation[i] -= 1
                s.elevation[low] = min(255, s.elevation[low]+1)
                s.fert[low] = min(1, s.fert[low]+.003)
            # Saturated depressions fill; adjacent water lets pools expand.
            basin = all(s.elevation[i] <= s.elevation[j] for j in ns)
            if wet and s.moisture[i] > 200 and (basin or near_water):
                s.make_water(i, temporary=True)
        for i in list(s.pools):
            wet = s.atmosphere.rain_at(i % W, i // W) > .05
            s.pools[i] += 20 if wet else -40 if s.wx['kind'] in ('drought', 'heatwave') else -10
            s.pools[i] = min(1800, s.pools[i])
            if s.pools[i] <= 0:
                del s.pools[i]
                s.terr[i] = 0
                s.land += 1
                s.fert[i] = max(.35, s.fert[i])
                s.bg.set_at((i % W, i // W), (80, 92, 56))
        # Rising water pushes land animals to nearby shores.
        for c in s.crs:
            i = int(c.y)*W+int(c.x)
            if s.terr[i] != 1 or can_swim(c.g) or c.z > 0: continue
            shores = [j for j in s.neighbors(i) if s.terr[j] in (0, 3)]
            if shores:
                j = rnd.choice(shores)
                c.x, c.y = j % W + .5, j // W + .5
            else: c.e -= c.maxE * .03

    def near_prey(s, kind):
        """Predators are placed among herbivores (they would starve in an empty corner)."""
        herb = [c for c in s.crs if c.g['diet'] < .45]
        if kind in ('herb', 'microbe') or not herb: return ()
        for _ in range(30):
            h = rnd.choice(herb)
            x, y = h.x + rnd.uniform(-14, 14), h.y + rnd.uniform(-14, 14)
            if s.ok(x, y, False): return (x, y)
        return ()

    def setp(s, i, st, lineage=None):
        old = s.pl[i]
        previous_lineage = s.vegetation.ids[i]
        if lineage is not None: s.vegetation.ids[i] = lineage
        elif not old and st: s.vegetation.ids[i] = 0 if s.terr[i] == 1 else 1
        current_lineage = s.vegetation.ids[i]
        veg = s.vegetation
        if old and (not st or previous_lineage != current_lineage):
            veg.counts[previous_lineage] -= 1
            if veg.counts[previous_lineage] == 0: veg.died[previous_lineage] = s.t
        if st and (not old or previous_lineage != current_lineage):
            veg.counts[current_lineage] += 1
            veg.peaks[current_lineage] = max(veg.peaks[current_lineage],veg.counts[current_lineage])
            veg.died[current_lineage] = None
        s.vegetation.track(i, st)
        if old == st: return
        s.pl[i] = st
        x, y = i % W, i // W
        k = (y // CH) * CWc + x // CH
        if st >= 2:
            s.pchunk.setdefault(k, set()).add(i)
        elif old >= 2:
            ss = s.pchunk.get(k)
            if ss: ss.discard(i)
        s.nplants += (st > 0) - (old > 0)
        s.psurf.set_at((x, y), PCOL[st])

    def newsp(s, parent, g, note=''):
        names = {p.name for p in s.species}
        while True:
            nm = ''.join(rnd.choice(SYL) for _ in range(rnd.choice((2, 2, 3)))).capitalize()
            if nm not in names: break
        sp = Sp(len(s.species), parent.id if parent else None, s.t, g, nm)
        sp.note = note
        s.species.append(sp)
        return sp

    def spawn(s, h1, h2, sp, par, x=None, y=None, e=None, note='founder', mom=None, g=None):
        g = g or phenotype(h1, h2)
        sex = rnd.choice('MF')
        if sex == 'F':
            g['horn'] *= .35; g['tail'] *= .7                       # sexual dimorphism: males carry the ornaments
        if x is None:
            i = -1
            for _ in range(80):
                j = rnd.randrange(N)
                if (s.terr[j] == 1 if aquatic(g) else s.terr[j] in (0, 3)):
                    i = j
                    if abs(s.tmp[j] / 255 - g['tol']) < .14: break  # founders start where the climate suits them
            if i < 0: raise ValueError('No suitable founder habitat')
            x, y = i % W + .5, i // W + .5
        c = Cr()
        c.z, c.breath = 0., 1.
        c.id = s.nid; s.nid += 1
        c.g, c.x, c.y, c.vx, c.vy, c.f, c.sex, c.hap = g, x, y, 0.0, 0.0, 1, sex, (h1, h2)
        c.age = rnd.randint(0, int(g['mat'])) if par is None else 0
        c.gen = par.gen + 1 if par else 0
        c.nut = .6
        c.pf = .75 + .4 * c.nut
        gr = min(1.0, c.age / g['mat'])
        c.s = g['size'] * c.pf * (.35 + .65 * (1 - (1 - gr) ** 2))
        mm = MK * c.s ** 3
        c.m34, c.maxE = mm ** .75, 60 + 110 * mm
        c.e = e if e is not None else c.maxE * .7
        c.cool, c.sp, c.tgt, c.thr, c.dead, c.gut, c.wa, c.rest = 200, sp, None, None, False, [], rr() * 6.28, False
        c.preg, c.mom, c.kids = None, mom, []
        c.sick, c.imm, c.fv = None, {}, None
        c.nurse = int(150 + 500 * g['care']) if mom else 0
        cl = lambda v: min(1.0, max(0.0, v))
        c.pers = (cl(g['bold'] + rnd.gauss(0, .15)), cl(g['social'] + rnd.gauss(0, .15)), cl(g['curio'] + rnd.gauss(0, .15)))
        c.nm = pname()
        c.het = sum(abs(h1[k] - h2[k]) / (G[k][1] - G[k][0]) for k in DK) / len(DK)
        h = g['hue'] + rnd.gauss(0, 3)
        sat, val = 55 + 30 * g['diet'], 88 - 8 * g['diet'] + rnd.randint(-5, 5)
        if 'Camouflage' in g['q']: sat, val = sat * .45, val * .75
        c.col, c.dk, c.lt = hsv(h, sat, val), hsv(h, 70, 50), hsv(h, 30, 100)
        c.c2 = hsv(g['hue2'], 62, 92)
        c.mk, c.mkc = rnd.randrange(1, 9), hsv(h + rnd.uniform(-30, 30), 75, 100)      # unique birthmark
        sp.pop += 1; sp.peak = max(sp.peak, sp.pop)
        s.lin[c.id] = (par.id if par else 0, c.gen, sp.id, note, s.t)
        s.crs.append(c)
        return c

    def logmsg(s, txt, col=(200, 210, 225)):
        s.log.append((s.t, txt, col))

    # ---- life events
    def classify(s, g, sp):
        same_group = (g['complexity'] < .42) == (sp.founder['complexity'] < .42)
        if same_group and dist(g, sp.founder) <= SPEC_T: return sp
        for o in s.species:
            if o.pop > 0 and (g['complexity'] < .42) == (o.founder['complexity'] < .42) and dist(g, o.founder) < SPEC_T * .75: return o
        nsp = s.newsp(sp,g,'animal body plan' if not same_group else trait_note(g,sp.founder))
        s.logmsg('%s splits from %s: %s' % (nsp.name, sp.name, nsp.note), nsp.col)
        return nsp

    def deliver(s, mom, sperm):
        e0 = (.10 + .25 * mom.g['care']) * mom.maxE
        for hd, nd in sperm:
            if len(s.crs) >= MAXPOP: break
            hm, nm_ = gamete(*mom.hap)
            g = phenotype(hm, hd)
            sp = s.classify(g, mom.sp)
            note = nm_ or nd
            cx, cy = mom.x + rnd.uniform(-2, 2), mom.y + rnd.uniform(-2, 2)
            if not s.ok(cx, cy, can_swim(g)): cx, cy = mom.x, mom.y
            ch = s.spawn(hm, hd, sp, mom, cx, cy, e0, note, mom=mom, g=g)
            mom.e -= e0
            mom.kids.append(ch)
            if note.startswith('!'): s.logmsg('MACRO-MUTANT in %s: %s' % (sp.name, note[1:]), (255, 210, 120))
        mom.e = max(mom.e, 1.0)

    def kill(s, c, corpse=True):
        if c.dead: return
        c.dead = True
        sp = c.sp
        sp.pop -= 1
        if sp.pop == 0:
            sp.died = s.t
            if sp.peak >= 8: s.logmsg('%s went extinct (peak %d)' % (sp.name, sp.peak), (150, 150, 160))
        if corpse:
            k = Co(); k.x, k.y, k.e, k.ttl, k.dead = c.x, c.y, (MK * c.s ** 3) * 120 + 4, 900, False
            k.size, k.col = c.s, c.col
            k.buried, k.burial, k.bone = False, 0., None
            k.has_bones = c.g.get('complexity',.8) >= .42
            k.inf = c.sick[0] if c.sick else None
            s.cor.append(k)

    # ---- disease -------------------------------------------------------------------------------
    def new_strain(s, parent=None):
        if parent is None:
            base = pname() + rnd.choice(DNAME)
            st = Strain(len(s.strains), base, 1, rnd.uniform(.006, .03), rnd.uniform(.15, .8), rnd.randint(350, 900),
                        rnd.randint(30, 160), rnd.random(), rnd.uniform(.3, .6), s.t)
        else:
            var = 1 + sum(1 for o in s.strains if o.base == parent.base)
            cl = lambda v, lo, hi: min(hi, max(lo, v))
            st = Strain(len(s.strains), parent.base, var, cl(parent.trans * math.exp(rnd.gauss(0, .25)), .003, .06),
                        cl(parent.vir * math.exp(rnd.gauss(0, .2)), .08, 1.0), int(cl(parent.dur * math.exp(rnd.gauss(0, .15)), 200, 1200)),
                        int(cl(parent.incub * math.exp(rnd.gauss(0, .2)), 10, 250)), cl(parent.center + rnd.gauss(0, .1), 0, 1),
                        cl(parent.width + rnd.gauss(0, .05), .2, .8), s.t, parent.id)
            st.col = parent.col
            s.logmsg('New strain: %s (virulence %.0f%%, spreads %.1fx)' % (st.name, st.vir * 100, st.trans / parent.trans), st.col)
        s.strains.append(st)
        return st

    def susc(s, c, st):
        e = c.imm.get(st.id)
        if e is not None:
            if e > s.t: return 0.0
            del c.imm[st.id]
        tr = max(0.0, 1 - abs(c.g['diet'] - st.center) / st.width)
        return tr * (1 - .8 * c.g['immune']) * (1.6 if c.e < .3 * c.maxE else 1.0)

    def infect(s, c, st):
        if c.sick or c.dead: return
        c.sick = (st, s.t)
        st.cases += 1

    def spread(s, c, st):
        cg, x, y = s.cgrid, c.x, c.y
        for gy in range(max(0, int(y - 5) // CG), min(GHc - 1, int(y + 5) // CG) + 1):
            for gx in range(max(0, int(x - 5) // CG), min(GWc - 1, int(x + 5) // CG) + 1):
                for o in cg.get(gy * GWc + gx, ()):
                    if o is c or o.sick or o.dead or (o.x - x) ** 2 + (o.y - y) ** 2 > 25: continue
                    if rr() < 1-(1-min(1,st.trans*3*s.susc(o,st)))**s.dt:
                        s.infect(o, st if rr() > .05 else s.new_strain(st))

    def outbreak(s, c=None):
        healthy = [o for o in s.crs if not o.dead and not o.sick]
        if c is None and not healthy: return
        c = c or rnd.choice(healthy)
        st = s.new_strain()
        st.center, st.width = c.g['diet'], max(.5,st.width)
        c.sick = (st, s.t - st.incub)
        st.cases += 1
        s.logmsg('OUTBREAK: %s appears in %s' % (st.name, c.sp.name), (150, 255, 120))
        s.banner = ('OUTBREAK: ' + st.name, 160, (150, 255, 120))

    # ---- weather / fire / meteors --------------------------------------------------------------
    def weather_step(s):
        w, t = s.wx, s.t
        w['ttl'] -= s.dt
        kind = w['kind']
        w['int'] = 0.0 if kind == 'clear' else min(1.0, (t - w['t0']) / 150, max(0.0, w['ttl'] / 150))
        if w['ttl'] <= 0:
            si = int(4 * ((t % SEASON) / SEASON))
            wt = {'clear': 5, 'rain': 3 if si in (0, 2) else 1.5, 'storm': 1.5 if si == 1 else .6, 'fog': 1.5 if si != 1 else .4,
                  'drought': 1.3 if si == 1 else .1, 'heatwave': 1.3 if si == 1 else 0, 'coldsnap': 1.5 if si == 3 else 0}
            kind = rnd.choices(list(wt), list(wt.values()))[0]
            w.update(kind=kind, ttl=rnd.randint(800, 2500) if kind == 'clear' else rnd.randint(600, 2000), t0=t, int=0.0)
            if kind != 'clear': s.logmsg('Weather: %s' % kind, WXCOL[kind])
        i = w['int']
        s.wx_grow = {'drought': 1 - .6 * i, 'heatwave': 1 - .2 * i, 'coldsnap': 1 - .35 * i}.get(kind, 1.0)
        s.wx_vis = {'fog': 1 - .25 * i}.get(kind, 1.0)
        s.wx_dT = {'heatwave': .14 * i, 'coldsnap': -.14 * i}.get(kind, 0.0)
        s.atmosphere.step(s)
        if kind in ('drought', 'heatwave') and rr() < 1-(1-.0004*i)**s.dt:
            for _ in range(40):
                j = rnd.randrange(N)
                if s.pl[j] >= 2 and s.ignite(j):
                    s.logmsg('Wildfire sparked by the heat', (255, 150, 70)); break

    def lightning(s, target=None):
        j = -1 if target is None else target
        for _ in range(30 if target is None else 0):
            k = rnd.randrange(N)
            if s.terr[k] in (0, 3): j = k; break
        if j < 0: return
        x, y = j % W, j // W
        s.flash, s.bolt = 1.0, [x, y, 10]
        for c in list(s.crs):
            if (c.x - x) ** 2 + (c.y - y) ** 2 < 5: s.kill(c)
        if rr() < .6 and s.ignite(j): s.logmsg('Lightning strike starts a fire', (255, 210, 90))

    def ignite(s, i, ttl=None):
        if len(s.fire) > 4000 or i in s.fire or i in s.ash or s.terr[i] not in (0, 3): return False
        s.fire[i] = ttl or rnd.randint(18, 40)
        s.fchunk.setdefault((i // W // CH) * CWc + (i % W) // CH, set()).add(i)
        return True

    def fire_step(s):
        fire = s.fire
        if not fire: return
        kind, wi = s.wx['kind'], s.wx['int']
        base_spread = .045 * (1 + (1.2 * wi if kind in ('drought', 'heatwave') else 0.0))
        pl, terr, ash = s.pl, s.terr, s.ash
        done = []
        for i, ttl in list(fire.items()):
            wet = s.atmosphere.rain_at(i % W, i // W)
            spread = base_spread * (1 - .92 * wet) * (1 - .5 * s.moisture[i]/255)
            if wet and rr() < 1-(1-.1*wet)**s.dt: done.append((i, False)); continue
            if ttl <= 1: done.append((i, True)); continue
            fire[i] = ttl - s.dt
            x = i % W
            for j in (i - 1 if x > 0 else -1, i + 1 if x < W - 1 else -1, i - W, i + W):
                if 0 <= j < N and j not in fire and terr[j] in (0, 3) and j not in ash and rr() < 1-(1-spread*(1.0 if pl[j] else .08))**s.dt:
                    s.ignite(j)
        for i, burnt in done:
            del fire[i]
            k = (i // W // CH) * CWc + (i % W) // CH
            ss = s.fchunk.get(k)
            if ss:
                ss.discard(i)
                if not ss: del s.fchunk[k]
            if burnt:
                s.setp(i, 0); ash[i] = 1800
                s.fert[i] = min(1.0, s.fert[i] + .08)                  # ash fertilises the soil

    def fire_near(s, x, y, r):
        cx, cy = int(x) // CH, int(y) // CH
        best, bd = None, r * r
        for gy in range(cy - 3, cy + 4):
            for gx in range(cx - 3, cx + 4):
                st = s.fchunk.get(gy * CWc + gx)
                if st:
                    for i in st:
                        dx, dy = i % W - x, i // W - y
                        d = dx * dx + dy * dy
                        if d < bd: bd, best = d, (dx, dy)
        if best is None: return None
        d = math.sqrt(bd) + 1e-6
        return (-best[0] / d, -best[1] / d)

    def meteor_spawn(s, x=None, y=None, big=False):
        if s.meteor: return
        if x is None:
            j = 0
            for _ in range(40):
                j = rnd.randrange(N)
                if s.terr[j] in (0, 3): break
            x, y = j % W, j // W
        r = rnd.randint(18, 26) if big else rnd.choice((4, 5, 5, 6, 7, 9))
        s.meteor = {'x': int(x), 'y': int(y), 'r': r, 'ttl': 100}
        s.banner = ('METEOR INCOMING', 140, (255, 190, 110))
        s.logmsg('A %s meteor streaks across the sky!' % ('HUGE' if big else 'small'), (255, 190, 110))

    def meteor_hit(s):
        m, s.meteor = s.meteor, None
        cx, cy, r = m['x'], m['y'], m['r']
        s.flash = 1.6
        big = r >= 15
        s.banner = ('EXTINCTION-LEVEL IMPACT!' if big else 'IMPACT!', 220, (255, 120, 90))
        R = int(r * 2.4)
        for y in range(max(1, cy - R), min(H - 1, cy + R + 1)):
            for x in range(max(1, cx - R), min(W - 1, cx + R + 1)):
                d = math.hypot(x - cx, y - cy)
                if d > R: continue
                i = y * W + x
                if s.terr[i] == 1: continue
                nz = rnd.randint(-8, 8)
                if d <= r:                                                     # crater
                    s.terr[i] = 0 if s.terr[i] != 2 else 2
                    s.setp(i, 0); s.fert[i] = .02
                    s.fire.pop(i, None); s.ash.pop(i, None)
                    s.bg.set_at((x, y), (50 + nz, 40 + nz, 36 + nz))
                elif d <= r * 1.35:                                            # raised rim
                    if rr() < .55:
                        s.terr[i] = 2; s.setp(i, 0)
                        s.bg.set_at((x, y), (112 + nz, 106 + nz, 102 + nz))
                else:
                    if d <= r * 2 and rr() < .6: s.setp(i, 0)
                    if rr() < .3: s.ignite(i, rnd.randint(25, 60))
        for c in list(s.crs):
            d = math.hypot(c.x - cx, c.y - cy)
            if d <= r * 1.3: s.kill(c, corpse=False)
            elif d <= r * 2.4: c.e -= .75 * c.maxE
            elif d <= r * 4: c.e -= .2 * c.maxE
        s.dust = min(1.0, s.dust + (r / 20.0) ** 2)
        s.logmsg('IMPACT! crater %.0f cm wide%s' % (r * 2 * CELL_CM, ' - impact winter begins' if s.dust > .4 else ''), (255, 120, 90))

    def corpse_exposure(s, corpse, c):
        if corpse.inf is None or c.sick or c.dead or c.z > .5: return 0.
        distance = (corpse.x-c.x)**2+(corpse.y-c.y)**2
        if distance > 25: return 0.
        barrier = .01 if corpse.buried else 1.
        return min(.8, corpse.inf.trans*2*s.susc(c,corpse.inf)*barrier*(1-distance/25))

    def corpse_step(s):
        for bone in s.bones:
            bone.ttl -= s.dt
        s.bones = [bone for bone in s.bones if bone.ttl > 0]
        for k in s.cor:
            k.ttl -= s.dt*max(.25,1+s.dT*2)*(.6 if k.buried else 1)
            k.e *= .9995**s.dt
            if k.has_bones and k.bone is None and (k.ttl <= 300 or k.dead or k.e <= 0):
                bone = Bo()
                bone.x, bone.y, bone.size, bone.ttl, bone.buried = k.x,k.y,k.size,DAY*8,k.buried
                k.bone = bone
                s.bones.append(bone)
            if not k.inf and k.ttl < 650 and rr() < 1-(1-.001)**s.dt:
                if s.environment_strain is None:
                    s.environment_strain = s.new_strain()
                    s.environment_strain.center, s.environment_strain.width = .5,.8
                    s.logmsg('Rotting remains harbor '+s.environment_strain.name, (170,210,100))
                k.inf = s.environment_strain
            if k.inf and s.every(12):
                for gy in range(max(0,int(k.y-5)//CG),min(GHc-1,int(k.y+5)//CG)+1):
                    for gx in range(max(0,int(k.x-5)//CG),min(GWc-1,int(k.x+5)//CG)+1):
                        for c in s.cgrid.get(gy*GWc+gx, ()):
                            risk = s.corpse_exposure(k,c)
                            if risk and rr() < 1-(1-risk)**12: s.infect(c,k.inf)
            if k.ttl <= 0 or k.e <= 0:
                if k.e > 0 and k.ttl <= 0:
                    i = int(k.y)*W+int(k.x)
                    for j in (i,*s.neighbors(i)):
                        if s.terr[j] == 0:
                            s.fert[j] = min(1,s.fert[j]+.05)
                            if s.pl[j] == 0: s.setp(j,1)
                k.dead = True
        s.cor = [k for k in s.cor if not k.dead]

    def burial_step(s, c, corpse):
        if corpse.dead or corpse.buried or c.g.get('burial',0) < .55: return
        corpse.burial += s.dt*(.5+c.g['burial'])
        c.e -= .0015*c.maxE*s.dt
        if corpse.burial >= 60:
            corpse.buried = True
            if corpse.bone: corpse.bone.buried = True
            if 'burial' not in s.milestones:
                s.milestones.add('burial')
                s.logmsg('EVOLUTION: '+c.sp.name+' buries infectious remains', c.col)

    def near_corpses(s, x, y, radius):
        for gy in range(max(0, int(y-radius)//CG), min(GHc-1, int(y+radius)//CG)+1):
            for gx in range(max(0, int(x-radius)//CG), min(GWc-1, int(x+radius)//CG)+1):
                yield from s.kgrid.get(gy * GWc + gx, ())

    def earthquake(s, x=None, y=None, radius=None):
        x = rnd.randrange(1, W-1) if x is None else x
        y = rnd.randrange(1, H-1) if y is None else y
        radius = rnd.randint(20, 50) if radius is None else radius
        s.quake = [x, y, radius, 100]
        s.banner = ('EARTHQUAKE', 160, (235, 185, 120))
        s.logmsg('Earthquake: landslide and habitat damage', (235, 185, 120))
        for c in s.crs:
            d = math.hypot(c.x-x, c.y-y)
            if d < radius:
                c.e -= c.maxE * .65 * (1-d/radius) * (1-.4*c.g['armor'])
                if c.e <= 0: s.kill(c)
        for _ in range(radius * 8):
            xx, yy = int(x+rnd.gauss(0, radius/2)), int(y+rnd.gauss(0, radius/2))
            if not (0 <= xx < W and 0 <= yy < H): continue
            i = yy*W+xx
            if s.terr[i] not in (0, 3): continue
            s.setp(i, 0)
            s.fert[i] *= .8
            s.bg.set_at((xx, yy), (100, 87, 65))

    def find_plant(s, x, y, vis):
        cx, cy = int(x) // CH, int(y) // CH
        best, bd = -1, 1e18
        pch = s.pchunk
        for r in range(int(vis) // CH + 2):
            for gy in range(cy - r, cy + r + 1):
                if gy < 0 or gy >= CHc: continue
                step = 1 if (gy == cy - r or gy == cy + r) else 2 * r
                for gx in range(cx - r, cx + r + 1, step or 1):
                    if gx < 0 or gx >= CWc: continue
                    st = pch.get(gy * CWc + gx)
                    if st:
                        for i in st:
                            dx = i % W - x; dy = i // W - y; d = dx * dx + dy * dy
                            if d < bd: bd, best = d, i
            if best >= 0 and bd <= (r * CH) ** 2: break
        return best if bd <= vis * vis else -1

    def look(s, c):
        g, q, dk = c.g, c.g['q'], s.dark
        bold, social, curio = c.pers
        vis = g['vis'] * ((.65 + .9 * dk) if 'Nocturnal' in q else (1 - .35 * dk)) * s.wx_vis * (1 - .3 * s.atmosphere.rain_at(c.x, c.y))
        x, y, v2, cs, sp = c.x, c.y, vis * vis, c.s, c.sp
        thr = prey = mate = None
        td = pd = md = 1e18
        hx = hy = hn = 0
        ready = g['complexity'] >= .42 and c.sex == 'M' and c.age >= g['mat'] and c.cool <= 0 and c.e >= g['thresh'] * c.maxE * .8
        carn = g['diet'] > .4
        cg = s.cgrid
        for gy in range(max(0, int(y - vis) // CG), min(GHc - 1, int(y + vis) // CG) + 1):
            for gx in range(max(0, int(x - vis) // CG), min(GWc - 1, int(x + vis) // CG) + 1):
                lst = cg.get(gy * GWc + gx)
                if not lst: continue
                for o in lst:
                    if o is c or o.dead or abs(c.z-o.z) > 2: continue
                    dx = o.x - x; dy = o.y - y; d = dx * dx + dy * dy
                    og = o.g
                    if 'Camouflage' in og['q']: d *= 3                     # hard to spot
                    if d > v2: continue
                    kin = o.sp is sp
                    if kin:
                        hx += o.x; hy += o.y; hn += 1
                    if ready and o.sex == 'F' and o.preg is None and d < md and o.cool <= 0 and o.age >= og['mat'] \
                            and o.e >= og['thresh'] * o.maxE * .7 and (kin or dist(og, g) < .3):
                        mate, md = o, d
                    elif kin:
                        continue
                    elif og['diet'] > .5 and o.s > cs * .85:
                        if d < td: thr, td = o, d
                    elif carn and o.s < cs * .9 and d < pd and dist(og, g) > .25:
                        prey, pd = o, d
        fr = 1.0 if 'Skittish' in q else .75 - .35 * bold
        c.thr = thr if thr and td <= (vis * fr) ** 2 else None
        tg = None
        hunger = c.e / c.maxE
        aggr = min(1.0, max(0.0, g['aggr'] + .3 * (bold - .5)))
        if c.thr is None:
            burial_target = None
            if g['burial'] > .55 and g['complexity'] > .6 and hunger > .55:
                options = [k for k in s.near_corpses(x,y,vis) if not k.dead and not k.buried and (k.inf or k.ttl < 650) and (k.x-x)**2+(k.y-y)**2 < v2 and s.terr[int(k.y)*W+int(k.x)] in (0,3)]
                if options: burial_target = min(options,key=lambda k:(k.x-x)**2+(k.y-y)**2)
            if burial_target:
                tg = ('b',burial_target)
            elif mate:
                tg = ('m', mate)
            else:
                diet = g['diet']
                if hunger < .92:
                    if prey and hunger < .3 + .6 * aggr: tg = ('c', prey)
                    if tg is None and (diet > .45 or g['scavenge'] > .25 or 'Scavenger' in q):
                        bk, bd = None, v2
                        for k in s.near_corpses(x, y, vis):
                            d = (k.x - x) ** 2 + (k.y - y) ** 2
                            if d < bd and not k.dead and not k.buried: bk, bd = k, d
                        if bk: tg = ('k', bk)
                    if tg is None and diet < .7:
                        i = s.find_plant(x, y, vis)
                        if i >= 0: tg = ('p', i)
                if tg is None and hn >= 2 and ('Herder' in q or social > .65):
                    mx, my = hx / hn, hy / hn
                    if (mx - x) ** 2 + (my - y) ** 2 > 36 + 100 * (1 - social): tg = ('h', (mx, my))
        c.tgt = tg

    def ok(s, x, y, sw):
        if x < 1 or y < 1 or x >= W - 1 or y >= H - 1: return False
        t = s.terr[int(y) * W + int(x)]
        return t == 0 or t == 3 or (sw and t == 1)

    def live(s, c):
        g, q, t = c.g, c.g['q'], s.t
        c.age += s.dt
        gr = min(1.0, c.age / g['mat'])
        if gr < 1: c.pf = .75 + .4 * c.nut                  # nutrition while growing sets adult size (stunting)
        c.s = g['size'] * c.pf * (.35 + .65 * (1 - (1 - gr) ** 2))
        mm = MK * c.s ** 3
        c.m34 = mm ** .75
        c.maxE = 60 + 110 * mm
        if c.cool > 0: c.cool -= s.dt
        if g['complexity'] < .42 and c.age >= g['mat'] and c.cool <= 0 and c.e > .75*c.maxE and len(s.crs) < MAXPOP:
            h1, _ = gamete(*c.hap)
            h2, note = gamete(*c.hap)
            child_g = phenotype(h1, h2)
            sp = s.classify(child_g, c.sp)
            energy = c.e * .4
            s.spawn(h1, h2, sp, c, c.x, c.y, energy, note, g=child_g)
            c.e -= energy
            c.cool = int(g['mat'])

        c.nut += (min(1.0, c.e / c.maxE) - c.nut) * (1-.997**s.dt)
        if c.preg and t >= c.preg[0]:
            s.deliver(c, c.preg[1]); c.preg = None; c.cool = 500
        if s.every(10 if c.tgt else 3, c.id): s.look(c)
        x, y = c.x, c.y
        idx = int(y) * W + int(x)
        T = s.atmosphere.temperature(s, idx)
        if not s.fchunk: c.fv = None
        elif s.every(3,c.id): c.fv = s.fire_near(x, y, 28)
        sickf = 0.0
        if c.sick:
            st, t0 = c.sick
            ag = t - t0
            if ag >= st.incub:
                if ag < st.incub + st.dur:
                    sickf = st.vir*(1-.55*g['immune'])
                    c.e -= sickf * .0006 * c.maxE * s.dt
                    if s.every(3,c.id): s.spread(c, st)
                else:
                    c.imm[st.id] = t + 4000                                 # recovered: temporary immunity
                    c.sick = None
        hib = 'Hibernator' in q and T < .32
        ux = uy = 0.0
        mul = .55 + .25 * c.pers[2]
        tg = c.tgt
        hunger = c.e / c.maxE
        if c.fv is not None:
            ux, uy, mul = c.fv[0], c.fv[1], 1.6
        elif c.thr is not None and not c.thr.dead:
            o = c.thr
            dx, dy = x - o.x, y - o.y
            d = math.hypot(dx, dy) + 1e-6
            ux, uy, mul = dx / d, dy / d, (1.4 if 'Skittish' in q else 1.2)
        elif tg:
            k, o = tg
            if k == 'p':
                tx, ty = o % W + .5, o // W + .5
                alive = s.pl[o] >= 2
            elif k == 'h':
                tx, ty = o; alive = True
            else:
                tx, ty = o.x, o.y; alive = not o.dead
            if alive:
                dx, dy = tx - x, ty - y
                d = math.hypot(dx, dy) + 1e-6
                ux, uy = dx / d, dy / d
                mul = {'p': .8, 'c': 1.1, 'k': .9, 'm': .9, 'h': .6, 'b': .7}[k]
            else:
                c.tgt = None
        if aquatic(g) and s.terr[idx] != 1:
            water = [(xx,yy) for yy in range(max(1,int(y)-6),min(H-1,int(y)+7)) for xx in range(max(1,int(x)-6),min(W-1,int(x)+7)) if s.terr[yy*W+xx] == 1]
            if water:
                tx, ty = min(water, key=lambda p:(p[0]-x)**2+(p[1]-y)**2)
                d = math.hypot(tx-x,ty-y)+1e-6
                ux, uy = (tx-x)/d, (ty-y)/d
        if ux == 0 and uy == 0:
            mom = c.mom
            if mom is not None and not mom.dead and c.age < c.nurse:       # babies stay near mum
                dx, dy = mom.x - x, mom.y - y
                d = math.hypot(dx, dy)
                if d > 6: ux, uy, mul = dx / d, dy / d, 1.0
            if ux == 0 and uy == 0:
                c.wa += rnd.uniform(-.3, .3)*math.sqrt(s.dt)
                ux, uy = math.cos(c.wa), math.sin(c.wa)
        c.rest = False
        if c.thr is None and c.fv is None:
            if hib or (not c.tgt and hunger > .6 and (('Nocturnal' in q and s.dark < .25) or ('Nocturnal' not in q and s.dark > .75))):
                c.rest = True; mul = 0
        sen = 1 - .4 * max(0.0, (c.age / g['life'] - .7) / .3)             # old age slows you down
        sp = g['spd'] * mul * (.6 + .4 * gr) * sen * (1 - .35 * sickf)
        inertia = .6**s.dt
        c.vx = c.vx*inertia+ux*sp*(1-inertia)
        c.vy = c.vy*inertia+uy*sp*(1-inertia)
        sw = can_swim(g)
        # Flight is an energetically costly movement layer, with takeoff/landing.
        target_near = False
        if tg:
            k, o = tg
            tx, ty = (o%W+.5, o//W+.5) if k == 'p' else o if k == 'h' else (o.x, o.y)
            target_near = (tx-x)**2+(ty-y)**2 < (c.s+4)**2
        flying = can_fly(g) and gr >= .7 and c.e > .35*c.maxE and not c.rest and not target_near
        c.z = min(5., c.z+.25*s.dt) if flying else max(0., c.z-.35*s.dt)
        if c.z > 0:
            c.vx *= 1.15
            c.vy *= 1.15
        # Trace movement through each crossed cell so coarse time cannot tunnel
        # across a shoreline or a rock barrier.
        nx, ny = x, y
        allowed = lambda xx, yy: (1 <= xx < W-1 and 1 <= yy < H-1) if c.z > .5 else s.ok(xx, yy, sw)
        for _ in range(s.dt):
            tx, ty = nx+c.vx, ny+c.vy
            if allowed(tx,ty): nx,ny = tx,ty
            elif allowed(tx,ny): nx = tx
            elif allowed(nx,ty): ny = ty
            else:
                c.wa, c.tgt = rr()*6.28, None
                c.vx = c.vy = 0
                break
        c.x, c.y = nx, ny
        in_water = s.terr[int(ny) * W + int(nx)] == 1 and c.z <= .5
        if in_water:
            drag = .55 + .4*g['fins']
            c.vx *= drag; c.vy *= drag
        oxygen = g['gills'] if in_water else g['lungs']
        if c.z > .5: oxygen = g['lungs']
        c.breath = min(1, c.breath+.02*s.dt) if oxygen > .4 else max(0, c.breath-.005*s.dt)
        if c.breath <= 0: c.e -= c.maxE*.015*s.dt
        if g['complexity'] < .42 and in_water:
            # Colonies graze suspended nutrients; photosynthesis is daylight-dependent.
            c.e += .035*s.dt*(1-s.dark)*(.4+s.moisture[idx]/255)

        if c.vx > .04: c.f = 1
        elif c.vx < -.04: c.f = -1
        v = math.hypot(c.vx, c.vy)
        dT = abs(T - g['tol']) - .15 - g['fur'] * max(0, g['tol'] - T) * .5                                       # climate mismatch costs energy
        therm = 1 + 8 * dT * dT if dT > 0 else 1.0
        orn = (g['horn'] + g['tail']) * .5 if c.sex == 'M' else 0           # ornaments are expensive
        cost = (KREST * c.m34 * g['metab'] + KMOVE * c.m34 * v * v + .0002 * g['vis']) * therm * (1 + .25 * orn) * (1 + .12 * g['immune'] + .12 * g['armor'] + .08 * g['fur'])      # immunity is metabolically expensive
        cost *= (.2+.8*g['complexity'])
        if c.z > .5: cost *= 1.8+g['size']*.08
        if c.preg: cost *= 1.2
        if 'Glutton' in q: cost *= 1.15
        if c.rest: cost *= .3 if hib else .8
        c.e -= cost * s.dt
        if s.fire and idx in s.fire and c.z <= .5: c.e -= c.maxE * .08 * s.dt                   # standing in flames
        # ---- act on target
        if c.tgt and c.z <= .5:
            k, o = c.tgt
            glut = 1.3 if 'Glutton' in q else 1
            reach = c.s * .5 + 1.8
            if k == 'p':
                if (c.x - o % W - .5) ** 2 + (c.y - o // W - .5) ** 2 < reach * reach and s.pl[o] >= 2:
                    plant = s.vegetation.lineage(o)
                    c.e += (1-.6*plant.defense) * s.pl[o] * 2.2 * max(.05, 1 - g['diet'] * 1.4) * (.6 + .8 * c.m34) * glut
                    s.setp(o, 1 if plant.wood > .3 else 0 if rr() < .5 else 1)
                    if rr() < .75: c.gut.append(max(c.gut[-1] + 5 if c.gut else 0, t + rnd.randint(140, 260)))
                    c.tgt = None
            elif k == 'b':
                if (c.x-o.x)**2+(c.y-o.y)**2 < reach*reach:
                    s.burial_step(c,o)
                    if o.buried: c.tgt = None
            elif k == 'k':
                if not o.dead and not o.buried and (c.x - o.x) ** 2 + (c.y - o.y) ** 2 < reach * reach:
                    bite = min(o.e, 8 + 24 * c.m34)
                    efficiency = 1 if 'Scavenger' in q else .35 + .65 * g['scavenge']
                    c.e += bite * efficiency * glut
                    o.e -= bite
                    if o.inf and rr() < .5 * (1 - .85 * g['scavenge']) * s.susc(c, o.inf): s.infect(c, o.inf)
                    o.dead = o.e <= 0; c.tgt = None
            elif k == 'c':
                if not o.dead and (c.x - o.x) ** 2 + (c.y - o.y) ** 2 < ((c.s + o.s) * .5 + 1.5) ** 2:
                    if 'Spiky' in o.g['q']:
                        c.e -= 6
                        if rr() < .4: c.tgt = None; c.thr = None; o = None
                    if o is not None:
                        pk = .38 + .3 * (c.s / o.s - .9)
                        pk *= 1 - .45 * o.g['armor']
                        if 'Thick-hide' in o.g['q']: pk *= .55
                        if rr() < pk:
                            c.e += (o.e * .5 + o.s * 4) * (.3 + .7 * g['diet']) * glut
                            if o.sick and rr() < .4 * s.susc(c, o.sick[0]): s.infect(c, o.sick[0])
                            if 'Venomous' in o.g['q']:
                                c.e -= 25
                                if rr() < .25: c.e -= 20
                            s.kill(o)
                            c.tgt = None
            elif k == 'm':                                             # courtship (male -> female)
                if not o.dead and o.preg is None and (c.x - o.x) ** 2 + (c.y - o.y) ** 2 < ((c.s + o.s) * .5 + 2) ** 2:
                    if c.cool <= 0 and o.cool <= 0:
                        ornm = (g['horn'] + g['tail']) * .5
                        if rr() < .25 + .75 * (1 - o.g['choosy'] * (1 - ornm)):     # female choice -> sexual selection
                            pro = 'Prolific' in o.g['q']
                            gest = int(280 * (.7 if pro else 1) * (.8 + .4 * rr()))
                            lit = max(1, int(round(o.g['litter'] * (1.25 - .55 * o.g['care']) + (1 if pro else 0))))
                            o.preg = (t + gest, [gamete(*c.hap) for _ in range(lit)])
                            c.cool = 350
                            c.e -= .05 * c.maxE
                        else:
                            c.cool = 40
                    c.tgt = None
        if c.kids and s.every(10,c.id):                              # nursing
            c.kids = [k for k in c.kids if not k.dead]
            for k in c.kids:
                if k.age < k.nurse and k.e < .65 * k.maxE and c.e > .5 * c.maxE and (k.x - c.x) ** 2 + (k.y - c.y) ** 2 < 64:
                    a = min(4.0, c.e * .05)
                    c.e -= a; k.e += a * .9
        c.e = min(c.e, c.maxE)
        if c.gut and c.gut[0] <= t:                                      # seeds come out the other end
            c.gut.pop(0)
            s.poop.append([c.x, c.y, 500, True, c.sick[0] if c.sick else None])
        elif s.every(420,c.id*17):
            s.poop.append([c.x, c.y, 500, False, c.sick[0] if c.sick else None])
        r = c.age / g['life']
        if c.e <= 0 or r > 1.6 or (sickf and rr() < 1-(1-sickf**2*.0004)**s.dt) or (r > .5 and rr() < 1-(1-min(1,1e-4*math.exp(7*(r-.7))))**s.dt):   # ageing / disease
            s.kill(c)

    # ---- tick
    def grow_plants(s):
        season = (1 + .55 * math.sin(2 * math.pi * s.t / SEASON)) * s.wx_grow * (1 - .6 * s.dust)
        pl, fert, terr, veg = s.pl, s.fert, s.terr, s.vegetation
        crowd = s.crowd = max(0., 1-s.nplants/max(1,s.land*.1))
        # random() avoids Python randrange rejection loops in this hot path.
        for _ in range(PS):
            i = int(rr()*N)
            st = pl[i]
            if st == 0:
                if terr[i] in (0,1) and crowd > 0 and rr() < 1-(1-min(1,.0008*season))**s.dt:
                    s.setp(i, 1)
                continue
            p = veg.lineage(i)
            temperature = s.tmp[i]/255+s.dT
            suit = veg.suitability(i, s.moisture[i], temperature, terr[i] == 1)
            if suit < .1 and rr() < 1-(.92+.06*p.girth)**s.dt:
                s.setp(i, 0)
                continue
            fertility = .5 if terr[i] == 1 else fert[i]
            shade = s.atmosphere.shade.get(s.atmosphere.key(i%W,i//W), 0)
            rate = fertility*season*suit*(1-.4*shade)/(1+p.wood+.3*p.bloom+.2*p.girth)
            if st >= 2: rate *= 1-.6*veg.crowding(i, pl, W, H)
            if st < 3:
                if rr() < 1-(1-min(1,.65*rate))**s.dt: s.setp(i, st+1)
            elif rr() < 1-(1-min(1,.35*crowd*rate))**s.dt:
                reach = max(2, int(3+p.dispersal*10+p.bloom*6))
                x = i%W+int(rr()*(2*reach+1))-reach
                y = i//W+int(rr()*(2*reach+1))-reach
                if 0 <= x < W and 0 <= y < H:
                    j = y*W+x
                    if terr[j] != 2 and pl[j] == 0:
                        child = veg.seed(i,j,s.t)
                        if veg.suitability(j,s.moisture[j],s.tmp[j]/255+s.dT,terr[j] == 1) > .2 and rr() > .7*veg.crowding(j, pl, W, H):
                            s.setp(j,1,lineage=child.id)


    def every(s, period, offset=0):
        return (s.t+offset)//period != (s.t-s.dt+offset)//period

    def step(s, ticks=1):
        if ticks not in (1, 8): raise ValueError('Simulation step must be 1 or 8 ticks')
        s.dt = ticks
        s.t += ticks
        t = s.t
        s.dark = min(1.0, max(0.0, 1.6 * (.5 - .5 * math.cos(2 * math.pi * (t % DAY) / DAY)) - .3))
        s.weather_step()
        s.water_step()
        s.dust *= .9996**s.dt
        s.dT = .1 * math.sin(2 * math.pi * t / SEASON) + s.wx_dT - .12 * s.dust
        s.fire_step()
        if s.every(20) and s.ash:
            for i in list(s.ash):
                s.ash[i] -= 20
                if s.ash[i] <= 0: del s.ash[i]
        if s.meteor:
            s.meteor['ttl'] -= s.dt
            if s.meteor['ttl'] <= 0: s.meteor_hit()
        elif rr() < 1-(1-1/45000)**s.dt: s.meteor_spawn(big=rr() < .12)             # very rare
        if s.quake:
            s.quake[3] -= s.dt
            if s.quake[3] <= 0: s.quake = None
        if rr() < 1-(1-1/32000)**s.dt: s.earthquake()
        if t >= s.next_outbreak and len(s.crs) >= 14:
            if sum(c.sick is not None for c in s.crs) < max(2,len(s.crs)//10): s.outbreak()
            s.next_outbreak = t+rnd.randint(DAY,DAY*3)
        s.flash *= .88**s.dt
        if s.bolt:
            s.bolt[2] -= s.dt
            if s.bolt[2] <= 0: s.bolt = None
        if s.banner:
            s.banner = (s.banner[0], s.banner[1] - s.dt, s.banner[2]) if s.banner[1] > 1 else None
        s.grow_plants()
        for p in s.poop:
            p[2] -= s.dt*(2 if s.atmosphere.rain_at(p[0], p[1]) > .1 else 1)
            if p[4] and s.every(30):
                for c in s.cgrid.get(int(p[1]) // CG * GWc + int(p[0]) // CG, ()):
                    if (c.x-p[0])**2 + (c.y-p[1])**2 < 9 and rr() < .08 * s.susc(c, p[4]):
                        s.infect(c, p[4])
            if p[2] <= 0:
                i = int(p[1]) * W + int(p[0])
                if s.terr[i] in (0, 3):
                    s.fert[i] = min(1.0, s.fert[i] + .04)
                    if p[3] and s.pl[i] == 0 and (s.crowd > 0 or rr() < .25): s.setp(i, 1)
                    j = i + rnd.randint(-1, 1) + W * rnd.randint(-1, 1)
                    if 0 <= j < N and s.terr[j] == 0 and s.pl[j] == 0 and p[3] and rr() < .6: s.setp(j, 1)
        s.poop = [p for p in s.poop if p[2] > 0]
        s.corpse_step()
        s.kgrid = {}
        for k in s.cor:
            s.kgrid.setdefault(int(k.y) // CG * GWc + int(k.x) // CG, []).append(k)
        cg = s.cgrid = {}
        for c in s.crs:
            cg.setdefault(int(c.y) // CG * GWc + int(c.x) // CG, []).append(c)
        for c in list(s.crs):
            if not c.dead: s.live(c)
        s.crs = [c for c in s.crs if not c.dead]
        if not s.primordial and len(s.crs) < 14:                       # a meteor-proof ark: new founders
            b = base_genome('microbe' if s.primordial else 'herb')
            sp = s.newsp(None, b)
            for _ in range(10): s.spawn(mutate(b, .8)[0], mutate(b, .8)[0], sp, None)
            s.logmsg('New founders appear: %s' % sp.name, (120, 220, 160))
        if not s.primordial and s.every(3000) and t > 0 and not any(c.g['diet'] > .65 for c in s.crs) and len(s.crs) > 60:
            b = base_genome('carn')                              # immigration: a new predator lineage wanders in
            sp = s.newsp(None, b)
            for _ in range(10): s.spawn(mutate(b, .8)[0], mutate(b, .8)[0], sp, None, *s.near_prey('carn'))
            s.logmsg('Predators migrate in: %s' % sp.name, (255, 140, 120))
        if s.every(30):
            for c in s.crs:
                for label, reached in (('flight',can_fly(c.g)), ('swimming limbs',c.g['fins']>.5), ('complex animals',c.g['complexity']>.65)):
                    if reached and label not in s.milestones:
                        s.milestones.add(label)
                        s.logmsg('EVOLUTION: '+label+' in '+c.sp.name, c.col)
            cnt = {}
            for c in s.crs:
                if c.sick: cnt[c.sick[0].id] = cnt.get(c.sick[0].id, 0) + 1
            s.nsick = sum(cnt.values())
            for st in s.strains:
                n = cnt.get(st.id, 0)
                if st.act > 0 and n == 0 and st.peak >= 5: s.logmsg('%s died out (%d cases total)' % (st.name, st.cases), (140, 190, 120))
                st.act, st.peak = n, max(st.peak, n)
        if s.every(15):
            h = sum(1 for c in s.crs if c.g['diet'] < .45)
            k = sum(1 for c in s.crs if c.g['diet'] > .65)
            s.hist.append((h, k, len(s.crs) - h - k, s.nplants, s.nsick))
            if len(s.hist) > 220: s.hist.pop(0)

    # ---- drawing
    def draw_world(s, surf):
        surf.blit(s.bg, (0, 0))
        surf.blit(s.psurf, (0, 0))
        draw_land_details(s, surf)
        # Cloud shadows affect habitat light/temperature as well as appearance.
        if s.atmosphere.shade:
            shadow = pygame.Surface((16, 16), pygame.SRCALPHA)
            for key, amount in s.atmosphere.shade.items():
                shadow.fill((30, 40, 63, int(40*amount)))
                surf.blit(shadow, ((key % s.atmosphere.cols)*16, (key // s.atmosphere.cols)*16))
        for p in s.poop: surf.set_at((int(p[0]), int(p[1])), (110, 70, 40))
        for k in s.cor:
            if k.buried:
                surf.fill((116,92,61), (int(k.x)-1,int(k.y),3,1))
                continue
            color = mix((90, 65, 55), k.col, max(0, k.ttl / 900) * .55)
            x, y, w = int(k.x)-2, int(k.y)-1, max(3, int(k.size))
            surf.fill(color, (x+1, y, w-2, 3))
            surf.fill(color, (x, y+1, w, 1))
            surf.set_at((int(k.x), int(k.y)), BONE)
        for bone in s.bones:
            if bone.buried: continue
            x,y = int(bone.x),int(bone.y)
            length = max(2,int(bone.size*.7))
            surf.fill(BONE, (x-length//2,y,length,1))
            surf.fill(BONE, (x-length//2,y-1,1,3))
            surf.fill(BONE, (x+length//2-1,y-1,1,3))
        for i in s.ash: surf.set_at((i % W, i // W), (48, 43, 38))
        for i in s.fire: surf.set_at((i % W, i // W), (255, 120 + (s.t+i)%120, 35))
        veg = s.vegetation
        for i in sorted(veg.trees):          # row order = back-to-front
            p = veg.lineage(i)
            spr, ax, ay = plant_sprite(p, s.pl[i])
            surf.blit(spr, (i % W - ax, i // W - ay))
        for c in s.crs: draw_cr(surf, c, s.t, s.dark, c.x, c.y)
        sel = s.sel
        if sel and not sel.dead:
            pygame.draw.circle(surf, (255, 255, 255), (int(sel.x), int(sel.y)), int(sel.g['vis']), 1)
            r = int(sel.s) + 3
            pygame.draw.rect(surf, (255, 255, 80), (int(sel.x) - r, int(sel.y) - r, 2 * r, 2 * r), 1)


_SH = {}
_PLANT_SPR = {}


def plant_sprite(p, growth):
    """Procedural tree/shrub/kelp sprite from heritable architecture; cached per lineage."""
    key = (p.id, growth)
    hit = _PLANT_SPR.get(key)
    if hit: return hit
    if len(_PLANT_SPR) > 6000: _PLANT_SPR.clear()
    rng = random.Random(p.id * 7919 + growth)
    form = p.form
    age = .7 if growth < 3 else 1.
    if form == 'kelp':
        h = int((5 + p.height * 12) * age)
        sw, sh = 7, h + 1
        spr = pygame.Surface((sw, sh), pygame.SRCALPHA)
        col, dark = hsv(p.hue, 60, 50), hsv(p.hue, 70, 35)
        for strand in range(1 + int(p.branching * 2)):
            x = 3 + strand - 1
            for y in range(h):
                xx = x + int(round(math.sin(y * .6 + strand) * (1 + p.droop)))
                if 0 <= xx < sw: spr.set_at((xx, sh - 1 - y), col if y % 3 else dark)
        res = (spr, 3, sh - 1)
        _PLANT_SPR[key] = res
        return res
    tree = form == 'tree'
    trunk_h = int(((3 + p.height * 9) if tree else (1 + p.height * 2)) * age)
    cw = (2 + p.crown * 6 + p.branching * 2) * (1 if tree else .7) * age      # crown half-width
    ch = (3 + (1 - p.crown) * 7 + p.height * 3) * (1 if tree else .55) * age  # crown height
    cw, ch = max(1.5, cw), max(2., ch)
    sw = int(2 * cw + 5 + p.droop * 4)
    sh = int(trunk_h + ch + 3 + p.droop * ch)
    spr = pygame.Surface((sw, sh), pygame.SRCALPHA)
    cx, base = sw // 2, sh - 1
    bark, bark2 = hsv(25 + p.hue * .05, 50, 38), hsv(25, 45, 26)
    girth = 1 + (p.girth > .45) + (p.girth > .8 and tree)
    for y in range(trunk_h + int(ch * .4)):
        for k in range(girth):
            spr.set_at((cx - girth // 2 + k, base - y), bark2 if k == 0 else bark)
    if girth > 1: spr.set_at((cx - girth // 2 - 1, base), bark2); spr.set_at((cx + girth - girth // 2, base), bark2)
    # limbs
    top = base - trunk_h
    for b in range(int(1 + p.branching * 4)):
        side = -1 if b % 2 else 1
        by = top - int(rng.random() * ch * .6)
        ln = int(cw * (.4 + rng.random() * .6))
        for t in range(ln):
            x, y = cx + side * t, by - int(t * (1 - p.crown) * .8) + int(t * p.droop * .5)
            if 0 <= x < sw and 0 <= y < sh: spr.set_at((x, y), bark)
    # foliage: blob clusters inside a crown silhouette shaped by crown/droop
    leaf = hsv(p.hue, 50 + p.density * 25, 40 + p.density * 15)
    light = hsv(p.hue + 8, 40 + p.density * 20, 62 + p.density * 15)
    shadow = hsv(p.hue - 6, 65, 26 + p.density * 8)
    ccy = top - ch * .55
    clusters = int(6 + p.density * 14 + p.branching * 6)
    for _ in range(clusters):
        a, r = rng.random() * 2 * math.pi, rng.random() ** .6
        dx = math.cos(a) * r * cw
        # conical (low crown) narrows toward the top
        vy = math.sin(a) * r * ch * .5
        if p.crown < .4: dx *= max(.15, .5 + vy / ch + (1 - p.crown) * .2) if vy < 0 else 1
        x, y = cx + dx, ccy + vy
        rad = max(1, int(1 + p.density * 2 * rng.random() + (1 if tree else 0)))
        pygame.draw.circle(spr, shadow, (int(x), int(y) + 1), rad)
        pygame.draw.circle(spr, leaf, (int(x), int(y)), rad)
        if y < ccy and rng.random() < .6: spr.set_at((int(x) - rad // 2, int(y) - rad // 2), light)
        if p.droop > .45 and rng.random() < p.droop:   # weeping curtains
            for t in range(int(p.droop * ch * .9)):
                yy = int(y) + rad + t
                if 0 <= int(x) < sw and yy < base - 1: spr.set_at((int(x), yy), leaf if t % 2 else shadow)
    if p.bloom > .35 and growth >= 3:
        bc = hsv(p.bloom_hue, 70, 95)
        for _ in range(int(p.bloom * 10)):
            a, r = rng.random() * 2 * math.pi, rng.random() ** .5
            x, y = int(cx + math.cos(a) * r * cw), int(ccy + math.sin(a) * r * ch * .45)
            if 0 <= x < sw and 0 <= y < sh: spr.set_at((x, y), bc)
    res = (spr, cx, base)
    _PLANT_SPR[key] = res
    return res


def _shadow(w):
    sf = _SH.get(w)
    if sf is None:
        sf = _SH[w] = pygame.Surface((w, 1), pygame.SRCALPHA)
        sf.fill((0, 0, 0, 80))
    return sf


BONE, WHITE = (232, 226, 196), (245, 245, 245)


def draw_cr(surf, c, t, dark, cx, cy):
    """Pixel sprite built from the genome: every visual gene is a body part."""
    g, q, f = c.g, c.g['q'], c.f
    if g.get('complexity', .8) < .22:
        x, y = int(cx), int(cy)
        surf.fill(c.col, (x, y, 2, 2))
        surf.set_at((x, y), c.lt)
        return
    cy -= getattr(c, 'z', 0)
    gr = min(1.0, c.age / g['mat'])
    juv = gr < 1
    w = max(4, int(round(c.s * CSCALE)))
    h = max(3, int(round(w * g['body']))) + (1 if 'Glutton' in q and w >= 4 else 0)
    legl = 1 + (g['legs'] > .7)
    x0, y0 = int(cx) - w // 2, int(cy) - (h + legl) // 2
    col, dk, lt, c2 = c.col, c.dk, c.lt, c.c2
    moving = abs(c.vx) + abs(c.vy) > .08
    ph = (t // 4) & 1 if moving else 0
    surf.blit(_shadow(w), (x0, y0 + h + legl))
    # legs (webbed cyan for swimmers)
    lc = (60, 200, 220) if 'Swimmer' in q else dk
    surf.fill(lc, (x0 + 1 + ph, y0 + h, 1, legl))
    surf.fill(lc, (x0 + w - 2 - ph, y0 + h, 1, legl))
    # tail
    tl = g['tail'] * (.4 + .6 * gr)
    if tl > .2:
        n = 1 + int(tl * 3)
        tx = x0 - 1 if f > 0 else x0 + w
        for i in range(n):
            wag = 1 if (i == n - 1 and n > 1 and (t // 7) & 1) else 0
            surf.set_at((tx - f * i, y0 + 1 - wag - (1 if tl > .8 and i == n - 1 else 0)), c2 if i == n - 1 else col)
    # body (rounded corners)
    if w >= 4:
        surf.fill(col, (x0 + 1, y0, w - 2, h)); surf.fill(col, (x0, y0 + 1, w, h - 2))
    else:
        surf.fill(col, (x0, y0, w, h))
    pt = g['pat']
    if pt == 1:
        for xx in range(x0 + 1, x0 + w - 1, 2): surf.fill(c2, (xx, y0, 1, h))
    elif pt == 2:
        surf.set_at((x0 + 1, y0 + 1), c2); surf.set_at((x0 + w // 2, y0), c2)
        if w >= 5: surf.set_at((x0 + w - 2, y0 + h - 2), c2)
    elif pt == 3:
        surf.fill(lt, (x0 + 1, y0 + h - 1, w - 2, 1))
    elif pt == 4:
        surf.fill(c2, (x0 + w - 2 if f > 0 else x0, y0, 2, h))
    elif pt == 5:
        surf.fill(c2, (x0 + 1, y0, w - 2, 1))
    if 'Thick-hide' in q: surf.fill(dk, (x0 + 1, y0 + h - 1, w - 2, 1))
    if 'Venomous' in q:
        surf.set_at((x0 + 1, y0), (110, 230, 90)); surf.set_at((x0 + w - 2, y0 + h - 1), (110, 230, 90))
    if 'Prolific' in q: surf.set_at((x0 + w // 2, y0 + h - 1), (255, 170, 200))
    if c.preg: surf.set_at((x0 + w // 2, y0 + h), lt)                       # pregnant belly
    surf.set_at((x0 + 1 + c.mk % max(1, w - 2), y0 + 1), c.mkc)            # individual birthmark
    # head side
    hx = x0 + w - 1 if f > 0 else x0
    ex = x0 + w - 2 if f > 0 else x0 + 1
    ey = y0 + (1 if h >= 3 else 0)
    if w >= 5: surf.set_at((x0 + w if f > 0 else x0 - 1, y0 + h - 2), col)       # snout
    # ears / horns
    ear = max(g['ears'], .75) if 'Skittish' in q else g['ears']
    ax = x0 + w - 2 if f > 0 else x0 + 1
    if ear > .3:
        surf.set_at((ax, y0 - 1), col)
        if ear > .65: surf.set_at((ax, y0 - 2), dk)
        if ear > .5 and w >= 6: surf.set_at((x0 + w - 4 if f > 0 else x0 + 3, y0 - 1), col)
    hn = g['horn'] * gr
    if hn > .4 and w >= 4:
        bx = x0 + w - 3 if f > 0 else x0 + 2
        surf.set_at((bx, y0 - 1), BONE)
        if hn > .7: surf.set_at((bx + f, y0 - 2), BONE)
        if hn > .9 and w >= 6: surf.set_at((bx - 2 * f, y0 - 1), BONE)
    if 'Spiky' in q:
        for xx in range(x0 + 1, x0 + w - 1, 2): surf.set_at((xx, y0 - 1), (240, 240, 240))
    if 'Swimmer' in q: surf.set_at((x0 + (1 if f > 0 else w - 2), y0 - 1), (60, 220, 230))
    # eyes (babies get big ones)
    e = g['eyes'] + (.35 if juv else 0)
    if c.rest:
        surf.set_at((ex, ey), dk)
        surf.set_at((x0 + w, y0 - 3), WHITE); surf.set_at((x0 + w + 1, y0 - 4), WHITE)
    else:
        pc = (235, 40, 40) if g['diet'] > .55 else (255, 230, 90) if ('Nocturnal' in q and dark > .4) else (15, 15, 25)
        if e > .5 and w >= 4: surf.set_at((ex - f, ey), WHITE)
        surf.set_at((ex, ey), pc)
        if e > .85 and h >= 4: surf.set_at((ex, ey + 1), pc)
        if g['diet'] < .3 and w >= 5: surf.set_at((ex, min(y0 + h - 1, ey + 2)), (255, 150, 170))      # blush
    if g['diet'] > .6 and w >= 5: surf.set_at((hx, y0 + h - 1), WHITE)                                 # fang
    if w >= 5 and ('Scavenger' in q or 'Herder' in q):                                                 # scarf
        surf.fill((190, 110, 230) if 'Scavenger' in q else (120, 170, 255), (x0 + w - 3 if f > 0 else x0 + 2, y0 + h - 2, 1, 2))
    # Acquired limbs use the same one-pixel grid as the original body.
    if g.get('wings', 0) > .35:
        wing = max(1, int(g['wings']*4))
        wy = y0-1-((t//4)&1 if getattr(c,'z',0) > 0 else 0)
        surf.fill(c.c2, (x0+w//2-wing, wy, wing, 1))
        surf.fill(c.lt, (x0+w//2-wing+1, wy-1, max(1,wing-1), 1))
    if g.get('fins', 0) > .35:
        surf.fill(c.c2, (x0+1, y0+h, 2, 1))
        surf.fill(c.c2, (x0+w-3, y0+h, 2, 1))
    if g.get('gills',0) > .55:
        surf.set_at((x0+w-3 if f > 0 else x0+2, y0+1), c.dk)
    if c.sick:
        surf.set_at((x0+w//2,y0-4), (145,205,90))
    # status emotes
    mx = x0 + w // 2
    if juv is False and c.cool > 480:
        for dx, dy in ((-1, -6), (1, -6), (-1, -5), (0, -5), (1, -5), (0, -4)): surf.set_at((mx + dx, y0 + dy), (255, 90, 130))
    elif c.thr is not None:
        surf.set_at((x0 - 1 if f > 0 else x0 + w, y0 - 1), (150, 220, 255))


PCOL = [(0, 0, 0, 0), (52, 96, 50, 255), (60, 138, 58, 255), (112, 196, 70, 255)]

# ---------------------------------------------------------------- camera
def new_view():
    return {'z': min(MAPW / W, MAPH / H), 'x': 0.0, 'y': 0.0, 'drag': None}


def clamp_view(v):
    vw, vh = MAPW / v['z'], MAPH / v['z']
    v['x'] = (W - vw) / 2 if vw >= W else min(max(v['x'], 0), W - vw)
    v['y'] = (H - vh) / 2 if vh >= H else min(max(v['y'], 0), H - vh)


def zoom_at(v, mx, my, f):
    wx, wy = v['x'] + mx / v['z'], v['y'] + my / v['z']
    v['z'] = min(ZMAX, max(ZMIN, v['z'] * f))
    v['x'], v['y'] = wx - mx / v['z'], wy - my / v['z']
    clamp_view(v)


def to_world(v, mx, my):
    return v['x'] + mx / v['z'], v['y'] + my / v['z']


# ---------------------------------------------------------------- UI panel
BG, FG, DIM = (14, 16, 24), (205, 214, 228), (115, 124, 140)


def txt(surf, font, s, x, y, col=FG):
    surf.blit(font.render(s, False, col), (x, y))


def bar(surf, x, y, w, frac, col, h=6):
    pygame.draw.rect(surf, (36, 40, 54), (x, y, w, h))
    pygame.draw.rect(surf, col, (x, y, int(w * max(0, min(1, frac))), h))


def draw_graph(surf, rect, hist):
    x, y, w, h = rect
    pygame.draw.rect(surf, (20, 23, 34), rect)
    if len(hist) < 2: return
    mx = max(10, max(max(r[:3]) for r in hist))
    pm = max(1, max(r[3] for r in hist))
    for j, col in enumerate(((90, 200, 110), (235, 80, 80), (240, 200, 90), (80, 130, 70))):
        pts = [(x + i * (w - 1) / 219, y + h - 2 - (r[j] / mx if j < 3 else r[j] / pm * .6) * (h - 4)) for i, r in enumerate(hist)]
        pygame.draw.lines(surf, col, False, pts, 1)


def draw_tree(sim, surf, f, fs, rect):
    x0, y0, w, h = rect
    pygame.draw.rect(surf, (20, 23, 34), rect)
    sps = sim.species
    vis = {p.id for p in sps if p.peak >= 6}
    for i in list(vis):
        p = sps[i].parent
        while p is not None and p not in vis:
            vis.add(p); p = sps[p].parent
    kids = {}
    for i in vis:
        kids.setdefault(sps[i].parent, []).append(i)
    MAXR = 12
    while len(vis) > MAXR:                       # prune oldest extinct leaves, smallest first
        leaves = [i for i in vis if sps[i].pop == 0 and i not in kids]
        if not leaves: break
        d = min(leaves, key=lambda i: (sps[i].peak, sps[i].born))
        vis.discard(d)
        kids = {}
        for i in vis: kids.setdefault(sps[i].parent, []).append(i)
    order, stack = [], sorted(kids.get(None, []), key=lambda i: -sps[i].born)
    while stack:
        i = stack.pop(); order.append(i)
        stack += sorted(kids.get(i, []), key=lambda j: -sps[j].born)
    if not order: return
    rh = min(17, (h - 8) / len(order))
    row = {i: y0 + 6 + (k + .5) * rh for k, i in enumerate(order)}
    now = max(sim.t, 1)
    X = lambda t: x0 + 6 + (w - 14) * t / now
    clip = surf.get_clip(); surf.set_clip(rect)
    for i in order:
        p = sps[i]
        a = p.pop > 0
        col = p.col if a else tuple(int(v * .45) for v in p.col)
        xs, xe, y = X(p.born), X(p.died if p.died is not None else now), row[i]
        if p.parent in row:
            pygame.draw.line(surf, col, (xs, row[p.parent]), (xs, y), 1)
        pygame.draw.line(surf, col, (xs, y), (xe, y), 2 if a else 1)
        if not a:
            pygame.draw.line(surf, (200, 80, 80), (xe - 2, y - 2), (xe + 2, y + 2), 1)
            pygame.draw.line(surf, (200, 80, 80), (xe - 2, y + 2), (xe + 2, y - 2), 1)
        label = p.name + (' ' + p.note if p.note else '') + ('  n=%d' % p.pop if a else '')
        lw = fs.size(label)[0]
        surf.blit(fs.render(label, False, col if a else DIM), (min(xs + 4, x0 + w - lw - 3), y - 11))
    surf.set_clip(clip)


def persona(c):
    b, so, cu = c.pers
    w = ['bold' if b > .65 else 'shy' if b < .35 else None, 'social' if so > .65 else 'loner' if so < .35 else None,
         'curious' if cu > .65 else 'cautious' if cu < .35 else None]
    w = [x for x in w if x]
    return ', '.join(w) if w else 'easy-going'


def draw_inspector(sim, surf, f, fs, x, y, w):
    c = sim.sel
    if c is None:
        for i, l in enumerate(('click creature = inspect | drag = pan | wheel/+/- = zoom',
                               'arrows pan | 0 reset view | RMB seeds | H/C founders',
                               'T or click tree = fullscreen tree | N world size',
                               '1-7 speed | SPACE pause | F5 save | F9 load')):
            txt(surf, fs, l, x, y + 4 + i * 14, DIM)
        return
    g, sp = c.g, c.sp
    pw, ph = 20, 16
    ps = pygame.Surface((pw, ph))
    ps.fill((30, 38, 34))
    draw_cr(ps, c, sim.t, sim.dark, pw // 2, ph // 2 + 1)
    surf.blit(pygame.transform.scale(ps, (pw * 5, ph * 5)), (x, y))
    pygame.draw.rect(surf, (50, 56, 76), (x, y, pw * 5, ph * 5), 1)
    tx = x + 108
    mass_g = MK * c.s ** 3 * 1000
    txt(surf, f, '%s' % c.nm, tx, y, sp.col)
    txt(surf, fs, '%s  gen %d  %s' % (sp.name, c.gen, '[dead]' if c.dead else ''), tx, y + 17, DIM)
    txt(surf, fs, '%s %s%s' % ('male' if c.sex == 'M' else 'female', 'adult' if c.age >= g['mat'] else 'juvenile',
                               '  PREGNANT x%d' % len(c.preg[1]) if c.preg else ''), tx, y + 30, FG)
    txt(surf, fs, '%.1f cm  %s  %.0f cm/s' % (c.s * CELL_CM, ('%.0f g' % mass_g) if mass_g < 1000 else '%.2f kg' % (mass_g / 1000),
                                               g['spd'] * CELL_CM / TICK_S), tx, y + 43, FG)
    txt(surf, fs, 'age %.1f/%.1f d  hetero %.2f' % (c.age / DAY, g['life'] / DAY, c.het), tx, y + 56, DIM)
    txt(surf, fs, 'energy', tx, y + 69, DIM)
    bar(surf, tx + 52, y + 71, 130, c.e / c.maxE, (240, 190, 60), 7)
    txt(surf, fs, persona(c), tx, y + 82, (160, 230, 160))
    qx, qy = x, y + 98
    txt(surf, fs, 'quirks', qx, qy, DIM)
    qx += 50
    if not g['q']: txt(surf, fs, 'none', qx, qy, DIM)
    for qn in sorted(g['q']):
        if qx + fs.size(qn)[0] > x + w: qx, qy = x + 50, qy + 12
        txt(surf, fs, qn, qx, qy, QUIRKS[qn]); qx += fs.size(qn)[0] + 8
    gy = qy + 18
    rows = [('speed', 'spd'), ('size', 'size'), ('vision', 'vis'), ('diet', 'diet'), ('metab', 'metab'), ('aggr', 'aggr'),
            ('climate', 'tol'), ('care', 'care'), ('choosy', 'choosy'), ('breed', 'thresh'),
            ('bold', 0), ('social', 1), ('curious', 2), ('horns', 'horn'), ('tail', 'tail'), ('ears', 'ears'),
            ('legs', 'legs'), ('eyes', 'eyes')]
    for i, (lab, k) in enumerate(rows):
        cx, cy = x + (i % 2) * 195, gy + (i // 2) * 13
        txt(surf, fs, lab, cx, cy - 2, DIM)
        if isinstance(k, int):
            frac, col = c.pers[k], (140, 220, 150)
        else:
            lo, hi = G[k]
            frac, col = (g[k] - lo) / (hi - lo), (220, 80, 80) if k == 'diet' else (200, 140, 240) if k in VK else (110, 170, 240)
        bar(surf, cx + 56, cy + 1, 114, frac, col, 6)
    ay = gy + 9 * 13 + 2
    txt(surf, fs, 'ancestry (newest first):', x, ay, DIM)
    cur, parts = c.id, []
    while cur in sim.lin and len(parts) < 24:
        par, gen, spid, note, born = sim.lin[cur]
        parts.append((gen, note, spid))
        if par == 0: break
        cur = par
    cx, cy = x, ay + 13
    for gen, note, spid in parts[:14]:
        s_ = 'g%d %s ' % (gen, note or '-')
        if note.startswith('!'): col = (255, 210, 120)
        elif note[:1] in '+-': col = (160, 230, 160)
        else: col = FG
        wd = fs.size(s_)[0]
        if cx + wd > x + w:
            cx, cy = x, cy + 12
            if cy > ay + 38: break
        txt(surf, fs, s_, cx, cy, col); cx += wd + 5


# ---------------------------------------------------------------- fullscreen phylogeny
HDR = 72


def new_tv():
    return {'fs': False, 'zx': 1.0, 'zy': 1.0, 'px': 0.0, 'py': 0.0, 'pin': None, 'ex': True, 'mn': False,
            'kind': 'all', 'tabs': [], 'drag': None, 'geo': None, 'hits': [], 'close': pygame.Rect(0, 0, 0, 0)}


def tree_layout(sim, show_ex, minpeak):
    sps = sim.species
    vis = {p.id for p in sps if p.pop > 0 or (show_ex and p.peak >= minpeak)}
    for i in list(vis):
        p = sps[i].parent
        while p is not None and p not in vis:
            vis.add(p); p = sps[p].parent
    kids = {}
    for i in vis: kids.setdefault(sps[i].parent, []).append(i)
    order, stack = [], sorted(kids.get(None, []), key=lambda i: -sps[i].born)
    while stack:
        i = stack.pop(); order.append(i)
        stack += sorted(kids.get(i, []), key=lambda j: -sps[j].born)
    return order, kids


def dummy_cr(g, sp):
    c = Cr()
    c.id, c.g, c.x, c.y, c.vx, c.vy, c.f = 0, g, 0.0, 0.0, 0.0, 0.0, 1
    c.age, c.s = int(g['mat']) + 10, g['size']
    c.maxE = 25 + 13 * c.s
    c.e = c.maxE
    c.cool, c.sp, c.tgt, c.thr, c.dead, c.gut, c.wa, c.rest = 0, sp, None, None, False, [], 0.0, False
    h = g['hue']
    c.col, c.dk, c.lt = hsv(h, 55 + 30 * g['diet'], 88 - 8 * g['diet']), hsv(h, 70, 50), hsv(h, 30, 100)
    c.c2 = hsv(g['hue2'], 62, 92)
    c.sex, c.preg, c.nut, c.pf, c.mom, c.nurse, c.hap = 'M', None, .6, 1.0, None, 0, None
    c.pers, c.nm, c.mk, c.mkc, c.kids, c.het, c.m34 = (.5, .5, .5), '', 3, c.lt, [], 0.0, 0.5
    c.sick, c.imm, c.fv = None, {}, None
    return c


def portrait(c, scale, t=0, dark=0.0, pw=20, ph=16):
    ps = pygame.Surface((pw, ph))
    ps.fill((30, 38, 34))
    draw_cr(ps, c, t, dark, pw // 2, ph // 2 + 1)
    return pygame.transform.scale(ps, (pw * scale, ph * scale))


def species_card(sim, surf, f, fs, sp, x, y, w):
    sps = sim.species
    txt(surf, f, sp.name, x, y, sp.col)
    txt(surf, fs, ('branched from ' + sps[sp.parent].name) if sp.parent is not None else 'root species (starting population)', x, y + 19, DIM)
    surf.blit(portrait(dummy_cr(sp.founder, sp), 6), (x, y + 36))
    pygame.draw.rect(surf, (50, 56, 76), (x, y + 36, 120, 96), 1)
    txt(surf, fs, 'founder', x + 4, y + 38, DIM)
    end = sp.died if sp.died is not None else sim.t
    info = (('born', 'day %d  t=%d' % (sp.born // DAY + 1, sp.born)),
            ('status', ('alive  n=%d' % sp.pop) if sp.pop > 0 else 'extinct t=%d' % sp.died),
            ('peak', str(sp.peak)), ('lasted', '%.1f days' % ((end - sp.born) / DAY)),
            ('trait', sp.note or 'founder'), ('kids', '%d species' % sum(1 for o in sps if o.parent == sp.id)))
    for i, (k, v) in enumerate(info):
        txt(surf, fs, k, x + 130, y + 38 + i * 15, DIM)
        txt(surf, fs, v, x + 130 + 46, y + 38 + i * 15, FG)
    members = [c for c in sim.crs if c.sp is sp]
    my = y + 142
    if members:
        txt(surf, fs, 'living members (%d)' % len(members), x, my, DIM)
        pick = members[::max(1, len(members) // 3)][:3]
        for j, c in enumerate(pick):
            surf.blit(portrait(c, 4, sim.t, sim.dark), (x + j * 90, my + 14))
        my += 14 + 64 + 8
    else:
        txt(surf, fs, 'no living members', x, my, (200, 90, 90))
        my += 22
    g = sp.founder
    txt(surf, fs, 'founder genome', x, my, DIM)
    rows = [('speed', 'spd'), ('size', 'size'), ('vision', 'vis'), ('diet', 'diet'), ('metab', 'metab'), ('aggr', 'aggr'),
            ('breed', 'thresh'), ('body', 'body'), ('ears', 'ears'), ('horns', 'horn'), ('tail', 'tail'),
            ('legs', 'legs'), ('eyes', 'eyes'), ('mutab', 'mut')]
    for i, (lab, k) in enumerate(rows):
        cx, cy = x + (i % 2) * 150, my + 16 + (i // 2) * 13
        txt(surf, fs, lab, cx, cy - 2, DIM)
        lo, hi = G[k]
        bar(surf, cx + 44, cy + 1, 90, (g[k] - lo) / (hi - lo),
            (220, 80, 80) if k == 'diet' else (200, 140, 240) if k in VK else (110, 170, 240), 6)
    my += 16 + 7 * 13 + 6
    qx = x
    txt(surf, fs, 'quirks', qx, my, DIM); qx += 50
    if not g['q']: txt(surf, fs, 'none', qx, my, DIM)
    for qn in sorted(g['q']):
        if qx + fs.size(qn)[0] > x + w: qx, my = x + 50, my + 12
        txt(surf, fs, qn, qx, my, QUIRKS[qn]); qx += fs.size(qn)[0] + 8
    chain, q = [], sp
    while q is not None:
        chain.append(q.name); q = sps[q.parent] if q.parent is not None else None
    my += 20
    txt(surf, fs, 'lineage', x, my, DIM)
    line, ly = '', my + 13
    for nm in reversed(chain):
        piece = nm + ' > '
        if fs.size(line + piece)[0] > w:
            txt(surf, fs, line, x, ly); ly += 12; line = ''
        line += piece
    txt(surf, fs, line.rstrip(' >'), x, ly)
    if sp.pop > 0:
        txt(surf, fs, 'ENTER: jump to a living member', x, ly + 22, (120, 220, 160))


def draw_tree_fs(sim, screen, f, fs, tv, mp):
    SW, SH = screen.get_size()
    AW = SW - 320
    screen.fill(BG)
    order, kids = tree_layout(sim, tv['ex'], 1 if tv['mn'] else 6)
    sps = sim.species
    if tv['kind'] != 'all':
        microbial = tv['kind'] == 'microbial'
        order = [i for i in order if (sps[i].founder.get('complexity',.8) < .42) == microbial]
    n = len(order)
    RH0 = max(14.0, min(26.0, (SH - HDR - 40) / max(1, n)))
    TW, X0, Y0 = AW - 120, 50, HDR + 12
    now = max(sim.t, 1)
    tv['geo'] = (X0, TW, Y0, RH0, AW)
    zx, zy = tv['zx'], tv['zy']
    tv['px'] = max(-60.0, min(tv['px'], TW * zx - AW + 170))
    tv['py'] = max(-30.0, min(tv['py'], max(0.0, n * RH0 * zy + HDR + 50 - SH)))
    px, py = tv['px'], tv['py']
    ppt = TW * zx / now
    X = lambda t: X0 + t * ppt - px
    row = {i: Y0 + (k + .5) * RH0 * zy - py for k, i in enumerate(order)}
    items, hover, hd = {}, None, 1e9
    tv['hits'] = []
    for i in order:
        p = sps[i]
        xs, xe, y = X(p.born), X(p.died if p.died is not None else now), row[i]
        label = p.name + (' ' + p.note if p.note else '') + (('  n=%d' % p.pop) if p.pop > 0 else '  x')
        lw = fs.size(label)[0]
        lx = min(max(xs + 6, 6), AW - lw - 6)
        items[i] = (xs, xe, y, label, lx, lw)
        lo_x = min(xs, lx) - 4
        r = pygame.Rect(int(max(lo_x, -5000)), int(y - 8), int(min(max(xe, lx + lw) + 4, AW + 5000) - max(lo_x, -5000)), 16)
        tv['hits'].append((i, r))
        if mp[0] < AW and HDR < mp[1] < SH - 18 and r.collidepoint(mp) and abs(mp[1] - y) < hd:
            hover, hd = i, abs(mp[1] - y)
    focus = tv['pin'] if tv['pin'] is not None else hover
    chain = set()
    if focus is not None:
        q = focus
        while q is not None: chain.add(q); q = sps[q].parent
        st = [focus]
        while st:
            j = st.pop()
            for k in kids.get(j, ()): chain.add(k); st.append(k)
    screen.set_clip((0, HDR, AW, SH - HDR - 18))
    # time axis
    for st_t in (DAY // 4, DAY // 2, DAY, 2 * DAY, 5 * DAY, 10 * DAY, 25 * DAY, 50 * DAY, 100 * DAY, 250 * DAY, 1000 * DAY):
        if st_t * ppt >= 80: break
    k0, k1 = max(0, int((px - X0) / ppt / st_t)), int((px + AW - X0) / ppt / st_t) + 1
    for k in range(k0, k1 + 1):
        if k * st_t > now: break
        gx = X(k * st_t)
        pygame.draw.line(screen, (26, 30, 44), (gx, HDR), (gx, SH))
        txt(screen, fs, 'day %g' % round(k * st_t / DAY, 2), gx + 3, HDR + 1, (70, 78, 98))
    pygame.draw.line(screen, (60, 90, 70), (X(now), HDR), (X(now), SH))
    dim = lambda c, m: tuple(int(v * m) for v in c)
    for i in order:                                    # connectors + branches
        p = sps[i]
        a = p.pop > 0
        base = p.col if a else dim(p.col, .55)
        col = base if (focus is None or i in chain) else dim(base, .3)
        xs, xe, y = items[i][:3]
        if p.parent in row: pygame.draw.line(screen, col, (xs, row[p.parent]), (xs, y), 1)
        th = min(6, 2 + int(math.sqrt(p.pop) / 3)) if a else 1
        pygame.draw.line(screen, col, (max(xs, -5000), y), (min(xe, AW + 5000), y), th)
        if -10 < xs < AW + 10: pygame.draw.circle(screen, col, (int(xs), int(y)), 3)
        if not a and -10 < xe < AW + 10:
            pygame.draw.line(screen, (200, 80, 80), (xe - 3, y - 3), (xe + 3, y + 3), 1)
            pygame.draw.line(screen, (200, 80, 80), (xe - 3, y + 3), (xe + 3, y - 3), 1)
    for i in order:                                    # labels
        p = sps[i]
        xs, xe, y, label, lx, lw = items[i]
        if xe < 0 or xs > AW: continue
        a = p.pop > 0
        base = p.col if a else dim(p.col, .7)
        col = base if (focus is None or i in chain) else dim(base, .35)
        txt(screen, fs, label, lx, y - 12, col)
        if i == focus: pygame.draw.rect(screen, (255, 255, 255), (lx - 3, y - 14, lw + 6, 15), 1)
    screen.set_clip(None)
    # header / footer / preview panel
    pygame.draw.rect(screen, BG, (0, 0, SW, HDR))
    pygame.draw.line(screen, (50, 56, 76), (0, HDR), (AW, HDR))
    alive = sum(1 for p in sps if p.pop > 0)
    txt(screen, f, 'PHYLOGENY', 10, 6, (120, 220, 160))
    txt(screen, fs, '%d shown / %d alive / %d total     zoom x%.1f y%.1f' % (n, alive, len(sps), zx, zy), 120, 9, DIM)
    txt(screen, fs, 'time ->  (thick line = big population, x = extinct, dots = where a species branched off)', 10, 26, DIM)
    tv['close'] = pygame.Rect(SW - 34, 6, 26, 22)
    pygame.draw.rect(screen, (40, 46, 64), tv['close'])
    txt(screen, f, 'X', SW - 27, 8, FG)
    pygame.draw.rect(screen, BG, (0, SH - 18, AW, 18))
    txt(screen, fs, 'wheel zoom (shift: time only, ctrl: rows only) | drag pan | click pin | 0 fit | E extinct:%s | M minor:%s | ESC close' %
        ('on' if tv['ex'] else 'off', 'on' if tv['mn'] else 'off'), 8, SH - 15, DIM)
    pygame.draw.line(screen, (50, 56, 76), (AW, 0), (AW, SH))
    if focus is not None:
        species_card(sim, screen, f, fs, sps[focus], AW + 14, 12, 296)
    else:
        for i, l in enumerate(('hover a species to preview it', 'click to pin it', '', 'highlighted = its ancestors and',
                               'all species that descended from it')):
            txt(screen, fs, l, AW + 14, 14 + i * 14, DIM)


def tree_key(e, sim, tv, view):
    if e.key == pygame.K_TAB:
        kinds = ['all','microbial','animal','plant']
        switch_phylogeny(tv,kinds[(kinds.index(tv['kind'])+1)%len(kinds)])
    elif e.key == pygame.K_e: tv['ex'] = not tv['ex']
    elif e.key == pygame.K_m: tv['mn'] = not tv['mn']
    elif e.key == pygame.K_0: tv.update(zx=1.0, zy=1.0, px=0.0, py=0.0)
    elif e.key in (pygame.K_RETURN, pygame.K_KP_ENTER) and tv['pin'] is not None:
        if tv['kind'] == 'plant':
            members = [i for i,st in enumerate(sim.pl) if st and sim.vegetation.ids[i] == tv['pin']]
            if members:
                i = members[0]
                view.update(z=8., x=i%W-MAPW/16, y=i//W-MAPH/16)
                clamp_view(view)
                tv['fs'] = False
            return
        mem = [c for c in sim.crs if c.sp is sim.species[tv['pin']]]
        if mem:
            c = max(mem, key=lambda c: c.age)
            sim.sel = c
            view['z'] = 8.0
            view['x'], view['y'] = c.x - MAPW / 16, c.y - MAPH / 16
            clamp_view(view)
            tv['fs'] = False


def tree_mouse(e, sim, tv):
    if e.type == pygame.MOUSEBUTTONDOWN and e.button == 1:
        for kind,rect in tv['tabs']:
            if rect.collidepoint(e.pos):
                switch_phylogeny(tv,kind)
                return
    geo = tv['geo']
    if not geo: return
    X0, TW, Y0, RH0, AW = geo
    if e.type == pygame.MOUSEWHEEL:
        mx, my = pygame.mouse.get_pos()
        if mx >= AW or my < HDR: return
        mods, fct = pygame.key.get_mods(), 1.2 ** e.y
        zx, zy = tv['zx'], tv['zy']
        nzx = zx if mods & pygame.KMOD_CTRL else min(400.0, max(.4, zx * fct))
        nzy = zy if mods & pygame.KMOD_SHIFT else min(10.0, max(.4, zy * fct))
        u = (mx + tv['px'] - X0) / (TW * zx)
        v = (my + tv['py'] - Y0) / (RH0 * zy)
        tv['zx'], tv['zy'] = nzx, nzy
        tv['px'] = X0 + u * TW * nzx - mx
        tv['py'] = Y0 + v * RH0 * nzy - my
    elif e.type == pygame.MOUSEBUTTONDOWN and e.button == 1:
        if tv['close'].collidepoint(e.pos): tv['fs'] = False
        elif e.pos[0] < AW and e.pos[1] > HDR: tv['drag'] = [e.pos, False]
    elif e.type == pygame.MOUSEMOTION and tv['drag'] and e.buttons[0]:
        d = tv['drag']
        if d[1] or abs(e.pos[0] - d[0][0]) + abs(e.pos[1] - d[0][1]) > 5:
            d[1] = True
            tv['px'] -= e.rel[0]; tv['py'] -= e.rel[1]
    elif e.type == pygame.MOUSEBUTTONUP and e.button == 1 and tv['drag']:
        if not tv['drag'][1]:
            tv['pin'] = None
            for i, r in tv['hits']:
                if r.collidepoint(e.pos): tv['pin'] = i; break
        tv['drag'] = None


def switch_phylogeny(tv, kind):
    tv.update(kind=kind,pin=None,px=0.,py=0.,zx=1.,zy=1.,drag=None,geo=None,hits=[])


def draw_phylogeny_tabs(screen, fs, tv):
    tv['tabs'] = []
    for j,(kind,label) in enumerate((('all','All creatures'),('microbial','Microbial'),('animal','Animals'),('plant','Plants'))):
        rect = pygame.Rect(10+j*148,46,140,21)
        screen.fill((44,70,57) if tv['kind'] == kind else (29,34,46),rect)
        txt(screen,fs,label,rect.x+8,rect.y+4,(143,223,167) if tv['kind'] == kind else DIM)
        tv['tabs'].append((kind,rect))
    txt(screen,fs,'TAB: switch life form',610,51,DIM)


def draw_plant_phylogeny(sim,screen,f,fs,tv,mp):
    veg = sim.vegetation
    SW,SH = screen.get_size()
    AW = SW-320
    screen.fill(BG)
    visible = {p.id for p in veg.lineages if veg.counts[p.id] > 0 or (tv['ex'] and veg.peaks[p.id] >= (1 if tv['mn'] else 3))}
    for i in list(visible):
        parent = veg.lineages[i].parent
        while parent is not None and parent not in visible:
            visible.add(parent)
            parent = veg.lineages[parent].parent
    kids = {}
    for i in visible: kids.setdefault(veg.lineages[i].parent,[]).append(i)
    order,stack = [],list(reversed(kids.get(None,[])))
    while stack:
        i = stack.pop(); order.append(i)
        stack.extend(reversed(kids.get(i,[])))
    rh = max(14.,min(26.,(SH-HDR-40)/max(1,len(order))))
    tw,x0,y0 = AW-120,50,HDR+12
    tv['py'] = max(0,min(tv['py'],max(0,len(order)*rh*tv['zy']-SH+HDR+40)))
    tv['geo'] = (x0,tw,y0,rh,AW)
    now = max(1,sim.t)
    X = lambda tick: x0+tick/now*tw*tv['zx']-tv['px']
    rows = {i:y0+(n+.5)*rh*tv['zy']-tv['py'] for n,i in enumerate(order)}
    tv['hits'] = []
    hover = None
    screen.set_clip((0,HDR,AW,SH-HDR-20))
    for i in order:
        p = veg.lineages[i]
        y = rows[i]
        if y < HDR-25 or y > SH+25: continue
        live = veg.counts[i]
        end = now if live else veg.died[i] if veg.died[i] is not None else p.born
        col = hsv(p.hue,55,85 if live else 45)
        xs,xe = X(p.born),X(end)
        if p.parent in rows: pygame.draw.line(screen,col,(xs,rows[p.parent]),(xs,y),1)
        pygame.draw.line(screen,col,(xs,y),(max(xs+2,xe),y),min(5,1+int(math.sqrt(live)/3)))
        screen.fill(col,(int(xs)-1,int(y)-1,3,3))
        label = '%s #%d | gen %d | n=%d' % (p.form,i,p.generation,live)
        lx = min(max(5,xs+5),AW-fs.size(label)[0]-5)
        txt(screen,fs,label,lx,y-13,col)
        rect = pygame.Rect(int(min(xs,lx))-3,int(y)-15,int(max(xe,lx+fs.size(label)[0])-min(xs,lx))+6,20)
        tv['hits'].append((i,rect))
        if rect.collidepoint(mp): hover = i
    screen.set_clip(None)
    focus = tv['pin'] if tv['pin'] is not None else hover
    txt(screen,f,'PLANT PHYLOGENY',10,8,(130,218,143))
    txt(screen,fs,'%d lineages shown / %d living | seeds inherit and mutate' % (len(order),sum(n>0 for n in veg.counts)),200,11,DIM)
    if not order: txt(screen,fs,'Plant lineages have not established yet.',20,HDR+30,DIM)
    pygame.draw.line(screen,(50,56,76),(AW,0),(AW,SH))
    if focus is not None and focus < len(veg.lineages):
        p = veg.lineages[focus]
        x,y = AW+14,18
        txt(screen,f,'%s #%d' % (p.form, p.id),x,y,hsv(p.hue,55,90))
        for n,line in enumerate(('generation %d' % p.generation,'parent '+str(p.parent),'born day %.2f' % (p.born/DAY+1),'population %d / peak %d' % (veg.counts[p.id],veg.peaks[p.id]))):
            txt(screen,fs,line,x,y+28+n*17,DIM)
        for n,key in enumerate(('height','wood','moisture','temperature','aquatic','dispersal','defense','crown','droop','density','branching','girth','bloom')):
            yy = y+110+n*17
            txt(screen,fs,key,x,yy,DIM)
            bar(screen,x+88,yy+2,160,getattr(p,key),(108,184,106),7)
        txt(screen,fs,'ENTER: jump to a living plant',x,y+340,(120,220,160))
        if p.form in ('tree','shrub','kelp'):
            spr = plant_sprite(p, 3)[0]
            k = max(1, min(6, int(min(240/spr.get_width(), 200/spr.get_height()))))
            screen.blit(pygame.transform.scale(spr,(spr.get_width()*k,spr.get_height()*k)),(x,y+362))
    else:
        txt(screen,fs,'Hover or pin a plant lineage.',AW+14,18,DIM)
    tv['close'] = pygame.Rect(SW-34,6,26,22)
    screen.fill((40,46,64),tv['close'])
    txt(screen,f,'X',SW-27,8,FG)
    txt(screen,fs,'wheel zoom | drag pan | click pin | E extinct | M minor | 0 fit | ESC close',8,SH-15,DIM)


def draw_land_details(sim, surf):
    """Sparse one-cell accents share the original terrain and sprite pixel grid."""
    # Deterministic sampling avoids scanning every cell and consuming simulation RNG.
    for i in range(0, N, 43):
        x, y = i % W, i // W
        if sim.terr[i] == 1:
            if (sim.t // 16 + x) % 7 == 0:
                surf.fill((65, 133, 171), (x, y, 2, 1))
        elif sim.terr[i] == 2:
            surf.set_at((x, y), (132, 132, 142))
        elif sim.pl[i] == 0 and (i * 17) % 11 == 0:
            surf.set_at((x, y), (119, 105, 73))
    for i in sim.springs:
        x, y = i % W, i // W
        surf.fill((89, 165, 190), (x-1, y, 3, 1))
        surf.set_at((x, y-1), (125, 188, 205))


def draw_pixel_weather(sim, surf):
    """Weather is drawn at native world resolution before nearest-neighbor zoom."""
    atmosphere = sim.atmosphere
    # Persistent entities have world positions, water reservoirs and altitude.
    # Only their actual simulation state is drawn; no random screen rain.
    for i in atmosphere.snow:
        surf.set_at((i % W, i // W), (207, 224, 230))
    for cloud in atmosphere.clouds:
        x, y = int(cloud.x), int(cloud.y)-6
        w = max(6, int(cloud.radius*1.5))
        dark = cloud.water > 18
        edge = (94, 112, 128) if dark else (147, 166, 175)
        body = (133, 151, 162) if dark else (188, 204, 207)
        light = (153, 171, 180) if dark else (211, 220, 216)
        surf.fill(edge, (x-w//2+2, y+3, w-4, 2))
        surf.fill(body, (x-w//2, y, w, 3))
        surf.fill(body, (x-w//3, y-2, w*2//3, 2))
        surf.fill(light, (x-w//4, y-3, max(2,w//3), 1))
    for drop in atmosphere.drops:
        x, y = int(drop.x), int(drop.y-drop.z)
        surf.fill((211, 229, 235) if drop.snow else (103, 162, 201), (x, y, 1, 1 if drop.snow else 2))
    for x, y, ttl in atmosphere.splashes:
        surf.fill((87, 145, 178), (x, y, 2 if ttl > 2 else 1, 1))
    if sim.bolt:
        x, y, _ = sim.bolt
        for dx, dy, h in ((-2,-19,6),(-1,-13,5),(-2,-8,4),(-1,-4,4)):
            surf.fill((242, 233, 174), (x+dx,y+dy,1,h))
    if sim.quake:
        x, y, r, ttl = sim.quake
        # Short, stepped fissures replace the smooth expanding shockwave.
        for n in range(min(20, int(r)), 0, -1):
            xx, yy = int(x)+n-r//3, int(y)+n//3+(n//4)%2
            surf.fill((63, 52, 42), (xx, yy, 2, 1))
    if sim.flash > .05:
        flash = pygame.Surface((W, H), pygame.SRCALPHA)
        flash.fill((255, 245, 210, min(130, int(sim.flash*95))))
        surf.blit(flash, (0, 0))


class SimulationWorker:
    """One simulation owner; the UI holds the same lock while reading/editing state.

    Bounded batches avoid a backlog and allow input between batches. Python's GIL
    still limits CPU throughput; this worker isolates scheduling, not CPU cores.
    """
    def __init__(self):
        self.lock = threading.RLock()
        self.condition = threading.Condition(self.lock)
        self.pending = None
        self.continuous = None
        self.stopped = False
        self.error = None
        self.tps = 0.0
        self.thread = threading.Thread(target=self.run, name='evosim-simulation', daemon=True)
        self.thread.start()

    def submit(self, sim, ticks):
        with self.condition:
            if self.error: raise RuntimeError('Simulation worker failed') from self.error
            self.continuous = sim if ticks >= 10000 else None
            self.pending = None if self.continuous is not None else (sim, ticks)
            self.condition.notify()

    def pause(self):
        with self.condition:
            self.pending = self.continuous = None

    def run(self):
        try:
            while True:
                with self.condition:
                    self.condition.wait_for(lambda: self.stopped or self.pending is not None or self.continuous is not None)
                    if self.stopped: return
                    continuous = self.continuous is not None
                    sim, ticks = (self.continuous, 1000000) if continuous else self.pending
                    self.pending = None
                    start = time.perf_counter()
                    count = 0
                    while count < ticks:
                        if continuous: sim.step(8)
                        else: sim.step()
                        count += 8 if continuous else 1
                        if time.perf_counter() - start >= (.008 if continuous else .025): break
                    elapsed = time.perf_counter() - start
                    self.tps = count / max(elapsed, 1e-6)
                # Yield the lock and GIL so input/pause can interrupt continuous mode.
                time.sleep(0)
        except BaseException as exc:
            with self.lock: self.error = exc

    def close(self):
        with self.condition:
            self.stopped = True
            self.condition.notify()
        self.thread.join()


def draw_atmosphere(sim, screen, view, fs):
    """Compact status labels; weather itself uses the world pixel grid."""
    kind = sim.wx['kind']
    screen.set_clip((0, 0, MAPW, MAPH))
    if sim.banner:
        txt(screen, fs, sim.banner[0], 16, 40, sim.banner[2])
    if sim.sel and not sim.sel.dead:
        c = sim.sel
        temperature = sim.atmosphere.temperature(sim, int(c.y)*W+int(c.x))*60-20
        status = c.sick[0].name if c.sick else 'healthy'
        txt(screen, fs, 'immune %.0f%% | burial %.0f%% | wings %.0f%% | fins %.0f%%' % (c.g['immune']*100,c.g['burial']*100,c.g['wings']*100,c.g['fins']*100), 16, 90, DIM)
        txt(screen, fs, '%s | %s | oxygen %.0f%%' % (stage(c.g), habitat(c.g), c.breath*100), 16, 75, WHITE)
        txt(screen, fs, '%s | %.1f C | %s | corpse digestion %.0f%%' % (c.nm, temperature, status, c.g['scavenge']*100), 16, 60, WHITE)
    txt(screen, fs, '%s | regional %.1f C | sick %d | corpses %d | poop %d' % (kind, (.5+sim.dT)*60-20, sum(c.sick is not None for c in sim.crs), len(sim.cor), len(sim.poop)), 16, 12, WHITE)
    txt(screen, fs, 'clouds %d | drops %d | bones %d | plant lineages %d' % (len(sim.atmosphere.clouds),len(sim.atmosphere.drops),len(sim.bones),len(sim.vegetation.lineages)), 16, 25, DIM)
    screen.set_clip(None)


def render(sim, screen, world, night, fonts, speed, paused, view, tv):
    f, fs = fonts
    if tv['fs']:
        if tv['kind'] == 'plant': draw_plant_phylogeny(sim,screen,f,fs,tv,pygame.mouse.get_pos())
        else: draw_tree_fs(sim, screen, f, fs, tv, pygame.mouse.get_pos())
        draw_phylogeny_tabs(screen,fs,tv)
        return
    sim.draw_world(world)
    draw_pixel_weather(sim, world)
    night.fill((8, 12, 50, int(135 * sim.dark)))
    world.blit(night, (0, 0))
    z = view['z']
    x0, y0 = max(0, int(view['x'])), max(0, int(view['y']))
    x1, y1 = min(W, int(view['x'] + MAPW / z) + 2), min(H, int(view['y'] + MAPH / z) + 2)
    screen.fill((8, 10, 16), (0, 0, MAPW, MAPH))
    screen.set_clip((0, 0, MAPW, MAPH))
    crop = world.subsurface((x0, y0, x1 - x0, y1 - y0))
    screen.blit(pygame.transform.scale(crop, (int((x1 - x0) * z + .5), int((y1 - y0) * z + .5))),
                (int((x0 - view['x']) * z), int((y0 - view['y']) * z)))
    screen.set_clip(None)
    draw_atmosphere(sim, screen, view, fs)
    cm_px = CELL_CM / z
    for L in (1, 2, 5, 10, 20, 50, 100, 200, 500, 1000, 2000, 5000, 10000):
        if L / cm_px >= 80: break
    wpx = int(L / cm_px)
    bx, by = 16, MAPH - 22
    for off, col in ((1, (0, 0, 0)), (0, (240, 240, 240))):
        pygame.draw.line(screen, col, (bx + off, by + off), (bx + wpx + off, by + off), 2)
        pygame.draw.line(screen, col, (bx + off, by - 4 + off), (bx + off, by + 4 + off), 2)
        pygame.draw.line(screen, col, (bx + wpx + off, by - 4 + off), (bx + wpx + off, by + 4 + off), 2)
        txt(screen, fs, ('%d cm' % L) if L < 100 else ('%g m' % (L / 100)), bx + off, by - 17 + off, col)
    px = MAPW
    pygame.draw.rect(screen, BG, (px, 0, PANEL, MAPH))
    pygame.draw.line(screen, (50, 56, 76), (px, 0), (px, MAPH))
    x, w = px + 10, PANEL - 20
    seas = ('spring', 'summer', 'autumn', 'winter')[int(4 * ((sim.t % SEASON) / SEASON))]
    txt(screen, f, 'EVOSIM', x, 6, (120, 220, 160))
    txt(screen, fs, 'day %d  %s  %s  %s%s' % (sim.t // DAY + 1, seas, 'night' if sim.dark > .5 else 'day',
                                                'HYPER 8t' if speed >= 10000 else 'x%d' % speed, '  PAUSED' if paused else ''), x + 80, 9, DIM)
    txt(screen, fs, 'world %dx%d cells = %.1f x %.1f m   (1 cell = %g cm)' % (W, H, W * CELL_CM / 100, H * CELL_CM / 100, CELL_CM), x, 55, (90, 98, 118))
    alive = sum(1 for p in sim.species if p.pop > 0)
    mg = max((c.gen for c in sim.crs), default=0)
    txt(screen, fs, 'pop %d   species %d alive / %d total   max gen %d' % (len(sim.crs), alive, len(sim.species), mg), x, 26)
    txt(screen, fs, 'plants %d   herb %s   omni %s   carn %s' % ((sim.nplants,) + tuple(
        str(v) for v in (sim.hist[-1][:3] if sim.hist else (0, 0, 0)))), x, 40)
    draw_graph(screen, (x, 70, w, 40), sim.hist)
    txt(screen, fs, 'PHYLOGENY  (click to expand)', x, 114, DIM)
    draw_tree(sim, screen, f, fs, (x, 128, w, 168))
    draw_inspector(sim, screen, f, fs, x, 302, w)
    ly = MAPH - 7 * 13 - 4
    pygame.draw.line(screen, (50, 56, 76), (x, ly - 4), (x + w, ly - 4))
    for i, (t, m, col) in enumerate(list(sim.log)[-7:]):
        txt(screen, fs, '%6d %s' % (t, m), x, ly + i * 13, col)


SAVE_DIR = Path(__file__).resolve().parent / 'saves'
AUTOSAVE = SAVE_DIR / 'autosave.evo'
QUICKSAVE = SAVE_DIR / 'quicksave.evo'
AUTOSAVE_SECONDS = 60


def save_world(path, sim, view, speed_index=1, paused=False):
    write_snapshot(path, dict(version=1, width=W, height=H, sim=sim,
                              rng=rnd.getstate(), view=dict(view, drag=None),
                              speed_index=speed_index, paused=paused))


def upgrade_evolution(sim, width, height):
    """Keep saves made before body-plan/plant evolution usable."""
    if not hasattr(sim, 'dt'): sim.dt = 1
    if not hasattr(sim, 'bones'): sim.bones = []
    if not hasattr(sim, 'environment_strain'): sim.environment_strain = None
    if not hasattr(sim, 'next_outbreak'): sim.next_outbreak = sim.t+DAY//2
    for k in sim.cor:
        for key,value in dict(buried=False,burial=0.,bone=None,has_bones=True).items():
            if not hasattr(k,key): setattr(k,key,value)
    if not hasattr(sim, 'primordial'): sim.primordial = False
    if not hasattr(sim, 'milestones'): sim.milestones = set()
    genomes = [sp.founder for sp in sim.species]
    for c in sim.crs + ([sim.sel] if sim.sel is not None else []):
        genomes.extend((c.g, *c.hap))
        if c.preg: genomes.extend(h for h, note in c.preg[1])
        if not hasattr(c, 'z'): c.z = 0.
        if not hasattr(c, 'breath'): c.breath = 1.
    for g in genomes:
        for key, value in DEFAULTS.items(): g.setdefault(key, value)
        g.setdefault('epoch', 0)
    if not hasattr(sim, 'vegetation'):
        sim.vegetation = Vegetation(width,height,rnd,sim.primordial)
        for i, growth in enumerate(sim.pl):
            sim.vegetation.ids[i] = 0 if sim.terr[i] == 1 else 1
    sim.vegetation.rng = rnd
    veg = sim.vegetation
    if not hasattr(veg,'counts'):
        veg.counts,veg.peaks,veg.died = [0]*len(veg.lineages),[0]*len(veg.lineages),[None]*len(veg.lineages)
        for i,st in enumerate(sim.pl):
            if st: veg.counts[veg.ids[i]] += 1
        veg.peaks = veg.counts.copy()


def load_world(path):
    payload = read_snapshot(path, {cls.__name__: cls for cls in (Sim, Cr, Co, Bo, Sp, Strain)})
    w, h, sim = payload['width'], payload['height'], payload['sim']
    if not (isinstance(w, int) and isinstance(h, int) and 120 <= w <= 4096 and 120 <= h <= 4096):
        raise ValueError('Invalid saved world dimensions')
    if not isinstance(sim, Sim) or len(sim.terr) != w*h or sim.bg.get_size() != (w,h):
        raise ValueError('Incomplete saved world')
    if sim.atmosphere.width != w or sim.atmosphere.height != h:
        raise ValueError('Invalid saved atmosphere')
    # Validate RNG/UI before changing the active world's global dimensions.
    probe = random.Random()
    probe.setstate(payload['rng'])
    si = int(payload.get('speed_index', 1))
    if si not in range(7): raise ValueError('Invalid saved speed')
    view = payload['view']
    if not all(isinstance(view[k], (int,float)) and math.isfinite(view[k]) for k in ('x','y','z')) or not .01 <= view['z'] <= ZMAX:
        raise ValueError('Invalid saved camera')
    upgrade_evolution(sim,w,h)
    set_world(w, h)
    rnd.setstate(payload['rng'])
    sim.atmosphere.rng = rnd
    view['drag'] = None
    clamp_view(view)
    return sim, view, si, bool(payload.get('paused', False))


def welcome_screen(screen, fonts, world_name='giant'):
    f,fs = fonts
    SW,SH = screen.get_size()
    names = list(WORLDS)
    index = names.index(world_name) if world_name in names else 2
    clock = pygame.time.Clock()
    selected = 0
    buttons = [('primordial','NEW PRIMORDIAL WORLD'),('resume','CONTINUE AUTOSAVE'),('load','LOAD QUICKSAVE'),('animals','ANIMAL SANDBOX')]
    while True:
        enabled = [True,AUTOSAVE.exists(),QUICKSAVE.exists(),True]
        rects = [pygame.Rect(SW//2-210,242+j*57,420,44) for j in range(4)]
        for event in pygame.event.get():
            if event.type == pygame.QUIT: return None
            if event.type == pygame.KEYDOWN:
                if event.key == pygame.K_ESCAPE: return None
                if event.key in (pygame.K_LEFT,pygame.K_RIGHT): index = (index+(1 if event.key == pygame.K_RIGHT else -1))%len(names)
                if event.key in (pygame.K_UP,pygame.K_DOWN): selected = (selected+(1 if event.key == pygame.K_DOWN else -1))%4
                if event.key in (pygame.K_RETURN,pygame.K_KP_ENTER) and enabled[selected]: return buttons[selected][0],names[index]
                if event.key == pygame.K_p: return 'primordial',names[index]
                if event.key == pygame.K_c and enabled[1]: return 'resume',names[index]
                if event.key == pygame.K_l and enabled[2]: return 'load',names[index]
            if event.type == pygame.MOUSEBUTTONDOWN and event.button == 1:
                for j,rect in enumerate(rects):
                    if rect.collidepoint(event.pos) and enabled[j]: return buttons[j][0],names[index]
        screen.fill(BG)
        # Small palette, hard edges and nearest-neighbor type match the world.
        title = f.render('EVOSIM',False,(120,220,160))
        title = pygame.transform.scale(title,(title.get_width()*4,title.get_height()*4))
        screen.blit(title,(SW//2-title.get_width()//2,72))
        line = 'From microbial colonies to branching ecosystems'
        txt(screen,fs,line,SW//2-fs.size(line)[0]//2,164,DIM)
        line = '<  %s  %dx%d cells  >    LEFT / RIGHT: world size' % ((names[index],)+WORLDS[names[index]])
        txt(screen,fs,line,SW//2-fs.size(line)[0]//2,205,DIM)
        mouse = pygame.mouse.get_pos()
        for j,(action,label) in enumerate(buttons):
            rect = rects[j]
            active = enabled[j] and (rect.collidepoint(mouse) or selected == j)
            screen.fill((39,68,49) if active else (24,29,38),rect)
            pygame.draw.rect(screen,(98,168,119) if active else (48,55,67),rect,1)
            txt(screen,f,label,rect.x+18,rect.y+13,(153,229,170) if enabled[j] else (62,68,77))
        for j,line in enumerate(('Primordial: all animals descend from the starting colonies.',
                                 'Traits spread through inheritance, survival and reproduction.',
                                 'F5 save / F9 load / autosave every 60 seconds and on exit',
                                 'ENTER choose / arrows select / ESC quit')):
            txt(screen,fs,line,SW//2-fs.size(line)[0]//2,500+j*23,DIM)
        pygame.display.flip()
        clock.tick(30)


def main():
    args = sys.argv[1:]
    bench = '--bench' in args
    if bench: os.environ['SDL_VIDEODRIVER'] = 'dummy'
    wname = 'giant'
    if '--world' in args:
        a = args[args.index('--world') + 1].lower()
        if a in WORLDS: wname = a
        elif 'x' in a:
            wname = 'custom'
            set_world(*[max(120, int(v)) for v in a.split('x')[:2]])
    if wname != 'custom': set_world(*WORLDS[wname])
    pygame.init()
    screen = pygame.display.set_mode((MAPW + PANEL, MAPH))
    pygame.display.set_caption('EVOSIM')
    mono = 'dejavusansmono,menlo,consolas,couriernew,monospace'
    fonts = (pygame.font.SysFont(mono, 15, bold=True), pygame.font.SysFont(mono, 11))
    menu_choice = None
    if not any(flag in args for flag in ('--bench','--new','--primordial','--animals','--world','--load')):
        choice = welcome_screen(screen,fonts,wname)
        if choice is None:
            pygame.quit()
            return
        menu_choice,wname = choice
        set_world(*WORLDS[wname])
    load_path = AUTOSAVE if menu_choice == 'resume' else QUICKSAVE if menu_choice == 'load' else None
    if '--load' in args: load_path = Path(args[args.index('--load')+1])

    load_error = None
    if load_path is not None:
        try:
            sim, view, si, paused = load_world(load_path)
        except Exception as exc:
            if '--load' in args: raise SystemExit('Cannot load save: %s' % exc)
            load_error = str(exc)
            load_path = None
    if load_path is None:
        sim = Sim(W,H,primordial=menu_choice != 'animals' and '--animals' not in args)
        view = new_view()
        si, paused = 1, False
        if load_error: sim.logmsg('Autosave could not load: '+load_error, (255,150,100))
    clamp_view(view)
    world, night = pygame.Surface((W,H)), pygame.Surface((W,H), pygame.SRCALPHA)
    tv = new_tv()
    speeds = (1, 2, 4, 8, 20, 80, 10000)
    if bench:
        bi = args.index('--bench')
        n = int(args[bi + 1])
        t0 = time.time()
        for i in range(n):
            sim.step()
            if i % 1000 == 999:
                print(sim.t, sim.hist[-1][:3], 'pop', len(sim.crs), 'plants', sim.nplants, 'species', len(sim.species),
                      'alive', sum(1 for p in sim.species if p.pop > 0), 'dt %.1fs' % (time.time() - t0))
        sim.sel = sim.crs[0] if sim.crs else None
        render(sim, screen, world, night, fonts, 1, False, view, tv)
        pygame.image.save(screen, args[bi + 2] if len(args) > bi + 2 and not args[bi + 2].startswith('--') else 'evosim.png')
        return
    clock = pygame.time.Clock()
    worker = SimulationWorker()
    last_save = time.monotonic()
    last_render = 0.0
    rate_time, rate_ticks, actual_tps = time.monotonic(), sim.t, 0.0
    try:
        while True:
            with worker.lock:
                for e in pygame.event.get():
                    if e.type == pygame.QUIT: return
                    if e.type == pygame.KEYDOWN and e.key in (pygame.K_F5, pygame.K_F9):
                        worker.pause()
                        try:
                            if e.key == pygame.K_F5:
                                save_world(QUICKSAVE, sim, view, si, paused)
                                sim.logmsg('Saved: quicksave.evo', (120,220,160))
                            else:
                                path = QUICKSAVE if QUICKSAVE.exists() else AUTOSAVE
                                sim, view, si, paused = load_world(path)
                                world, night = pygame.Surface((W,H)), pygame.Surface((W,H), pygame.SRCALPHA)
                                tv, wname = new_tv(), 'custom'
                                rate_time, rate_ticks = time.monotonic(), sim.t
                                sim.logmsg('Loaded: '+path.name, (120,220,160))
                        except Exception as exc:
                            sim.logmsg('Save/load failed: '+str(exc), (255,150,100))
                        last_render = 0
                        continue
                    if tv['fs']:
                        if e.type == pygame.KEYDOWN:
                            if e.key in (pygame.K_ESCAPE, pygame.K_t): tv['fs'] = False
                            elif e.key == pygame.K_SPACE: paused = not paused
                            elif pygame.K_1 <= e.key <= pygame.K_7: si = e.key - pygame.K_1
                            else: tree_key(e, sim, tv, view)
                        else:
                            tree_mouse(e, sim, tv)
                        continue
                    if e.type == pygame.MOUSEBUTTONDOWN and e.button == 1 and MAPW + 10 <= e.pos[0] <= MAPW + PANEL - 10 and 128 <= e.pos[1] < 296:
                        tv.update(fs=True, drag=None)
                        continue
                    if e.type == pygame.KEYDOWN:
                        if e.key == pygame.K_ESCAPE: return
                        if e.key == pygame.K_t: tv['fs'] = True
                        if e.key == pygame.K_SPACE: paused = not paused
                        if e.key == pygame.K_r:
                            worker.pause()
                            sim = Sim(W, H, primordial=sim.primordial); tv = new_tv(); view = new_view(); clamp_view(view)
                        if e.key == pygame.K_n:
                            worker.pause()
                            names = list(WORLDS)
                            wname = names[(names.index(wname) + 1) % len(names)] if wname in names else 'small'
                            sim = Sim(*WORLDS[wname], primordial=sim.primordial); tv = new_tv()
                            world, night = pygame.Surface((W, H)), pygame.Surface((W, H), pygame.SRCALPHA)
                            view = new_view(); clamp_view(view)
                        if pygame.K_1 <= e.key <= pygame.K_7: si = e.key - pygame.K_1
                        if e.key == pygame.K_s: pygame.image.save(screen, 'evosim.png')
                        if e.key in (pygame.K_EQUALS, pygame.K_PLUS, pygame.K_KP_PLUS): zoom_at(view, MAPW / 2, MAPH / 2, 1.25)
                        if e.key in (pygame.K_MINUS, pygame.K_KP_MINUS): zoom_at(view, MAPW / 2, MAPH / 2, 1 / 1.25)
                        if e.key == pygame.K_0: view.update(new_view()); clamp_view(view)
                        if e.key in (pygame.K_h, pygame.K_c) and not sim.primordial:
                            mx, my = pygame.mouse.get_pos()
                            if mx < MAPW:
                                wx, wy = to_world(view, mx, my)
                                b = base_genome('herb' if e.key == pygame.K_h else 'carn')
                                sp = sim.newsp(None, b, 'founder')
                                for _ in range(6):
                                    cx, cy = wx + rnd.uniform(-4, 4), wy + rnd.uniform(-4, 4)
                                    if sim.ok(cx, cy, False): sim.spawn(mutate(b, .6)[0], mutate(b, .6)[0], sp, None, cx, cy)
                                sim.logmsg('Founders placed: %s' % sp.name, sp.col)
                    if e.type == pygame.MOUSEWHEEL:
                        mx, my = pygame.mouse.get_pos()
                        if mx < MAPW: zoom_at(view, mx, my, 1.2 ** e.y)
                    if e.type == pygame.MOUSEBUTTONDOWN and e.pos[0] < MAPW:
                        if e.button == 1:
                            view['drag'] = [e.pos, False]
                        elif e.button == 3:
                            wx, wy = to_world(view, *e.pos)
                            for _ in range(25):
                                i = int(wy + rnd.randint(-6, 6)) * W + int(wx + rnd.randint(-6, 6))
                                if 0 <= i < N and sim.terr[i] == 0: sim.setp(i, 3)
                    if e.type == pygame.MOUSEMOTION and view['drag'] and e.buttons[0]:
                        d = view['drag']
                        if d[1] or abs(e.pos[0] - d[0][0]) + abs(e.pos[1] - d[0][1]) > 5:
                            d[1] = True
                            view['x'] -= e.rel[0] / view['z']; view['y'] -= e.rel[1] / view['z']
                            clamp_view(view)
                    if e.type == pygame.MOUSEBUTTONUP and e.button == 1 and view['drag']:
                        if not view['drag'][1]:
                            wx, wy = to_world(view, *e.pos)
                            best, bd = None, max(3.0, 14 / view['z']) ** 2
                            for c in sim.crs:
                                d = (c.x - wx) ** 2 + (c.y - wy) ** 2
                                if d < bd: best, bd = c, d
                            sim.sel = best
                        view['drag'] = None
                ks = pygame.key.get_pressed()
                if ks[pygame.K_LEFT] or ks[pygame.K_RIGHT] or ks[pygame.K_UP] or ks[pygame.K_DOWN]:
                    view['x'] += (ks[pygame.K_RIGHT] - ks[pygame.K_LEFT]) * 9 / view['z']
                    view['y'] += (ks[pygame.K_DOWN] - ks[pygame.K_UP]) * 9 / view['z']
                    clamp_view(view)
                if not paused: worker.submit(sim, speeds[si])
                else:
                    worker.pause()
                    if worker.error: raise RuntimeError('Simulation worker failed') from worker.error
                now = time.monotonic()
                if now-last_save >= AUTOSAVE_SECONDS:
                    try:
                        save_world(AUTOSAVE, sim, view, si, paused)
                        sim.logmsg('Autosaved', (120,220,160))
                    except Exception as exc:
                        sim.logmsg('Autosave failed: '+str(exc), (255,150,100))
                    last_save = now
                if now-rate_time >= 1:
                    actual_tps = max(0, sim.t-rate_ticks)/(now-rate_time)
                    rate_time, rate_ticks = now, sim.t
                if si != 6 or paused or now-last_render >= .1:
                    render(sim, screen, world, night, fonts, speeds[si], paused, view, tv)
                    txt(screen, fonts[1], '%.0f ticks/s | F5 save | F9 load' % actual_tps, MAPW+10, MAPH-16, DIM)
                    pygame.display.flip()
                    last_render = now
            clock.tick(60)
    finally:
        worker.close()
        try:
            save_world(AUTOSAVE, sim, view, si, paused)
        except Exception as exc:
            print('[evosim] Exit autosave failed: %s' % exc, file=sys.stderr)
        pygame.quit()


if __name__ == '__main__':
    main()
