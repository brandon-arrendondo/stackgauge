"""Generate minimal synthetic ELF32 ARM/DWARF4 fixtures (repository MIT license).

No compiler or third-party input is used. The function contains PUSH {r7,lr}
and SUB sp,#16, for a 24-byte frame. The Xtensa variant uses ENTRY a1,32;
the RISC-V variant deliberately retains ARM bytes to test ISA rejection.
Run: python3 tests/fixtures/elf-architecture/generate.py
"""
from pathlib import Path
import struct


def elf():
    abbrev = bytes([1, 0x11, 1, 0, 0, 2, 0x2e, 0, 3, 8, 0x11, 1, 0x12, 6, 0, 0, 0])
    entries = bytes([1, 2]) + b'frame\0' + struct.pack('<II', 0x1000, 4) + b'\0'
    unit = struct.pack('<HI B', 4, 0, 4) + entries
    info = struct.pack('<I', len(unit)) + unit
    names = b'\0.text\0.debug_abbrev\0.debug_info\0.shstrtab\0'
    sections = [(1, 1, 6, 0x1000, b'\x80\xb5\x84\xb0'), (7, 1, 0, 0, abbrev)]
    sections += [(21, 1, 0, 0, info), (33, 3, 0, 0, names)]
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
    arm = elf()
    (root / 'arm.elf').write_bytes(arm)
    xtensa = bytearray(arm)
    xtensa[18:20] = (94).to_bytes(2, 'little')  # EM_XTENSA
    xtensa[52:56] = b'\x36\x41\x00\x00'  # ENTRY a1,32
    (root / 'xtensa.elf').write_bytes(xtensa)
    arm[18:20] = (243).to_bytes(2, 'little')  # EM_RISCV
    (root / 'riscv.elf').write_bytes(arm)
