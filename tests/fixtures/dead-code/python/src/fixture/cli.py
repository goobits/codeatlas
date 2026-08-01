from . import alias_target as target
from .api import public_api


def main():
    public_api()
    target.cli_alias_value()
