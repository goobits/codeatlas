import importlib


def _used_private():
    return "used"


def _unused_private():
    return "unused"


def public_api():
    importlib.import_module("fixture.lazy")
    return _used_private()
