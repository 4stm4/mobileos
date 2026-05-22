"""
test_path_traversal.py — /api/profiles/{name} must not read/write/delete
files outside PROFILES_DIR, regardless of URL encoding tricks.
"""
import urllib.parse


# Note: "" and "." normalize at the HTTP layer to /api/profiles/ (the collection
# endpoint), which is intentionally accessible — they never reach the handler,
# so they're not security-relevant. Same for ".." which routes to a parent path.
TRAVERSAL_NAMES = [
    "../etc/passwd",
    "..%2Fetc%2Fpasswd",
    "..%2F..%2Fetc%2Fpasswd",
    "../../settings",
    "/etc/passwd",
    "x/y",
    "x\\y",
    "name\x00.yaml",
    "a" * 200,  # too long
    "name with spaces",
    "name;rm -rf /",
]


class TestProfileTraversal:
    def test_get_profile_rejects_traversal(self, tmp_configurator):
        c = tmp_configurator["client"]
        for name in TRAVERSAL_NAMES:
            r = c.get(f"/api/profiles/{urllib.parse.quote(name, safe='')}")
            assert r.status_code in (400, 404, 405, 422), \
                f"profile name {name!r} should be rejected, got {r.status_code}"

    def test_put_profile_rejects_traversal(self, tmp_configurator):
        c = tmp_configurator["client"]
        for name in TRAVERSAL_NAMES:
            r = c.put(f"/api/profiles/{urllib.parse.quote(name, safe='')}",
                      json={"foo": "bar"})
            assert r.status_code in (400, 404, 405, 422), \
                f"PUT {name!r} should be rejected, got {r.status_code}"

    def test_delete_profile_rejects_traversal(self, tmp_configurator):
        c = tmp_configurator["client"]
        for name in TRAVERSAL_NAMES:
            r = c.delete(f"/api/profiles/{urllib.parse.quote(name, safe='')}")
            assert r.status_code in (400, 404, 405, 422), \
                f"DELETE {name!r} should be rejected, got {r.status_code}"

    def test_settings_file_not_clobberable_via_profile_put(self, tmp_configurator):
        """PUT /api/profiles/../settings must NOT overwrite settings.yaml."""
        settings_file = tmp_configurator["settings_file"]
        before = settings_file.read_text()
        c = tmp_configurator["client"]
        for name in ("..%2Fsettings", "..%2F..%2Fsettings"):
            r = c.put(f"/api/profiles/{name}", json={"pwned": True})
            assert r.status_code in (400, 404, 405, 422)
        assert settings_file.read_text() == before, "settings.yaml was clobbered!"

    def test_legit_profile_name_still_works(self, tmp_configurator):
        c = tmp_configurator["client"]
        r = c.put("/api/profiles/my-test", json={"target": "qemu-aarch64"})
        assert r.status_code == 200
        r = c.get("/api/profiles/my-test")
        assert r.status_code == 200
        assert r.json() == {"target": "qemu-aarch64"}
        r = c.delete("/api/profiles/my-test")
        assert r.status_code == 200

    def test_build_endpoint_rejects_traversal_name(self, tmp_configurator):
        c = tmp_configurator["client"]
        for name in TRAVERSAL_NAMES:
            r = c.post(f"/api/profiles/{urllib.parse.quote(name, safe='')}/build")
            assert r.status_code in (400, 404, 405, 422), \
                f"BUILD {name!r} should be rejected, got {r.status_code}"


class TestBuildIdValidation:
    def test_get_build_rejects_traversal(self, tmp_configurator):
        c = tmp_configurator["client"]
        for bad in ("../foo", "x;y", "name with space"):
            r = c.get(f"/api/builds/{urllib.parse.quote(bad, safe='')}")
            assert r.status_code in (400, 404, 405, 422)


class TestSettingsAuthProtection:
    def test_put_settings_cannot_change_auth(self, tmp_configurator):
        """PUT /api/settings must preserve the existing auth section."""
        c = tmp_configurator["client"]
        # Try to disable auth and change credentials
        r = c.put("/api/settings", json={
            "build": {"server": "localhost", "base_dir": "/tmp",
                      "output": {"qemu-aarch64": "out"}},
            "artifacts": {"dir": "/tmp/artifacts"},
            "auth": {"username": "attacker", "password": "pwn", "disabled": True},
        })
        assert r.status_code == 200
        # Re-read settings; auth should be unchanged
        import yaml
        with open(tmp_configurator["settings_file"]) as f:
            on_disk = yaml.safe_load(f)
        assert on_disk["auth"] == {"disabled": True}, \
            f"auth section was modified by PUT: {on_disk['auth']}"
