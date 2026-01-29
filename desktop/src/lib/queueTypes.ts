// Queue item status
export type QueueItemStatus = 'pending' | 'playing' | 'paused' | 'completed' | 'failed';

// Queue item from the database
export interface QueueItem {
  id: number;
  uuid: string;
  sessionId: string;
  text: string;
  status: QueueItemStatus;
  position: number;
  createdAt: number;
  startedAt?: number;
  completedAt?: number;
  durationMs?: number;
  errorMessage?: string;
}

// Queue state for the UI
export interface QueueState {
  items: QueueItem[];
  currentItem: QueueItem | null;
  isPlaying: boolean;
  isPaused: boolean;
  autoPlay: boolean;
  volume: number;
  currentPosition: number; // Playback position in ms for pause/resume
}

// Payload from the speak event
export interface SpeakEventPayload {
  text: string;
  session_id: string;
  timestamp: number;
  queue_item?: QueueItem;
}

// Queue event for real-time updates
export interface QueueUpdateEvent {
  type: 'added' | 'removed' | 'updated' | 'reordered' | 'cleared';
  item?: QueueItem;
  items?: QueueItem[];
}

// Playback state event
export interface PlaybackStateEvent {
  isPlaying: boolean;
  isPaused: boolean;
  currentItem: QueueItem | null;
  positionMs: number;
  durationMs: number;
}
