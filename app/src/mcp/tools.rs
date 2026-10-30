//! The tools the MCP server offers: names, descriptions and argument schemas.
//! The app carries them out (`app::mcp`).

use serde_json::{Value, json};

/// Sent to the client with `initialize`.
pub const INSTRUCTIONS: &str = "Listen is a desktop music player. Songs, albums, artists and playlists \
are referred to by the `ref` that search, get_album, get_playlist, get_queue and the other tools return \
(e.g. \"song:<server>|<id>\"); pass those refs back to play, queue, like or add items. Queue entries are referred \
to by their `uid`. Settings can be read with get_settings and changed with set_settings (a partial \
object, merged key by key).";

const PLAY_HOW: &str =
    "now = replace the queue and play, next = play after the current song, last = add to the end";

fn s(description: &str) -> Value {
    json!({ "type": "string", "description": description })
}

fn n(description: &str) -> Value {
    json!({ "type": "number", "description": description })
}

fn i(description: &str) -> Value {
    json!({ "type": "integer", "description": description })
}

fn b(description: &str) -> Value {
    json!({ "type": "boolean", "description": description })
}

fn refs(description: &str) -> Value {
    json!({ "type": "array", "items": { "type": "string" }, "description": description })
}

fn one_of(values: &[&str], description: &str) -> Value {
    json!({ "type": "string", "enum": values, "description": description })
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required },
    })
}

/// Whether `name` is one of the tools.
pub fn exists(name: &str) -> bool {
    definitions().iter().any(|t| t["name"] == name)
}

/// Every tool with its schema, for `tools/list`.
pub fn definitions() -> Vec<Value> {
    vec![
        // ---- playback
        tool(
            "get_now_playing",
            "The playing song, position and duration (seconds), whether it plays, volume, play mode, repeat, \
             speed and the server it comes from.",
            json!({}),
            &[],
        ),
        tool("play", "Resume playback.", json!({}), &[]),
        tool("pause", "Pause playback.", json!({}), &[]),
        tool("toggle_play", "Play or pause.", json!({}), &[]),
        tool("next", "Skip to the next song.", json!({}), &[]),
        tool("previous", "Go to the previous song (or the start of this one).", json!({}), &[]),
        tool(
            "seek",
            "Jump to a position in the playing song.",
            json!({ "seconds": n("Position in seconds") }),
            &["seconds"],
        ),
        tool("set_volume", "Set the volume.", json!({ "volume": i("0 to 100") }), &["volume"]),
        tool(
            "set_play_mode",
            "What happens when the queue runs out: off, shuffle, auto_dj (similar songs) or ai_dj (needs a \
             Gemini key).",
            json!({ "mode": one_of(&["off", "shuffle", "auto_dj", "ai_dj"], "The play mode") }),
            &["mode"],
        ),
        tool(
            "set_repeat",
            "Repeat nothing, the whole queue or the current song.",
            json!({ "repeat": one_of(&["off", "all", "one"], "Repeat mode") }),
            &["repeat"],
        ),
        tool("set_speed", "Playback speed.", json!({ "speed": n("0.5 to 2.0, 1.0 = normal") }), &["speed"]),
        tool(
            "sleep_timer",
            "Stop playback later: after some minutes, at the end of the song or album, or cancel the timer.",
            json!({
                "minutes": i("Stop after this many minutes"),
                "at": one_of(&["end_of_song", "end_of_album", "cancel"], "Instead of minutes"),
            }),
            &[],
        ),
        // ---- queue
        tool(
            "get_queue",
            "The queue with each entry's uid; `current` marks the playing one.",
            json!({}),
            &[],
        ),
        tool(
            "play_songs",
            "Play songs, albums, artists or playlists given by their refs.",
            json!({
                "refs": refs("Refs of songs, albums, artists or playlists"),
                "how": one_of(&["now", "next", "last"], PLAY_HOW),
                "shuffle": b("Shuffle them first"),
            }),
            &["refs"],
        ),
        tool(
            "add_to_queue",
            "Add songs, albums, artists or playlists to the end of the queue (or after the current song).",
            json!({ "refs": refs("Refs to add"), "next": b("Insert after the current song instead") }),
            &["refs"],
        ),
        tool(
            "play_queue_entry",
            "Jump to a queue entry.",
            json!({ "uid": i("The entry's uid from get_queue") }),
            &["uid"],
        ),
        tool(
            "remove_from_queue",
            "Remove entries from the queue (the playing one stays).",
            json!({ "uids": { "type": "array", "items": { "type": "integer" }, "description": "Entry uids" } }),
            &["uids"],
        ),
        tool(
            "move_in_queue",
            "Move entries to a position in the queue.",
            json!({
                "uids": { "type": "array", "items": { "type": "integer" } },
                "position": i("0-based index they should start at"),
            }),
            &["uids", "position"],
        ),
        tool("shuffle_queue", "Shuffle the queue (the playing song stays where it is).", json!({}), &[]),
        tool("clear_queue", "Destructive: empties the whole queue and stops playback.", json!({}), &[]),
        // ---- library
        tool("list_servers", "The music servers set up in Listen and which one is active.", json!({}), &[]),
        tool(
            "switch_server",
            "Make another server the active one.",
            json!({ "server": s("Server id from list_servers") }),
            &["server"],
        ),
        tool(
            "search",
            "Search songs, albums, artists and playlists on a server (the active one by default).",
            json!({
                "query": s("What to look for"),
                "server": s("Server id; default: the active server"),
                "limit": i("Results per kind, default 20"),
            }),
            &["query"],
        ),
        tool("get_album", "An album with its songs.", json!({ "ref": s("Album ref") }), &["ref"]),
        tool(
            "get_artist",
            "An artist with albums and top songs.",
            json!({ "ref": s("Artist ref") }),
            &["ref"],
        ),
        tool(
            "list_playlists",
            "The playlists on a server (the active one by default).",
            json!({ "server": s("Server id") }),
            &[],
        ),
        tool(
            "get_playlist",
            "A playlist with its songs (positions are 0-based).",
            json!({ "ref": s("Playlist ref") }),
            &["ref"],
        ),
        tool(
            "start_radio",
            "Play a radio of similar songs, seeded with a song or artist.",
            json!({ "ref": s("Song or artist ref") }),
            &["ref"],
        ),
        tool("get_lyrics", "The lyrics of the playing song, if any were found.", json!({}), &[]),
        // ---- playlists
        tool(
            "create_playlist",
            "Create a playlist on a server, optionally with songs.",
            json!({
                "name": s("Playlist name"),
                "server": s("Server id; default: the active server"),
                "refs": refs("Song refs to put in it"),
            }),
            &["name"],
        ),
        tool(
            "add_to_playlist",
            "Add songs (or the songs of albums/artists) to a playlist.",
            json!({ "playlist": s("Playlist ref"), "refs": refs("Refs to add") }),
            &["playlist", "refs"],
        ),
        tool(
            "remove_from_playlist",
            "Remove entries from a playlist by their 0-based positions (see get_playlist).",
            json!({
                "playlist": s("Playlist ref"),
                "positions": { "type": "array", "items": { "type": "integer" } },
            }),
            &["playlist", "positions"],
        ),
        tool(
            "rename_playlist",
            "Rename a playlist.",
            json!({ "playlist": s("Playlist ref"), "name": s("New name") }),
            &["playlist", "name"],
        ),
        tool(
            "delete_playlist",
            "Destructive: deletes a playlist on its server. This cannot be undone.",
            json!({ "playlist": s("Playlist ref") }),
            &["playlist"],
        ),
        // ---- favorites
        tool(
            "set_favorite",
            "Like or unlike songs, albums or artists (YouTube Music: like / save to library).",
            json!({ "refs": refs("Refs"), "favorite": b("true = like, false = unlike") }),
            &["refs", "favorite"],
        ),
        tool(
            "set_rating",
            "Rate songs, albums or artists (servers with ratings only).",
            json!({ "refs": refs("Refs"), "stars": i("0 (clear) to 5") }),
            &["refs", "stars"],
        ),
        // ---- AI DJ
        tool(
            "get_ai_dj",
            "The AI DJ's setup: on/off, persona, voice model, language, location, details, set size and \
             whether a Gemini key is stored (never the key itself).",
            json!({}),
            &[],
        ),
        tool("list_personas", "The AI DJ's personas with their voices.", json!({}), &[]),
        tool(
            "set_ai_dj",
            "Change the AI DJ. Only the given fields change.",
            json!({
                "enabled": b("Switch the AI DJ on or off (on needs a Gemini key)"),
                "persona": s("Persona id from list_personas"),
                "tts_model": one_of(&["gemini-3.8-flash-lite-tts", "gemini-3.8-flash-tts"], "Voice model"),
                "language": s("Language code the hosts speak (en, de, fr, ...); empty = the app's language"),
                "location": s("Where the show airs; empty = no home town"),
                "pronunciation": s("How the voice says the place name, e.g. \"Keel\" for Kiel"),
                "details": s("Free text for the hosts: names, likes, topics to avoid"),
                "set_max": i("Songs per set, 2 to 8"),
                "talk_every": i("Talk after every n-th set, 1 to 5"),
                "voice_gain_db": n("Voice loudness relative to the music, -12 to 12 dB"),
            }),
            &[],
        ),
        tool(
            "set_gemini_key",
            "Store a Gemini API key for the AI DJ in the system keyring (empty removes it). The key is never \
             returned by any tool.",
            json!({ "key": s("The API key from Google AI Studio") }),
            &["key"],
        ),
        // ---- settings
        tool(
            "get_settings",
            "All of Listen's settings as JSON (passwords and keys left out). Use the same shape with \
             set_settings.",
            json!({}),
            &[],
        ),
        tool(
            "set_settings",
            "Change settings: a partial settings object, merged key by key, e.g. {\"playback\":{\"gapless\":\
             \"yes\"}}. Invalid values are refused and reported; the rest is applied.",
            json!({ "settings": { "type": "object", "description": "Partial settings object" } }),
            &["settings"],
        ),
        tool(
            "get_analysis_status",
            "How far the song analysis for DJ transitions is (songs analysed, waiting, current song).",
            json!({}),
            &[],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_and_schemas_are_objects() {
        let defs = definitions();
        let mut names: Vec<&str> = defs.iter().filter_map(|t| t["name"].as_str()).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "duplicate tool names");
        for t in &defs {
            assert_eq!(t["inputSchema"]["type"], "object", "{}", t["name"]);
            assert!(t["description"].as_str().is_some_and(|d| d.len() > 10), "{}", t["name"]);
            for req in t["inputSchema"]["required"].as_array().unwrap() {
                assert!(t["inputSchema"]["properties"].get(req.as_str().unwrap()).is_some(), "{}", t["name"]);
            }
        }
        assert!(exists("search") && !exists("format_disk"));
    }
}
