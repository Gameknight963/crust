//! Directive editing is independent of the checked program, as in Roslyn's
//! preprocessor keyword recommenders and directive trivia classifier:
//! https://github.com/dotnet/roslyn/blob/main/src/Workspaces/CSharp/Portable/Classification/Worker_Preprocesser.cs
use super::*;
use std::ops::Range;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

const KEYWORDS: &[&str] = &[
    "include", "define", "undef", "if", "ifdef", "ifndef", "elif", "else", "endif", "pragma",
    "error", "warning", "line",
];

fn line(text: &str, cursor: usize) -> Option<Range<usize>> {
    return crate::preprocess::directives(text)
        .into_iter()
        .find(|range| range.start <= cursor && cursor <= range.end);
}

fn description(name: &str) -> &'static str {
    return match name {
        "begin_c" => {
            "`#pragma crust begin_c` starts a file-scope native C region. Put C headers and C definitions here. Close it with `#pragma crust end_c`."
        }
        "end_c" => {
            "`#pragma crust end_c` closes the native C region. The following source is checked as Crust."
        }
        "crust" | "pragma" => {
            "Crust's native C markers are `#pragma crust begin_c` and `#pragma crust end_c`. Other pragmas are passed to the C compiler."
        }
        "include" => {
            "Includes a file through the selected C compiler's preprocessor. Put C headers such as `<stdio.h>` inside `#pragma crust begin_c` / `#pragma crust end_c`. Quoted headers also search the source file's directory."
        }
        "define" => {
            "Defines an object or function macro. C preprocessing expands it before Crust analysis."
        }
        "undef" => "Removes a macro definition.",
        "if" | "ifdef" | "ifndef" | "elif" | "else" | "endif" => {
            "Selects source during C preprocessing. Inactive branches are excluded from Crust checking."
        }
        "error" | "warning" => {
            "Emits a C preprocessor diagnostic with the text following the directive."
        }
        "line" => "Sets the source filename and line number used by the C preprocessor.",
        _ => "",
    };
}

pub(super) fn complete(text: &str, cursor: usize, uri: &str) -> Option<Value> {
    let directive = line(text, cursor)?;
    if cursor == directive.start {
        return Some(json!([]));
    }
    let mut start = cursor;
    while start > directive.start + 1
        && (text.as_bytes()[start - 1].is_ascii_alphanumeric()
            || text.as_bytes()[start - 1] == b'_')
    {
        start -= 1;
    }
    let before = text[directive.start + 1..start].trim();
    if let Some(headers) = headers(text, cursor, &directive, uri) {
        return Some(headers);
    }
    // No directive keywords inside a trailing comment, quoted string, or macro
    // body. An incomplete include is handled by the header provider above.
    let words: Vec<_> = before.split_whitespace().collect();
    let names: Vec<(&str, &str)> = match words.as_slice() {
        [] => KEYWORDS.iter().map(|name| (*name, *name)).collect(),
        ["pragma"] => vec![
            ("crust", "crust"),
            ("begin_c", "crust begin_c"),
            ("end_c", "crust end_c"),
            ("once", "once"),
            ("pack", "pack"),
        ],
        ["pragma", "crust"] => vec![("begin_c", "begin_c"), ("end_c", "end_c")],
        _ => Vec::new(),
    };
    let prefix = &text[start..cursor];
    let end = cursor
        + text.as_bytes()[cursor..directive.end]
            .iter()
            .take_while(|byte| byte.is_ascii_alphanumeric() || **byte == b'_')
            .count();
    let range = json!({"start": position(text, start), "end": position(text, end)});
    let items: Vec<_> = names
        .into_iter()
        .filter(|(name, _)| name.starts_with(prefix))
        .map(|(name, insertion)| {
            json!({"label": name, "kind": 14, "detail": "C preprocessor directive",
            "documentation": {"kind": "markdown", "value": description(name)},
            "textEdit": {"range": range, "newText": insertion}})
        })
        .collect();
    return Some(json!(items));
}

fn include(text: &str, range: &Range<usize>) -> Option<(usize, char)> {
    let body = text[range.start + 1..range.end].trim_start();
    let rest = body.strip_prefix("include")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim_start();
    let delimiter = rest.chars().next()?;
    if !matches!(delimiter, '"' | '<') {
        return None;
    }
    return Some((range.end - rest.len() + 1, delimiter));
}

fn search_directories(uri: &str, quoted: bool) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    if let Some(path) = uri_path(uri) {
        if quoted && let Some(parent) = path.parent() {
            directories.push(parent.to_owned());
        }
        if let Ok(manifest) = crate::project::Project::discover(path.parent().unwrap_or(&path))
            && let Ok(project) = crate::project::Project::load(&manifest)
        {
            for target in project.targets {
                directories.extend(target.native.include_dirs);
            }
        }
    }
    static SYSTEM: OnceLock<Mutex<HashMap<std::ffi::OsString, Vec<PathBuf>>>> = OnceLock::new();
    let cc =
        std::env::var_os("CC").unwrap_or_else(|| if cfg!(windows) { "clang" } else { "cc" }.into());
    let cache = SYSTEM.get_or_init(Mutex::default);
    let cached = cache.lock().unwrap().get(&cc).cloned();
    let system = cached.unwrap_or_else(|| {
        let mut system = Vec::new();
        if let Ok(output) = Command::new(&cc)
            .args(["-E", "-x", "c", "-v", "-"])
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .output()
        {
            let mut searching = false;
            for line in String::from_utf8_lossy(&output.stderr).lines() {
                if line.contains("search starts here:") {
                    searching = true;
                    continue;
                }
                if line.contains("End of search list.") {
                    searching = false;
                }
                if searching {
                    let path = PathBuf::from(line.trim());
                    if path.is_dir() {
                        system.push(path);
                    }
                }
            }
        }
        cache.lock().unwrap().insert(cc, system.clone());
        return system;
    });
    directories.extend(system);
    return directories;
}

fn headers(text: &str, cursor: usize, directive: &Range<usize>, uri: &str) -> Option<Value> {
    let (start, delimiter) = include(text, directive)?;
    if cursor < start {
        return None;
    }
    let prefix = &text[start..cursor];
    if prefix.contains(['"', '<', '>', '\r', '\n']) {
        return Some(json!([]));
    }
    let (parent, prefix) = prefix.rsplit_once(['/', '\\']).unwrap_or(("", prefix));
    let start = cursor - prefix.len();
    let end = cursor
        + text[cursor..directive.end]
            .chars()
            .take_while(|character| !matches!(character, '"' | '>' | '/' | '\\'))
            .map(char::len_utf8)
            .sum::<usize>();
    let range = json!({"start": position(text, start), "end": position(text, end)});
    let mut items = std::collections::BTreeMap::new();
    for directory in search_directories(uri, delimiter == '"') {
        let Ok(entries) = std::fs::read_dir(directory.join(parent)) else {
            continue;
        };
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !name.starts_with(prefix) || name.starts_with('.') {
                continue;
            }
            let folder = entry.path().is_dir();
            let name = if folder { format!("{name}/") } else { name };
            items.entry(name.clone()).or_insert_with(|| {
                json!({
                    "label": name, "kind": if folder { 19 } else { 17 },
                    "detail": entry.path().to_string_lossy(),
                    "textEdit": {"range": range, "newText": name}
                })
            });
        }
    }
    return Some(json!(items.into_values().collect::<Vec<_>>()));
}

pub(super) fn hover(text: &str, cursor: usize) -> Option<Value> {
    let directive = line(text, cursor)?;
    if cursor == directive.start {
        return Some(Value::Null);
    }
    let word = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let mut start = cursor;
    let mut end = cursor;
    while start > directive.start && word(text.as_bytes()[start - 1]) {
        start -= 1;
    }
    while end < directive.end && word(text.as_bytes()[end]) {
        end += 1;
    }
    let name = &text[start..end];
    let before = text[directive.start + 1..start].trim();
    if !(before.is_empty()
        || before == "pragma" && matches!(name, "crust" | "begin_c" | "end_c")
        || before == "pragma crust" && matches!(name, "begin_c" | "end_c"))
    {
        return Some(Value::Null);
    }
    let description = description(name);
    if description.is_empty() {
        return Some(Value::Null);
    }
    return Some(
        json!({"contents": {"kind": "markdown", "value": description},
        "range": {"start": position(text, start), "end": position(text, end)}}),
    );
}
