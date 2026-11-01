"""Writes fallow.res, the Windows resource with Fallow's icon (from fallow.ico) and its version
information (the name "Fallow" and the version in Cargo.toml), which Windows shows for the
program and its notifications.

The MSVC linker takes a .res file as an input (build.rs passes it), so no resource compiler is
needed at build time. Run it again after changing fallow.ico:

    python assets/icon/make_res.py
"""
import os
import struct

HERE = os.path.dirname(os.path.abspath(__file__))
RT_ICON, RT_GROUP_ICON, RT_VERSION = 3, 14, 16


def entry(rtype, name, data, flags):
    """One resource: its header (numeric type and name), then its data, both DWORD-aligned."""
    head = struct.pack("<HHHH", 0xFFFF, rtype, 0xFFFF, name)
    head += struct.pack("<IHHII", 0, flags, 0, 0, 0)
    out = struct.pack("<II", len(data), 8 + len(head)) + head + data
    return out + b"\0" * (-len(out) % 4)


def block(key, value=b"", children=b"", text=False):
    """One VS_VERSIONINFO node: its length, its value's length (in characters for text), its
    type, its key, then its value and its children, each DWORD-aligned."""
    value_len = len(value) // 2 if text else len(value)
    out = struct.pack("<HHH", 0, value_len, 1 if text else 0) + key.encode("utf-16-le") + b"\0\0"
    out += b"\0" * (-len(out) % 4) + value
    if children:
        out += b"\0" * (-len(out) % 4) + children
    out = struct.pack("<H", len(out)) + out[2:]
    return out + b"\0" * (-len(out) % 4)


def version_info(version):
    """The version resource: the fixed part, then the strings in English (U.S.), Unicode."""
    a, b, c = (int(x) for x in version.split(".")[:3])
    fixed = struct.pack(
        "<13I", 0xFEEF04BD, 0x00010000, (a << 16) | b, c << 16, (a << 16) | b, c << 16,
        0x3F, 0, 0x40004, 1, 0, 0, 0,
    )
    strings = b"".join(
        block(k, (v + "\0").encode("utf-16-le"), text=True)
        for k, v in [
            ("CompanyName", "Fallow"),
            ("FileDescription", "Fallow"),
            ("FileVersion", version),
            ("InternalName", "fallow"),
            ("OriginalFilename", "fallow.exe"),
            ("ProductName", "Fallow"),
            ("ProductVersion", version),
        ]
    )
    table = block("040904B0", children=strings, text=True)
    sfi = block("StringFileInfo", children=table, text=True)
    var = block("Translation", struct.pack("<HH", 0x0409, 0x04B0))
    vfi = block("VarFileInfo", children=var, text=True)
    return block("VS_VERSION_INFO", fixed, sfi + vfi)


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
    with open(os.path.join(HERE, "..", "..", "Cargo.toml"), encoding="utf-8") as f:
        version = next(l.split('"')[1] for l in f if l.startswith("version ="))
    res += entry(RT_VERSION, 1, version_info(version), 0x0030)
    with open(os.path.join(HERE, "fallow.res"), "wb") as f:
        f.write(res)
    print(f"fallow.res: {count} images, {len(res)} bytes")


if __name__ == "__main__":
    main()
