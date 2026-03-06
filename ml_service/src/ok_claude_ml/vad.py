"""Voice activity detection via Silero-VAD."""

import numpy as np
import torch


class VoiceActivityDetector:
    """Wraps Silero-VAD for speech boundary detection on 16kHz i16 PCM chunks."""

    def __init__(
        self,
        threshold: float = 0.5,
        min_silence_ms: int = 300,
        speech_pad_ms: int = 30,
    ):
        self._model, utils = torch.hub.load(
            "snakers4/silero-vad", "silero_vad", trust_repo=True
        )
        self._threshold = threshold
        self._sample_rate = 16000

        vad_iterator_cls = utils[3]  # VADIterator
        self._iterator = vad_iterator_cls(
            self._model,
            threshold=threshold,
            sampling_rate=self._sample_rate,
            min_silence_duration_ms=min_silence_ms,
            speech_pad_ms=speech_pad_ms,
        )

    def process_chunk(self, pcm_i16: bytes) -> int:
        """Feed raw i16 LE PCM bytes. Returns VAD status constant.

        Returns 0x00 (no event), 0x01 (speech_start), or 0x02 (speech_end).
        """
        from .protocol import VAD_NO_EVENT, VAD_SPEECH_START, VAD_SPEECH_END

        audio = np.frombuffer(pcm_i16, dtype=np.int16).astype(np.float32) / 32768.0
        tensor = torch.from_numpy(audio)
        result = self._iterator(tensor, return_seconds=False)

        if result is None:
            return VAD_NO_EVENT
        if "start" in result:
            return VAD_SPEECH_START
        if "end" in result:
            return VAD_SPEECH_END
        return VAD_NO_EVENT

    def reset(self) -> None:
        self._iterator.reset_states()
