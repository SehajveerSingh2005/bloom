//! whatsapp\contacts.json: the names and numbers WhatsApp syncs from the
//! phone's address book, and the groups the user is in. Names, numbers,
//! group subjects and ids only, never message text. A lower-priority source:
//! phones.json and contacts.json always win and are never written from here,
//! and nothing here widens the automatic-reply allow-list. It lives in the
//! session folder, so Unlink removes it with the session.

use serde::{Deserialize, Serialize};
use std::path::Path;

const FILE: &str = "contacts.json";

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
pub struct Book {
    #[serde(default)]
    pub contacts: Vec<Contact>,
    #[serde(default)]
    pub groups: Vec<Group>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Contact {
    pub name: String,
    /// "+<number>".
    pub number: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Group {
    pub subject: String,
    /// "<id>@g.us", the group's chat key.
    pub jid: String,
    /// Member count, when WhatsApp gave the participant list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub members: Option<usize>,
}

/// The book in `dir` (the session folder); missing is empty, unreadable is
/// an error.
pub fn load(dir: &Path) -> Result<Book, String> {
    match std::fs::read_to_string(dir.join(FILE)) {
        Ok(c) => serde_json::from_str(&c).map_err(|_| {
            "whatsapp\\contacts.json is not valid JSON; delete it to sync again".to_string()
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Book::default()),
        Err(e) => Err(format!("Cannot read whatsapp\\contacts.json: {e}")),
    }
}

/// Applies a sync and saves it. `full`: `contacts` is the whole address book
/// and replaces the old one (unless empty: that is a failed sync); otherwise
/// they are added or renamed by number.
/// `groups`, when given, replaces the group list. An unreadable file is left
/// as it is. Returns the saved book.
pub fn update(
    dir: &Path,
    full: bool,
    contacts: Vec<Contact>,
    groups: Option<Vec<Group>>,
) -> Result<Book, String> {
    let mut book = load(dir)?;
    if full && !contacts.is_empty() {
        book.contacts.clear();
    }
    for c in contacts {
        if c.name.trim().is_empty() || !c.number.starts_with('+') {
            continue;
        }
        book.contacts.retain(|o| o.number != c.number);
        book.contacts.push(c);
    }
    book.contacts.sort_by_key(|c| c.name.to_lowercase());
    if let Some(groups) = groups {
        book.groups = groups;
        book.groups.sort_by_key(|g| g.subject.to_lowercase());
    }
    let json = serde_json::to_string_pretty(&book).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = dir.join("contacts.json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, dir.join(FILE)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })?;
    Ok(book)
}

impl Book {
    /// Contacts whose name or number contains every word of `query`.
    pub fn people(&self, query: &str) -> Vec<&Contact> {
        let words: Vec<String> = query
            .to_lowercase()
            .split_whitespace()
            .map(String::from)
            .collect();
        if words.is_empty() {
            return Vec::new();
        }
        self.contacts
            .iter()
            .filter(|c| {
                let hay = format!("{} {}", c.name.to_lowercase(), c.number);
                words.iter().all(|w| hay.contains(w.as_str()))
            })
            .collect()
    }

    /// Groups whose subject contains `query`, ignoring case.
    pub fn groups_named(&self, query: &str) -> Vec<&Group> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return Vec::new();
        }
        self.groups
            .iter()
            .filter(|g| g.subject.to_lowercase().contains(&q))
            .collect()
    }

    pub fn group(&self, jid: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.jid == jid)
    }

    /// The address-book name for "+<number>".
    pub fn name_of(&self, number: &str) -> Option<&str> {
        self.contacts
            .iter()
            .find(|c| c.number == number)
            .map(|c| c.name.as_str())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::testutil::temp_dir;

    pub fn person(name: &str, number: &str) -> Contact {
        Contact {
            name: name.into(),
            number: number.into(),
        }
    }

    pub fn group(subject: &str, jid: &str, members: Option<usize>) -> Group {
        Group {
            subject: subject.into(),
            jid: jid.into(),
            members,
        }
    }

    #[test]
    fn full_syncs_replace_updates_merge_and_groups_replace() {
        let dir = temp_dir();
        assert_eq!(load(&dir).unwrap(), Book::default());
        let book = update(
            &dir,
            true,
            vec![
                person("Sam Lee", "+491"),
                person("Neha", "+492"),
                person("", "+493"),
                person("Lid only", "123@lid"),
            ],
            Some(vec![group("Family", "1@g.us", Some(5))]),
        )
        .unwrap();
        let names: Vec<&str> = book.contacts.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Neha", "Sam Lee"], "nameless and LID-only skipped");
        // An app-state update renames by number and keeps the groups.
        let book = update(&dir, false, vec![person("Samuel Lee", "+491")], None).unwrap();
        assert_eq!(book.name_of("+491"), Some("Samuel Lee"));
        assert_eq!(book.contacts.len(), 2);
        assert_eq!(book.groups.len(), 1);
        // A failed full sync (nothing came) drops no one.
        let book = update(&dir, true, Vec::new(), None).unwrap();
        assert_eq!(book.contacts.len(), 2);
        // A full sync drops who is gone; a new group list replaces the old.
        let book = update(&dir, true, vec![person("Neha", "+492")], Some(vec![])).unwrap();
        assert_eq!(book.contacts, [person("Neha", "+492")]);
        assert!(book.groups.is_empty());
        assert_eq!(load(&dir).unwrap(), book);
        let raw = std::fs::read_to_string(dir.join(FILE)).unwrap();
        assert!(raw.contains(r#""contacts""#) && raw.contains(r#""groups""#));
    }

    #[test]
    fn a_corrupt_file_is_never_overwritten() {
        let dir = temp_dir();
        std::fs::write(dir.join(FILE), "{bad").unwrap();
        assert!(load(&dir).is_err());
        assert!(update(&dir, true, vec![person("Neha", "+492")], None).is_err());
        assert_eq!(std::fs::read_to_string(dir.join(FILE)).unwrap(), "{bad");
    }

    #[test]
    fn lookups_match_words_and_subjects_ignoring_case() {
        let book = Book {
            contacts: vec![person("Neha Sharma", "+491"), person("Sam", "+492")],
            groups: vec![
                group("Family", "1@g.us", None),
                group("Work Friends", "2@g.us", None),
            ],
        };
        assert_eq!(book.people("sharma neha").len(), 1);
        assert_eq!(book.people("+492")[0].name, "Sam");
        assert!(book.people(" ").is_empty());
        assert_eq!(book.groups_named("FAMILY")[0].jid, "1@g.us");
        assert_eq!(book.groups_named("friends")[0].jid, "2@g.us");
        assert_eq!(book.group("2@g.us").unwrap().subject, "Work Friends");
    }
}
