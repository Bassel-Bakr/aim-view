// A 2560 x 1440 NV12 frame to the detector's 1280 x 720 RGB24, as src/convert.rs's 2:1 path computes it: each
// output pixel's luma the rounded mean of its 2 x 2 block, each pair of pixels' chroma the rounded mean of two chroma
// samples of its row, then ffmpeg's yuv2rgb tables (RgbTables). Integers only, so the same bytes.

Texture2D<uint> luma : register(t0);      // the NV12 texture's luma plane (R8_UINT view), 2560 x 1440
Texture2D<uint2> chroma : register(t1);   // its chroma plane (R8G8_UINT view), 1280 x 720, U and V interleaved
StructuredBuffer<int> tables : register(t2);  // y_table (2048), then red_v, green_u, blue_u, green_v (256 each)
RWByteAddressBuffer rgb : register(u0);   // 1280 x 720 x 3 bytes, row by row

static const uint OUT_WIDTH = 1280;
static const uint OUT_HEIGHT = 720;
static const uint PIXELS_PER_THREAD = 4;  // 12 bytes: three whole words
static const int RED_V = 2048;
static const int GREEN_U = RED_V + 256;
static const int BLUE_U = GREEN_U + 256;
static const int GREEN_V = BLUE_U + 256;

uint pixel_rgb(uint x, uint row, uint2 uv)
{
    uint sum = luma.Load(int3(2 * x, 2 * row, 0)) + luma.Load(int3(2 * x + 1, 2 * row, 0))
        + luma.Load(int3(2 * x, 2 * row + 1, 0)) + luma.Load(int3(2 * x + 1, 2 * row + 1, 0));
    int y = (int)((sum + 2) >> 2);
    uint r = (uint)tables[tables[RED_V + uv.y] + y];
    uint g = (uint)tables[tables[GREEN_U + uv.x] + tables[GREEN_V + uv.y] + y];
    uint b = (uint)tables[tables[BLUE_U + uv.x] + y];
    return r | (g << 8) | (b << 16);
}

[numthreads(64, 1, 1)]
void main(uint3 id : SV_DispatchThreadID)
{
    uint quads_per_row = OUT_WIDTH / PIXELS_PER_THREAD;
    if (id.x >= quads_per_row * OUT_HEIGHT)
        return;
    uint row = id.x / quads_per_row;
    uint first = (id.x % quads_per_row) * PIXELS_PER_THREAD;
    uint pixels[4];
    [unroll]
    for (uint pair = 0; pair < 2; pair++)
    {
        uint chroma_x = first + 2 * pair;  // the pair's two chroma samples: columns 2p and 2p + 1 of its row
        uint2 left = chroma.Load(int3(chroma_x, row, 0));
        uint2 right = chroma.Load(int3(chroma_x + 1, row, 0));
        uint2 uv = (left + right + 1) >> 1;
        pixels[2 * pair] = pixel_rgb(first + 2 * pair, row, uv);
        pixels[2 * pair + 1] = pixel_rgb(first + 2 * pair + 1, row, uv);
    }
    // four RGB pixels, 12 bytes, packed into three little-endian words
    uint word0 = pixels[0] | (pixels[1] << 24);
    uint word1 = (pixels[1] >> 8) | (pixels[2] << 16);
    uint word2 = (pixels[2] >> 16) | (pixels[3] << 8);
    rgb.Store3((row * OUT_WIDTH + first) * 3, uint3(word0, word1, word2));
}
