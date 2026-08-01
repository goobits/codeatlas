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


def _register(function):
    return function


@_register
def registered_func():
    return 3
