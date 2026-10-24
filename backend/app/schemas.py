from datetime import datetime
from typing import Literal
from uuid import UUID

from pydantic import BaseModel, Field, field_validator

AgeGroup = Literal["under_18", "18_24", "25_34", "35_44", "45_plus"]


class User(BaseModel):
    """Public profile. Never includes the token."""

    id: UUID
    name: str
    school: str | None
    bio: str | None
    discord_handle: str | None
    age_group: AgeGroup | None
    country: str | None
    sharing_enabled: bool
    created_at: datetime


class UserUpdate(BaseModel):
    """PATCH body: only the fields sent are changed. Send null to clear one."""

    name: str | None = Field(default=None, min_length=1, max_length=60)
    school: str | None = Field(default=None, max_length=80)
    bio: str | None = Field(default=None, max_length=120)
    discord_handle: str | None = Field(default=None, max_length=40)
    age_group: AgeGroup | None = None
    country: str | None = Field(default=None, pattern=r"^[A-Za-z]{2}$")
    sharing_enabled: bool | None = None

    @field_validator("country")
    @classmethod
    def upper_country(cls, value: str | None) -> str | None:
        return value.upper() if value else value
