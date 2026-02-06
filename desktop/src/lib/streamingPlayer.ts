/**
 * Streaming audio player using MediaSource Extensions.
 * Starts playback as soon as the first chunk arrives (~200ms)
 * instead of waiting for the full download (2-4s).
 */

type StreamingPlayerState = 'idle' | 'buffering' | 'playing' | 'ended' | 'error';

export class StreamingPlayer {
  private audio: HTMLAudioElement | null = null;
  private mediaSource: MediaSource | null = null;
  private sourceBuffer: SourceBuffer | null = null;
  private pendingChunks: Uint8Array[] = [];
  private state: StreamingPlayerState = 'idle';
  private streamDone = false;
  private abortController: AbortController | null = null;

  onStateChange?: (state: StreamingPlayerState) => void;
  onEnded?: () => void;
  onError?: (error: string) => void;
  onTimeUpdate?: (currentTime: number, duration: number) => void;

  get currentAudio(): HTMLAudioElement | null {
    return this.audio;
  }

  get isPlaying(): boolean {
    return this.state === 'playing' || this.state === 'buffering';
  }

  /**
   * Start streaming TTS audio from the backend.
   */
  async play(text: string, sessionId: string, apiUrl: string, volume: number, voiceId?: string): Promise<void> {
    this.stop();
    this.state = 'buffering';
    this.streamDone = false;
    this.pendingChunks = [];
    this.onStateChange?.('buffering');

    // Check if MediaSource is supported
    if (!('MediaSource' in window) || !MediaSource.isTypeSupported('audio/mpeg')) {
      console.log('[StreamingPlayer] MediaSource not supported, falling back to blob playback');
      return this.fallbackPlay(text, sessionId, apiUrl, volume, voiceId);
    }

    this.abortController = new AbortController();
    this.mediaSource = new MediaSource();
    this.audio = new Audio();
    this.audio.volume = volume;

    const objectUrl = URL.createObjectURL(this.mediaSource);
    this.audio.src = objectUrl;

    this.audio.ontimeupdate = () => {
      if (this.audio) {
        const duration = isNaN(this.audio.duration) ? 0 : this.audio.duration;
        this.onTimeUpdate?.(this.audio.currentTime, duration);
      }
    };

    this.audio.onended = () => {
      this.state = 'ended';
      this.onStateChange?.('ended');
      this.onEnded?.();
      URL.revokeObjectURL(objectUrl);
    };

    this.audio.onerror = () => {
      const msg = this.audio?.error?.message || 'Playback error';
      this.state = 'error';
      this.onStateChange?.('error');
      this.onError?.(msg);
      URL.revokeObjectURL(objectUrl);
    };

    // Wait for MediaSource to open
    await new Promise<void>((resolve) => {
      this.mediaSource!.addEventListener('sourceopen', () => resolve(), { once: true });
    });

    try {
      this.sourceBuffer = this.mediaSource.addSourceBuffer('audio/mpeg');
    } catch (e) {
      console.warn('[StreamingPlayer] Failed to add source buffer, using fallback');
      return this.fallbackPlay(text, sessionId, apiUrl, volume, voiceId);
    }

    this.sourceBuffer.addEventListener('updateend', () => {
      this.processPendingChunks();
    });

    // Start streaming fetch
    try {
      const formData = new FormData();
      formData.append('text', text);
      formData.append('session_id', sessionId);
      if (voiceId) {
        formData.append('voice_id', voiceId);
      }

      const response = await fetch(`${apiUrl}/api/v1/tts/stream`, {
        method: 'POST',
        body: formData,
        signal: this.abortController.signal,
      });

      if (!response.ok || !response.body) {
        // Fallback to non-streaming endpoint
        console.log('[StreamingPlayer] Stream endpoint unavailable, using fallback');
        return this.fallbackPlay(text, sessionId, apiUrl, volume, voiceId);
      }

      const reader = response.body.getReader();
      let firstChunk = true;

      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        if (value && value.length > 0) {
          this.pendingChunks.push(value);
          this.processPendingChunks();

          // Start playback on first chunk
          if (firstChunk) {
            firstChunk = false;
            try {
              await this.audio!.play();
              this.state = 'playing';
              this.onStateChange?.('playing');
            } catch (e) {
              console.warn('[StreamingPlayer] Play failed:', e);
            }
          }
        }
      }

      this.streamDone = true;
      this.processPendingChunks();

    } catch (e: any) {
      if (e.name === 'AbortError') return;
      console.error('[StreamingPlayer] Stream error:', e);
      // Try fallback
      return this.fallbackPlay(text, sessionId, apiUrl, volume, voiceId);
    }
  }

  private processPendingChunks(): void {
    if (!this.sourceBuffer || this.sourceBuffer.updating || this.pendingChunks.length === 0) {
      // If stream is done and no more chunks, end the stream
      if (this.streamDone && this.pendingChunks.length === 0 && this.mediaSource?.readyState === 'open') {
        try {
          this.mediaSource.endOfStream();
        } catch { /* ignore */ }
      }
      return;
    }

    const chunk = this.pendingChunks.shift()!;
    try {
      this.sourceBuffer.appendBuffer(new Uint8Array(chunk).buffer as ArrayBuffer);
    } catch (e) {
      console.warn('[StreamingPlayer] appendBuffer failed:', e);
    }
  }

  /** Fallback: download entire blob then play */
  private async fallbackPlay(text: string, sessionId: string, apiUrl: string, volume: number, voiceId?: string): Promise<void> {
    this.stop();

    const formData = new FormData();
    formData.append('text', text);
    formData.append('session_id', sessionId);
    if (voiceId) {
      formData.append('voice_id', voiceId);
    }

    const response = await fetch(`${apiUrl}/api/v1/tts`, {
      method: 'POST',
      body: formData,
    });

    if (!response.ok) {
      throw new Error(`TTS API failed (${response.status})`);
    }

    const blob = await response.blob();
    const url = URL.createObjectURL(blob);

    this.audio = new Audio(url);
    this.audio.volume = volume;

    this.audio.ontimeupdate = () => {
      if (this.audio) {
        const duration = isNaN(this.audio.duration) ? 0 : this.audio.duration;
        this.onTimeUpdate?.(this.audio.currentTime, duration);
      }
    };

    this.audio.onended = () => {
      this.state = 'ended';
      this.onStateChange?.('ended');
      this.onEnded?.();
      URL.revokeObjectURL(url);
    };

    this.audio.onerror = () => {
      this.state = 'error';
      this.onStateChange?.('error');
      this.onError?.(this.audio?.error?.message || 'Playback error');
      URL.revokeObjectURL(url);
    };

    await this.audio.play();
    this.state = 'playing';
    this.onStateChange?.('playing');
  }

  pause(): void {
    this.audio?.pause();
  }

  resume(): void {
    this.audio?.play();
  }

  stop(): void {
    this.abortController?.abort();
    this.abortController = null;
    if (this.audio) {
      this.audio.pause();
      this.audio.src = '';
      this.audio = null;
    }
    if (this.mediaSource?.readyState === 'open') {
      try { this.mediaSource.endOfStream(); } catch { /* ignore */ }
    }
    this.mediaSource = null;
    this.sourceBuffer = null;
    this.pendingChunks = [];
    this.state = 'idle';
    this.streamDone = false;
  }

  setVolume(vol: number): void {
    if (this.audio) this.audio.volume = vol;
  }

  setPlaybackRate(rate: number): void {
    if (this.audio) this.audio.playbackRate = rate;
  }
}
