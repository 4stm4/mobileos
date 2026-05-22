import os
from pathlib import Path

import yaml
from fastapi import FastAPI, HTTPException, Request
from fastapi.responses import HTMLResponse, StreamingResponse
from fastapi.staticfiles import StaticFiles
from fastapi.templating import Jinja2Templates

from auth import BasicAuthMiddleware
from build_runner import builds, start_build, stream_events
from flash_runner import (flashes, list_artifacts, list_devices,
                          start_flash, stream_flash_events)
from safety import UnsafeInputError, safe_name, safe_profile_path

# BASE can be overridden via env (used by the test suite to point at a sandbox)
BASE         = Path(os.environ.get("MOBILEOS_CONF_DIR", Path(__file__).parent))
PROFILES_DIR = BASE / "profiles"
PROFILES_DIR.mkdir(exist_ok=True)
SETTINGS_FILE = BASE / "settings.yaml"
TEMPLATES_DIR = Path(__file__).parent / "templates"
STATIC_DIR    = Path(__file__).parent / "static"

app = FastAPI(title="mobileos Configurator")
app.mount("/static", StaticFiles(directory=STATIC_DIR), name="static")
templates = Jinja2Templates(directory=TEMPLATES_DIR)


def _apply_auth(application: FastAPI) -> None:
    """Add BasicAuthMiddleware if credentials are configured in settings.yaml."""
    try:
        with open(SETTINGS_FILE) as f:
            cfg = yaml.safe_load(f) or {}
        auth = cfg.get("auth", {})
        if auth.get("disabled"):
            return
        username = auth.get("username", "").strip()
        password = auth.get("password", "").strip()
        if username and password:
            application.add_middleware(BasicAuthMiddleware,
                                       username=username, password=password)
            print(f"[auth] Basic Auth enabled for user '{username}'")
        else:
            print("[auth] WARNING: no auth credentials set — configurator is unprotected!")
    except FileNotFoundError:
        pass  # settings not yet created


_apply_auth(app)

# targets/packages catalogues live with the code (not user-configurable)
_CATALOG_DIR = Path(__file__).parent
with open(_CATALOG_DIR / "targets.yaml") as f:
    TARGETS = yaml.safe_load(f)

with open(_CATALOG_DIR / "packages.yaml") as f:
    PACKAGES = yaml.safe_load(f)


def load_settings() -> dict:
    with open(SETTINGS_FILE) as f:
        return yaml.safe_load(f)


# ── UI ───────────────────────────────────────────────────────────────

@app.get("/", response_class=HTMLResponse)
async def index(request: Request):
    return templates.TemplateResponse(request, "index.html")


# ── Targets / Packages ───────────────────────────────────────────────

@app.get("/api/targets")
async def get_targets():
    return TARGETS


@app.get("/api/packages")
async def get_packages():
    return PACKAGES


# ── Settings ─────────────────────────────────────────────────────────

@app.get("/api/settings")
async def get_settings():
    return load_settings()


@app.put("/api/settings")
async def save_settings(request: Request):
    body = await request.json()
    if not isinstance(body, dict):
        raise HTTPException(400, "settings must be an object")
    # NEVER let API clients edit the auth section — that would let an
    # authenticated user lock out the admin or disable auth entirely.
    # Preserve whatever auth config is currently on disk.
    existing = load_settings() or {}
    if "auth" in existing:
        body["auth"] = existing["auth"]
    elif "auth" in body:
        body.pop("auth", None)
    with open(SETTINGS_FILE, "w") as f:
        yaml.dump(body, f, allow_unicode=True, sort_keys=False)
    return {"ok": True}


# ── Profiles ─────────────────────────────────────────────────────────

@app.get("/api/profiles")
async def list_profiles():
    return [{"name": f.stem} for f in sorted(PROFILES_DIR.glob("*.yaml"))]


def _profile_path(name: str):
    try:
        return safe_profile_path(PROFILES_DIR, name)
    except UnsafeInputError as e:
        raise HTTPException(400, str(e))


@app.get("/api/profiles/{name}")
async def get_profile(name: str):
    path = _profile_path(name)
    if not path.exists():
        raise HTTPException(404, "Profile not found")
    with open(path) as f:
        return yaml.safe_load(f)


@app.put("/api/profiles/{name}")
async def save_profile(name: str, request: Request):
    path = _profile_path(name)
    body = await request.json()
    if not isinstance(body, dict):
        raise HTTPException(400, "profile must be an object")
    with open(path, "w") as f:
        yaml.dump(body, f, allow_unicode=True, sort_keys=False)
    return {"ok": True}


@app.delete("/api/profiles/{name}")
async def delete_profile(name: str):
    path = _profile_path(name)
    if path.exists():
        path.unlink()
    return {"ok": True}


# ── Builds ───────────────────────────────────────────────────────────

@app.post("/api/profiles/{name}/build")
async def build_profile(name: str):
    path = _profile_path(name)
    if not path.exists():
        raise HTTPException(404, "Profile not found")
    with open(path) as f:
        profile = yaml.safe_load(f) or {}
    if not isinstance(profile, dict):
        raise HTTPException(400, "profile yaml must be an object")
    # Settings and targets come from the server-side config — clients may NOT
    # override them (otherwise PUT /api/builds/start becomes RCE via _settings).
    profile["_targets"]  = TARGETS
    profile["_settings"] = load_settings()
    try:
        build_id = start_build(profile)
    except UnsafeInputError as e:
        raise HTTPException(400, str(e))
    return {"build_id": build_id}


@app.post("/api/builds/start")
async def build_inline(request: Request):
    profile = await request.json()
    if not isinstance(profile, dict):
        raise HTTPException(400, "profile must be an object")
    # Drop any client-supplied _settings / _targets — server-side only.
    profile.pop("_settings", None)
    profile.pop("_targets", None)
    profile["_targets"]  = TARGETS
    profile["_settings"] = load_settings()
    try:
        build_id = start_build(profile)
    except UnsafeInputError as e:
        raise HTTPException(400, str(e))
    return {"build_id": build_id}


@app.get("/api/builds")
async def list_builds():
    return list(builds.values())


@app.get("/api/builds/{build_id}")
async def get_build(build_id: str):
    try:
        safe_name(build_id, field="build_id")
    except UnsafeInputError as e:
        raise HTTPException(400, str(e))
    b = builds.get(build_id)
    if not b:
        raise HTTPException(404, "Build not found")
    return b


@app.get("/api/builds/{build_id}/events")
async def build_events(build_id: str):
    try:
        safe_name(build_id, field="build_id")
    except UnsafeInputError as e:
        raise HTTPException(400, str(e))
    async def generate():
        async for chunk in stream_events(build_id):
            yield chunk
    return StreamingResponse(generate(), media_type="text/event-stream",
                             headers={"Cache-Control": "no-cache",
                                      "X-Accel-Buffering": "no"})


# ── Flash ────────────────────────────────────────────────────────────

@app.get("/api/devices")
async def get_devices():
    return list_devices(load_settings())


@app.get("/api/artifacts")
async def get_artifacts():
    return list_artifacts(load_settings())


@app.post("/api/flash/start")
async def flash_start(request: Request):
    body   = await request.json()
    if not isinstance(body, dict):
        raise HTTPException(400, "request body must be an object")
    device = body.get("device")
    image  = body.get("image")
    if not device or not image:
        raise HTTPException(400, "device and image required")
    try:
        flash_id = start_flash(device, image, load_settings())
    except UnsafeInputError as e:
        raise HTTPException(400, str(e))
    return {"flash_id": flash_id}


@app.get("/api/flash/{flash_id}/events")
async def flash_events(flash_id: str):
    try:
        safe_name(flash_id, field="flash_id")
    except UnsafeInputError as e:
        raise HTTPException(400, str(e))
    async def generate():
        async for chunk in stream_flash_events(flash_id):
            yield chunk
    return StreamingResponse(generate(), media_type="text/event-stream",
                             headers={"Cache-Control": "no-cache",
                                      "X-Accel-Buffering": "no"})


@app.get("/api/flash")
async def list_flashes():
    return list(flashes.values())
