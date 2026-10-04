//! Delayed conversations: out of the lists until a morning some days ahead, then back in the
//! inbox as unread.

use std::{collections::HashMap, time::Duration};

use anyhow::Result;
use chrono::{DateTime, Days, Local, NaiveTime, TimeZone};

use crate::{
    mail::{imap, sync},
    state::{AppState, now},
};

/// Delays that can be chosen, in days.
pub const CHOICES: [i64; 4] = [1, 2, 3, 7];
const RETURN_HOUR: u32 = 7;
const CHECK_EVERY: Duration = Duration::from_secs(60);

/// The moment a conversation delayed at `from` for `days` days returns: 7:00 local time on that day.
pub fn return_time<Tz: TimeZone>(from: DateTime<Tz>, days: i64) -> i64 {
    let day = from.date_naive() + Days::new(days.max(0) as u64);
    let morning = day.and_time(NaiveTime::from_hms_opt(RETURN_HOUR, 0, 0).expect("valid time"));
    // A local time that exists twice or not at all (clock change) still maps to one moment.
    from.timezone()
        .from_local_datetime(&morning)
        .earliest()
        .map(|t| t.timestamp())
        .unwrap_or_else(|| from.timestamp() + days * 86_400)
}

/// Brings back every conversation whose delay is over: clears the delay, marks its received mail
/// unread (locally and on the server) and remembers the moment, which sorts it to the top.
pub async fn wake_due(state: &AppState) -> Result<usize> {
    let due: Vec<(i64, i64)> =
        sqlx::query_as("SELECT id, user_id FROM threads WHERE snoozed_until IS NOT NULL AND snoozed_until <= ?")
            .bind(now())
            .fetch_all(&state.db)
            .await?;
    for (thread_id, user_id) in &due {
        sqlx::query("UPDATE threads SET snoozed_until = NULL, returned_at = ? WHERE id = ?")
            .bind(now())
            .bind(thread_id)
            .execute(&state.db)
            .await?;
        let received: Vec<(i64, i64, String, Option<u32>)> = sqlx::query_as(
            "SELECT m.id, m.account_id, f.name, m.uid FROM messages m JOIN folders f ON f.id = m.folder_id
             WHERE m.thread_id = ? AND m.is_outgoing = 0 AND f.role IN ('inbox', 'feed', 'delayed')",
        )
        .bind(thread_id)
        .fetch_all(&state.db)
        .await?;
        sqlx::query(
            "UPDATE messages SET seen = 0 WHERE thread_id = ? AND is_outgoing = 0
             AND folder_id IN (SELECT id FROM folders WHERE role IN ('inbox', 'feed', 'delayed'))",
        )
        .bind(thread_id)
        .execute(&state.db)
        .await?;

        // Where delayed mail waits in a folder of its own, the sync brings it back from there.
        let mut accounts: Vec<i64> = received.iter().map(|(_, account_id, _, _)| *account_id).collect();
        accounts.sort_unstable();
        accounts.dedup();
        for account_id in accounts {
            state.wake_sync(account_id).await;
        }

        let mut by_folder: HashMap<(i64, String), Vec<u32>> = HashMap::new();
        for (_, account_id, folder, uid) in received {
            if let Some(uid) = uid {
                by_folder.entry((account_id, folder)).or_default().push(uid);
            }
        }
        for ((account_id, folder), uids) in by_folder {
            sync::spawn_action(
                state,
                account_id,
                "marking returned mail unread",
                move |mut session| async move {
                    session.select(&folder).await?;
                    imap::set_seen(&mut session, &uids, false).await?;
                    let _ = session.logout().await;
                    Ok(())
                },
            );
        }
        state.notify(*user_id, "mail");
    }
    Ok(due.len())
}

/// Checks once a minute for delays that are over.
pub async fn run(state: AppState) {
    loop {
        if let Err(e) = wake_due(&state).await {
            tracing::warn!("returning delayed mail failed: {e:#}");
        }
        tokio::time::sleep(CHECK_EVERY).await;
    }
}

/// `return_time` from this moment in the server's time zone.
pub fn return_time_from_now(days: i64) -> i64 {
    return_time(Local::now(), days)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, Timelike};

    #[test]
    fn returns_at_seven_in_the_morning_of_that_day() {
        let berlin = FixedOffset::east_opt(2 * 3600).unwrap();
        // Saturday afternoon.
        let from = berlin.with_ymd_and_hms(2026, 10, 3, 16, 40, 0).unwrap();
        for (days, expected_day) in [(1, 4), (2, 5), (3, 6), (7, 10)] {
            let back = berlin.timestamp_opt(return_time(from, days), 0).unwrap();
            assert_eq!(
                (back.date_naive().to_string(), back.hour(), back.minute()),
                (format!("2026-10-{expected_day:02}"), 7, 0)
            );
        }
        // Delayed shortly after midnight: one day still means the next morning, not this one.
        let night = berlin.with_ymd_and_hms(2026, 10, 3, 0, 30, 0).unwrap();
        let back = berlin.timestamp_opt(return_time(night, 1), 0).unwrap();
        assert_eq!(back.date_naive().to_string(), "2026-10-04");
        // Across a month end.
        let end = berlin.with_ymd_and_hms(2026, 10, 30, 12, 0, 0).unwrap();
        let back = berlin.timestamp_opt(return_time(end, 3), 0).unwrap();
        assert_eq!(back.date_naive().to_string(), "2026-11-02");
    }
}
