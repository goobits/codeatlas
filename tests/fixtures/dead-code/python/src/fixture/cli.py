from . import alias_target as target
from .api import public_api


def main():
    public_api()
    target.cli_alias_value()


def pep_plugin():
    return "pep"


def poetry_script():
    return "poetry-script"


def poetry_plugin():
    return "poetry-plugin"
