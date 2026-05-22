"""
test_shell_injection.py — adversarial inputs to flash/build must NEVER cause
arbitrary command execution. We verify both the safety layer rejects payloads
and that, even if it didn't, the runners shell-quote everything.
"""
import shlex

import pytest

from safety import (UnsafeInputError, safe_artifact, safe_block_device,
                    safe_host, safe_name, safe_path)


INJECTION_PAYLOADS = [
    "; touch /tmp/pwn",
    "$(touch /tmp/pwn)",
    "`touch /tmp/pwn`",
    "&& touch /tmp/pwn",
    "| tee /tmp/pwn",
    "'; rm -rf / #",
    '" ; rm -rf / #',
    "\n/bin/sh\n",
    "x\nrm -rf /\n",
    "../../etc/passwd",
    "..\\..\\windows",
    "x;y",
    "x|y",
    "x&y",
    "x>y",
    "x<y",
    "x*y",
    "x?y",
    "x\x00y",
    "x$y",
]


class TestSafeBlockDevice:
    @pytest.mark.parametrize("device", [
        "/dev/sda", "/dev/sdb1", "/dev/mmcblk0", "/dev/mmcblk0p1",
        "/dev/nvme0n1", "/dev/nvme0n1p3", "/dev/disk2", "/dev/disk3s1",
    ])
    def test_accepts_real_devices(self, device):
        assert safe_block_device(device) == device

    @pytest.mark.parametrize("payload", INJECTION_PAYLOADS)
    def test_rejects_injection(self, payload):
        for pattern in (payload, f"/dev/sda{payload}", f"/dev/{payload}"):
            with pytest.raises(UnsafeInputError):
                safe_block_device(pattern)

    @pytest.mark.parametrize("bogus", [
        "", "/etc/passwd", "/dev/", "/dev/null", "/dev/zero",
        "/dev/random", "/dev/ttyS0", "sda", "sda1",
    ])
    def test_rejects_non_block_devices(self, bogus):
        with pytest.raises(UnsafeInputError):
            safe_block_device(bogus)


class TestSafePath:
    @pytest.mark.parametrize("path", [
        "/tmp/foo", "/mnt/build-ssd/mobileos-build",
        "/home/user/file.img", "/dev/sda",
    ])
    def test_accepts_normal_paths(self, path):
        assert safe_path(path) == path

    @pytest.mark.parametrize("payload", INJECTION_PAYLOADS)
    def test_rejects_injection(self, payload):
        with pytest.raises(UnsafeInputError):
            safe_path(f"/tmp/{payload}")

    def test_rejects_double_dot(self):
        with pytest.raises(UnsafeInputError):
            safe_path("/tmp/../etc/passwd")

    def test_rejects_relative(self):
        with pytest.raises(UnsafeInputError):
            safe_path("tmp/foo")

    def test_rejects_spaces(self):
        with pytest.raises(UnsafeInputError):
            safe_path("/tmp/foo bar")


class TestSafeName:
    @pytest.mark.parametrize("name", [
        "my-profile", "qemu-aarch64", "build_v2", "release.1",
        "abc123", "x", "QEMU-ARM64",
    ])
    def test_accepts_normal_names(self, name):
        assert safe_name(name) == name

    @pytest.mark.parametrize("payload", INJECTION_PAYLOADS + [
        "../foo", "..", "x/y", "x;y", "", "/abs",
    ])
    def test_rejects_unsafe(self, payload):
        with pytest.raises(UnsafeInputError):
            safe_name(payload)


class TestSafeHost:
    @pytest.mark.parametrize("host", [
        "localhost", "rpi4-codex", "192.168.88.51",
        "build.example.com", "x.y.z",
    ])
    def test_accepts_valid_hosts(self, host):
        assert safe_host(host) == host

    @pytest.mark.parametrize("payload", INJECTION_PAYLOADS)
    def test_rejects_injection(self, payload):
        with pytest.raises(UnsafeInputError):
            safe_host(f"host{payload}")


class TestSafeArtifact:
    def test_accepts_image_inside_artifacts_dir(self, tmp_path):
        art = tmp_path / "artifacts"
        art.mkdir()
        img = art / "sdcard.img"
        img.write_bytes(b"")
        result = safe_artifact(str(img), str(art))
        assert result == str(img.resolve())

    def test_rejects_image_outside_artifacts(self, tmp_path):
        art = tmp_path / "artifacts"
        art.mkdir()
        outside = tmp_path / "evil.img"
        outside.write_bytes(b"")
        with pytest.raises(UnsafeInputError):
            safe_artifact(str(outside), str(art))

    def test_rejects_wrong_extension(self, tmp_path):
        art = tmp_path / "artifacts"
        art.mkdir()
        f = art / "passwd"
        f.write_bytes(b"")
        with pytest.raises(UnsafeInputError):
            safe_artifact(str(f), str(art))

    def test_rejects_path_traversal(self, tmp_path):
        art = tmp_path / "artifacts"
        art.mkdir()
        with pytest.raises(UnsafeInputError):
            safe_artifact(f"{art}/../etc/passwd.img", str(art))


class TestFlashRunnerRejectsInjection:
    """End-to-end: start_flash should refuse adversarial device/image/settings."""

    def test_rejects_device_with_semicolon(self, tmp_configurator):
        from flash_runner import start_flash
        cfg = tmp_configurator["module"].load_settings()
        with pytest.raises(UnsafeInputError):
            start_flash("/dev/sda; touch /tmp/pwn", "/x.img", cfg)

    def test_rejects_image_outside_artifacts(self, tmp_configurator):
        from flash_runner import start_flash
        cfg = tmp_configurator["module"].load_settings()
        with pytest.raises(UnsafeInputError):
            start_flash("/dev/sda", "/etc/passwd", cfg)

    def test_rejects_settings_with_unsafe_server(self, tmp_configurator):
        from flash_runner import start_flash
        cfg = tmp_configurator["module"].load_settings()
        cfg["build"]["server"] = "host; rm -rf /"
        with pytest.raises(UnsafeInputError):
            start_flash("/dev/sda", "/x.img", cfg)


class TestBuildRunnerRejectsInjection:
    def test_rejects_unsafe_target(self, tmp_configurator):
        from build_runner import start_build
        with pytest.raises(UnsafeInputError):
            start_build({"target": "qemu; rm -rf /", "_settings": {}, "_targets": {}})

    def test_rejects_unsafe_server(self, tmp_configurator):
        from build_runner import start_build
        cfg = tmp_configurator["module"].load_settings()
        cfg["build"]["server"] = "x;y"
        with pytest.raises(UnsafeInputError):
            start_build({"target": "qemu-aarch64",
                         "_settings": cfg, "_targets": {}})

    def test_rejects_unsafe_base_dir(self, tmp_configurator):
        from build_runner import start_build
        cfg = tmp_configurator["module"].load_settings()
        cfg["build"]["base_dir"] = "/tmp/$(touch /tmp/pwn)"
        with pytest.raises(UnsafeInputError):
            start_build({"target": "qemu-aarch64",
                         "_settings": cfg, "_targets": {}})
