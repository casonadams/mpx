use std::time::{SystemTime, UNIX_EPOCH};

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// A Unix time in local time, as the `#{t:}` and `#{t/p:}` operators and the UTC offset need it.
struct LocalTime {
    /// Day of the week, `Sun` is 0.
    weekday: i64,
    /// Month of the year, January is 1.
    month: i64,
    day: i64,
    hour: i64,
    min: i64,
    sec: i64,
    year: i64,
}

impl LocalTime {
    fn weekday_name(&self) -> &'static str {
        name_at(&WEEKDAYS, self.weekday)
    }

    fn month_name(&self) -> &'static str {
        name_at(&MONTHS, self.month - 1)
    }
}

/// The entry of `names` at `index`, empty when the index is out of range.
fn name_at(names: &'static [&'static str], index: i64) -> &'static str {
    usize::try_from(index)
        .ok()
        .and_then(|i| names.get(i))
        .copied()
        .unwrap_or("")
}

/// The local time of Unix time `secs`; `None` when it cannot be converted.
#[cfg(unix)]
fn local_tm(secs: i64) -> Option<LocalTime> {
    let secs = libc::time_t::try_from(secs).ok()?;
    // SAFETY: `tm` is a plain C struct of integers, valid when zeroed; `localtime_r` fills it.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers refer to live locals; `localtime_r` only writes into `tm`.
    let converted = !unsafe { libc::localtime_r(&secs, &mut tm) }.is_null();
    converted.then(|| LocalTime {
        weekday: i64::from(tm.tm_wday),
        month: i64::from(tm.tm_mon) + 1,
        day: i64::from(tm.tm_mday),
        hour: i64::from(tm.tm_hour),
        min: i64::from(tm.tm_min),
        sec: i64::from(tm.tm_sec),
        year: i64::from(tm.tm_year) + 1900,
    })
}

/// The local time of Unix time `secs`; `None` when it cannot be converted.
#[cfg(windows)]
fn local_tm(secs: i64) -> Option<LocalTime> {
    use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    // FILETIME counts 100 ns ticks from 1601-01-01 UTC, 11_644_473_600 s before the Unix epoch.
    let ticks = u64::try_from(secs.checked_add(11_644_473_600)?.checked_mul(10_000_000)?).ok()?;
    let utc_time = FILETIME {
        dwLowDateTime: u32::try_from(ticks & 0xFFFF_FFFF).ok()?,
        dwHighDateTime: u32::try_from(ticks >> 32).ok()?,
    };
    // SAFETY: a zeroed SYSTEMTIME is plain integers; the calls read their inputs and write only
    // the output pointers, which refer to live locals of the types they expect.
    let mut utc: SYSTEMTIME = unsafe { std::mem::zeroed() };
    let mut local: SYSTEMTIME = unsafe { std::mem::zeroed() };
    let to_utc = unsafe { FileTimeToSystemTime(&utc_time, &mut utc) } != 0;
    // SAFETY: a null zone selects the current time zone; both pointers refer to live locals.
    let to_local =
        unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) } != 0;
    (to_utc && to_local).then(|| LocalTime {
        weekday: i64::from(local.wDayOfWeek),
        month: i64::from(local.wMonth),
        day: i64::from(local.wDay),
        hour: i64::from(local.wHour),
        min: i64::from(local.wMinute),
        sec: i64::from(local.wSecond),
        year: i64::from(local.wYear),
    })
}

/// Other platforms have no local time conversion, so the time operators print nothing there.
#[cfg(not(any(unix, windows)))]
fn local_tm(_secs: i64) -> Option<LocalTime> {
    None
}

/// Seconds east of UTC of the local time zone at Unix time `secs`; `None` when it cannot be
/// determined.
pub fn utc_offset_secs(secs: i64) -> Option<i64> {
    let t = local_tm(secs)?;
    let local_secs =
        days_from_civil(t.year, t.month, t.day) * 86_400 + t.hour * 3_600 + t.min * 60 + t.sec;
    Some(local_secs - secs)
}

/// Days from 1970-01-01 to the civil date `year`-`month`-`day` in the proleptic Gregorian
/// calendar.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let year_of_era = y - era * 400;
    let shifted_month = (month + 9) % 12;
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// `#{t:var}`: the Unix time in `var` as local time, `Thu Nov  2 10:00:00 2026` (ctime layout).
/// Empty when `var` is not a number or the time cannot be converted.
pub(super) fn local_time(value: &str) -> String {
    let Some(t) = value.trim().parse::<i64>().ok().and_then(local_tm) else {
        return String::new();
    };
    format!(
        "{} {} {:>2} {:02}:{:02}:{:02} {}",
        t.weekday_name(),
        t.month_name(),
        t.day,
        t.hour,
        t.min,
        t.sec,
        t.year
    )
}

/// `#{t/p:var}`: how long ago the Unix time in `var` was. Within a day it is `HH:MM`, within a
/// week `Wed14:05`, and older `Wed02Nov`, in local time. Empty when `var` is not a number.
pub(super) fn pretty_time(value: &str) -> String {
    let Ok(secs) = value.trim().parse::<i64>() else {
        return String::new();
    };
    let Some(t) = local_tm(secs) else {
        return String::new();
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
    let age = now.saturating_sub(secs);
    if age < 86_400 {
        format!("{:02}:{:02}", t.hour, t.min)
    } else if age < 7 * 86_400 {
        format!("{}{:02}:{:02}", t.weekday_name(), t.hour, t.min)
    } else {
        format!("{}{:02}{}", t.weekday_name(), t.day, t.month_name())
    }
}
