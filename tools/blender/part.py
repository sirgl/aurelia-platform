"""Model part "platform": the shared and generic models (see parts.py, catalogs.py for the interface).

The unit catalog is units/catalog.py, the world models are the modules of world/ listed by
world_catalog.all_models; the platform draws no content keys of its own (the manifest has none).
"""
import catalog
import world_catalog

OWNER = "platform"
NAMESPACE = None


def unit_class_models():
    return catalog.class_models()


def unit_fallback():
    return catalog.unknown_model()


def world_models():
    return world_catalog.all_models()
