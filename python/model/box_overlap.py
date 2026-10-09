"""How much two boxes overlap, for the scripts that match a detector's boxes to labels (calibrate.py, label_score.py)
and to each other (build_merged_pairs.py). A box is (cx, cy, w, h), in any one unit.
"""


def iou(a, b):
    """The overlap of two boxes (cx, cy, w, h) over their union; 0 when the union is empty."""
    ax0, ay0, ax1, ay1 = a[0] - a[2] / 2, a[1] - a[3] / 2, a[0] + a[2] / 2, a[1] + a[3] / 2
    bx0, by0, bx1, by1 = b[0] - b[2] / 2, b[1] - b[3] / 2, b[0] + b[2] / 2, b[1] + b[3] / 2
    inter = max(0.0, min(ax1, bx1) - max(ax0, bx0)) * max(0.0, min(ay1, by1) - max(ay0, by0))
    union = a[2] * a[3] + b[2] * b[3] - inter
    return inter / union if union > 0 else 0.0
