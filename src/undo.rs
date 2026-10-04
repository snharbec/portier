//! Taking back an action for a few seconds after it was done.
//!
//! An action changes what Portier shows at once, but what it does beyond that (moving mail on
//! the mail server, sending a mail) waits until the time to undo it has passed. Undoing puts
//! the shown state back and the waiting part never happens.

use std::{future::Future, time::Duration};

use crate::{
    crypto,
    error::{ApiError, ApiResult},
    mail::sync,
    models::Folder,
    state::AppState,
};

/// How long the clients offer Undo.
pub const WINDOW: Duration = Duration::from_secs(4);
/// The server waits a little longer, so an Undo pressed at the last moment still arrives.
const GRACE: Duration = Duration::from_secs(1);

/// One thing to put back.
pub enum Step {
    /// Messages and whether each was seen before.
    Seen(Vec<(i64, bool)>),
    /// Messages and whether each was flagged before.
    Flagged(Vec<(i64, bool)>),
    /// Conversations with the delay and the return moment they had before.
    Delay(Vec<(i64, Option<i64>, Option<i64>)>),
    /// Mail that was moved out of `source` here, though not yet on the mail server.
    Moved { source: Folder, rows: sync::Moved },
    /// A conversation and the note it had before.
    Note { thread: i64, text: String },
    /// A draft that is waiting to be sent.
    Draft(i64),
}

/// What an action can still take back, kept until its time is up.
pub struct Pending {
    user_id: i64,
    steps: Vec<Step>,
}

/// Keeps `steps` for the time an action can be undone and returns the token that undoes it.
/// `commit` is what the action does for good; it runs once that time has passed, unless the
/// action was undone.
pub fn hold<F>(state: &AppState, user_id: i64, steps: Vec<Step>, commit: F) -> String
where
    F: Future<Output = ()> + Send + 'static,
{
    let token = crypto::random_token();
    state
        .undo
        .lock()
        .expect("undo list")
        .insert(token.clone(), Pending { user_id, steps });
    let (state, key) = (state.clone(), token.clone());
    tokio::spawn(async move {
        // The tests do not wait four seconds for every action.
        let wait = if cfg!(test) {
            Duration::from_millis(300)
        } else {
            WINDOW + GRACE
        };
        tokio::time::sleep(wait).await;
        // Whoever takes the entry out decides: here the action stands, in `undo` it is taken back.
        let stands = state.undo.lock().expect("undo list").remove(&key).is_some();
        if stands {
            commit.await;
        }
    });
    token
}

/// Takes an action back. Returns the draft that was about to be sent, if that is what was undone.
pub async fn undo(state: &AppState, user_id: i64, token: &str) -> ApiResult<Option<i64>> {
    let pending = {
        let mut waiting = state.undo.lock().expect("undo list");
        match waiting.get(token) {
            Some(pending) if pending.user_id == user_id => waiting.remove(token),
            _ => None,
        }
    };
    let Some(pending) = pending else {
        return Err(ApiError(
            axum::http::StatusCode::GONE,
            "It is too late to undo this.".into(),
        ));
    };

    let mut draft = None;
    // Back in the opposite order: the last change first.
    for step in pending.steps.into_iter().rev() {
        match step {
            Step::Seen(rows) => {
                for (id, seen) in rows {
                    sqlx::query("UPDATE messages SET seen = ? WHERE id = ?")
                        .bind(seen)
                        .bind(id)
                        .execute(&state.db)
                        .await?;
                }
            }
            Step::Flagged(rows) => {
                for (id, flagged) in rows {
                    sqlx::query("UPDATE messages SET flagged = ? WHERE id = ?")
                        .bind(flagged)
                        .bind(id)
                        .execute(&state.db)
                        .await?;
                }
            }
            Step::Delay(rows) => {
                for (id, until, returned) in rows {
                    sqlx::query("UPDATE threads SET snoozed_until = ?, returned_at = ? WHERE id = ?")
                        .bind(until)
                        .bind(returned)
                        .bind(id)
                        .execute(&state.db)
                        .await?;
                }
            }
            Step::Moved { source, rows } => sync::restore_local(state, &source, rows).await?,
            Step::Note { thread, text } => {
                sqlx::query("UPDATE threads SET note = ? WHERE id = ?")
                    .bind(text)
                    .bind(thread)
                    .execute(&state.db)
                    .await?;
            }
            Step::Draft(id) => {
                sqlx::query("UPDATE drafts SET sending_at = NULL WHERE id = ?")
                    .bind(id)
                    .execute(&state.db)
                    .await?;
                draft = Some(id);
            }
        }
    }
    state.notify(user_id, "mail");
    Ok(draft)
}
