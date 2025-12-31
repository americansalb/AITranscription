from fastapi import APIRouter, Depends, HTTPException, status
from pydantic import BaseModel, Field
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.api.auth import get_current_user
from app.core.database import get_db
from app.models.dictionary import DictionaryEntry
from app.models.user import User

router = APIRouter(prefix="/dictionary", tags=["dictionary"])


# Request/Response schemas
class DictionaryEntryCreate(BaseModel):
    """Request body for creating a dictionary entry."""

    word: str = Field(min_length=1, max_length=255, description="The word or phrase")
    pronunciation: str | None = Field(
        default=None, max_length=255, description="How the word might be pronounced"
    )
    description: str | None = Field(default=None, description="Description or context")
    category: str | None = Field(
        default=None, max_length=100, description="Category (e.g., 'names', 'technical')"
    )


class DictionaryEntryUpdate(BaseModel):
    """Request body for updating a dictionary entry."""

    word: str | None = Field(default=None, min_length=1, max_length=255)
    pronunciation: str | None = None
    description: str | None = None
    category: str | None = Field(default=None, max_length=100)


class DictionaryEntryResponse(BaseModel):
    """Response containing a dictionary entry."""

    id: int
    word: str
    pronunciation: str | None
    description: str | None
    category: str | None


class DictionaryListResponse(BaseModel):
    """Response containing a list of dictionary entries."""

    entries: list[DictionaryEntryResponse]
    total: int


# Routes
@router.get("", response_model=DictionaryListResponse)
async def list_dictionary_entries(
    category: str | None = None,
    current_user: User = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
):
    """List all dictionary entries for the current user."""
    query = select(DictionaryEntry).where(DictionaryEntry.user_id == current_user.id)

    if category:
        query = query.where(DictionaryEntry.category == category)

    query = query.order_by(DictionaryEntry.word)

    result = await db.execute(query)
    entries = result.scalars().all()

    return DictionaryListResponse(
        entries=[
            DictionaryEntryResponse(
                id=entry.id,
                word=entry.word,
                pronunciation=entry.pronunciation,
                description=entry.description,
                category=entry.category,
            )
            for entry in entries
        ],
        total=len(entries),
    )


@router.post("", response_model=DictionaryEntryResponse, status_code=status.HTTP_201_CREATED)
async def create_dictionary_entry(
    request: DictionaryEntryCreate,
    current_user: User = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
):
    """Create a new dictionary entry."""
    # Check if word already exists for this user
    existing = await db.execute(
        select(DictionaryEntry).where(
            DictionaryEntry.user_id == current_user.id,
            DictionaryEntry.word == request.word,
        )
    )
    if existing.scalar_one_or_none():
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=f"Word '{request.word}' already exists in your dictionary",
        )

    entry = DictionaryEntry(
        user_id=current_user.id,
        word=request.word,
        pronunciation=request.pronunciation,
        description=request.description,
        category=request.category,
    )
    db.add(entry)
    await db.commit()
    await db.refresh(entry)

    return DictionaryEntryResponse(
        id=entry.id,
        word=entry.word,
        pronunciation=entry.pronunciation,
        description=entry.description,
        category=entry.category,
    )


@router.get("/{entry_id}", response_model=DictionaryEntryResponse)
async def get_dictionary_entry(
    entry_id: int,
    current_user: User = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
):
    """Get a specific dictionary entry."""
    result = await db.execute(
        select(DictionaryEntry).where(
            DictionaryEntry.id == entry_id,
            DictionaryEntry.user_id == current_user.id,
        )
    )
    entry = result.scalar_one_or_none()

    if not entry:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Dictionary entry not found",
        )

    return DictionaryEntryResponse(
        id=entry.id,
        word=entry.word,
        pronunciation=entry.pronunciation,
        description=entry.description,
        category=entry.category,
    )


@router.put("/{entry_id}", response_model=DictionaryEntryResponse)
async def update_dictionary_entry(
    entry_id: int,
    request: DictionaryEntryUpdate,
    current_user: User = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
):
    """Update a dictionary entry."""
    result = await db.execute(
        select(DictionaryEntry).where(
            DictionaryEntry.id == entry_id,
            DictionaryEntry.user_id == current_user.id,
        )
    )
    entry = result.scalar_one_or_none()

    if not entry:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Dictionary entry not found",
        )

    # Update fields if provided
    if request.word is not None:
        # Check for duplicates if changing the word
        if request.word != entry.word:
            existing = await db.execute(
                select(DictionaryEntry).where(
                    DictionaryEntry.user_id == current_user.id,
                    DictionaryEntry.word == request.word,
                )
            )
            if existing.scalar_one_or_none():
                raise HTTPException(
                    status_code=status.HTTP_400_BAD_REQUEST,
                    detail=f"Word '{request.word}' already exists in your dictionary",
                )
        entry.word = request.word

    if request.pronunciation is not None:
        entry.pronunciation = request.pronunciation
    if request.description is not None:
        entry.description = request.description
    if request.category is not None:
        entry.category = request.category

    await db.commit()
    await db.refresh(entry)

    return DictionaryEntryResponse(
        id=entry.id,
        word=entry.word,
        pronunciation=entry.pronunciation,
        description=entry.description,
        category=entry.category,
    )


@router.delete("/{entry_id}", status_code=status.HTTP_204_NO_CONTENT)
async def delete_dictionary_entry(
    entry_id: int,
    current_user: User = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
):
    """Delete a dictionary entry."""
    result = await db.execute(
        select(DictionaryEntry).where(
            DictionaryEntry.id == entry_id,
            DictionaryEntry.user_id == current_user.id,
        )
    )
    entry = result.scalar_one_or_none()

    if not entry:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail="Dictionary entry not found",
        )

    await db.delete(entry)
    await db.commit()


@router.get("/words/list", response_model=list[str])
async def get_dictionary_words(
    current_user: User = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
):
    """Get just the words from the dictionary (for use in transcription)."""
    result = await db.execute(
        select(DictionaryEntry.word)
        .where(DictionaryEntry.user_id == current_user.id)
        .order_by(DictionaryEntry.word)
    )
    words = result.scalars().all()
    return list(words)
