//! people.json: the canonical contacts. One record per person with any number
//! of emails and phones, each remembering where it came from (`source`:
//! "user", "mail", "whatsapp" or "outlook"), plus free-form tags.
//!
//! Built on first load from the old contacts.json (name to one email) and
//! phones.json (name to one number), which stay on disk untouched as a
//! backup. Every write goes through `update`, which validates every person
//! (more than one email needs a tag) and never overwrites a file it could not
//! read. The mail harvest and the WhatsApp address book are lower-priority
//! layers merged in at lookup time (`merge`), so Unlink and a deleted cache
//! take their names with them.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

const FILE: &str = "people.json";
const CORRUPT: &str =
    "people.json is not valid JSON; fix it, or delete it to rebuild from contacts.json and phones.json";

pub const USER: &str = "user";
pub const MAIL: &str = "mail";
pub const WHATSAPP: &str = "whatsapp";
pub const OUTLOOK: &str = "outlook";
const SOURCES: [&str; 4] = [USER, MAIL, WHATSAPP, OUTLOOK];

/// Score scale for `score` (0 to 1): resolve at or above `SURE` with a lead of
/// `MARGIN`; between `MAYBE` and that, ask which one.
pub const SURE: f32 = 0.85;
pub const MARGIN: f32 = 0.15;
pub const MAYBE: f32 = 0.6;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Email {
    pub address: String,
    #[serde(default)]
    pub label: String,
    #[serde(default = "user")]
    pub source: String,
    #[serde(default)]
    pub primary: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Phone {
    /// E.164, "+<digits>".
    pub number: String,
    #[serde(default)]
    pub label: String,
    #[serde(default = "user")]
    pub source: String,
    #[serde(default)]
    pub primary: bool,
}

fn user() -> String {
    USER.into()
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Person {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub emails: Vec<Email>,
    #[serde(default)]
    pub phones: Vec<Phone>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub org: String,
    #[serde(default)]
    pub sources: Vec<String>,
    /// Unix seconds of the last change.
    #[serde(default)]
    pub updated: i64,
    /// Lookup only: names other sources know this person by.
    #[serde(skip)]
    pub aka: Vec<String>,
    /// Lookup only: tie-break between equal scores (user first, then mail
    /// the user sent, then by how often the address was seen).
    #[serde(skip)]
    pub weight: u32,
}

impl Person {
    pub fn new(name: &str, source: &str) -> Person {
        Person {
            name: name.trim().to_string(),
            sources: vec![source.to_string()],
            weight: if source == USER { u32::MAX } else { 0 },
            ..Person::default()
        }
    }

    /// Anything the user saved themselves.
    pub fn is_user(&self) -> bool {
        self.emails.iter().any(|e| e.source == USER)
            || self.phones.iter().any(|p| p.source == USER)
            || self.sources.iter().any(|s| s == USER)
    }

    pub fn has_email(&self, address: &str) -> bool {
        self.emails.iter().any(|e| e.address == address)
    }

    pub fn has_phone(&self, number: &str) -> bool {
        self.phones.iter().any(|p| p.number == number)
    }

    fn add_source(&mut self, source: &str) {
        if !self.sources.iter().any(|s| s == source) {
            self.sources.push(source.to_string());
        }
    }

    pub fn add_email(&mut self, address: &str, label: &str, source: &str) {
        if !self.has_email(address) {
            self.emails.push(Email {
                address: address.to_string(),
                label: label.trim().to_lowercase(),
                source: source.to_string(),
                primary: self.emails.is_empty(),
            });
        }
        self.add_source(source);
    }

    pub fn add_phone(&mut self, number: &str, label: &str, source: &str) {
        if !self.has_phone(number) {
            self.phones.push(Phone {
                number: number.to_string(),
                label: label.trim().to_lowercase(),
                source: source.to_string(),
                primary: self.phones.is_empty(),
            });
        }
        self.add_source(source);
    }

    /// Adds tags (normalised, deduped); an unusable tag is an error.
    pub fn add_tags(&mut self, tags: &[String]) -> Result<(), String> {
        for t in tags {
            let t = norm_tag(t)?;
            if !self.tags.contains(&t) {
                self.tags.push(t);
            }
        }
        Ok(())
    }
}

/// Folds a name for matching: lowercase, common diacritics removed,
/// punctuation as spaces, single spaces.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.to_lowercase().chars() {
        let mapped: &str = match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
            'ç' | 'ć' | 'č' => "c",
            'ď' | 'đ' => "d",
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => "e",
            'ğ' => "g",
            'ì' | 'í' | 'î' | 'ï' | 'ī' | 'ı' => "i",
            'ł' => "l",
            'ñ' | 'ń' | 'ň' => "n",
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => "o",
            'ř' => "r",
            'ś' | 'š' | 'ş' => "s",
            'ť' | 'ţ' => "t",
            'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' => "u",
            'ý' | 'ÿ' => "y",
            'ź' | 'ż' | 'ž' => "z",
            'ß' => "ss",
            'æ' => "ae",
            'œ' => "oe",
            c if c.is_alphanumeric() => {
                out.push(c);
                continue;
            }
            _ => " ",
        };
        out.push_str(mapped);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn norm_email(s: &str) -> String {
    s.trim().to_lowercase()
}

/// A tag is one lowercase word (letters, digits, hyphens); spaces become
/// hyphens.
pub fn norm_tag(s: &str) -> Result<String, String> {
    let t: String = s
        .trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect();
    if t.is_empty() || t.len() > 32 {
        return Err(format!(
            "\"{}\" is not a usable tag (one short word, like work)",
            s.trim()
        ));
    }
    Ok(t)
}

/// The rules every saved person must follow.
pub fn validate(p: &Person) -> Result<(), String> {
    if p.name.trim().is_empty() {
        return Err("A contact needs a name.".into());
    }
    for e in &p.emails {
        if !crate::email::is_email(&e.address) || e.address != norm_email(&e.address) {
            return Err(format!("{} is not an email address", e.address));
        }
    }
    for ph in &p.phones {
        if !ph.number.starts_with('+') || !ph.number[1..].bytes().all(|b| b.is_ascii_digit()) {
            return Err(format!("{} is not a phone number", ph.number));
        }
    }
    let sources = p
        .emails
        .iter()
        .map(|e| &e.source)
        .chain(p.phones.iter().map(|x| &x.source));
    for s in sources.chain(p.sources.iter()) {
        if !SOURCES.contains(&s.as_str()) {
            return Err(format!("unknown contact source {s}"));
        }
    }
    for t in &p.tags {
        if norm_tag(t).as_deref() != Ok(t.as_str()) {
            return Err(format!("\"{t}\" is not a usable tag"));
        }
    }
    if p.emails.len() > 1 && p.tags.is_empty() {
        return Err(format!(
            "{} has {} email addresses, so they need at least one tag (like work or personal) \
             saying which is which.",
            p.name,
            p.emails.len()
        ));
    }
    Ok(())
}

/// The old files as one list, merged by folded name and by shared address or
/// number. A file that is there but unreadable is an error: nothing is built
/// from half the data.
fn migrate(dir: &Path) -> Result<Vec<Person>, String> {
    let read = |file: &str, bad: &str| -> Result<BTreeMap<String, String>, String> {
        match std::fs::read_to_string(dir.join(file)) {
            Ok(c) => serde_json::from_str(&c).map_err(|_| bad.to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(format!("Cannot read {file}: {e}")),
        }
    };
    let contacts = read(
        "contacts.json",
        "contacts.json is unreadable; fix or delete it",
    )?;
    let phones = read(
        "phones.json",
        "phones.json is not valid JSON; fix or delete it",
    )?;
    let mut people: Vec<Person> = Vec::new();
    // Folded names seen so far, including ones merged away by a shared value.
    let mut names: HashMap<String, usize> = HashMap::new();
    let mut place = |name: &str, people: &mut Vec<Person>, same: Option<usize>| {
        let key = fold(name);
        let i = same
            .or_else(|| names.get(&key).copied())
            .unwrap_or_else(|| {
                people.push(Person::new(name, USER));
                people.len() - 1
            });
        names.insert(key, i);
        i
    };
    for (name, address) in contacts {
        let address = norm_email(&address);
        if name.trim().is_empty() || !crate::email::is_email(&address) {
            continue;
        }
        let same = people.iter().position(|p| p.has_email(&address));
        let i = place(&name, &mut people, same);
        let p = &mut people[i];
        p.add_email(&address, "", USER);
        // "Neha" and "neha" with two addresses: both kept, tagged so the
        // tag rule holds.
        if p.emails.len() > 1 && p.tags.is_empty() {
            p.tags.push("imported".into());
        }
    }
    for (name, number) in phones {
        if name.trim().is_empty() || !number.starts_with('+') {
            continue;
        }
        let same = people.iter().position(|p| p.has_phone(&number));
        let i = place(&name, &mut people, same);
        people[i].add_phone(&number, "", USER);
    }
    let now = now();
    for (n, p) in people.iter_mut().enumerate() {
        p.id = format!("p{now}-{n}");
        p.updated = now;
    }
    Ok(people)
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// The store; built from the old files when people.json is missing.
pub fn load(dir: &Path) -> Result<Vec<Person>, String> {
    match std::fs::read_to_string(dir.join(FILE)) {
        Ok(c) => {
            let mut people: Vec<Person> =
                serde_json::from_str(&c).map_err(|_| CORRUPT.to_string())?;
            for p in &mut people {
                if p.is_user() {
                    p.weight = u32::MAX;
                }
            }
            Ok(people)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let people = migrate(dir)?;
            // Written once so the old files are never read again; a failed
            // write just migrates again next time.
            let _ = write(dir, &people);
            Ok(people)
        }
        Err(e) => Err(format!("Cannot read people.json: {e}")),
    }
}

/// Temp file + rename, so a crash never leaves half a file.
fn write(dir: &Path, people: &[Person]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(people).map_err(|e| e.to_string())?;
    let tmp = dir.join("people.json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, dir.join(FILE)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

/// The only way to change the store: load, change, validate everyone, save.
/// Nothing is written when `change` or a rule fails.
pub fn update<T>(
    dir: &Path,
    change: impl FnOnce(&mut Vec<Person>) -> Result<T, String>,
) -> Result<T, String> {
    // A tool and a Settings edit at once must not lose either change.
    // ponytail: one lock for every data dir; per-dir locks if that ever matters.
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _held = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut people = load(dir)?;
    let before = people.clone();
    let out = change(&mut people)?;
    for p in &mut people {
        if p.id.is_empty() {
            p.id = new_id(&before);
        }
        if p.sources.is_empty() {
            p.sources.push(USER.into());
        }
        // Exactly one primary of each kind.
        if !p.emails.is_empty() && !p.emails.iter().any(|e| e.primary) {
            p.emails[0].primary = true;
        }
        if !p.phones.is_empty() && !p.phones.iter().any(|x| x.primary) {
            p.phones[0].primary = true;
        }
        validate(p)?;
        if !before.contains(p) {
            p.updated = now();
        }
    }
    if people != before {
        write(dir, &people)?;
    }
    Ok(out)
}

fn new_id(taken: &[Person]) -> String {
    let base = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
    let mut n = 0;
    loop {
        let id = format!("p{base}-{n}");
        if !taken.iter().any(|p| p.id == id) {
            return id;
        }
        n += 1;
    }
}

/// The person whose folded name is `name`, preferring the user's own.
pub fn named<'a>(people: &'a mut [Person], name: &str) -> Option<&'a mut Person> {
    let key = fold(name);
    let i = people
        .iter()
        .position(|p| fold(&p.name) == key && p.is_user())
        .or_else(|| people.iter().position(|p| fold(&p.name) == key))?;
    Some(&mut people[i])
}

enum Field<'a> {
    Email(&'a str),
    Phone(&'a str),
}

/// The old one-value-per-name save: `name` gets this as its primary
/// user-saved value (replacing the old one) and the name's new spelling.
fn set(dir: &Path, name: &str, value: Field) -> Result<(), String> {
    let name = name.trim();
    update(dir, |people| {
        if named(people, name).is_none() {
            people.push(Person::new(name, USER));
        }
        let p = named(people, name).expect("just added");
        p.name = name.to_string();
        match value {
            Field::Email(a) => {
                p.emails.retain(|e| e.address != a);
                let slot = p.emails.iter().position(|e| e.source == USER && e.primary);
                match slot.or_else(|| p.emails.iter().position(|e| e.source == USER)) {
                    Some(i) => p.emails[i].address = a.to_string(),
                    None => {
                        p.emails.iter_mut().for_each(|e| e.primary = false);
                        p.add_email(a, "", USER);
                        p.emails.last_mut().expect("added").primary = true;
                    }
                }
            }
            Field::Phone(n) => {
                p.phones.retain(|x| x.number != n);
                let slot = p.phones.iter().position(|x| x.source == USER && x.primary);
                match slot.or_else(|| p.phones.iter().position(|x| x.source == USER)) {
                    Some(i) => p.phones[i].number = n.to_string(),
                    None => {
                        p.phones.iter_mut().for_each(|x| x.primary = false);
                        p.add_phone(n, "", USER);
                        p.phones.last_mut().expect("added").primary = true;
                    }
                }
            }
        }
        p.add_source(USER);
        Ok(())
    })
}

pub fn set_email(dir: &Path, name: &str, address: &str) -> Result<(), String> {
    set(dir, name, Field::Email(address))
}

pub fn set_phone(dir: &Path, name: &str, number: &str) -> Result<(), String> {
    set(dir, name, Field::Phone(number))
}

/// Name to primary user-saved email, as contacts.json used to be.
#[cfg(test)]
pub fn user_emails(dir: &Path) -> Result<BTreeMap<String, String>, String> {
    Ok(load(dir)?
        .into_iter()
        .filter_map(|p| {
            let mut mine = p.emails.iter().filter(|e| e.source == USER);
            let e = mine.clone().find(|e| e.primary).or_else(|| mine.next())?;
            Some((p.name.clone(), e.address.clone()))
        })
        .collect())
}

/// Name to primary user-saved number, as phones.json used to be. Only these
/// count for the automatic-reply "anyone in my contacts".
pub fn user_phones(dir: &Path) -> Result<BTreeMap<String, String>, String> {
    Ok(load(dir)?
        .into_iter()
        .filter_map(|p| {
            let mut mine = p.phones.iter().filter(|x| x.source == USER);
            let x = mine.clone().find(|x| x.primary).or_else(|| mine.next())?;
            Some((p.name.clone(), x.number.clone()))
        })
        .collect())
}

/// Adds a synced record to the lookup list. Same email or number means same
/// person: its new values join that person, tagged by source when that gives
/// them a second email (so a sync never trips the tag rule). Similar names
/// alone never merge.
pub fn merge(people: &mut Vec<Person>, incoming: Person) {
    let shared = |p: &Person| {
        incoming.emails.iter().any(|e| p.has_email(&e.address))
            || incoming.phones.iter().any(|x| p.has_phone(&x.number))
    };
    let Some(p) = people.iter_mut().find(|p| shared(p)) else {
        people.push(incoming);
        return;
    };
    let names = std::iter::once(&incoming.name).chain(incoming.aka.iter());
    for n in names {
        if !n.trim().is_empty() && fold(n) != fold(&p.name) && !p.aka.contains(n) {
            p.aka.push(n.clone());
        }
    }
    for e in &incoming.emails {
        p.add_email(&e.address, &e.label, &e.source);
    }
    for x in &incoming.phones {
        p.add_phone(&x.number, &x.label, &x.source);
    }
    for s in &incoming.sources {
        p.add_source(s);
        if p.emails.len() > 1 && p.tags.is_empty() {
            p.tags.push(s.clone());
        }
    }
    p.weight = p.weight.max(incoming.weight);
}

/// Nickname groups: any two names in one line are the same first name.
const NICKNAMES: &[&str] = &[
    "abigail abby abbie gail",
    "alexander alex alec al lex xander sandy",
    "alexandra alex alexa lexi sandra sandy",
    "andrew andy drew",
    "anthony tony",
    "barbara barb babs",
    "benjamin ben benny benji",
    "catherine katherine kathryn cathy kathy kate katie kat",
    "charles charlie chuck",
    "christopher chris kit topher",
    "christina christine chris tina chrissy",
    "daniel dan danny",
    "david dave davey",
    "deborah debra deb debbie",
    "dorothy dot dottie",
    "douglas doug",
    "edward ed eddie ted ned",
    "eleanor ellie nell nora",
    "elizabeth liz lizzie beth betty eliza libby bess",
    "francis frances frank fran frankie",
    "frederick fred freddie",
    "gabriel gabe",
    "gerald gerry jerry",
    "gregory greg",
    "henry harry hank",
    "isabella isabel izzy bella",
    "jacob jake",
    "james jim jimmy jamie",
    "jeffrey geoffrey jeff geoff",
    "jennifer jen jenny",
    "jessica jess jessie",
    "john jon johnny jack",
    "jonathan jon jonny nathan",
    "joseph joe joey",
    "joshua josh",
    "judith judy",
    "kenneth ken kenny",
    "lawrence laurence larry",
    "leonard leo len lenny",
    "margaret maggie meg peggy marge greta",
    "matthew matt matty",
    "michael mike mikey mick",
    "nathaniel nathan nate nat",
    "nicholas nick nicky",
    "patricia pat patty trish tricia",
    "patrick pat paddy",
    "peter pete",
    "philip phillip phil",
    "raymond ray",
    "rebecca becky becca",
    "richard rick ricky rich dick",
    "robert rob robbie bob bobby bert",
    "ronald ron ronnie",
    "samantha sam sammy",
    "samuel sam sammy",
    "stephen steven steve stevie",
    "susan sue susie suzy",
    "theodore theo ted teddy",
    "thomas tom tommy",
    "timothy tim timmy",
    "victoria vicky tori",
    "william will bill billy liam willy",
    "zachary zach zack",
];

fn nicknames(name: &str) -> Vec<&'static str> {
    NICKNAMES
        .iter()
        .filter(|g| g.split(' ').any(|n| n == name))
        .flat_map(|g| g.split(' '))
        .filter(|n| *n != name)
        .collect()
}

/// Damerau-Levenshtein distance (optimal string alignment) over chars.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    d[0] = (0..=b.len()).collect();
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

/// How well one query word fits one name word, 0 to 1:
/// 1.0 the same word; 0.9 a nickname of it (Ben, Benjamin, Benny);
/// 0.7-0.95 a prefix (more of the word typed scores higher);
/// up to 0.95 * (1 - d/(len+2)) for a typo of the word or of a nickname
/// (d = Damerau-Levenshtein distance, at most 1 for short words, 2 for long).
fn word_score(q: &str, t: &str) -> f32 {
    if q == t {
        return 1.0;
    }
    let mut best: f32 = 0.0;
    if nicknames(t).contains(&q) {
        best = 0.9;
    }
    let (ql, tl) = (q.chars().count(), t.chars().count());
    if ql >= 2 && t.starts_with(q) {
        best = best.max(0.7 + 0.25 * ql as f32 / tl as f32);
    }
    if ql >= 3 {
        let mut forms = vec![t];
        forms.extend(nicknames(t));
        for c in forms {
            let len = ql.max(c.chars().count());
            let d = edit_distance(q, c);
            if d <= if len <= 5 { 1 } else { 2 } {
                best = best.max(0.95 * (1.0 - d as f32 / (len as f32 + 2.0)));
            }
        }
    }
    best
}

/// How well `query` fits a name, 0 to 1: 1.0 for the whole name; otherwise
/// the mean over query words of each word's best `word_score` against the
/// name's words, so "Ben" fits "Benjamin Tan" at 0.9 and "Ben Lim" at 0.5.
fn name_score(query: &str, name: &str) -> f32 {
    let (q, n) = (fold(query), fold(name));
    if q.is_empty() || n.is_empty() {
        return 0.0;
    }
    if q == n {
        return 1.0;
    }
    let words: Vec<&str> = n.split(' ').collect();
    let qs: Vec<&str> = q.split(' ').collect();
    let total: f32 = qs
        .iter()
        .map(|w| words.iter().map(|t| word_score(w, t)).fold(0.0, f32::max))
        .sum();
    total / qs.len() as f32
}

/// How well `query` fits a person, 0 to 1, and whether the best fit was a
/// name only another source gave. Names score by `name_score`; an exact
/// address or number is 1.0; words found only inside an address's local part
/// score 0.65 (the weakest signal: enough to ask, never to pick alone).
pub fn score(query: &str, p: &Person) -> (f32, bool) {
    let mut best = (name_score(query, &p.name), false);
    for a in &p.aka {
        let s = name_score(query, a);
        if s > best.0 {
            best = (s, true);
        }
    }
    let q = query.trim().to_lowercase();
    let digits: String = q.chars().filter(char::is_ascii_digit).collect();
    let exact = (q.contains('@') && p.has_email(&q))
        || (digits.len() >= 6
            && p.phones
                .iter()
                .any(|x| x.number.trim_start_matches('+') == digits));
    if exact {
        return (1.0, false);
    }
    let words: Vec<String> = fold(&q)
        .split(' ')
        .filter(|w| w.len() >= 3)
        .map(String::from)
        .collect();
    let in_address = !words.is_empty()
        && p.emails.iter().any(|e| {
            let local = fold(e.address.split('@').next().unwrap_or_default());
            words.iter().all(|w| local.contains(w.as_str()))
        });
    if in_address && best.0 < 0.65 {
        best = (0.65, false);
    }
    best
}

pub enum Found<'a> {
    /// One person, and whether a name from another source picked them.
    One(&'a Person, bool),
    Many(Vec<&'a Person>),
    None,
}

/// Resolves `query` among `people`. The user's own contacts come first: one of
/// them wins when it is sure (at least `SURE`, `MARGIN` ahead of the next
/// user contact) and nothing synced scores higher. Synced people are picked
/// alone only when no user contact is even a maybe; otherwise every close
/// candidate is listed, so a synced name never silently beats the user's.
pub fn lookup<'a>(people: &'a [Person], query: &str) -> Found<'a> {
    let mut scored: Vec<(f32, bool, &Person)> = people
        .iter()
        .map(|p| {
            let (s, aka) = score(query, p);
            (s, aka, p)
        })
        .filter(|(s, _, _)| *s >= MAYBE)
        .collect();
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.2.weight.cmp(&a.2.weight))
    });
    type Scored<'p> = (f32, bool, &'p Person);
    let (user, synced): (Vec<&Scored>, Vec<&Scored>) =
        scored.iter().partition(|(_, _, p)| p.is_user());
    let sure = |list: &[&(f32, bool, &'a Person)]| -> Option<(&'a Person, bool)> {
        let first = list.first()?;
        let lead = list.get(1).map_or(1.0, |second| first.0 - second.0);
        (first.0 >= SURE && lead >= MARGIN - 1e-4).then_some((first.2, first.1))
    };
    let top_synced = synced.first().map_or(0.0, |s| s.0);
    if let Some((p, aka)) = sure(&user).filter(|_| user[0].0 >= top_synced) {
        return Found::One(p, aka);
    }
    if user.is_empty() {
        if let Some((p, aka)) = sure(&synced) {
            return Found::One(p, aka);
        }
    }
    if scored.is_empty() {
        return Found::None;
    }
    Found::Many(scored.iter().take(5).map(|s| s.2).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::temp_dir;

    fn person(name: &str, emails: &[&str]) -> Person {
        let mut p = Person::new(name, USER);
        for e in emails {
            p.add_email(e, "", USER);
        }
        p
    }

    fn picked(people: &[Person], q: &str) -> String {
        match lookup(people, q) {
            Found::One(p, _) => p.name.clone(),
            Found::Many(ps) => format!(
                "many: {}",
                ps.iter()
                    .map(|p| p.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Found::None => "none".into(),
        }
    }

    #[test]
    fn one_email_creates_and_more_need_a_tag() {
        let dir = temp_dir();
        update(&dir, |ps| {
            ps.push(person("Ben Tan", &["ben@a.com"]));
            Ok(())
        })
        .unwrap();
        let err = update(&dir, |ps| {
            named(ps, "ben tan")
                .unwrap()
                .add_email("ben@work.com", "work", USER);
            Ok(())
        })
        .unwrap_err();
        assert!(err.contains("need at least one tag"), "{err}");
        assert_eq!(load(&dir).unwrap()[0].emails.len(), 1, "nothing saved");
        update(&dir, |ps| {
            let p = named(ps, "Ben Tan").unwrap();
            p.add_tags(&["Work".into(), " work ".into(), "Old Friends".into()])?;
            p.add_email("ben@work.com", "Work", USER);
            Ok(())
        })
        .unwrap();
        let p = &load(&dir).unwrap()[0];
        assert_eq!(p.tags, ["work", "old-friends"]);
        assert_eq!(p.emails[1].label, "work");
        assert!(p.emails[0].primary && !p.emails[1].primary);
        // Removing the last tag of a two-address person is refused.
        let err = update(&dir, |ps| {
            named(ps, "ben tan").unwrap().tags.clear();
            Ok(())
        });
        assert!(err.is_err());
        update(&dir, |ps| {
            named(ps, "ben tan").unwrap().tags.retain(|t| t != "work");
            Ok(())
        })
        .unwrap();
        assert_eq!(load(&dir).unwrap()[0].tags, ["old-friends"]);
        assert!(norm_tag("  ").is_err() && norm_tag("!!").is_err());
    }

    #[test]
    fn migrates_old_files_once_and_keeps_them() {
        let dir = temp_dir();
        let contacts = r#"{"Neha Aggarwal": "Neha@Example.com", "N. Aggarwal": "neha@example.com", "Sam": "sam@x.org"}"#;
        let phones = r#"{"neha aggarwal": "+919876543210", "Bob": "+14155550100"}"#;
        std::fs::write(dir.join("contacts.json"), contacts).unwrap();
        std::fs::write(dir.join("phones.json"), phones).unwrap();
        let people = load(&dir).unwrap();
        let names: Vec<&str> = people.iter().map(|p| p.name.as_str()).collect();
        // Same address: one person (the first name, alphabetically, wins).
        assert_eq!(names, ["N. Aggarwal", "Sam", "Bob"]);
        assert!(
            people[0].has_phone("+919876543210"),
            "the merged-away name still matches"
        );
        assert_eq!(user_emails(&dir).unwrap()["Sam"], "sam@x.org");
        assert_eq!(user_phones(&dir).unwrap()["Bob"], "+14155550100");
        assert!(people
            .iter()
            .all(|p| p.sources == [USER] && !p.id.is_empty()));
        assert!(dir.join("people.json").exists());
        // The old files stay as they were; later edits do not touch them.
        set_email(&dir, "Sam", "sam@new.org").unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("contacts.json")).unwrap(),
            contacts
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("phones.json")).unwrap(),
            phones
        );
        assert_eq!(user_emails(&dir).unwrap()["Sam"], "sam@new.org");
    }

    #[test]
    fn migration_merges_by_name_too() {
        let dir = temp_dir();
        std::fs::write(
            dir.join("contacts.json"),
            r#"{"Neha Aggarwal": "neha@example.com"}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("phones.json"),
            r#"{"neha  AGGARWAL": "+919876543210"}"#,
        )
        .unwrap();
        let people = load(&dir).unwrap();
        assert_eq!(people.len(), 1);
        assert!(people[0].has_email("neha@example.com") && people[0].has_phone("+919876543210"));
    }

    #[test]
    fn corrupt_files_are_never_overwritten() {
        let dir = temp_dir();
        std::fs::write(dir.join("phones.json"), "{bad").unwrap();
        assert!(load(&dir).unwrap_err().contains("phones.json"));
        assert!(set_email(&dir, "Bob", "bob@x.com").is_err());
        assert!(!dir.join("people.json").exists());
        let dir = temp_dir();
        std::fs::write(dir.join(FILE), "[{").unwrap();
        assert!(set_email(&dir, "Bob", "bob@x.com")
            .unwrap_err()
            .contains("not valid JSON"));
        assert_eq!(std::fs::read_to_string(dir.join(FILE)).unwrap(), "[{");
    }

    #[test]
    fn fold_and_scores() {
        assert_eq!(fold("  José  O'Brien-Núñez "), "jose o brien nunez");
        assert_eq!(edit_distance("bent", "ben"), 1);
        assert_eq!(edit_distance("jhon", "john"), 1, "a swap is one edit");
        assert_eq!(name_score("Benjamin Tan", "benjamin tan"), 1.0);
        assert!(name_score("ben", "Benjamin Tan") >= SURE);
        assert!(name_score("bent tan", "Benjamin Tan") >= SURE);
        assert!(name_score("benjamn tan", "Benjamin Tan") >= SURE);
        assert!(name_score("bent tan", "Alex Tan") < MAYBE);
        assert!(name_score("xyz", "Benjamin Tan") == 0.0);
    }

    #[test]
    fn fuzzy_queries_find_benjamin_tan() {
        let people = vec![
            person("Benjamin Tan", &["ben@tan.com"]),
            person("Alex Tan", &["alex@tan.com"]),
            person("Alex Lim", &["alex@lim.com"]),
            person("Neha Aggarwal", &["neha.aggarwal2004@gmail.com"]),
        ];
        for q in [
            "Ben",
            "Benjamin",
            "Ben Tan",
            "bent tan",
            "Benny",
            "benjamin TAN",
            "ben@tan.com",
        ] {
            assert_eq!(picked(&people, q), "Benjamin Tan", "{q}");
        }
        assert_eq!(picked(&people, "Alex"), "many: Alex Tan, Alex Lim");
        assert_eq!(picked(&people, "lim"), "Alex Lim", "last name only");
        assert_eq!(picked(&people, "Alex Lim"), "Alex Lim");
        assert_eq!(picked(&people, "Tan"), "many: Benjamin Tan, Alex Tan");
        assert_eq!(picked(&people, "Agarwal"), "Neha Aggarwal", "typo");
        assert_eq!(picked(&people, "Zed"), "none");
        // Only inside an address: enough to ask, never to pick.
        assert_eq!(picked(&people, "aggarwal2004"), "many: Neha Aggarwal");
    }

    #[test]
    fn synced_names_never_silently_beat_the_users() {
        let mut people = vec![person("Ben Lin", &["ben@lin.com"])];
        let mut mail = Person::new("Ben Lim", MAIL);
        mail.add_email("ben@lim.com", "", MAIL);
        merge(&mut people, mail);
        // Exact synced, close user contact: both listed.
        assert_eq!(picked(&people, "ben lim"), "many: Ben Lim, Ben Lin");
        assert_eq!(picked(&people, "lim"), "many: Ben Lim, Ben Lin");
        assert!(matches!(lookup(&people, "Ben Lin"), Found::One(p, false) if p.is_user()));
        // The user's exact match wins over an equally good synced one.
        let mut wa = Person::new("Ben Lin", WHATSAPP);
        wa.add_phone("+4917000000009", "", WHATSAPP);
        merge(&mut people, wa);
        assert_eq!(people.len(), 3);
        assert!(matches!(lookup(&people, "ben lin"), Found::One(p, false) if p.is_user()));
        // No user candidate at all: the synced one is picked.
        let mut zoe = Person::new("Zoe Park", MAIL);
        zoe.add_email("zoe@park.com", "", MAIL);
        merge(&mut people, zoe);
        assert_eq!(picked(&people, "zoe"), "Zoe Park");
    }

    #[test]
    fn same_email_or_phone_merges_and_tags_by_source() {
        let mut people = vec![person("Neha", &["neha@example.com"])];
        let mut p = Person::new("Neha Sharma", "outlook");
        p.add_email("neha@example.com", "", OUTLOOK);
        p.add_email("neha@work.com", "work", OUTLOOK);
        p.add_phone("+491701234567", "", OUTLOOK);
        merge(&mut people, p);
        assert_eq!(people.len(), 1, "same email: same person");
        let n = &people[0];
        assert_eq!(n.name, "Neha", "the user's name stays");
        assert_eq!(n.aka, ["Neha Sharma"]);
        assert_eq!(n.emails.len(), 2);
        assert_eq!(n.tags, ["outlook"], "a sync never trips the tag rule");
        assert!(validate(n).is_ok());
        assert_eq!(n.sources, [USER, OUTLOOK]);
        // Matched only through the other source's name: flagged.
        assert!(matches!(
            lookup(&people, "neha sharma"),
            Found::One(_, true)
        ));
        // A similar name with nothing shared stays a separate person.
        let mut other = Person::new("Neha", WHATSAPP);
        other.add_phone("+4917000000002", "", WHATSAPP);
        merge(&mut people, other);
        assert_eq!(people.len(), 2);
        // Dedupe by number.
        let mut again = Person::new("N", WHATSAPP);
        again.add_phone("+491701234567", "", WHATSAPP);
        merge(&mut people, again);
        assert_eq!(people.len(), 2);
    }
}
