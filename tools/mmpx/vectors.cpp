// Reference-vector generator: synthetic images through the MMPX reference C++ (McGuire & Gagiu
// 2021, MIT; the MMPXAlgorithm class of the supplement's code/source/cppPerf.cpp, included
// verbatim), printed as the fixture u6-render's tests read. 4x = the reference twice.
#include <cstdint>
#include <cstdio>
#include <string>
#include <vector>
#include <algorithm>

typedef uint32_t uint32;
typedef uint32_t ABGR8;
typedef std::string String;
struct Color4unorm8 { uint8_t r, g, b, a; };
namespace G3D {
inline int clamp(int v, int lo, int hi) { return v < lo ? lo : (v > hi ? hi : v); }
}

// The supplement's Algorithm base, as far as MMPX uses it (src() clamps to the border).
class Algorithm {
protected:
    mutable const ABGR8* m_srcBuffer;
    mutable int m_srcWidth;
    mutable int m_srcHeight;
    mutable int m_srcMaxX;
    mutable int m_srcMaxY;

    inline ABGR8 src(int x, int y) const {
        if (uint32(x) > uint32(m_srcMaxX) || uint32(y) > uint32(m_srcMaxY)) {
            x = G3D::clamp(x, 0, m_srcMaxX);
            y = G3D::clamp(y, 0, m_srcMaxY);
        }
        return m_srcBuffer[y * m_srcWidth + x];
    }

    void init(const Color4unorm8* srcBuffer, int srcWidth, int srcHeight) const {
        m_srcBuffer = (const ABGR8*)srcBuffer;
        m_srcWidth = srcWidth;
        m_srcHeight = srcHeight;
        m_srcMaxX = srcWidth - 1;
        m_srcMaxY = srcHeight - 1;
    }

public:
    virtual ~Algorithm() {}
    virtual String name() const = 0;
    virtual void run(const Color4unorm8* srcBuffer, Color4unorm8* dstBuffer, int srcWidth, int srcHeight) const = 0;
};

#include "mmpx_ref_body.inc"

static uint64_t rng = 0x9E3779B97F4A7C15ull;
static uint32_t next(uint32_t n) {
    rng ^= rng << 13; rng ^= rng >> 7; rng ^= rng << 17;
    return (uint32_t)(rng % n);
}

static std::vector<uint32_t> mmpx(const std::vector<uint32_t>& in, int w, int h) {
    std::vector<uint32_t> out((size_t)w * h * 4);
    MMPXAlgorithm a;
    a.run((const Color4unorm8*)in.data(), (Color4unorm8*)out.data(), w, h);
    return out;
}

static const char* DIGITS = "0123456789abcdefghijklmnopqrstuvwxyz";

static int total_cases = 0, nontrivial = 0;

// Prints one case: the palette, the input as palette indices, the output likewise.
static void emit(const char* name, int w, int h, int s, const std::vector<uint32_t>& pal,
                 const std::vector<int>& idx) {
    std::vector<uint32_t> in(idx.size());
    for (size_t i = 0; i < idx.size(); i++) in[i] = pal[idx[i]];
    std::vector<uint32_t> out = mmpx(in, w, h);
    int ow = w * 2, oh = h * 2;
    if (s == 4) { out = mmpx(out, ow, oh); ow *= 2; oh *= 2; }
    printf("case %s %d %d %d\npalette", name, w, h, s);
    for (uint32_t c : pal) printf(" %08x", c);
    printf("\n");
    for (int y = 0; y < h; y++) {
        for (int x = 0; x < w; x++) putchar(DIGITS[idx[y * w + x]]);
        putchar('\n');
    }
    printf("=\n");
    int differ = 0;
    for (int y = 0; y < oh; y++) {
        for (int x = 0; x < ow; x++) {
            uint32_t c = out[y * ow + x];
            int k = (int)(std::find(pal.begin(), pal.end(), c) - pal.begin());
            if (k == (int)pal.size()) { fprintf(stderr, "a new colour in %s\n", name); return; }
            putchar(DIGITS[k]);
            if (c != in[(y * h / oh) * w + (x * w / ow)]) differ++;
        }
        putchar('\n');
    }
    total_cases++;
    if (differ > 0) nontrivial++;
    fprintf(stderr, "%s: %d of %d output pixels differ from nearest\n", name, differ, ow * oh);
}

// Biased noise: each pixel copies its left (pl/100), upper (pu/100) or a diagonal (pd/100)
// neighbour, else takes a random palette entry.
static std::vector<int> noise(int w, int h, int k, int pl, int pu, int pd) {
    std::vector<int> idx((size_t)w * h);
    for (int y = 0; y < h; y++)
        for (int x = 0; x < w; x++) {
            int r = (int)next(100), v;
            if (r < pl && x > 0) v = idx[y * w + x - 1];
            else if (r < pl + pu && y > 0) v = idx[(y - 1) * w + x];
            else if (r < pl + pu + pd && y > 0 && x > 0) v = idx[(y - 1) * w + x - 1];
            else if (r < pl + pu + 2 * pd && y > 0 && x + 1 < w) v = idx[(y - 1) * w + x + 1];
            else v = (int)next(k);
            idx[y * w + x] = v;
        }
    return idx;
}

// Lines of slope dy/dx in colour 1.. over colour 0, and filled discs.
static std::vector<int> shapes(int w, int h, int k) {
    std::vector<int> idx((size_t)w * h, 0);
    int n = 2 + (int)next(4);
    for (int i = 0; i < n; i++) {
        int c = 1 + (int)next(k - 1);
        int kind = (int)next(4);
        if (kind == 0) {  // a disc
            int cx = (int)next(w), cy = (int)next(h), r = 1 + (int)next(std::max(1, std::min(w, h) / 3));
            for (int y = 0; y < h; y++)
                for (int x = 0; x < w; x++)
                    if ((x - cx) * (x - cx) + (y - cy) * (y - cy) <= r * r) idx[y * w + x] = c;
        } else {  // a line of slope 1:1, 2:1 or 1:2
            int x0 = (int)next(w), y0 = (int)next(h), sx = kind == 2 ? 2 : 1, sy = kind == 3 ? 2 : 1;
            int dir = next(2) ? 1 : -1;
            for (int t = 0; t < w + h; t++) {
                int x = x0 + dir * (t * sx / std::max(sx, sy)), y = y0 + (t * sy / std::max(sx, sy));
                if (x >= 0 && x < w && y >= 0 && y < h) idx[y * w + x] = c;
            }
        }
    }
    return idx;
}

int main() {
    printf("# MMPX reference vectors (u6-render hd tests): synthetic images, run through the MMPX\n");
    printf("# reference C++ of McGuire & Gagiu 2021 (MIT; the JCGT supplement's cppPerf.cpp\n");
    printf("# MMPXAlgorithm, clamp-to-border sampling), x4 = the reference twice. Per case: its name,\n");
    printf("# width, height and scale; its palette (packed little-endian RGBA words, alpha in the top\n");
    printf("# byte); the input as palette indices (0-9 a-z), then '=' and the output likewise.\n");
    // Palettes: opaque colours of distinct and equal luma, fully transparent, half transparent.
    const std::vector<uint32_t> p2 = {0xff000000, 0xffffffff};
    const std::vector<uint32_t> p3 = {0xff202020, 0xff2040c0, 0xffe0e0e0};
    const std::vector<uint32_t> p4t = {0x00000000, 0xff1010a0, 0xff30d030, 0xfff0f0f0};
    const std::vector<uint32_t> p5 = {0xff000000, 0xff0000ff, 0xff00ff00, 0xffff0000, 0xff808080};
    const std::vector<uint32_t> p6a = {0x00000000, 0x80ff8000, 0xff402010, 0xff804020, 0xffc08040, 0xffffffff};
    const std::vector<uint32_t> p8 = {0xff000000, 0xff550000, 0xff005500, 0xff000055, 0xffaaaaaa,
                                      0xff5555ff, 0xffff5555, 0xffffffff};
    char name[64];
    // Tiny and thin images: the border clamp on every side.
    emit("one-pixel", 1, 1, 2, p2, {1});
    emit("one-row", 6, 1, 2, p3, {0, 1, 1, 2, 0, 2});
    emit("one-column", 1, 6, 2, p3, {2, 2, 0, 1, 0, 0});
    emit("two-by-two", 2, 2, 2, p2, {0, 1, 1, 0});
    emit("one-row-x4", 5, 1, 4, p4t, {0, 1, 2, 2, 3});
    // Fixed structures: a checker, a diagonal, a staircase, a lone pixel, a plus.
    {
        std::vector<int> c(64), d(64, 0), st(64, 0), lone(49, 0), plus(49, 0);
        for (int i = 0; i < 64; i++) c[i] = ((i % 8) + (i / 8)) % 2;
        for (int i = 0; i < 8; i++) d[i * 8 + i] = 1;
        for (int y = 0; y < 8; y++) for (int x = 0; x < 8; x++) if (x >= 2 * (y / 2) + y % 2) st[y * 8 + x] = 1;
        lone[3 * 7 + 3] = 1;
        for (int i = 0; i < 7; i++) { plus[3 * 7 + i] = 1; plus[i * 7 + 3] = 1; }
        emit("checker", 8, 8, 2, p2, c);
        emit("diagonal", 8, 8, 2, p2, d);
        emit("diagonal-x4", 8, 8, 4, p2, d);
        emit("staircase", 8, 8, 2, p2, st);
        emit("staircase-x4", 8, 8, 4, p2, st);
        emit("lone-pixel", 7, 7, 2, p2, lone);
        emit("plus", 7, 7, 2, p2, plus);
        // The same diagonal light on dark and dark on light: luma breaks the ties.
        std::vector<int> d3(64, 2);
        for (int i = 0; i < 8; i++) d3[i * 8 + i] = 0;
        emit("dark-diagonal-on-light", 8, 8, 2, p3, d3);
        // The intersection rules whose neighbourhoods noise rarely makes: a pixel with its left
        // (then its upper) neighbour and that neighbour's two across in another colour.
        emit("intersection-left", 7, 5, 2, p3, {
            0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 1, 0, 0, 0,
            0, 0, 1, 0, 0, 0, 0,
            0, 0, 0, 1, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0});
        emit("intersection-left-x4", 7, 5, 4, p4t, {
            3, 3, 3, 3, 3, 3, 3,
            3, 3, 3, 1, 3, 3, 3,
            3, 2, 1, 3, 3, 3, 3,
            3, 3, 3, 1, 3, 3, 3,
            3, 3, 3, 3, 3, 3, 3});
        emit("intersection-top", 5, 6, 2, p3, {
            2, 2, 2, 2, 2,
            2, 2, 2, 2, 2,
            2, 2, 0, 2, 2,
            2, 0, 2, 0, 2,
            2, 2, 2, 2, 2,
            2, 2, 2, 2, 2});
        emit("intersection-top-x4", 5, 6, 4, p4t, {
            0, 0, 2, 0, 0,
            0, 0, 0, 0, 0,
            0, 0, 1, 0, 0,
            0, 1, 0, 1, 0,
            0, 0, 0, 0, 0,
            0, 0, 0, 0, 0});
        // Triangle tips whose tip has the luma of the body in another colour (p5: blue, green
        // and red weigh the same): the strict luma test keeps the body.
        std::vector<int> tip = {
            0, 0, 0, 0, 0,
            0, 0, 2, 0, 0,
            0, 0, 3, 0, 0,
            0, 3, 3, 3, 0,
            0, 0, 3, 0, 0};
        emit("tip-equal-luma-up", 5, 5, 2, p5, tip);
        std::vector<int> down(25), left(25), right(25);
        for (int y = 0; y < 5; y++)
            for (int x = 0; x < 5; x++) {
                down[(4 - y) * 5 + x] = tip[y * 5 + x];
                left[x * 5 + y] = tip[y * 5 + x];
                right[x * 5 + (4 - y)] = tip[y * 5 + x];
            }
        emit("tip-equal-luma-down", 5, 5, 2, p5, down);
        emit("tip-equal-luma-left", 5, 5, 2, p5, left);
        emit("tip-equal-luma-right", 5, 5, 2, p5, right);
        tip[7] = 0;
        tip[2] = 1;
        emit("tip-darker", 5, 5, 2, p5, tip);
        // A 1:1 corner whose colour has the body's luma, the body running on two pixels past it
        // (the last clause of the 1:1 rules), in its four mirror images.
        const std::vector<int> corner = {
            0, 0, 3, 0, 0,
            0, 2, 2, 3, 0,
            3, 2, 3, 0, 0,
            0, 0, 0, 0, 0,
            0, 0, 0, 0, 0};
        const char* names[] = {"corner-equal-luma", "corner-equal-luma-h", "corner-equal-luma-v", "corner-equal-luma-hv"};
        for (int m = 0; m < 4; m++) {
            std::vector<int> c(25);
            for (int y = 0; y < 5; y++)
                for (int x = 0; x < 5; x++)
                    c[(m & 2 ? 4 - y : y) * 5 + (m & 1 ? 4 - x : x)] = corner[y * 5 + x];
            emit(names[m], 5, 5, 2, p5, c);
        }
    }
    struct Spec { const char* tag; int w, h, s; const std::vector<uint32_t>* pal; int pl, pu, pd; };
    const Spec specs[] = {
        {"noise", 16, 16, 2, &p2, 30, 30, 15}, {"noise", 16, 16, 2, &p3, 35, 25, 15},
        {"noise", 16, 16, 2, &p4t, 30, 30, 20}, {"noise", 16, 16, 2, &p5, 40, 30, 10},
        {"noise", 16, 16, 2, &p6a, 30, 30, 20}, {"noise", 16, 16, 2, &p8, 35, 35, 10},
        {"noise", 13, 9, 2, &p4t, 25, 25, 25}, {"noise", 20, 13, 2, &p3, 45, 30, 10},
        {"noise", 24, 24, 2, &p5, 35, 35, 15}, {"noise", 3, 5, 2, &p5, 20, 20, 20},
        {"noise", 7, 4, 2, &p4t, 30, 30, 20}, {"noise", 16, 16, 4, &p4t, 30, 30, 20},
        {"noise", 16, 16, 4, &p3, 35, 30, 15}, {"noise", 12, 10, 4, &p6a, 30, 30, 20},
        {"noise", 5, 7, 4, &p8, 25, 25, 20},
    };
    int n = 0;
    for (const Spec& s : specs) {
        snprintf(name, sizeof name, "%s-%d", s.tag, n++);
        emit(name, s.w, s.h, s.s, *s.pal, noise(s.w, s.h, (int)s.pal->size(), s.pl, s.pu, s.pd));
    }
    const Spec shape_specs[] = {
        {"shapes", 16, 16, 2, &p4t}, {"shapes", 16, 16, 2, &p5}, {"shapes", 24, 16, 2, &p3},
        {"shapes", 16, 16, 4, &p4t}, {"shapes", 16, 16, 4, &p8}, {"shapes", 11, 14, 2, &p6a},
    };
    for (const Spec& s : shape_specs) {
        snprintf(name, sizeof name, "%s-%d", s.tag, n++);
        emit(name, s.w, s.h, s.s, *s.pal, shapes(s.w, s.h, (int)s.pal->size()));
    }
    fprintf(stderr, "%d cases, %d with output beyond nearest\n", total_cases, nontrivial);
    return 0;
}
