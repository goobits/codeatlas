import pytest

from fixture.test_support import test_support


@pytest.mark.unit
def test_support_value():
    assert test_support()


test_support()
