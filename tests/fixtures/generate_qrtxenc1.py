"""Reproduce the independent QRTXENC1 vector using libargon2 and Python cryptography.

Run from the repository root. Fixed salt/nonce are for this test vector ONLY.
"""
import ctypes
import gzip
import json
from pathlib import Path
from cryptography.hazmat.primitives.ciphers.aead import AESGCM

password = "  päss\u0065\u0301🔒  "
salt = bytes(range(16))
nonce = bytes(range(16, 28))
files = [("export.cqs.json.gz", gzip.compress(b'{"format":"cqs","items":[]}', mtime=0)), ("目录/empty.txt", b"")]
plain = b"QRTXFIL1" + len(files).to_bytes(4, "big")
for name, data in files:
    name = name.encode("utf-8")
    plain += len(name).to_bytes(4, "big") + name + len(data).to_bytes(8, "big") + data
argon2 = ctypes.CDLL("libargon2.so.1")
derive = argon2.argon2id_hash_raw
derive.argtypes = [ctypes.c_uint32, ctypes.c_uint32, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_size_t]
derive.restype = ctypes.c_int
key = ctypes.create_string_buffer(32)
encoded = password.encode("utf-8")
assert derive(3, 65536, 1, encoded, len(encoded), salt, len(salt), key, 32) == 0
header = b"QRTXENC1" + salt + nonce
envelope = header + AESGCM(key.raw).encrypt(nonce, plain, header)
vector = dict(password=password, key_hex=key.raw.hex(), plaintext_hex=plain.hex(), envelope_hex=envelope.hex(), files=[dict(name=name, hex=data.hex()) for name, data in files])
Path("tests/fixtures/qrtxenc1.json").write_text(json.dumps(vector, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
