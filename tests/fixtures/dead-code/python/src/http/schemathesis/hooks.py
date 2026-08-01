import http.client
from http.cookies import SimpleCookie


def response_name(status):
    SimpleCookie()
    return http.client.responses.get(status, "Unknown")
