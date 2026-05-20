from pathlib import Path

import yaml
from fastapi import FastAPI, HTTPException, Request
from fastapi.responses import HTMLResponse, StreamingResponse
from fastapi.staticfiles import StaticFiles
from fastapi.templating import Jinja2Templates

from build_runner import builds, start_build, stream_events

BASE         = Path(__file__).parent
PROFILES_DIR = BASE / "profiles"
PROFILES_DIR.mkdir(exist_ok=True)

app = FastAPI(title="mobileos Configurator")
app.mount("/static", StaticFiles(directory=BASE / "static"), name="static")
templates = Jinja2Templates(directory=BASE / "templates")

with open(BASE / "targets.yaml") as f:
    TARGETS = yaml.safe_load(f)

with open(BASE / "packages.yaml") as f:
    PACKAGES = yaml.safe_load(f)


@app.get("/", response_class=HTMLResponse)
async def index(request: Request):
    return templates.TemplateResponse("index.html", {"request": request})


@app.get("/api/targets")
async def get_targets():
    return TARGETS


@app.get("/api/packages")
async def get_packages():
    return PACKAGES


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


@app.post("/api/profiles/{name}/build")
async def build_profile(name: str):
    path = PROFILES_DIR / f"{name}.yaml"
    if not path.exists():
        raise HTTPException(404, "Profile not found")
    with open(path) as f:
        profile = yaml.safe_load(f)
    profile["_targets"] = TARGETS
    build_id = start_build(profile)
    return {"build_id": build_id}


@app.post("/api/builds/start")
async def build_inline(request: Request):
    profile = await request.json()
    profile["_targets"] = TARGETS
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
