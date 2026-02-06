// Queue item status
export type QueueItemStatus = 'pending' | 'playing' | 'paused' | 'completed' | 'failed';

// Priority level for smart queue
export type PriorityLevel = 'critical' | 'normal' | 'low';

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
  // Feature 5: Message batching
  batchCount?: number;
  // Feature 7: Per-session voice
  voiceId?: string;
  // Feature 8: Session identity
  sessionName?: string;
  sessionColor?: string;
  // Feature 4: Priority
  priority?: PriorityLevel;
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
  // Feature 3: Interrupt state
  interrupted: boolean;
  // Feature 1: Playback speed
  playbackSpeed: number;
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
