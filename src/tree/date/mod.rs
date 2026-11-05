/*
 * tree/date.rs
 *
 * ftml - Library to parse Wikidot text
 * Copyright (C) 2019-2026 Wikijump Team
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program. If not, see <http://www.gnu.org/licenses/>.
 */

use icu_calendar::{Date as IcuDate, Iso};
use icu_datetime::input::{DateTime as IcuDateTime, Time as IcuTime};
use icu_datetime::pattern::DateTimePattern;
use icu_decimal::input::Decimal;
use icu_experimental::relativetime::RelativeTimeFormatterOptions;
use icu_locale::Locale;
use std::fmt;
use std::io;
use std::sync::LazyLock;
use time::format_description::parse_strftime_borrowed;
use time::{Date, OffsetDateTime, PrimitiveDateTime, UtcOffset};
use writeable::TryWriteable;

const MAX_DATE_FORMAT_BYTES: usize = 512;
const MAX_DATE_FORMAT_DIRECTIVES: usize = 64;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum DateFormatFlavor {
    Strftime,
    Wikidot,
}

mod cache;

/// The default Wikidot date format, used when no explicit format is given.
pub const WIKIDOT_DEFAULT_DATE_FORMAT: &str = "%d %b %Y %H:%M";

#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case", untagged)]
pub enum DateItem {
    Date(Date),
    DateTime(PrimitiveDateTime),
    DateTimeTz(OffsetDateTime),
    /// A Unix timestamp outside the calendar range (before year 0 or after
    /// year 9999). Live Wikidot still renders these with its algorithmic
    /// date, so the raw value is kept for the renderers to format directly.
    Timestamp(i64),
}

impl DateItem {
    pub fn add_timezone(self, offset: UtcOffset) -> Option<Self> {
        let datetime_tz = match self {
            DateItem::Date(date) => date.midnight().assume_offset(offset),
            DateItem::DateTime(datetime) => datetime.assume_offset(offset),
            DateItem::Timestamp(timestamp) => {
                return Some(DateItem::Timestamp(
                    timestamp.saturating_add(i64::from(offset.whole_seconds())),
                ));
            }
            DateItem::DateTimeTz(_) => return None,
        };

        Some(DateItem::DateTimeTz(datetime_tz))
    }

    pub fn timestamp(self) -> i64 {
        match self {
            DateItem::Date(date) => date.midnight().assume_utc().unix_timestamp(),
            DateItem::DateTime(datetime) => datetime.assume_utc().unix_timestamp(),
            DateItem::DateTimeTz(datetime_tz) => datetime_tz.unix_timestamp(),
            DateItem::Timestamp(timestamp) => timestamp,
        }
    }

    pub fn time_since(self) -> i64 {
        // Out-of-range timestamps can sit past the i64 difference bounds.
        self.timestamp().saturating_sub(now().timestamp())
    }

    pub fn to_datetime_tz(self) -> OffsetDateTime {
        match self {
            DateItem::Date(date) => date.midnight().assume_utc(),
            DateItem::DateTime(datetime) => datetime.assume_utc(),
            DateItem::DateTimeTz(datetime_tz) => datetime_tz,
            DateItem::Timestamp(_) => {
                // Out-of-range timestamps have no calendar representation;
                // the formatting entry points render them directly.
                unreachable!("Timestamp cannot convert to OffsetDateTime");
            }
        }
    }

    /// Render an out-of-range Unix timestamp the way live Wikidot does: a
    /// proleptic Gregorian date with a zero-padded day, abbreviated month,
    /// full year (zero-padded to four digits), and 24-hour time.
    pub fn format_timestamp(timestamp: i64) -> String {
        const MONTHS: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov",
            "Dec",
        ];

        let days = timestamp.div_euclid(86_400);
        let seconds = timestamp.rem_euclid(86_400);
        // Howard Hinnant's civil-from-days with C-truncation era division.
        let z = days + 719_468;
        let era = if z >= 0 {
            z / 146_097
        } else {
            (z - 146_096) / 146_097
        };
        let day_of_era = z - era * 146_097;
        let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524
            - day_of_era / 146_096)
            / 365;
        let mut year = year_of_era + era * 400;
        let day_of_year =
            day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let month_prime = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
        let month = if month_prime < 10 {
            month_prime + 3
        } else {
            month_prime - 9
        } as usize;
        if month <= 2 {
            year += 1;
        }

        let year_text = if year < 0 {
            format!("-{:04}", -year)
        } else {
            format!("{:04}", year)
        };
        format!(
            "{:02} {} {} {:02}:{:02}",
            day,
            MONTHS[month - 1],
            year_text,
            seconds / 3_600,
            seconds % 3_600 / 60,
        )
    }

    pub fn format(self, format: Option<&str>, language: &str) -> io::Result<String> {
        match format {
            Some(format) => {
                self.format_strftime(format, language, DateFormatFlavor::Strftime)
            }
            None => self.format_default(language),
        }
    }

    pub fn format_or_default(self, format: Option<&str>, language: &str) -> String {
        let formatted = self.format(format, language);
        format_or_default_result(formatted, || self.format(None, language))
    }

    pub fn format_wikidot(
        self,
        format: Option<&str>,
        language: &str,
    ) -> io::Result<String> {
        match format {
            Some(format) => {
                self.format_strftime(format, language, DateFormatFlavor::Wikidot)
            }
            None => self.format_default(language),
        }
    }

    pub fn format_or_default_wikidot(
        self,
        format: Option<&str>,
        language: &str,
    ) -> String {
        let formatted = self.format_wikidot(format, language);
        format_or_default_result(formatted, || self.format_wikidot(None, language))
    }

    fn format_strftime(
        self,
        format: &str,
        language: &str,
        flavor: DateFormatFlavor,
    ) -> io::Result<String> {
        if !date_format_within_limits(format) {
            return Err(date_format_limit_error());
        }

        // Live Wikidot's static date text always uses the default format,
        // so an explicit format only affects the rendered class.
        if let DateItem::Timestamp(timestamp) = self {
            return Ok(DateItem::format_timestamp(timestamp));
        }

        let datetime = self.to_datetime_tz();
        let locale = cache::locale_from_language(language);
        let mut rendered = String::new();
        let mut literal_start = 0;
        let mut chars = format.char_indices();

        while let Some((index, ch)) = chars.next() {
            if ch != '%' {
                continue;
            }

            rendered.push_str(&format[literal_start..index]);

            let Some((directive_index, mut directive)) = chars.next() else {
                if flavor == DateFormatFlavor::Wikidot {
                    rendered.push('%');
                    literal_start = index + 1;
                    continue;
                }

                return Err(invalid_strftime_error(
                    format,
                    "unexpected end of input after '%'",
                ));
            };

            let mut spec_end = directive_index + directive.len_utf8();
            let mut padded = false;
            if matches!(directive, '_' | '-' | '0') {
                let Some((component_index, component)) = chars.next() else {
                    if flavor == DateFormatFlavor::Wikidot {
                        rendered.push_str(&format[index..spec_end]);
                        literal_start = spec_end;
                        continue;
                    }

                    return Err(invalid_strftime_error(
                        format,
                        "unexpected end of input after padding modifier",
                    ));
                };

                directive = component;
                spec_end = component_index + component.len_utf8();
                padded = true;
            }

            let spec = &format[index..spec_end];
            if flavor == DateFormatFlavor::Wikidot
                && (padded || !wikidot_format_directive(directive))
            {
                rendered.push_str(spec);
            } else {
                render_directive(&mut rendered, &datetime, &locale, directive, spec)?;
            }
            literal_start = spec_end;
        }

        rendered.push_str(&format[literal_start..]);

        Ok(rendered)
    }

    fn format_default(self, language: &str) -> io::Result<String> {
        if let DateItem::Timestamp(timestamp) = self {
            return Ok(DateItem::format_timestamp(timestamp));
        }

        let datetime = self.to_datetime_tz();
        let locale = cache::locale_from_language(language);
        let mut rendered = String::new();

        if append_localized_datetime_full_year(&mut rendered, &datetime, &locale).is_err()
            && datetime.year().unsigned_abs() >= 10_000
        {
            rendered = format!(
                "{:02} {} {} {:02}:{:02}",
                datetime.day(),
                wikidot_month_abbreviation(datetime.month()),
                datetime.year(),
                datetime.hour(),
                datetime.minute(),
            );
        } else if rendered.is_empty() {
            append_localized_datetime_full_year(&mut rendered, &datetime, &locale)?;
        }

        Ok(rendered)
    }
}

fn wikidot_month_abbreviation(month: time::Month) -> &'static str {
    match month {
        time::Month::January => "Jan",
        time::Month::February => "Feb",
        time::Month::March => "Mar",
        time::Month::April => "Apr",
        time::Month::May => "May",
        time::Month::June => "Jun",
        time::Month::July => "Jul",
        time::Month::August => "Aug",
        time::Month::September => "Sep",
        time::Month::October => "Oct",
        time::Month::November => "Nov",
        time::Month::December => "Dec",
    }
}

pub(crate) fn date_format_within_limits(format: &str) -> bool {
    if format.len() > MAX_DATE_FORMAT_BYTES {
        return false;
    }

    let mut directive_count = 0;
    let mut chars = format.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            continue;
        }

        directive_count += 1;
        if directive_count > MAX_DATE_FORMAT_DIRECTIVES {
            return false;
        }

        let Some(directive) = chars.next() else {
            return true;
        };
        if matches!(directive, '_' | '-' | '0') {
            let _ = chars.next();
        }
    }

    true
}

fn date_format_error_fallback(
    first_error: &io::Error,
    fallback_error: &io::Error,
) -> String {
    error!(
        "Error formatting date into string: initial error: {first_error}; fallback error: {fallback_error}"
    );
    str!("<ERROR>")
}

fn format_or_default_result<F>(formatted: io::Result<String>, fallback: F) -> String
where
    F: FnOnce() -> io::Result<String>,
{
    match formatted {
        Ok(datetime) => datetime,
        Err(first_error) => match fallback() {
            Ok(datetime) => datetime,
            Err(fallback_error) => {
                date_format_error_fallback(&first_error, &fallback_error)
            }
        },
    }
}

fn map_format_result(result: Result<String, time::error::Format>) -> io::Result<String> {
    use time::error::Format;

    result.map_err(|error| match error {
        Format::StdIo(io_error) => io_error,
        _ => io::Error::other(error),
    })
}

fn wikidot_format_directive(directive: char) -> bool {
    matches!(
        directive,
        'r' | 'R'
            | 'T'
            | 'D'
            | 'a'
            | 'A'
            | 'b'
            | 'B'
            | 'c'
            | 'd'
            | 'e'
            | 'H'
            | 'I'
            | 'm'
            | 'M'
            | 'p'
            | 'S'
            | 'y'
            | 'Y'
            | 'z'
            | 'O'
    )
}

fn render_directive(
    rendered: &mut String,
    datetime: &OffsetDateTime,
    locale: &Locale,
    directive: char,
    spec: &str,
) -> io::Result<()> {
    match directive {
        '%' => rendered.push('%'),
        'a' => append_localized_weekday(rendered, datetime, locale, true)?,
        'A' => append_localized_weekday(rendered, datetime, locale, false)?,
        'b' => append_localized_month(rendered, datetime, locale, true)?,
        'B' => append_localized_month(rendered, datetime, locale, false)?,
        'c' => append_localized_datetime_full_year(rendered, datetime, locale)?,
        'O' => append_relative_time(rendered, *datetime, locale)?,
        'p' => append_localized_day_period(rendered, datetime, locale, true)?,
        'P' => append_localized_day_period(rendered, datetime, locale, false)?,
        'r' => append_localized_time_h12(rendered, datetime, locale)?,
        'X' => append_localized_time(rendered, datetime, locale)?,
        'x' => append_localized_date(rendered, datetime, locale)?,
        'Z' | 'z' => append_timezone_gmt(rendered, &datetime.offset()),
        _ => append_unlocalized_directive(rendered, datetime, spec)?,
    }

    Ok(())
}

fn append_localized_weekday(
    rendered: &mut String,
    datetime: &OffsetDateTime,
    locale: &Locale,
    abbreviated: bool,
) -> io::Result<()> {
    let length = if abbreviated {
        cache::FormatLength::Short
    } else {
        cache::FormatLength::Long
    };
    let date = to_icu_date(*datetime)?;

    cache::with_weekday_formatter(locale, length, |formatter| {
        append_normalized_display(rendered, formatter.format(&date))
    })
}

fn append_localized_month(
    rendered: &mut String,
    datetime: &OffsetDateTime,
    locale: &Locale,
    abbreviated: bool,
) -> io::Result<()> {
    let length = if abbreviated {
        cache::FormatLength::Medium
    } else {
        cache::FormatLength::Long
    };
    let date = to_icu_date(*datetime)?;

    cache::with_month_formatter(locale, length, |formatter| {
        append_normalized_display(rendered, formatter.format(&date))
    })
}

fn append_localized_date(
    rendered: &mut String,
    datetime: &OffsetDateTime,
    locale: &Locale,
) -> io::Result<()> {
    let date = to_icu_date(*datetime)?;

    cache::with_date_formatter(locale, |formatter| {
        append_normalized_display(rendered, formatter.format(&date))
    })
}

fn append_localized_datetime_full_year(
    rendered: &mut String,
    datetime: &OffsetDateTime,
    locale: &Locale,
) -> io::Result<()> {
    let datetime = to_icu_datetime(*datetime)?;

    cache::with_datetime_formatter(locale, |formatter| {
        append_normalized_display(rendered, formatter.format(&datetime))
    })
}

fn append_localized_time(
    rendered: &mut String,
    datetime: &OffsetDateTime,
    locale: &Locale,
) -> io::Result<()> {
    let time = to_icu_time(*datetime)?;

    cache::with_time_formatter(locale, cache::TimeHourCycle::LocaleDefault, |formatter| {
        append_normalized_display(rendered, formatter.format(&time))
    })
}

fn append_localized_day_period(
    rendered: &mut String,
    datetime: &OffsetDateTime,
    locale: &Locale,
    uppercase: bool,
) -> io::Result<()> {
    static DAY_PERIOD_PATTERN: LazyLock<DateTimePattern> =
        LazyLock::new(|| "a".parse().expect("valid day-period pattern"));

    let time = to_icu_time(*datetime)?;
    cache::with_day_period_names(locale, |names| {
        let formatter = names
            .with_pattern_unchecked(&DAY_PERIOD_PATTERN)
            .format(&time);
        let raw = formatter
            .try_write_to_string()
            .map_err(|(error, _)| localization_error(error))?;

        if uppercase {
            append_normalized_display(rendered, raw.as_ref())
        } else {
            append_normalized_display(rendered, raw.as_ref().to_lowercase())
        }
    })
}

fn append_localized_time_h12(
    rendered: &mut String,
    datetime: &OffsetDateTime,
    locale: &Locale,
) -> io::Result<()> {
    let time = to_icu_time(*datetime)?;

    cache::with_time_formatter(locale, cache::TimeHourCycle::H12, |formatter| {
        append_normalized_display(rendered, formatter.format(&time))
    })
}

fn append_relative_time(
    rendered: &mut String,
    datetime: OffsetDateTime,
    locale: &Locale,
) -> io::Result<()> {
    let (value, unit) = relative_time_value(datetime);
    let numeric = match unit {
        RelativeTimeUnit::Second | RelativeTimeUnit::Minute | RelativeTimeUnit::Hour => {
            cache::RelativeTimeNumeric::Always
        }
        RelativeTimeUnit::Day => cache::RelativeTimeNumeric::Auto,
    };

    cache::with_relative_time_formatter(locale, unit, numeric, |formatter| {
        append_normalized_display(rendered, formatter.format(Decimal::from(value)))
    })
}

/// Helper to create a `RelativeTimeFormatterOptions`.
/// Because the struct is non-exhaustive, we cannot use the struct syntax to
/// create new instances here.
fn relative_time_fmt_options(
    numeric: cache::RelativeTimeNumeric,
) -> RelativeTimeFormatterOptions {
    let mut options = RelativeTimeFormatterOptions::default();
    options.numeric = Some(numeric.as_icu_numeric());
    options
}

fn relative_time_value(datetime: OffsetDateTime) -> (i64, RelativeTimeUnit) {
    let delta_seconds = datetime.unix_timestamp() - now().timestamp();
    let abs_delta = delta_seconds.unsigned_abs();

    if abs_delta < 60 {
        (delta_seconds, RelativeTimeUnit::Second)
    } else if abs_delta < 3_600 {
        (delta_seconds / 60, RelativeTimeUnit::Minute)
    } else if abs_delta < 86_400 {
        (delta_seconds / 3_600, RelativeTimeUnit::Hour)
    } else {
        (delta_seconds / 86_400, RelativeTimeUnit::Day)
    }
}

#[derive(Copy, Clone, Eq, Hash, PartialEq)]
enum RelativeTimeUnit {
    Second,
    Minute,
    Hour,
    Day,
}

fn append_timezone_gmt(rendered: &mut String, offset: &UtcOffset) {
    let total_seconds = offset.whole_seconds();
    let sign = if total_seconds < 0 { '-' } else { '+' };
    let absolute_seconds = total_seconds.abs();
    let hours = absolute_seconds / 3600;
    let minutes = (absolute_seconds % 3600) / 60;

    if minutes == 0 {
        str_write!(rendered, "GMT{sign}{hours:02}");
    } else {
        str_write!(rendered, "GMT{sign}{hours:02}:{minutes:02}");
    }
}

fn append_normalized_display(
    rendered: &mut String,
    value: impl fmt::Display,
) -> io::Result<()> {
    use fmt::Write;

    write!(&mut NormalizingWriter(rendered), "{value}")
        .map_err(|_| io::Error::other("normalizing localized date display failed"))
}

struct NormalizingWriter<'a>(&'a mut String);

impl fmt::Write for NormalizingWriter<'_> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        for ch in value.chars() {
            if matches!(ch, '\u{00A0}' | '\u{202F}') {
                self.0.push(' ');
            } else {
                self.0.push(ch);
            }
        }

        Ok(())
    }
}

fn append_unlocalized_directive(
    rendered: &mut String,
    datetime: &OffsetDateTime,
    spec: &str,
) -> io::Result<()> {
    let items = parse_strftime_borrowed(spec).map_err(|error| {
        invalid_strftime_error(spec, &format!("failed to parse directive: {error}"))
    })?;

    rendered.push_str(&map_format_result(datetime.format(&items))?);
    Ok(())
}

fn to_icu_date(datetime: OffsetDateTime) -> io::Result<IcuDate<Iso>> {
    let date = datetime.date();

    IcuDate::try_new_iso(date.year(), date.month().into(), date.day())
        .map_err(localization_error)
}

fn to_icu_datetime(datetime: OffsetDateTime) -> io::Result<IcuDateTime<Iso>> {
    Ok(IcuDateTime {
        date: to_icu_date(datetime)?,
        time: to_icu_time(datetime)?,
    })
}

fn to_icu_time(datetime: OffsetDateTime) -> io::Result<IcuTime> {
    let time = datetime.time();

    IcuTime::try_new(time.hour(), time.minute(), time.second(), time.nanosecond())
        .map_err(localization_error)
}

fn invalid_strftime_error(format: &str, message: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("invalid strftime format string '{format}': {message}"),
    )
}

fn date_format_limit_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "strftime format string exceeds supported limits",
    )
}

fn localization_error(error: impl ToString) -> io::Error {
    io::Error::other(error.to_string())
}

impl From<Date> for DateItem {
    #[inline]
    fn from(date: Date) -> Self {
        DateItem::Date(date)
    }
}

impl From<PrimitiveDateTime> for DateItem {
    #[inline]
    fn from(datetime: PrimitiveDateTime) -> Self {
        DateItem::DateTime(datetime)
    }
}

impl From<OffsetDateTime> for DateItem {
    #[inline]
    fn from(datetime_tz: OffsetDateTime) -> Self {
        DateItem::DateTimeTz(datetime_tz)
    }
}

cfg_if! {
    if #[cfg(test)] {
        #[inline]
        fn now() -> DateItem {
            time::macros::datetime!(2010-01-01 08:10:00).into()
        }
    } else {
        #[inline]
        fn now() -> DateItem {
            OffsetDateTime::now_utc().into()
        }
    }
}

#[test]
fn date_format_supports_strftime() {
    let date = DateItem::from(time::macros::datetime!(2010-01-01 08:10:00 +00:00));

    assert_eq!(
        date.format(Some("%Y-%m-%d %H:%M:%S %z"), "en").unwrap(),
        "2010-01-01 08:10:00 GMT+00",
    );
}

#[test]
fn date_format_rejects_invalid_strftime() {
    let date = DateItem::from(time::macros::datetime!(2010-01-01 08:10:00 +00:00));

    date.format(Some("%Q"), "en")
        .expect_err("invalid strftime format unexpectedly succeeded");
}

#[test]
fn date_items_without_timezone_assume_utc() {
    assert_eq!(
        DateItem::from(time::macros::date!(2010 - 01 - 01)).to_datetime_tz(),
        time::macros::datetime!(2010-01-01 00:00:00 +00:00),
    );
    assert_eq!(
        DateItem::from(time::macros::datetime!(2010-01-01 08:10:00)).to_datetime_tz(),
        time::macros::datetime!(2010-01-01 08:10:00 +00:00),
    );
}

#[test]
fn date_format_supports_escaped_percent_and_padding_modifiers() {
    let date = DateItem::from(time::macros::datetime!(2010-01-01 08:10:00 +00:00));

    assert_eq!(
        date.format(Some("%% [%_d] [%-m] [%0H]"), "en").unwrap(),
        "% [ 1] [1] [08]",
    );
}

#[test]
fn date_format_rejects_trailing_percent_and_padding_modifier() {
    let date = DateItem::from(time::macros::datetime!(2010-01-01 08:10:00 +00:00));

    let error = date.format(Some("%"), "en").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(
        error.to_string(),
        "invalid strftime format string '%': unexpected end of input after '%'",
    );

    for format in ["%_", "%-", "%0"] {
        let error = date.format(Some(format), "en").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(
            error.to_string(),
            format!(
                "invalid strftime format string '{format}': unexpected end of input after padding modifier"
            ),
        );
    }
}

#[test]
fn date_format_limits_reject_expensive_inputs_before_rendering() {
    let date = DateItem::from(time::macros::datetime!(2010-01-01 08:10:00 +00:00));
    let too_long = "x".repeat(MAX_DATE_FORMAT_BYTES + 1);
    let too_many_directives = "%c".repeat(MAX_DATE_FORMAT_DIRECTIVES + 1);

    assert!(!date_format_within_limits(&too_long));
    assert!(!date_format_within_limits(&too_many_directives));
    assert!(date_format_within_limits("%"));
    assert!(date_format_within_limits("%_d %-m %0H"));

    for format in [too_long.as_str(), too_many_directives.as_str()] {
        let error = date.format(Some(format), "en").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(
            error.to_string(),
            "strftime format string exceeds supported limits",
        );
    }
}

#[test]
fn date_format_falls_back_to_default() {
    let date = DateItem::from(time::macros::datetime!(2010-01-01 08:10:00 +00:00));

    assert_eq!(
        date.format_or_default(Some("%Q"), "en"),
        date.format(None, "en").unwrap(),
    );
}

#[test]
fn date_format_wikidot_expands_only_format_odate_directives() {
    let date = DateItem::from(time::macros::datetime!(2009-12-25 08:18:05 +02:00));
    let format = "[a %a] [A %A] [b %b] [B %B] [c %c] [d %d] [D %D] [e %e] [H %H] [I %I] [m %m] [M %M] [O %O] [p %p] [r %r] [R %R] [S %S] [T %T] [y %y] [Y %Y] [z %z]";

    assert_eq!(
        date.format_wikidot(Some(format), "en").unwrap(),
        "[a Fri] [A Friday] [b Dec] [B December] [c 12/25/2009, 8:18:05 AM] [d 25] [D 12/25/09] [e 25] [H 08] [I 08] [m 12] [M 18] [O 7 days ago] [p AM] [r 8:18:05 AM] [R 08:18] [S 05] [T 08:18:05] [y 09] [Y 2009] [z GMT+02]",
    );
}

#[test]
fn date_format_wikidot_leaves_other_directives_literal() {
    let date = DateItem::from(time::macros::datetime!(2009-12-25 08:18:05 +02:00));
    let format = "[C %C] [F %F] [g %g] [G %G] [h %h] [j %j] [n %n] [P %P] [s %s] [t %t] [u %u] [U %U] [V %V] [w %w] [W %W] [x %x] [X %X] [Z %Z] [Q %Q]";

    assert_eq!(date.format_wikidot(Some(format), "en").unwrap(), format);
    assert_eq!(date.format_wikidot(Some("%%"), "en").unwrap(), "%%");
    assert_eq!(
        date.format_wikidot(Some("%_d %-d %0d"), "en").unwrap(),
        "%_d %-d %0d",
    );
    assert_eq!(date.format_wikidot(Some("%Y %"), "en").unwrap(), "2009 %",);
    assert_eq!(date.format_wikidot(Some("%_"), "en").unwrap(), "%_");
    assert_eq!(date.format_or_default_wikidot(Some("%s"), "en"), "%s");
}

#[test]
fn date_format_error_fallback_returns_error_marker() {
    let first_error = io::Error::new(io::ErrorKind::InvalidInput, "initial");
    let fallback_error = io::Error::other("fallback");

    assert_eq!(
        format_or_default_result(Err(first_error), || Err(fallback_error)),
        "<ERROR>",
    );
}

#[test]
fn date_format_defaults_to_localized_short_datetime() {
    let date = DateItem::from(time::macros::datetime!(2010-01-01 08:10:00 +00:00));

    assert_eq!(date.format(None, "en-US").unwrap(), "1/1/2010, 8:10:00 AM");
}

#[test]
fn date_format_cold_and_warm_localized_outputs_match() {
    let date = DateItem::from(time::macros::datetime!(2009-12-25 08:18:05 +02:00));
    let format =
        "[a %a] [A %A] [b %b] [B %B] [c %c] [O %O] [p %p] [P %P] [r %r] [x %x] [X %X]";

    for language in ["en-US", "es-ES", "fr", "ja", "zh-CN"] {
        let cold_default = date.format(None, language).unwrap();
        let warm_default = date.format(None, language).unwrap();
        assert_eq!(warm_default, cold_default);

        let cold_custom = date.format(Some(format), language).unwrap();
        let warm_custom = date.format(Some(format), language).unwrap();
        assert_eq!(warm_custom, cold_custom);
    }
}

#[test]
fn date_format_uses_localized_relative_time_patterns() {
    let past_date = DateItem::from(time::macros::datetime!(2009-12-31 08:10:00 +00:00));
    let future_date = DateItem::from(time::macros::datetime!(2010-01-02 08:10:00 +00:00));

    assert_eq!(past_date.format(Some("%O"), "fr").unwrap(), "hier");
    assert_eq!(future_date.format(Some("%O"), "fr").unwrap(), "demain");
}

#[test]
fn date_format_relative_time_covers_seconds_minutes_hours() {
    let cases = [
        (
            time::macros::datetime!(2010-01-01 08:10:45 +00:00),
            "in 45 seconds",
        ),
        (
            time::macros::datetime!(2010-01-01 08:12:00 +00:00),
            "in 2 minutes",
        ),
        (
            time::macros::datetime!(2010-01-01 10:10:00 +00:00),
            "in 2 hours",
        ),
    ];

    for (datetime, expected) in cases {
        assert_eq!(
            DateItem::from(datetime)
                .format(Some("%O"), "en-US")
                .unwrap(),
            expected,
        );
    }
}

#[test]
fn date_error_adapters_map_to_io_errors() {
    use time::error::Format;

    let error = map_format_result(Err(Format::StdIo(io::Error::new(
        io::ErrorKind::BrokenPipe,
        "writer stopped",
    ))))
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    assert_eq!(error.to_string(), "writer stopped");

    let error = map_format_result(Err(Format::InvalidComponent("year"))).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert_eq!(
        error.to_string(),
        "The year component cannot be formatted into the requested format.",
    );

    let error = localization_error("icu failed");
    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert_eq!(error.to_string(), "icu failed");
}

#[test]
fn date_format_supports_prefix_and_non_ascii_day_periods() {
    let date_am = DateItem::from(time::macros::datetime!(2009-12-25 08:18:05 +00:00));
    let date_pm = DateItem::from(time::macros::datetime!(2009-12-25 13:18:05 +00:00));

    assert_eq!(date_am.format(Some("%p"), "en-US").unwrap(), "AM");
    assert_eq!(date_am.format(Some("%P"), "en-US").unwrap(), "am");
    assert_eq!(date_pm.format(Some("%p"), "en-US").unwrap(), "PM");
    assert_eq!(date_pm.format(Some("%P"), "en-US").unwrap(), "pm");

    assert_eq!(date_am.format(Some("%p"), "ja").unwrap(), "午前");
    assert_eq!(date_am.format(Some("%P"), "ja").unwrap(), "午前");
    assert_eq!(date_pm.format(Some("%p"), "ja").unwrap(), "午後");
    assert_eq!(date_pm.format(Some("%P"), "ja").unwrap(), "午後");
    assert_eq!(date_am.format(Some("%p"), "zh-CN").unwrap(), "上午");
    assert_eq!(date_am.format(Some("%p"), "fa").unwrap(), "ق.ظ.");
}

#[test]
fn date_format_supports_prefix_day_periods_in_twelve_hour_time() {
    let date = DateItem::from(time::macros::datetime!(2009-12-25 08:18:05 +00:00));

    assert_eq!(date.format(Some("%r"), "zh-CN").unwrap(), "上午8:18:05");
    assert_eq!(date.format(Some("%r"), "ja").unwrap(), "午前8:18:05");
}

#[test]
fn date_format_supports_full_regression_matrix_in_spanish() {
    let date = DateItem::from(time::macros::datetime!(2009-12-25 08:18:05 +02:00));
    let format = "[a %a] [A %A] [b %b] [B %B] [c %c] [d %d] [D %D] [e %e] [H %H] [I %I] [m %m] [M %M] [O %O] [p %p] [P %P] [r %r] [R %R] [S %S] [T %T] [x %x] [X %X] [y %y] [Y %Y] [z %z] [Z %Z]";

    assert_eq!(
        date.format(Some(format), "es-ES").unwrap(),
        "[a vie] [A viernes] [b dic] [B diciembre] [c 25/12/2009, 08:18:05] [d 25] [D 12/25/09] [e 25] [H 08] [I 08] [m 12] [M 18] [O hace 7 d\u{00ED}as] [p a. m.] [P a. m.] [r 8:18:05 a. m.] [R 08:18] [S 05] [T 08:18:05] [x 25/12/09] [X 08:18:05] [y 09] [Y 2009] [z GMT+02] [Z GMT+02]"
    );
}

#[test]
fn date_format_supports_full_regression_matrix_in_english() {
    let date = DateItem::from(time::macros::datetime!(2009-12-25 08:18:05 +02:00));
    let format = "[a %a] [A %A] [b %b] [B %B] [c %c] [d %d] [D %D] [e %e] [H %H] [I %I] [m %m] [M %M] [O %O] [p %p] [P %P] [r %r] [R %R] [S %S] [T %T] [x %x] [X %X] [y %y] [Y %Y] [z %z] [Z %Z]";

    assert_eq!(
        date.format(Some(format), "en-US").unwrap(),
        "[a Fri] [A Friday] [b Dec] [B December] [c 12/25/2009, 8:18:05 AM] [d 25] [D 12/25/09] [e 25] [H 08] [I 08] [m 12] [M 18] [O 7 days ago] [p AM] [P am] [r 8:18:05 AM] [R 08:18] [S 05] [T 08:18:05] [x 12/25/09] [X 8:18:05 AM] [y 09] [Y 2009] [z GMT+02] [Z GMT+02]"
    );
}

#[test]
fn date_format_supports_full_regression_matrix_in_japanese() {
    let date = DateItem::from(time::macros::datetime!(2009-12-25 08:18:05 +02:00));
    let format = "[a %a] [A %A] [b %b] [B %B] [c %c] [d %d] [D %D] [e %e] [H %H] [I %I] [m %m] [M %M] [O %O] [p %p] [P %P] [r %r] [R %R] [S %S] [T %T] [x %x] [X %X] [y %y] [Y %Y] [z %z] [Z %Z]";

    assert_eq!(
        date.format(Some(format), "ja").unwrap(),
        "[a 金] [A 金曜日] [b 12月] [B 12月] [c 2009/12/25 8:18:05] [d 25] [D 12/25/09] [e 25] [H 08] [I 08] [m 12] [M 18] [O 7 日前] [p 午前] [P 午前] [r 午前8:18:05] [R 08:18] [S 05] [T 08:18:05] [x 2009/12/25] [X 8:18:05] [y 09] [Y 2009] [z GMT+02] [Z GMT+02]"
    );
}
