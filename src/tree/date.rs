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
use icu_datetime::fieldsets::{E, M, T, YMD, YMDT};
use icu_datetime::input::{DateTime as IcuDateTime, Time as IcuTime};
use icu_datetime::options::{TimePrecision, YearStyle};
use icu_datetime::preferences::HourCycle;
use icu_datetime::{DateTimeFormatter, DateTimeFormatterPreferences};
use icu_decimal::input::Decimal;
use icu_experimental::relativetime::options::Numeric;
use icu_experimental::relativetime::{
    RelativeTimeFormatter, RelativeTimeFormatterOptions, RelativeTimeFormatterPreferences,
};
use icu_locale::Locale;
use std::io;
use time::format_description::parse_strftime_borrowed;
use time::{Date, OffsetDateTime, PrimitiveDateTime, UtcOffset};

#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case", untagged)]
pub enum DateItem {
    Date(Date),
    DateTime(PrimitiveDateTime),
    DateTimeTz(OffsetDateTime),
}

impl DateItem {
    pub fn add_timezone(self, offset: UtcOffset) -> Option<Self> {
        let datetime_tz = match self {
            DateItem::Date(date) => date.midnight().assume_offset(offset),
            DateItem::DateTime(datetime) => datetime.assume_offset(offset),
            DateItem::DateTimeTz(_) => return None,
        };

        Some(DateItem::DateTimeTz(datetime_tz))
    }

    pub fn timestamp(self) -> i64 {
        match self {
            DateItem::Date(date) => date.midnight().assume_utc().unix_timestamp(),
            DateItem::DateTime(datetime) => datetime.assume_utc().unix_timestamp(),
            DateItem::DateTimeTz(datetime_tz) => datetime_tz.unix_timestamp(),
        }
    }

    pub fn time_since(self) -> i64 {
        self.timestamp() - now().timestamp()
    }

    pub fn to_datetime_tz(self) -> OffsetDateTime {
        match self {
            DateItem::Date(date) => date.midnight().assume_utc(),
            DateItem::DateTime(datetime) => datetime.assume_utc(),
            DateItem::DateTimeTz(datetime_tz) => datetime_tz,
        }
    }

    pub fn format(self, format: Option<&str>, language: &str) -> io::Result<String> {
        match format {
            Some(format) => self.format_strftime(format, language),
            None => self.format_default(language),
        }
    }

    pub fn format_or_default(self, format: Option<&str>, language: &str) -> String {
        match self.format(format, language) {
            Ok(datetime) => datetime,
            Err(first_error) => self.format(None, language).unwrap_or_else(|fallback_error| {
                error!(
                    "Error formatting date into string: initial error: {first_error}; fallback error: {fallback_error}"
                );
                String::from("<ERROR>")
            }),
        }
    }

    fn format_strftime(self, format: &str, language: &str) -> io::Result<String> {
        let datetime = self.to_datetime_tz();
        let locale = locale_from_language(language);
        let mut rendered = String::new();
        let mut literal_start = 0;
        let mut chars = format.char_indices().peekable();

        while let Some((index, ch)) = chars.next() {
            if ch != '%' {
                continue;
            }

            rendered.push_str(&format[literal_start..index]);

            let Some((directive_index, mut directive)) = chars.next() else {
                return Err(invalid_strftime_error(
                    format,
                    "unexpected end of input after '%'",
                ));
            };

            let mut spec_end = directive_index + directive.len_utf8();
            if matches!(directive, '_' | '-' | '0') {
                let Some((component_index, component)) = chars.next() else {
                    return Err(invalid_strftime_error(
                        format,
                        "unexpected end of input after padding modifier",
                    ));
                };

                directive = component;
                spec_end = component_index + component.len_utf8();
            }

            let spec = &format[index..spec_end];
            rendered.push_str(&render_directive(&datetime, &locale, directive, spec)?);
            literal_start = spec_end;
        }

        rendered.push_str(&format[literal_start..]);

        Ok(rendered)
    }

    fn format_default(self, language: &str) -> io::Result<String> {
        let datetime = self.to_datetime_tz();
        let locale = locale_from_language(language);

        format_localized_datetime_short(&datetime, &locale)
    }
}

fn map_format_result(result: Result<String, time::error::Format>) -> io::Result<String> {
    use time::error::Format;

    result.map_err(|error| match error {
        Format::StdIo(io_error) => io_error,
        _ => io::Error::other(error),
    })
}

fn render_directive(
    datetime: &OffsetDateTime,
    locale: &Locale,
    directive: char,
    spec: &str,
) -> io::Result<String> {
    match directive {
        '%' => Ok(String::from("%")),
        'a' => format_localized_weekday(datetime, locale, true),
        'A' => format_localized_weekday(datetime, locale, false),
        'b' => format_localized_month(datetime, locale, true),
        'B' => format_localized_month(datetime, locale, false),
        'c' => format_localized_datetime_full_year(datetime, locale),
        'O' => format_relative_time(*datetime, locale),
        'p' => format_localized_day_period(datetime, locale, true),
        'P' => format_localized_day_period(datetime, locale, false),
        'r' => format_localized_twelve_hour_time(datetime, locale),
        'X' => format_localized_time(datetime, locale),
        'x' => format_localized_date(datetime, locale),
        'Z' => Ok(format_timezone_gmt(&datetime.offset())),
        _ => format_unlocalized_directive(datetime, spec),
    }
}

fn format_localized_weekday(
    datetime: &OffsetDateTime,
    locale: &Locale,
    abbreviated: bool,
) -> io::Result<String> {
    let formatter = if abbreviated {
        DateTimeFormatter::try_new(locale.clone().into(), E::short())
    } else {
        DateTimeFormatter::try_new(locale.clone().into(), E::long())
    }
    .map_err(localization_error)?;
    let date = to_icu_date(*datetime)?;

    Ok(normalize_icu_spacing(formatter.format(&date).to_string()))
}

fn format_localized_month(
    datetime: &OffsetDateTime,
    locale: &Locale,
    abbreviated: bool,
) -> io::Result<String> {
    let formatter = if abbreviated {
        DateTimeFormatter::try_new(locale.clone().into(), M::medium())
    } else {
        DateTimeFormatter::try_new(locale.clone().into(), M::long())
    }
    .map_err(localization_error)?;
    let date = to_icu_date(*datetime)?;

    Ok(normalize_icu_spacing(formatter.format(&date).to_string()))
}

fn format_localized_date(
    datetime: &OffsetDateTime,
    locale: &Locale,
) -> io::Result<String> {
    let formatter = DateTimeFormatter::try_new(locale.clone().into(), YMD::short())
        .map_err(localization_error)?;
    let date = to_icu_date(*datetime)?;

    Ok(normalize_icu_spacing(formatter.format(&date).to_string()))
}

fn format_localized_datetime_short(
    datetime: &OffsetDateTime,
    locale: &Locale,
) -> io::Result<String> {
    let formatter = DateTimeFormatter::try_new(locale.clone().into(), YMDT::short())
        .map_err(localization_error)?;
    let datetime = to_icu_datetime(*datetime)?;

    Ok(normalize_icu_spacing(
        formatter.format(&datetime).to_string(),
    ))
}

fn format_localized_datetime_full_year(
    datetime: &OffsetDateTime,
    locale: &Locale,
) -> io::Result<String> {
    let formatter = DateTimeFormatter::try_new(
        locale.clone().into(),
        YMDT::short()
            .with_year_style(YearStyle::Full)
            .with_time_precision(TimePrecision::Second),
    )
    .map_err(localization_error)?;
    let datetime = to_icu_datetime(*datetime)?;

    Ok(normalize_icu_spacing(
        formatter.format(&datetime).to_string(),
    ))
}

fn format_localized_time(
    datetime: &OffsetDateTime,
    locale: &Locale,
) -> io::Result<String> {
    let formatter = DateTimeFormatter::try_new(locale.clone().into(), T::medium())
        .map_err(localization_error)?;
    let time = to_icu_time(*datetime)?;

    Ok(normalize_icu_spacing(formatter.format(&time).to_string()))
}

fn format_localized_twelve_hour_time(
    datetime: &OffsetDateTime,
    locale: &Locale,
) -> io::Result<String> {
    let period = format_localized_day_period(datetime, locale, true)?;
    let time = datetime.time();
    let hour = match time.hour() % 12 {
        0 => 12,
        hour => hour,
    };

    if period.is_empty() {
        Ok(format!(
            "{hour:02}:{:02}:{:02}",
            time.minute(),
            time.second()
        ))
    } else {
        Ok(format!(
            "{hour:02}:{:02}:{:02} {period}",
            time.minute(),
            time.second(),
        ))
    }
}

fn format_localized_day_period(
    datetime: &OffsetDateTime,
    locale: &Locale,
    uppercase: bool,
) -> io::Result<String> {
    let formatted = extract_day_period(&format_localized_time_h12(datetime, locale)?);

    if uppercase {
        Ok(normalize_icu_spacing(formatted))
    } else {
        Ok(normalize_icu_spacing(formatted.to_lowercase()))
    }
}

fn format_localized_time_h12(
    datetime: &OffsetDateTime,
    locale: &Locale,
) -> io::Result<String> {
    let mut prefs: DateTimeFormatterPreferences = locale.clone().into();
    prefs.hour_cycle = Some(HourCycle::H12);

    let formatter =
        DateTimeFormatter::try_new(prefs, T::medium()).map_err(localization_error)?;
    let time = to_icu_time(*datetime)?;

    Ok(normalize_icu_spacing(formatter.format(&time).to_string()))
}

fn extract_day_period(formatted_time: &str) -> String {
    formatted_time
        .char_indices()
        .rfind(|(_, ch)| ch.is_ascii_digit())
        .map(|(index, ch)| formatted_time[index + ch.len_utf8()..].trim().to_string())
        .unwrap_or_else(|| formatted_time.trim().to_string())
}

fn format_relative_time(datetime: OffsetDateTime, locale: &Locale) -> io::Result<String> {
    let (value, unit) = relative_time_value(datetime);
    let prefs: RelativeTimeFormatterPreferences = locale.clone().into();
    let formatter = match unit {
        RelativeTimeUnit::Second => RelativeTimeFormatter::try_new_long_second(
            prefs,
            RelativeTimeFormatterOptions {
                numeric: Numeric::Always,
            },
        ),
        RelativeTimeUnit::Minute => RelativeTimeFormatter::try_new_long_minute(
            prefs,
            RelativeTimeFormatterOptions {
                numeric: Numeric::Always,
            },
        ),
        RelativeTimeUnit::Hour => RelativeTimeFormatter::try_new_long_hour(
            prefs,
            RelativeTimeFormatterOptions {
                numeric: Numeric::Always,
            },
        ),
        RelativeTimeUnit::Day => RelativeTimeFormatter::try_new_long_day(
            prefs,
            RelativeTimeFormatterOptions {
                numeric: Numeric::Auto,
            },
        ),
    }
    .map_err(localization_error)?;

    Ok(normalize_icu_spacing(
        formatter.format(Decimal::from(value)).to_string(),
    ))
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

#[derive(Copy, Clone)]
enum RelativeTimeUnit {
    Second,
    Minute,
    Hour,
    Day,
}

fn format_timezone_gmt(offset: &UtcOffset) -> String {
    let total_seconds = offset.whole_seconds();
    let sign = if total_seconds < 0 { '-' } else { '+' };
    let absolute_seconds = total_seconds.abs();
    let hours = absolute_seconds / 3600;
    let minutes = (absolute_seconds % 3600) / 60;

    if minutes == 0 {
        format!("GMT{sign}{hours:02}")
    } else {
        format!("GMT{sign}{hours:02}:{minutes:02}")
    }
}

fn normalize_icu_spacing(value: String) -> String {
    value.replace(['\u{00A0}', '\u{202F}'], " ")
}

fn format_unlocalized_directive(
    datetime: &OffsetDateTime,
    spec: &str,
) -> io::Result<String> {
    let items = parse_strftime_borrowed(spec).map_err(|error| {
        invalid_strftime_error(spec, &format!("failed to parse directive: {error}"))
    })?;

    map_format_result(datetime.format(&items))
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

fn locale_from_language(language: &str) -> Locale {
    Locale::try_from_str(language).unwrap_or_else(|error| {
        debug!(
            "Invalid date render locale '{language}', falling back to English: {error}"
        );
        Locale::try_from_str("en").expect("English locale should always parse")
    })
}

fn invalid_strftime_error(format: &str, message: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("invalid strftime format string '{format}': {message}"),
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
        /// Produces a fixed constant value as "now".
        ///
        /// We need a consistent date for render tests to not constantly expire.
        #[inline]
        fn now() -> DateItem {
            time::macros::datetime!(2026-03-12 06:18:05 +00:00).into()
        }
    } else {
        /// Helper function to get the current date and time, UTC.
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
        "2010-01-01 08:10:00 +0000",
    );
}

#[test]
fn date_format_rejects_invalid_strftime() {
    let date = DateItem::from(time::macros::datetime!(2010-01-01 08:10:00 +00:00));

    date.format(Some("%Q"), "en")
        .expect_err("invalid strftime format unexpectedly succeeded");
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
fn date_format_defaults_to_localized_short_datetime() {
    let date = DateItem::from(time::macros::datetime!(2010-01-01 08:10:00 +00:00));

    assert_eq!(date.format(None, "en-US").unwrap(), "1/1/10, 8:10:00 AM");
}

#[test]
fn date_format_uses_localized_relative_time_patterns() {
    let past_date = DateItem::from(time::macros::datetime!(2026-03-11 06:18:05 +00:00));
    let future_date = DateItem::from(time::macros::datetime!(2026-03-13 06:18:05 +00:00));

    assert_eq!(past_date.format(Some("%O"), "fr").unwrap(), "hier");
    assert_eq!(future_date.format(Some("%O"), "fr").unwrap(), "demain");
}

#[test]
fn date_format_supports_full_regression_matrix_in_spanish() {
    let date = DateItem::from(time::macros::datetime!(2025-10-12 08:18:05 +02:00));
    let format = "[a %a] [A %A] [b %b] [B %B] [c %c] [d %d] [D %D] [e %e] [H %H] [I %I] [m %m] [M %M] [O %O] [p %p] [P %P] [r %r] [R %R] [S %S] [T %T] [x %x] [X %X] [y %y] [Y %Y] [z %z] [Z %Z]";

    assert_eq!(
        date.format(Some(format), "es-ES").unwrap(),
        "[a dom] [A domingo] [b oct] [B octubre] [c 12/10/2025, 08:18:05] [d 12] [D 10/12/25] [e 12] [H 08] [I 08] [m 10] [M 18] [O hace 151 d\u{00ED}as] [p a. m.] [P a. m.] [r 08:18:05 a. m.] [R 08:18] [S 05] [T 08:18:05] [x 12/10/25] [X 08:18:05] [y 25] [Y 2025] [z +0200] [Z GMT+02]"
    );
}

#[test]
fn date_format_supports_full_regression_matrix_in_english() {
    let date = DateItem::from(time::macros::datetime!(2025-10-12 08:18:05 +02:00));
    let format = "[a %a] [A %A] [b %b] [B %B] [c %c] [d %d] [D %D] [e %e] [H %H] [I %I] [m %m] [M %M] [O %O] [p %p] [P %P] [r %r] [R %R] [S %S] [T %T] [x %x] [X %X] [y %y] [Y %Y] [z %z] [Z %Z]";

    assert_eq!(
        date.format(Some(format), "en-US").unwrap(),
        "[a Sun] [A Sunday] [b Oct] [B October] [c 10/12/2025, 8:18:05 AM] [d 12] [D 10/12/25] [e 12] [H 08] [I 08] [m 10] [M 18] [O 151 days ago] [p AM] [P am] [r 08:18:05 AM] [R 08:18] [S 05] [T 08:18:05] [x 10/12/25] [X 8:18:05 AM] [y 25] [Y 2025] [z +0200] [Z GMT+02]"
    );
}
