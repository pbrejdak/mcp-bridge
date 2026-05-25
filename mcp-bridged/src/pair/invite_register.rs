//! In-memory tracker for active and recently-consumed pairing invites.
//!
//! Enforces the 60-second invite lifetime from
//! [`docs/SPEC.md`](../../../../docs/SPEC.md) §4.3 and the single-use nonce
//! rule from [`docs/SPEC.md`](../../../../docs/SPEC.md) §4.5 rule 9
//! (Direction B).
//!
//! Shape: an actor task owns the state. `InviteRegister` is a cheap
//! `Clone` handle that exchanges typed `Command`s over an mpsc channel —
//! the message-passing pattern preferred by [`mcp-bridged/CLAUDE.md`] §4
//! over `Arc<Mutex<…>>`.

use std::collections::HashMap;
use std::time::Duration;

use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant, interval};
use tokio_util::sync::CancellationToken;

use super::invite::Invite;
use super::nonce::Nonce;

/// Lifetime of an unconsumed invite per SPEC.md §4.3.
pub const INVITE_LIFETIME: Duration = Duration::from_secs(60);

/// How often the actor sweeps expired entries.
const GC_INTERVAL: Duration = Duration::from_secs(10);

/// Cap on in-flight commands. 64 is comfortably above the rate any human
/// pair flow can drive; if it ever saturates, something is wrong upstream.
const COMMAND_BUFFER: usize = 64;

/// Handle to the invite-register actor. Cheap to `Clone`.
#[derive(Debug, Clone)]
pub struct InviteRegister {
    tx: mpsc::Sender<Command>,
}

impl InviteRegister {
    /// Spawn the actor task and return a handle.
    #[must_use]
    pub fn spawn(cancel: CancellationToken) -> Self {
        let (tx, rx) = mpsc::channel(COMMAND_BUFFER);
        tokio::spawn(actor_loop(rx, cancel));
        Self { tx }
    }

    /// Record an active invite. Errors if its nonce is already in flight
    /// or was recently consumed.
    pub async fn register(&self, invite: Invite) -> Result<(), Error> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Command::Register { invite, reply })
            .await
            .map_err(|_| Error::ActorDown)?;
        rx.await.map_err(|_| Error::ActorDown)?
    }

    /// Consume an active invite by nonce. Errors if the nonce never
    /// matched an invite, has expired, or has already been consumed.
    pub async fn consume(&self, nonce: Nonce) -> Result<Invite, Error> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Command::Consume { nonce, reply })
            .await
            .map_err(|_| Error::ActorDown)?;
        rx.await.map_err(|_| Error::ActorDown)?
    }
}

/// Internal command wire — never crosses the crate boundary.
#[derive(Debug)]
enum Command {
    Register {
        invite: Invite,
        reply: oneshot::Sender<Result<(), Error>>,
    },
    Consume {
        nonce: Nonce,
        reply: oneshot::Sender<Result<Invite, Error>>,
    },
}

/// Failure modes for [`InviteRegister`] operations.
#[derive(Debug, Error)]
pub enum Error {
    #[error("an invite with this nonce is already pending or recently consumed")]
    AlreadyKnown,
    #[error("no active invite matches this nonce")]
    NoActiveInvite,
    #[error("this nonce was already consumed within the lifetime window")]
    AlreadyConsumed,
    #[error("the invite-register actor is no longer running")]
    ActorDown,
}

/// Actor state lives only inside this loop. No `Arc`, no `Mutex`.
async fn actor_loop(mut rx: mpsc::Receiver<Command>, cancel: CancellationToken) {
    let mut active: HashMap<Nonce, ActiveEntry> = HashMap::new();
    let mut consumed: HashMap<Nonce, Instant> = HashMap::new();
    let mut gc = interval(GC_INTERVAL);
    gc.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            () = cancel.cancelled() => break,
            cmd = rx.recv() => {
                match cmd {
                    Some(cmd) => handle(cmd, &mut active, &mut consumed),
                    None => break, // all handles dropped
                }
            }
            _ = gc.tick() => sweep_expired(&mut active, &mut consumed),
        }
    }
}

struct ActiveEntry {
    invite: Invite,
    deadline: Instant,
}

fn handle(
    cmd: Command,
    active: &mut HashMap<Nonce, ActiveEntry>,
    consumed: &mut HashMap<Nonce, Instant>,
) {
    match cmd {
        Command::Register { invite, reply } => {
            let nonce = invite.nonce;
            if active.contains_key(&nonce) || consumed.contains_key(&nonce) {
                let _ = reply.send(Err(Error::AlreadyKnown));
                return;
            }
            let deadline = Instant::now() + INVITE_LIFETIME;
            active.insert(nonce, ActiveEntry { invite, deadline });
            let _ = reply.send(Ok(()));
        }
        Command::Consume { nonce, reply } => {
            if consumed.contains_key(&nonce) {
                let _ = reply.send(Err(Error::AlreadyConsumed));
                return;
            }
            let result = match active.remove(&nonce) {
                Some(ActiveEntry { invite, deadline }) if Instant::now() <= deadline => {
                    consumed.insert(nonce, deadline);
                    Ok(invite)
                }
                _ => Err(Error::NoActiveInvite),
            };
            let _ = reply.send(result);
        }
    }
}

fn sweep_expired(active: &mut HashMap<Nonce, ActiveEntry>, consumed: &mut HashMap<Nonce, Instant>) {
    let now = Instant::now();
    active.retain(|_, entry| now <= entry.deadline);
    consumed.retain(|_, expiry| now <= *expiry);
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::identity::{DisplayName, Ed25519Pubkey};
    use crate::pair::lan_addr::LanAddr;

    fn make_invite(nonce_byte: u8) -> Invite {
        Invite::new(
            Ed25519Pubkey::from_bytes([0u8; 32]),
            DisplayName::new("test").unwrap(),
            LanAddr::new("https://10.0.0.5:8765/pair").unwrap(),
            Nonce::from_bytes([nonce_byte; 16]),
        )
    }

    #[tokio::test(start_paused = true)]
    async fn register_then_consume_returns_the_same_invite() {
        let cancel = CancellationToken::new();
        let reg = InviteRegister::spawn(cancel.clone());

        let invite = make_invite(1);
        let nonce = invite.nonce;
        reg.register(invite.clone()).await.unwrap();
        let back = reg.consume(nonce).await.unwrap();
        assert_eq!(back, invite);

        cancel.cancel();
    }

    #[tokio::test(start_paused = true)]
    async fn double_register_with_same_nonce_is_rejected() {
        let cancel = CancellationToken::new();
        let reg = InviteRegister::spawn(cancel.clone());

        let invite = make_invite(2);
        reg.register(invite.clone()).await.unwrap();
        let err = reg.register(invite).await.unwrap_err();
        assert!(matches!(err, Error::AlreadyKnown));

        cancel.cancel();
    }

    #[tokio::test(start_paused = true)]
    async fn consume_without_register_is_rejected() {
        let cancel = CancellationToken::new();
        let reg = InviteRegister::spawn(cancel.clone());

        let nonce = Nonce::from_bytes([9u8; 16]);
        let err = reg.consume(nonce).await.unwrap_err();
        assert!(matches!(err, Error::NoActiveInvite));

        cancel.cancel();
    }

    #[tokio::test(start_paused = true)]
    async fn double_consume_is_rejected() {
        let cancel = CancellationToken::new();
        let reg = InviteRegister::spawn(cancel.clone());

        let invite = make_invite(3);
        let nonce = invite.nonce;
        reg.register(invite).await.unwrap();
        reg.consume(nonce).await.unwrap();
        let err = reg.consume(nonce).await.unwrap_err();
        assert!(matches!(err, Error::AlreadyConsumed));

        cancel.cancel();
    }

    #[tokio::test(start_paused = true)]
    async fn invite_expires_after_60_seconds() {
        let cancel = CancellationToken::new();
        let reg = InviteRegister::spawn(cancel.clone());

        let invite = make_invite(4);
        let nonce = invite.nonce;
        reg.register(invite).await.unwrap();

        // Advance past the 60-second lifetime.
        tokio::time::advance(INVITE_LIFETIME + Duration::from_secs(1)).await;

        let err = reg.consume(nonce).await.unwrap_err();
        assert!(matches!(err, Error::NoActiveInvite));

        cancel.cancel();
    }

    #[tokio::test(start_paused = true)]
    async fn consume_just_inside_window_succeeds() {
        let cancel = CancellationToken::new();
        let reg = InviteRegister::spawn(cancel.clone());

        let invite = make_invite(5);
        let nonce = invite.nonce;
        reg.register(invite.clone()).await.unwrap();

        // Advance to just before the deadline.
        tokio::time::advance(INVITE_LIFETIME - Duration::from_millis(100)).await;

        let back = reg.consume(nonce).await.unwrap();
        assert_eq!(back, invite);

        cancel.cancel();
    }

    #[tokio::test(start_paused = true)]
    async fn cancel_token_stops_the_actor() {
        let cancel = CancellationToken::new();
        let reg = InviteRegister::spawn(cancel.clone());

        cancel.cancel();
        // Give the runtime a tick to observe the cancel.
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;

        let invite = make_invite(6);
        // The actor should be gone — the channel may be open but no one is
        // reading; eventually send fails or reply never arrives. We use a
        // short timeout to avoid hanging if behaviour regresses.
        let result = tokio::time::timeout(Duration::from_secs(1), reg.register(invite)).await;
        // The actor may have closed the channel cleanly, or the timeout
        // fires — either way is acceptable evidence the actor is down.
        // Either is acceptable evidence the actor is down: a canonical
        // ActorDown (send/recv failed) OR the timeout firing because no
        // reply ever came.
        match result {
            Ok(Err(Error::ActorDown)) | Err(_) => {}
            other => panic!("expected ActorDown or timeout, got {other:?}"),
        }
    }
}
