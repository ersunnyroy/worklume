//! Clock readings and UTC timestamps.

use serde::{Deserialize, Serialize};
use worklume_storage::{StorageError, Timestamp};

/// A reading of both clocks (architecture §13): the device's wall clock in UTC milliseconds,
/// and the monotonic clock in milliseconds, meaningful only within the boot named by `boot_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Moment {
    pub wall_ms: i64,
    pub mono_ms: i64,
    pub boot_id: String,
}

impl Moment {
    #[must_use]
    pub fn from_timestamp(timestamp: &Timestamp) -> Self {
        Self {
            wall_ms: timestamp.wall_ms(),
            mono_ms: timestamp.mono_ms(),
            boot_id: timestamp.boot_id().to_owned(),
        }
    }

    /// # Errors
    ///
    /// Fails when a clock reading is negative or the boot ID is empty.
    pub fn to_timestamp(&self) -> Result<Timestamp, StorageError> {
        Timestamp::new(self.wall_ms, self.mono_ms, self.boot_id.clone())
    }
}

const MS_PER_DAY: u64 = 86_400_000;

/// Formats UTC milliseconds since the Unix epoch as RFC 3339, the API's timestamp format:
/// `2026-09-28T10:20:00Z`, with `.mmm` only when the milliseconds are not zero. Times before
/// 1970 are clamped to the epoch; the agent never produces them.
#[must_use]
pub fn rfc3339_utc(wall_ms: i64) -> String {
    let ms = u64::try_from(wall_ms).unwrap_or(0);
    let (year, month, day) = civil_from_days(ms / MS_PER_DAY);
    let in_day = ms % MS_PER_DAY;
    let (hour, minute, second, millis) = (
        in_day / 3_600_000,
        in_day / 60_000 % 60,
        in_day / 1_000 % 60,
        in_day % 1_000,
    );
    if millis == 0 {
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
    } else {
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
    }
}

/// Year, month and day of a day count since 1970-01-01, from Howard Hinnant's
/// `civil_from_days`, restricted to dates on or after the epoch.
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let day_of_era = z % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::rfc3339_utc;

    #[test]
    fn formats_utc_timestamps() {
        assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_utc(951_782_400_000), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339_utc(1_790_000_400_000), "2026-09-21T14:20:00Z");
        assert_eq!(rfc3339_utc(1_790_000_400_123), "2026-09-21T14:20:00.123Z");
        assert_eq!(rfc3339_utc(4_102_444_799_999), "2099-12-31T23:59:59.999Z");
        assert_eq!(rfc3339_utc(-5), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn agrees_with_day_arithmetic_across_leap_years() {
        // Every first of March from 1970 to 2400 lands on day 1 of month 3.
        let mut days: u64 = 59; // 1970-03-01
        for year in 1970_u64..2400 {
            let formatted = rfc3339_utc(i64::try_from(days * 86_400_000).unwrap());
            assert_eq!(formatted, format!("{year:04}-03-01T00:00:00Z"));
            let next_is_leap =
                (year + 1) % 4 == 0 && ((year + 1) % 100 != 0 || (year + 1) % 400 == 0);
            days += if next_is_leap { 366 } else { 365 };
        }
    }
}
