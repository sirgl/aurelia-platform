"""Animation rig and weapon of every unit model, derived from the catalog. No bpy.

The renderer animates the baked, bone-less meshes in the vertex shader (docs/animation.md):
which deformation applies is the model's `rig`, what it does when it attacks is its
`weapon`, and `baked_yaw` is the angle of the model's forward from the camera axis (+z) toward +x, so the animation can be aligned with the model's own axes.
Both are written to the manifest next to the other per-model facts.

    python3 platform/tools/blender/units/rigs.py [--part NAME]
        # rewrite the rig/weapon/yaw fields of <part>/assets/models/manifest.json (default: every part)

export_units.py applies the same rules while it writes the manifest, so this is only needed to refresh
the fields of a manifest without a Blender run.
"""
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

RIGS = ("biped", "horse", "elephant", "camel", "chariot", "cart", "engine", "vehicle", "vessel", "plane",
        "missile", "static")
WEAPONS = ("none", "blade", "club", "polearm", "bow", "crossbow", "thrown", "firearm", "autogun", "rocket",
           "cannon", "catapult", "ballista", "ram", "broadside", "torpedo", "bomb", "missile")

GEAR = {
    "sword": "blade", "longsword": "blade", "axe": "blade", "katana": "blade", "kris": "blade", "sabre": "blade",
    "pistol": "blade", "club": "club", "mace": "club", "staff": "club", "hammer": "club", "pick": "club",
    "spear": "polearm", "lance": "polearm", "pike": "polearm", "halberd": "polearm",
    "bow": "bow", "longbow": "bow", "crossbow": "crossbow", "repeater": "crossbow",
    "sling": "thrown", "atlatl": "thrown", "musket": "firearm", "rifle": "firearm",
    "smg": "autogun", "assault": "autogun", "mg": "autogun", "bazooka": "rocket",
}

MACHINE_RIG = {
    "catapult": ("engine", "catapult"), "trebuchet": ("engine", "catapult"), "ballista": ("engine", "ballista"),
    "hwacha": ("engine", "rocket"), "ram": ("engine", "ram"), "siege_tower": ("engine", "none"),
    "cannon": ("engine", "cannon"), "field_gun": ("engine", "cannon"), "aa_gun": ("engine", "autogun"),
    "gatling": ("engine", "autogun"), "mg_nest": ("engine", "autogun"), "rocket_truck": ("vehicle", "rocket"),
    "tank": ("vehicle", "cannon"), "landship": ("vehicle", "cannon"), "mech_infantry": ("vehicle", "autogun"),
    "mobile_sam": ("vehicle", "rocket"), "robot": ("biped", "autogun"),
    "helicopter": ("plane", "autogun"), "triplane": ("plane", "autogun"), "prop_fighter": ("plane", "autogun"),
    "jet": ("plane", "autogun"), "biplane_bomber": ("plane", "bomb"), "heavy_bomber": ("plane", "bomb"),
    "stealth": ("plane", "bomb"), "missile": ("missile", "missile"), "bomb": ("missile", "bomb"),
    "ss_booster": ("static", "none"), "ss_cockpit": ("static", "none"), "ss_engine": ("static", "none"),
    "ss_stasis": ("static", "none"),
}

SHIP_WEAPON = {
    "work_boat": "none", "cargo_ship": "none", "flagship": "broadside", "submarine": "torpedo",
    "missile_cruiser": "missile", "carrier": "broadside", "trireme": "broadside",
}


def gear_weapon(look):
    """The deadliest thing a person carries (ranged beats melee)."""
    found = [GEAR.get(g, "none") for g in (look.right, look.left) if g]
    for rank in ("rocket", "autogun", "firearm", "crossbow", "bow", "thrown", "polearm", "blade", "club"):
        if rank in found:
            return rank
    return "none"


def of(model):
    """(rig, weapon, yaw) of a catalog Model."""
    art = getattr(model.build, "art", None)
    if art is None:
        return "static", "none", 0
    yaw = art.get("yaw", 0)
    # `baked_yaw` is the angle of the model's forward from +z (the camera) toward +x, in degrees.
    # A person faces -y and is turned by `yaw`; everything built "long" points along +x, a quarter
    # turn away, and is turned by `yaw` as well.
    if art["kind"] == "long":
        yaw = 90 + yaw
    if art["kind"] == "person":
        return "biped", gear_weapon(art["look"]), yaw
    fn = art["fn"]
    name = fn.__name__
    args = art["args"]
    if name == "ride":
        mount, rider = args[0], args[1]
        rig = {"horse": "horse", "barded": "horse", "elephant": "elephant", "camel": "camel"}[mount]
        return rig, gear_weapon(rider), yaw
    if name == "chariot":
        return "chariot", gear_weapon(args[1]), yaw
    if name in MACHINE_RIG:
        rig, weapon = MACHINE_RIG[name]
        return rig, weapon, yaw
    if model.budget == "ship" or fn.__module__ == "ships":
        weapon = SHIP_WEAPON.get(name, "broadside")
        if model.category == "civilian":
            weapon = "none"
        return "vessel", weapon, yaw
    if name in ("caravan", "wagon"):
        return "cart", "none", 90
    if name == "great_general":
        return "horse", "none", 55
    # Civilians built on the person helper turn 20 degrees like soldiers.
    return "biped", "none", 20 if art["kind"] == "plain" and fn.__module__ == "civilians" else yaw


def apply(entry, model):
    rig, weapon, yaw = of(model)
    entry["rig"] = rig
    entry["weapon"] = weapon
    entry["baked_yaw"] = yaw
    return entry


def main(argv):
    import catalogs
    import parts
    cats = catalogs.Catalogs()
    for part in parts.selected(argv):
        path = parts.manifest_path(part)
        manifest = json.loads(path.read_text())
        by_id = {m.id: m for m in cats.part[part].unit_models()}
        counts = {}
        for entry in manifest["models"]:
            apply(entry, by_id[entry["id"]])
            counts[(entry["rig"], entry["weapon"])] = counts.get((entry["rig"], entry["weapon"]), 0) + 1
        path.write_text(json.dumps(manifest, indent=1) + "\n")
        print(part)
        for (rig, weapon), n in sorted(counts.items()):
            print(f"  {rig:<9} {weapon:<9} {n}")


if __name__ == "__main__":
    main(sys.argv[1:])
