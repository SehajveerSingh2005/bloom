//! Media transport and metadata through MPRIS over D-Bus.
//!
//! This replaces the Windows GSMTC integration. Bloom identifies itself as an
//! ordinary MPRIS client: it reads the properties a media widget needs and
//! issues the standard transport calls.

use crate::{state::ANY_MEDIA_PLAYING, types::MediaInfo};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::{
    collections::HashMap,
    path::Path,
    sync::{mpsc::Sender, atomic::Ordering, OnceLock},
    time::Duration,
};
use tauri::{AppHandle, Emitter};
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::{ObjectPath, OwnedValue, Value},
};

/// Every MPRIS player claims a bus name starting with this prefix.
const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const PLAYER_PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";
const DBUS_SERVICE: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
const DBUS_INTERFACE: &str = "org.freedesktop.DBus";

/// Transport requests are funnelled through the polling thread, which owns the
/// active player and its track id.
#[derive(Clone, Copy, Debug)]
pub enum MediaCommand {
    PlayPause,
    Next,
    Previous,
    Seek(i64),
}

static SENDER: OnceLock<Sender<MediaCommand>> = OnceLock::new();

/// Send a transport command to the media thread.
pub fn send(command: MediaCommand) -> Result<(), String> {
    SENDER
        .get()
        .ok_or_else(|| "Media control is not available".to_string())?
        .send(command)
        .map_err(|_| "Media control has stopped".to_string())
}

fn take_string(metadata: &mut HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    let value: Value<'static> = metadata.remove(key)?.into();
    String::try_from(value).ok().filter(|text| !text.is_empty())
}

fn take_string_list(metadata: &mut HashMap<String, OwnedValue>, key: &str) -> Vec<String> {
    let Some(owned) = metadata.remove(key) else {
        return Vec::new();
    };
    let value: Value<'static> = owned.into();
    Vec::<String>::try_from(value).unwrap_or_default()
}

fn take_i64(metadata: &mut HashMap<String, OwnedValue>, key: &str) -> Option<i64> {
    let value: Value<'static> = metadata.remove(key)?.into();
    i64::try_from(value).ok()
}

fn take_object_path(metadata: &mut HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    let value: Value<'static> = metadata.remove(key)?.into();
    ObjectPath::try_from(value)
        .ok()
        .map(|path| path.as_str().to_owned())
}

/// Read `mpris:artUrl` into a data URI.
///
/// The webview's CSP only allows `data:` and `blob:` images, so artwork must be
/// inlined. Local files are read directly; remote URLs are deliberately not
/// fetched, because that would add a network dependency and leak playback
/// activity.
fn artwork_data_uri(metadata: &mut HashMap<String, OwnedValue>) -> Option<String> {
    let url = take_string(metadata, "mpris:artUrl")?;
    let path = url.strip_prefix("file://")?;
    // Percent-encoded paths are common; decode the escapes we are likely to see.
    let decoded = percent_decode(path);
    let path = Path::new(&decoded);
    let mime = match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => return None,
    };
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.len() > 8 * 1024 * 1024 {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    Some(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

struct Snapshot {
    player: String,
    track_id: Option<String>,
    info: MediaInfo,
}

/// Read one player's current state. Returns `None` for players that expose no
/// usable metadata (a stopped player, for example).
fn snapshot(connection: &Connection, player: &str) -> Option<Snapshot> {
    let proxy = Proxy::new(connection, player, PLAYER_PATH, PLAYER_INTERFACE).ok()?;
    let status: String = proxy.get_property("PlaybackStatus").ok()?;
    let mut metadata: HashMap<String, OwnedValue> = proxy.get_property("Metadata").ok()?;
    let title = take_string(&mut metadata, "xesam:title")?;
    let track_id = take_object_path(&mut metadata, "mpris:trackid");

    let artists = take_string_list(&mut metadata, "xesam:artist");
    let artist = if artists.is_empty() {
        take_string(&mut metadata, "xesam:albumArtist").unwrap_or_default()
    } else {
        artists.join(", ")
    };

    let duration_ms = take_i64(&mut metadata, "mpris:length")
        .map(|micros| (micros / 1000).max(0))
        .unwrap_or(0);
    let artwork = artwork_data_uri(&mut metadata).map(|uri| vec![uri]);
    // `Position` is only meaningful while a track is loaded.
    let position_ms = proxy
        .get_property::<i64>("Position")
        .map(|micros| (micros / 1000).max(0))
        .unwrap_or(0);
    let seek_enabled = proxy.get_property::<bool>("CanSeek").unwrap_or(false);

    Some(Snapshot {
        player: player.to_owned(),
        track_id,
        info: MediaInfo {
            title,
            artist,
            is_playing: status == "Playing",
            has_media: true,
            artwork,
            position_ms,
            duration_ms,
            seek_enabled,
            position_updated_at: now_ms(),
        },
    })
}

fn run_command(connection: &Connection, active: Option<&Snapshot>, command: MediaCommand) {
    let Some(active) = active else { return };
    let Ok(proxy) = Proxy::new(connection, active.player.as_str(), PLAYER_PATH, PLAYER_INTERFACE)
    else {
        return;
    };
    match command {
        MediaCommand::PlayPause => {
            let _ = proxy.call_noreply("PlayPause", &());
        }
        MediaCommand::Next => {
            let _ = proxy.call_noreply("Next", &());
        }
        MediaCommand::Previous => {
            let _ = proxy.call_noreply("Previous", &());
        }
        MediaCommand::Seek(position_ms) => {
            // MPRIS `SetPosition` takes the track object path and microseconds.
            let Some(track_id) = active.track_id.as_deref() else {
                return;
            };
            if let Ok(track) = ObjectPath::try_from(track_id.to_owned()) {
                let _ = proxy.call_noreply("SetPosition", &(track, position_ms * 1000));
            }
        }
    }
}

/// Poll MPRIS players and emit `media-update` whenever the visible state
/// changes. Mirrors the Windows backend: playing players win, then the first
/// player that has metadata, and position is only re-emitted on a >1s change.
fn empty_snapshot() -> Snapshot {
    Snapshot {
        player: String::new(),
        track_id: None,
        info: MediaInfo {
            title: String::new(),
            artist: String::new(),
            is_playing: false,
            has_media: false,
            artwork: None,
            position_ms: 0,
            duration_ms: 0,
            seek_enabled: false,
            position_updated_at: 0,
        },
    }
}

/// Prefer a playing player, otherwise keep the first one that has metadata.
fn choose(best: Option<Snapshot>, candidate: Snapshot) -> Option<Snapshot> {
    match best {
        Some(current) if current.info.is_playing => Some(current),
        Some(current) => {
            if candidate.info.is_playing {
                Some(candidate)
            } else {
                Some(current)
            }
        }
        None => Some(candidate),
    }
}

/// One poll cycle. Returns `false` when the session bus is no longer usable, so
/// the caller reconnects (for example after a bus restart).
fn poll_once(
    app: &AppHandle,
    connection: &Connection,
    receiver: &std::sync::mpsc::Receiver<MediaCommand>,
    active: &mut Option<Snapshot>,
    last_emitted: &mut Option<MediaInfo>,
) -> bool {
    while let Ok(command) = receiver.try_recv() {
        run_command(connection, active.as_ref(), command);
    }

    let Ok(proxy) = Proxy::new(connection, DBUS_SERVICE, DBUS_PATH, DBUS_INTERFACE) else {
        return false;
    };
    let Ok(names) = proxy.call::<_, _, Vec<String>>("ListNames", &()) else {
        return false;
    };

    let best = names
        .into_iter()
        .filter(|name| name.starts_with(MPRIS_PREFIX))
        .filter_map(|player| snapshot(connection, &player))
        .fold(None::<Snapshot>, choose);
    let current = best.unwrap_or_else(empty_snapshot);

    ANY_MEDIA_PLAYING.store(current.info.is_playing, Ordering::Relaxed);

    let artwork = current.info.artwork.as_ref().and_then(|art| art.first());
    let changed = match last_emitted {
        None => true,
        Some(previous) => {
            previous.title != current.info.title
                || previous.artist != current.info.artist
                || previous.is_playing != current.info.is_playing
                || previous.has_media != current.info.has_media
                || previous.artwork.as_ref().and_then(|art| art.first()) != artwork
                || (previous.position_ms - current.info.position_ms).abs() > 1000
        }
    };
    if changed {
        let _ = app.emit("media-update", &current.info);
        *last_emitted = Some(current.info.clone());
    }
    *active = Some(current);
    true
}

pub fn start(app: AppHandle) {
    let (sender, receiver) = std::sync::mpsc::channel::<MediaCommand>();
    let _ = SENDER.set(sender);

    std::thread::spawn(move || {
        let mut last_emitted: Option<MediaInfo> = None;
        let mut active: Option<Snapshot> = None;
        let mut connection: Option<Connection> = None;
        loop {
            if connection.is_none() {
                match Connection::session() {
                    Ok(session) => connection = Some(session),
                    Err(_) => {
                        std::thread::sleep(Duration::from_secs(5));
                        continue;
                    }
                }
            }
            let Some(session) = connection.as_ref() else {
                continue;
            };
            if !poll_once(&app, session, &receiver, &mut active, &mut last_emitted) {
                // The bus went away; drop the connection and retry.
                connection = None;
                active = None;
            }
            std::thread::sleep(Duration::from_millis(2000));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encoded_paths_are_decoded() {
        assert_eq!(percent_decode("/music/My%20Song.png"), "/music/My Song.png");
        assert_eq!(percent_decode("/plain/path.png"), "/plain/path.png");
        assert_eq!(percent_decode("/odd/%zz.png"), "/odd/%zz.png");
    }

    /// Build the `OwnedValue` a real `a{sv}` metadata dictionary would contain.
    fn owned(value: Value<'static>) -> OwnedValue {
        value.try_to_owned().expect("value should convert")
    }

    #[test]
    fn metadata_values_are_extracted_by_type() {
        let mut metadata: HashMap<String, OwnedValue> = HashMap::new();
        metadata.insert("xesam:title".into(), owned(Value::from("Song")));
        metadata.insert(
            "xesam:artist".into(),
            owned(Value::from(vec!["First", "Second"])),
        );
        metadata.insert(
            "mpris:length".into(),
            owned(Value::from(135_000_000i64)),
        );

        assert_eq!(take_string(&mut metadata, "xesam:title").as_deref(), Some("Song"));
        assert_eq!(
            take_string_list(&mut metadata, "xesam:artist"),
            vec!["First", "Second"]
        );
        assert_eq!(take_i64(&mut metadata, "mpris:length"), Some(135_000_000));
        // Missing keys and wrong types must not panic.
        assert!(take_string(&mut metadata, "xesam:title").is_none());
        assert!(take_i64(&mut metadata, "noxesam:album").is_none());
    }

    #[test]
    fn send_reports_a_useful_error_before_start() {
        // Before `start`, transport commands must fail explicitly.
        assert!(send(MediaCommand::PlayPause).is_err());
    }
}
