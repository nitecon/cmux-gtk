"""Shared assertions for retained protocol scenarios."""
from cmux import cmuxError


def require(condition: object, message: str) -> None:
    """Raise the protocol client's existing error with the supplied detail when a condition is false."""
    if not condition:
        raise cmuxError(message)
