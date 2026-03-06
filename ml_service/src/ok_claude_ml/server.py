"""ML inference service -- Unix socket server with binary framed protocol."""

import asyncio
import logging
import os
import signal
import struct
import sys

from .protocol import (
    MSG_PING,
    MSG_SYNTHESIZE,
    MSG_TRANSCRIBE,
    MSG_VAD_FEED,
    HEADER_SIZE,
    read_frame,
    write_error,
    write_frame,
)
from .stt import SpeechToText
from .tts import TextToSpeech
from .vad import VoiceActivityDetector

log = logging.getLogger("ok_claude_ml")


class MlServer:
    def __init__(
        self,
        socket_path: str,
        stt_model: str = "base",
        stt_language: str | None = None,
        tts_model: str | None = None,
    ):
        self._socket_path = socket_path
        self._stt_model = stt_model
        self._stt_language = stt_language
        self._tts_model = tts_model
        self._vad: VoiceActivityDetector | None = None
        self._stt: SpeechToText | None = None
        self._tts: TextToSpeech | None = None
        self._executor: asyncio.AbstractEventLoop | None = None

    def _load_models(self) -> None:
        log.info("Loading VAD model...")
        self._vad = VoiceActivityDetector()
        log.info("Loading STT model (faster-whisper %s)...", self._stt_model)
        self._stt = SpeechToText(model_size=self._stt_model, language=self._stt_language)
        log.info("Loading TTS model (Piper)...")
        self._tts = TextToSpeech(model_path=self._tts_model)
        log.info("All models loaded.")

    async def _handle_client(
        self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter
    ) -> None:
        loop = asyncio.get_running_loop()
        try:
            while True:
                try:
                    msg_type, body = await read_frame(reader)
                except asyncio.IncompleteReadError:
                    break

                if msg_type == MSG_PING:
                    await write_frame(writer, MSG_PING, b"")

                elif msg_type == MSG_VAD_FEED:
                    status = self._vad.process_chunk(body)
                    await write_frame(writer, MSG_VAD_FEED, bytes([status]))

                elif msg_type == MSG_TRANSCRIBE:
                    text = await loop.run_in_executor(
                        None, self._stt.transcribe, body
                    )
                    await write_frame(writer, MSG_TRANSCRIBE, text.encode("utf-8"))

                elif msg_type == MSG_SYNTHESIZE:
                    text = body.decode("utf-8")
                    audio_data = await loop.run_in_executor(
                        None, self._tts.synthesize, text
                    )
                    await write_frame(writer, MSG_SYNTHESIZE, audio_data)

                else:
                    await write_error(writer, f"unknown message type: 0x{msg_type:02x}")

        except Exception as e:
            log.exception("Client handler error")
            try:
                await write_error(writer, str(e))
            except Exception:
                pass
        finally:
            writer.close()
            await writer.wait_closed()

    async def run(self) -> None:
        self._load_models()

        if os.path.exists(self._socket_path):
            os.unlink(self._socket_path)

        server = await asyncio.start_unix_server(
            self._handle_client, path=self._socket_path
        )
        log.info("ML service listening on %s", self._socket_path)

        stop = asyncio.Event()
        loop = asyncio.get_running_loop()
        for sig in (signal.SIGINT, signal.SIGTERM):
            loop.add_signal_handler(sig, stop.set)

        async with server:
            await stop.wait()

        log.info("ML service shutting down")
        if os.path.exists(self._socket_path):
            os.unlink(self._socket_path)


def main() -> None:
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
    )

    socket_path = os.environ.get(
        "OK_CLAUDE_ML_SOCKET",
        os.path.join(
            os.environ.get("XDG_RUNTIME_DIR", f"/tmp/ok_claude-{os.getuid()}"),
            "ok_claude_ml.sock",
        ),
    )
    stt_model = os.environ.get("OK_CLAUDE_STT_MODEL", "base")
    stt_language = os.environ.get("OK_CLAUDE_STT_LANGUAGE")
    tts_model = os.environ.get("OK_CLAUDE_TTS_MODEL")

    server = MlServer(
        socket_path=socket_path,
        stt_model=stt_model,
        stt_language=stt_language,
        tts_model=tts_model or None,
    )
    asyncio.run(server.run())


if __name__ == "__main__":
    main()
