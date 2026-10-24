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


class PushFilters(BaseModel):
    """Optional match filters, compared against my own profile."""

    same_country: bool = False
    same_age_group: bool = False


class TaskPush(BaseModel):
    """What the app is working on now, summarized on the device."""

    task_summary: str = Field(min_length=1, max_length=1000)
    topic: str | None = Field(default=None, max_length=60)   # short label, e.g. "LeetCode"
    filters: PushFilters = PushFilters()


class CandidateScore(BaseModel):
    user_id: UUID
    name: str
    score: float


class MatchResult(BaseModel):
    """Outcome of the match search run by a push."""

    match_id: int | None
    other_name: str | None
    scores: list[CandidateScore]                 # Jev P(good match), best first
    skipped_reason: str | None                   # why no search ran / nothing scored


class CurrTask(BaseModel):
    user_id: UUID
    task_summary: str
    topic: str | None
    pushed_at: datetime
    match: MatchResult


MatchStatus = Literal["pending", "accepted", "declined"]


class OtherPerson(BaseModel):
    """The other side of a match: public profile plus their live task."""

    id: UUID
    name: str
    school: str | None
    bio: str | None
    age_group: AgeGroup | None
    country: str | None
    discord_handle: str | None
    task_summary: str | None


class MatchView(BaseModel):
    match_id: int
    topic: str
    score: float | None
    created_at: datetime
    my_status: MatchStatus
    their_status: MatchStatus
    expired: bool                                # still undecided after 2 min
    other: OtherPerson


class RespondBody(BaseModel):
    accept: bool
