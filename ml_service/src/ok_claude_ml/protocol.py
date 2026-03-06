"""Binary framed protocol for Rust daemon <-> Python ML service communication.

Frame format:
    [msg_type: 1 byte] [body_len: 4 bytes u32 LE] [body: body_len bytes]

Message types:
    0x01  VAD_FEED     -> VAD_RESULT     (audio chunk -> speech boundary event)
    0x02  TRANSCRIBE   -> TRANSCRIPTION  (audio -> text)
    0x03  SYNTHESIZE   -> AUDIO_OUT      (text -> audio)
    0x04  PING         -> PONG           (health check)
    0xFF  ERROR        (response only)
"""

import asyncio
import struct

MSG_VAD_FEED = 0x01
MSG_TRANSCRIBE = 0x02
MSG_SYNTHESIZE = 0x03
MSG_PING = 0x04
MSG_ERROR = 0xFF

# VAD result status bytes
VAD_NO_EVENT = 0x00
VAD_SPEECH_START = 0x01
VAD_SPEECH_END = 0x02

HEADER_FMT = "<BI"  # msg_type (u8) + body_len (u32 LE)
HEADER_SIZE = struct.calcsize(HEADER_FMT)


async def read_frame(reader: asyncio.StreamReader) -> tuple[int, bytes]:
    """Read one frame from the stream. Returns (msg_type, body)."""
    header = await reader.readexactly(HEADER_SIZE)
    msg_type, body_len = struct.unpack(HEADER_FMT, header)
    body = await reader.readexactly(body_len) if body_len > 0 else b""
    return msg_type, body


async def write_frame(writer: asyncio.StreamWriter, msg_type: int, body: bytes) -> None:
    """Write one frame to the stream."""
    header = struct.pack(HEADER_FMT, msg_type, len(body))
    writer.write(header + body)
    await writer.drain()


async def write_error(writer: asyncio.StreamWriter, message: str) -> None:
    """Write an error response frame."""
    await write_frame(writer, MSG_ERROR, message.encode("utf-8"))
