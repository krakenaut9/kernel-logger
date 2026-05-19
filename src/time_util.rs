use time::{Duration, OffsetDateTime};
use windows_sys::Wdk::System::SystemServices::KeQuerySystemTimePrecise;

const HUNDRED_NS_PER_SECOND: i64 = 10_000_000;

/// Difference between Windows epoch (1601-01-01)
/// and Unix epoch (1970-01-01), in seconds.
const WINDOWS_TO_UNIX_EPOCH_SECONDS: i64 = 11_644_473_600;

#[cfg(test)]
const WINDOWS_TO_UNIX_EPOCH_100NS: i128 = 11_644_473_600 * HUNDRED_NS_PER_SECOND as i128;

pub(crate) fn windows_time_to_offset_datetime(
    windows_100ns: i64,
) -> Result<OffsetDateTime, time::error::ComponentRange> {
    let secs = (windows_100ns / HUNDRED_NS_PER_SECOND) - WINDOWS_TO_UNIX_EPOCH_SECONDS;

    let nanos = ((windows_100ns % HUNDRED_NS_PER_SECOND) * 100) as i64;

    Ok(OffsetDateTime::from_unix_timestamp(secs)? + Duration::nanoseconds(nanos))
}

#[cfg(test)]
pub(crate) fn offset_datetime_to_windows_time(offset_datetime: OffsetDateTime) -> Option<i64> {
    let unix_100ns = offset_datetime.unix_timestamp_nanos().div_euclid(100);
    let windows_100ns = unix_100ns + WINDOWS_TO_UNIX_EPOCH_100NS;
    i64::try_from(windows_100ns).ok()
}

#[inline]
#[must_use]
pub(crate) fn current_time() -> i64 {
    let mut timestamp = 0;
    unsafe { KeQuerySystemTimePrecise(&mut timestamp) };
    timestamp
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;
    use time::{Duration, OffsetDateTime};

    #[test]
    fn windows_epoch_is_1601_01_01_utc() {
        // Windows epoch: 1601-01-01 00:00:00Z corresponds to FILETIME tick 0.
        let dt = super::windows_time_to_offset_datetime(0).unwrap();
        assert_eq!(dt, datetime!(1601-01-01 0:00 UTC));
    }

    #[test]
    fn unix_epoch_maps_to_known_windows_constant() {
        // The delta between Windows epoch (1601) and Unix epoch (1970) is
        // 116444736000000000 100ns ticks.
        let windows_at_unix_epoch = WINDOWS_TO_UNIX_EPOCH_100NS as i64;

        // Converting that Windows tick count must yield Unix epoch.
        let dt = windows_time_to_offset_datetime(windows_at_unix_epoch).unwrap();
        assert_eq!(dt, OffsetDateTime::UNIX_EPOCH);

        // And converting Unix epoch back must return that tick count.
        let back = offset_datetime_to_windows_time(OffsetDateTime::UNIX_EPOCH).unwrap();
        assert_eq!(back, windows_at_unix_epoch);
    }

    #[test]
    fn exact_fractional_tick_roundtrip_when_multiple_of_100ns() {
        // Pick a nanoseconds value that's exactly divisible by 100 (100ns precision).
        let nanos = 123_456_700i64; // divisible by 100
        let dt = OffsetDateTime::UNIX_EPOCH + Duration::nanoseconds(nanos);

        // Convert -> Windows ticks -> Convert back.
        let w = offset_datetime_to_windows_time(dt).unwrap();
        let dt2 = windows_time_to_offset_datetime(w).unwrap();

        // Should be exact: no precision lost because input aligns to 100ns.
        assert_eq!(dt2, dt);

        // Also check tick arithmetic: 123_456_700ns == 1_234_567 * 100ns ticks.
        let expected = (WINDOWS_TO_UNIX_EPOCH_100NS as i64) + (nanos as i64 / 100);
        assert_eq!(w, expected);
    }

    #[test]
    fn sub_100ns_precision_is_truncated_to_100ns() {
        // Not divisible by 100ns: Windows time cannot represent 1ns granularity.
        // Your forward conversion uses div_euclid(100), i.e. floors to 100ns ticks.
        let nanos = 123_456_789i64; // not divisible by 100
        let dt = OffsetDateTime::UNIX_EPOCH + Duration::nanoseconds(nanos);

        let w = offset_datetime_to_windows_time(dt).unwrap();
        let dt2 = windows_time_to_offset_datetime(w).unwrap();

        // Expected loss: floor to nearest 100ns boundary (down).
        let expected_nanos = nanos - (nanos.rem_euclid(100)); // 123_456_700
        let expected_dt = OffsetDateTime::UNIX_EPOCH + Duration::nanoseconds(expected_nanos);

        assert_eq!(dt2, expected_dt);
        assert!(dt2 <= dt, "conversion should not round up");
    }

    #[test]
    fn roundtrip_various_datetimes() {
        let cases = [
            datetime!(1601-01-01 0:00 UTC),
            datetime!(1700-01-01 12:34:56 UTC),
            datetime!(1900-02-28 23:59:59.999999900 UTC), // aligned to 100ns
            datetime!(1969-12-31 23:59:59.999999900 UTC), // just before Unix epoch, aligned
            datetime!(1970-01-01 0:00 UTC),
            datetime!(2000-01-01 0:00 UTC),
            datetime!(2024-05-18 10:20:30.123456700 UTC), // aligned to 100ns
        ];

        for &dt in &cases {
            let w = offset_datetime_to_windows_time(dt).unwrap();
            let dt2 = windows_time_to_offset_datetime(w).unwrap();
            assert_eq!(dt2, dt, "failed roundtrip for {dt:?}");
        }
    }
}
