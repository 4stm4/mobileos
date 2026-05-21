from pathlib import Path

import yaml
from fastapi import FastAPI, HTTPException, Request
from fastapi.responses import HTMLResponse, StreamingResponse
from fastapi.staticfiles import StaticFiles
from fastapi.templating import Jinja2Templates

from build_runner import builds, start_build, stream_events
from flash_runner import (flashes, list_artifacts, list_devices,
                          start_flash, stream_flash_events)

BASE         = Path(__file__).parent
PROFILES_DIR = BASE / "profiles"
PROFILES_DIR.mkdir(exist_ok=True)
SETTINGS_FILE = BASE / "settings.yaml"

app = FastAPI(title="mobileos Configurator")
app.mount("/static", StaticFiles(directory=BASE / "static"), name="static")
templates = Jinja2Templates(directory=BASE / "templates")

with open(BASE / "targets.yaml") as f:
    TARGETS = yaml.safe_load(f)

with open(BASE / "packages.yaml") as f:
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
    with open(SETTINGS_FILE, "w") as f:
        yaml.dump(body, f, allow_unicode=True, sort_keys=False)
    return {"ok": True}


# ── Profiles ─────────────────────────────────────────────────────────

@app.get("/api/profiles")
async def list_profiles():
    return [{"name": f.stem} for f in sorted(PROFILES_DIR.glob("*.yaml"))]


@app.get("/api/profiles/{name}")
async def get_profile(name: str):
    path = PROFILES_DIR / f"{name}.yaml"
    if not path.exists():
        raise HTTPException(404, "Profile not found")
    with open(path) as f:
        return yaml.safe_load(f)


@app.put("/api/profiles/{name}")
async def save_profile(name: str, request: Request):
    body = await request.json()
    path = PROFILES_DIR / f"{name}.yaml"
    with open(path, "w") as f:
        yaml.dump(body, f, allow_unicode=True, sort_keys=False)
    return {"ok": True}


@app.delete("/api/profiles/{name}")
async def delete_profile(name: str):
    path = PROFILES_DIR / f"{name}.yaml"
    if path.exists():
        path.unlink()
    return {"ok": True}


# ── Builds ───────────────────────────────────────────────────────────

@app.post("/api/profiles/{name}/build")
async def build_profile(name: str):
    path = PROFILES_DIR / f"{name}.yaml"
    if not path.exists():
        raise HTTPException(404, "Profile not found")
    with open(path) as f:
        profile = yaml.safe_load(f)
    profile["_targets"]  = TARGETS
    profile["_settings"] = load_settings()
    build_id = start_build(profile)
    return {"build_id": build_id}


@app.post("/api/builds/start")
async def build_inline(request: Request):
    profile = await request.json()
    profile["_targets"]  = TARGETS
    profile["_settings"] = load_settings()
    build_id = start_build(profile)
    return {"build_id": build_id}


@app.get("/api/builds")
async def list_builds():
    return list(builds.values())


@app.get("/api/builds/{build_id}")
async def get_build(build_id: str):
    b = builds.get(build_id)
    if not b:
        raise HTTPException(404, "Build not found")
    return b


@app.get("/api/builds/{build_id}/events")
async def build_events(build_id: str):
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
    device = body.get("device")
    image  = body.get("image")
    if not device or not image:
        raise HTTPException(400, "device and image required")
    flash_id = start_flash(device, image, load_settings())
    return {"flash_id": flash_id}


@app.get("/api/flash/{flash_id}/events")
async def flash_events(flash_id: str):
    async def generate():
        async for chunk in stream_flash_events(flash_id):
            yield chunk
    return StreamingResponse(generate(), media_type="text/event-stream",
                             headers={"Cache-Control": "no-cache",
                                      "X-Accel-Buffering": "no"})


@app.get("/api/flash")
async def list_flashes():
    return list(flashes.values())
