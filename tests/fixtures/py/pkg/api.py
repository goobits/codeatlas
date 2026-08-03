from typing import Literal

PUBLIC_TIMEOUT: int = 30
PUBLIC_LABEL = "fixture-secret"


def public_func():
    """Return the public fixture value."""
    def nested_helper():
        return 1

    return nested_helper()


class PublicClient:
    endpoint: str
    retries = 3

    def request(self):
        return self.endpoint


def unused_func():
    return 2


class CatModel:
    pass


class DogModel:
    pass


class PetModel:
    pet: "CatModel | DogModel"


class LiteralOnlyModel:
    pass


class TaggedModel:
    kind: Literal["LiteralOnlyModel"]


class RequestModel:
    pass


class ResponseModel:
    pass


def typed_endpoint(request: "RequestModel") -> "ResponseModel":
    raise NotImplementedError


def cli_only():
    return 4


def plugin_only():
    return 5


def poetry_script_only():
    return 6


def poetry_plugin_only():
    return 7


def _register(function):
    return function


@_register
def registered_func():
    return 3
