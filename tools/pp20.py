#!/usr/bin/env python3
"""PowerPacker PP20 decruncher (algorithm per Stuart Caie's ppdepack)."""
import sys


def decrunch(data: bytes) -> bytes:
    if data[:4] != b"PP20":
        raise ValueError("not a PP20 file")
    offset_lens = data[4:8]
    src = data[8:-4]
    dest_len = (data[-4] << 16) | (data[-3] << 8) | data[-2]
    skip_bits = data[-1]
    out = bytearray(dest_len)
    pos = dest_len
    src_pos = len(src)
    bit_buf = 0
    bits_left = 0

    def read(n):
        nonlocal bit_buf, bits_left, src_pos
        while bits_left < n:
            if src_pos <= 0:
                raise ValueError("out of source bits")
            src_pos -= 1
            bit_buf |= src[src_pos] << bits_left
            bits_left += 8
        v = 0
        bits_left -= n
        for _ in range(n):
            v = (v << 1) | (bit_buf & 1)
            bit_buf >>= 1
        return v

    read(skip_bits)
    while pos > 0:
        if read(1) == 0:
            todo = 1
            while True:
                x = read(2)
                todo += x
                if x != 3:
                    break
            for _ in range(todo):
                pos -= 1
                out[pos] = read(8)
            if pos == 0:
                break
        x = read(2)
        offbits = offset_lens[x]
        todo = x + 2
        if x == 3:
            if read(1) == 0:
                offbits = 7
            offset = read(offbits)
            while True:
                x = read(3)
                todo += x
                if x != 7:
                    break
        else:
            offset = read(offbits)
        if pos + offset >= dest_len:
            raise ValueError("match overflow")
        for _ in range(todo):
            pos -= 1
            out[pos] = out[pos + 1 + offset]
    return bytes(out)


if __name__ == "__main__":
    src, dst = sys.argv[1], sys.argv[2]
    with open(src, "rb") as f:
        result = decrunch(f.read())
    with open(dst, "wb") as f:
        f.write(result)
    print(f"{src}: {len(result)} bytes")
