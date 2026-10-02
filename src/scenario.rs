//! What a scenario file (.sce) says about a run (python/review.py: `scenario_facts`, `target_counts`): its kind, its
//! time limit, and how many targets are alive at once. Read from the file's part before "[Map Data]".

use serde::Serialize;

/// The kinds of run the review tells apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Static,
    Dynamic,
    Tracking,
    Switching,
}

/// A scenario's facts: its kind, its time limit in seconds, and its targets alive at once (one per bot added).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Facts {
    pub kind: Kind,
    pub limit: Option<f64>,
    pub targets: Option<usize>,
}

/// The file's part the facts come from: everything before "[Map Data]".
pub fn header(text: &str) -> &str {
    text.split("[Map Data]").next().unwrap_or("")
}

/// Python's `re.search(r"^key(.*)$", text, re.M)`: the rest of the first line starting with the key (with any "\r").
fn line_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.split('\n').find_map(|line| line.strip_prefix(key))
}

/// Python's `re.search(r"^key(pattern)", text, re.M)`: the first line starting with the key whose value `read`
/// accepts (a line where the pattern finds nothing does not stop the search).
fn first_line<'a, T>(text: &'a str, key: &str, read: impl Fn(&'a str) -> Option<T>) -> Option<T> {
    text.split('\n').filter_map(|line| line.strip_prefix(key)).find_map(read)
}

/// The `\w+` run at the start of a value.
fn word(value: &str) -> Option<&str> {
    let end = value.char_indices().find(|&(_, c)| !is_word(c)).map_or(value.len(), |(i, _)| i);
    (end > 0).then(|| &value[..end])
}

/// Python's `\w`: letters, digits and the underscore, any script.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The first `[-\d.]+` run at the start of a value, as `float()` reads it.
fn number(value: &str, allow_minus: bool) -> Option<f64> {
    let end = value
        .char_indices()
        .find(|&(_, c)| !(c.is_ascii_digit() || c == '.' || (allow_minus && c == '-')))
        .map_or(value.len(), |(i, _)| i);
    if end == 0 { None } else { value[..end].parse().ok() }
}

/// The facts of one scenario file's text. The kind comes from the game's AimTypeTag and AimSubTypeTag; an untagged
/// (older) file is tracking when its first weapon fires fully automatic, static clicking when no bot can move (every
/// MaxSpeed 0), and dynamic clicking otherwise.
pub fn facts(text: &str) -> Facts {
    let t = header(text);
    let tag = line_value(t, "AimTypeTag=").map(str::trim).unwrap_or("");
    let sub = line_value(t, "AimSubTypeTag=").map(str::trim).unwrap_or("");
    let kind = match (tag, sub) {
        ("Tracking", _) => Kind::Tracking,
        ("Target Switching", _) => Kind::Switching,
        ("Clicking", "Static") => Kind::Static,
        ("Clicking", "Dynamic") => Kind::Dynamic,
        _ => {
            let weapons = t.split_once("[Weapon Profile]").map_or(t, |(_, rest)| rest);
            let category = first_line(weapons, "Category=", word);
            let speeds: Vec<f64> = characters(t)
                .filter(|c| line_value(c, "Name=Player").is_none())
                .flat_map(|c| c.split('\n').filter_map(|l| l.strip_prefix("MaxSpeed=")).filter_map(|v| number(v, true)))
                .collect();
            if category == Some("FullyAuto") {
                Kind::Tracking
            } else if !speeds.is_empty() && speeds.iter().copied().fold(f64::MIN, f64::max) == 0.0 {
                Kind::Static
            } else {
                Kind::Dynamic
            }
        }
    };
    let limit = first_line(t, "Timelimit=", |v| number(v, false));
    let targets = line_value(t, "AddedBots=").map(|v| v.trim().split(';').filter(|b| !b.is_empty()).count());
    Facts { kind, limit, targets }
}

/// Python's `re.split(r"\r?\n(?=\[Character Profile\])", t)[1:]`: each character profile, from its heading to the
/// next profile (or the end).
fn characters(t: &str) -> impl Iterator<Item = &str> {
    let mut starts: Vec<usize> = t.match_indices("\n[Character Profile]").map(|(i, _)| i + 1).collect();
    starts.push(t.len() + 1);
    let last = starts.len() - 2;
    (0..starts.len() - 1).map(move |k| {
        let end = starts[k + 1] - 1;
        // the "\r" before a newline that splits them goes with the split; the end of the text keeps its own
        let end = if k < last && t[..end].ends_with('\r') { end - 1 } else { end };
        &t[starts[k]..end.max(starts[k])]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_tags_the_limit_and_the_bots() {
        let text = "Name=x\r\nAimTypeTag=Clicking\r\nAimSubTypeTag=Dynamic\r\nTimelimit=60.0\r\nAddedBots=a;b;c;\r\n[Map Data]\r\nTimelimit=1";
        assert_eq!(facts(text), Facts { kind: Kind::Dynamic, limit: Some(60.0), targets: Some(3) });
    }

    #[test]
    fn untagged_files_go_by_the_weapon_and_the_bots() {
        let still = "[Character Profile]\nName=Player\nMaxSpeed=600\n[Character Profile]\nName=Bot\nMaxSpeed=0\n";
        assert_eq!(facts(still).kind, Kind::Static);
        let auto = "[Weapon Profile]\nCategory=FullyAuto\n";
        assert_eq!(facts(auto).kind, Kind::Tracking);
        assert_eq!(facts("nothing").kind, Kind::Dynamic);
    }
}
