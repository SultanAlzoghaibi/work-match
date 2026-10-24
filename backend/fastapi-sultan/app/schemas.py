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
    career: str | None
    bio: str | None
    discord_handle: str | None
    avatar_url: str | None
    age_group: AgeGroup | None
    country: str | None
    languages: list[str]
    sharing_enabled: bool
    created_at: datetime


class UserUpdate(BaseModel):
    """PATCH body: only the fields sent are changed. Send null to clear one."""

    name: str | None = Field(default=None, min_length=1, max_length=60)
    school: str | None = Field(default=None, max_length=80)
    career: str | None = Field(default=None, max_length=80)
    bio: str | None = Field(default=None, max_length=120)
    discord_handle: str | None = Field(default=None, max_length=40)
    age_group: AgeGroup | None = None
    country: str | None = Field(default=None, pattern=r"^[A-Za-z]{2}$")
    sharing_enabled: bool | None = None
    languages: list[str] | None = Field(default=None, max_length=10)

    @field_validator("languages")
    @classmethod
    def clean_languages(cls, value: list[str] | None) -> list[str] | None:
        if value is None:
            return value
        cleaned = []
        for language in value:
            language = language.strip()[:30]
            if language and language not in cleaned:
                cleaned.append(language)
        return cleaned

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


class TopicScore(BaseModel):
    topic: str
    score: float


class CurrTask(BaseModel):
    user_id: UUID
    task_summary: str
    topic: str | None
    pushed_at: datetime
    match: MatchResult
    connection_topics: list[TopicScore]          # my connections' topics vs this task


MatchStatus = Literal["pending", "accepted", "declined"]


class OtherPerson(BaseModel):
    """The other side of a match: public profile plus their live task."""

    id: UUID
    name: str
    school: str | None
    career: str | None
    bio: str | None
    age_group: AgeGroup | None
    country: str | None
    discord_handle: str | None
    avatar_url: str | None
    languages: list[str] = []
    task_summary: str | None


class MatchView(BaseModel):
    match_id: int
    topic: str
    score: float | None
    created_at: datetime
    my_status: MatchStatus
    their_status: MatchStatus
    expired: bool                                # still undecided after 2 min
    my_keep: bool | None                         # "keep in touch?" after both accept
    their_keep: bool | None
    connected: bool                              # a connection exists for this pair + topic
    room_id: str | None                          # Jitsi room, set when both accept; apps join it
    other: OtherPerson


class RespondBody(BaseModel):
    accept: bool


class KeepBody(BaseModel):
    keep: bool


class Connection(BaseModel):
    """One of my connections (work friends) on a topic."""

    connection_id: int
    topic: str
    active: bool                                 # friend is working on this topic right now
    seen_at: datetime | None                     # their last push on this topic, if active
    friend: OtherPerson


class TaskHistoryItem(BaseModel):
    """One past push from curr_task_history."""

    id: int
    task_summary: str
    topic: str | None
    pushed_at: datetime


class Filters(BaseModel):
    """Who I'm willing to be matched with (user_filters). Empty list = any.

    Matching is mutual: a pair only matches if both people's filters allow it.
    """

    age_groups: list[AgeGroup] = []
    countries: list[str] = Field(default=[], max_length=20)
    shared_language: bool = False

    @field_validator("countries")
    @classmethod
    def iso_countries(cls, value: list[str]) -> list[str]:
        cleaned = []
        for country in value:
            country = country.strip().upper()
            if len(country) != 2 or not country.isalpha():
                raise ValueError(f"country must be a 2-letter ISO code, got {country!r}")
            if country not in cleaned:
                cleaned.append(country)
        return cleaned
