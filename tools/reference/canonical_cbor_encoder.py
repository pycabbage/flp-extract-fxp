# cbor.py — RFC 8949 CBOR subset encoder/decoder + XferJson container builder
# following docs/serum2-state-format.md:
#   maps 0xA0..0xB9 (count), arrays 0x80..0x99, text 0x60..0x79,
#   ints major 0/1 (minimal), false 0xF4 / true 0xF5 / null 0xF6,
#   f32 (0xFA+BE) when the f64 value is exactly representable in f32, else
#   f64 (0xFB+BE), byte strings major 2.
# Map keys are emitted in std::map (lexicographic byte) order.
import struct, hashlib, json as _pyjson
from compression import zstd

ZSTD_MAGIC = b"\x28\xb5\x2f\xfd"


def _head(n, maj):
    if n < 24:
        return bytes([maj << 5 | n])
    if n < 0x100:
        return bytes([maj << 5 | 24, n])
    if n < 0x10000:
        return bytes([maj << 5 | 25]) + n.to_bytes(2, "big")
    if n < 0x100000000:
        return bytes([maj << 5 | 26]) + n.to_bytes(4, "big")
    return bytes([maj << 5 | 27]) + n.to_bytes(8, "big")


def enc_int(v):
    if v < 0:
        return _head(-1 - v, 1)
    return _head(v, 0)


def enc_f64_exact(v):
    if v != v:                                  # NaN -> f64 (nlohmann rule)
        return b"\xfb" + struct.pack(">d", v)
    try:
        packed = struct.pack(">f", v)
    except (OverflowError, ValueError):
        return b"\xfb" + struct.pack(">d", v)
    if struct.unpack(">f", packed)[0] == v:
        return b"\xfa" + packed
    return b"\xfb" + struct.pack(">d", v)


def cbor_enc(v):
    if v is None:
        return b"\xf6"
    if v is True:
        return b"\xf5"
    if v is False:
        return b"\xf4"
    if isinstance(v, str):
        raw = v.encode("utf-8")
        return _head(len(raw), 3) + raw
    if isinstance(v, (bytes, bytearray)):
        return _head(len(v), 2) + bytes(v)
    if isinstance(v, int):
        return enc_int(v)
    if isinstance(v, float):
        return enc_f64_exact(v)
    if isinstance(v, dict):
        if "__i__" in v:
            return enc_int(v["__i__"])
        if "__u__" in v:
            return _head(v["__u__"], 0)
        if "__f__" in v:
            bits = v["__f__"] & 0xFFFFFFFFFFFFFFFF
            f = struct.unpack("<d", bits.to_bytes(8, "little"))[0]
            return enc_f64_exact(f)
        if "__bin__" in v:
            raw = bytes.fromhex(v["__bin__"])
            return _head(len(raw), 2) + raw
        if "__bool__" in v:
            return b"\xf5" if v["__bool__"] else b"\xf4"
        if "__null__" in v:
            return b"\xf6"
        if "__error__" in v:
            raise ValueError("tree contains error node: " + str(v))
        out = _head(len(v), 5)
        for k in sorted(v.keys(), key=lambda s: s.encode("utf-8")):
            out += cbor_enc(k) + cbor_enc(v[k])
        return out
    if isinstance(v, list):
        out = _head(len(v), 4)
        for x in v:
            out += cbor_enc(x)
        return out
    raise TypeError(f"cannot encode {type(v)}")


# ---------------- decoder (with byte accounting) ----------------

class CBORReader:
    def __init__(self, data):
        self.d = data
        self.p = 0

    def _arg(self, info, n):
        if info < 24:
            return info
        if info == 24:
            return self.d[self.p] if False else self._u8()
        return None

    def _u(self, n):
        v = int.from_bytes(self.d[self.p:self.p + n], "big")
        self.p += n
        return v

    def _item(self):
        b = self.d[self.p]; self.p += 1
        maj, info = b >> 5, b & 0x1F
        if info < 24:
            ln = info
        elif info == 24:
            ln = self._u(1)
        elif info == 25:
            ln = self._u(2)
        elif info == 26:
            ln = self._u(4)
        elif info == 27:
            ln = self._u(8)
        elif info == 31:
            ln = None
        else:
            raise ValueError(f"bad info {info}")
        if maj == 0:
            return ln
        if maj == 1:
            return -1 - ln
        if maj == 2:
            v = self.d[self.p:self.p + ln]; self.p += ln; return v
        if maj == 3:
            v = self.d[self.p:self.p + ln]; self.p += ln; return v.decode("utf-8")
        if maj == 4:
            out = []
            for _ in range(ln):
                out.append(self._item())
            return out
        if maj == 5:
            out = {}
            for _ in range(ln):
                k = self._item()
                out[k] = self._item()
            return out
        if maj == 6:
            self._item(); return None   # tags: skip
        if maj == 7:
            # the argument bytes were already consumed into `ln` above
            if info == 20: return False
            if info == 21: return True
            if info in (22, 23): return None
            if info == 25:
                return struct.unpack(">e", ln.to_bytes(2, "big"))[0]
            if info == 26:
                return struct.unpack(">f", ln.to_bytes(4, "big"))[0]
            if info == 27:
                return struct.unpack(">d", ln.to_bytes(8, "big"))[0]
            if info in (0, 1):
                return ("half", ln)
            return ("simple", info)
        raise ValueError("unreachable")


def cbor_dec(data):
    r = CBORReader(data)
    v = r._item()
    return v, r.p


# ---------------- container ----------------

def make_container(body: bytes, hash_md5: str | None = None,
                   header_overrides: dict | None = None):
    frame = zstd.compress(body, level=3)
    if hash_md5 is None:
        hash_md5 = hashlib.md5(frame).hexdigest()
    hdr = {
        "component": "processor",
        "hash": hash_md5,
        "product": "Serum2",
        "productVersion": "2.0.23",
        "url": "https://xferrecords.com/",
        "vendor": "Xfer Records",
        "version": 9.0,
    }
    if header_overrides:
        hdr.update(header_overrides)
    text = _pyjson.dumps(hdr, separators=(",", ":"), sort_keys=True)
    raw = text.encode("ascii")
    assert len(raw) == 183, len(raw)
    out = b"XferJson\0" + struct.pack("<Q", 183) + raw
    out += struct.pack("<I", len(body)) + struct.pack("<I", 2) + frame
    return out


def parse_container(data: bytes):
    assert data[:9] == b"XferJson\0", data[:9]
    jlen = int.from_bytes(data[9:17], "little")
    header = data[17:17 + jlen].decode()
    body_len, fmt = struct.unpack_from("<II", data, 17 + jlen)
    stream = data[17 + jlen + 8:]
    body = zstd.decompress(stream)
    return header, body_len, fmt, stream, body
