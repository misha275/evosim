"""Versioned, atomic EVOSIM snapshots, including object references and SDL pixels."""
import gzip
import os
import pickle
import tempfile
from pathlib import Path
import pygame

MAGIC = b'EVOSIM-SAVE\x01\n'


def restore_surface(pixels, size, alpha):
    if len(size) != 2 or not all(isinstance(v, int) and 0 < v <= 10000 for v in size):
        raise ValueError('Invalid saved surface dimensions')
    mode = 'RGBA' if alpha else 'RGB'
    if len(pixels) != size[0]*size[1]*len(mode):
        raise ValueError('Invalid saved surface pixels')
    return pygame.image.frombytes(pixels, size, mode)


class SnapshotPickler(pickle.Pickler):
    def reducer_override(self, obj):
        if isinstance(obj, pygame.Surface):
            alpha = bool(obj.get_flags() & pygame.SRCALPHA)
            return restore_surface, (pygame.image.tobytes(obj, 'RGBA' if alpha else 'RGB'), obj.get_size(), alpha)
        return NotImplemented


class SnapshotReader(pickle.Unpickler):
    def __init__(self, stream, classes):
        super().__init__(stream)
        self.classes = classes

    def find_class(self, module, name):
        if module in ('evosim', '__main__') and name in self.classes:
            return self.classes[name]
        if module == 'savegame' and name == 'restore_surface':
            return restore_surface
        if module == 'weather' and name in ('Atmosphere', 'Cloud', 'Drop'):
            import weather
            return getattr(weather, name)
        if module == 'vegetation' and name in ('Vegetation', 'PlantLineage'):
            import vegetation
            return getattr(vegetation, name)
        if module == 'array' and name in ('array', '_array_reconstructor'):
            import array
            return getattr(array, name)
        if module == 'random' and name == 'Random':
            from random import Random
            return Random
        if module == 'collections' and name == 'deque':
            from collections import deque
            return deque
        if module == 'builtins' and name in ('set', 'frozenset', 'bytearray'):
            import builtins
            return getattr(builtins, name)
        raise ValueError('Unsupported save object: %s.%s' % (module, name))


def write_snapshot(path, payload):
    """Commit a complete compressed snapshot; a failed write preserves the old save."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=path.name+'.', suffix='.tmp', dir=path.parent)
    try:
        with os.fdopen(fd, 'wb') as stream:
            stream.write(MAGIC)
            with gzip.GzipFile(fileobj=stream, mode='wb', compresslevel=1, mtime=0) as compressed:
                SnapshotPickler(compressed, protocol=5).dump(payload)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary): os.unlink(temporary)


def read_snapshot(path, classes):
    with open(path, 'rb') as stream:
        if stream.read(len(MAGIC)) != MAGIC:
            raise ValueError('Unsupported or damaged EVOSIM save')
        with gzip.GzipFile(fileobj=stream, mode='rb') as compressed:
            payload = SnapshotReader(compressed, classes).load()
    if not isinstance(payload, dict) or payload.get('version') != 1:
        raise ValueError('Unsupported EVOSIM save version')
    return payload
