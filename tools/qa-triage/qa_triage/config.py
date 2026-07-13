from functools import lru_cache
from pathlib import Path

from pydantic import field_validator
from pydantic_settings import BaseSettings, SettingsConfigDict

CORE_ROOT = Path(__file__).resolve().parents[3]


class Settings(BaseSettings):
    model_config = SettingsConfigDict(
        env_file=(CORE_ROOT / ".env", ".env"),
        env_file_encoding="utf-8",
        extra="ignore",
    )

    scraper_data_dir: Path = Path("data")
    scraper_cache_dir: Path = Path("cache")
    mistral_api_token: str = ""  # env: MISTRAL_API_TOKEN

    @field_validator("scraper_data_dir", "scraper_cache_dir", mode="before")
    @classmethod
    def resolve_path(cls, value: str | Path) -> Path:
        path = Path(value)
        if path.is_absolute():
            return path
        core_candidate = (CORE_ROOT / path).resolve()
        if core_candidate.exists():
            return core_candidate
        return (Path.cwd() / path).resolve()

    @property
    def data_dir(self) -> Path:
        return self.scraper_data_dir.resolve()

    @property
    def cache_dir(self) -> Path:
        return self.scraper_cache_dir.resolve()

    @property
    def qa_dir(self) -> Path:
        return self.data_dir / "qa"

    @property
    def reports_dir(self) -> Path:
        return self.qa_dir / "reports"

    def parquet(self, *parts: str) -> Path:
        return self.data_dir.joinpath(*parts)


@lru_cache
def get_settings() -> Settings:
    return Settings()
