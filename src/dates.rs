//! Civil dates (year, month, day of the Gregorian calendar) as days since 1970-01-01 and back, by Howard Hinnant's
//! algorithms, which count years from March so that a leap day ends the year.
//!
//! In: the dates in file names (a stats file's "2026.09.30-04.55.23"). Out: day counts for the mouse log's reader
//! (mouse.rs); the service's file names (service/src/library/names.rs) do the same sums.

/// Years in an era: the Gregorian calendar repeats every 400 years.
const YEARS_PER_ERA: i64 = 400;
/// The days in an era of YEARS_PER_ERA years.
const DAYS_PER_ERA: i64 = 146_097;
/// The days from 0000-03-01, where the eras count from, to 1970-01-01.
const DAYS_TO_UNIX_EPOCH: i64 = 719_468;

/// Days since 1970-01-01 of a civil date (Howard Hinnant's `days_from_civil`); month 1 to 12, day from 1.
pub fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(YEARS_PER_ERA);
    let year_of_era = year - era * YEARS_PER_ERA;
    // the days before the month, counted from March: its lengths follow (153 * month + 2) / 5
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * DAYS_PER_ERA + day_of_era - DAYS_TO_UNIX_EPOCH
}

/// The civil date (year, month 1 to 12, day from 1) of a day counted from 1970-01-01 (Howard Hinnant's
/// `civil_from_days`).
pub fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let from_march_0000 = days + DAYS_TO_UNIX_EPOCH;
    let era = from_march_0000.div_euclid(DAYS_PER_ERA);
    let day_of_era = from_march_0000 - era * DAYS_PER_ERA;
    // the leap days so far in the era taken out, so 365 days make each year
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 { month_from_march + 3 } else { month_from_march - 9 };
    (year_of_era + era * YEARS_PER_ERA + i64::from(month <= 2), month, day)
}

/// The days in a month (1 to 12) of a year of the Gregorian calendar.
pub fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Checks the day counts against known dates and each other.
#[cfg(test)]
mod tests {
    use super::*;

    /// Known dates give their day counts, and every day from 1600 to 2400 comes back as the date it was made from.
    #[test]
    fn days_and_dates_agree() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 1, 1), 10_957);
        assert_eq!(days_from_civil(2026, 9, 30), 20_726);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
        let mut days = days_from_civil(1600, 1, 1);
        for year in 1600..2400 {
            for month in 1..=12 {
                for day in 1..=days_in_month(year, month) {
                    assert_eq!(days_from_civil(year, month, day), days);
                    assert_eq!(civil_from_days(days), (year, month, day));
                    days += 1;
                }
            }
        }
    }
}
