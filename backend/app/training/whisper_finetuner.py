"""Whisper LoRA fine-tuner for personalized speech recognition.

Handles:
- LoRA fine-tuning of Whisper on user's audio samples
- Model versioning and storage
- Export for inference
"""
import logging
import os
from datetime import datetime
from pathlib import Path
from typing import Optional

import torch
from datasets import Dataset, Audio
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession
from transformers import (
    WhisperProcessor,
    WhisperForConditionalGeneration,
    Seq2SeqTrainer,
    Seq2SeqTrainingArguments,
)

from app.models.learning import AudioSample, ModelVersion
from app.services.audio_collector import AudioCollector
from app.training.utils import _get_torch_device

logger = logging.getLogger(__name__)


# Configuration
MODEL_DIR = os.environ.get("MODEL_DIR", "./models")
BASE_WHISPER_MODEL = "openai/whisper-small"
MIN_SAMPLES_FOR_TRAINING = 50
BATCH_SIZE = 4
LEARNING_RATE = 1e-4
MAX_EPOCHS = 3

# Check if PEFT is available
try:
    from peft import LoraConfig, get_peft_model, TaskType

    PEFT_AVAILABLE = True
except ImportError:
    PEFT_AVAILABLE = False
    logger.warning("PEFT not available - Whisper fine-tuning disabled")


class WhisperFineTuner:
    """Handles LoRA fine-tuning of Whisper for user-specific speech recognition."""

    def __init__(self, db: AsyncSession, user_id: int):
        self.db = db
        self.user_id = user_id
        self.model_dir = Path(MODEL_DIR) / str(user_id) / "whisper"
        self.model_dir.mkdir(parents=True, exist_ok=True)
        self.device = _get_torch_device()
        self.audio_collector = AudioCollector(db, user_id)

    async def get_training_samples(self) -> Optional[list[AudioSample]]:
        """Get audio samples with corrections for training."""
        return await self.audio_collector.get_training_samples(
            min_samples=MIN_SAMPLES_FOR_TRAINING,
            unused_only=True,
        )

    async def get_latest_model_version(self) -> Optional[ModelVersion]:
        """Get the latest Whisper LoRA model version for this user."""
        result = await self.db.execute(
            select(ModelVersion)
            .where(
                ModelVersion.user_id == self.user_id,
                ModelVersion.model_type == "whisper_lora",
            )
            .order_by(ModelVersion.version.desc())
            .limit(1)
        )
        return result.scalar_one_or_none()

    def _prepare_dataset(self, samples: list[AudioSample]) -> Dataset:
        """Prepare HuggingFace dataset from audio samples."""
        data = {
            "audio": [s.audio_path for s in samples],
            "transcription": [s.corrected_transcription for s in samples],
        }

        dataset = Dataset.from_dict(data)
        dataset = dataset.cast_column("audio", Audio(sampling_rate=16000))

        return dataset

    def _create_lora_config(self) -> "LoraConfig":
        """Create LoRA configuration for Whisper."""
        return LoraConfig(
            r=8,  # Rank of the LoRA matrices
            lora_alpha=32,  # Scaling factor
            target_modules=["q_proj", "v_proj"],  # Attention modules to adapt
            lora_dropout=0.1,
            bias="none",
            task_type=TaskType.SEQ_2_SEQ_LM,
        )

    async def train(
        self,
        epochs: int = MAX_EPOCHS,
        batch_size: int = BATCH_SIZE,
        learning_rate: float = LEARNING_RATE,
    ) -> Optional[dict]:
        """Fine-tune Whisper with LoRA on user's audio samples.

        Requires GPU for training. Returns None if not enough samples.
        """
        if not PEFT_AVAILABLE:
            raise RuntimeError(
                "PEFT library not available. Install with: pip install peft"
            )

        if self.device.type == "cpu":
            logger.warning(
                "No GPU detected (CUDA or MPS). Whisper fine-tuning on CPU will be very slow."
            )

        # Get training samples
        samples = await self.get_training_samples()
        if samples is None:
            logger.info(
                f"Not enough training samples for user {self.user_id}. "
                f"Need at least {MIN_SAMPLES_FOR_TRAINING}."
            )
            return None

        logger.info(f"Starting Whisper fine-tuning with {len(samples)} samples")

        # Prepare dataset
        dataset = self._prepare_dataset(samples)

        # Load base model and processor
        processor = WhisperProcessor.from_pretrained(BASE_WHISPER_MODEL)
        model = WhisperForConditionalGeneration.from_pretrained(BASE_WHISPER_MODEL)

        # Apply LoRA
        lora_config = self._create_lora_config()
        model = get_peft_model(model, lora_config)
        model.print_trainable_parameters()

        # Preprocessing function
        def preprocess_function(batch):
            audio = batch["audio"]
            inputs = processor(
                audio["array"],
                sampling_rate=audio["sampling_rate"],
                return_tensors="pt",
            )
            labels = processor.tokenizer(
                batch["transcription"],
                return_tensors="pt",
                padding=True,
                truncation=True,
            ).input_ids
            return {
                "input_features": inputs.input_features[0],
                "labels": labels[0],
            }

        # Process dataset
        processed_dataset = dataset.map(
            preprocess_function,
            remove_columns=dataset.column_names,
        )

        # Determine new version
        latest_version = await self.get_latest_model_version()
        new_version = (latest_version.version + 1) if latest_version else 1

        # Training arguments
        output_dir = self.model_dir / f"v{new_version}"
        training_args = Seq2SeqTrainingArguments(
            output_dir=str(output_dir),
            per_device_train_batch_size=batch_size,
            learning_rate=learning_rate,
            num_train_epochs=epochs,
            evaluation_strategy="no",
            save_strategy="epoch",
            logging_steps=10,
            remove_unused_columns=False,
            fp16=self.device.type == "cuda",
            predict_with_generate=True,
        )

        # Data collator
        from transformers import DataCollatorForSeq2Seq

        data_collator = DataCollatorForSeq2Seq(
            tokenizer=processor.tokenizer,
            model=model,
            padding=True,
        )

        # Initialize trainer
        trainer = Seq2SeqTrainer(
            model=model,
            args=training_args,
            train_dataset=processed_dataset,
            data_collator=data_collator,
            tokenizer=processor.tokenizer,
        )

        # Train
        train_result = trainer.train()

        # Save LoRA weights
        model.save_pretrained(str(output_dir))

        # Calculate WER on training set (simplified)
        # In production, would use a held-out validation set
        training_loss = train_result.training_loss

        # Mark samples as used
        sample_ids = [s.id for s in samples]
        await self.audio_collector.mark_samples_as_used(sample_ids)

        # Record in database
        model_version = ModelVersion(
            user_id=self.user_id,
            model_type="whisper_lora",
            version=new_version,
            model_path=str(output_dir),
            training_samples=len(samples),
            training_loss=training_loss,
        )
        self.db.add(model_version)
        await self.db.commit()
        await self.db.refresh(model_version)

        logger.info(
            f"Whisper fine-tuning complete: version={new_version}, "
            f"loss={training_loss:.4f}, samples={len(samples)}"
        )

        return {
            "version": new_version,
            "training_loss": training_loss,
            "training_samples": len(samples),
            "epochs_trained": epochs,
            "model_path": str(output_dir),
        }


class LocalWhisperInference:
    """Local inference with fine-tuned Whisper model."""

    def __init__(self, db: AsyncSession, user_id: int):
        self.db = db
        self.user_id = user_id
        self.model_dir = Path(MODEL_DIR) / str(user_id) / "whisper"
        self.device = _get_torch_device()
        self._model = None
        self._processor = None
        self._model_version = None

    async def _load_model(self) -> bool:
        """Load the latest fine-tuned Whisper model."""
        if not PEFT_AVAILABLE:
            return False

        result = await self.db.execute(
            select(ModelVersion)
            .where(
                ModelVersion.user_id == self.user_id,
                ModelVersion.model_type == "whisper_lora",
            )
            .order_by(ModelVersion.version.desc())
            .limit(1)
        )
        model_version = result.scalar_one_or_none()

        if not model_version or not model_version.model_path:
            return False

        if not Path(model_version.model_path).exists():
            logger.warning(f"Whisper model not found: {model_version.model_path}")
            return False

        try:
            from peft import PeftModel

            # Load base model and processor
            self._processor = WhisperProcessor.from_pretrained(BASE_WHISPER_MODEL)
            base_model = WhisperForConditionalGeneration.from_pretrained(
                BASE_WHISPER_MODEL
            )

            # Load LoRA weights
            self._model = PeftModel.from_pretrained(
                base_model, model_version.model_path
            )
            self._model = self._model.to(self.device)
            self._model.eval()
            self._model_version = model_version.version

            logger.info(f"Loaded Whisper LoRA model v{model_version.version}")
            return True

        except Exception as e:
            logger.error(f"Failed to load Whisper model: {e}")
            return False

    async def has_model(self) -> bool:
        """Check if user has a fine-tuned Whisper model."""
        if self._model is not None:
            return True
        return await self._load_model()

    @torch.no_grad()
    async def transcribe(
        self,
        audio_data: bytes,
        language: Optional[str] = None,
    ) -> Optional[dict]:
        """Transcribe audio using fine-tuned Whisper model.

        Args:
            audio_data: Raw audio bytes
            language: Optional language code

        Returns:
            Dict with transcription and metadata, or None if no model available
        """
        if not await self.has_model():
            return None

        import io
        import librosa
        import numpy as np

        try:
            # Load audio from bytes
            audio_array, sample_rate = librosa.load(
                io.BytesIO(audio_data),
                sr=16000,  # Whisper expects 16kHz
            )

            # Process audio
            inputs = self._processor(
                audio_array,
                sampling_rate=16000,
                return_tensors="pt",
            )
            input_features = inputs.input_features.to(self.device)

            # Generate transcription
            forced_decoder_ids = None
            if language:
                forced_decoder_ids = self._processor.get_decoder_prompt_ids(
                    language=language, task="transcribe"
                )

            generated_ids = self._model.generate(
                input_features,
                forced_decoder_ids=forced_decoder_ids,
                max_new_tokens=225,
            )

            # Decode
            transcription = self._processor.batch_decode(
                generated_ids, skip_special_tokens=True
            )[0]

            return {
                "text": transcription.strip(),
                "model_version": self._model_version,
                "source": "local_whisper",
            }

        except Exception as e:
            logger.error(f"Local Whisper transcription failed: {e}")
            return None


class HybridTranscriptionService:
    """Hybrid transcription using local Whisper with Groq fallback."""

    def __init__(self, db: AsyncSession, user_id: int):
        self.db = db
        self.user_id = user_id
        self.local_whisper = LocalWhisperInference(db, user_id)

    async def transcribe(
        self,
        audio_data: bytes,
        filename: str = "audio.wav",
        language: Optional[str] = None,
        prefer_local: bool = True,
    ) -> dict:
        """Transcribe audio using best available method.

        Tries local fine-tuned Whisper first (if available and preferred),
        falls back to Groq API if local is unavailable or fails.
        """
        from app.services.transcription import transcription_service

        # Try local model first
        if prefer_local and await self.local_whisper.has_model():
            result = await self.local_whisper.transcribe(audio_data, language)
            if result:
                logger.debug(f"Used local Whisper v{result['model_version']}")
                return {
                    "text": result["text"],
                    "source": "local_whisper",
                    "model_version": result["model_version"],
                }

        # Fallback to Groq
        result = await transcription_service.transcribe(
            audio_data=audio_data,
            filename=filename,
            language=language,
        )

        return {
            "text": result["text"],
            "duration": result.get("duration"),
            "language": result.get("language"),
            "source": "groq",
        }
