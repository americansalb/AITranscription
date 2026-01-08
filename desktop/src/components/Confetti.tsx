import { useEffect, useState } from "react";

interface ConfettiPiece {
  id: number;
  x: number;
  y: number;
  rotation: number;
  color: string;
  scale: number;
  velocityX: number;
  velocityY: number;
}

interface ConfettiProps {
  isActive: boolean;
  onComplete?: () => void;
}

const COLORS = ["#6366f1", "#22c55e", "#f59e0b", "#ef4444", "#ec4899", "#8b5cf6", "#06b6d4"];
const PIECE_COUNT = 50;

export function Confetti({ isActive, onComplete }: ConfettiProps) {
  const [pieces, setPieces] = useState<ConfettiPiece[]>([]);

  useEffect(() => {
    if (!isActive) {
      setPieces([]);
      return;
    }

    // Generate confetti pieces
    const newPieces: ConfettiPiece[] = Array.from({ length: PIECE_COUNT }, (_, i) => ({
      id: i,
      x: Math.random() * 100,
      y: -10 - Math.random() * 20,
      rotation: Math.random() * 360,
      color: COLORS[Math.floor(Math.random() * COLORS.length)],
      scale: 0.5 + Math.random() * 0.5,
      velocityX: (Math.random() - 0.5) * 3,
      velocityY: 2 + Math.random() * 3,
    }));

    setPieces(newPieces);

    // Clean up after animation
    const timeout = setTimeout(() => {
      setPieces([]);
      onComplete?.();
    }, 3000);

    return () => clearTimeout(timeout);
  }, [isActive, onComplete]);

  if (pieces.length === 0) return null;

  return (
    <div className="confetti-container">
      {pieces.map((piece) => (
        <div
          key={piece.id}
          className="confetti-piece"
          style={{
            left: `${piece.x}%`,
            backgroundColor: piece.color,
            transform: `rotate(${piece.rotation}deg) scale(${piece.scale})`,
            animationDelay: `${Math.random() * 0.5}s`,
            // @ts-ignore - CSS custom properties
            "--velocity-x": piece.velocityX,
            "--velocity-y": piece.velocityY,
          }}
        />
      ))}
    </div>
  );
}
