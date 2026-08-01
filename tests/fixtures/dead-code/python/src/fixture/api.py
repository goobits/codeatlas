import importlib

_PRIVATE_TOKEN = "private"


class _LocalClient:
    def value(self):
        return _PRIVATE_TOKEN


def _used_private():
    return "used"


def _unused_private():
    return "unused"


def _scoped_values():
    from . import alias_target as target
    from .nested.used import nested_value

    return f"{target.alias_value()}-{nested_value()}"


def public_api():
    importlib.import_module("fixture.lazy")
    return f"{_used_private()}-{_LocalClient().value()}-{_scoped_values()}"
