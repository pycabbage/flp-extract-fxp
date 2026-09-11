# njson.py — reader for the nlohmann::json (MSVC x64) layout used by Serum2.vst3
# Derived empirically + from runtime disassembly:
#   json node = 16 B: {type u8 @0, pad, union qword @8}
#   type enum (nlohmann value_t): null=0, object=1, array=2, string=3,
#       boolean=4, number_integer=5, number_unsigned=6, number_float=7,
#       binary=8, discarded=9
#   object  -> std::map<std::string, json>: wrapper {_Myhead@0, _Mysize@8};
#              node (0x50 B) {_Left@0,_Parent@8,_Right@0x10,_Color@0x18,
#               _Isnil@0x19, key std::string(32B)@0x20, value json(16B)@0x40}
#   array   -> std::vector<json> (24B triple, 16B elements inline)
#   string  -> std::string* (32B): {data/ptr@0, size@0x10, cap@0x18}, SSO if cap<16
#   binary  -> std::vector<uint8_t> (24B triple)
# All reads go through kernel32.ReadProcessMemory (SEH-safe: bad pointers
# return False instead of crashing).
import ctypes, struct, base64

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
_hproc = k32.GetCurrentProcess()
_RPM = k32.ReadProcessMemory
_RPM.restype = ctypes.c_int
_RPM.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p,
                 ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]


def peek(addr, n):
    """Read n bytes; None if unreadable."""
    if not addr:
        return None
    buf = ctypes.create_string_buffer(n)
    got = ctypes.c_size_t(0)
    if _RPM(_hproc, ctypes.c_void_p(addr), buf, n, ctypes.byref(got)):
        return buf.raw[:got.value]
    return None


def q(addr, off=0):
    d = peek(addr + off, 8)
    return int.from_bytes(d, "little") if d else None


def _read_msvc_string(addr):
    """MSVC std::string (32 B): {data/ptr@0, size@0x10, cap@0x18}."""
    if not addr:
        return None
    d = peek(addr, 32)
    if not d:
        return None
    size = int.from_bytes(d[0x10:0x18], "little")
    cap = int.from_bytes(d[0x18:0x20], "little")
    if cap >= 16:
        ptr = int.from_bytes(d[0:8], "little")
        raw = peek(ptr, size + 1) if size else b""
    else:
        raw = d[0:size]
    if raw is None or len(raw) < size:
        return None
    try:
        return raw[:size].decode("utf-8")
    except UnicodeDecodeError:
        return raw[:size].decode("utf-8", "replace")


def _read_binary(addr):
    """vector<uint8_t> 24B triple."""
    if not addr:
        return None
    d = peek(addr, 24)
    if not d:
        return None
    b, e, c = (int.from_bytes(d[i:i+8], "little") for i in (0, 8, 16))
    if not (b and b <= e <= c) or (e - b) > (1 << 26):
        return None
    return peek(b, e - b)


def _walk_map(map_ptr):
    head = q(map_ptr, 0)
    size = q(map_ptr, 8)
    if not head or size is None or size > (1 << 21):
        return None
    out = {}
    stack = []
    node = q(head, 8)          # root = head._Parent
    while stack or node:
        while node and node != head and len(out) < size:
            stack.append(node)
            node = q(node, 0)  # _Left
        if not stack:
            break
        node = stack.pop()
        key = _read_msvc_string(node + 0x20)
        val = read_json(node + 0x40)
        if key is None:
            return None
        out[key] = val
        node = q(node, 0x10)   # _Right
    return out


def read_json(addr, depth=0):
    if addr is None or depth > 64:
        return {"__error__": "depth/unreadable"}
    t = peek(addr, 1)
    if t is None:
        return {"__error__": "unreadable"}
    ty = t[0]
    u = q(addr, 8)
    if ty == 0:
        return None
    if ty == 1:
        m = _walk_map(u) if u else None
        if m is None:
            return {"__error__": "map walk failed", "ptr": hex(u or 0)}
        return m
    if ty == 2:
        d = peek(u, 24) if u else None
        if not d:
            return {"__error__": "array vector unreadable"}
        b, e = (int.from_bytes(d[i:i+8], "little") for i in (0, 8))
        n = (e - b) >> 4 if e >= b else 0
        if n > (1 << 21):
            return {"__error__": "array too big"}
        arr = []
        for k in range(n):
            arr.append(read_json(b + 16 * k, depth + 1))
        return arr
    if ty == 3:
        return _read_msvc_string(u)
    if ty == 4:
        return bool(u & 0xFF)
    if ty == 5:
        return int.from_bytes(peek(addr + 8, 8) or b"\0" * 8, "little", signed=True)
    if ty == 6:
        return u if u is not None else 0
    if ty == 7:
        d = peek(addr + 8, 8)
        return struct.unpack("<d", d)[0] if d else None
    if ty == 8:
        d = peek(u, 24) if u else None
        if not d:
            return {"__error__": "binary vector unreadable", "ptr": hex(u or 0)}
        b, e, c = (int.from_bytes(d[i:i+8], "little") for i in (0, 8, 16))
        if not (b and b <= e <= c) or (e - b) > (1 << 26):
            return {"__error__": "binary vector bad", "ptr": hex(u or 0)}
        raw = peek(b, e - b)
        if raw is None:
            return {"__error__": "binary data unreadable", "ptr": hex(u or 0)}
        return {"__bin__": raw.hex()}
    if ty == 9:
        return {"__discarded__": True}
    return {"__error__": f"unknown type {ty}"}


def to_jsonable(v):
    """Make the walked tree strictly JSON-serializable (recursively)."""
    import base64
    if isinstance(v, dict):
        if "__bin__" in v and len(v) == 1:
            return {"__bin_b64__": base64.b64encode(bytes.fromhex(v["__bin__"])).decode()}
        if "__error__" in v or "__discarded__" in v:
            return v
        return {k: to_jsonable(x) for k, x in v.items()}
    if isinstance(v, list):
        return [to_jsonable(x) for x in v]
    if isinstance(v, float):
        return v
    return v


# ---------------- typed walking (exact nlohmann node types) ----------------

def _walk_map_typed(map_ptr, depth):
    head = q(map_ptr, 0)
    size = q(map_ptr, 8)
    if not head or size is None or size > (1 << 21):
        return None
    out = {}
    stack = []
    node = q(head, 8)
    while stack or node:
        while node and node != head and len(out) < size:
            stack.append(node)
            node = q(node, 0)
        if not stack:
            break
        node = stack.pop()
        key = _read_msvc_string(node + 0x20)
        if key is None:
            return None
        out[key] = read_json_typed(node + 0x40, depth)
        node = q(node, 0x10)
    return out


def read_json_typed(addr, depth=0):
    """Same walk but annotates every leaf with its exact nlohmann type:
    ints -> {"__i__": v} / {"__u__": v}; floats -> {"__f__": bits} (exact f64
    bit pattern); null -> {"__null__": true}; binary -> {"__bin__": hex}."""
    if addr is None or depth > 64:
        return {"__error__": "depth/unreadable"}
    t = peek(addr, 1)
    if t is None:
        return {"__error__": "unreadable"}
    ty = t[0]
    u = q(addr, 8)
    if ty == 0:
        return {"__null__": True}
    if ty == 1:
        m = _walk_map_typed(u, depth + 1) if u else None
        return m if m is not None else {"__error__": "map walk failed"}
    if ty == 2:
        d = peek(u, 24) if u else None
        if not d:
            return {"__error__": "array vector unreadable"}
        b, e = (int.from_bytes(d[i:i+8], "little") for i in (0, 8))
        n = (e - b) >> 4 if e >= b else 0
        return [read_json_typed(b + 16 * k, depth + 1) for k in range(n)]
    if ty == 3:
        return _read_msvc_string(u)
    if ty == 4:
        return bool(u & 0xFF)
    if ty == 5:
        d = peek(addr + 8, 8)
        return {"__i__": int.from_bytes(d, "little", signed=True)}
    if ty == 6:
        return {"__u__": u}
    if ty == 7:
        d = peek(addr + 8, 8)
        if not d:
            return {"__error__": "float unreadable"}
        return {"__f__": int.from_bytes(d, "little")}
    if ty == 8:
        d = peek(u, 24) if u else None
        if not d:
            return {"__error__": "binary vector unreadable"}
        b, e = (int.from_bytes(d[i:i+8], "little") for i in (0, 8))
        raw = peek(b, e - b) if (b and e >= b) else None
        return {"__bin__": raw.hex() if raw is not None else None}
    return {"__error__": f"unknown type {ty}"}
