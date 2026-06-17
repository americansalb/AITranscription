from app.models.base import Base
from app.models.user import User, SubscriptionTier
from app.models.dictionary import DictionaryEntry
# User.transcripts -> Transcript: import it here so the ORM registry is complete
# whenever the models package is loaded (otherwise mapper configuration fails to
# resolve the "Transcript" relationship target in contexts that don't import it).
from app.models.transcript import Transcript
from app.models.learning import (
    CorrectionEmbedding,
    AudioSample,
    LearningMetrics,
    ModelVersion,
    CorrectionRule,
)
from app.models.gamification import (
    AchievementDefinition,
    UserGamification,
    UserAchievement,
    XPTransaction,
    PrestigeTier,
    AchievementRarity,
    AchievementCategory,
)
from app.models.media_library import MediaItem, TranscriptSegment

__all__ = [
    "Base",
    "User",
    "SubscriptionTier",
    "DictionaryEntry",
    "Transcript",
    "CorrectionEmbedding",
    "AudioSample",
    "LearningMetrics",
    "ModelVersion",
    "CorrectionRule",
    "AchievementDefinition",
    "UserGamification",
    "UserAchievement",
    "XPTransaction",
    "PrestigeTier",
    "AchievementRarity",
    "AchievementCategory",
    "MediaItem",
    "TranscriptSegment",
]
