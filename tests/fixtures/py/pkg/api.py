def public_func():
    """Return the public fixture value."""
    return 1


def unused_func():
    return 2


def _register(function):
    return function


@_register
def registered_func():
    return 3
