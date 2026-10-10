//! The three security levels. They guard only emails, PowerShell scripts and
//! MCP tool calls; the other tools are safe by construction (write_file never
//! overwrites, open refuses programs).

use crate::config::Tier;

pub fn email_needs_confirm(tier: Tier, known_recipient: bool, tainted: bool) -> bool {
    match tier {
        Tier::Conservative => true,
        Tier::Competent => !known_recipient || tainted,
        Tier::CarteBlanche => false,
    }
}

/// MCP tools: competent trusts only servers marked `"trusted": true` in
/// mcp.json, and only while the request is untainted.
pub fn mcp_needs_confirm(tier: Tier, trusted_server: bool, tainted: bool) -> bool {
    match tier {
        Tier::Conservative => true,
        Tier::Competent => !trusted_server || tainted,
        Tier::CarteBlanche => false,
    }
}

pub fn script_needs_confirm(tier: Tier, script: &str, tainted: bool) -> bool {
    match tier {
        Tier::Conservative => true,
        Tier::Competent => tainted || !is_read_only(script),
        Tier::CarteBlanche => false,
    }
}

/// Fragments that can run code, write, or reach out, whatever surrounds them.
const DANGER: [&str; 13] = [
    "::",
    "`",
    "&",
    ">",
    "invoke",
    "iex",
    "start-",
    "-encodedcommand",
    "add-type",
    "new-object",
    "[scriptblock]",
    "$executioncontext",
    "downloadstring",
];

/// Read-only verbs, matched as `verb-` at the start of a command.
const READ_VERBS: [&str; 11] = [
    "get-",
    "test-",
    "measure-",
    "select-",
    "where-",
    "sort-",
    "format-",
    "group-",
    "compare-",
    "resolve-",
    "convertto-",
];

/// Aliases and cmdlets outside READ_VERBS that only read or reshape output.
const READ_COMMANDS: [&str; 18] = [
    "foreach-object",
    "%",
    "where",
    "?",
    "ls",
    "dir",
    "gci",
    "cat",
    "gc",
    "type",
    "pwd",
    "select",
    "sort",
    "measure",
    "ft",
    "fl",
    "out-string",
    "write-output",
];

/// A static allowlist check, conservative by design: when unsure, it says no
/// and the user is asked.
/// ponytail: string scan, not a parser. Upgrade to PowerShell's AST parser
/// (System.Management.Automation.Language.Parser) if harmless scripts ask too often.
pub fn is_read_only(script: &str) -> bool {
    let s = script.to_ascii_lowercase();

    // Check for member assignment (any . or : on left side of =, +=, -=, etc.)
    if has_member_assignment(&s) {
        return false;
    }

    // Check for dynamic method calls (.'(...), ."(...), .(, )(...), .$(...))
    if has_dynamic_method_call(&s) {
        return false;
    }

    // Check for % or foreach-object not immediately followed by {
    if has_foreach_without_block(&s) {
        return false;
    }

    if DANGER.iter().any(|d| s.contains(d)) || has_method_call(&s) {
        return false;
    }
    // Every place a command can start: line and statement starts, pipes,
    // blocks, sub-expressions and the right side of an assignment.
    s.split(['\n', '\r', ';', '|', '{', '}', '(', ')', '='])
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .all(|segment| {
            let first = segment.split_whitespace().next().unwrap_or_default();
            // An expression (variable, parameter, string, number, array, type literal).
            first.starts_with(['$', '-', '"', '\'', '@', '[', '!'])
                || first.starts_with(|c: char| c.is_ascii_digit())
                || READ_COMMANDS.contains(&first)
                || READ_VERBS.iter().any(|v| {
                    first.starts_with(v)
                        && first[v.len()..].chars().all(|c| c.is_ascii_alphabetic())
                })
        })
}

/// Reject assignments with . or : on the left side ($x.y = ..., $global:x = ..., etc.).
/// Plain variable assignment like $files = ... is still allowed.
fn has_member_assignment(s: &str) -> bool {
    let b = s.as_bytes();
    let statement_boundaries = *b";\n\r|{(";

    for i in 0..b.len() {
        // Check for =, +=, -=, *=, /=, %=
        let is_assign = b[i] == b'=';
        let is_compound = i > 0
            && (b[i - 1] == b'+'
                || b[i - 1] == b'-'
                || b[i - 1] == b'*'
                || b[i - 1] == b'/'
                || b[i - 1] == b'%');

        if is_assign && (is_compound || (i + 1 >= b.len() || b[i + 1] != b'=')) {
            // Found an assignment operator. Check left side back to previous boundary.
            let mut left_start = 0;
            for j in (0..i).rev() {
                if statement_boundaries.contains(&b[j]) {
                    left_start = j + 1;
                    break;
                }
            }
            let left_side = std::str::from_utf8(&b[left_start..i]).unwrap_or("");
            // If left side contains . or :, it's a member assignment.
            if left_side.contains('.') || left_side.contains(':') {
                return true;
            }
        }
    }
    false
}

/// Reject .(...), ."(...), .'(...), .$(...), and )(...) (dynamic method calls).
fn has_dynamic_method_call(s: &str) -> bool {
    let b = s.as_bytes();
    for i in 0..b.len() {
        if b[i] == b'.' && i + 1 < b.len() {
            let next = b[i + 1];
            // .(, .'(...), ."(...), .$(...) are all dynamic calls.
            if next == b'(' || next == b'\'' || next == b'"' || next == b'$' {
                return true;
            }
        }
        if b[i] == b')' {
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < b.len() && b[j] == b'(' {
                return true;
            }
        }
    }
    false
}

/// Allow % and foreach-object only when next token is { (script block).
fn has_foreach_without_block(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let is_percent = b[i] == b'%';
        let is_foreach = i + 14 <= b.len() && &b[i..i + 14] == b"foreach-object";

        if is_percent || is_foreach {
            let mut j = i + if is_percent { 1 } else { 14 };
            while j < b.len() && b[j] == b' ' {
                j += 1;
            }
            if j < b.len() && b[j] != b'{' {
                return true;
            }
            if is_percent {
                i += 1;
            } else {
                i += 14;
            }
        } else {
            i += 1;
        }
    }
    false
}

/// `.Name(` anywhere: a .NET method call, which can do anything.
fn has_method_call(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'.' {
            let mut j = i + 1;
            while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                j += 1;
            }
            let mut k = j;
            while k < b.len() && b[k] == b' ' {
                k += 1;
            }
            if j > i + 1 && k < b.len() && b[k] == b'(' {
                return true;
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Tier::*;

    #[test]
    fn email_levels() {
        assert!(email_needs_confirm(Conservative, true, false));
        assert!(!email_needs_confirm(Competent, true, false));
        assert!(email_needs_confirm(Competent, false, false));
        assert!(email_needs_confirm(Competent, true, true));
        assert!(!email_needs_confirm(CarteBlanche, false, true));
    }

    #[test]
    fn script_levels() {
        let read = "Get-Date";
        let write = "Remove-Item C:\\x";
        assert!(script_needs_confirm(Conservative, read, false));
        assert!(!script_needs_confirm(Competent, read, false));
        assert!(script_needs_confirm(Competent, read, true));
        assert!(script_needs_confirm(Competent, write, false));
        assert!(!script_needs_confirm(CarteBlanche, write, true));
    }

    #[test]
    fn read_only_scripts() {
        for script in [
            "Get-Date",
            "Get-ChildItem $env:USERPROFILE\\Downloads | Sort-Object Length -Descending | Select-Object -First 5 Name, Length",
            "Get-Process | Where-Object { $_.CPU -gt 10 } | Format-Table Name, CPU",
            "$files = Get-ChildItem C:\\Users; $files.Count",
            "ls | measure",
            "Get-Content notes.txt | Select-String eggs",
            "Get-ChildItem | % { $_.Name }",
            "Get-ChildItem | ForEach-Object { $_.Length }",
        ] {
            assert!(is_read_only(script), "should be read-only: {script}");
        }
    }

    #[test]
    fn anything_else_is_not() {
        for script in [
            "Remove-Item C:\\x",
            "Get-ChildItem | Remove-Item",
            "$x = rm C:\\x",
            "Get-ChildItem; Stop-Process -Name x",
            "[System.IO.File]::Delete('x')",
            "(New-Object Net.WebClient).DownloadString('u')",
            "iex 'x'",
            "& 'C:\\x.exe'",
            "Get-Content a > b",
            "foreach ($f in ls) { del $f }",
            "$p = Get-Process x; $p.Kill()",
            "Get-ChildItem | ForEach-Object { Remove-Item $_ }",
            "Invoke-WebRequest https://x",
            "cmd /c del x",
            "Set-Content a b",
            "Start-Process notepad",
            ". .\\script.ps1",
            "Get-ChildItem `\n| Remove-Item",
            "Get-ChildItem 'C:\\a (b)'",
            "Get-ChildItem | % Delete",
            "Get-Process notepad | % Kill",
            "Get-ChildItem | ForEach-Object Delete",
            "$p.('Kill')()",
            "$p.\"Kill\"()",
            "$p.'Kill'()",
            "$f.IsReadOnly = $true",
            "$f.LastWriteTime = Get-Date",
            "$f = Get-Item x; $f.Parent.IsReadOnly = $true",
            "$global:f.IsReadOnly = $true",
            "$f.Attributes += 'ReadOnly'",
            "$m = 'Kill'; $p = Get-Process x; $p.$m()",
        ] {
            assert!(!is_read_only(script), "should need a confirm: {script}");
        }
    }
}
