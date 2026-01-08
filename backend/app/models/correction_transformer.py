"""Character-level Transformer for text correction.

A small (~2M parameters) model that learns user-specific correction patterns.
Designed to handle:
- Spelling corrections
- Common transcription errors
- User vocabulary preferences
"""
import math
from typing import Optional

import torch
import torch.nn as nn
import torch.nn.functional as F


class PositionalEncoding(nn.Module):
    """Sinusoidal positional encoding for transformer."""

    def __init__(self, d_model: int, max_len: int = 512, dropout: float = 0.1):
        super().__init__()
        self.dropout = nn.Dropout(p=dropout)

        # Create positional encoding matrix
        pe = torch.zeros(max_len, d_model)
        position = torch.arange(0, max_len, dtype=torch.float).unsqueeze(1)
        div_term = torch.exp(
            torch.arange(0, d_model, 2).float() * (-math.log(10000.0) / d_model)
        )

        pe[:, 0::2] = torch.sin(position * div_term)
        pe[:, 1::2] = torch.cos(position * div_term)
        pe = pe.unsqueeze(0)  # Shape: (1, max_len, d_model)

        self.register_buffer("pe", pe)

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        """Add positional encoding to input tensor."""
        x = x + self.pe[:, : x.size(1), :]
        return self.dropout(x)


class CorrectionTransformer(nn.Module):
    """Character-level Transformer for text correction.

    Architecture:
    - Character embedding (vocab_size -> d_model)
    - Positional encoding
    - N Transformer encoder layers
    - Output projection (d_model -> vocab_size)

    ~2M parameters with default settings.
    """

    # Character vocabulary (ASCII printable + special tokens)
    PAD_TOKEN = 0
    UNK_TOKEN = 1
    START_TOKEN = 2
    END_TOKEN = 3
    VOCAB_OFFSET = 4  # First regular char index

    def __init__(
        self,
        vocab_size: int = 256,  # Extended ASCII
        d_model: int = 256,
        nhead: int = 4,
        num_layers: int = 4,
        dim_feedforward: int = 512,
        dropout: float = 0.1,
        max_len: int = 512,
    ):
        super().__init__()
        self.d_model = d_model
        self.vocab_size = vocab_size
        self.max_len = max_len

        # Embedding layers
        self.embedding = nn.Embedding(vocab_size, d_model, padding_idx=self.PAD_TOKEN)
        self.pos_encoder = PositionalEncoding(d_model, max_len, dropout)

        # Transformer encoder
        encoder_layer = nn.TransformerEncoderLayer(
            d_model=d_model,
            nhead=nhead,
            dim_feedforward=dim_feedforward,
            dropout=dropout,
            batch_first=True,
        )
        self.transformer = nn.TransformerEncoder(encoder_layer, num_layers=num_layers)

        # Output projection
        self.output_proj = nn.Linear(d_model, vocab_size)

        # Initialize weights
        self._init_weights()

    def _init_weights(self):
        """Initialize weights with Xavier uniform."""
        for p in self.parameters():
            if p.dim() > 1:
                nn.init.xavier_uniform_(p)

    def forward(
        self,
        src: torch.Tensor,
        src_mask: Optional[torch.Tensor] = None,
        src_key_padding_mask: Optional[torch.Tensor] = None,
    ) -> torch.Tensor:
        """Forward pass.

        Args:
            src: Input character indices, shape (batch, seq_len)
            src_mask: Attention mask
            src_key_padding_mask: Padding mask, shape (batch, seq_len)

        Returns:
            Output logits, shape (batch, seq_len, vocab_size)
        """
        # Embed and add positional encoding
        x = self.embedding(src) * math.sqrt(self.d_model)
        x = self.pos_encoder(x)

        # Apply transformer
        x = self.transformer(x, mask=src_mask, src_key_padding_mask=src_key_padding_mask)

        # Project to vocabulary
        return self.output_proj(x)

    def encode_text(self, text: str) -> torch.Tensor:
        """Convert text string to tensor of character indices."""
        indices = [self.START_TOKEN]
        for char in text[: self.max_len - 2]:  # Leave room for START and END
            idx = ord(char)
            if idx < self.vocab_size - self.VOCAB_OFFSET:
                indices.append(idx + self.VOCAB_OFFSET)
            else:
                indices.append(self.UNK_TOKEN)
        indices.append(self.END_TOKEN)
        return torch.tensor(indices, dtype=torch.long)

    def decode_tensor(self, tensor: torch.Tensor) -> str:
        """Convert tensor of character indices back to text."""
        chars = []
        for idx in tensor.tolist():
            if idx == self.END_TOKEN:
                break
            if idx >= self.VOCAB_OFFSET:
                char_code = idx - self.VOCAB_OFFSET
                if char_code < 128:  # Valid ASCII
                    chars.append(chr(char_code))
        return "".join(chars)

    @torch.no_grad()
    def correct(
        self,
        text: str,
        temperature: float = 0.7,
        top_k: int = 10,
    ) -> tuple[str, float]:
        """Correct input text using the model.

        Args:
            text: Input text to correct
            temperature: Sampling temperature (lower = more conservative)
            top_k: Number of top candidates to consider

        Returns:
            Tuple of (corrected_text, confidence_score)
        """
        self.eval()
        device = next(self.parameters()).device

        # Encode input
        src = self.encode_text(text).unsqueeze(0).to(device)

        # Get predictions
        logits = self.forward(src)

        # Apply temperature and get probabilities
        probs = F.softmax(logits / temperature, dim=-1)

        # Get top-k predictions for each position
        top_probs, top_indices = torch.topk(probs, top_k, dim=-1)

        # Build corrected output using most likely characters
        output_indices = top_indices[0, :, 0]  # Batch 0, top-1 prediction
        confidence = top_probs[0, :, 0].mean().item()

        # Decode output
        corrected = self.decode_tensor(output_indices)

        return corrected, confidence

    def get_parameter_count(self) -> int:
        """Return total number of trainable parameters."""
        return sum(p.numel() for p in self.parameters() if p.requires_grad)


class CorrectionDataset(torch.utils.data.Dataset):
    """Dataset for training correction transformer."""

    def __init__(
        self,
        original_texts: list[str],
        corrected_texts: list[str],
        max_len: int = 512,
    ):
        assert len(original_texts) == len(corrected_texts)
        self.original_texts = original_texts
        self.corrected_texts = corrected_texts
        self.max_len = max_len
        self.model = CorrectionTransformer()  # For encoding

    def __len__(self) -> int:
        return len(self.original_texts)

    def __getitem__(self, idx: int) -> dict:
        src = self.model.encode_text(self.original_texts[idx])
        tgt = self.model.encode_text(self.corrected_texts[idx])

        # Pad to max_len
        src_padded = F.pad(
            src, (0, self.max_len - len(src)), value=CorrectionTransformer.PAD_TOKEN
        )
        tgt_padded = F.pad(
            tgt, (0, self.max_len - len(tgt)), value=CorrectionTransformer.PAD_TOKEN
        )

        # Create padding mask
        src_mask = src_padded == CorrectionTransformer.PAD_TOKEN

        return {
            "src": src_padded,
            "tgt": tgt_padded,
            "src_mask": src_mask,
        }


def create_correction_model() -> CorrectionTransformer:
    """Create a correction transformer with default settings.

    Model size: ~2M parameters
    """
    model = CorrectionTransformer(
        vocab_size=256,
        d_model=256,
        nhead=4,
        num_layers=4,
        dim_feedforward=512,
        dropout=0.1,
        max_len=512,
    )
    return model


# Quick model size verification
if __name__ == "__main__":
    model = create_correction_model()
    param_count = model.get_parameter_count()
    print(f"Model parameters: {param_count:,}")
    print(f"Model size (MB): {param_count * 4 / 1024 / 1024:.2f}")  # 4 bytes per float32
