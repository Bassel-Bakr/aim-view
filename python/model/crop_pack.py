"""The training crops packed once per folder: build_data.py writes a crop as one zip of arrays, and reading 64 of them
a batch costs more than the training step (44 ms with 8 worker processes on Windows, against 56 ms for the step). A
folder's crops are decoded once into memory-mapped arrays beside it (<folder>.pack/: the RGB, the fixed map and
target mask 8 pixels a byte, the boxes as train.py reads them), and a batch is read from them with a few gathers on a
thread, ahead of the GPU. A pack is made again when its folder's crops change (their names, count, sizes or times).
train.py's Crops and the packs read a crop alike (read_crop)."""
import json
import os
import queue
import shutil
import threading
from concurrent.futures import ProcessPoolExecutor, ThreadPoolExecutor
from pathlib import Path

import numpy as np
import torch

MAXBOX = 40                     # boxes kept per crop; a batch pads each crop's boxes to this
MAXIGNORE = 8                   # ignore boxes kept per crop, padded the same way
SAME_TARGET_PX = 1.5            # the labeller can report one target twice: two labels this close are one
CROP_PX = 256
MASK_BYTES = CROP_PX * CROP_PX // 8         # a 0/1 map, 8 pixels a byte
PACK_VERSION = 1                # in each pack's stamp: a change to the layout makes every pack again
BUILD_CHUNK = 256               # the crops a packing worker decodes and writes at once
BUILD_WORKERS = 8               # packing processes at most (fewer on a machine with fewer cores)
PREFETCH = 3                    # the batches read ahead of the GPU
# threads reading a batch's RGB rows: 64 rows take 1.8 ms on 8 threads, 5.8 ms on one, and 35 to 45 ms through a
# memory map (a page fault at a time)
READ_THREADS = 8
RGB_BYTES = CROP_PX * CROP_PX * 3
SMALL = ("fixed", "tmask", "boxes", "counts", "ignore")     # kept in memory: about 1.2 GB for 68,000 crops
_READERS = threading.local()                                # each reading thread's open pack files
_POOL = []                                                  # the reading threads' pool, made on first use


def layout(count):
    """A pack's arrays: name, shape and type."""
    return (("rgb", (count, CROP_PX, CROP_PX, 3), np.uint8), ("fixed", (count, MASK_BYTES), np.uint8),
            ("tmask", (count, MASK_BYTES), np.uint8), ("boxes", (count, MAXBOX, 4), np.float32),
            ("counts", (count,), np.int64), ("ignore", (count, MAXIGNORE, 4), np.float32))


def read_crop(file):
    """A crop file's arrays as training reads them: rgb (256, 256, 3) uint8, the fixed map and the target mask
    (256, 256) uint8, up to MAXBOX boxes (cx, cy, w, h), their count, and up to MAXIGNORE ignore boxes."""
    crop = np.load(file)
    keep = []                       # the labeller can report one target twice (overlapping search windows)
    for box in crop["boxes"]:
        if all(np.hypot(box[0] - kept[0], box[1] - kept[1]) > SAME_TARGET_PX for kept in keep):
            keep.append(box)
    boxes = np.zeros((MAXBOX, 4), np.float32)
    count = min(MAXBOX, len(keep))
    if count:
        boxes[:count] = np.array(keep[:count])
    ignore = np.zeros((MAXIGNORE, 4), np.float32)   # "ignore": a target there, but not one to learn (under the
    if "ignore" in crop.files:                      # crosshair); padding has width 0
        ignored = min(MAXIGNORE, len(crop["ignore"]))
        ignore[:ignored] = crop["ignore"][:ignored]
    return crop["rgb"], crop["fixed"], crop["tmask"], boxes, count, ignore


def stamp(files):
    """What tells a pack made from these crop files from one made before they changed."""
    stats = [file.stat() for file in files]
    return dict(version=PACK_VERSION, names=[file.name for file in files], bytes=sum(stat.st_size for stat in stats),
                newest=max((stat.st_mtime_ns for stat in stats), default=0))


def write_chunk(job):
    """A packing worker's share: crops decoded and written into the pack's arrays at their rows."""
    partial, files, start = job
    arrays = {name: np.load(Path(partial) / f"{name}.npy", mmap_mode="r+") for name, _, _ in layout(0)}
    for offset, file in enumerate(files):
        rgb, fixed, mask, boxes, count, ignore = read_crop(file)
        if rgb.shape != (CROP_PX, CROP_PX, 3) or fixed.max() > 1 or mask.max() > 1:
            raise ValueError(f"{file}: not a {CROP_PX} px crop with a 0/1 fixed map and target mask")
        row = start + offset
        arrays["rgb"][row] = rgb
        arrays["fixed"][row] = np.packbits(fixed.reshape(-1))
        arrays["tmask"][row] = np.packbits(mask.reshape(-1))
        arrays["boxes"][row], arrays["counts"][row], arrays["ignore"][row] = boxes, count, ignore
    for array in arrays.values():
        array.flush()


def build(files, pack):
    """The crop files decoded into a pack (made in a .tmp folder, then put in place of the stale one)."""
    partial = pack.with_name(pack.name + ".tmp")
    if partial.exists():
        shutil.rmtree(partial)
    partial.mkdir(parents=True)
    for name, shape, dtype in layout(len(files)):
        np.lib.format.open_memmap(partial / f"{name}.npy", "w+", dtype, shape).flush()
    jobs = [(str(partial), [str(file) for file in files[start:start + BUILD_CHUNK]], start)
            for start in range(0, len(files), BUILD_CHUNK)]
    with ProcessPoolExecutor(min(BUILD_WORKERS, os.cpu_count() or 1)) as pool:
        list(pool.map(write_chunk, jobs))
    (partial / "stamp.json").write_text(json.dumps(stamp(files)))
    if pack.exists():
        shutil.rmtree(pack)         # a cache of crops that changed since; the crops themselves stay
    partial.rename(pack)


def read_rows(job):
    """One RGB row of a pack read into its place in a batch (on a reading thread, with its own file handle)."""
    path, offset, row, out, place = job
    files = _READERS.__dict__.setdefault("files", {})
    if path not in files:
        files[path] = open(path, "rb", buffering=0)     # noqa: SIM115 (kept open for the thread's life)
    file = files[path]
    file.seek(offset + row * RGB_BYTES)
    file.readinto(memoryview(out)[place * RGB_BYTES:(place + 1) * RGB_BYTES])


def reading_pool():
    """The READ_THREADS threads that read RGB rows, shared by every pack and made once."""
    if not _POOL:
        _POOL.append(ThreadPoolExecutor(READ_THREADS))
    return _POOL[0]


class CropPack:
    """One folder's crops, packed (made or made again first when needed): the RGB read from its file a row at a
    time, the rest in memory."""

    def __init__(self, folder):
        """Opens the pack of the folder's crops (<folder>.pack beside it), making it first when it is missing or
        stale. A folder without crops gets no pack."""
        folder = Path(folder)
        self.files = sorted(folder.glob("*.npz"))
        self.rows = {file: row for row, file in enumerate(self.files)}
        self.small, self.rgb_path, self.rgb_offset = {}, None, 0
        if not self.files:
            return
        pack = folder.with_name(folder.name + ".pack")
        made = pack / "stamp.json"
        if not made.is_file() or json.loads(made.read_text()) != stamp(self.files):
            print(f"packing the {len(self.files)} crops of {folder} into {pack}", flush=True)
            build(self.files, pack)
        self.small = {name: np.load(pack / f"{name}.npy") for name in SMALL}
        self.rgb_path = str(pack / "rgb.npy")
        self.rgb_offset = np.load(self.rgb_path, mmap_mode="r").offset


class PackedCrops:
    """The crops of several folders, in train.py Crops' order (sorted by path, then the repeated ones again), read a
    batch at a time from their packs."""

    def __init__(self, folders, repeat=(), times=1):
        """repeat: the name prefixes (a crop name's first 10 characters) whose crops come `times` times in all, or a
        dict of prefix to times."""
        self.packs = [CropPack(folder) for folder in folders]
        where = {file: (k, row) for k, pack in enumerate(self.packs) for file, row in pack.rows.items()}
        files = sorted(where)
        repeats = repeat if isinstance(repeat, dict) else {prefix: times for prefix in repeat}
        files += [file for file in files for _ in range(repeats.get(file.name[:10], 1) - 1)]
        self.pack_of = np.array([where[file][0] for file in files], np.int64)
        self.row_of = np.array([where[file][1] for file in files], np.int64)

    def __len__(self):
        """The crop count, repeats included."""
        return len(self.row_of)

    def batch(self, indices):
        """The crops at these indices as one batch: what DataLoader gives for train.Crops (rgb, fixed, tmask, boxes,
        counts, ignore)."""
        indices = np.asarray(indices, np.int64)
        out = {name: np.empty(shape, dtype) for name, shape, dtype in layout(len(indices))}
        packs, rows = self.pack_of[indices], self.row_of[indices]
        jobs, rgb_bytes = [], out["rgb"].reshape(-1)        # a flat view: rows are placed by byte
        for k in np.unique(packs):
            pack, places = self.packs[k], np.nonzero(packs == k)[0]
            for name in SMALL:
                out[name][places] = pack.small[name][rows[places]]
            jobs += [(pack.rgb_path, pack.rgb_offset, int(rows[place]), rgb_bytes, int(place)) for place in places]
        list(reading_pool().map(read_rows, jobs))
        for name in ("fixed", "tmask"):
            out[name] = np.unpackbits(out[name], axis=1).reshape(len(indices), CROP_PX, CROP_PX)
        return tuple(torch.from_numpy(out[name]) for name in ("rgb", "fixed", "tmask", "boxes", "counts", "ignore"))


class PackLoader:
    """A PackedCrops' batches in the sampler's order (else in order), the last short one dropped with drop_last, read
    ahead on a thread (pinned with pin_memory) while the GPU works on the batch before."""

    def __init__(self, dataset, batch, sampler=None, drop_last=False, pin_memory=False):
        """The arguments as DataLoader takes them: the PackedCrops, the batch size, and an optional sampler."""
        self.dataset, self.batch, self.sampler = dataset, batch, sampler
        self.drop_last, self.pin_memory = drop_last, pin_memory

    def __len__(self):
        """The batch count, the short last one included unless drop_last."""
        count = len(self.sampler) if self.sampler is not None else len(self.dataset)
        return count // self.batch if self.drop_last else -(-count // self.batch)

    def chunks(self):
        """The crop indices of each batch, in order (the sampler draws its order once, here)."""
        order = list(self.sampler) if self.sampler is not None else list(range(len(self.dataset)))
        return [order[start:start + self.batch] for start in range(0, len(order), self.batch)
                if not self.drop_last or start + self.batch <= len(order)]

    def __iter__(self):
        """Yields each batch as the reading thread hands it over, PREFETCH at most waiting. An error on that thread
        is raised here; leaving the loop early stops the thread."""
        ready, stop = queue.Queue(PREFETCH), threading.Event()

        def read():
            """The reading thread: puts each batch in the queue, then None at the end, or the error that stopped
            it."""
            try:
                for chunk in self.chunks():
                    if stop.is_set():
                        return
                    batch = self.dataset.batch(chunk)
                    ready.put(tuple(part.pin_memory() for part in batch) if self.pin_memory else batch)
                ready.put(None)
            except BaseException as error:  # noqa: BLE001 (raised again in the training loop)
                ready.put(error)

        reader = threading.Thread(target=read, daemon=True)
        reader.start()
        try:
            while (item := ready.get()) is not None:
                if isinstance(item, BaseException):
                    raise item
                yield item
        finally:
            stop.set()
            while reader.is_alive():        # a reader waiting to hand over a batch is let go
                try:
                    ready.get_nowait()
                except queue.Empty:
                    reader.join(0.01)
