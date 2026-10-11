//! Skills (agentskills.io format): `skills/<folder>/SKILL.md` in the data dir,
//! a frontmatter with `name:` and `description:` then Markdown instructions.
//! A skill never grants powers; any script it describes still goes through
//! `run_powershell` and its security tier.

use std::path::{Path, PathBuf};

pub const MAX_SKILLS: usize = 50;
pub const MAX_DESC: usize = 200;
pub const MAX_BYTES: usize = 20 * 1024;
const MAX_SLUG: usize = 40;

#[derive(Clone, Debug, PartialEq)]
pub struct Skill {
    pub folder: String,
    pub name: String,
    pub description: String,
}

pub fn dir(data: &Path) -> PathBuf {
    data.join("skills")
}

/// Makes sure the folder exists, for Reveal.
pub fn ensure(data: &Path) -> Result<PathBuf, String> {
    let d = dir(data);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    Ok(d)
}

fn unquote(v: &str) -> &str {
    let v = v.trim();
    for q in ['"', '\''] {
        if let Some(i) = v.strip_prefix(q).and_then(|s| s.strip_suffix(q)) {
            return i;
        }
    }
    v
}

/// Lenient frontmatter parse: (name, description, body). None without both fields.
pub fn parse(text: &str) -> Option<(String, String, String)> {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let (mut name, mut desc) = (String::new(), String::new());
    let mut closed = false;
    let mut used = 1;
    for l in lines {
        used += 1;
        if l.trim() == "---" {
            closed = true;
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            match k.trim() {
                "name" => name = unquote(v).chars().take(64).collect(),
                "description" => desc = unquote(v).to_string(),
                _ => {}
            }
        }
    }
    if !closed || name.is_empty() || desc.is_empty() {
        return None;
    }
    let body: Vec<&str> = text.lines().skip(used).collect();
    let desc: String = desc.chars().take(MAX_DESC).collect();
    Some((name, desc, body.join("\n").trim().to_string()))
}

/// Lowercase `[a-z0-9-]`, runs collapsed, max 40; empty when nothing usable.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.to_lowercase().chars() {
        let c = if c.is_ascii_alphanumeric() { c } else { '-' };
        if c == '-' && (out.is_empty() || out.ends_with('-')) {
            continue;
        }
        out.push(c);
    }
    out.truncate(MAX_SLUG);
    out.trim_end_matches('-').to_string()
}

fn read_skill(folder: &Path) -> Option<(Skill, String)> {
    let file = folder.join("SKILL.md");
    // list() runs every request: never read an oversized file.
    if std::fs::metadata(&file).ok()?.len() > 4 * MAX_BYTES as u64 {
        return None;
    }
    let text = std::fs::read_to_string(file).ok()?;
    let (name, description, body) = parse(&text)?;
    let folder = folder.file_name()?.to_string_lossy().into_owned();
    Some((
        Skill {
            folder,
            name,
            description,
        },
        body,
    ))
}

/// Valid skills, sorted by folder, capped at 50. Unreadable ones are skipped.
pub fn list(data: &Path) -> Vec<Skill> {
    let Ok(rd) = std::fs::read_dir(dir(data)) else {
        return vec![];
    };
    let mut folders: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    folders.sort();
    folders
        .iter()
        .filter_map(|f| read_skill(f).map(|(s, _)| s))
        .take(MAX_SKILLS)
        .collect()
}

pub fn count(data: &Path) -> usize {
    list(data).len()
}

/// Finds a listed skill by name or folder, case-insensitive.
fn find(data: &Path, name: &str) -> Option<(Skill, String)> {
    let n = name.trim().to_lowercase();
    list(data)
        .into_iter()
        .find(|s| s.name.to_lowercase() == n || s.folder.to_lowercase() == n)
        .and_then(|s| read_skill(&dir(data).join(&s.folder)))
}

/// The folder `save` writes to: the matching skill's own folder (by name or
/// folder), else a new `<slug>` folder. `exists` agrees with it.
fn target(data: &Path, name: &str) -> String {
    match find(data, name) {
        Some((s, _)) => s.folder,
        None => slug(name),
    }
}

pub fn exists(data: &Path, name: &str) -> bool {
    dir(data).join(target(data, name)).join("SKILL.md").exists()
}

fn cut(s: &str) -> String {
    if s.len() <= MAX_BYTES {
        return s.to_string();
    }
    let mut end = MAX_BYTES;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[truncated]", &s[..end])
}

fn other_files(root: &Path) -> Vec<String> {
    let mut out = vec![];
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            // file_type does not follow links, so a junction loop cannot hang this.
            let Ok(t) = e.file_type() else { continue };
            let p = e.path();
            if t.is_dir() {
                stack.push(p);
            } else if t.is_file() {
                if let Ok(rel) = p.strip_prefix(root) {
                    let rel = rel.to_string_lossy().replace('\\', "/");
                    if rel != "SKILL.md" {
                        out.push(rel);
                    }
                }
            }
            if out.len() >= 50 {
                out.sort();
                return out;
            }
        }
    }
    out.sort();
    out
}

/// The skill's instructions plus the names of the other files in its folder.
pub fn use_skill(data: &Path, name: &str) -> Result<String, String> {
    let (s, body) = find(data, name).ok_or_else(|| format!("No skill named {name}."))?;
    let mut out = cut(&body);
    let files = other_files(&dir(data).join(&s.folder));
    if !files.is_empty() {
        out.push_str(&format!(
            "\n\nOther files in this skill (read with read_skill_file):\n{}",
            files.join("\n")
        ));
    }
    Ok(out)
}

const NOT_ALLOWED: &str = "That path is not allowed; use a relative file name inside the skill.";

/// A text file inside one skill's folder. Rejects `..`, absolute paths, drive
/// letters and anything whose canonical path leaves the folder.
pub fn read_file(data: &Path, name: &str, file: &str) -> Result<String, String> {
    let (s, _) = find(data, name).ok_or_else(|| format!("No skill named {name}."))?;
    let bad = file.trim().is_empty()
        || file.contains("..")
        || file.contains(':')
        || file.starts_with('/')
        || file.starts_with('\\');
    if bad {
        return Err(NOT_ALLOWED.into());
    }
    let root = dir(data)
        .join(&s.folder)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let full = root
        .join(file)
        .canonicalize()
        .map_err(|_| format!("No file {file} in that skill."))?;
    if !full.starts_with(&root) || !full.is_file() {
        return Err(NOT_ALLOWED.into());
    }
    let bytes = std::fs::read(&full).map_err(|e| e.to_string())?;
    let text = String::from_utf8(bytes).map_err(|_| "That file is not text.".to_string())?;
    Ok(cut(&text))
}

/// Writes `skills/<slug>/SKILL.md` (temp file + rename). Returns the slug.
pub fn save(
    data: &Path,
    name: &str,
    description: &str,
    instructions: &str,
) -> Result<String, String> {
    let slug = slug(name);
    let folder_name = target(data, name);
    if slug.is_empty() {
        return Err("The skill needs a name with letters or digits.".into());
    }
    let desc = description.split_whitespace().collect::<Vec<_>>().join(" ");
    let desc: String = desc.chars().take(MAX_DESC).collect();
    if desc.is_empty() || instructions.trim().is_empty() {
        return Err("The skill needs a description and instructions.".into());
    }
    let folder = dir(data).join(&folder_name);
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let file = folder.join("SKILL.md");
    let tmp = folder.join("SKILL.md.tmp");
    let text = format!(
        "---\nname: {slug}\ndescription: {desc}\n---\n\n{}\n",
        cut(instructions.trim())
    );
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &file).map_err(|e| e.to_string())?;
    Ok(folder_name)
}

/// The "Skills you can use" prompt section; empty when none.
pub fn prompt_section(data: &Path) -> String {
    let skills = list(data);
    if skills.is_empty() {
        return String::new();
    }
    let lines: Vec<String> = skills
        .iter()
        .map(|s| format!("- {}: {}", s.name, s.description))
        .collect();
    format!(
        "\nSkills you can use. When a request matches one, call use_skill(name) first and follow it:\n{}\n",
        lines.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::temp_dir;

    fn put(d: &Path, folder: &str, text: &str) {
        let f = dir(d).join(folder);
        std::fs::create_dir_all(&f).unwrap();
        std::fs::write(f.join("SKILL.md"), text).unwrap();
    }

    #[test]
    fn name_is_capped_at_64_chars() {
        let t = format!("---\nname: {}\ndescription: x\n---\nbody", "n".repeat(100));
        assert_eq!(parse(&t).unwrap().0.len(), 64);
    }

    #[test]
    fn parses_valid_crlf_quoted_and_ignores_other_keys() {
        let t = "---\r\nname: \"Tidy\"\r\nversion: 2\r\ndescription: Clean up\r\n---\r\n# Steps\r\nDo it\r\n";
        let (n, d, b) = parse(t).unwrap();
        assert_eq!((n.as_str(), d.as_str()), ("Tidy", "Clean up"));
        assert_eq!(b, "# Steps\nDo it");
        let long = format!("---\nname: a\ndescription: {}\n---\nx", "d".repeat(300));
        assert_eq!(parse(&long).unwrap().1.len(), MAX_DESC);
    }

    #[test]
    fn missing_fields_or_frontmatter_are_skipped() {
        assert!(parse("---\nname: a\n---\nbody").is_none());
        assert!(parse("---\ndescription: a\n---\nbody").is_none());
        assert!(parse("name: a\ndescription: b\n").is_none());
        assert!(parse("---\nname: a\ndescription: b\nno close").is_none());
        let d = temp_dir();
        put(&d, "bad", "just text");
        put(&d, "ok", "---\nname: ok\ndescription: fine\n---\nhi");
        assert_eq!(count(&d), 1);
    }

    #[test]
    fn slugging() {
        assert_eq!(slug("  Weekly Report: Q3!  "), "weekly-report-q3");
        assert_eq!(slug("../etc"), "etc");
        assert_eq!(slug("!!!"), "");
        assert_eq!(slug(&"a".repeat(80)).len(), 40);
    }

    #[test]
    fn caps_at_50_skills() {
        let d = temp_dir();
        for i in 0..55 {
            put(
                &d,
                &format!("s{i:02}"),
                &format!("---\nname: s{i}\ndescription: x\n---\nb"),
            );
        }
        assert_eq!(list(&d).len(), MAX_SKILLS);
    }

    #[test]
    fn use_skill_lists_other_files_and_read_file_stays_inside() {
        let d = temp_dir();
        put(&d, "tidy", "---\nname: tidy\ndescription: x\n---\nDo it");
        let f = dir(&d).join("tidy");
        std::fs::create_dir_all(f.join("scripts")).unwrap();
        std::fs::write(f.join("scripts/run.ps1"), "echo hi").unwrap();
        std::fs::write(f.join("notes.md"), "n").unwrap();
        std::fs::write(dir(&d).join("secret.txt"), "top secret").unwrap();
        put(&d, "other", "---\nname: other\ndescription: x\n---\nb");
        let out = use_skill(&d, "TIDY").unwrap();
        assert!(
            out.starts_with("Do it") && out.contains("scripts/run.ps1") && out.contains("notes.md")
        );
        assert_eq!(read_file(&d, "tidy", "scripts/run.ps1").unwrap(), "echo hi");
        for bad in [
            "../secret.txt",
            "..\\secret.txt",
            "scripts/../../secret.txt",
            "/etc/passwd",
            "\\windows\\win.ini",
            "C:\\Windows\\win.ini",
            "C:win.ini",
            "",
            "scripts",
            "../other/SKILL.md",
        ] {
            assert!(read_file(&d, "tidy", bad).is_err(), "{bad}");
        }
        assert!(read_file(&d, "nope", "notes.md").is_err());
    }

    #[test]
    fn save_round_trips_and_overwrite_is_detected() {
        let d = temp_dir();
        assert!(!exists(&d, "Weekly Report"));
        let s = save(&d, "Weekly Report", "Make it\nfast", "1. do\n2. done").unwrap();
        assert_eq!(s, "weekly-report");
        assert!(exists(&d, "Weekly Report") && exists(&d, "weekly-report"));
        let out = use_skill(&d, "weekly-report").unwrap();
        assert_eq!(out, "1. do\n2. done");
        assert_eq!(list(&d)[0].description, "Make it fast");
        assert!(save(&d, "!!", "d", "i").is_err());
        // A skill whose name differs from its folder is replaced in place.
        put(
            &d,
            "odd-folder",
            "---\nname: Odd Name\ndescription: x\n---\nold",
        );
        assert!(exists(&d, "odd name"));
        assert_eq!(save(&d, "Odd Name", "y", "new").unwrap(), "odd-folder");
        assert_eq!(use_skill(&d, "odd-name").unwrap(), "new");
        assert_eq!(count(&d), 2);
        assert!(prompt_section(&d).contains("- weekly-report: Make it fast"));
    }
}
