"""The target detector: a tiny CenterNet-style network ("Objects as Points", Zhou et al. 2019) and its decoding.

Input  (N, 4, H, W): RGB scaled to 0-1, plus the fixed map (1 where the screen stays put: crosshair, HUD). H and W must
       be multiples of 16; a 1280 x 720 frame works as is.
Output (N, 5, H/4, W/4): target-centre heatmap logit, sub-pixel offset x and y (0-1 of a cell), log width and log
       height in input pixels.
Only standard ONNX operators are used (Conv, BatchNormalization folded at export, Relu, Add, Resize nearest, MaxPool in
decoding), so the same file runs under ONNX Runtime (CPU, GPU, Web/WebGPU, WASM) and pure-Rust tract.
"""
import torch
import torch.nn as nn
import torch.nn.functional as F

STRIDE = 4


def conv_bn(cin, cout, k=3, s=1, groups=1, dil=1):
    return nn.Sequential(nn.Conv2d(cin, cout, k, s, dil * (k // 2), dilation=dil, groups=groups, bias=False),
                         nn.BatchNorm2d(cout), nn.ReLU(inplace=True))


class Block(nn.Module):
    """Depthwise 3x3 + pointwise 1x1, with a residual when the shape allows it."""

    def __init__(self, cin, cout, s=1, dil=1):
        super().__init__()
        self.dw = conv_bn(cin, cin, 3, s, groups=cin, dil=dil)
        self.pw = conv_bn(cin, cout, 1)
        self.res = s == 1 and cin == cout

    def forward(self, x):
        y = self.pw(self.dw(x))
        return x + y if self.res else y


class Detector(nn.Module):
    def __init__(self, widths=(16, 32, 48, 64), blocks=(2, 2, 2), head=32):
        super().__init__()
        c0, c4, c8, c16 = widths
        self.stem = conv_bn(4, c0, 3, 2)                                     # stride 2
        self.s4 = nn.Sequential(Block(c0, c4, 2), *[Block(c4, c4) for _ in range(blocks[0])])
        self.s8 = nn.Sequential(Block(c4, c8, 2), *[Block(c8, c8) for _ in range(blocks[1])])
        self.s16 = nn.Sequential(Block(c8, c16, 2), *[Block(c16, c16, dil=2) for _ in range(blocks[2])])
        self.lat16 = conv_bn(c16, c8, 1)
        self.lat8 = conv_bn(c8, c4, 1)
        self.head = nn.Sequential(Block(c4, head), nn.Conv2d(head, 5, 1))
        nn.init.constant_(self.head[-1].bias, 0.0)
        with torch.no_grad():
            self.head[-1].bias[0] = -2.19                                     # heatmap prior 0.1

    def forward(self, x):
        f4 = self.s4(self.stem(x))
        f8 = self.s8(f4)
        f16 = self.s16(f8)
        u8 = f8 + F.interpolate(self.lat16(f16), scale_factor=2.0, mode="nearest")
        u4 = f4 + F.interpolate(self.lat8(u8), scale_factor=2.0, mode="nearest")
        return self.head(u4)


def build(cfg):
    m = cfg["model"]
    return Detector(tuple(m["widths"]), tuple(m["blocks"]), m["head"])


def prepare(rgb, fixed):
    """uint8 RGB (N, H, W, 3) and 0/1 fixed map (N, H, W) tensors to the network input (N, 4, H, W) float."""
    x = rgb.permute(0, 3, 1, 2).float() / 255.0
    return torch.cat([x, fixed[:, None].float()], dim=1)


def decode(out, thr=0.3, k=100):
    """Network output to detections per image: tensor (n, 5) of cx, cy, w, h (input px) and score."""
    hm = torch.sigmoid(out[:, 0:1].float())
    peak = (hm == F.max_pool2d(hm, 3, 1, 1)) & (hm > thr)
    dets = []
    for b in range(out.shape[0]):
        ys, xs = torch.nonzero(peak[b, 0], as_tuple=True)
        sc = hm[b, 0, ys, xs]
        if len(sc) > k:
            sc, i = sc.topk(k)
            ys, xs = ys[i], xs[i]
        o = out[b, :, ys, xs].float()
        cx = (xs.float() + o[1]) * STRIDE
        cy = (ys.float() + o[2]) * STRIDE
        dets.append(torch.stack([cx, cy, o[3].exp(), o[4].exp(), sc], dim=1))
    return dets
