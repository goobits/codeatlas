import importlib


def reflected(function):
    return function


@reflected
def load_plugin(name):
    return importlib.import_module(name)


load_plugin("plugin")
