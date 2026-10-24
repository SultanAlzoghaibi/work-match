from dataclasses import dataclass, field


@dataclass(frozen=True)
class Person:
    """One online user: profile plus their latest summary."""

    user_id: str
    name: str
    summary: str
    topic: str
    subtopics: list[str] = field(default_factory=list)
    context: str | None = None
    activity_type: str | None = None
    focus: str = "medium"
    stuck: bool = False
    school: str | None = None
    bio: str | None = None
    field: str | None = None

    @classmethod
    def from_row(cls, row: dict) -> "Person":
        return cls(
            user_id=row["user_id"],
            name=row.get("name") or "Someone",
            summary=row["summary"],
            topic=row["topic"],
            subtopics=list(row.get("subtopics") or []),
            context=row.get("context"),
            activity_type=row.get("activity_type"),
            focus=row.get("focus") or "medium",
            stuck=bool(row.get("stuck")),
            school=row.get("school"),
            bio=row.get("bio"),
            field=row.get("field"),
        )

    @classmethod
    def from_task(cls, row: dict) -> "Person":
        """From a curr_task row joined with users (backend/schema.sql)."""
        return cls(
            user_id=str(row["user_id"]),
            name=row.get("name") or "Someone",
            summary=row["task_summary"],
            topic=row.get("topic") or "",
            school=row.get("school"),
            bio=row.get("bio"),
        )

    def describe(self) -> str:
        """Text sent to the decision model. Leaves out the name on purpose."""
        parts = []
        if self.school or self.field:
            parts.append(", ".join(p for p in (self.field, self.school) if p) + ".")
        if self.bio:
            parts.append(f"Bio: {self.bio}")
        state = f"{self.focus} focus" + (", stuck" if self.stuck else "")
        parts.append(f"Right now ({state}): {self.summary}")
        parts.append(f"Topic: {self.topic}.")
        if self.subtopics:
            parts.append(f"Subtopics: {', '.join(self.subtopics)}.")
        if self.context:
            parts.append(f"Context: {self.context}.")
        if self.activity_type:
            parts.append(f"Activity: {self.activity_type}.")
        return " ".join(parts)
