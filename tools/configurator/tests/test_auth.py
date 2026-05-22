"""
test_auth.py — Basic Auth must protect every route (UI, /api/*, SSE, /static).
"""
import base64


def _hdr(user: str, password: str) -> dict:
    raw = f"{user}:{password}".encode()
    return {"Authorization": "Basic " + base64.b64encode(raw).decode()}


PROTECTED_ROUTES = [
    ("GET",  "/"),
    ("GET",  "/api/targets"),
    ("GET",  "/api/packages"),
    ("GET",  "/api/settings"),
    ("PUT",  "/api/settings"),
    ("GET",  "/api/profiles"),
    ("GET",  "/api/profiles/test"),
    ("PUT",  "/api/profiles/test"),
    ("DELETE", "/api/profiles/test"),
    ("POST", "/api/builds/start"),
    ("GET",  "/api/builds"),
    ("GET",  "/api/devices"),
    ("GET",  "/api/artifacts"),
    ("POST", "/api/flash/start"),
    ("GET",  "/api/flash"),
]


class TestAuthRequired:
    def test_no_credentials_returns_401(self, tmp_configurator_with_auth):
        c = tmp_configurator_with_auth["client"]
        for method, path in PROTECTED_ROUTES:
            r = c.request(method, path,
                          json={} if method in ("PUT", "POST") else None)
            assert r.status_code == 401, \
                f"{method} {path} should require auth, got {r.status_code}"
            # RFC 7235 — must include WWW-Authenticate
            assert "www-authenticate" in {k.lower() for k in r.headers}, \
                f"{method} {path} missing WWW-Authenticate header"

    def test_wrong_password_returns_401(self, tmp_configurator_with_auth):
        c = tmp_configurator_with_auth["client"]
        for method, path in PROTECTED_ROUTES:
            r = c.request(method, path,
                          headers=_hdr("admin", "wrong"),
                          json={} if method in ("PUT", "POST") else None)
            assert r.status_code == 401, \
                f"{method} {path} with bad password returned {r.status_code}"

    def test_wrong_username_returns_401(self, tmp_configurator_with_auth):
        c = tmp_configurator_with_auth["client"]
        r = c.get("/api/targets", headers=_hdr("attacker", "secret123"))
        assert r.status_code == 401

    def test_correct_credentials_pass(self, tmp_configurator_with_auth):
        c = tmp_configurator_with_auth["client"]
        r = c.get("/api/targets", headers=_hdr("admin", "secret123"))
        # /api/targets needs a real targets.yaml — but at minimum, not 401
        assert r.status_code != 401

    def test_static_files_also_protected(self, tmp_configurator_with_auth):
        c = tmp_configurator_with_auth["client"]
        r = c.get("/static/app.css")
        assert r.status_code == 401

    def test_malformed_auth_header_returns_401(self, tmp_configurator_with_auth):
        c = tmp_configurator_with_auth["client"]
        for header in (
            {"Authorization": "Bearer abc"},
            {"Authorization": "Basic notbase64!"},
            {"Authorization": "Basic " + base64.b64encode(b"no-colon").decode()},
            {"Authorization": ""},
        ):
            r = c.get("/api/targets", headers=header)
            assert r.status_code == 401

    def test_constant_time_compare_does_not_leak_timing(self,
                                                         tmp_configurator_with_auth):
        """Sanity: BasicAuthMiddleware uses secrets.compare_digest."""
        from auth import BasicAuthMiddleware
        import inspect
        src = inspect.getsource(BasicAuthMiddleware)
        assert "compare_digest" in src, \
            "Basic Auth must use secrets.compare_digest (constant time)"


class TestAuthDisabled:
    """If auth.disabled: true or no auth config, all routes are accessible."""

    def test_disabled_auth_allows_anonymous(self, tmp_configurator):
        c = tmp_configurator["client"]
        r = c.get("/api/settings")
        assert r.status_code == 200
