"""
test_daemon_protocol.py — fuzz Unix-socket protocols of mobileos daemons.

These tests connect to live daemon sockets and send adversarial inputs:
- malformed JSON, oversized lines, NUL bytes, control characters, partial writes
- AT-command injection payloads for simd
- wpa_supplicant injection for netd

If a socket doesn't exist (no running daemon), the test is SKIPPED. This is
intentional — on developer machines daemons aren't running; on a target device
running mobileos these tests verify the daemons don't panic on hostile input.

To run on target:
    scp tests/test_daemon_protocol.py mobileos-device:/tmp/
    ssh mobileos-device 'cd /tmp && python3 -m pytest test_daemon_protocol.py -v'
"""
import json
import os
import socket
import time
from pathlib import Path

import pytest


DAEMONS = {
    "commd-ui":      "/run/commd/ui.sock",
    "commd-backend": "/run/commd/backend.sock",
    "commd-admin":   "/run/commd/admin.sock",
    "netd":          "/run/netd.sock",
    "powerd":        "/run/powerd.sock",
    "localbe":       "/run/localbe.sock",
    "hardwared":     "/run/hardwared.sock",
    "simd":          "/run/simd.sock",
    "telegramd":     "/run/telegramd.sock",
}


# Adversarial payloads designed to crash naive parsers.
FUZZ_PAYLOADS = [
    b"",                                       # empty line
    b"\n",
    b"\r\n",
    b"\x00",                                   # NUL
    b"A" * 1_000_000,                          # 1 MB line — bounded reader?
    b"\xff\xfe\xfd\xfc",                       # invalid UTF-8
    b"{invalid json\n",
    b'{"nested": ' + b'{"a": ' * 1000 + b'1' + b'}' * 1000 + b'}\n',  # deep nesting
    b"STATUS\x00DIAL 0\n",                     # NUL-separated commands
    b"STATUS\nSTATUS\nSTATUS\n" * 100,         # rapid-fire
    b"SET_BRIGHTNESS\n",                       # missing arg — powerd L300 panic
    b"SET_BRIGHTNESS ",                        # arg-less, no newline
    b"DIAL 0; AT+CMGD=1,4\n",                  # AT injection (simd)
    b"DIAL \xe2\x98\xa0\n",                    # emoji in number
    b"\x1b[31mevil\x1b[0m\n",                  # ANSI escapes
]


def _socket_available(path: str) -> bool:
    if not os.path.exists(path):
        return False
    try:
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.settimeout(0.5)
        s.connect(path)
        s.close()
        return True
    except (OSError, socket.timeout):
        return False


def _send_and_check_alive(sock_path: str, payload: bytes,
                          *, recv_timeout: float = 1.0) -> bool:
    """Send `payload`, then verify daemon still accepts a new connection."""
    try:
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.settimeout(recv_timeout)
        s.connect(sock_path)
        s.sendall(payload)
        # Try to read a response (may be empty / error / nothing)
        try:
            s.recv(4096)
        except socket.timeout:
            pass
        s.close()
    except (OSError, socket.timeout):
        # Connection error on this attempt is OK if the daemon is still alive
        pass
    # Verify daemon is alive: open a fresh connection
    time.sleep(0.1)
    return _socket_available(sock_path)


@pytest.mark.parametrize("daemon,path", list(DAEMONS.items()))
class TestDaemonAvailability:
    """Sanity: if daemons are deployed, every listed socket should be reachable."""

    def test_socket_reachable(self, daemon, path):
        if not _socket_available(path):
            pytest.skip(f"{daemon} not running (no socket at {path})")
        assert _socket_available(path)


@pytest.mark.parametrize("daemon,path", list(DAEMONS.items()))
@pytest.mark.parametrize("payload", FUZZ_PAYLOADS,
                          ids=lambda p: repr(p[:32]) + ("..." if len(p) > 32 else ""))
class TestDaemonSurvivesFuzz:
    """For each daemon × each payload: send, then check daemon is still alive."""

    def test_does_not_crash(self, daemon, path, payload):
        if not _socket_available(path):
            pytest.skip(f"{daemon} not running")
        alive = _send_and_check_alive(path, payload)
        assert alive, (
            f"{daemon} died after receiving payload {payload[:64]!r} — "
            f"likely a panic in the connection handler"
        )


class TestSpecificDaemonBugs:
    """Regression tests for specific bugs found in code review."""

    def test_powerd_set_brightness_no_arg(self):
        """powerd L300: cmd[15..] panics if cmd == 'SET_BRIGHTNESS' (14 bytes)."""
        path = DAEMONS["powerd"]
        if not _socket_available(path):
            pytest.skip("powerd not running")
        # Exact 14-byte command — triggers byte-slice panic
        assert _send_and_check_alive(path, b"SET_BRIGHTNESS\n"), \
            "powerd crashed on bare SET_BRIGHTNESS"
        # With multi-byte UTF-8 right after 'SET_BRIGHTNESS '
        assert _send_and_check_alive(path, "SET_BRIGHTNESS ☠\n".encode()), \
            "powerd crashed on UTF-8 after SET_BRIGHTNESS"

    def test_simd_dial_with_at_injection(self):
        """simd L220: ATD{number}; injection via 'DIAL 0; AT+CMGD=1,4'."""
        path = DAEMONS["simd"]
        if not _socket_available(path):
            pytest.skip("simd not running")
        # Daemon must reject (or escape) — not pass through to modem
        assert _send_and_check_alive(path, b"DIAL 0; AT+CMGD=1,4\n"), \
            "simd crashed on AT injection"
        # If daemon sends literal injected string to /dev/ttyAMA0, that's a security
        # bug we can only confirm with a mock modem (see test_simd_mock_modem.py).

    def test_netd_wifi_psk_injection(self):
        """netd L154: format!(... ssid={} psk={}) breaks out of wpa_supplicant.conf."""
        path = DAEMONS["netd"]
        if not _socket_available(path):
            pytest.skip("netd not running")
        # Try to inject extra wpa_supplicant network block via newlines in PSK
        evil_payload = json.dumps({
            "type":  "WIFI_CONNECT",
            "ssid":  "test",
            "psk":   "abc\"\nnetwork={\n\tssid=\"evil\"\n\tkey_mgmt=NONE\n}",
        }).encode() + b"\n"
        assert _send_and_check_alive(path, evil_payload), \
            "netd crashed on WIFI_CONNECT injection"
        # After this, /data/netd/wpa_supplicant.conf should NOT contain the
        # "ssid=\"evil\"" block. Verify (requires read access — skipped here).
        conf = Path("/data/netd/wpa_supplicant.conf")
        if conf.exists() and os.access(conf, os.R_OK):
            content = conf.read_text()
            assert "ssid=\"evil\"" not in content, \
                "netd wrote attacker-controlled network into wpa_supplicant.conf!"

    def test_commd_oversized_message(self):
        """Unbounded buffered_reader.read_line() — memory exhaustion via huge line."""
        path = DAEMONS["commd-ui"]
        if not _socket_available(path):
            pytest.skip("commd not running")
        # 10 MB of garbage, no newline
        assert _send_and_check_alive(path, b"X" * (10 * 1024 * 1024),
                                      recv_timeout=2.0), \
            "commd OOM on oversized line — needs read_line() size cap"
