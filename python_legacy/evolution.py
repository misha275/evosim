"""Heritable body plans; abilities require compatible anatomy, not a global unlock."""
TRAITS = {'complexity': (0, 1), 'wings': (0, 1), 'gills': (0, 1), 'fins': (0, 1), 'lungs': (0, 1), 'burial': (0, 1)}
DEFAULTS = dict(complexity=.8, wings=0., gills=0., fins=0., lungs=1., burial=0.)
INNOVATION_GENERATION = dict(wings=12, fins=3, gills=4, lungs=6, burial=8)


def stage(g):
    c = g.get('complexity', .8)
    return 'microbial colony' if c < .22 else 'colonial life' if c < .42 else 'simple animal' if c < .65 else 'complex animal'


def can_swim(g):
    return 'Swimmer' in g['q'] or g.get('gills', 0) > .45 or g.get('fins', 0) > .35


def aquatic(g):
    return g.get('gills', 0) > .55 and g.get('lungs', 1) < .4


def can_fly(g):
    # Heavy armor and mass trade off against wing area and respiration.
    lift = g.get('wings', 0) - .025*g['size'] - .16*g.get('armor', 0)
    return g.get('complexity', .8) > .6 and g.get('lungs', 1) > .5 and lift > .48


def habitat(g):
    if can_fly(g): return 'winged / aerial'
    if aquatic(g): return 'aquatic / gill breathing'
    if can_swim(g): return 'amphibious'
    return 'terrestrial'
