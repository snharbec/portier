//! The search box language: free text plus `from:`, `to:`, `subject:` (or `title:`),
//! `attachment:` and `received:` filters.

use chrono::{Datelike, Days, Months, NaiveDate};

#[derive(Debug, Default, PartialEq)]
pub struct SearchQuery {
    /// Words and quoted phrases searched in subject, addresses and body.
    pub text: Vec<String>,
    pub from: Vec<String>,
    pub to: Vec<String>,
    pub subject: Vec<String>,
    pub attachment: Option<bool>,
    /// First and last day, both included. Either end may be open.
    pub received: Option<(Option<NaiveDate>, Option<NaiveDate>)>,
}

impl SearchQuery {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Free text as an FTS5 query of quoted prefix terms, so user input cannot be FTS syntax.
    pub fn fts(&self) -> String {
        self.text
            .iter()
            .map(|term| term.replace('"', ""))
            .filter(|term| !term.trim().is_empty())
            .map(|term| format!("\"{term}\"*"))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Splits on spaces, keeping `"quoted phrases"` and `key:"quoted values"` together.
fn tokens(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in input.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn date(text: &str) -> Option<NaiveDate> {
    let text = text.trim();
    // 01.09.2026 (day first), 2026/09/01 or 2026-09-01 (year first)
    let parts: Vec<&str> = text.split(['.', '/', '-']).collect();
    let [a, b, c] = parts.as_slice() else {
        return None;
    };
    let (a, b, c): (i32, u32, i32) = (a.parse().ok()?, b.parse().ok()?, c.parse().ok()?);
    if text.contains('.') {
        NaiveDate::from_ymd_opt(c, b, a as u32)
    } else {
        NaiveDate::from_ymd_opt(a, b, c as u32)
    }
}

/// A period named in words, or `first..last`, or one day.
fn period(value: &str, today: NaiveDate) -> Result<(Option<NaiveDate>, Option<NaiveDate>), String> {
    let month_start = |d: NaiveDate| d.with_day(1).expect("day 1 exists");
    let year_start = |d: NaiveDate| NaiveDate::from_ymd_opt(d.year(), 1, 1).expect("1 January exists");
    let week_start = today - Days::new(today.weekday().num_days_from_monday() as u64);
    let closed = |first: NaiveDate, last: NaiveDate| Ok((Some(first), Some(last)));

    match value
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .as_str()
    {
        "today" => closed(today, today),
        "yesterday" => closed(today - Days::new(1), today - Days::new(1)),
        "this week" => closed(week_start, today),
        "last week" => closed(week_start - Days::new(7), week_start - Days::new(1)),
        "this month" => closed(month_start(today), today),
        "last month" => {
            let first = month_start(today) - Months::new(1);
            closed(first, month_start(today) - Days::new(1))
        }
        "this year" => closed(year_start(today), today),
        "last year" => {
            let first = year_start(today) - Months::new(12);
            closed(first, year_start(today) - Days::new(1))
        }
        other => {
            let unreadable = || {
                format!(
                    "\"received:{value}\" is not a date or period. Try received:\"last month\", \
                     received:01.09.2026..01.10.2026 or received:2026/09/01..2026/10/01."
                )
            };
            let one = |text: &str| -> Result<Option<NaiveDate>, String> {
                if text.trim().is_empty() {
                    Ok(None)
                } else {
                    date(text).map(Some).ok_or_else(unreadable)
                }
            };
            match other.split_once("..") {
                Some((first, last)) => {
                    let range = (one(first)?, one(last)?);
                    if range == (None, None) {
                        Err(unreadable())
                    } else {
                        Ok(range)
                    }
                }
                None => {
                    let day = date(other).ok_or_else(unreadable)?;
                    closed(day, day)
                }
            }
        }
    }
}

/// Parses the search box. `today` is the user's current date, for words like "last month".
pub fn parse(input: &str, today: NaiveDate) -> Result<SearchQuery, String> {
    let mut query = SearchQuery::default();
    let mut tokens = tokens(input).into_iter().peekable();
    while let Some(token) = tokens.next() {
        let Some((key, value)) = token.split_once(':') else {
            query.text.push(token);
            continue;
        };
        let mut value = value.to_string();
        match key.to_lowercase().as_str() {
            "from" if !value.is_empty() => query.from.push(value),
            "to" if !value.is_empty() => query.to.push(value),
            "subject" | "title" if !value.is_empty() => query.subject.push(value),
            "attachment" | "attachments" => {
                query.attachment = Some(match value.to_lowercase().as_str() {
                    "true" | "yes" | "1" | "" => true,
                    "false" | "no" | "0" => false,
                    _ => {
                        return Err(format!(
                            "\"attachment:{value}\" must be attachment:true or attachment:false."
                        ));
                    }
                });
            }
            "received" => {
                // `received:last month` without quotes: the period's second word is the next token.
                let two_words = matches!(value.to_lowercase().as_str(), "last" | "this")
                    && tokens
                        .peek()
                        .is_some_and(|next| matches!(next.to_lowercase().as_str(), "week" | "month" | "year"));
                if two_words {
                    value = format!("{value} {}", tokens.next().expect("peeked"));
                }
                query.received = Some(period(&value, today)?);
            }
            // Not a filter (a URL, a time like 10:30, an unknown key): search for it as written.
            _ => query.text.push(token),
        }
    }
    Ok(query)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }
    // Saturday
    const TODAY: (i32, u32, u32) = (2026, 10, 3);
    fn q(input: &str) -> SearchQuery {
        parse(input, day(TODAY.0, TODAY.1, TODAY.2)).unwrap()
    }
    fn received(input: &str) -> (Option<NaiveDate>, Option<NaiveDate>) {
        q(input).received.unwrap()
    }

    #[test]
    fn text_and_sender() {
        let query = q("hallo from:carsten");
        assert_eq!(query.text, ["hallo"]);
        assert_eq!(query.from, ["carsten"]);
        assert_eq!(query.fts(), "\"hallo\"*");
    }

    #[test]
    fn subject_title_and_quotes() {
        assert_eq!(q("title:Rechnung").subject, ["Rechnung"]);
        assert_eq!(q("subject:Rechnung").subject, ["Rechnung"]);
        assert_eq!(
            q("SUBJECT:\"offene Rechnung\" from:\"Carsten Meier\"").subject,
            ["offene Rechnung"]
        );
        assert_eq!(q("from:\"Carsten Meier\"").from, ["Carsten Meier"]);
        assert_eq!(q("\"two words\" three").text, ["two words", "three"]);
        assert_eq!(q("to:anna@example.com").to, ["anna@example.com"]);
    }

    #[test]
    fn attachment_values() {
        assert_eq!(q("attachment:true").attachment, Some(true));
        assert_eq!(q("attachment:yes").attachment, Some(true));
        assert_eq!(q("attachment:false").attachment, Some(false));
        assert_eq!(q("invoice").attachment, None);
        assert!(parse("attachment:maybe", day(2026, 1, 1)).is_err());
    }

    #[test]
    fn received_periods_in_words() {
        assert_eq!(
            received("received:last month"),
            (Some(day(2026, 9, 1)), Some(day(2026, 9, 30)))
        );
        assert_eq!(
            received("received:\"last month\""),
            (Some(day(2026, 9, 1)), Some(day(2026, 9, 30)))
        );
        assert_eq!(
            received("received:this month"),
            (Some(day(2026, 10, 1)), Some(day(2026, 10, 3)))
        );
        assert_eq!(
            received("received:today"),
            (Some(day(2026, 10, 3)), Some(day(2026, 10, 3)))
        );
        assert_eq!(
            received("received:yesterday"),
            (Some(day(2026, 10, 2)), Some(day(2026, 10, 2)))
        );
        assert_eq!(
            received("received:this week"),
            (Some(day(2026, 9, 28)), Some(day(2026, 10, 3)))
        );
        assert_eq!(
            received("received:last week"),
            (Some(day(2026, 9, 21)), Some(day(2026, 9, 27)))
        );
        assert_eq!(
            received("received:last year"),
            (Some(day(2025, 1, 1)), Some(day(2025, 12, 31)))
        );
        // The words after the period are still part of the search.
        let query = q("received:last month hallo");
        assert_eq!(query.text, ["hallo"]);
        // January: last month is in the previous year.
        let january = parse("received:last month", day(2026, 1, 15))
            .unwrap()
            .received
            .unwrap();
        assert_eq!(january, (Some(day(2025, 12, 1)), Some(day(2025, 12, 31))));
    }

    #[test]
    fn received_date_ranges() {
        let september = (Some(day(2026, 9, 1)), Some(day(2026, 10, 1)));
        assert_eq!(received("received:01.09.2026..1.10.2026"), september);
        assert_eq!(received("received:2026/09/01..2026/10/01"), september);
        assert_eq!(received("received:2026-09-01..2026-10-01"), september);
        assert_eq!(
            received("received:15.09.2026"),
            (Some(day(2026, 9, 15)), Some(day(2026, 9, 15)))
        );
        assert_eq!(received("received:01.09.2026.."), (Some(day(2026, 9, 1)), None));
        assert_eq!(received("received:..2026/09/01"), (None, Some(day(2026, 9, 1))));
    }

    #[test]
    fn unreadable_received_is_an_error_that_shows_examples() {
        for bad in [
            "received:sometime",
            "received:31.02.2026",
            "received:..",
            "received:last",
        ] {
            let error = parse(bad, day(2026, 10, 3)).unwrap_err();
            assert!(error.contains("last month"), "{bad}: {error}");
        }
    }

    #[test]
    fn other_colons_are_plain_text() {
        assert_eq!(
            q("https://example.com 10:30 foo:bar").text,
            ["https://example.com", "10:30", "foo:bar"]
        );
        assert_eq!(q("from:").text, ["from:"]);
        assert_eq!(q("a\"b OR").fts(), "\"ab OR\"*");
        assert!(q("   ").is_empty());
        assert_eq!(q("\"\"").fts(), "");
    }
}
