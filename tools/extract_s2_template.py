import struct

buf = open(r"assets\serina flps\serina1\serina1.flp", "rb").read()
print("flp size:", len(buf))
hdrlen = struct.unpack_from("<I", buf, 4)[0]
pos = 8 + hdrlen
assert buf[pos:pos + 4] == b"FLdt"
dtlen = struct.unpack_from("<I", buf, pos + 4)[0]
stream = buf[pos + 8:pos + 8 + dtlen]
print("dtlen:", dtlen, "first bytes:", stream[:8].hex())


def varint(s, o):
    v = 0
    sh = 0
    while True:
        b = s[o]
        o += 1
        v |= (b & 0x7F) << sh
        if not (b & 0x80):
            return v, o
        sh += 7


count = 0
p = 0
while p < len(stream):
    eid = stream[p]
    p += 1
    if eid < 64:
        dlen = 1
    elif eid < 128:
        dlen = 2
    elif eid < 192:
        dlen = 4
    else:
        dlen, p = varint(stream, p)
    data = stream[p:p + dlen]
    if eid == 213:
        try:
            ver = struct.unpack_from("<I", data, 0)[0]
            if ver >= 5:
                q = 4
                recs = []
                while q + 12 <= len(data):
                    cid = struct.unpack_from("<I", data, q)[0]
                    sz = struct.unpack_from("<Q", data, q + 4)[0]
                    q += 12
                    recs.append((cid, sz, data[q:q + sz]))
                    q += sz
                nm = [r for r in recs if r[0] == 54]
                nmv = nm[0][2].decode("utf-8", "replace") if nm else ""
                if nmv in ("Serum2", "Serum 2"):
                    print(f"=== Serum2 instance, stream offset {p - 1 - dlen - 1:#x}")
                    for cid, sz, pl in recs:
                        print(f"  cid {cid}: {sz} B")
                    state = [r for r in recs if r[0] == 53][0][2]
                    print("  state first bytes:", state[:20].hex())
                    prol = struct.unpack_from("<I", state, 0)[0]
                    print("  prologue:", prol)
                    q = 4
                    while q + 12 <= len(state):
                        icid = struct.unpack_from("<I", state, q)[0]
                        isz = struct.unpack_from("<Q", state, q + 4)[0]
                        q += 12
                        pl = state[q:q + isz]
                        print(f"    inner cid {icid}: {isz} B")
                        if icid == 4:
                            open("docs\\data\\serum2_cid4.bin", "wb").write(pl)
                            print("    -> saved serum2_cid4.bin")
                        if icid == 2:
                            open("docs\\data\\serum2_controller_record.bin", "wb").write(pl)
                            print("    -> saved serum2_controller_record.bin")
                        q += isz
                    count += 1
        except Exception as e:
            print("parse error:", e)
    p += dlen
print("S2 instances found:", count)
