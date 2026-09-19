//! The desktop's accent colour, used by Bloom's adaptive theme.
//!
//! Windows reads the accent from the registry and falls back to the DWM
//! colorization colour. Linux has no single equivalent, so this follows the
//! desktops in the order they apply: Cinnamon's own accent setting, then the
//! XApp portal that Linux Mint ships, then the GNOME key that most GTK desktops
//! expose.
//!
//! Where none of them exist, there is genuinely no system accent to read, and
//! that is reported as an error rather than invented. Bloom's adaptive mode
//! keeps its previous colours in that case.

use std::process::Command;

/// The accent choices Cinnamon and GNOME expose, as the standard Adwaita
/// palette the desktops draw from.
const PALETTE: [(&str, &str); 9] = [
    ("blue", "#3584e4"),
    ("teal", "#2190a4"),
    ("green", "#3a944a"),
    ("yellow", "#c88800"),
    ("orange", "#ed5b00"),
    ("red", "#e62d42"),
    ("pink", "#d56199"),
    ("purple", "#9141ac"),
    ("slate", "#6f8396"),
];

/// Read one setting through `gsettings`, which is how both Cinnamon and GNOME
/// publish these values. A missing schema or key (a desktop without accent
/// support) is reported as absent rather than as an error.
fn setting(schema: &str, key: &str) -> Option<String> {
    let output = Command::new("gsettings")
        .args(["get", schema, key])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    // `gsettings` prints strings quoted, and an unset one as ''.
    let value = value.trim_matches('\'').trim().to_owned();
    (!value.is_empty()).then_some(value)
}

/// Map an accent name from the desktop's palette onto its hex colour.
fn palette_hex(name: &str) -> Option<String> {
    let name = name.trim().to_ascii_lowercase();
    PALETTE
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, hex)| (*hex).to_owned())
}

/// Normalise an `accent-rgb` style value, which may be `#rrggbb`, bare
/// `rrggbb`, `#rgb`, or a comma-separated triple.
fn parse_rgb(value: &str) -> Option<String> {
    let value = value.trim().trim_matches('\'').trim().to_ascii_lowercase();
    if let Some(rest) = value.strip_prefix('#') {
        return match rest.len() {
            3 => {
                // Expand the short form, doubling each nibble.
                let expanded: String = rest.chars().flat_map(|c| [c, c]).collect();
                is_hex(&expanded).then(|| format!("#{expanded}"))
            }
            6 => is_hex(rest).then(|| format!("#{rest}")),
            _ => None,
        };
    }
    if is_hex(&value) && value.len() == 6 {
        return Some(format!("#{value}"));
    }
    let parts: Vec<&str> = value.split(',').map(str::trim).collect();
    if parts.len() == 3 {
        let channels: Option<Vec<u8>> = parts
            .iter()
            .map(|part| part.parse::<u8>().ok())
            .collect();
        if let Some(channels) = channels {
            return Some(format!(
                "#{:02x}{:02x}{:02x}",
                channels[0], channels[1], channels[2]
            ));
        }
    }
    None
}

fn is_hex(value: &str) -> bool {
    value.chars().all(|c| c.is_ascii_hexdigit())
}

/// The desktop's accent colour as `#rrggbb`.
pub fn accent_color() -> Result<String, String> {
    // Cinnamon's own setting wins where the version has it, then the XApp
    // portal Mint uses, then the GNOME key used by other GTK desktops.
    for (schema, key) in [
        ("org.cinnamon.desktop.interface", "accent-color"),
        ("org.gnome.desktop.interface", "accent-color"),
    ] {
        if let Some(hex) = setting(schema, key).and_then(|value| palette_hex(&value)) {
            return Ok(hex);
        }
    }
    if let Some(hex) = setting("org.x.apps.portal", "accent-rgb").and_then(|value| parse_rgb(&value))
    {
        return Ok(hex);
    }
    Err("This desktop does not expose an accent colour".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_names_map_to_their_colour() {
        assert_eq!(palette_hex("blue").as_deref(), Some("#3584e4"));
        assert_eq!(palette_hex("Purple").as_deref(), Some("#9141ac"));
        assert_eq!(palette_hex("chartreuse"), None);
    }

    #[test]
    fn rgb_values_are_normalised() {
        assert_eq!(parse_rgb("#3584E4").as_deref(), Some("#3584e4"));
        assert_eq!(parse_rgb("3584e4").as_deref(), Some("#3584e4"));
        // Short form expands rather than erroring.
        assert_eq!(parse_rgb("#abc").as_deref(), Some("#aabbcc"));
        assert_eq!(parse_rgb("53, 132, 228").as_deref(), Some("#3584e4"));
    }

    #[test]
    fn unusable_rgb_values_are_rejected() {
        assert_eq!(parse_rgb(""), None);
        assert_eq!(parse_rgb("#12345"), None);
        assert_eq!(parse_rgb("zzzzzz"), None);
        // Out of range channels are not silently wrapped.
        assert_eq!(parse_rgb("300,0,0"), None);
    }
}
