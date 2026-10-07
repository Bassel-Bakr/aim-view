"""The target detector: a tiny CenterNet-style network ("Objects as Points", Zhou et al. 2019) and its decoding.

Input  (N, 4, H, W): RGB scaled to 0-1, plus the fixed map (1 where the screen stays put: crosshair, HUD). H and W must
       be multiples of 16; a 1280 x 720 frame works as is.
Output (N, 5, H/4, W/4): target-center heatmap logit, sub-pixel offset x and y (0-1 of a cell), log width and log
       height in input pixels.
Only standard ONNX operators are used (Conv, BatchNormalization folded at export, Relu, Add, Resize nearest, MaxPool in
decoding), so the same file runs under ONNX Runtime (CPU, GPU, Web/WebGPU, WASM) and pure-Rust tract.
"""
import torch
import torch.nn as nn
import torch.nn.functional as F

STRIDE = 4                      # input pixels per output cell
INPUT_CHANNELS = 4              # RGB and the fixed map
OUTPUT_CHANNELS = 5             # heatmap logit, offset x and y, log width and height
HEATMAP_PRIOR_LOGIT = -2.19     # the heatmap's starting bias: a prior of 0.1
PEAK_WINDOW = 3                 # cells: a peak is the largest score in its 3 x 3 window
MAX_DETECTIONS = 100            # detections kept per image at most, the strongest


def conv_bn(in_channels, out_channels, kernel=3, stride=1, groups=1, dilation=1, padding=None):
    """A convolution without bias, then batch norm and ReLU. The padding keeps the size (at stride 1) unless given."""
    padding = dilation * (kernel // 2) if padding is None else padding
    return nn.Sequential(nn.Conv2d(in_channels, out_channels, kernel, stride, padding,
                                   dilation=dilation, groups=groups, bias=False),
                         nn.BatchNorm2d(out_channels), nn.ReLU(inplace=True))


class Block(nn.Module):
    """Depthwise 3x3 + pointwise 1x1, with a residual when the shape allows it. The attribute names are the
    checkpoints' keys."""

    def __init__(self, in_channels, out_channels, stride=1, dilation=1):
        """`stride` 2 halves the map's size; `dilation` widens the depthwise kernel's reach."""
        super().__init__()
        self.dw = conv_bn(in_channels, in_channels, 3, stride, groups=in_channels, dilation=dilation)
        self.pw = conv_bn(in_channels, out_channels, 1)
        self.res = stride == 1 and in_channels == out_channels

    def forward(self, features):
        """The block's output for a (N, in_channels, H, W) map, with the input added back when the shapes match."""
        out = self.pw(self.dw(features))
        return features + out if self.res else out


class Detector(nn.Module):
    """The network. Its attribute names (stem, s4, s8, s16, lat16, lat8, head) are the checkpoints' keys."""

    def __init__(self, widths=(16, 32, 48, 64), blocks=(2, 2, 2), head=32, stem="conv"):
        """widths: the channels of the stem and of the stride 4, 8 and 16 maps. blocks: the extra blocks at strides 4,
        8 and 16. head: the head block's channels. stem: "conv" (3 x 3 at stride 2) or "patch" (4 x 4 at stride 4).
        The heatmap's bias starts at a prior of 0.1, the other outputs' at 0."""
        super().__init__()
        stem_channels, channels4, channels8, channels16 = widths
        if stem == "patch":
            # each 4 x 4 pixels to one cell at once: no map at stride 2, where most of the memory traffic was
            self.stem = conv_bn(INPUT_CHANNELS, stem_channels, STRIDE, STRIDE, padding=0)
            first_stride = 1
        else:
            self.stem = conv_bn(INPUT_CHANNELS, stem_channels, 3, 2)         # stride 2
            first_stride = 2
        self.s4 = nn.Sequential(Block(stem_channels, channels4, first_stride), *[Block(channels4, channels4)
                                                                                 for _ in range(blocks[0])])
        self.s8 = nn.Sequential(Block(channels4, channels8, 2), *[Block(channels8, channels8) for _ in range(blocks[1])])
        self.s16 = nn.Sequential(Block(channels8, channels16, 2),
                                 *[Block(channels16, channels16, dilation=2) for _ in range(blocks[2])])
        self.lat16 = conv_bn(channels16, channels8, 1)
        self.lat8 = conv_bn(channels8, channels4, 1)
        self.head = nn.Sequential(Block(channels4, head), nn.Conv2d(head, OUTPUT_CHANNELS, 1))
        nn.init.constant_(self.head[-1].bias, 0.0)
        with torch.no_grad():
            self.head[-1].bias[0] = HEATMAP_PRIOR_LOGIT

    def forward(self, frames):
        """The output (N, 5, H/4, W/4) for the input (N, 4, H, W): the stride 16 and 8 maps are brought up and added
        to the finer ones before the head."""
        stride4 = self.s4(self.stem(frames))
        stride8 = self.s8(stride4)
        stride16 = self.s16(stride8)
        up8 = stride8 + F.interpolate(self.lat16(stride16), scale_factor=2.0, mode="nearest")
        up4 = stride4 + F.interpolate(self.lat8(up8), scale_factor=2.0, mode="nearest")
        return self.head(up4)


def build(config):
    """A new, untrained Detector of the shape a training config's "model" section gives."""
    model = config["model"]
    return Detector(tuple(model["widths"]), tuple(model["blocks"]), model["head"], model.get("stem", "conv"))


def prepare(rgb, fixed):
    """uint8 RGB (N, H, W, 3) and 0/1 fixed map (N, H, W) tensors to the network input (N, 4, H, W) float."""
    frames = rgb.permute(0, 3, 1, 2).float() / 255.0
    return torch.cat([frames, fixed[:, None].float()], dim=1)


def decode(out, threshold=0.3, max_detections=MAX_DETECTIONS):
    """Network output to detections, a tensor (n, 5) per image of cx, cy, w, h (input px) and score: each heatmap peak
    whose score is over `threshold`, the strongest `max_detections` at most."""
    heat = torch.sigmoid(out[:, 0:1].float())
    peak = (heat == F.max_pool2d(heat, PEAK_WINDOW, 1, 1)) & (heat > threshold)
    detections = []
    for image in range(out.shape[0]):
        ys, xs = torch.nonzero(peak[image, 0], as_tuple=True)
        scores = heat[image, 0, ys, xs]
        if len(scores) > max_detections:
            scores, strongest = scores.topk(max_detections)
            ys, xs = ys[strongest], xs[strongest]
        cells = out[image, :, ys, xs].float()
        center_x = (xs.float() + cells[1]) * STRIDE
        center_y = (ys.float() + cells[2]) * STRIDE
        detections.append(torch.stack([center_x, center_y, cells[3].exp(), cells[4].exp(), scores], dim=1))
    return detections
