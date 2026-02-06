# Queue System Bug Tracker

## Status: In Progress

---

## Critical Issues

### 1. Race Condition - Playback Mutex Released Too Early
- **File**: `desktop/src/lib/queueStore.ts` around line 269
- **Status**: 🔴 NOT FIXED
- **Problem**: The `isStartingPlayback` mutex is released BEFORE the async TTS API call (`fetch` to `http://127.0.0.1:19836/api/v1/tts`) completes. This means `playNext()` can be called again while the first TTS request is still in flight, causing two items to play simultaneously.
- **Flow**: update DB → update state → release mutex → call TTS API (too late!)
- **Fix**: Move `isStartingPlayback = false` into the audio event callbacks (`oncanplay`/`onerror`) or after the fetch response is received and audio element is created. Only release the mutex once we know playback has actually started or definitively failed.

### 2. Missing Error Handling After DB Updates
- **File**: `desktop/src/lib/queueStore.ts` lines 253-267
- **Status**: 🔴 NOT FIXED
- **Problem**: After calling `db.updateQueueItemStatus(uuid, "playing")`, local state is updated regardless of whether the DB call succeeded. If DB throws, UI shows "playing" but DB still has "pending", causing divergence.
- **Fix**: Wrap DB calls in try-catch and only update local state on success. On failure, revert or skip.

---

## High Issues

### 3. Pause Position Uses Wall Clock
- **File**: `desktop/src/lib/queueStore.ts` line 488
- **Status**: 🔴 NOT FIXED
- **Problem**: `const position = (Date.now() - audioStartTime)` uses wall clock instead of `currentAudio.currentTime * 1000`. Inaccurate if audio start was delayed.
- **Fix**: Use `currentAudio.currentTime * 1000`

### 4. Resume Time Calculation Incorrect
- **File**: `desktop/src/lib/queueStore.ts` line 501
- **Status**: 🔴 NOT FIXED
- **Problem**: `audioStartTime = Date.now() - state.currentPosition` recalculates wall clock offset. Breaks after long pauses or with non-1.0 playback speed.
- **Fix**: Use `currentAudio.currentTime` directly, don't recalculate audioStartTime.

---

## Medium Issues

### 5. Duration Estimate Unreliable
- **File**: `queueStore.ts` line 90, `QueueTab.tsx` line 90
- **Status**: 🔴 NOT FIXED
- **Problem**: Falls back to 150 words/min estimate. Progress bar jumps when real duration loads.

### 6. Drag-and-Drop Uses Stale Positions
- **File**: `QueueTab.tsx` lines 425-434
- **Status**: 🔴 NOT FIXED
- **Problem**: `targetItem.position` may be stale if items were reordered concurrently.

### 7. finalizedItems Set Memory Leak
- **File**: `queueStore.ts` lines 415-426
- **Status**: 🔴 NOT FIXED
- **Problem**: `finalizedItems` Set grows indefinitely. Never cleaned up.

### 8. Session Cache Never Invalidates
- **File**: `queueStore.ts` lines 752-758
- **Status**: 🔴 NOT FIXED
- **Problem**: No mechanism to update stale session name/color/voiceId.

### 9. Voice Disable Doesn't Clear Pending Queue
- **File**: `speak.ts` lines 237-241
- **Status**: 🔴 NOT FIXED
- **Problem**: Disabling voice drops new messages but existing queued items still play.

### 10. Listener Notification Unsafe During Modification
- **File**: `queueStore.ts` lines 66-70
- **Status**: 🔴 NOT FIXED
- **Problem**: Iterating `listeners` Set while a listener might modify it.

---

## Low / Architectural Issues

### 11. No Multi-Instance Conflict Resolution
### 12. No SQLite WAL Mode
### 13. No Text Length Validation

---

## Files Involved

### Rust (Backend/Tauri)
- `desktop/src-tauri/src/main.rs` - Speak endpoint, Tauri commands
- `desktop/src-tauri/src/database.rs` - SQLite schema and init
- `desktop/src-tauri/src/queue.rs` - All queue DB operations

### TypeScript (Frontend)
- `desktop/src/lib/queueTypes.ts` - Type definitions
- `desktop/src/lib/queueDatabase.ts` - Tauri command wrappers
- `desktop/src/lib/queueStore.ts` - Central state management (775 lines)
- `desktop/src/lib/queueSync.ts` - Cross-window BroadcastChannel sync
- `desktop/src/lib/speak.ts` - Speak event listener and TTS entry point
- `desktop/src/lib/messageBatcher.ts` - Message grouping with debounce
- `desktop/src/components/QueueTab.tsx` - Main queue UI (563 lines)
- `desktop/src/components/QueueItem.tsx` - Legacy item component
- `desktop/src/components/QueueControls.tsx` - Playback controls
- `desktop/src/components/NowPlaying.tsx` - Legacy now-playing display
