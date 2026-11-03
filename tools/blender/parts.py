"""Model parts: which parts of the repository have a Blender model catalog, and how their modules load.
No bpy; runs under python3 and inside Blender.

A model part is a folder beside platform/ that has a PART file and a tools/blender/part.py:

    platform/   the shared and generic models (used by both rules modules, the unit fallback, the city
                kit's centers and houses, the terrain props), the palette, the budgets and shapes
    classic/    the models only the classic module draws, and its content keys ("core:...")
    lands/      the content keys of Lands & Laws ("lal:...") onto the platform's models; no models of its own

Every part keeps its models in <part>/assets/models/ (GLB files + manifest.json) and its catalogs in
<part>/tools/blender/. The geometry library (geom.py, people.py, machines.py, ships.py, kit.py, ...) is
the platform's and is imported by its flat module names (the folders units/, world/, terrain/ and this
one are on sys.path, see use_library). The modules of a part are NOT put on sys.path, because several
parts have a module of the same name (improvements.py, catalog.py, ...): they are loaded by file path
under the name "<part>_<folder>_<file>" (see load), and a part module imports the platform module of the
same name by its flat name (`from improvements import _model`).

    --part NAME   (repeatable) the exporters and the contact sheets work on these parts only; default:
                  every part present, platform first
"""
import importlib.util
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
PLATFORM = "platform"
LIBRARY = (HERE, HERE / "units", HERE / "world", HERE / "terrain")


def use_library():
    """Puts the platform's geometry library on sys.path (idempotent)."""
    for path in reversed(LIBRARY):
        if str(path) not in sys.path:
            sys.path.insert(0, str(path))


def blender_dir(part):
    return REPO / part / "tools" / "blender"


def models_dir(part):
    return REPO / part / "assets" / "models"


def manifest_path(part):
    return models_dir(part) / "manifest.json"


def docs_dir(part):
    return REPO / part / "docs"


def present():
    """Names of the model parts in this checkout, platform first, the others by name."""
    names = [PLATFORM]
    for folder in sorted(REPO.iterdir()):
        if folder.name != PLATFORM and (folder / "PART").is_file() and (blender_dir(folder.name) / "part.py").is_file():
            names.append(folder.name)
    return names


def load(part, relpath):
    """The module <part>/tools/blender/<relpath>.py (relpath without .py, e.g. "world/city_kit"),
    loaded once under the name "<part>_<relpath with / as _>"."""
    name = f"{part}_{relpath.replace('/', '_')}"
    if name in sys.modules:
        return sys.modules[name]
    use_library()
    path = blender_dir(part) / f"{relpath}.py"
    if not path.is_file():
        raise SystemExit(f"{part}: no {path.relative_to(REPO)}")
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def cli_args(argv=None):
    """Own arguments under python3, or after `--` under `blender -b -P script.py -- ...`."""
    argv = sys.argv if argv is None else argv
    if "--" in argv:
        return argv[argv.index("--") + 1:]
    if Path(argv[0]).name.lower().startswith("blender"):
        return []
    return argv[1:]


def selected(args):
    """Parts named by --part NAME / --part=NAME in `args` (default: every part present)."""
    names, i = [], 0
    while i < len(args):
        if args[i] == "--part" and i + 1 < len(args):
            names.append(args[i + 1])
            i += 1
        elif args[i].startswith("--part="):
            names.append(args[i].split("=", 1)[1])
        i += 1
    available = present()
    for name in names:
        if name not in available:
            raise SystemExit(f"--part {name}: not a model part here (have: {', '.join(available)})")
    return [n for n in available if n in names] if names else available


def positional(args):
    """Arguments that are neither options nor the value of --part."""
    out, skip = [], False
    for a in args:
        if skip:
            skip = False
        elif a == "--part":
            skip = True
        elif not a.startswith("--"):
            out.append(a)
    return out
