#![allow(dead_code)] // TODO: remove this.

use std::num::NonZeroUsize;

use chrono::{Utc, DurationRound, Duration};
use lru::LruCache;
use tokio::sync::RwLock;

use crate::models::NodeStats;

type Events = [u64; 4];
type Timestamp = i64;

pub(crate) enum EventMetric {
    Stuck, Exception, Failed, Attempt,
}

use self::EventMetric::*;

pub(crate) struct PlayerPenalty {
    cached_events: RwLock<LruCache<Timestamp, Events>>,
    minute: Duration,
}

impl PlayerPenalty {
    pub(crate) fn new(cached_events: NonZeroUsize) -> Self {
        Self {
            cached_events: RwLock::new(LruCache::new(cached_events)),
            minute: Duration::minutes(1),
        }
    }

    pub(crate) async fn register_event(&self, event: EventMetric) {
        // Truncates the date in the current minute.
        let truncated_timestamp = Utc::now()
            .duration_trunc(self.minute)
            .unwrap()
            .timestamp();

        let mut cached = self.cached_events.write().await;

        // If there's an entry in the truncated date, creates a new one.
        let events = cached.get_or_insert_mut(truncated_timestamp, || [0; 4]);
        events[event as usize] += 1;
    }

    async fn cumulative_events(&self) -> Events {
        let cached = self.cached_events.read().await;

        let mut sum: Events = [0; 4];

        cached.iter().for_each(|(_, partial_sum)| {
            sum[Stuck as usize] += partial_sum[Stuck as usize];
            sum[Exception as usize] += partial_sum[Exception as usize];
            sum[Failed as usize] += partial_sum[Failed as usize];
            sum[Attempt as usize] += partial_sum[Attempt as usize];
        });

        sum
    }

    pub(crate) async fn penalty(&self, stats: NodeStats) -> u64 {
        let events = self.cumulative_events().await;

        let attempts = events[Attempt as usize];
        let fails = events[Failed as usize];

        if attempts > 0 && attempts == fails { return u64::MAX; }

        let player_p = stats.playing_players;

        let system_load = stats.cpu.system_load;
        let cpu_p = (1.05_f64).powf(100.*system_load*10.-10.) as u64;

        let mut deficit_frame_p = 0;
        let mut null_frame_p = 0;
        if let Some(frame_stats) = stats.frame_stats {
            let deficit = frame_stats.deficit as f64;
            let nulled = frame_stats.nulled as f64;

            deficit_frame_p = (
                1.03_f64.powf(500.*(deficit/300.)*600.-600.)
            ) as u64;
            null_frame_p = (
                1.03_f64.powf(500.*(nulled/300.)*600.-600.)
            ) as u64;
        }

        let stucks = events[Stuck as usize];
        let exceptions = events[Exception as usize];

        let track_stuck_p = stucks*100-100;
        let track_exeptions_p  = exceptions*100-100;
        let load_failed_p = if fails > 0 { fails/attempts } else { 0 };

        player_p+cpu_p+deficit_frame_p+null_frame_p+
        track_stuck_p+track_exeptions_p+load_failed_p
    }
}

