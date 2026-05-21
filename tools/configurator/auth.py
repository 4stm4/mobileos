"""
Basic Auth middleware for mobileos Configurator.

Credentials are loaded from settings.yaml:
  auth:
    username: admin
    password: changeme

If the 'auth' section is absent or disabled: false — auth is skipped.
"""
import base64
import secrets

from starlette.middleware.base import BaseHTTPMiddleware
from starlette.requests import Request
from starlette.responses import Response


class BasicAuthMiddleware(BaseHTTPMiddleware):
    """HTTP Basic Auth protecting all routes."""

    # Paths that bypass auth (health probes, etc.)
    SKIP_PATHS = {"/health"}

    def __init__(self, app, username: str, password: str):
        super().__init__(app)
        # Store as bytes for compare_digest (constant-time)
        self._user = username.encode()
        self._pass = password.encode()

    async def dispatch(self, request: Request, call_next):
        if request.url.path in self.SKIP_PATHS:
            return await call_next(request)

        auth_header = request.headers.get("Authorization", "")
        if not auth_header.startswith("Basic "):
            return self._deny()

        try:
            decoded = base64.b64decode(auth_header[6:]).decode("utf-8", errors="replace")
            username, _, password = decoded.partition(":")
        except Exception:
            return self._deny()

        ok_user = secrets.compare_digest(username.encode(), self._user)
        ok_pass = secrets.compare_digest(password.encode(), self._pass)

        if not (ok_user and ok_pass):
            return self._deny()

        return await call_next(request)

    @staticmethod
    def _deny() -> Response:
        return Response(
            content="401 Unauthorized — mobileos Configurator requires login",
            status_code=401,
            headers={"WWW-Authenticate": 'Basic realm="mobileos Configurator"'},
        )
