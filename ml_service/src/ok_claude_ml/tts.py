"""Text-to-speech via Piper TTS."""

import io
import struct
import wave

from piper import PiperVoice


class TextToSpeech:
    """Wraps Piper TTS for synthesizing text to raw i16 PCM audio."""

    def __init__(self, model_path: str | None = None):
        if model_path is None:
            model_path = self._default_model_path()
        self._voice = PiperVoice.load(model_path)
        self._sample_rate = self._voice.config.sample_rate

    def synthesize(self, text: str) -> bytes:
        """Synthesize text to raw bytes: 4-byte sample_rate (u32 LE) + i16 LE PCM.

        This format matches the AUDIO_OUT response body in the binary protocol.
        """
        pcm_buf = io.BytesIO()
        with wave.open(pcm_buf, "wb") as wav:
            self._voice.synthesize(text, wav)

        # Extract raw PCM from WAV (skip header)
        pcm_buf.seek(0)
        with wave.open(pcm_buf, "rb") as wav:
            raw_pcm = wav.readframes(wav.getnframes())

        return struct.pack("<I", self._sample_rate) + raw_pcm

    @property
    def sample_rate(self) -> int:
        return self._sample_rate

    @staticmethod
    def _default_model_path() -> str:
        import subprocess
        import shutil

        # Try to find a piper model in common locations
        for path in [
            "/usr/share/piper-voices",
            "~/.local/share/piper/voices",
        ]:
            import os

            expanded = os.path.expanduser(path)
            if os.path.isdir(expanded):
                for root, _dirs, files in os.walk(expanded):
                    for f in files:
                        if f.endswith(".onnx"):
                            return os.path.join(root, f)

        raise FileNotFoundError(
            "No Piper TTS model found. Download one from "
            "https://github.com/rhasspy/piper/blob/master/VOICES.md "
            "and set tts.model in ~/.config/ok_claude/config.toml"
        )
