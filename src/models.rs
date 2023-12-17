//! TODO:

use std::collections::HashMap;

use serde::{Deserialize, de};
use serde_json::value::RawValue;

// ############### Types ###############

/// Milliseconds representation.
pub type Milli = usize;
/// HTTP status code.
pub type StatusCode = u16;
/// Player volume.
pub type Volume = u16;
/// Player filter volume adjustment.
pub type FilterVolume = f32;
/// Unparsed json (i.e. stills in raw format).
pub type RawData<'a> = &'a RawValue;
/// Map of configurations for each plugin filter(s).
// pub type PluginFilters<'a> = HashMap<PluginName, ArbitraryData<'a>>;

// ############### Models ###############

/// Contains the decoded error metadata.
///
/// This error is returned by some Lavalink instance upon an unexpected
/// behaviour while loading tracks or controling the players trough its
/// REST API.
#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct ErrorData {
    /// The error time in milliseconds since Unix epoch.
    pub timestamp: Milli,
    /// HTTP status code.
    pub status: StatusCode,
    /// HTTP Status code error message.
    pub error: String,
    /// Error message (i.e. explanation).
    pub message: String,
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
pub struct TrackData<'a> {
    /// The track unique identifier.
    pub encoded: String,
    /// The track description.
    pub info: TrackInfo,
    /// Aditional info that may be injected by some plugin.
    #[serde(rename = "pluginInfo")]
    #[serde(borrow)]
    pub plugin_info: RawData<'a>,
    /// Additional user data that might have been sent in the update player
    /// endpoint.
    #[serde(rename = "userData")]
    #[serde(borrow)]
    pub user_data: RawData<'a>,
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
    pub selected_track: Option<usize>,
}

/// Contains the decoded playlist metadata.
#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct PlaylistData<'a> {
    /// The playlist info.
    info: PlaylistInfo,
    /// Aditional info that may be injected by some plugin.
    #[serde(rename = "pluginInfo")]
    #[serde(borrow)]
    plugin_info: RawData<'a>,
    /// Collection of tracks.
    tracks: Vec<TrackData<'a>>,
}

/// Contains the response got from loading tracks.
#[derive(Deserialize, Debug)]
#[serde(tag = "loadType", content = "data")]
#[allow(dead_code)]
pub enum LoadResult<'a> {
    /// When searching by the track identifier or its url.
    #[serde(rename = "track")]
    #[serde(borrow)]
    SingleTrack(TrackData<'a>),
    /// When searching by the playlist identifier or its url.
    #[serde(rename = "playlist")]
    Playlist(PlaylistData<'a>),
    /// When a search engine is used (e.g., `ytsearch`, `soundcloud`).
    #[serde(rename = "search")]
    TracksSearch(Vec<TrackData<'a>>),
    /// When there's no match for the given identifier/url.
    #[serde(rename = "empty")]
    EmptyMatch(
        #[serde(deserialize_with = "deserialize_empty_match")]
        ()
    ),
    /// When something went wrong.
    #[serde(rename = "error")]
    Fail(ErrorData),
}

pub use self::LoadResult::*;

/// Represents the player state (e.g., if is connected, current track
/// position...).
#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct State {
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
#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct VoiceState {
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
#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct Equalizer {
    /// One of 0 to 14.
    pub band: u8,
    /// Between -0.25 to 1.0.
    pub gain: f32,
}

/// Represents a karaoke equilization.
///
/// Used to eliminate part of a band, usually targeting vocals.
#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct Kareoke {
    /// Between 0.0 (low effect) and 1.0 (full effect).
    pub level: Option<f32>,
    /// Between 0.0 (low effect) and 1.0 (full effect).
    #[serde(rename = "monoLevel")]
    pub mono_level: Option<f32>,
    /// Filter band in Hz.
    #[serde(rename = "filterBand")]
    pub filter_band: Option<f32>,
    /// Filter width.
    #[serde(rename = "filterWidth")]
    pub filter_width: Option<f32>,
}

/// Contains a map of plugins raw configuration.
///
/// Configuration isn't already parsed since it depends on the plugins that you
/// use.
#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct PluginFilters<'a> {
    #[serde(borrow)]
    filters: HashMap<&'a str, RawData<'a>>,
}

impl<'a> PluginFilters<'a> {
    /// Returns the raw configuration data for a given plugin name, if any.
    pub fn get(&self, name: &'a str) -> Option<&RawData<'a>> {
        self.filters.get(name)
    }
}

/// Represents the player filters.
#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct Filters<'a> {
    /// Adjusts the player volume from 0.0 to 5.0, where 1.0 is 100%.
    pub volume: Option<FilterVolume>,
    /// Equalizer with 15 different bands.
    pub equalizer: Option<Vec<Equalizer>>,
    /// Eliminates part of a band, usually targeting vocals.
    pub karaoke: Option<Kareoke>,

    // TODO: remaining filters.

    /// Plugin filters.
    #[serde(rename = "pluginFilters")]
    #[serde(borrow)]
    #[serde(deserialize_with = "deserialize_plugin_filters")]
    pub plugin_filters: Option<PluginFilters<'a>>,
}

/// Contains the decoded player metadata.
#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct PlayerData<'a> {
    /// The player guild identifier.
    #[serde(rename = "guildId")]
    pub guild_id: String,
    /// The track that is currently playing, if any.
    pub track: Option<TrackInfo>,
    /// Current player volume (0 to 1000).
    pub volume: Volume,
    /// Whether the player is paused.
    pub paused: bool,
    /// The player state.
    pub state: State,
    /// The voice state of the player.
    pub voice: VoiceState,
    /// The filters used by the player.
    #[serde(borrow)]
    pub filters: Filters<'a>,
}

// ############### Deserialization Utils ###############

/// Deserialize an instance of `T` from a raw value.
///
/// See [`serde_json::from_str`] to know more.
pub fn parse_arbitrary<'a, T>(
    data: RawData<'a>
) -> Result<T, serde_json::Error>
where
    T: serde::Deserialize<'a>
{
    serde_json::from_str(data.get())
}

fn deserialize_selected_track<'de, D>(
    deserializer: D
) -> Result<Option<usize>, D::Error>
where
    D: de::Deserializer<'de>,
{
    struct SelectedTrackVisitor;

    impl<'de> de::Visitor<'de> for SelectedTrackVisitor {
        type Value = Option<usize>;

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
            Ok(if v > -1 { Some(v as usize) } else { None })
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

fn deserialize_plugin_filters<'de, D>(
    deserializer: D
) -> Result<Option<PluginFilters<'de>>, D::Error>
where
    D: de::Deserializer<'de>,
{
    struct PluginFiltersVisitor;

    impl<'de> de::Visitor<'de> for PluginFiltersVisitor {
        type Value = Option<PluginFilters<'de>>;

        fn expecting(
            &self,
            formatter: &mut std::fmt::Formatter
        ) -> std::fmt::Result {
            formatter.write_str(
                "a map with configuration data for each plugin name"
            )
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: de::MapAccess<'de>,
        {
            let mut filters: HashMap<&'de str, RawData<'de>> = HashMap::new();

            while let Some((key, value)) = map.next_entry()? {
                filters.insert(key, value);
            }

            Ok(Some(PluginFilters { filters }))
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }
    }

    deserializer.deserialize_map(PluginFiltersVisitor)
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

    #[test]
    fn test_plugin_filters() {
        let raw = r#"{
            "pluginFilters": {
                "p1": true,
                "p2": { "something": 1 }
            }
        }"#;

        let res = serde_json::from_str::<Filters>(raw);
        assert!(res.is_ok(), "got error: {:?}", res);

        let plugin_filters = res.unwrap().plugin_filters;
        assert!(plugin_filters.is_some());

        let plugin_filters = plugin_filters.unwrap();

        let p1 = plugin_filters.get("p1");
        assert!(p1.is_some(), "expecting p1 entry in plugin filters map");
        assert_eq!(p1.unwrap().get(), r#"true"#);

        let p2 = plugin_filters.get("p2");
        assert!(p2.is_some(), "expecting p2 entry in plugin filters map");
        assert_eq!(p2.unwrap().get(), r#"{ "something": 1 }"#);
    }
}
