// A 2560 x 1440 NV12 frame to the detector's 1280 x 720 RGB24, as src/convert.rs's 2:1 path computes it (each output
// pixel's luma the rounded mean of its 2 x 2 block, each pair of pixels' chroma the rounded mean of two chroma samples
// of its row, then ffmpeg's yuv2rgb tables, RgbTables), that 720p luma itself for the camera (convert.rs's `luma`, the
// same means), and the Y plane's first `plane_rows` rows laid out row by row for the HUD, which reads no lower. Integers
// only, so the same bytes (prototypes/gpu_decode checked them against convert.rs).

Texture2D<uint> luma : register(t0);          // the NV12 texture's luma plane (R8_UINT view), 2560 x 1440
Texture2D<uint2> chroma : register(t1);       // its chroma plane (R8G8_UINT view), 1280 x 720, U and V interleaved
StructuredBuffer<int> tables : register(t2);  // y_table (2048), then red_v, green_u, blue_u, green_v (256 each)
RWByteAddressBuffer rgb : register(u0);       // 1280 x 720 x 3 bytes, row by row
RWByteAddressBuffer plane : register(u1);     // plane_rows x 2560 bytes: the Y plane's top rows, row by row
RWByteAddressBuffer small : register(u2);     // 1280 x 720 bytes: the luma means, row by row
cbuffer Rows : register(b0)
{
    uint plane_rows;                          // the Y plane's rows the HUD reads (hud::rows_read); 0: none
};

static const uint OUT_WIDTH = 1280;
static const uint OUT_HEIGHT = 720;
static const uint SRC_WIDTH = 2560;
static const uint PIXELS_PER_THREAD = 4;      // 12 RGB bytes: three whole words; 8 luma bytes a row: two words
static const int RED_V = 2048;
static const int GREEN_U = RED_V + 256;
static const int BLUE_U = GREEN_U + 256;
static const int GREEN_V = BLUE_U + 256;

// the rounded mean of the 2 x 2 block of luma under output pixel (x, row)
uint mean_luma(uint x, uint row)
{
    uint sum = luma.Load(int3(2 * x, 2 * row, 0)) + luma.Load(int3(2 * x + 1, 2 * row, 0))
        + luma.Load(int3(2 * x, 2 * row + 1, 0)) + luma.Load(int3(2 * x + 1, 2 * row + 1, 0));
    return (sum + 2) >> 2;
}

uint pixel_rgb(int y, uint2 uv)
{
    uint r = (uint)tables[tables[RED_V + uv.y] + y];
    uint g = (uint)tables[tables[GREEN_U + uv.x] + tables[GREEN_V + uv.y] + y];
    uint b = (uint)tables[tables[BLUE_U + uv.x] + y];
    return r | (g << 8) | (b << 16);
}

// four luma bytes of a row from column x on, as one little-endian word
uint luma_word(uint x, uint y)
{
    return luma.Load(int3(x, y, 0)) | (luma.Load(int3(x + 1, y, 0)) << 8) | (luma.Load(int3(x + 2, y, 0)) << 16)
        | (luma.Load(int3(x + 3, y, 0)) << 24);
}

[numthreads(64, 1, 1)]
void main(uint3 id : SV_DispatchThreadID)
{
    uint quads_per_row = OUT_WIDTH / PIXELS_PER_THREAD;
    if (id.x >= quads_per_row * OUT_HEIGHT)
        return;
    uint row = id.x / quads_per_row;
    uint first = (id.x % quads_per_row) * PIXELS_PER_THREAD;
    uint means[4];
    uint pixels[4];
    // two pixels at a time: a pair shares its chroma, the rounded mean of the chroma samples in its own two columns
    [unroll]
    for (uint at = 0; at < PIXELS_PER_THREAD; at += 2)
    {
        uint x = first + at;
        uint2 uv = (chroma.Load(int3(x, row, 0)) + chroma.Load(int3(x + 1, row, 0)) + 1) >> 1;
        means[at] = mean_luma(x, row);
        means[at + 1] = mean_luma(x + 1, row);
        pixels[at] = pixel_rgb((int)means[at], uv);
        pixels[at + 1] = pixel_rgb((int)means[at + 1], uv);
    }
    // four RGB pixels, 12 bytes, packed into three little-endian words
    uint word0 = pixels[0] | (pixels[1] << 24);
    uint word1 = (pixels[1] >> 8) | (pixels[2] << 16);
    uint word2 = (pixels[2] >> 16) | (pixels[3] << 8);
    rgb.Store3((row * OUT_WIDTH + first) * 3, uint3(word0, word1, word2));
    small.Store(row * OUT_WIDTH + first, means[0] | (means[1] << 8) | (means[2] << 16) | (means[3] << 24));
    // the eight luma columns these pixels came from, on both of their rows, where the HUD reads
    uint column = 2 * first;
    [unroll]
    for (uint half_row = 0; half_row < 2; half_row++)
    {
        uint y = 2 * row + half_row;
        if (y < plane_rows)
            plane.Store2(y * SRC_WIDTH + column, uint2(luma_word(column, y), luma_word(column + 4, y)));
    }
}
