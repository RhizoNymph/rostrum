//! Pairing codes: issued on the page, redeemed once over the API.
//!
//! A pure state machine over explicit instants, so every rule is a literal in
//! a test:
//!
//! - **Single use.** A redeemed code is gone.
//! - **Short-lived.** A code expires `ttl` after it is issued. An expired code
//!   is remembered for one more `ttl` so the phone is told "expired" rather
//!   than "wrong", and a retry with it is not counted as a guess.
//! - **Five strikes.** A failed attempt could be a guess at any outstanding
//!   code, so every failure strikes every live code; a code struck five times
//!   is burned. Forty random bits with five guesses each is not a search
//!   anyone finishes.
//! - **Per-address throttle.** Ten failures from one address (an IPv6 `/64`)
//!   within ten minutes, and that address is refused outright — even with a
//!   correct code — until the window slides.
//!
//! Every comparison runs over all outstanding codes without an early exit.

use std::{
    collections::{HashMap, VecDeque},
    net::IpAddr,
    time::Duration,
};

use rostrum_remote::PairingCode;
use tokio::time::Instant;

use crate::net::client::throttle_key;

/// Failed attempts a code survives.
pub const MAX_STRIKES: u8 = 5;
/// Failed attempts an address may make per [`FAILURE_WINDOW`].
pub const FAILURE_LIMIT: usize = 10;
pub const FAILURE_WINDOW: Duration = Duration::from_secs(10 * 60);
/// Codes outstanding at once; issuing another retires the oldest.
pub const MAX_OUTSTANDING: usize = 8;

#[derive(Debug)]
pub struct CodeBook {
    ttl: Duration,
    codes: Vec<Issued>,
    failures: HashMap<IpAddr, VecDeque<Instant>>,
}

#[derive(Debug)]
struct Issued {
    code: PairingCode,
    expires_at: Instant,
    state: CodeState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CodeState {
    Live { strikes: u8 },
    Burned,
}

/// Why a code was not accepted. The messages are shown on the phone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RedeemError {
    #[error("too many failed pairing attempts from this address; wait ten minutes and try again")]
    RateLimited,
    #[error("that pairing code is not valid; generate a new one on the computer")]
    Invalid,
    #[error("that pairing code has expired; generate a new one on the computer")]
    Expired,
    #[error(
        "that pairing code was disabled after too many wrong attempts; generate a new one on the computer"
    )]
    Burned,
}

impl CodeBook {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            codes: Vec::new(),
            failures: HashMap::new(),
        }
    }

    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    /// Record a freshly minted code; returns when it expires.
    pub fn issue(&mut self, code: PairingCode, now: Instant) -> Instant {
        self.prune(now);
        self.codes.retain(|issued| !issued.code.ct_eq(&code));
        while self.codes.len() >= MAX_OUTSTANDING {
            let oldest = self
                .codes
                .iter()
                .enumerate()
                .min_by_key(|(_, issued)| issued.expires_at)
                .map(|(ix, _)| ix);
            match oldest {
                Some(ix) => {
                    self.codes.remove(ix);
                }
                None => break,
            }
        }
        let expires_at = now + self.ttl;
        self.codes.push(Issued {
            code,
            expires_at,
            state: CodeState::Live { strikes: 0 },
        });
        expires_at
    }

    /// Try a code from `from`. `Ok` consumes it.
    pub fn redeem(
        &mut self,
        attempt: &PairingCode,
        from: IpAddr,
        now: Instant,
    ) -> Result<(), RedeemError> {
        self.prune(now);
        let key = throttle_key(from);
        if self
            .failures
            .get(&key)
            .is_some_and(|failures| failures.len() >= FAILURE_LIMIT)
        {
            return Err(RedeemError::RateLimited);
        }

        let mut found = None;
        for (ix, issued) in self.codes.iter().enumerate() {
            if issued.code.ct_eq(attempt) {
                found = Some(ix);
            }
        }

        match found {
            Some(ix) if self.codes[ix].expires_at <= now => Err(RedeemError::Expired),
            Some(ix) => match self.codes[ix].state {
                CodeState::Burned => Err(RedeemError::Burned),
                CodeState::Live { .. } => {
                    self.codes.remove(ix);
                    Ok(())
                }
            },
            None => {
                self.record_failure(key, now);
                Err(RedeemError::Invalid)
            }
        }
    }

    /// Codes that can still be redeemed.
    pub fn live(&self, now: Instant) -> usize {
        self.codes
            .iter()
            .filter(|issued| {
                issued.expires_at > now && matches!(issued.state, CodeState::Live { .. })
            })
            .count()
    }

    fn record_failure(&mut self, key: IpAddr, now: Instant) {
        self.failures.entry(key).or_default().push_back(now);
        for issued in &mut self.codes {
            if issued.expires_at <= now {
                continue;
            }
            if let CodeState::Live { strikes } = issued.state {
                let strikes = strikes.saturating_add(1);
                issued.state = if strikes >= MAX_STRIKES {
                    CodeState::Burned
                } else {
                    CodeState::Live { strikes }
                };
            }
        }
    }

    fn prune(&mut self, now: Instant) {
        let ttl = self.ttl;
        self.codes.retain(|issued| issued.expires_at + ttl > now);
        for failures in self.failures.values_mut() {
            while failures
                .front()
                .is_some_and(|at| now.duration_since(*at) >= FAILURE_WINDOW)
            {
                failures.pop_front();
            }
        }
        self.failures.retain(|_, failures| !failures.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TTL: Duration = Duration::from_secs(300);

    fn code(text: &str) -> PairingCode {
        PairingCode::parse(text).expect("code")
    }

    fn ip(text: &str) -> IpAddr {
        text.parse().expect("ip")
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn a_code_is_accepted_once() {
        let t0 = Instant::now();
        let mut book = CodeBook::new(TTL);
        assert_eq!(book.issue(code("AAAA-BBBB"), t0), t0 + TTL);
        assert_eq!(book.live(t0), 1);
        assert_eq!(
            book.redeem(&code("AAAA-BBBB"), ip("192.168.0.5"), t0),
            Ok(())
        );
        assert_eq!(book.live(t0), 0);
        assert_eq!(
            book.redeem(&code("AAAA-BBBB"), ip("192.168.0.5"), t0),
            Err(RedeemError::Invalid)
        );
    }

    #[test]
    fn a_code_expires_after_its_ttl_and_says_so() {
        let t0 = Instant::now();
        let mut book = CodeBook::new(TTL);
        book.issue(code("AAAA-BBBB"), t0);
        assert_eq!(
            book.redeem(&code("AAAA-BBBB"), ip("10.0.0.2"), t0 + TTL),
            Err(RedeemError::Expired)
        );
        // Retrying an expired code is not a guess: it neither burns other
        // codes nor counts toward the throttle.
        for _ in 0..20 {
            assert_eq!(
                book.redeem(&code("AAAA-BBBB"), ip("10.0.0.2"), t0 + TTL + secs(1)),
                Err(RedeemError::Expired)
            );
        }
        // Long after, it is simply unknown.
        assert_eq!(
            book.redeem(&code("AAAA-BBBB"), ip("10.0.0.2"), t0 + TTL * 2 + secs(1)),
            Err(RedeemError::Invalid)
        );
    }

    #[test]
    fn a_code_is_valid_until_the_instant_it_expires() {
        let t0 = Instant::now();
        let mut book = CodeBook::new(TTL);
        book.issue(code("AAAA-BBBB"), t0);
        assert_eq!(
            book.redeem(&code("AAAA-BBBB"), ip("10.0.0.2"), t0 + TTL - secs(1)),
            Ok(())
        );
    }

    #[test]
    fn five_wrong_guesses_burn_every_live_code() {
        let t0 = Instant::now();
        let mut book = CodeBook::new(TTL);
        book.issue(code("AAAA-BBBB"), t0);
        book.issue(code("CCCC-DDDD"), t0);
        for n in 0..MAX_STRIKES {
            // From different addresses, so the throttle is not what stops it.
            let from = ip(&format!("192.168.1.{n}"));
            assert_eq!(
                book.redeem(&code("ZZZZ-ZZZZ"), from, t0),
                Err(RedeemError::Invalid)
            );
        }
        assert_eq!(book.live(t0), 0);
        assert_eq!(
            book.redeem(&code("AAAA-BBBB"), ip("192.168.0.5"), t0),
            Err(RedeemError::Burned)
        );
        assert_eq!(
            book.redeem(&code("CCCC-DDDD"), ip("192.168.0.5"), t0),
            Err(RedeemError::Burned)
        );
    }

    #[test]
    fn four_wrong_guesses_leave_the_code_usable() {
        let t0 = Instant::now();
        let mut book = CodeBook::new(TTL);
        book.issue(code("AAAA-BBBB"), t0);
        for _ in 0..MAX_STRIKES - 1 {
            let _ = book.redeem(&code("ZZZZ-ZZZZ"), ip("192.168.1.9"), t0);
        }
        assert_eq!(
            book.redeem(&code("AAAA-BBBB"), ip("192.168.1.9"), t0),
            Ok(())
        );
    }

    #[test]
    fn a_code_issued_after_the_guessing_starts_has_its_own_strikes() {
        let t0 = Instant::now();
        let mut book = CodeBook::new(TTL);
        book.issue(code("AAAA-BBBB"), t0);
        for n in 0..MAX_STRIKES {
            let _ = book.redeem(&code("ZZZZ-ZZZZ"), ip(&format!("10.1.0.{n}")), t0);
        }
        book.issue(code("CCCC-DDDD"), t0 + secs(1));
        assert_eq!(
            book.redeem(&code("CCCC-DDDD"), ip("10.2.0.1"), t0 + secs(2)),
            Ok(())
        );
    }

    #[test]
    fn ten_failures_from_one_address_are_throttled_even_with_the_right_code() {
        let t0 = Instant::now();
        let mut book = CodeBook::new(TTL);
        let attacker = ip("192.168.0.66");
        for n in 0..FAILURE_LIMIT {
            // Keep a fresh code each round so burning is not what we observe.
            book.issue(PairingCode::from_entropy([n as u8, 1, 2, 3, 4]), t0);
            assert_eq!(
                book.redeem(&code("ZZZZ-ZZZZ"), attacker, t0),
                Err(RedeemError::Invalid)
            );
        }
        book.issue(code("AAAA-BBBB"), t0);
        assert_eq!(
            book.redeem(&code("AAAA-BBBB"), attacker, t0),
            Err(RedeemError::RateLimited)
        );
        // Another address is unaffected, and the code is still there for it.
        assert_eq!(
            book.redeem(&code("AAAA-BBBB"), ip("192.168.0.7"), t0),
            Ok(())
        );
    }

    #[test]
    fn the_throttle_window_slides() {
        let t0 = Instant::now();
        let mut book = CodeBook::new(Duration::from_secs(3600));
        let from = ip("192.168.0.66");
        for _ in 0..FAILURE_LIMIT {
            let _ = book.redeem(&code("ZZZZ-ZZZZ"), from, t0);
        }
        book.issue(code("AAAA-BBBB"), t0 + secs(60));
        assert_eq!(
            book.redeem(&code("AAAA-BBBB"), from, t0 + secs(60)),
            Err(RedeemError::RateLimited)
        );
        assert_eq!(
            book.redeem(&code("AAAA-BBBB"), from, t0 + FAILURE_WINDOW),
            Ok(())
        );
    }

    #[test]
    fn the_throttle_counts_an_ipv6_slash_64_as_one_address() {
        let t0 = Instant::now();
        let mut book = CodeBook::new(TTL);
        for n in 0..FAILURE_LIMIT {
            let from = ip(&format!("2001:db8:0:1::{n:x}"));
            let _ = book.redeem(&code("ZZZZ-ZZZZ"), from, t0);
        }
        book.issue(code("AAAA-BBBB"), t0);
        assert_eq!(
            book.redeem(&code("AAAA-BBBB"), ip("2001:db8:0:1::ffff"), t0),
            Err(RedeemError::RateLimited)
        );
    }

    #[test]
    fn issuing_beyond_the_limit_retires_the_oldest() {
        let t0 = Instant::now();
        let mut book = CodeBook::new(TTL);
        let first = PairingCode::from_entropy([0, 0, 0, 0, 1]);
        book.issue(first.clone(), t0);
        for n in 1..=MAX_OUTSTANDING {
            book.issue(
                PairingCode::from_entropy([0, 0, 0, 1, n as u8]),
                t0 + secs(n as u64),
            );
        }
        assert_eq!(book.live(t0 + secs(10)), MAX_OUTSTANDING);
        assert_eq!(
            book.redeem(&first, ip("10.0.0.1"), t0 + secs(10)),
            Err(RedeemError::Invalid)
        );
    }
}
