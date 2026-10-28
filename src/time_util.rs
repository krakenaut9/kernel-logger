use time::{Duration, OffsetDateTime};
use windows_sys::Wdk::System::SystemServices::KeQuerySystemTimePrecise;

const HUNDRED_NS_PER_SECOND: i64 = 10_000_000;

/// Difference between Windows epoch (1601-01-01)
/// and Unix epoch (1970-01-01), in seconds.
const WINDOWS_TO_UNIX_EPOCH_SECONDS: i64 = 11_644_473_600;

#[cfg(test)]
const WINDOWS_TO_UNIX_EPOCH_100NS: i64 = WINDOWS_TO_UNIX_EPOCH_SECONDS * HUNDRED_NS_PER_SECOND;

fn windows_time_to_offset_datetime(
    windows_100ns: i64,
) -> Result<OffsetDateTime, time::error::ComponentRange> {
    let secs = (windows_100ns / HUNDRED_NS_PER_SECOND) - WINDOWS_TO_UNIX_EPOCH_SECONDS;

    let nanos = (windows_100ns % HUNDRED_NS_PER_SECOND) * 100;

    Ok(OffsetDateTime::from_unix_timestamp(secs)? + Duration::nanoseconds(nanos))
}

#[cfg(test)]
pub(crate) fn offset_datetime_to_windows_time(offset_datetime: OffsetDateTime) -> Option<i64> {
    let unix_100ns = offset_datetime.unix_timestamp_nanos().div_euclid(100);
    let windows_100ns = unix_100ns + i128::from(WINDOWS_TO_UNIX_EPOCH_100NS);
    i64::try_from(windows_100ns).ok()
}

#[inline]
#[must_use]
pub(crate) fn current_time() -> i64 {
    let mut timestamp = 0;
    // SAFETY: KeQuerySystemTimePrecise writes the time to `timestamp`, a valid i64, and may
    // be called at any IRQL.
    unsafe { KeQuerySystemTimePrecise(&raw mut timestamp) };
    timestamp
}

/// The length of a timestamp: `2026-05-19 10:29:12.345678Z`.
pub(crate) const TIMESTAMP_LEN: usize = 27;
/// A timestamp before its digits are written over the zeros.
const TIMESTAMP_TEMPLATE: [u8; TIMESTAMP_LEN] = *b"0000-00-00 00:00:00.000000Z";
/// Stands in for a time outside the years 0 to 9999, in the same width, so that the columns
/// after it stay aligned.
const UNKNOWN_TIMESTAMP: [u8; TIMESTAMP_LEN] = *b"????-??-?? ??:??:??.??????Z";

/// A system time in UTC to the microsecond, cut rather than rounded, so that it is never
/// later than the time itself: `2026-05-19 10:29:12.345678Z`.
pub(crate) struct Timestamp([u8; TIMESTAMP_LEN]);

impl Timestamp {
    /// The timestamp of `windows_100ns`, a system time, or [`UNKNOWN_TIMESTAMP`] if that is
    /// outside the years 0 to 9999.
    pub(crate) fn new(windows_100ns: i64) -> Self {
        let Ok(time) = windows_time_to_offset_datetime(windows_100ns) else {
            return Self(UNKNOWN_TIMESTAMP);
        };

        let Ok(year) = u32::try_from(time.year()) else {
            return Self(UNKNOWN_TIMESTAMP);
        };

        // The digits are written by hand rather than with `write!`'s padded `{:02}`s, which
        // cost more than the rest of the line: this formats a whole line about twice as fast.
        // Each field's digits go over its zeros in the template, which starts at:
        // YYYY-MM-DD HH:MM:SS.ffffffZ
        // 0    5  8  11 14 17 20
        let mut stamp = TIMESTAMP_TEMPLATE;

        for (field, value) in [
            (0..4, year),
            (5..7, u32::from(u8::from(time.month()))),
            (8..10, u32::from(time.day())),
            (11..13, u32::from(time.hour())),
            (14..16, u32::from(time.minute())),
            (17..19, u32::from(time.second())),
            (20..26, time.microsecond()),
        ] {
            if let Some(digits) = stamp.get_mut(field) {
                write_digits(digits, value);
            }
        }

        Self(stamp)
    }

    /// The timestamp as text: it is all ASCII.
    pub(crate) fn as_str(&self) -> &str {
        core::str::from_utf8(&self.0).unwrap_or_default()
    }
}

/// Writes the last `digits.len()` decimal digits of `value` into `digits`, padded with
/// zeros.
fn write_digits(digits: &mut [u8], mut value: u32) {
    for digit in digits.iter_mut().rev() {
        // `value % 10` is below 10, so it always fits.
        *digit = b'0' + u8::try_from(value % 10).unwrap_or_default();

        value /= 10;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::fmt::Write;
    use heapless::String;
    use time::macros::datetime;
    use time::{Date, Duration, Month, OffsetDateTime, PrimitiveDateTime};

    /// The timestamp of `time`.
    fn timestamp_of(time: OffsetDateTime) -> Timestamp {
        Timestamp::new(offset_datetime_to_windows_time(time).unwrap())
    }

    #[test]
    fn a_timestamp_is_utc_to_the_microsecond_cut_not_rounded() {
        let stamp = timestamp_of(datetime!(2026-05-19 10:29:12.3456789 UTC));

        assert_eq!(stamp.as_str(), "2026-05-19 10:29:12.345678Z");
    }

    #[test]
    fn every_field_of_a_timestamp_is_padded_with_zeros() {
        let stamp = timestamp_of(datetime!(2026-01-02 03:04:05.000006 UTC));

        assert_eq!(stamp.as_str(), "2026-01-02 03:04:05.000006Z");
    }

    #[test]
    fn the_years_0_to_9999_have_timestamps_and_a_time_outside_them_the_placeholder() {
        let first = Date::from_calendar_date(0, Month::January, 1)
            .unwrap()
            .midnight()
            .assume_utc();
        let first_ticks = offset_datetime_to_windows_time(first).unwrap();
        let last_ticks =
            offset_datetime_to_windows_time(PrimitiveDateTime::MAX.assume_utc()).unwrap();

        assert_eq!(
            Timestamp::new(first_ticks).as_str(),
            "0000-01-01 00:00:00.000000Z"
        );
        assert_eq!(
            Timestamp::new(last_ticks).as_str(),
            "9999-12-31 23:59:59.999999Z"
        );

        // 100 ns before the first and after the last, and the latest system time of all.
        for ticks in [first_ticks - 1, last_ticks + 1, i64::MAX] {
            assert_eq!(
                Timestamp::new(ticks).as_str(),
                "????-??-?? ??:??:??.??????Z"
            );
        }
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "takes minutes under Miri, and has no unsafe code to check"
    )]
    fn a_timestamp_is_what_write_makes_of_the_time() {
        // Times from 1601-01-01, tick 0, to the end of 9999, picked by a xorshift generator.
        let last_ticks =
            offset_datetime_to_windows_time(PrimitiveDateTime::MAX.assume_utc()).unwrap();
        let mut state = 0x2545_f491_4f6c_dd1d_u64;

        for _ in 0..2_000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;

            let ticks = i64::try_from(state % last_ticks.unsigned_abs()).unwrap();
            let time = windows_time_to_offset_datetime(ticks).unwrap();

            let mut expected = String::<32>::new();
            write!(
                expected,
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:06}Z",
                time.year(),
                u8::from(time.month()),
                time.day(),
                time.hour(),
                time.minute(),
                time.second(),
                time.microsecond()
            )
            .unwrap();

            assert_eq!(Timestamp::new(ticks).as_str(), expected.as_str(), "{ticks}");
        }
    }

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
        let windows_at_unix_epoch = WINDOWS_TO_UNIX_EPOCH_100NS;

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
        let expected = WINDOWS_TO_UNIX_EPOCH_100NS + nanos / 100;
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
