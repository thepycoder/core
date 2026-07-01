from contextlib import asynccontextmanager
from pathlib import Path

from dotenv import load_dotenv
from fastapi import FastAPI
from fastapi.staticfiles import StaticFiles

from app.db import get_db
from app.routes.api import router as api_router

STATIC_DIR = Path(__file__).resolve().parent / "static"


@asynccontextmanager
async def lifespan(_app: FastAPI):
    load_dotenv(Path(__file__).resolve().parents[2] / ".env")
    load_dotenv(Path(__file__).resolve().parents[3] / ".env")
    get_db()
    yield


app = FastAPI(title="Graph Debug Viewer", lifespan=lifespan)
app.include_router(api_router)
app.mount("/", StaticFiles(directory=STATIC_DIR, html=True), name="static")
