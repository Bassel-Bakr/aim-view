"""Copies of the checked health-bar crops with the bar painted other colors, with and without its text (REPRODUCE.md
step 1). KovaaK's lets a player pick the bar's color and hide the bot's name in it. Every bar the user recorded is
orange or red, so without these the model would learn that an orange bar is no target, and could still box a green
or white one.

Reads a checked set (checked_data.py's output, data_bars_checked: each crop's targets in `boxes`, the bar's box the
model drew in `fix`). The fill: the colored pixels (channels differing by more than SATURATED) joined to those in the
bar's box, within FILL_REACH_PX sideways and the bar's height up or down, off the targets' own pixels. The text: the
pixels inside the fill that are not its color (text_of). A copy changes each pixel near the fill (within EDGE_PX) by
its share of the fill's color, so the anti-aliased edges blend: its color (its difference from gray) is turned round
the gray axis from the fill's hue to the new one and scaled to the new color's strength, and its brightness moves by
the same share. A gray pixel (the wall, the bar's empty part) has no color to change. A copy without text paints the
text and its edge in the fill's new color. Boxes, target mask and fixed map stay as they are: a color moves no box.

Each train crop gets COPIES copies, each in a different color of PALETTE (none within SAME_HUE_DEG of the fill's own
hue), seeded by the crop's name; when it has text, every other copy hides it, and one more copy hides it in the
fill's own color. Val and test crops are not copied. Each name gets TAG in front (train.py --repeat).
Usage: python python/model/recolor_bars.py [--data test_out/vod_model/data_bars_checked]
       [--out test_out/vod_model/data_bars_recolored]
"""
import argparse
import random
from pathlib import Path

import numpy as np
from scipy import ndimage

ROOT = Path(__file__).resolve().parents[2]
TAG = "chk_barcol"              # put before each copy's name: 10 characters, train.py --repeat's key
CHECKED_TAG = "chk_bars__"      # the checked crops' own tag, taken off before TAG goes on
COPIES = 3                      # recolored copies of each train crop (one more without text when it has text)
SATURATED = 60                  # a pixel whose channels differ by more than this is colored
FILL_REACH_PX = 80              # how far sideways of the bar's box the fill is followed
SEED_MARGIN_PX = 2              # the bar's box, grown by this, gives the fill's first pixels
EDGE_PX = 2                     # pixels this near the fill are blended by their share of its color
MIN_FILL_PX = 6                 # a fill smaller than this is left alone (no copies)
MIN_TEXT_PX = 8                 # enclosed pixels that count as text
TEXT_CLOSE_PX = 2               # gaps in the fill up to twice this wide are closed before looking inside it
FILL_COLOR_DIFF = 25            # inside the bar, a pixel this far from the fill's color (any channel) is text
BOT_COLOR_DIFF = 40             # a pixel in a target's box this near its middle color is the target's
MIN_CHROMA = 20.0               # a fill whose color is weaker than this has no hue to turn
SAME_HUE_DEG = 30.0             # a palette color within this of the fill's own hue is not used
# the new colors, RGB
PALETTE = {
    "green": (60, 190, 70), "blue": (40, 120, 235), "cyan": (30, 200, 220), "yellow": (235, 215, 50),
    "purple": (150, 70, 220), "pink": (235, 90, 180), "red": (215, 40, 40), "white": (240, 240, 240),
    "gray": (140, 140, 140),
}
# the unit vector of gray in RGB: the axis a color turns round to change its hue
GRAY_AXIS = np.ones(3) / np.sqrt(3)


def bot_pixels(rgb, boxes):
    """The targets' own pixels: in each target's box (grown by a pixel), those within BOT_COLOR_DIFF of the middle
    color of its box (a bar crossing the box's top is not the bot's color, so it is no bot pixel)."""
    mask = np.zeros(rgb.shape[:2], bool)
    for tx, ty, tw, th in boxes:
        rows = slice(max(0, int(ty - th / 2) - 1), int(ty + th / 2) + 2)
        cols = slice(max(0, int(tx - tw / 2) - 1), int(tx + tw / 2) + 2)
        middle = rgb[max(0, int(ty - th / 4)):int(ty + th / 4) + 1, max(0, int(tx - tw / 4)):int(tx + tw / 4) + 1]
        color = np.median(middle.reshape(-1, 3).astype(float), 0)
        mask[rows, cols] |= np.abs(rgb[rows, cols] - color).max(2) < BOT_COLOR_DIFF
    return mask


def fill_of(rgb, bar, boxes):
    """(the bar's fill, the area searched) as masks. The fill: the colored pixels connected to those in the bar's box
    (`bar`: cx, cy, w, h in crop px), within the area searched. That area: near the bar's box and off the targets'
    own pixels."""
    cx, cy, width, height = bar[:4]
    near = np.zeros(rgb.shape[:2], bool)
    reach_x, reach_y = max(FILL_REACH_PX, 4 * width), max(height, 6)
    near[max(0, int(cy - reach_y)):int(cy + reach_y) + 1, max(0, int(cx - reach_x)):int(cx + reach_x) + 1] = True
    near &= ~bot_pixels(rgb, boxes)
    colored = (rgb.max(2).astype(int) - rgb.min(2)) > SATURATED
    parts, _ = ndimage.label(colored & near, structure=np.ones((3, 3)))
    seed = np.zeros_like(near)
    seed[max(0, int(cy - height / 2) - SEED_MARGIN_PX):int(cy + height / 2) + SEED_MARGIN_PX + 1,
         max(0, int(cx - width / 2) - SEED_MARGIN_PX):int(cx + width / 2) + SEED_MARGIN_PX + 1] = True
    picked = np.unique(parts[seed & (parts > 0)])
    return np.isin(parts, picked) & (parts > 0), near


def turned(fill_color, new_color):
    """The angle (radians) that turns the fill's color round the gray axis onto the new one, and the new color's
    strength over the fill's (0 for white or gray)."""
    fill_chroma = fill_color - fill_color.mean()
    new_chroma = np.asarray(new_color, float) - np.mean(new_color)
    angle = np.arctan2(GRAY_AXIS @ np.cross(fill_chroma, new_chroma), fill_chroma @ new_chroma)
    return angle, np.linalg.norm(new_chroma) / np.linalg.norm(fill_chroma)


def paint(rgb, fill, near, new_color):
    """A copy of rgb with the fill and its edge moved from the fill's color to new_color."""
    pixels = rgb.astype(float)
    fill_color = np.median(pixels[fill], 0)
    fill_chroma_size = np.linalg.norm(fill_color - fill_color.mean())
    angle, strength = turned(fill_color, new_color)
    edge = ndimage.binary_dilation(fill, iterations=EDGE_PX) & near
    chosen = pixels[edge]
    gray = chosen.mean(1, keepdims=True)
    chroma = chosen - gray
    share = np.clip(np.linalg.norm(chroma, axis=1, keepdims=True) / fill_chroma_size, 0, 1)
    rotated = chroma * np.cos(angle) + np.cross(GRAY_AXIS, chroma) * np.sin(angle)
    out = pixels.copy()
    out[edge] = gray + share * (np.mean(new_color) - fill_color.mean()) + strength * rotated
    return np.clip(np.round(out), 0, 255).astype(np.uint8)


def text_of(rgb, fill):
    """The bot's name written in the bar, as a mask: inside the fill (its gaps up to twice TEXT_CLOSE_PX wide closed,
    as a letter can touch the bar's edge), every pixel not the fill's own color, the letters' anti-aliased edges with
    them. None when the fill encloses fewer than MIN_TEXT_PX pixels."""
    inside = ndimage.binary_fill_holes(ndimage.binary_closing(fill, iterations=TEXT_CLOSE_PX))
    if (inside & ~fill).sum() < MIN_TEXT_PX:
        return None
    fill_color = np.median(rgb[fill].astype(float), 0)
    return inside & (np.abs(rgb - fill_color).max(2) >= FILL_COLOR_DIFF)


def hide_text(rgb, fill, text):
    """rgb with the text painted the fill's color (the median of the fill as it is in rgb)."""
    out = rgb.copy()
    out[text] = np.median(rgb[fill & ~text], 0).astype(np.uint8)
    return out


def hue_deg(color):
    """An RGB color's hue in degrees, 0 to 360: the angle of its difference from gray round the gray axis, from red."""
    chroma = np.asarray(color, float) - np.mean(color)
    reference = np.array([1.0, -0.5, -0.5])          # red
    return np.degrees(np.arctan2(GRAY_AXIS @ np.cross(reference, chroma), reference @ chroma)) % 360


def colors_for(name, fill_color):
    """COPIES palette colors for one crop, none the fill's own hue, picked by the crop's name."""
    own = hue_deg(fill_color)
    choices = [color for color, value in PALETTE.items()
               if np.ptp(value) < SATURATED or abs((hue_deg(value) - own + 180) % 360 - 180) > SAME_HUE_DEG]
    return random.Random(name).sample(choices, COPIES)


def copies_of(path):
    """One crop as loaded, and its copies as (suffix, rgb) pairs; no copies when its fill is too small or has no
    color."""
    crop = np.load(path, allow_pickle=True)
    rgb = crop["rgb"]
    fill, near = fill_of(rgb, crop["fix"], crop["boxes"])
    if fill.sum() < MIN_FILL_PX:
        return crop, []
    fill_color = np.median(rgb[fill].astype(float), 0)
    if np.linalg.norm(fill_color - fill_color.mean()) < MIN_CHROMA:
        return crop, []
    text = text_of(rgb, fill)
    out = []
    for k, color in enumerate(colors_for(path.name, fill_color)):
        painted = paint(rgb, fill, near, PALETTE[color])
        hidden = text is not None and k % 2 == 1
        out.append((f"{color}_notext" if hidden else color, hide_text(painted, fill, text) if hidden else painted))
    if text is not None:
        out.append(("notext", hide_text(rgb, fill, text)))
    return crop, out


def main():
    """Writes the copies of every train crop of --data into --out/train/ and prints the counts."""
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--data", type=Path, default=ROOT / "test_out" / "vod_model" / "data_bars_checked")
    parser.add_argument("--out", type=Path, default=ROOT / "test_out" / "vod_model" / "data_bars_recolored")
    args = parser.parse_args()
    (args.out / "train").mkdir(parents=True, exist_ok=True)
    crops = written = with_text = 0
    for path in sorted((args.data / "train").glob("*.npz")):
        crop, copies = copies_of(path)
        crops += 1
        with_text += any(suffix.endswith("notext") for suffix, _ in copies)
        stem = path.stem.removeprefix(CHECKED_TAG)
        for suffix, rgb in copies:
            np.savez_compressed(args.out / "train" / f"{TAG}{stem}_{suffix}.npz", **{key: crop[key] for key in crop.files
                                                                                  if key != "rgb"}, rgb=rgb)
            written += 1
    print(f"{written} copies of {crops} crops ({with_text} with text) in {args.out}")


if __name__ == "__main__":
    main()
