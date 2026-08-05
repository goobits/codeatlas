def publish(bundle: str) -> bool:
    """@codeatlas-fuzz deny: publishes to the real artifact registry"""
    return bool(bundle)


def stale_allow(bundle: str) -> bool:
    """@codeatlas-fuzz allow: stale comments may not grant authority"""
    return bool(bundle)
