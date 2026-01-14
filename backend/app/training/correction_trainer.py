"""Incremental trainer for correction transformer model.

Handles:
- Loading/saving model checkpoints
- Incremental training on new corrections
- Model versioning
- Export to ONNX for client-side inference
"""
import logging
import os
from datetime import datetime
from pathlib import Path
from typing import Optional

import torch
import torch.nn as nn
import torch.nn.functional as F
from torch.utils.data import DataLoader
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from app.models.correction_transformer import (
    CorrectionDataset,
    CorrectionTransformer,
    create_correction_model,
)
from app.models.learning import CorrectionEmbedding, ModelVersion

logger = logging.getLogger(__name__)

# Configuration
MODEL_DIR = os.environ.get("MODEL_DIR", "./models")
MIN_SAMPLES_FOR_TRAINING = 50
BATCH_SIZE = 16
LEARNING_RATE = 1e-4
MAX_EPOCHS = 10
EARLY_STOP_PATIENCE = 3


class CorrectionTrainer:
    """Handles training of user-specific correction models."""

    def __init__(self, db: AsyncSession, user_id: int):
        self.db = db
        self.user_id = user_id
        self.model_dir = Path(MODEL_DIR) / str(user_id)
        self.model_dir.mkdir(parents=True, exist_ok=True)
        self.device = torch.device("cuda" if torch.cuda.is_available() else "cpu")

    async def get_training_data(self) -> tuple[list[str], list[str]]:
        """Fetch correction pairs from database."""
        result = await self.db.execute(
            select(CorrectionEmbedding)
            .where(CorrectionEmbedding.user_id == self.user_id)
            .order_by(CorrectionEmbedding.created_at.desc())
        )
        corrections = list(result.scalars())

        original_texts = [c.original_text for c in corrections]
        corrected_texts = [c.corrected_text for c in corrections]

        return original_texts, corrected_texts

    async def get_latest_model_version(self) -> Optional[ModelVersion]:
        """Get the latest trained model version for this user."""
        result = await self.db.execute(
            select(ModelVersion)
            .where(
                ModelVersion.user_id == self.user_id,
                ModelVersion.model_type == "correction_nn",
            )
            .order_by(ModelVersion.version.desc())
            .limit(1)
        )
        return result.scalar_one_or_none()

    def load_model(self, model_path: Optional[str] = None) -> CorrectionTransformer:
        """Load model from checkpoint or create new one."""
        model = create_correction_model()

        if model_path and Path(model_path).exists():
            try:
                checkpoint = torch.load(model_path, map_location=self.device)
                model.load_state_dict(checkpoint["model_state_dict"])
                logger.info(f"Loaded model from {model_path}")
            except Exception as e:
                logger.warning(f"Failed to load model, creating new: {e}")

        return model.to(self.device)

    def save_model(
        self,
        model: CorrectionTransformer,
        optimizer: torch.optim.Optimizer,
        epoch: int,
        loss: float,
        version: int,
    ) -> str:
        """Save model checkpoint."""
        checkpoint_path = self.model_dir / f"correction_model_v{version}.pt"

        torch.save(
            {
                "model_state_dict": model.state_dict(),
                "optimizer_state_dict": optimizer.state_dict(),
                "epoch": epoch,
                "loss": loss,
                "version": version,
            },
            checkpoint_path,
        )

        logger.info(f"Saved model checkpoint: {checkpoint_path}")
        return str(checkpoint_path)

    async def train(
        self,
        epochs: int = MAX_EPOCHS,
        batch_size: int = BATCH_SIZE,
        learning_rate: float = LEARNING_RATE,
    ) -> Optional[dict]:
        """Train or fine-tune the correction model.

        Returns training results dict or None if not enough data.
        """
        # Get training data
        original_texts, corrected_texts = await self.get_training_data()

        if len(original_texts) < MIN_SAMPLES_FOR_TRAINING:
            logger.info(
                f"Not enough training samples: {len(original_texts)}/{MIN_SAMPLES_FOR_TRAINING}"
            )
            return None

        logger.info(f"Starting training with {len(original_texts)} samples")

        # Create dataset and dataloader
        dataset = CorrectionDataset(original_texts, corrected_texts)
        dataloader = DataLoader(
            dataset,
            batch_size=batch_size,
            shuffle=True,
            num_workers=0,  # Avoid multiprocessing issues
        )

        # Load existing model or create new
        latest_version = await self.get_latest_model_version()
        if latest_version:
            model = self.load_model(latest_version.model_path)
            new_version = latest_version.version + 1
        else:
            model = self.load_model()
            new_version = 1

        # Setup training
        optimizer = torch.optim.AdamW(model.parameters(), lr=learning_rate)
        criterion = nn.CrossEntropyLoss(ignore_index=CorrectionTransformer.PAD_TOKEN)

        # Training loop
        best_loss = float("inf")
        patience_counter = 0

        for epoch in range(epochs):
            model.train()
            total_loss = 0.0
            num_batches = 0

            for batch in dataloader:
                src = batch["src"].to(self.device)
                tgt = batch["tgt"].to(self.device)
                src_mask = batch["src_mask"].to(self.device)

                optimizer.zero_grad()

                # Forward pass
                output = model(src, src_key_padding_mask=src_mask)

                # Compute loss (shift targets for next-token prediction)
                # output shape: (batch, seq_len, vocab_size)
                # tgt shape: (batch, seq_len)
                loss = criterion(
                    output[:, :-1, :].reshape(-1, model.vocab_size),
                    tgt[:, 1:].reshape(-1),
                )

                # Backward pass
                loss.backward()
                torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
                optimizer.step()

                total_loss += loss.item()
                num_batches += 1

            avg_loss = total_loss / max(num_batches, 1)
            logger.info(f"Epoch {epoch + 1}/{epochs}, Loss: {avg_loss:.4f}")

            # Early stopping
            if avg_loss < best_loss:
                best_loss = avg_loss
                patience_counter = 0
            else:
                patience_counter += 1
                if patience_counter >= EARLY_STOP_PATIENCE:
                    logger.info(f"Early stopping at epoch {epoch + 1}")
                    break

        # Save model
        model_path = self.save_model(model, optimizer, epoch + 1, best_loss, new_version)

        # Record in database
        model_version = ModelVersion(
            user_id=self.user_id,
            model_type="correction_nn",
            version=new_version,
            model_path=model_path,
            training_samples=len(original_texts),
            training_loss=best_loss,
        )
        self.db.add(model_version)
        await self.db.commit()
        await self.db.refresh(model_version)

        logger.info(
            f"Training complete: version={new_version}, loss={best_loss:.4f}, "
            f"samples={len(original_texts)}"
        )

        return {
            "version": new_version,
            "training_loss": best_loss,
            "training_samples": len(original_texts),
            "epochs_trained": epoch + 1,
            "model_path": model_path,
        }

    def export_onnx(self, model_path: str, output_path: Optional[str] = None) -> str:
        """Export model to ONNX format for client-side inference.

        Args:
            model_path: Path to PyTorch model checkpoint
            output_path: Optional output path for ONNX file

        Returns:
            Path to exported ONNX file
        """
        import onnx

        model = self.load_model(model_path)
        model.eval()

        if output_path is None:
            output_path = str(model_path).replace(".pt", ".onnx")

        # Create dummy input
        dummy_input = torch.randint(
            0, model.vocab_size, (1, 128), dtype=torch.long, device=self.device
        )

        # Export to ONNX
        torch.onnx.export(
            model,
            dummy_input,
            output_path,
            export_params=True,
            opset_version=14,
            do_constant_folding=True,
            input_names=["input"],
            output_names=["output"],
            dynamic_axes={
                "input": {0: "batch_size", 1: "sequence"},
                "output": {0: "batch_size", 1: "sequence"},
            },
        )

        # Verify the exported model
        onnx_model = onnx.load(output_path)
        onnx.checker.check_model(onnx_model)

        logger.info(f"Exported ONNX model to {output_path}")
        return output_path


class MLCorrector:
    """Service for applying ML-based corrections."""

    def __init__(self, db: AsyncSession, user_id: int):
        self.db = db
        self.user_id = user_id
        self.model_dir = Path(MODEL_DIR) / str(user_id)
        self.device = torch.device("cuda" if torch.cuda.is_available() else "cpu")
        self._model: Optional[CorrectionTransformer] = None
        self._model_version: Optional[int] = None

    async def _load_model(self) -> Optional[CorrectionTransformer]:
        """Load the latest trained model for this user."""
        result = await self.db.execute(
            select(ModelVersion)
            .where(
                ModelVersion.user_id == self.user_id,
                ModelVersion.model_type == "correction_nn",
            )
            .order_by(ModelVersion.version.desc())
            .limit(1)
        )
        model_version = result.scalar_one_or_none()

        if not model_version or not model_version.model_path:
            return None

        if not Path(model_version.model_path).exists():
            logger.warning(f"Model file not found: {model_version.model_path}")
            return None

        model = create_correction_model()
        checkpoint = torch.load(model_version.model_path, map_location=self.device)
        model.load_state_dict(checkpoint["model_state_dict"])
        model = model.to(self.device)
        model.eval()

        self._model_version = model_version.version
        logger.info(f"Loaded correction model v{model_version.version}")

        return model

    async def get_model(self) -> Optional[CorrectionTransformer]:
        """Get cached model or load from disk."""
        if self._model is None:
            self._model = await self._load_model()
        return self._model

    async def correct(
        self,
        text: str,
        confidence_threshold: float = 0.7,
    ) -> Optional[dict]:
        """Apply ML correction to text.

        Args:
            text: Input text to correct
            confidence_threshold: Minimum confidence to return correction

        Returns:
            Dict with corrected text and metadata, or None if no model/low confidence
        """
        model = await self.get_model()
        if model is None:
            return None

        corrected, confidence = model.correct(text)

        if confidence < confidence_threshold:
            logger.debug(f"ML correction confidence too low: {confidence:.2f}")
            return None

        # Only return if actually different
        if corrected.strip() == text.strip():
            return None

        return {
            "original": text,
            "corrected": corrected,
            "confidence": confidence,
            "model_version": self._model_version,
            "source": "correction_nn",
        }

    async def has_trained_model(self) -> bool:
        """Check if user has a trained correction model."""
        model = await self.get_model()
        return model is not None
