from contextlib import asynccontextmanager
from pathlib import Path

from dotenv import load_dotenv
from fastapi import FastAPI
from fastapi.responses import HTMLResponse
from fastapi.staticfiles import StaticFiles
from starlette.middleware.base import BaseHTTPMiddleware
from starlette.requests import Request
from starlette.responses import Response

from app.db import get_db
from app.routes.api import router as api_router
from app.static_assets import STATIC_DIR, asset_version


class NoCacheStaticMiddleware(BaseHTTPMiddleware):
    async def dispatch(self, request: Request, call_next) -> Response:
        response = await call_next(request)
        path = request.url.path
        if path in {"/", "/app.js", "/style.css"} or path.endswith((".js", ".css")):
            response.headers["Cache-Control"] = "no-store, no-cache, must-revalidate"
            response.headers["Pragma"] = "no-cache"
        return response


@asynccontextmanager
async def lifespan(_app: FastAPI):
    load_dotenv(Path(__file__).resolve().parents[2] / ".env")
    load_dotenv(Path(__file__).resolve().parents[3] / ".env")
    get_db()
    yield


app = FastAPI(title="Graph Debug Viewer", lifespan=lifespan)
app.add_middleware(NoCacheStaticMiddleware)
app.include_router(api_router)


@app.get("/", response_class=HTMLResponse, include_in_schema=False)
async def index_page() -> HTMLResponse:
    version = asset_version()
    content = (STATIC_DIR / "index.html").read_text(encoding="utf-8")
    content = content.replace("{{ASSET_VERSION}}", version)
    return HTMLResponse(
        content,
        headers={"Cache-Control": "no-store, no-cache, must-revalidate"},
    )


app.mount("/", StaticFiles(directory=STATIC_DIR, html=False), name="static")
