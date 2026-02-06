/**
 * Cross-Window Sync via BroadcastChannel API.
 * Replaces 250ms polling with instant event-driven sync.
 * Falls back to Tauri events if BroadcastChannel is unavailable.
 */

import type { QueueState } from './queueTypes';

const CHANNEL_NAME = 'vaak-queue-sync';

type SyncMessage = {
  type: 'state-update';
  state: Partial<QueueState>;
  source: string;
};

type SyncListener = (state: Partial<QueueState>) => void;

class QueueSync {
  private channel: BroadcastChannel | null = null;
  private listeners = new Set<SyncListener>();
  private sourceId: string;
  private initialized = false;

  constructor() {
    this.sourceId = `window-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
  }

  /**
   * Initialize the sync channel.
   */
  init(): void {
    if (this.initialized) return;
    this.initialized = true;

    if ('BroadcastChannel' in window) {
      try {
        this.channel = new BroadcastChannel(CHANNEL_NAME);
        this.channel.onmessage = (event: MessageEvent<SyncMessage>) => {
          // Ignore our own messages
          if (event.data.source === this.sourceId) return;
          if (event.data.type === 'state-update') {
            this.listeners.forEach(cb => cb(event.data.state));
          }
        };
        console.log('[QueueSync] BroadcastChannel initialized');
      } catch (e) {
        console.warn('[QueueSync] BroadcastChannel failed, sync disabled:', e);
      }
    } else {
      console.log('[QueueSync] BroadcastChannel not available');
    }

    // Also try Tauri events as backup
    this.initTauriSync();
  }

  private async initTauriSync(): Promise<void> {
    if (!window.__TAURI__) return;

    try {
      const { listen } = await import('@tauri-apps/api/event');
      await listen<Partial<QueueState>>('queue-state-update', (event) => {
        this.listeners.forEach(cb => cb(event.payload));
      });
      console.log('[QueueSync] Tauri event listener initialized');
    } catch (e) {
      console.warn('[QueueSync] Tauri sync failed:', e);
    }
  }

  /**
   * Broadcast state change to other windows.
   */
  broadcast(state: Partial<QueueState>): void {
    if (this.channel) {
      const msg: SyncMessage = {
        type: 'state-update',
        state,
        source: this.sourceId,
      };
      try {
        this.channel.postMessage(msg);
      } catch { /* ignore */ }
    }

    // Also emit Tauri event
    if (window.__TAURI__) {
      import('@tauri-apps/api/event').then(({ emit }) => {
        emit('queue-state-update', state).catch(() => {});
      }).catch(() => {});
    }
  }

  /**
   * Subscribe to remote state updates.
   */
  onRemoteUpdate(listener: SyncListener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  /**
   * Cleanup.
   */
  destroy(): void {
    this.channel?.close();
    this.channel = null;
    this.listeners.clear();
    this.initialized = false;
  }
}

/** Singleton instance */
export const queueSync = new QueueSync();
