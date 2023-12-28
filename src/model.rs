//! Payloads to control Lavalink players.
//!
//! Some of them are used to fetch metadata about tracks, playlists or even
//! results from a search engine result (e.g., `ytsearch`, `soundcloud`).
//!
//! Theres're also structs that contain the current internal state of a given
//! node and route planner configuration.
//! TODO: last one not implemented yet.

use std::{collections::HashMap, fmt::Display};

use serde::{Deserialize, Serialize, de};
use serde_json::value::RawValue;

// ############### Types ###############

/// Guild Identifier.
type GuildId = String;
/// Milliseconds representation.
pub type Milli = u64;
/// Seconds representation.
pub type Secs = u64;
/// HTTP status code.
pub type StatusCode = u16;
/// Player volume.
pub type Volume = u16;
/// Player filter volume adjustment.
pub type FilterVolume = f64;
/// Unparsed json (i.e. stills in raw format).
pub type BoxedRawData= Box<RawValue>;
/// Plugin identifier.
pub type PluginName = String;
/// Discord close event code.
pub type CloseEventCode = u16;

// ############### Models ###############

/// Contains the decoded error metadata.
///
/// This error is returned by some Lavalink instance upon an unexpected
/// behaviour while loading tracks or controling the players trough its
/// REST API.
///
/// Note that trace is ommited here, since it isn't set in any call to the API.
#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct ApiError {
    /// The error time in milliseconds since Unix epoch.
    pub timestamp: Milli,
    /// HTTP status code.
    pub status: StatusCode,
    /// HTTP Status code error message.
    pub error: String,
    /// Error message (i.e. explanation).
    pub message: String,
}

impl Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "got: {}; reason: {}", self.status, self.message)
    }
}

/// Represents the track description.
#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct TrackInfo {
    /// The track id.
    pub identifier: String,
    /// Whether the track is seekable.
    #[serde(rename = "isSeekable")]
    pub is_seekable: bool,
    /// The track author.
    pub author: String,
    /// The track length in milliseconds.
    pub length: Milli,
    /// Whether the track is a stream.
    #[serde(rename = "isStream")]
    pub is_stream: bool,
    /// The track position in milliseconds.
    pub position: Milli,
    /// The track Title.
    pub title: String,
    /// The track uri (it may not be present).
    pub uri: Option<String>,
    /// The track artwork url aka thumbnail (it may not be present).
    #[serde(rename = "artworkUrl")]
    pub artwork_url: Option<String>,
    /// The track International Standard Recording Code (it may not be present).
    pub isrc: Option<String>,
    /// The track source (e.g. youtube or soundcloud),
    #[serde(rename = "sourceName")]
    pub source_name: String,
}

/// Contains the decoded track metadata.
#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct TrackData {
    /// The track unique identifier.
    pub encoded: String,
    /// The track description.
    pub info: TrackInfo,
    /// Aditional info that may be injected by some plugin.
    #[serde(rename = "pluginInfo")]
    pub plugin_info: BoxedRawData,
    /// Additional user data that might have been sent in the update player
    /// endpoint.
    #[serde(rename = "userData")]
    pub user_data: BoxedRawData,
}

/// Contains the playlist info.
#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct PlaylistInfo {
    /// The name of the playlist.
    pub name: String,
    /// Selected track index, if any.
    #[serde(rename = "selectedTrack")]
    #[serde(deserialize_with = "deserialize_selected_track")]
    pub selected_track: Option<u64>,
}

/// Contains the decoded playlist metadata.
#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct PlaylistData {
    /// The playlist info.
    info: PlaylistInfo,
    /// Aditional info that may be injected by some plugin.
    #[serde(rename = "pluginInfo")]
    plugin_info: BoxedRawData,
    /// Collection of tracks.
    tracks: Vec<TrackData>,
}

/// Contains the response got from loading tracks.
#[derive(Deserialize, Debug)]
#[serde(tag = "loadType", content = "data")]
#[allow(dead_code)]
pub enum LoadResult {
    /// When searching by the track identifier or its url.
    #[serde(rename = "track")]
    SingleTrack(TrackData),
    /// When searching by the playlist identifier or its url.
    #[serde(rename = "playlist")]
    Playlist(PlaylistData),
    /// When a search engine is used (e.g., `ytsearch`, `soundcloud`).
    #[serde(rename = "search")]
    TracksSearch(Vec<TrackData>),
    /// When there's no match for the given identifier/url.
    #[serde(rename = "empty")]
    EmptyMatch(
        #[serde(deserialize_with = "deserialize_empty_match")]
        ()
    ),
    /// When something went wrong.
    #[serde(rename = "error")]
    Fail(ApiError),
}

pub use self::LoadResult::*;

/// Represents the player state (e.g., if is connected, current track
/// position...).
#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct PlayerState {
    /// Unix timestamp in milliseconds,
    pub time: Milli,
    /// Position of the track in milliseconds.
    pub position: Milli,
    /// Whether Lavalink is connected to the voice gateway.
    pub connected: bool,
    /// The ping of the node to the Dsicord voice server in milliseconds.
    #[serde(deserialize_with = "deserialize_ping")]
    pub ping: Option<Milli>,
}

/// Represents the player voice channel state.
#[derive(Deserialize, Serialize, Debug)]
#[allow(dead_code)]
pub struct PlayerVoiceState {
    /// The Discord voice token to authenticate with.
    pub token: String,
    /// The Discord voice endpoint to connect to.
    pub endpoint: String,
    /// The Discord voice session id to authenticate with.
    #[serde(rename = "sessionId")]
    pub session_id: String,
}

/// Represents an equilizer band.
///
/// The [`gain`] is the multiplier for the given band (defaults to 0). -0.25
/// means the given band is completely muted, while 0.25 means it's doubled.
/// Besides that, it may change the volume of the output.
#[derive(Deserialize, Serialize, Debug)]
#[allow(dead_code)]
pub struct Equalizer {
    /// Band must be 0 <= x <= 14.
    pub band: u8,
    /// Gain must be -0.25 <= x <= 1.0.
    pub gain: f64,
}

/// Represents a karaoke equilization.
///
/// Used to eliminate part of a band, usually targeting vocals.
///
/// Level and mono level must be 0.0 (low effect) <= x <= 1.0 (full effect).
#[allow(missing_docs)]
#[derive(Deserialize, Serialize, Debug)]
#[allow(dead_code)]
pub struct Kareoke {
    pub level: Option<f64>,
    #[serde(rename = "monoLevel")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mono_level: Option<f64>,
    #[serde(rename = "filterBand")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_band: Option<f64>,
    #[serde(rename = "filterWidth")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_width: Option<f64>,
}

/// Changes the speed, pitch and rate.
///
/// For each variant, 1.0 is the default value and must be >= 0.0.
#[allow(missing_docs)]
#[derive(Deserialize, Serialize, Debug)]
#[allow(dead_code)]
pub struct Timescale {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pitch: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate: Option<f64>,
}

/// Create a shuddering effect by using amplification, where the volume quicky
/// oscillates.
#[derive(Deserialize, Serialize, Debug)]
#[allow(dead_code)]
pub struct Tremolo {
    /// Frenquency must be > 0.0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency: Option<f64>,
    /// Depth must be between 0.0 < x <= 1.0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth: Option<f64>,
}

/// Similar to tremolo, but this one oscillates the pitch.
#[derive(Deserialize, Serialize, Debug)]
#[allow(dead_code)]
pub struct Vibrato {
    /// Frequency must be 0.0 < x <= 14.0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency: Option<f64>,
    /// Vibrato depth must be 0.0 < x <= 1.0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth: Option<f64>,
}

/// Rotates the sound around the stereo channels/user headphones (also known as
/// Audio Panning).
#[derive(Deserialize, Serialize, Debug)]
#[allow(dead_code)]
pub struct Rotation {
    /// The frequency of the audio rotating around the listener in Hz.
    #[serde(rename = "rotationHz")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
}

/// Represents the distortion effect.
#[allow(missing_docs)]
#[derive(Deserialize, Serialize, Debug)]
#[allow(dead_code)]
pub struct Distortion {
    #[serde(rename = "sinOffset")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sin_offset: Option<f64>,
    #[serde(rename = "sinScale")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sin_scale: Option<f64>,
    #[serde(rename = "cosOffset")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cos_offset: Option<f64>,
    #[serde(rename = "cosScale")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cos_scale: Option<f64>,
    #[serde(rename = "tanOffset")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tan_offset: Option<f64>,
    #[serde(rename = "tanScale")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tan_scale: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
}

/// Mixes both channels (left and right), with a configurable factor on how much
/// each channel affects the other.
///
/// With the defaults, both channels are kept independent of each other.
///
/// Each channel mix factor must be 0.0 <= x <= 1.0.
///
/// Setting all factors to 0.5 means both channels get the same audio.
#[allow(missing_docs)]
#[derive(Deserialize, Serialize, Debug)]
#[allow(dead_code)]
pub struct ChannelMix {
    #[serde(rename = "leftToLeft")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left_to_left: Option<f64>,
    #[serde(rename = "leftToRight")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left_to_right: Option<f64>,
    #[serde(rename = "rightToLeft")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right_to_left: Option<f64>,
    #[serde(rename = "rightToRight")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right_to_right: Option<f64>,
}

/// Higher frequencies get suppressed, while lower frequencies pass through this
/// filter, thus the name low pass.
///
/// Any smoothing values <= 1.0 will disable the filter.
#[derive(Deserialize, Serialize, Debug)]
#[allow(dead_code)]
pub struct LowPass {
    /// Smoothing must be > 1.0 if you pretend to keep it active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smoothing: Option<f64>,
}

/// Represents the player filters.
#[derive(Deserialize, Serialize, Debug)]
#[allow(dead_code)]
pub struct PlayerFilters {
    /// Adjusts the player volume from 0.0 to 5.0, where 1.0 is 100%.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<FilterVolume>,
    /// Equalizer with 15 different bands.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub equalizer: Option<Vec<Equalizer>>,
    /// Eliminates part of a band, usually targeting vocals.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub karaoke: Option<Kareoke>,
    /// Composes the speed, pitch and rate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timescale: Option<Timescale>,
    /// Creates a shuddering effect, where the volume quicky oscillates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tremolo: Option<Tremolo>,
    /// Similar to tremolo, but this one oscillates the pitch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vibrato: Option<Vibrato>,
    /// Rotates the audio around the stereo channels/user headphones (aka Audio
    /// Panning)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<Rotation>,
    /// Distorion effect.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distortion: Option<Distortion>,
    /// Mixes both channels (left and right).
    #[serde(rename = "channelMix")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_mix: Option<ChannelMix>,
    /// Filters higher frequencies.
    #[serde(rename = "lowPass")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub low_pass: Option<LowPass>,
    /// Plugin filters.
    #[serde(rename = "pluginFilters")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_filters: Option<HashMap<PluginName, BoxedRawData>>,
}

/// Contains the decoded player metadata.
#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct PlayerData {
    /// The player guild identifier.
    #[serde(rename = "guildId")]
    pub guild_id: GuildId,
    /// The track that is currently playing, if any.
    pub track: Option<TrackInfo>,
    /// Current player volume (0 to 1000).
    pub volume: Volume,
    /// Whether the player is paused.
    pub paused: bool,
    /// The player state.
    pub state: PlayerState,
    /// The voice state of the player.
    pub voice: PlayerVoiceState,
    /// The filters used by the player.
    pub filters: PlayerFilters,
}

/// Memory stats in bytes of the node.
#[allow(missing_docs)]
#[derive(Deserialize, Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct Memory {
    pub free: u64,
    pub used: u64,
    pub allocated: u64,
    pub reservable: u64,
}

/// Cpu stats of the node.
#[derive(Deserialize, Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct Cpu {
    /// Amount of cores that the node has.
    pub cores: u64,
    /// Total load of the node.
    #[serde(rename = "systemLoad")]
    pub system_load: f64,
    /// Load of the running service (i.e. Lavalink instance).
    #[serde(rename = "lavalinkLoad")]
    pub lavalink_load: f64,
}

/// Frame stats of the node.
#[derive(Deserialize, Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct FrameStats {
    /// The amount of frames sent to Discord.
    pub sent: u64,
    /// The amount of frames that were nulled.
    pub nulled: u64,
    /// The difference between sent frames and the expected amount of frames.
    pub deficit: u64,
}

/// Collection of statistics reported by a node.
#[derive(Deserialize, Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct NodeStats {
    /// The amount of players connected to the server.
    pub players: u64,
    /// The amount of players playing a track.
    #[serde(rename = "playingPlayers")]
    pub playing_players: u64,
    /// The uptime of the node in milliseconds.
    pub uptime: Milli,
    /// The memory stats of the node.
    pub memory: Memory,
    /// The cpu stats of the node.
    pub cpu: Cpu,
    /// The frame stats of the node. None if node has no players or when
    /// retrieved via /v4/stats. TODO: change endpoint to the calling method.
    #[serde(rename = "frameStats")]
    pub frame_stats: Option<FrameStats>,
}

/// Why the track ended.
#[derive(Deserialize, Debug, Clone, Copy)]
#[serde(tag = "reason")]
#[allow(missing_docs)]
pub enum TrackEndReason {
    #[serde(rename = "finished")]
    Finished,
    #[serde(rename = "loadFailed")]
    LoadFailed,
    #[serde(rename = "stopped")]
    Stopped,
    #[serde(rename = "replaced")]
    Replaced,
    #[serde(rename = "cleanup")]
    Cleanup,
}

/// Severity of the track exception.
#[derive(Deserialize, Debug, Clone, Copy)]
#[serde(tag = "severity")]
pub enum TrackExceptionSeverity {
    /// The cause is known and expected, indicates that there is nothing wrong
    /// with the library itself.
    #[serde(rename = "common")]
    Common,
    /// The cause might not be exactly known, but is possibly caused by outside
    /// factors. For example when an outside service responds in a format that
    /// we do not expect
    #[serde(rename = "suspicious")]
    Suspicious,
    /// The probable cause is an issue with the library or there is no way to
    /// tell what the cause might be. This is the default level and other levels
    /// are used in cases where the thrower has more in-depth knowledge about
    /// the error
    #[serde(rename = "fault")]
    Fault,
}

/// Track exception thrown while trying to play some track.
#[derive(Deserialize, Debug)]
#[allow(missing_docs)]
pub struct TrackException {
    pub message: Option<String>,
    #[serde(flatten)]
    pub severity: TrackExceptionSeverity,
    pub cause: String,
}

/// Describes the reason by a node audio websocket to Discord was closed.
#[derive(Deserialize, Debug)]
#[allow(missing_docs)]
pub struct DiscordAudioWsClosed {
    pub code: CloseEventCode,
    pub reason: String,
    /// Whether the connection was closed by Discord.
    #[serde(rename = "byRemote")]
    pub remote: bool,
}

/// To update the node session state.
#[derive(Serialize, Debug)]
pub struct SessionState {
    /// Whether resuming is enabled for this session or not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resuming: Option<bool>,
    /// The timeout in seconds (default is 60 seconds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<Secs>,
}

impl SessionState {
    /// Sets resuming only, leaving the other in its current state.
    pub fn resuming_only(resuming: bool) -> Self {
        Self {
            resuming: Some(resuming),
            timeout: None
        }
    }

    /// Sets timeout only, leaving the other in its current state.
    pub fn timeout_only(timeout: Secs) -> Self {
        Self {
            resuming: None,
            timeout: Some(timeout)
        }
    }
}

/// To report the node session state.
#[derive(Deserialize, Debug)]
pub struct CurrentSessionState {
    /// Whether resuming is enabled for this session or not.
    pub resuming: bool,
    /// The timeout in seconds.
    pub timeout: Secs,
}

// TODO: route planners data...

// ############### Deserialization Utils ###############

/// Deserialize an instance of `T` from a raw value.
///
/// See [`serde_json::from_str`] to know more.
pub fn parse_arbitrary<'de, T>(
    data: &'de BoxedRawData
) -> Result<T, serde_json::Error>
where
    T: serde::Deserialize<'de>
{
    serde_json::from_str(data.get())
}

fn deserialize_selected_track<'de, D>(
    deserializer: D
) -> Result<Option<u64>, D::Error>
where
    D: de::Deserializer<'de>,
{
    struct SelectedTrackVisitor;

    impl<'de> de::Visitor<'de> for SelectedTrackVisitor {
        type Value = Option<u64>;

        fn expecting(
            &self,
            formatter: &mut std::fmt::Formatter
        ) -> std::fmt::Result {
            formatter.write_str(
                "an integer containing the selected track index"
            )
        }

        fn visit_i128<E>(self, v: i128) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(if v > -1 { Some(v as u64) } else { None })
        }
    }

    deserializer.deserialize_i128(SelectedTrackVisitor)
}

fn deserialize_ping<'de, D>(
    deserializer: D
) -> Result<Option<Milli>, D::Error>
where
    D: de::Deserializer<'de>,
{
    struct PingVisitor;

    impl<'de> de::Visitor<'de> for PingVisitor {
        type Value = Option<Milli>;

        fn expecting(
            &self,
            formatter: &mut std::fmt::Formatter
        ) -> std::fmt::Result {
            formatter.write_str(
                "an integer containing the player ping"
            )
        }

        fn visit_i128<E>(self, v: i128) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(if v > -1 { Some(v as Milli) } else { None })
        }
    }

    deserializer.deserialize_i128(PingVisitor)
}

fn deserialize_empty_match<'de, D>(
    deserializer: D
) -> Result<(), D::Error>
where
    D: de::Deserializer<'de>,
{
    struct EmptyMatchVisitor;

    impl<'de> de::Visitor<'de> for EmptyMatchVisitor {
        type Value = ();

        fn expecting(
            &self,
            formatter: &mut std::fmt::Formatter
        ) -> std::fmt::Result {
            formatter.write_str("a map (content doesn't matter)")
        }

        fn visit_map<A>(self, _: A) -> Result<Self::Value, A::Error>
        where
            A: de::MapAccess<'de>,
        {
            Ok(())
        }
    }

    deserializer.deserialize_map(EmptyMatchVisitor)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_playlist_info_without_selected_track() {
        let raw = r#"{
            "name": "Something",
            "selectedTrack": -1
        }"#;

        let res = serde_json::from_str::<PlaylistInfo>(raw);
        assert!(res.is_ok(), "got error: {:?}", res);

        let info = res.unwrap();

        assert_eq!(info.name, "Something");
        assert_eq!(info.selected_track, None);
    }

    #[test]
    fn test_playlist_info_with_selected_track() {
        let raw = r#"{
            "name": "Something",
            "selectedTrack": 453
        }"#;

        let res = serde_json::from_str::<PlaylistInfo>(raw);
        assert!(res.is_ok(), "got error: {:?}", res);

        let info = res.unwrap();

        assert_eq!(info.name, "Something");
        assert_eq!(info.selected_track, Some(453));
    }

    #[test]
    fn test_empty_load_result() {
        let raw = r#"{
            "loadType": "empty",
            "data": {}
        }"#;

        let res = serde_json::from_str::<LoadResult>(raw);
        assert!(res.is_ok(), "got error: {:?}", res);

        assert!(
            matches!(res.unwrap(), EmptyMatch(())),
            "expecting LoadResult::EmptyMatch(())"
        );
    }
}
