"""The parts of a recording's file name as KovOBS names it: "<scenario> - <score> - <stamp>.mp4". The scripts that find
a recording's scenario facts, target count or stats file read its scenario and stamp here.
"""
from pathlib import Path

NAME_SEPARATOR = " - "          # between a KovOBS name's scenario, score and stamp
NAME_SPLITS = 2                 # the score and the stamp split off the end (a scenario's name may hold the separator)


def name_parts(video):
    """A KovOBS recording's [scenario, score, stamp] from its file name (fewer parts when the name has fewer
    separators)."""
    return Path(video).stem.rsplit(NAME_SEPARATOR, NAME_SPLITS)


def scenario_name(video):
    """The scenario of a recording KovOBS named, as written."""
    return name_parts(video)[0]


def scenario_of(video):
    """The scenario of a recording KovOBS named, in lower case (the scenario facts' key)."""
    return scenario_name(video).lower()
