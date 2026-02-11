//! Sound engine — audio playback via rodio
//!
//! Plays Director 6 sounds (decoded via DecodedSound → WAV → rodio).
//! Supports one-shot playback, looping background music, named sound lookup,
//! and playback handles for cue-point based dialog synchronization.

use std::io::Cursor;
use std::sync::Arc;
use std::time::Instant;

use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};

use crate::assets::sound::DecodedSound;
use crate::assets::AssetStore;

/// A handle to a playing sound — tracks elapsed time for cue-point polling
#[derive(Debug)]
pub struct PlaybackHandle {
    /// When playback started
    start_time: Instant,
    /// Index into sfx_sinks (for checking if still playing)
    #[allow(dead_code)] // Used when checking playback status
    sink_index: usize,
}

impl PlaybackHandle {
    /// Elapsed time in milliseconds since playback started
    pub fn elapsed_ms(&self) -> u32 {
        self.start_time.elapsed().as_millis() as u32
    }
}

/// Central sound engine — manages output stream and active playback channels
pub struct SoundEngine {
    /// rodio output stream (must be kept alive)
    _stream: OutputStream,
    /// Handle for creating new sinks
    handle: OutputStreamHandle,
    /// Background music / ambient loop
    bg_sink: Option<Sink>,
    /// One-shot sound effects (kept alive until finished)
    sfx_sinks: Vec<Sink>,
    /// Current background sound name (to avoid restarting same track)
    current_bg: String,
    /// Master volume (0.0 – 1.0)
    volume: f32,
}

impl SoundEngine {
    /// Create a new sound engine. Returns None if audio device unavailable.
    pub fn new() -> Option<Self> {
        match OutputStream::try_default() {
            Ok((stream, handle)) => {
                tracing::info!("Audio output initialized");
                Some(Self {
                    _stream: stream,
                    handle,
                    bg_sink: None,
                    sfx_sinks: Vec::new(),
                    current_bg: String::new(),
                    volume: 1.0,
                })
            }
            Err(e) => {
                tracing::warn!("Failed to initialize audio: {}", e);
                None
            }
        }
    }

    /// Play a one-shot sound effect from a DecodedSound.
    /// Returns a PlaybackHandle for tracking elapsed time (used by cue-point system).
    pub fn play_sound(&mut self, sound: &DecodedSound) -> Option<PlaybackHandle> {
        let wav_bytes = sound.to_wav();
        match Decoder::new(Cursor::new(wav_bytes)) {
            Ok(source) => {
                match Sink::try_new(&self.handle) {
                    Ok(sink) => {
                        sink.set_volume(self.volume);
                        sink.append(source);
                        let index = self.sfx_sinks.len();
                        self.sfx_sinks.push(sink);
                        Some(PlaybackHandle {
                            start_time: Instant::now(),
                            sink_index: index,
                        })
                    }
                    Err(e) => {
                        tracing::warn!("Failed to create SFX sink: {}", e);
                        None
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to decode WAV for playback: {}", e);
                None
            }
        }
    }

    /// Play a sound by its Director cast member name (e.g. "10e001v0").
    /// Searches all loaded files for the named sound.
    /// Returns a PlaybackHandle for cue-point tracking.
    pub fn play_by_name(&mut self, name: &str, assets: &AssetStore) -> Option<PlaybackHandle> {
        if let Some((file, num)) = assets.find_sound_by_name(name) {
            if let Some(decoded) = assets.decode_sound(&file, num) {
                tracing::debug!("Playing sound '{}' from {}#{}", name, file, num);
                return self.play_sound(&decoded);
            } else {
                tracing::warn!("Sound '{}' found at {}#{} but failed to decode", name, file, num);
            }
        } else {
            tracing::debug!("Sound '{}' not found in any file", name);
        }
        None
    }

    /// Start a looping background sound. If the same name is already playing,
    /// this is a no-op. Pass "" to stop background audio.
    pub fn play_background(&mut self, name: &str, assets: &AssetStore) {
        if name == self.current_bg {
            return; // Already playing
        }

        // Stop current background
        self.stop_background();

        if name.is_empty() {
            return;
        }

        if let Some((file, num)) = assets.find_sound_by_name(name) {
            if let Some(decoded) = assets.decode_sound(&file, num) {
                let wav_bytes = decoded.to_wav();
                // Create a buffered source that we can loop
                let wav_arc = Arc::new(wav_bytes);
                match Sink::try_new(&self.handle) {
                    Ok(sink) => {
                        sink.set_volume(self.volume * 0.6); // BG slightly quieter
                        // Append looping source
                        match Decoder::new(Cursor::new((*wav_arc).clone())) {
                            Ok(source) => {
                                sink.append(source.repeat_infinite());
                                self.bg_sink = Some(sink);
                                self.current_bg = name.to_string();
                                tracing::debug!("Background loop: '{}' from {}#{}", name, file, num);
                            }
                            Err(e) => tracing::warn!("Failed to decode BG sound '{}': {}", name, e),
                        }
                    }
                    Err(e) => tracing::warn!("Failed to create BG sink: {}", e),
                }
            }
        } else {
            tracing::debug!("Background sound '{}' not found", name);
        }
    }

    /// Stop the background loop
    pub fn stop_background(&mut self) {
        if let Some(sink) = self.bg_sink.take() {
            sink.stop();
        }
        self.current_bg.clear();
    }

    /// Stop all sounds (background + SFX)
    pub fn stop_all(&mut self) {
        self.stop_background();
        for sink in self.sfx_sinks.drain(..) {
            sink.stop();
        }
    }

    /// Set master volume (0.0 – 1.0)
    pub fn set_volume(&mut self, vol: f32) {
        self.volume = vol.clamp(0.0, 1.0);
        if let Some(bg) = &self.bg_sink {
            bg.set_volume(self.volume * 0.6);
        }
    }

    /// Check if a playback handle's sound is still playing
    #[allow(dead_code)] // Available for future audio monitoring
    pub fn is_handle_playing(&self, handle: &PlaybackHandle) -> bool {
        if let Some(sink) = self.sfx_sinks.get(handle.sink_index) {
            !sink.empty()
        } else {
            false
        }
    }

    /// Clean up finished SFX sinks (called periodically from game loop).
    /// NOTE: After gc(), existing PlaybackHandle sink_index values may be
    /// invalidated. Only call gc() when no active handles are being tracked.
    pub fn gc(&mut self) {
        self.sfx_sinks.retain(|s| !s.empty());
    }
}
