//! Shared X11 session and EWMH helpers.
//!
//! Bloom talks to a single X server for both click-through hit testing and
//! window management. An X11 session is either present for the lifetime of the
//! process or absent entirely, so one lazily opened connection is reused instead
//! of opening a new one per poll.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};
use x11rb::{
    connection::Connection,
    protocol::xproto::{
        Atom, AtomEnum, ClientMessageEvent, ConnectionExt, EventMask, MapState, Window,
        CLIENT_MESSAGE_EVENT,
    },
    rust_connection::RustConnection,
};

pub struct Session {
    connection: Mutex<RustConnection>,
    pub root: Window,
    /// Cached atoms avoid a round trip on every monitor tick.
    atoms: Mutex<HashMap<&'static str, Atom>>,
}

impl Session {
    fn open() -> Result<Self, String> {
        let (connection, screen) =
            x11rb::connect(None).map_err(|e| format!("No X11 session available: {e}"))?;
        let root = connection.setup().roots[screen].root;
        Ok(Self {
            connection: Mutex::new(connection),
            root,
            atoms: Mutex::new(HashMap::new()),
        })
    }

    /// Borrow the connection for a request. Kept public so the click-through
    /// service can share this session instead of opening its own.
    pub fn connection_guard(&self) -> Result<std::sync::MutexGuard<'_, RustConnection>, String> {
        self.connection
            .lock()
            .map_err(|_| "X11 connection lock is unavailable".to_string())
    }

    /// Intern an atom once and reuse it for the life of the session.
    pub fn atom(&self, name: &'static str) -> Result<Atom, String> {
        if let Some(atom) = self
            .atoms
            .lock()
            .map_err(|_| "X11 atom cache is unavailable".to_string())?
            .get(name)
        {
            return Ok(*atom);
        }
        let atom = self
            .connection_guard()?
            .intern_atom(false, name.as_bytes())
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?
            .atom;
        self.atoms
            .lock()
            .map_err(|_| "X11 atom cache is unavailable".to_string())?
            .insert(name, atom);
        Ok(atom)
    }

    /// Cardinal (32-bit) property values, empty when the property is absent.
    pub fn cardinals(&self, window: Window, property: Atom) -> Vec<u32> {
        let Ok(connection) = self.connection_guard() else {
            return Vec::new();
        };
        connection
            .get_property(false, window, property, AtomEnum::ANY, 0, 4096)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .and_then(|reply| reply.value32().map(Iterator::collect))
            .unwrap_or_default()
    }

    /// UTF-8/STRING property value, trimmed of its NUL terminator.
    pub fn text(&self, window: Window, property: Atom) -> Option<String> {
        let connection = self.connection_guard().ok()?;
        let reply = connection
            .get_property(false, window, property, AtomEnum::ANY, 0, 1024)
            .ok()?
            .reply()
            .ok()?;
        let text = String::from_utf8_lossy(&reply.value)
            .trim_end_matches('\0')
            .to_owned();
        (!text.is_empty()).then_some(text)
    }

    /// Whether a window is actually on screen right now.
    ///
    /// `MapState` is `IsViewable` only when the window and every ancestor are
    /// mapped, so a minimized window reports `IsUnmapped` here while keeping its
    /// last geometry. Callers that reason about screen coverage must check this
    /// rather than trusting geometry alone.
    pub fn is_viewable(&self, window: Window) -> bool {
        let Ok(connection) = self.connection_guard() else {
            return false;
        };
        matches!(
            connection
                .get_window_attributes(window)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .map(|attributes| attributes.map_state),
            Some(MapState::VIEWABLE)
        )
    }

    /// Root-relative geometry of a window as `(x, y, width, height)`.
    ///
    /// `GetGeometry` reports the position within the window's parent, which for
    /// a reparented client window is the window manager's frame, so the position
    /// is translated to the root to get the real screen coordinates.
    pub fn root_geometry(&self, window: Window) -> Option<(i32, i32, u32, u32)> {
        let connection = self.connection_guard().ok()?;
        let geometry = connection.get_geometry(window).ok()?.reply().ok()?;
        let translated = connection
            .translate_coordinates(window, self.root, 0, 0)
            .ok()?
            .reply()
            .ok()?;
        Some((
            i32::from(translated.dst_x),
            i32::from(translated.dst_y),
            u32::from(geometry.width),
            u32::from(geometry.height),
        ))
    }

    /// Send an EWMH client message about `target`, as a taskbar legitimately
    /// does.
    ///
    /// Two different windows are involved and they are not interchangeable: the
    /// message *concerns* `target` (activate it, close it, iconify it) while it
    /// is *delivered* to the root window for the window manager to pick up. The
    /// window manager reads the target out of the message, so addressing the
    /// message at the root instead of at the target is accepted and then
    /// silently ignored.
    pub fn send_client_message_to_root(
        &self,
        target: Window,
        message_type: Atom,
        data: [u32; 5],
    ) -> Result<(), String> {
        let event = ClientMessageEvent {
            response_type: CLIENT_MESSAGE_EVENT,
            format: 32,
            sequence: 0,
            window: target,
            type_: message_type,
            data: data.into(),
        };
        let connection = self.connection_guard()?;
        connection
            .send_event(
                false,
                self.root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            )
            .map_err(|e| e.to_string())?;
        connection.flush().map_err(|e| e.to_string())
    }

    /// Read a string setting published by the desktop's XSETTINGS manager.
    ///
    /// Cinnamon and other desktops do not always write a gtk settings.ini, so
    /// this is how the real icon theme is discovered.
    pub fn xsettings_string(&self, key: &str) -> Option<String> {
        // Intern both atoms before taking the connection lock, which is not
        // reentrant.
        let selection = self.atom("_XSETTINGS_S0").ok()?;
        let property = self.atom("_XSETTINGS_SETTINGS").ok()?;
        let connection = self.connection_guard().ok()?;
        let owner = connection
            .get_selection_owner(selection)
            .ok()?
            .reply()
            .ok()?
            .owner;
        if owner == x11rb::NONE {
            return None;
        }
        let reply = connection
            .get_property(false, owner, property, AtomEnum::ANY, 0, 1 << 16)
            .ok()?
            .reply()
            .ok()?;
        xsettings_string_value(&reply.value, key)
    }
}

fn alignment_padding(length: usize) -> usize {
    (4 - (length % 4)) % 4
}

/// Find a string setting inside an `_XSETTINGS_SETTINGS` value.
///
/// The layout is defined by the XSETTINGS specification: a header, then one
/// record per setting, each padded to four byte boundaries.
fn xsettings_string_value(data: &[u8], key: &str) -> Option<String> {
    if data.len() < 12 {
        return None;
    }
    let big_endian = match data[0] {
        0 => false,
        1 => true,
        _ => return None,
    };
    let read_u32 = |bytes: &[u8]| -> Option<u32> {
        let array: [u8; 4] = bytes.get(..4)?.try_into().ok()?;
        Some(if big_endian {
            u32::from_be_bytes(array)
        } else {
            u32::from_le_bytes(array)
        })
    };
    let read_u16 = |bytes: &[u8]| -> Option<u16> {
        let array: [u8; 2] = bytes.get(..2)?.try_into().ok()?;
        Some(if big_endian {
            u16::from_be_bytes(array)
        } else {
            u16::from_le_bytes(array)
        })
    };

    let count = read_u32(&data[8..])? as usize;
    let mut position = 12;
    for _ in 0..count {
        // Setting header: type, one unused byte, then the name length.
        let setting_type = *data.get(position)?;
        let name_length = read_u16(data.get(position + 2..)?)? as usize;
        position += 4;
        let name = std::str::from_utf8(data.get(position..position + name_length)?).ok()?;
        position += name_length + alignment_padding(name_length);
        position += 4; // last-change serial
        match setting_type {
            0 => position += 4, // integer value
            1 => {
                let value_length = read_u32(data.get(position..)?)? as usize;
                position += 4;
                let value =
                    std::str::from_utf8(data.get(position..position + value_length)?).ok()?;
                if name == key {
                    return Some(value.to_owned());
                }
                position += value_length + alignment_padding(value_length);
            }
            2 => position += 8, // colour value
            _ => return None,
        }
    }
    None
}

/// The process-wide X11 session, or an error when Bloom is not on X11.
pub fn session() -> Result<&'static Session, String> {
    static SESSION: OnceLock<Result<Session, String>> = OnceLock::new();
    SESSION.get_or_init(Session::open).as_ref().map_err(Clone::clone)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string_setting(buffer: &mut Vec<u8>, name: &str, value: &str) {
        buffer.push(1);
        buffer.push(0);
        buffer.extend_from_slice(&(name.len() as u16).to_le_bytes());
        buffer.extend_from_slice(name.as_bytes());
        buffer.extend(std::iter::repeat(0).take(alignment_padding(name.len())));
        buffer.extend_from_slice(&7u32.to_le_bytes());
        buffer.extend_from_slice(&(value.len() as u32).to_le_bytes());
        buffer.extend_from_slice(value.as_bytes());
        buffer.extend(std::iter::repeat(0).take(alignment_padding(value.len())));
    }

    fn integer_setting(buffer: &mut Vec<u8>, name: &str, value: i32) {
        buffer.push(0);
        buffer.push(0);
        buffer.extend_from_slice(&(name.len() as u16).to_le_bytes());
        buffer.extend_from_slice(name.as_bytes());
        buffer.extend(std::iter::repeat(0).take(alignment_padding(name.len())));
        buffer.extend_from_slice(&7u32.to_le_bytes());
        buffer.extend_from_slice(&value.to_le_bytes());
    }

    fn settings_blob(settings: &[(&str, &str, bool)]) -> Vec<u8> {
        let mut buffer = vec![0, 0, 0, 0];
        buffer.extend_from_slice(&1u32.to_le_bytes());
        buffer.extend_from_slice(&(settings.len() as u32).to_le_bytes());
        for (name, value, is_string) in settings {
            if *is_string {
                string_setting(&mut buffer, name, value);
            } else {
                integer_setting(&mut buffer, name, value.parse().unwrap_or_default());
            }
        }
        buffer
    }

    #[test]
    fn reads_a_string_setting_from_a_typical_blob() {
        let blob = settings_blob(&[
            ("Gtk/EnableAnimations", "1", false),
            ("Net/IconThemeName", "Mint-L", true),
            ("Net/ThemeName", "Mint-Y", true),
        ]);
        assert_eq!(
            xsettings_string_value(&blob, "Net/IconThemeName").as_deref(),
            Some("Mint-L")
        );
        assert_eq!(
            xsettings_string_value(&blob, "Net/ThemeName").as_deref(),
            Some("Mint-Y")
        );
        assert!(xsettings_string_value(&blob, "Net/NotPresent").is_none());
    }

    #[test]
    fn ignores_integer_and_colour_records() {
        // A colour record sits between two strings and must be skipped by size.
        let mut blob = settings_blob(&[("Net/IconThemeName", "Papirus", true)]);
        blob[8..12].copy_from_slice(&2u32.to_le_bytes());
        blob.extend_from_slice(&[2, 0, 5, 0]);
        blob.extend_from_slice(b"Net/C");
        blob.extend_from_slice(&0u32.to_le_bytes());
        blob.extend_from_slice(&[0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]);
        assert_eq!(
            xsettings_string_value(&blob, "Net/IconThemeName").as_deref(),
            Some("Papirus")
        );
    }

    #[test]
    fn malformed_blobs_are_rejected_without_panicking() {
        assert!(xsettings_string_value(&[], "Net/IconThemeName").is_none());
        assert!(xsettings_string_value(&[9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], "x").is_none());
        assert!(xsettings_string_value(&[0, 0, 0, 0, 0, 0, 0, 0, 5, 0, 0, 0], "x").is_none());
    }
}
