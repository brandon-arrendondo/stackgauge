"""Generate minimal synthetic ELF32 ARM/DWARF4 fixtures (repository MIT license).

No compiler or third-party input is used. The function contains PUSH {r7,lr}
and SUB sp,#16, for a 24-byte frame. SHF_COMPRESSED uses ELF zlib headers,
the same representation emitted by -gz=zlib.
Run: python3 tests/fixtures/elf/generate.py
"""
from pathlib import Path
import struct
import zlib


def elf(compressed=False, oversized=False, damaged=False):
    abbrev = bytes([1, 0x11, 1, 0, 0, 2, 0x2e, 0, 3, 8, 0x11, 1, 0x12, 6, 0, 0, 0])
    entries = bytes([1, 2]) + b'frame\0' + struct.pack('<II', 0x1000, 4) + b'\0'
    unit = struct.pack('<HI B', 4, 0, 4) + entries
    info = struct.pack('<I', len(unit)) + unit
    names = b'\0.text\0.debug_abbrev\0.debug_info\0.shstrtab\0'
    sections = [(1, 1, 6, 0x1000, b'\x80\xb5\x84\xb0'), (7, 1, 0, 0, abbrev)]
    if compressed:
        packed = b'broken' if damaged else zlib.compress(info)
        info = struct.pack('<III', 1, 64 * 1024 * 1024 + 1 if oversized else len(info), 1) + packed
    sections += [(21, 1, 0x800 if compressed else 0, 0, info), (33, 3, 0, 0, names)]
    data = bytearray(52)
    headers = [bytes(40)]
    for name, kind, flags, addr, content in sections:
        headers.append(struct.pack('<10I', name, kind, flags, addr, len(data), len(content), 0, 0, 1, 0))
        data.extend(content)
    offset = len(data)
    data.extend(b''.join(headers))
    data[:52] = struct.pack('<16sHHIIIIIHHHHHH', b'\x7fELF\x01\x01\x01' + bytes(9), 2, 40, 1, 0x1000, 0, offset, 0, 52, 0, 0, 40, 5, 4)
    return data


if __name__ == '__main__':
    root = Path(__file__).parent
    for name, options in [('plain', {}), ('compressed', {'compressed': True}), ('oversized', {'compressed': True, 'oversized': True}), ('damaged', {'compressed': True, 'damaged': True})]:
        (root / (name + '.elf')).write_bytes(elf(**options))
    zstd = elf(compressed=True)
    section_table = struct.unpack_from('<I', zstd, 32)[0]
    info_offset = struct.unpack_from('<I', zstd, section_table + 3 * 40 + 16)[0]
    struct.pack_into('<I', zstd, info_offset, 2)  # ELFCOMPRESS_ZSTD
    (root / 'zstd.elf').write_bytes(zstd)
