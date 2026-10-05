//! The code map (CODEMAP.md), built from the Rust sources so a reader finds where a thing lives with one read or one
//! search instead of opening files: each file's header comment, then its public types with the first sentence of
//! their doc, their methods, and the file's functions and constants by name. The test fails when CODEMAP.md is out of
//! date; `CODE_MAP_WRITE=1 cargo test --profile quick --test code_map` writes it again.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// The folders mapped, from the repository's root: every crate's sources, then the tests, examples and benches.
const FOLDERS: [&str; 10] = [
    "src",
    "service/src",
    "server/src",
    "desktop/src",
    "browser-service/src",
    "tests",
    "examples",
    "service/examples",
    "desktop/examples",
    "benches",
];
const MAP_FILE: &str = "CODEMAP.md";
/// Set to write CODEMAP.md instead of comparing with it.
const WRITE_VAR: &str = "CODE_MAP_WRITE";
/// The map's line width, as the hand-formatted sources'.
const WIDTH: usize = 120;
/// Words before a function's `fn`.
const QUALIFIERS: [&str; 3] = ["const ", "async ", "unsafe "];
const INTRO: &str = "Where each thing lives in the Rust code: each file's header comment, then its public types \
(with the first sentence of their doc and their methods), functions and constants. tests/code_map.rs builds it from \
the sources and fails when this file is out of date; `CODE_MAP_WRITE=1 cargo test --profile quick --test code_map` \
writes it again. Who calls what is rust-analyzer's call hierarchy.";

/// One public type: what kind it is (struct, enum, trait, type), its name, the first sentence of its doc and its
/// public methods.
struct PublicType {
    kind: String,
    name: String,
    doc: String,
    methods: Vec<String>,
}

/// The public methods of a type another file declares.
struct ForeignMethods {
    owner: String,
    names: Vec<String>,
}

/// A public method found in an `impl` block: its type and its name.
struct Method {
    owner: String,
    name: String,
}

/// What the map says about one file.
#[derive(Default)]
struct FileMap {
    path: String,
    summary: String,
    types: Vec<PublicType>,
    foreign: Vec<ForeignMethods>,
    functions: Vec<String>,
    constants: Vec<String>,
}

impl FileMap {
    /// Adds a top-level public item, from what follows its `pub `; `above` is the file's lines before it.
    fn add_item(&mut self, rest: &str, above: &[&str]) {
        if let Some(name) = function_name(rest) {
            push_new(&mut self.functions, name);
            return;
        }
        let Some((kind, after)) = rest.split_once(' ') else { return };
        let name = identifier(after);
        match kind {
            "struct" | "enum" | "trait" | "type" => self.types.push(PublicType {
                kind: kind.to_string(),
                name,
                doc: first_sentence(&doc_above(above)),
                methods: Vec::new(),
            }),
            "const" | "static" => push_new(&mut self.constants, name),
            _ => {}
        }
    }

    /// Puts each method under its type, or under the methods of a type another file declares.
    fn attach(&mut self, methods: Vec<Method>) {
        for method in methods {
            if let Some(owner) = self.types.iter_mut().find(|item| item.name == method.owner) {
                push_new(&mut owner.methods, method.name);
            } else if let Some(owner) = self.foreign.iter_mut().find(|item| item.owner == method.owner) {
                push_new(&mut owner.names, method.name);
            } else {
                self.foreign.push(ForeignMethods { owner: method.owner, names: vec![method.name] });
            }
        }
    }
}

fn push_new(names: &mut Vec<String>, name: String) {
    if !names.contains(&name) {
        names.push(name);
    }
}

/// Reads one file's map from its text. The sources are hand-formatted: items start at column 0, methods at 4.
fn map_file(path: String, text: &str) -> FileMap {
    let lines: Vec<&str> = text.lines().collect();
    let summary = first_paragraph(lines.iter().map_while(|line| line.strip_prefix("//!")));
    let mut map = FileMap { path, summary, ..FileMap::default() };
    let mut owner: Option<String> = None;
    let mut methods = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if line.starts_with('}') {
            owner = None;
        } else if line.strip_prefix("impl").is_some_and(|rest| rest.starts_with([' ', '<'])) {
            owner = inherent_owner(line);
        } else if let Some(rest) = line.strip_prefix("    pub ") {
            if let (Some(type_name), Some(name)) = (&owner, function_name(rest)) {
                methods.push(Method { owner: type_name.clone(), name });
            }
        } else if let Some(rest) = line.strip_prefix("pub ") {
            map.add_item(rest, &lines[..index]);
        }
    }
    map.attach(methods);
    map
}

/// The type an inherent `impl` block belongs to; none for a trait's (its methods are the trait's).
fn inherent_owner(line: &str) -> Option<String> {
    let rest = after_generics(line.strip_prefix("impl")?.trim_start()).trim_start();
    let head = rest.split('{').next().unwrap_or(rest);
    if head.contains(" for ") {
        return None;
    }
    let path = head.split(|ch: char| ch == '<' || ch.is_whitespace()).next()?;
    let name = path.rsplit("::").next()?;
    (!name.is_empty()).then(|| name.to_string())
}

/// What follows a leading `<...>`, nested brackets included.
fn after_generics(text: &str) -> &str {
    if !text.starts_with('<') {
        return text;
    }
    let mut depth = 0;
    for (index, ch) in text.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    return &text[index + 1..];
                }
            }
            _ => {}
        }
    }
    ""
}

/// A function's name from what follows its `pub `, or none when it is not a function.
fn function_name(rest: &str) -> Option<String> {
    let mut text = rest;
    while let Some(next) = QUALIFIERS.iter().find_map(|qualifier| text.strip_prefix(qualifier)) {
        text = next;
    }
    text.strip_prefix("fn ").map(identifier)
}

fn identifier(text: &str) -> String {
    text.chars().take_while(|ch| ch.is_alphanumeric() || *ch == '_').collect()
}

/// The first paragraph of the `///` doc right above an item (its attributes skipped), joined into one line.
fn doc_above(above: &[&str]) -> String {
    let mut doc: Vec<&str> = above
        .iter()
        .rev()
        .skip_while(|line| line.starts_with("#["))
        .map_while(|line| line.strip_prefix("///"))
        .collect();
    doc.reverse();
    first_paragraph(doc.into_iter())
}

fn first_paragraph<'a>(lines: impl Iterator<Item = &'a str>) -> String {
    let words: Vec<&str> = lines.map(str::trim).take_while(|line| !line.is_empty()).collect();
    words.join(" ")
}

/// Up to the first full stop outside brackets and code, which ends the sentence.
fn first_sentence(text: &str) -> String {
    let mut depth = 0;
    let mut in_code = false;
    for (index, ch) in text.char_indices() {
        match ch {
            '`' => in_code = !in_code,
            '(' | '[' if !in_code => depth += 1,
            ')' | ']' if !in_code => depth -= 1,
            '.' if !in_code && depth == 0 && text[index + 1..].starts_with(' ') => {
                return text[..=index].to_string();
            }
            _ => {}
        }
    }
    text.to_string()
}

/// The text's words in lines of at most WIDTH characters, the first after `first`, the rest after `rest`.
fn wrap(text: &str, first: &str, rest: &str) -> String {
    let mut out = String::new();
    let mut line = first.to_string();
    let mut empty = true;
    for word in text.split_whitespace() {
        if !empty && line.chars().count() + 1 + word.chars().count() > WIDTH {
            out.push_str(&line);
            out.push('\n');
            line = rest.to_string();
            empty = true;
        }
        if !empty {
            line.push(' ');
        }
        line.push_str(word);
        empty = false;
    }
    out.push_str(&line);
    out.push('\n');
    out
}

fn code_names(names: &[String]) -> String {
    names.iter().map(|name| format!("`{name}`")).collect::<Vec<_>>().join(", ")
}

fn type_line(item: &PublicType) -> String {
    let mut line = format!("`{}` ({})", item.name, item.kind);
    if !item.doc.is_empty() {
        line.push_str(": ");
        line.push_str(&item.doc);
        if !item.doc.ends_with(['.', '!', '?', ':']) {
            line.push('.');
        }
    }
    if !item.methods.is_empty() {
        line.push_str(&format!(" Methods: {}.", code_names(&item.methods)));
    }
    line
}

fn render_file(map: &FileMap) -> String {
    let mut out = format!("\n## {}\n", map.path);
    if !map.summary.is_empty() {
        out.push('\n');
        out.push_str(&wrap(&map.summary, "", ""));
    }
    let mut items: Vec<String> = map.types.iter().map(type_line).collect();
    for other in &map.foreign {
        items.push(format!("`{}` methods: {}.", other.owner, code_names(&other.names)));
    }
    if !map.functions.is_empty() {
        items.push(format!("Functions: {}.", code_names(&map.functions)));
    }
    if !map.constants.is_empty() {
        items.push(format!("Constants: {}.", code_names(&map.constants)));
    }
    if !items.is_empty() {
        out.push('\n');
    }
    for item in items {
        out.push_str(&wrap(&item, "- ", "  "));
    }
    out
}

/// The `.rs` files under a folder, at any depth.
fn collect(folder: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

/// Every file's map, from the sources under the repository's root, in FOLDERS' order.
fn file_maps(root: &Path) -> Vec<FileMap> {
    let mut maps = Vec::new();
    for folder in FOLDERS {
        let mut files = Vec::new();
        collect(&root.join(folder), &mut files);
        files.sort();
        for file in files {
            let text = fs::read_to_string(&file).expect("a source file reads");
            let relative = file.strip_prefix(root).expect("under the root").to_string_lossy().replace('\\', "/");
            maps.push(map_file(relative, &text));
        }
    }
    maps
}

/// The whole map: the files with the first sentence of each one's header, then each file in full.
fn code_map(root: &Path) -> String {
    let maps = file_maps(root);
    let mut out = format!("# Code map\n\n{}\n## Files\n\n", wrap(INTRO, "", ""));
    for map in &maps {
        out.push_str(&wrap(&format!("`{}`: {}", map.path, first_sentence(&map.summary)), "- ", "  "));
    }
    for map in &maps {
        out.push_str(&render_file(map));
    }
    out
}

#[test]
fn the_code_map_is_current() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let map = code_map(root);
    let path = root.join(MAP_FILE);
    if env::var_os(WRITE_VAR).is_some() {
        fs::write(&path, &map).expect("CODEMAP.md writes");
        return;
    }
    let kept = fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
    assert!(
        kept == map,
        "CODEMAP.md is out of date: run `CODE_MAP_WRITE=1 cargo test --profile quick --test code_map`, then read its diff"
    );
}

#[test]
fn a_file_reads_into_its_map() {
    let text = "//! What it does.\n//! Where from.\n//!\n//! More.\n\n/// A thing (it has parts). And more.\n\
                #[derive(Debug)]\npub struct Thing<T> {\n}\n\nimpl<T: Clone> Thing<T> {\n    pub const fn new() {}\n    \
                fn hidden() {}\n}\n\nimpl Default for Thing<u8> {\n    pub fn default() {}\n}\n\nimpl other::Far {\n    \
                pub async fn go() {}\n}\n\npub fn helper() {}\npub(crate) fn inside() {}\npub const LIMIT: usize = 2;\n";
    let map = map_file("a.rs".to_string(), text);
    assert_eq!(
        render_file(&map),
        "\n## a.rs\n\nWhat it does. Where from.\n\n- `Thing` (struct): A thing (it has parts). Methods: `new`.\n\
         - `Far` methods: `go`.\n- Functions: `helper`.\n- Constants: `LIMIT`.\n"
    );
}
