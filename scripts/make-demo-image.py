"""Create the deterministic, exactly 2 KiB PNG used for optical smoke tests."""
from pathlib import Path
import hashlib
import struct
import zlib

output = Path(__file__).resolve().parents[1] / "tests" / "fixtures" / "webcam-2k.png"
width = height = 64
pixels = bytearray()
glyphs = ["01110011110", "11011011011", "11011011110", "11110011100", "00111011011"]
for y in range(height):
    pixels.append(0)
    for x in range(width):
        gx, gy = (x - 10) // 4, (y - 22) // 4
        ink = 0 <= gx < 11 and 0 <= gy < 5 and glyphs[gy][gx] == "1"
        color = (245, 248, 250) if ink else ((16, 115, 183) if x < 32 else (25, 155, 103))
        pixels.extend(color)

def chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

png = b"\x89PNG\r\n\x1a\n"
png += chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(bytes(pixels), 9))
padding_size = 2048 - len(png) - 24
png += chunk(b"tEXt", b"Comment\0" + b"QRTX webcam test " * ((padding_size - 8) // 17) + b"." * ((padding_size - 8) % 17))
png += chunk(b"IEND", b"")
assert len(png) == 2048
output.parent.mkdir(parents=True, exist_ok=True)
output.write_bytes(png)
print(f"{hashlib.sha256(png).hexdigest()}  {output} ({len(png)} bytes)")
