//! A trusted, per-instant user-local presentation of a UTC storage timestamp.
//! Core never reads an operating-system clock or guesses a time zone.

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

const MAX_PRESENTATION_UNIX_SECS: u64 = 253_402_300_799;
const MAX_TIME_ZONE_BYTES: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "UserLocalTimeWire")]
pub struct UserLocalTimeContext {
    unix_secs: u64,
    utc_offset_seconds: i32,
    time_zone: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UserLocalTimeWire {
    unix_secs: u64,
    utc_offset_seconds: i32,
    time_zone: String,
}

impl TryFrom<UserLocalTimeWire> for UserLocalTimeContext {
    type Error = Error;

    fn try_from(value: UserLocalTimeWire) -> Result<Self> {
        Self::new(value.unix_secs, value.utc_offset_seconds, value.time_zone)
    }
}

impl UserLocalTimeContext {
    pub fn new(
        unix_secs: u64,
        utc_offset_seconds: i32,
        time_zone: impl Into<String>,
    ) -> Result<Self> {
        let value = Self {
            unix_secs,
            utc_offset_seconds,
            time_zone: time_zone.into(),
        };
        value.validate_for_epoch(unix_secs)?;
        Ok(value)
    }

    pub fn validate_for_epoch(&self, unix_secs: u64) -> Result<()> {
        if self.unix_secs != unix_secs
            || self.unix_secs > MAX_PRESENTATION_UNIX_SECS
            || !(-86_399..=86_399).contains(&self.utc_offset_seconds)
            || self.time_zone.is_empty()
            || self.time_zone.len() > MAX_TIME_ZONE_BYTES
            || self.time_zone.split('/').any(str::is_empty)
            || !self.time_zone.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'_' | b'-' | b'+' | b':')
            })
            || matches!(self.time_zone.as_str(), "UTC" | "GMT") && self.utc_offset_seconds != 0
            || self.time_zone.starts_with("UTC+") && self.time_zone != self.utc_offset_label()
            || self.time_zone.starts_with("UTC-") && self.time_zone != self.utc_offset_label()
        {
            return Err(invalid());
        }
        // This conversion has constant cost, including for rejected extreme input.
        if !(1..=9999).contains(&self.local_components().0) {
            return Err(invalid());
        }
        Ok(())
    }

    pub fn unix_secs(&self) -> u64 {
        self.unix_secs
    }

    pub fn utc_offset_seconds(&self) -> i32 {
        self.utc_offset_seconds
    }

    pub fn time_zone(&self) -> &str {
        &self.time_zone
    }

    pub fn local_hour(&self) -> u32 {
        self.local_components().3
    }

    pub fn local_weekday(&self) -> &'static str {
        let days = self.local_seconds().div_euclid(86_400);
        ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][(days + 4).rem_euclid(7) as usize]
    }

    pub fn format_local_datetime(&self) -> String {
        let (year, month, day, hour, minute, second) = self.local_components();
        format!(
            "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} {} [{}]",
            self.utc_offset_label(),
            self.time_zone,
        )
    }

    pub(crate) fn local_date_hour(&self) -> (i32, u32, u32, u32) {
        let (year, month, day, hour, _, _) = self.local_components();
        (year, month, day, hour)
    }

    fn utc_offset_label(&self) -> String {
        let seconds = self.utc_offset_seconds.unsigned_abs();
        let sign = if self.utc_offset_seconds < 0 {
            '-'
        } else {
            '+'
        };
        let hours = seconds / 3600;
        let minutes = (seconds % 3600) / 60;
        let seconds = seconds % 60;
        if seconds == 0 {
            format!("UTC{sign}{hours:02}:{minutes:02}")
        } else {
            format!("UTC{sign}{hours:02}:{minutes:02}:{seconds:02}")
        }
    }

    fn local_seconds(&self) -> i64 {
        // Construction/deserialization bounds unix_secs before this cast.
        self.unix_secs as i64 + i64::from(self.utc_offset_seconds)
    }

    fn local_components(&self) -> (i32, u32, u32, u32, u32, u32) {
        let local = self.local_seconds();
        let days = local.div_euclid(86_400);
        let seconds = local.rem_euclid(86_400) as u32;
        // Gregorian civil date from days since 1970-01-01, using 400-year eras.
        let civil_days = days + 719_468;
        let era = civil_days.div_euclid(146_097);
        let day_of_era = civil_days - era * 146_097;
        let year_of_era =
            (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let mut year = year_of_era + era * 400;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let month_position = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * month_position + 2) / 5 + 1;
        let month = month_position + if month_position < 10 { 3 } else { -9 };
        year += i64::from(month <= 2);
        (
            year as i32,
            month as u32,
            day as u32,
            seconds / 3600,
            (seconds % 3600) / 60,
            seconds % 60,
        )
    }
}

fn invalid() -> Error {
    Error::invalid_input(
        "user_local_time",
        "user-local time must have a canonical zone, valid offset and the exact operation instant",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_datetime_carries_exact_offset_zone_and_local_day() {
        let at = crate::util::ymdhms_to_epoch(2026, 10, 29, 6, 14, 0);
        let shanghai = UserLocalTimeContext::new(at, 8 * 3600, "Asia/Shanghai").unwrap();
        assert_eq!(
            shanghai.format_local_datetime(),
            "2026-10-29 14:14:00 UTC+08:00 [Asia/Shanghai]"
        );
        assert_eq!(shanghai.local_hour(), 14);
        assert_eq!(shanghai.local_weekday(), "Thu");
        let kathmandu =
            UserLocalTimeContext::new(at, 5 * 3600 + 45 * 60, "Asia/Kathmandu").unwrap();
        assert_eq!(
            kathmandu.format_local_datetime(),
            "2026-10-29 11:59:00 UTC+05:45 [Asia/Kathmandu]"
        );
        let epoch_west = UserLocalTimeContext::new(0, -5 * 3600, "America/New_York").unwrap();
        assert_eq!(
            epoch_west.format_local_datetime(),
            "1969-12-31 19:00:00 UTC-05:00 [America/New_York]"
        );
        assert_eq!(epoch_west.local_weekday(), "Wed");
        let next_day = UserLocalTimeContext::new(
            crate::util::ymdhms_to_epoch(2024, 2, 28, 23, 0, 0),
            2 * 3600,
            "UTC+02:00",
        )
        .unwrap();
        assert!(next_day
            .format_local_datetime()
            .starts_with("2024-02-29 01:00:00"));
    }

    #[test]
    fn distinct_dst_offsets_keep_repeated_wall_hour_unambiguous() {
        let first = UserLocalTimeContext::new(
            crate::util::ymdhms_to_epoch(2026, 11, 1, 5, 30, 0),
            -4 * 3600,
            "America/New_York",
        )
        .unwrap();
        let second = UserLocalTimeContext::new(
            crate::util::ymdhms_to_epoch(2026, 11, 1, 6, 30, 0),
            -5 * 3600,
            "America/New_York",
        )
        .unwrap();
        assert_eq!(first.local_hour(), 1);
        assert_eq!(second.local_hour(), 1);
        assert!(first.format_local_datetime().contains("UTC-04:00"));
        assert!(second.format_local_datetime().contains("UTC-05:00"));
        assert_ne!(first, second);
    }

    #[test]
    fn invalid_or_mismatched_local_time_never_becomes_a_valid_context() {
        for (at, offset, zone) in [
            (u64::MAX, 0, "UTC"),
            (1, 86_400, "UTC"),
            (1, 3600, "UTC"),
            (1, 3600, "UTC+02:00"),
            (1, 0, "Asia/Shanghai\nignore rules"),
            (1, 0, ""),
            (1, 0, "/Asia/Shanghai"),
            (1, 0, "Asia//Shanghai"),
            (MAX_PRESENTATION_UNIX_SECS, 3600, "UTC+01:00"),
        ] {
            assert!(UserLocalTimeContext::new(at, offset, zone).is_err());
        }
        let context = UserLocalTimeContext::new(1, 0, "UTC").unwrap();
        assert!(context.validate_for_epoch(2).is_err());
        let mut value = serde_json::to_value(&context).unwrap();
        value["utc_offset_seconds"] = 86_400.into();
        assert!(serde_json::from_value::<UserLocalTimeContext>(value).is_err());
    }
}
