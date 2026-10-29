#!/usr/bin/env python
"""Generate SYNTHETIC camera-RAW stand-ins (minimal valid DNG files) without a camera and without third-party packages.

Run it isolated:   python -I tools/dev/make-synthetic-raw.py [OUTDIR] [--size WxH]
Self check:        python -I tools/dev/make-synthetic-raw.py selftest      (also: --selftest)

These files are NOT a camera corpus. They exist so the RAW decode path (LibRaw, the pr-imaging-raw container readers,
the preview-to-full swap in the viewer) can be exercised end to end on a machine without .cr2/.nef/.arw/.dng from a
real camera. They prove "a valid DNG of this shape is accepted and routed correctly", never "every real camera works".
docs/PARITY.md keeps the RAW corpus rows Partial for exactly this reason.

Output goes only into OUTDIR (default: %TEMP%/photoreview-synthetic-raw). Nothing is written into the repository, and
the generated binaries are never committed.

File format (DNG 1.4 / TIFF 6, little endian), see DNG specification 1.4.0.0:
  raw IFD   : NewSubfileType 0, 16 bit uncompressed (Compression 1), PhotometricInterpretation 32803 (CFA), RGGB
              (CFARepeatPatternDim 2x2, CFAPattern 0 1 1 2, CFALayout 1), BlackLevel 64, WhiteLevel 4095, DNGVersion
              1.4.0.0, UniqueCameraModel, ColorMatrix1 (XYZ -> linear sRGB, D65), AsShotNeutral 1 1 1,
              CalibrationIlluminant1 21 (D65), Orientation, one strip.
  preview   : NewSubfileType 1, Compression 7 (baseline JPEG), PhotometricInterpretation 6 (YCbCr) 8 bit, one strip.
              The JPEG is produced by the pure-Python baseline encoder below (4:4:4, flat custom quantisation tables,
              fixed-length canonical Huffman tables: valid baseline JPEG, not size-optimal).
  layouts   : "ifd0"   raw in IFD0, preview in the next IFD of the chain (what many converters write);
              "subifd" preview in IFD0, raw in a SubIFD (tag 330), the Adobe DNG Converter shape.
  exif      : an Exif IFD with DateTimeOriginal.

Scene: eight saturated colour bars (white, yellow, cyan, green, magenta, red, blue, black) over the top two thirds, a
grey ramp below, and a black square in the top-left corner so a wrong orientation is visible at a glance.
"""
import hashlib
import math
import os
import struct
import sys
import tempfile

BLACK_LEVEL = 64
WHITE_LEVEL = 4095
DEFAULT_SIZE = (600, 400)
MAKE = "PhotoReviewSynthetic"
MODEL = "Synthetic DNG 1"

# TIFF field types.
BYTE, ASCII, SHORT, LONG, RATIONAL, SRATIONAL = 1, 2, 3, 4, 5, 10
_TYPE_SIZE = {BYTE: 1, ASCII: 1, SHORT: 2, LONG: 4, RATIONAL: 8, SRATIONAL: 8}

# --------------------------------------------------------------------------------------------------- scene

_BARS = [(1, 1, 1), (1, 1, 0), (0, 1, 1), (0, 1, 0), (1, 0, 1), (1, 0, 0), (0, 0, 1), (0, 0, 0)]


def scene(u, v):
    """Linear RGB (0..1 floats) at normalised coordinates u, v in [0, 1)."""
    if u < 0.07 and v < 0.105:  # black marker in the top-left corner (white bar behind it)
        return (0.0, 0.0, 0.0)
    if v < 2.0 / 3.0:
        r, g, b = _BARS[min(int(u * 8), 7)]
        return (float(r), float(g), float(b))
    level = u  # linear ramp
    return (level, level, level)


def _gamma8(linear):
    return int(round(255.0 * (max(0.0, min(1.0, linear)) ** (1.0 / 2.2))))


def scene_rgb8(u, v):
    r, g, b = scene(u, v)
    return (_gamma8(r), _gamma8(g), _gamma8(b))


# --------------------------------------------------------------------------------------------------- raw data


def mosaic_rggb16(width, height):
    """16 bit little-endian RGGB mosaic of the scene, black level 64, white level 4095."""
    span = WHITE_LEVEL - BLACK_LEVEL
    out = bytearray(width * height * 2)
    pos = 0
    for y in range(height):
        v = (y + 0.5) / height
        even_row = (y & 1) == 0
        for x in range(width):
            u = (x + 0.5) / width
            rgb = scene(u, v)
            channel = rgb[0] if (even_row and not x & 1) else rgb[2] if (not even_row and x & 1) else rgb[1]
            value = BLACK_LEVEL + int(round(channel * span))
            out[pos] = value & 0xFF
            out[pos + 1] = value >> 8
            pos += 2
    return bytes(out)


# --------------------------------------------------------------------------------------------------- JPEG encoder

def _build_zigzag():
    order = sorted(((x + y, x if (x + y) % 2 == 0 else y, x, y) for y in range(8) for x in range(8)))
    return [y * 8 + x for _, _, x, y in order]


_ZIGZAG = _build_zigzag()
_COS = [[math.cos((2 * x + 1) * u * math.pi / 16.0) for x in range(8)] for u in range(8)]
_ALPHA = [math.sqrt(0.125)] + [0.5] * 7
# Flat custom quantisation tables (any 1..255 values are valid baseline JPEG).
_QUANT = [[4 + 2 * ((i // 8) + (i % 8)) for i in range(64)], [8 + 2 * ((i // 8) + (i % 8)) for i in range(64)]]


def _huffman_codes():
    """DC: 12 symbols, 4 bit codes; AC: 162 symbols, 8 bit codes (canonical, never all ones)."""
    dc_vals = list(range(12))
    ac_vals = [0x00, 0xF0] + [(run << 4) | size for run in range(16) for size in range(1, 11)]
    dc_bits = [0] * 16
    dc_bits[3] = len(dc_vals)
    ac_bits = [0] * 16
    ac_bits[7] = len(ac_vals)
    dc = {s: (i, 4) for i, s in enumerate(dc_vals)}
    ac = {s: (i, 8) for i, s in enumerate(ac_vals)}
    return dc_bits, dc_vals, ac_bits, ac_vals, dc, ac


class _BitWriter:
    def __init__(self):
        self.out = bytearray()
        self.acc = 0
        self.nbits = 0

    def put(self, code, length):
        self.acc = (self.acc << length) | (code & ((1 << length) - 1))
        self.nbits += length
        while self.nbits >= 8:
            self.nbits -= 8
            byte = (self.acc >> self.nbits) & 0xFF
            self.out.append(byte)
            if byte == 0xFF:
                self.out.append(0)
        self.acc &= (1 << self.nbits) - 1

    def finish(self):
        if self.nbits:
            self.put((1 << (8 - self.nbits)) - 1, 8 - self.nbits)
        return bytes(self.out)


def _category(value):
    return abs(value).bit_length()


def _amplitude_bits(value, size):
    return value if value >= 0 else value + (1 << size) - 1


def _dct_quantise(block, quant):
    # Separable 8x8 DCT-II on level-shifted samples, then quantisation and zigzag.
    tmp = [[0.0] * 8 for _ in range(8)]
    for y in range(8):
        row = block[y]
        for u in range(8):
            cu = _COS[u]
            tmp[y][u] = _ALPHA[u] * sum(row[x] * cu[x] for x in range(8))
    coefficients = [0] * 64
    for u in range(8):
        for v in range(8):
            cv = _COS[v]
            total = _ALPHA[v] * sum(tmp[y][u] * cv[y] for y in range(8))
            coefficients[v * 8 + u] = int(round(total / quant[v * 8 + u]))
    return [coefficients[i] for i in _ZIGZAG]


def encode_jpeg(rgb_at, width, height):
    """Baseline JPEG bytes of `rgb_at(x, y) -> (r, g, b)` (0..255 ints) for a width x height image."""
    padded_w, padded_h = (width + 7) // 8 * 8, (height + 7) // 8 * 8
    planes = [[[0.0] * padded_w for _ in range(padded_h)] for _ in range(3)]
    for y in range(padded_h):
        for x in range(padded_w):
            r, g, b = rgb_at(min(x, width - 1), min(y, height - 1))
            planes[0][y][x] = 0.299 * r + 0.587 * g + 0.114 * b - 128.0
            planes[1][y][x] = -0.168736 * r - 0.331264 * g + 0.5 * b
            planes[2][y][x] = 0.5 * r - 0.418688 * g - 0.081312 * b
    dc_bits, dc_vals, ac_bits, ac_vals, dc_codes, ac_codes = _huffman_codes()
    writer = _BitWriter()
    previous_dc = [0, 0, 0]
    for by in range(0, padded_h, 8):
        for bx in range(0, padded_w, 8):
            for comp in range(3):
                plane = planes[comp]
                block = [plane[by + y][bx : bx + 8] for y in range(8)]
                coded = _dct_quantise(block, _QUANT[0 if comp == 0 else 1])
                diff = coded[0] - previous_dc[comp]
                previous_dc[comp] = coded[0]
                size = _category(diff)
                writer.put(*dc_codes[size])
                if size:
                    writer.put(_amplitude_bits(diff, size), size)
                run = 0
                last = max((i for i in range(1, 64) if coded[i]), default=0)
                for i in range(1, last + 1):
                    value = coded[i]
                    if value == 0:
                        run += 1
                        continue
                    while run > 15:
                        writer.put(*ac_codes[0xF0])
                        run -= 16
                    size = _category(value)
                    writer.put(*ac_codes[(run << 4) | size])
                    writer.put(_amplitude_bits(value, size), size)
                    run = 0
                if last < 63:
                    writer.put(*ac_codes[0x00])
    scan = writer.finish()

    def segment(marker, payload):
        return b"\xFF" + bytes([marker]) + struct.pack(">H", len(payload) + 2) + payload

    out = bytearray(b"\xFF\xD8")
    out += segment(0xE0, b"JFIF\x00\x01\x01\x00\x00\x01\x00\x01\x00\x00")
    for table_id, table in enumerate(_QUANT):
        out += segment(0xDB, bytes([table_id]) + bytes(table[i] for i in _ZIGZAG))
    out += segment(0xC0, struct.pack(">BHHB", 8, height, width, 3) + b"".join(bytes([c, 0x11, 0 if c == 1 else 1]) for c in (1, 2, 3)))
    out += segment(0xC4, bytes([0x00]) + bytes(dc_bits) + bytes(dc_vals))
    out += segment(0xC4, bytes([0x10]) + bytes(ac_bits) + bytes(ac_vals))
    out += segment(0xDA, bytes([3, 1, 0x00, 2, 0x00, 3, 0x00, 0, 63, 0]))
    out += scan
    out += b"\xFF\xD9"
    return bytes(out)


def scene_jpeg(width, height):
    return encode_jpeg(lambda x, y: scene_rgb8((x + 0.5) / width, (y + 0.5) / height), width, height)


# --------------------------------------------------------------------------------------------------- TIFF writer


def _encode_values(kind, values):
    if kind == ASCII:
        return values.encode("ascii") + b"\x00", len(values) + 1
    if kind == BYTE:
        return bytes(values), len(values)
    if kind == SHORT:
        return struct.pack("<%dH" % len(values), *values), len(values)
    if kind == LONG:
        return struct.pack("<%dI" % len(values), *values), len(values)
    if kind == RATIONAL:
        return b"".join(struct.pack("<II", n, d) for n, d in values), len(values)
    if kind == SRATIONAL:
        return b"".join(struct.pack("<ii", n, d) for n, d in values), len(values)
    raise ValueError(kind)


class Ifd:
    """One IFD. Entries are (tag, type, values); LONG values may be `Ref` placeholders resolved at serialise time."""

    def __init__(self):
        self.entries = []

    def add(self, tag, kind, values):
        self.entries.append((tag, kind, values))
        return self

    def size(self):
        total = 2 + 12 * len(self.entries) + 4
        for _, kind, values in self.entries:
            length = len(values) if kind != ASCII else len(values) + 1
            data = length * _TYPE_SIZE[kind]
            if data > 4:
                total += data + (data & 1)
        return total

    def serialise(self, offset, next_ifd):
        table = bytearray(struct.pack("<H", len(self.entries)))
        extra = bytearray()
        extra_base = offset + 2 + 12 * len(self.entries) + 4
        for tag, kind, values in sorted(self.entries, key=lambda e: e[0]):
            data, count = _encode_values(kind, values)
            if len(data) <= 4:
                table += struct.pack("<HHI", tag, kind, count) + data.ljust(4, b"\x00")
            else:
                table += struct.pack("<HHII", tag, kind, count, extra_base + len(extra))
                extra += data + (b"\x00" if len(data) & 1 else b"")
        table += struct.pack("<I", next_ifd)
        return bytes(table + extra)


def _rational(values, den=10000):
    return [(int(round(v * den)), den) for v in values]


# XYZ(D65) -> linear sRGB, so the synthetic "camera" RGB is sRGB linear.
_COLOR_MATRIX = [3.2406, -1.5372, -0.4986, -0.9689, 1.8758, 0.0415, 0.0557, -0.2040, 1.0570]


def _dng_tags(ifd):
    ifd.add(271, ASCII, MAKE).add(272, ASCII, MODEL)
    ifd.add(50706, BYTE, [1, 4, 0, 0]).add(50707, BYTE, [1, 1, 0, 0])
    ifd.add(50708, ASCII, MODEL)
    ifd.add(50721, SRATIONAL, _rational(_COLOR_MATRIX))
    ifd.add(50728, RATIONAL, [(1, 1), (1, 1), (1, 1)])
    ifd.add(50778, SHORT, [21])


def _raw_tags(ifd, width, height, strip_offset, strip_len):
    ifd.add(254, LONG, [0]).add(256, LONG, [width]).add(257, LONG, [height])
    ifd.add(258, SHORT, [16]).add(259, SHORT, [1]).add(262, SHORT, [32803])
    ifd.add(273, LONG, [strip_offset]).add(277, SHORT, [1]).add(278, LONG, [height]).add(279, LONG, [strip_len])
    ifd.add(284, SHORT, [1])
    ifd.add(33421, SHORT, [2, 2]).add(33422, BYTE, [0, 1, 1, 2])
    ifd.add(50710, BYTE, [0, 1, 2]).add(50711, SHORT, [1])
    ifd.add(50713, SHORT, [1, 1]).add(50714, SHORT, [BLACK_LEVEL]).add(50717, SHORT, [WHITE_LEVEL])


def _preview_tags(ifd, width, height, strip_offset, strip_len):
    ifd.add(254, LONG, [1]).add(256, LONG, [width]).add(257, LONG, [height])
    ifd.add(258, SHORT, [8, 8, 8]).add(259, SHORT, [7]).add(262, SHORT, [6])
    ifd.add(273, LONG, [strip_offset]).add(277, SHORT, [3]).add(278, LONG, [height]).add(279, LONG, [strip_len])
    ifd.add(284, SHORT, [1])


def build_dng(width, height, preview_size=None, orientation=1, layout="ifd0", raw=None, jpeg=None):
    """Bytes of one DNG. `preview_size` None => no embedded preview. Returns (bytes, info dict)."""
    raw = raw if raw is not None else mosaic_rggb16(width, height)
    assert len(raw) == width * height * 2
    body = bytearray()
    raw_off = 8
    body += raw
    if len(body) & 1:
        body += b"\x00"
    jpeg_off = 8 + len(body)
    if preview_size is not None:
        jpeg = jpeg if jpeg is not None else scene_jpeg(*preview_size)
        body += jpeg + (b"\x00" if len(jpeg) & 1 else b"")
    base = 8 + len(body)

    exif = Ifd().add(36867, ASCII, "2026:10:29 12:00:00").add(36868, ASCII, "2026:10:29 12:00:00")
    raw_ifd = Ifd()
    _raw_tags(raw_ifd, width, height, raw_off, len(raw))
    preview_ifd = None
    if preview_size is not None:
        preview_ifd = Ifd()
        _preview_tags(preview_ifd, preview_size[0], preview_size[1], jpeg_off, len(jpeg))

    # Offsets of the IFD blocks, then the real tables (pointer tags need those offsets).
    ifd0 = Ifd()
    if layout == "ifd0" or preview_ifd is None:
        main = ifd0
        _raw_tags(main, width, height, raw_off, len(raw))
        _dng_tags(main)
        main.add(274, SHORT, [orientation]).add(34665, LONG, [0])
        order = [("ifd0", main), ("exif", exif)]
        if preview_ifd is not None:
            order.append(("preview", preview_ifd))
    elif layout == "subifd":
        main = ifd0
        _preview_tags(main, preview_size[0], preview_size[1], jpeg_off, len(jpeg))
        _dng_tags(main)
        main.add(274, SHORT, [orientation]).add(34665, LONG, [0]).add(330, LONG, [0])
        order = [("ifd0", main), ("exif", exif), ("raw", raw_ifd)]
    else:
        raise ValueError(layout)

    offsets = {}
    cursor = base
    for name, ifd in order:
        offsets[name] = cursor
        cursor += ifd.size()
        cursor += cursor & 1

    def patch(ifd, tag, value):
        ifd.entries = [(t, k, [value] if t == tag else v) for t, k, v in ifd.entries]

    patch(main, 34665, offsets["exif"])
    if layout == "subifd" and preview_ifd is not None:
        patch(main, 330, offsets["raw"])
    out = bytearray(b"II*\x00" + struct.pack("<I", offsets["ifd0"]))
    out += body
    for name, ifd in order:
        assert len(out) == offsets[name], (name, len(out), offsets[name])
        next_ifd = offsets["preview"] if (name == "ifd0" and "preview" in offsets) else 0
        out += ifd.serialise(offsets[name], next_ifd)
        if len(out) & 1:
            out += b"\x00"
    info = {
        "width": width,
        "height": height,
        "preview": preview_size,
        "orientation": orientation,
        "layout": layout,
        "raw_offset": raw_off,
        "raw_len": len(raw),
        "raw_sha256": hashlib.sha256(raw).hexdigest(),
    }
    return bytes(out), info


# --------------------------------------------------------------------------------------------------- the file set


def file_set(width, height):
    """(file name, builder kwargs or JPEG spec) for the whole set."""
    half = (width // 2, height // 2)
    return [
        ("plain.dng", dict(width=width, height=height)),
        ("preview.dng", dict(width=width, height=height, preview_size=half)),
        ("subifd.dng", dict(width=width, height=height, preview_size=half, layout="subifd")),
        ("tinypreview.dng", dict(width=width, height=height, preview_size=(width // 5, height // 5))),
        ("orient6.dng", dict(width=width, height=height, preview_size=half, orientation=6)),
        ("orient3.dng", dict(width=width, height=height, preview_size=half, orientation=3)),
        # A preview whose size differs from the sensor size: this is the file of the preview-to-full size swap.
        ("swap.dng", dict(width=width, height=height, preview_size=(width // 4, height // 4))),
        ("pair.dng", dict(width=width, height=height, preview_size=half)),
    ]


def generate(outdir, width, height):
    os.makedirs(outdir, exist_ok=True)
    report = []
    for name, kwargs in file_set(width, height):
        data, info = build_dng(**kwargs)
        path = os.path.join(outdir, name)
        with open(path, "wb") as handle:
            handle.write(data)
        report.append((name, len(data), info))
    # RAW+JPEG pair: pair.dng + pair.jpg (same stem, full-size JPEG of the same scene).
    jpeg = scene_jpeg(width, height)
    with open(os.path.join(outdir, "pair.jpg"), "wb") as handle:
        handle.write(jpeg)
    report.append(("pair.jpg", len(jpeg), {"width": width, "height": height}))
    return report


# --------------------------------------------------------------------------------------------------- selftest


def parse_tiff(data):
    """Minimal reader used by the selftest: returns {ifd_name: {tag: (type, count, raw value bytes)}} along the chain."""
    assert data[:4] == b"II*\x00", "not little-endian TIFF"
    ifds = []
    seen = set()

    def read_ifd(offset):
        assert offset not in seen and offset + 2 <= len(data), "IFD loop or out of range"
        seen.add(offset)
        (count,) = struct.unpack_from("<H", data, offset)
        entries = {}
        for i in range(count):
            tag, kind, n, value = struct.unpack_from("<HHII", data, offset + 2 + 12 * i)
            size = _TYPE_SIZE[kind] * n
            if size <= 4:
                raw = data[offset + 2 + 12 * i + 8 : offset + 2 + 12 * i + 8 + size]
            else:
                assert value + size <= len(data), "value out of range for tag %d" % tag
                raw = data[value : value + size]
            entries[tag] = (kind, n, raw)
        (next_ifd,) = struct.unpack_from("<I", data, offset + 2 + 12 * count)
        return entries, next_ifd

    (offset,) = struct.unpack_from("<I", data, 4)
    while offset:
        entries, offset = read_ifd(offset)
        ifds.append(entries)
        if 330 in entries:
            sub = struct.unpack_from("<I", entries[330][2])[0]
            ifds.append(read_ifd(sub)[0])
    return ifds


def _ints(entry):
    kind, n, raw = entry
    fmt = {BYTE: "B", SHORT: "H", LONG: "I"}[kind]
    return list(struct.unpack("<%d%s" % (n, fmt), raw))


def selftest():
    failures = []

    def check(condition, message):
        if not condition:
            failures.append(message)

    with tempfile.TemporaryDirectory() as first, tempfile.TemporaryDirectory() as second:
        report = generate(first, 120, 80)
        generate(second, 120, 80)
        names = sorted(os.listdir(first))
        check(names == sorted(n for n, _, _ in report), "report and directory differ")
        for name in names:
            with open(os.path.join(first, name), "rb") as a, open(os.path.join(second, name), "rb") as b:
                check(a.read() == b.read(), "%s is not deterministic" % name)
        for name, size, info in report:
            with open(os.path.join(first, name), "rb") as handle:
                data = handle.read()
            check(len(data) == size, "%s size mismatch" % name)
            if name.endswith(".jpg"):
                check(data[:2] == b"\xFF\xD8" and data[-2:] == b"\xFF\xD9", "%s lacks SOI/EOI" % name)
                continue
            ifds = parse_tiff(data)
            raw_ifd = next(i for i in ifds if _ints(i[254])[0] == 0)
            check(_ints(raw_ifd[256]) == [info["width"]] and _ints(raw_ifd[257]) == [info["height"]], name + " raw size")
            check(_ints(raw_ifd[262]) == [32803] and _ints(raw_ifd[259]) == [1], name + " CFA/uncompressed")
            check(_ints(raw_ifd[258]) == [16], name + " bits")
            check(_ints(raw_ifd[33422]) == [0, 1, 1, 2], name + " CFA pattern")
            offset, length = _ints(raw_ifd[273])[0], _ints(raw_ifd[279])[0]
            check(length == info["width"] * info["height"] * 2, name + " strip length")
            check(hashlib.sha256(data[offset : offset + length]).hexdigest() == info["raw_sha256"], name + " raw checksum")
            check(_ints(ifds[0][50706]) == [1, 4, 0, 0], name + " DNGVersion")
            check(_ints(ifds[0][274]) == [info["orientation"]], name + " orientation")
            previews = [i for i in ifds if _ints(i[254])[0] == 1]
            check(len(previews) == (1 if info["preview"] else 0), name + " preview count")
            for preview in previews:
                poff, plen = _ints(preview[273])[0], _ints(preview[279])[0]
                jpeg = data[poff : poff + plen]
                check(jpeg[:2] == b"\xFF\xD8" and jpeg[-2:] == b"\xFF\xD9", name + " preview SOI/EOI")
                sof = jpeg.find(b"\xFF\xC0")
                height, width = struct.unpack_from(">HH", jpeg, sof + 5)
                check((width, height) == info["preview"], name + " preview SOF size")
                check(_ints(preview[256]) == [width] and _ints(preview[257]) == [height], name + " preview tags")
    # Gradient sanity: the ramp is monotonic, the marker is black, the first bar is white.
    check(scene(0.0, 0.5) == (1.0, 1.0, 1.0) and scene(0.01, 0.01) == (0.0, 0.0, 0.0), "scene markers")
    check(scene_rgb8(0.99, 0.9)[0] > scene_rgb8(0.5, 0.9)[0] > scene_rgb8(0.1, 0.9)[0], "ramp monotonic")
    if failures:
        for message in failures:
            print("FAIL:", message)
        return 1
    print("selftest ok: %d files, deterministic, headers and checksums verified" % len(report))
    return 0


# --------------------------------------------------------------------------------------------------- main


def main(argv):
    if "selftest" in argv or "--selftest" in argv:
        return selftest()
    size = DEFAULT_SIZE
    outdir = None
    args = list(argv)
    while args:
        arg = args.pop(0)
        if arg == "--size":
            w, h = args.pop(0).lower().split("x")
            size = (int(w), int(h))
        elif arg in ("-h", "--help"):
            print(__doc__)
            return 0
        else:
            outdir = arg
    if size[0] % 8 or size[1] % 8 or size[0] < 80 or size[1] < 80:
        print("--size must be at least 80x80 and a multiple of 8 on both sides", file=sys.stderr)
        return 2
    outdir = outdir or os.path.join(tempfile.gettempdir(), "photoreview-synthetic-raw")
    for name, size_bytes, info in generate(outdir, *size):
        print("%-16s %8d bytes  %s" % (name, size_bytes, {k: v for k, v in info.items() if k != "raw_sha256"}))
    print("written to", outdir)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
