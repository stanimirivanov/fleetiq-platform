pub(super) fn status_lifecycle(value: &str) -> Option<&str> {
    value.split_once(',').map(|(status, _)| status.trim())
}

pub(super) fn legacy_status(value: &str) -> Option<&'static str> {
    let status = value
        .split_whitespace()
        .next()?
        .trim_end_matches([';', ',', '.']);
    if status.eq_ignore_ascii_case("superseded") {
        return Some("superseded");
    }
    ["proposed", "accepted", "rejected", "deprecated"]
        .into_iter()
        .find(|allowed| status.eq_ignore_ascii_case(allowed))
}

pub(super) fn valid_status_line(value: &str) -> bool {
    let Some((status, date)) = value.trim_end_matches('.').split_once(',') else {
        return false;
    };
    supported_status(status.trim()) && valid_long_date(date.trim())
}

pub(super) fn valid_iso_date(value: &str) -> bool {
    let parts: Vec<_> = value.split('-').collect();
    if parts.len() != 3 || parts[0].len() != 4 || parts[1].len() != 2 || parts[2].len() != 2 {
        return false;
    }
    let (Ok(year), Ok(month), Ok(day)) = (
        parts[0].parse::<u16>(),
        parts[1].parse::<u8>(),
        parts[2].parse::<u8>(),
    ) else {
        return false;
    };
    valid_date(year, month, day)
}

fn supported_status(value: &str) -> bool {
    [
        "proposed",
        "accepted",
        "rejected",
        "deprecated",
        "superseded",
    ]
    .iter()
    .any(|status| value.eq_ignore_ascii_case(status))
}

fn valid_long_date(value: &str) -> bool {
    let parts: Vec<_> = value.split_whitespace().collect();
    if parts.len() != 3 {
        return false;
    }
    let Ok(day) = parts[0].parse::<u8>() else {
        return false;
    };
    let Ok(year) = parts[2].parse::<u16>() else {
        return false;
    };
    let month = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ]
    .iter()
    .position(|candidate| *candidate == parts[1])
    .and_then(|index| u8::try_from(index + 1).ok());
    month.is_some_and(|month| valid_date(year, month, day))
}

fn valid_date(year: u16, month: u8, day: u8) -> bool {
    if year < 2000 || !(1..=12).contains(&month) {
        return false;
    }
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let maximum = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    (1..=maximum).contains(&day)
}
