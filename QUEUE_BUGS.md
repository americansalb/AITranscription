# Queue System Bug Tracker

## Status: 12 of 13 Fixed

---

## Critical Issues

### 1. Race Condition - Playback Mutex Released Too Early
- **File**: `desktop/src/lib/queueStore.ts` line 457
- **Status**: ✅ FIXED
- **Fix**: Mutex released in `oncanplay` callback (line 457), `onerror` (line 506), with 10s safety timeout (line 467) and `play()` safety net (line 547).

### 2. Missing Error Handling After DB Updates
- **File**: `desktop/src/lib/queueStore.ts` lines 350-359
- **Status**: ✅ FIXED
- **Fix**: DB update wrapped in try-catch; local state only updated after DB success. On DB failure, playback aborted and mutex released.

---

## High Issues

### 3. Pause Position Uses Wall Clock
- **File**: `desktop/src/lib/queueStore.ts` line 675
- **Status**: ✅ FIXED
- **Fix**: Uses `currentAudio.currentTime * 1000` instead of wall clock.

### 4. Resume Time Calculation Incorrect
- **File**: `desktop/src/lib/queueStore.ts` line 700
- **Status**: ✅ FIXED
- **Fix**: `resume()` calls `currentAudio.play()` directly without recalculating `audioStartTime`.

---

## Medium Issues

### 5. Duration Estimate Unreliable
- **File**: `QueueTab.tsx` NowPlayingCard component
- **Status**: ✅ FIXED
- **Problem**: Falls back to 150 words/min estimate. Progress bar jumped when real duration loaded.
- **Fix**: Smooth 500ms interpolation from estimated to real duration using `requestAnimationFrame`, preventing jarring progress bar jumps.

### 6. Drag-and-Drop Uses Stale Positions
- **File**: `QueueTab.tsx` handleMouseUp handler
- **Status**: ✅ FIXED
- **Problem**: `targetItem.position` was stale if items were reordered concurrently.
- **Fix**: Reload fresh items from DB via `queueStore.loadItems()` before computing target position in the mouseUp handler.

### 7. finalizedItems Set Memory Leak
- **File**: `queueStore.ts` lines 579-592
- **Status**: ✅ FIXED
- **Fix**: `cleanupFinalizedItems()` caps at 500 entries, deleting oldest half when exceeded. Called after every finalization.

### 8. Session Cache Never Invalidates
- **File**: `queueStore.ts` line 1078
- **Status**: ✅ FIXED
- **Fix**: 5-minute TTL on session cache entries. `enrichWithSessionInfo()` skips stale entries.

### 9. Voice Disable Doesn't Clear Pending Queue
- **File**: `App.tsx` line 266, `QueueApp.tsx` line 41
- **Status**: ✅ FIXED
- **Problem**: Disabling voice cleared pending items but currently playing item continued.
- **Fix**: Added `stopPlayback()` call before `clearPending()` in both App.tsx and QueueApp.tsx voice toggle handlers. Now disabling voice immediately stops audio and clears all pending items.

### 10. Listener Notification Unsafe During Modification
- **File**: `queueStore.ts` line 108
- **Status**: ✅ FIXED
- **Fix**: `notify()` clones the listeners Set to an array snapshot before iterating, preventing concurrent modification issues.

---

## Low / Architectural Issues

### 11. No Multi-Instance Conflict Resolution
- **Status**: 🔴 NOT FIXED (Architectural — needs design decision)

### 12. No SQLite WAL Mode
- **Status**: ✅ FIXED
- **Fix**: Added `PRAGMA journal_mode=WAL` in `database.rs` `init_database()` after opening the connection. Improves concurrent read performance.

### 13. No Text Length Validation
- **File**: `queueStore.ts` lines 374-378
- **Status**: ✅ FIXED
- **Fix**: TTS text truncated at 5000 characters with "... (truncated)" suffix before sending to API.

---

## Files Involved

### Rust (Backend/Tauri)
- `desktop/src-tauri/src/main.rs` - Speak endpoint, Tauri commands
- `desktop/src-tauri/src/database.rs` - SQLite schema and init
- `desktop/src-tauri/src/queue.rs` - All queue DB operations

### TypeScript (Frontend)
- `desktop/src/lib/queueTypes.ts` - Type definitions
- `desktop/src/lib/queueDatabase.ts` - Tauri command wrappers
- `desktop/src/lib/queueStore.ts` - Central state management (~1127 lines)
- `desktop/src/lib/queueSync.ts` - Cross-window BroadcastChannel sync
- `desktop/src/lib/speak.ts` - Speak event listener and TTS entry point
- `desktop/src/lib/messageBatcher.ts` - Message grouping with debounce
- `desktop/src/components/QueueTab.tsx` - Main queue UI (~617 lines)
- `desktop/src/components/QueueItem.tsx` - Legacy item component
- `desktop/src/components/QueueControls.tsx` - Playback controls
- `desktop/src/components/NowPlaying.tsx` - Legacy now-playing display
