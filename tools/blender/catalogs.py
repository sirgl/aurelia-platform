"""What every model part declares, without building a mesh: unit and world models, content keys, the
city kit's landmarks. And the rule that decides in which part a model is declared. No bpy.

Each part has <part>/tools/blender/part.py (see parts.py) that answers these questions; a function it
does not define means "none":

    OWNER, NAMESPACE        the part's name and its key namespace ("core", "lal"; None for platform)
    unit_class_models()     {slug: units/catalog.Model} of the part's own unit class models
    unit_unique_models(classes)   {slug: Model} variants of class models (`classes`: every class model
                            the part can see, i.e. the platform's and its own)
    unit_fallback()         the model that draws content without a model (platform only)
    unit_keys(classes, uniques)  (class map, unit map, fallbacks): the part's content keys -> model ids
    world_models()          [world_catalog.WorldModel] of the part's own world models
    world_keys()            {key: world model id}, "@" qualifies a variant
    world_icon_only(), world_no_map_presence()   {key: reason}
    world_landmarks()       {building key: city kit landmark entry}
    world_problems(models, keys, landmarks)  the checks that need the part's own documents (coverage)

Ownership rule (the one integration/tools/models-split.py applied once): a model belongs to the part whose
content keys draw it; keys of both rules modules, or no key at all (the unit fallback, the city kit's
centers and houses), put it in the platform. A unit variant is never in a part "below" its base.
`ownership_problems` fails when a model is declared in another part than the rule says.
"""
import parts

parts.use_library()

PLATFORM = parts.PLATFORM


class PartCatalog:
    def __init__(self, name):
        self.name = name
        self.mod = parts.load(name, "part")
        if getattr(self.mod, "OWNER", None) != name:
            raise SystemExit(f"{name}/tools/blender/part.py: OWNER must be {name!r}")
        self.namespace = getattr(self.mod, "NAMESPACE", None)
        self.classes = self._call("unit_class_models", {})
        self.fallback = self._call("unit_fallback", None)
        self.uniques = {}
        self.world = sorted(self._call("world_models", []), key=lambda m: m.id)
        self.keys = dict(self._call("world_keys", {}))
        self.icon_only = dict(self._call("world_icon_only", {}))
        self.no_map_presence = dict(self._call("world_no_map_presence", {}))
        self.landmarks = dict(self._call("world_landmarks", {}))

    def _call(self, name, default, *args):
        fn = getattr(self.mod, name, None)
        return default if fn is None else fn(*args)

    def unit_models(self):
        """The part's own unit models in manifest order: class models, variants (both by id), fallback."""
        models = sorted(self.classes.values(), key=lambda m: m.id)
        models += sorted(self.uniques.values(), key=lambda m: m.id)
        if self.fallback is not None:
            models.append(self.fallback)
        return models


class Catalogs:
    """The catalogs of the parts in `names` (default: every part present). Loading the whole set is
    cheap (no mesh), and the ownership rule needs every part's keys even when one part is exported."""

    def __init__(self, names=None):
        self.names = list(names or parts.present())
        if PLATFORM not in self.names:
            self.names.insert(0, PLATFORM)
        self.part = {n: PartCatalog(n) for n in self.names}
        for name, cat in self.part.items():
            visible = self.visible_classes(name)
            cat.uniques = cat._call("unit_unique_models", {}, visible)

    def modules(self):
        return [n for n in self.names if n != PLATFORM]

    def visible(self, name):
        """Parts whose models `name` may use: the platform and itself."""
        return [PLATFORM] if name == PLATFORM else [PLATFORM, name]

    def visible_classes(self, name):
        out, where = {}, {}
        for p in self.visible(name):
            for slug, model in self.part[p].classes.items():
                if slug in out:
                    raise SystemExit(f"unit class slug {slug!r} is declared in {where[slug]} and {p}")
                out[slug], where[slug] = model, p
        return out

    def visible_uniques(self, name):
        out, where = {}, {}
        for p in self.visible(name):
            for slug, model in self.part[p].uniques.items():
                if slug in out:
                    raise SystemExit(f"unique unit slug {slug!r} is declared in {where[slug]} and {p}")
                out[slug], where[slug] = model, p
        return out

    def unit_maps(self, name):
        """(class map, unit map, fallbacks) of the part's content keys."""
        cat = self.part[name]
        fn = getattr(cat.mod, "unit_keys", None)
        if fn is None:
            return {}, {}, {}
        return fn(self.visible_classes(name), self.visible_uniques(name))

    def visible_unit_models(self, name):
        out = []
        for p in self.visible(name):
            out += self.part[p].unit_models()
        return out

    def visible_world_models(self, name):
        out = []
        for p in self.visible(name):
            out += self.part[p].world
        return sorted(out, key=lambda m: m.id)

    def visible_landmarks(self, name):
        out = {}
        for p in self.visible(name):
            out.update(self.part[p].landmarks)
        return out

    def namespace_part(self):
        return {c.namespace: n for n, c in self.part.items() if c.namespace}


def namespace_of(key):
    return key.split("@")[0].split(":", 1)[0]


def owner_of(users, owner_of_namespace):
    if not users:
        return PLATFORM
    owners = {owner_of_namespace[u] for u in users}
    return owners.pop() if len(owners) == 1 else PLATFORM


def ownership_problems(cats):
    """Problems: ids declared twice, keys that draw a missing or invisible model, models declared in
    another part than the ownership rule says."""
    problems = []
    by_ns = cats.namespace_part()
    declared = {"unit": {}, "world": {}}   # model id -> the part that declares it
    for name in cats.names:
        cat = cats.part[name]
        for kind, models in (("unit", cat.unit_models()), ("world", cat.world)):
            for model in models:
                if model.id in declared[kind]:
                    problems.append(f"{kind} model {model.id} is declared in {declared[kind][model.id]} and {name}")
                declared[kind][model.id] = name
    # The users of every model: the key namespaces that draw it.
    users = {"unit": {}, "world": {}}

    def use(kind, part, key, model_id):
        users[kind].setdefault(model_id, set()).add(namespace_of(key))
        where = declared[kind].get(model_id)
        if where not in (PLATFORM, part):
            problems.append(f"{key} -> {model_id}: the model is "
                            f"{'declared in ' + where if where else 'not declared'}, not in {PLATFORM} or {part}")

    for name in cats.modules():
        cat = cats.part[name]
        for table in cats.unit_maps(name):
            for key, model_id in table.items():
                use("unit", name, key, model_id)
        for key, model_id in cat.keys.items():
            use("world", name, key, model_id)
        for building, entry in cat.landmarks.items():
            use("world", name, building, entry["model"])
    # The part the rule puts each model in.
    platform = cats.part[PLATFORM]
    owner = {"unit": {mid: owner_of(users["unit"].get(mid, ()), by_ns) for mid in declared["unit"]}}
    if platform.fallback is not None:
        owner["unit"][platform.fallback.id] = PLATFORM
    changed = True
    while changed:       # a variant is never in a part below its base
        changed = False
        for name in cats.names:
            for model in cats.part[name].unit_models():
                base = model.variant_of
                if base and owner["unit"].get(model.id) == PLATFORM and owner["unit"].get(base, PLATFORM) != PLATFORM:
                    owner["unit"][base] = PLATFORM
                    changed = True
    kit = platform_kit()
    generic = set(kit["centers"].values())
    for houses in kit["houses"].values():
        generic.update(houses)
    owner["world"] = {mid: PLATFORM if mid in generic else owner_of(users["world"].get(mid, ()), by_ns)
                      for mid in declared["world"]}
    for kind in ("unit", "world"):
        for mid, where in declared[kind].items():
            if owner[kind][mid] != where:
                drawn_by = ", ".join(sorted(users[kind].get(mid, ()))) or "none"
                problems.append(f"{kind} model {mid} is declared in {where} but the ownership rule puts it in "
                                f"{owner[kind][mid]} (drawn by keys of: {drawn_by})")
    for name in cats.names:
        for model in cats.part[name].unit_models():
            base = model.variant_of
            if base and declared["unit"].get(base) not in (PLATFORM, name):
                problems.append(f"unit variant {model.id} in {name} is based on {base}, "
                                f"declared in {declared['unit'].get(base)}")
    return problems


def require_ownership(cats):
    problems = ownership_problems(cats)
    if problems:
        print("\n".join(problems))
        raise SystemExit("model ownership violated: a model is declared in another part than its owner")


def platform_kit():
    import city_kit
    return city_kit.KIT
