import _thread
import time
from concurrent.futures import Future, ThreadPoolExecutor, as_completed
from dataclasses import dataclass
from typing import Dict, Tuple

import httpx

_TIMEOUT = 30.0

_EXECUTOR = ThreadPoolExecutor(max_workers=8)

# In-memory cache for connection results
# Key: (tuple of urls, filter_out_bad, use_ipv6)
# Value: (ConnectionResult | None, timestamp)
_CONNECTION_CACHE: Dict[
    Tuple[tuple, bool, bool], Tuple["ConnectionResult | None", float]
] = {}
_CACHE_TTL = 60.0 * 60.0  # Cache results for 1 hour


@dataclass
class ConnectionResult:
    host: str
    success: bool
    ipv4: bool


def _sanitize_host(host: str) -> str:
    if host.startswith("http"):
        return host
    is_local_host = "localhost" in host or "127.0.0.1" in host or "0.0.0.0" in host
    use_https = not is_local_host
    if use_https:
        return host if host.startswith("https://") else f"https://{host}"
    return host if host.startswith("http://") else f"http://{host}"


def _test_connection(host: str, use_ipv4: bool) -> ConnectionResult:
    # Function static cache
    host = _sanitize_host(host)
    transport = httpx.HTTPTransport(local_address="0.0.0.0") if use_ipv4 else None
    result: ConnectionResult | None = None
    try:
        with httpx.Client(
            timeout=_TIMEOUT,
            transport=transport,
        ) as test_client:
            test_response = test_client.get(
                f"{host}/healthz", timeout=3, follow_redirects=True
            )
            result = ConnectionResult(host, test_response.status_code == 200, use_ipv4)
    except KeyboardInterrupt:
        _thread.interrupt_main()
        result = ConnectionResult(host, False, use_ipv4)
    except TimeoutError:
        result = ConnectionResult(host, False, use_ipv4)
    except Exception:
        result = ConnectionResult(host, False, use_ipv4)
    return result


def find_good_connection(
    urls: list[str], filter_out_bad=True, use_ipv6: bool = True
) -> ConnectionResult | None:
    # Create cache key from parameters
    cache_key = (tuple(sorted(urls)), filter_out_bad, use_ipv6)
    current_time = time.time()

    # Check if we have a cached result
    if cache_key in _CONNECTION_CACHE:
        cached_result, cached_time = _CONNECTION_CACHE[cache_key]
        if current_time - cached_time < _CACHE_TTL:
            return cached_result
        else:
            # Remove expired cache entry
            del _CONNECTION_CACHE[cache_key]

    # No valid cache entry, perform the actual connection test
    futures: list[Future] = []
    for url in urls:

        f = _EXECUTOR.submit(_test_connection, url, use_ipv4=True)
        futures.append(f)
        if use_ipv6 and "localhost" not in url:
            f_v6 = _EXECUTOR.submit(_test_connection, url, use_ipv4=False)
            futures.append(f_v6)

    result = None
    try:
        # Return first successful result
        for future in as_completed(futures):
            connection_result: ConnectionResult = future.result()
            if connection_result.success or not filter_out_bad:
                result = connection_result
                break
    finally:
        # Cancel any remaining futures
        for future in futures:
            future.cancel()

    # Cache the result (even if None)
    _CONNECTION_CACHE[cache_key] = (result, current_time)

    return result
