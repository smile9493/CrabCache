/// Possible downstream states during request multiplexing
#[derive(Debug, Clone, Copy)]
pub(crate) enum DownstreamStateMachine {
    /// more request (body) to read
    Reading,
    /// no more data to read
    ReadingFinished,
    /// downstream is already errored or closed
    Errored,
}

#[allow(clippy::wrong_self_convention)]
impl DownstreamStateMachine {
    pub fn new(finished: bool) -> Self {
        if finished {
            Self::ReadingFinished
        } else {
            Self::Reading
        }
    }

    // Can call read() to read more data or wait on closing
    pub fn can_poll(&self) -> bool {
        !matches!(self, Self::Errored)
    }

    pub fn is_reading(&self) -> bool {
        matches!(self, Self::Reading)
    }

    pub fn is_done(&self) -> bool {
        !matches!(self, Self::Reading)
    }

    pub fn is_errored(&self) -> bool {
        matches!(self, Self::Errored)
    }

    /// Move the state machine to Finished state if `set` is true
    pub fn maybe_finished(&mut self, set: bool) {
        if set {
            *self = Self::ReadingFinished
        }
    }

    /// Reset if we should continue reading from the downstream again.
    /// Only used with upgraded connections when body mode changes.
    pub fn reset(&mut self) {
        *self = Self::Reading;
    }

    pub fn to_errored(&mut self) {
        *self = Self::Errored
    }
}

/// Possible upstream states during request multiplexing
#[derive(Debug, Clone, Copy)]
pub(crate) struct ResponseStateMachine {
    upstream_response_done: bool,
    cached_response_done: bool,
}

impl ResponseStateMachine {
    pub fn new() -> Self {
        ResponseStateMachine {
            upstream_response_done: false,
            cached_response_done: true, // no cached response by default
        }
    }

    pub fn is_done(&self) -> bool {
        self.upstream_response_done && self.cached_response_done
    }

    pub fn upstream_done(&self) -> bool {
        self.upstream_response_done
    }

    pub fn cached_done(&self) -> bool {
        self.cached_response_done
    }

    pub fn enable_cached_response(&mut self) {
        self.cached_response_done = false;
    }

    pub fn maybe_set_upstream_done(&mut self, done: bool) {
        if done {
            self.upstream_response_done = true;
        }
    }

    pub fn maybe_set_cache_done(&mut self, done: bool) {
        if done {
            self.cached_response_done = true;
        }
    }
}

use crate::Session;
use crate::ProxyHttp;
use log::debug;
use pingora_core::prelude::*;
use std::time::Duration;
use tokio::time::{MissedTickBehavior, interval};

/// Flush Responses wire bootstrap immediately after upstream headers (Codex prefill keepalive).
pub(crate) async fn try_initial_downstream_response_body<SV>(
    inner: &SV,
    session: &mut Session,
    ctx: &mut SV::CTX,
) -> Result<()>
where
    SV: ProxyHttp + Send + Sync,
    SV::CTX: Send + Sync,
{
    if session.response_written().is_none() {
        return Ok(());
    }
    if let Some(bytes) = inner.initial_downstream_response_body(session, ctx).await? {
        if session.write_response_body(Some(bytes), false).await.is_ok() {
            debug!("early Responses wire bootstrap written after upstream headers");
        }
    }
    Ok(())
}

/// Synthesize Responses completion as soon as the upstream body channel closes.
pub(crate) async fn try_graceful_upstream_finalize<SV>(
    inner: &SV,
    session: &mut Session,
    ctx: &mut SV::CTX,
) -> Result<()>
where
    SV: ProxyHttp + Send + Sync,
    SV::CTX: Send + Sync,
{
    if session.response_written().is_none() {
        return Ok(());
    }
    if let Some(bytes) = inner.finalize_aborted_upstream_stream(session, ctx).await? {
        if session.write_response_body(Some(bytes), true).await.is_ok() {
            debug!("graceful Responses stream tail written on upstream close");
        }
    }
    Ok(())
}

/// Push Responses wire heartbeats while upstream stalls between chunks.
pub(crate) async fn try_poll_downstream_keepalive<SV>(
    inner: &SV,
    session: &mut Session,
    ctx: &mut SV::CTX,
) -> Result<()>
where
    SV: ProxyHttp + Send + Sync,
    SV::CTX: Send + Sync,
{
    if session.response_written().is_none() {
        return Ok(());
    }
    if let Some(bytes) = inner.poll_downstream_stream_keepalive(session, ctx).await? {
        if session.write_response_body(Some(bytes), false).await.is_ok() {
            debug!("Responses wire keepalive written to downstream");
        }
    }
    Ok(())
}

/// Interval for [`try_poll_downstream_keepalive`] in the upstream/downstream duplex loop.
pub(crate) fn downstream_stream_keepalive_interval() -> tokio::time::Interval {
    let mut tick = interval(Duration::from_secs(5));
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    tick
}
