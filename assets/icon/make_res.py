"""Writes fallow.res, the Windows resource with Fallow's icon, from fallow.ico.

The MSVC linker takes a .res file as an input (build.rs passes it), so no resource compiler is
needed at build time. Run it again after changing fallow.ico:

    python assets/icon/make_res.py
"""
import os
import struct

HERE = os.path.dirname(os.path.abspath(__file__))
RT_ICON, RT_GROUP_ICON = 3, 14


def entry(rtype, name, data, flags):
    """One resource: its header (numeric type and name), then its data, both DWORD-aligned."""
    head = struct.pack("<HHHH", 0xFFFF, rtype, 0xFFFF, name)
    head += struct.pack("<IHHII", 0, flags, 0, 0, 0)
    out = struct.pack("<II", len(data), 8 + len(head)) + head + data
    return out + b"\0" * (-len(out) % 4)


def main():
    with open(os.path.join(HERE, "fallow.ico"), "rb") as f:
        ico = f.read()
    reserved, kind, count = struct.unpack_from("<HHH", ico, 0)
    assert reserved == 0 and kind == 1, "not an icon file"
    # The empty first entry every .res starts with.
    res = struct.pack("<II", 0, 32) + struct.pack("<HHHH", 0xFFFF, 0, 0xFFFF, 0) + struct.pack("<IHHII", 0, 0, 0, 0, 0)
    group = struct.pack("<HHH", 0, 1, count)
    for i in range(count):
        w, h, colors, _, _planes, bits, size, offset = struct.unpack_from("<BBBBHHII", ico, 6 + 16 * i)
        res += entry(RT_ICON, i + 1, ico[offset:offset + size], 0x1010)
        # rc.exe writes 1 plane in the group, whatever the file says.
        group += struct.pack("<BBBBHHIH", w, h, colors, 0, 1, bits, size, i + 1)
    res += entry(RT_GROUP_ICON, 1, group, 0x1030)
    with open(os.path.join(HERE, "fallow.res"), "wb") as f:
        f.write(res)
    print(f"fallow.res: {count} images, {len(res)} bytes")


if __name__ == "__main__":
    main()
