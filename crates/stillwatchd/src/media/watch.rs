//! The watch loop: follows players coming and going and their status
//! changes, and reports the playing set whenever it changes.

use std::collections::HashMap;

use futures_util::StreamExt as _;
use stillwatch_core::backend::{BackendError, EventSink};
use stillwatch_core::event::Event;
use zbus::zvariant::Value;
use zbus::{Connection, Message, MessageStream};

use super::Live;
use super::bus::{self, disconnected};
use super::players::PlayerSet;
use super::properties::{StatusChange, status_change};

/// Watches `conn` until the bus connection is lost.
pub(crate) async fn run(
    conn: &Connection,
    sink: &dyn EventSink,
    live: &Live,
) -> Result<(), BackendError> {
    // Subscribe before listing, so a player that changes while we list is
    // caught by a queued signal rather than missed.
    let mut owners = subscribe(conn, bus::owner_changes()).await?;
    let mut statuses = subscribe(conn, bus::status_changes()).await?;
    let mut players = PlayerSet::default();
    bus::discover(conn, &mut players).await?;
    publish(&mut players, sink, live);
    loop {
        tokio::select! {
            message = owners.next() => on_owner_changed(conn, &mut players, &next(message)?).await,
            message = statuses.next() => on_status_changed(conn, &mut players, &next(message)?).await,
            () = conn.closed() => {
                return Err(BackendError::Disconnected("session bus connection closed".into()));
            }
        }
        publish(&mut players, sink, live);
    }
}

async fn subscribe(
    conn: &Connection,
    rule: zbus::Result<zbus::MatchRule<'static>>,
) -> Result<MessageStream, BackendError> {
    let rule = rule.map_err(|err| BackendError::Protocol(err.to_string()))?;
    MessageStream::for_match_rule(rule, conn, None)
        .await
        .map_err(disconnected)
}

fn next(message: Option<zbus::Result<Message>>) -> Result<Message, BackendError> {
    match message {
        Some(Ok(message)) => Ok(message),
        Some(Err(err)) => Err(disconnected(err)),
        None => Err(BackendError::Disconnected("signal stream ended".into())),
    }
}

async fn on_owner_changed(conn: &Connection, players: &mut PlayerSet, message: &Message) {
    let Ok((name, _, owner)) = message.body().deserialize::<(String, String, String)>() else {
        return;
    };
    if owner.is_empty() {
        tracing::debug!(player = name, "MPRIS player left");
        players.remove(&name);
        return;
    }
    match bus::describe(conn, &name, &owner).await {
        Some(player) => {
            tracing::debug!(player = player.name, "MPRIS player appeared");
            players.insert(name, player);
        }
        None => players.remove(&name),
    }
}

async fn on_status_changed(conn: &Connection, players: &mut PlayerSet, message: &Message) {
    let header = message.header();
    let Some(owner) = header.sender().map(zbus::names::UniqueName::as_str) else {
        return;
    };
    if !players.owns(owner) {
        return;
    }
    let change = {
        let body = message.body();
        match body.deserialize::<(&str, HashMap<&str, Value<'_>>, Vec<&str>)>() {
            Ok((_, changed, invalidated)) => status_change(&changed, &invalidated),
            Err(_) => StatusChange::Unchanged,
        }
    };
    match change {
        StatusChange::Playing(playing) => players.set_playing(owner, playing),
        StatusChange::Invalidated => {
            let playing = bus::playing(conn, owner).await;
            players.set_playing(owner, playing);
        }
        StatusChange::Unchanged => {}
    }
}

fn publish(players: &mut PlayerSet, sink: &dyn EventSink, live: &Live) {
    live.set(Some(players.all()));
    if let Some(playing) = players.take_change() {
        let names: Vec<&str> = playing.iter().map(|player| player.name.as_str()).collect();
        tracing::debug!(players = ?names, "MPRIS playing set changed");
        sink.send(Event::Media { playing });
    }
}
