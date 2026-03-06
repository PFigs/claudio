"""Speech-to-text via faster-whisper."""

import numpy as np
from faster_whisper import WhisperModel


class SpeechToText:
    """Wraps faster-whisper for transcribing raw i16 PCM audio."""

    def __init__(self, model_size: str = "base", device: str = "cpu", language: str | None = None):
        self._model = WhisperModel(
            model_size,
            device=device,
            compute_type="int8" if device == "cpu" else "float16",
        )
        self._language = language

    def transcribe(self, pcm_i16: bytes) -> str:
        """Transcribe raw i16 LE PCM bytes at 16kHz to text."""
        audio = np.frombuffer(pcm_i16, dtype=np.int16).astype(np.float32) / 32768.0
        segments, _info = self._model.transcribe(
            audio,
            language=self._language,
            beam_size=5,
            vad_filter=True,
        )
        return " ".join(seg.text.strip() for seg in segments)
