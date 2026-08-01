import importlib


def reflected(function):
    return function


def _registered_helper():
    return "registered"


@reflected
def load_plugin(name):
    _registered_helper()
    return importlib.import_module(name)


load_plugin("plugin")
