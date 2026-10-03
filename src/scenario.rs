//! What a scenario file (.sce) says about a run (python/review.py: `scenario_facts`, `target_counts`): its kind, its
//! time limit, how many targets are alive at once, and the player's weapon's ammo rules. Read from the file's part
//! before "[Map Data]".

use serde::{Deserialize, Serialize};

/// The kinds of run the review tells apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Static,
    Dynamic,
    Tracking,
    Switching,
}

/// A scenario's facts: its kind, its time limit in seconds, its targets alive at once (one per bot added), and the
/// player's weapon's ammo rules (none when its magazine never runs out).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Facts {
    pub kind: Kind,
    pub limit: Option<f64>,
    pub targets: Option<usize>,
    pub reload: Option<AmmoRules>,
}

/// The ammo rules of a weapon whose magazine can run out (KovaaK's weapon profile, and the scenario's points for a
/// reload): the magazine's size (MagazineMax), the ammo a shot uses (AmmoPerShot), the ammo a kill puts back, up to a
/// full magazine (AmmoReloadedOnKill), the reload's time in seconds from an empty magazine and from a part-used one
/// (ReloadTimeFromEmpty, ReloadTimeFromPartial), and the points a reload takes off (ScoreLossPerReload).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AmmoRules {
    pub magazine: i64,
    pub per_shot: i64,
    pub on_kill: i64,
    pub from_empty: f64,
    pub from_partial: f64,
    pub score_loss: f64,
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
    Facts { kind, limit, targets, reload: ammo_rules(t) }
}

/// The trimmed value of the first line starting with the key.
fn value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    line_value(text, key).map(str::trim)
}

/// The profile under a heading ("Weapon Profile]") with this name (any case), from its heading to the next heading.
fn profile<'a>(t: &'a str, heading: &str, name: &str) -> Option<&'a str> {
    t.split("\n[")
        .filter_map(|p| p.strip_prefix(heading))
        .find(|p| value(p, "Name=").is_some_and(|n| n.eq_ignore_ascii_case(name)))
}

/// The ammo rules of the weapon the player holds at the start, as KovaaK picks it: the scenario's PlayerProfile names
/// the player's character profile (in any case), whose first weapon in WeaponProfileNames is the one held. None when
/// its magazine never runs out (MagazineMax or AmmoPerShot 0, as in most scenarios), when a profile is missing, or when
/// the weapon reloads a round at a time (UseIncReload: no scenario here uses it, so its timing is not guessed).
fn ammo_rules(t: &str) -> Option<AmmoRules> {
    let top = t.split("\n[").next().unwrap_or("");
    let character = profile(t, "Character Profile]", value(top, "PlayerProfile=")?)?;
    let held = value(character, "WeaponProfileNames=")?.split(';').map(str::trim).find(|w| !w.is_empty())?;
    let weapon = profile(t, "Weapon Profile]", held)?;
    let read = |text: &str, key: &str| value(text, key).and_then(|v| number(v, true));
    let (magazine, per_shot) = (read(weapon, "MagazineMax=")? as i64, read(weapon, "AmmoPerShot=")? as i64);
    if magazine <= 0 || per_shot <= 0 || value(weapon, "UseIncReload=").is_some_and(|v| v.eq_ignore_ascii_case("true")) {
        return None;
    }
    let from_empty = read(weapon, "ReloadTimeFromEmpty=")?;
    Some(AmmoRules {
        magazine,
        per_shot,
        on_kill: read(weapon, "AmmoReloadedOnKill=").map_or(0, |v| v as i64),
        from_empty,
        from_partial: read(weapon, "ReloadTimeFromPartial=").unwrap_or(from_empty),
        score_loss: read(top, "ScoreLossPerReload=").unwrap_or(0.0),
    })
}

/// Python's `re.split(r"\r?\n(?=\[Character Profile\])", t)[1:]`: each character profile, from its heading to the
/// next profile (or the end).
fn characters(t: &str) -> impl Iterator<Item = &str> {
    let mut starts: Vec<usize> = t.match_indices("\n[Character Profile]").map(|(i, _)| i + 1).collect();
    starts.push(t.len() + 1);
    // with no profile there is nothing to cut, and no last one
    let last = starts.len().saturating_sub(2);
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

    /// An untagged file with no character profile: dynamic clicking (no bot's speed is known), without a panic.
    #[test]
    fn reads_a_file_with_no_characters() {
        assert_eq!(facts("Name=x\r\nTimelimit=60\r\n").kind, Kind::Dynamic);
        assert_eq!(characters("Name=x").count(), 0);
    }

    #[test]
    fn reads_the_tags_the_limit_and_the_bots() {
        let text = "Name=x\r\nAimTypeTag=Clicking\r\nAimSubTypeTag=Dynamic\r\nTimelimit=60.0\r\nAddedBots=a;b;c;\r\n[Map Data]\r\nTimelimit=1";
        assert_eq!(facts(text), Facts { kind: Kind::Dynamic, limit: Some(60.0), targets: Some(3), reload: None });
    }

    /// An excerpt of "1w2ts reload.sce" (its user's scenarios folder): the player holds BB Gun, a magazine of 3 that a
    /// kill fills again. Its other weapon, a bot's, never runs out (MagazineMax 0).
    const RELOAD: &str = "Name=1w2ts reload\r\nPlayerCharacters=Player\r\nTimelimit=60.0\r\nPlayerProfile=Player\r\n\
        ScoreLossPerMiss=0.0\r\nScoreLossPerReload=0.0\r\nAimTypeTag=Clicking\r\nAimSubTypeTag=Static\r\n\r\n\
        [Bot Profile]\r\nName=target\r\nWeaponsProfileNames=\r\n\r\n\
        [Character Profile]\r\nName=Player\r\nMaxHealth=100.0\r\nWeaponProfileNames=BB Gun;;;;;;;\r\nAmmoRegainedOnKill=0\r\n\r\n\
        [Character Profile]\r\nName=target\r\nWeaponProfileNames=;;;;;;;\r\n\r\n\
        [Weapon Profile]\r\nName=BB Gun\r\nType=Hitscan\r\nShotsPerClick=1\r\nCategory=SemiAuto\r\nCooldownType=InfiniteUse\r\n\
        MagazineMax=3\r\nReloadTimeFromEmpty=0.5\r\nReloadTimeFromPartial=0.5\r\nAmmoPerShot=1\r\nAmmoReloadedOnKill=4\r\n\
        CancelReloadOnKill=false\r\nUseIncReload=false\r\nIncReloadStartupTime=0.1\r\nIncReloadLoopTime=0.1\r\n\r\n\
        [Weapon Profile]\r\nName=explode250ms\r\nType=Hitscan\r\nMagazineMax=0\r\nReloadTimeFromEmpty=0.5\r\n\
        ReloadTimeFromPartial=0.5\r\nAmmoPerShot=1\r\nAmmoReloadedOnKill=0\r\nCancelReloadOnKill=false\r\nUseIncReload=false\r\n\r\n\
        [Map Data]\r\nMagazineMax=9\r\n";

    #[test]
    fn reads_the_player_weapons_ammo_rules() {
        let rules = AmmoRules { magazine: 3, per_shot: 1, on_kill: 4, from_empty: 0.5, from_partial: 0.5, score_loss: 0.0 };
        assert_eq!(facts(RELOAD).reload, Some(rules));
        // the player's character named in another case still holds it
        assert!(facts(&RELOAD.replace("PlayerProfile=Player", "PlayerProfile=player")).reload.is_some());
        // a weapon whose magazine never runs out, one that reloads a round at a time, a missing weapon: none
        assert_eq!(facts(&RELOAD.replace("BB Gun;", "explode250ms;")).reload, None);
        assert_eq!(facts(&RELOAD.replace("UseIncReload=false", "UseIncReload=true")).reload, None);
        assert_eq!(facts(&RELOAD.replace("BB Gun;", "Gone;")).reload, None);
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
