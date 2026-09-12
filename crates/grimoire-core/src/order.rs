//! The order lifecycle.
//!
//! There is no escrow in EverQuest and no moderator in this app, so the whole protocol is two
//! people taking turns saying what they just did. That makes *who* may advance a step the
//! important part — if either side can move the order on alone, the state stops being
//! evidence of anything.
//!
//! ```text
//!   OFFERED    buyer has asked            → crafter accepts or declines
//!   ACCEPTED   crafter said yes           → buyer sends coin
//!   PAID       buyer says coin is sent    → crafter confirms it arrived
//!   CONFIRMED  in his queue               → crafter starts work
//!   FORGE      at the forge               → crafter posts the goods
//!   DELIVERED  posted to you              → buyer confirms receipt
//!   COMPLETE   discharged                   both may rate
//! ```

use crate::regard::Regard;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Phase {
    Offered,
    Accepted,
    Paid,
    Confirmed,
    Forge,
    Delivered,
    Complete,
    /// Ended before completion. Carries who walked away.
    Cancelled(Party),
}

/// The two sides. There is no third.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Party {
    Buyer,
    Crafter,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Event {
    /// Crafter takes the job.
    Accept,
    /// Crafter turns it down.
    Decline,
    /// Buyer has sent the coin.
    CoinSent,
    /// Crafter has the coin; the job enters his queue.
    CoinReceived,
    /// Crafter has started.
    Started,
    /// Crafter has posted the goods.
    Posted,
    /// Buyer has them.
    Received,
    /// Either side walks away.
    Cancel(Party),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Refused {
    /// The event does not apply in this phase.
    OutOfOrder { phase: Phase, event: Event },
    /// Right event, wrong person.
    NotYours { needs: Party },
    /// The order is over.
    Closed,
    /// Money has changed hands; walking away is no longer unilateral.
    CoinInPlay,
}

impl Phase {
    /// The banner the UI shows for this phase.
    pub fn seal(self) -> &'static str {
        match self {
            Phase::Offered => "Offered",
            Phase::Accepted => "Awaiting coin",
            Phase::Paid => "Coin sent",
            Phase::Confirmed => "In his queue",
            Phase::Forge => "At the forge",
            Phase::Delivered => "Posted to you",
            Phase::Complete => "Discharged",
            Phase::Cancelled(_) => "Withdrawn",
        }
    }

    pub fn is_closed(self) -> bool {
        matches!(self, Phase::Complete | Phase::Cancelled(_))
    }

    /// Whose move it is, if anyone's.
    pub fn waiting_on(self) -> Option<Party> {
        Some(match self {
            Phase::Offered => Party::Crafter,
            Phase::Accepted => Party::Buyer,
            Phase::Paid => Party::Crafter,
            Phase::Confirmed => Party::Crafter,
            Phase::Forge => Party::Crafter,
            Phase::Delivered => Party::Buyer,
            Phase::Complete | Phase::Cancelled(_) => return None,
        })
    }

    /// Has the buyer's coin left his hands?
    ///
    /// Past this point neither side may simply cancel — someone is out of pocket, and the
    /// app has no way to claw it back, so it refuses to pretend otherwise.
    pub fn coin_in_play(self) -> bool {
        matches!(
            self,
            Phase::Paid | Phase::Confirmed | Phase::Forge | Phase::Delivered
        )
    }

    /// Apply an event, as `actor`.
    pub fn apply(self, actor: Party, event: Event) -> Result<Phase, Refused> {
        use Event::*;
        use Party::*;
        use Phase::*;

        if self.is_closed() {
            return Err(Refused::Closed);
        }

        // Cancellation is its own rule, and it is a narrow one.
        if let Cancel(by) = event {
            if by != actor {
                return Err(Refused::NotYours { needs: by });
            }
            if self.coin_in_play() {
                return Err(Refused::CoinInPlay);
            }
            return Ok(Cancelled(actor));
        }

        let (needs, next) = match (self, event) {
            (Offered, Accept) => (Crafter, Accepted),
            (Offered, Decline) => (Crafter, Cancelled(Crafter)),
            (Accepted, CoinSent) => (Buyer, Paid),
            (Paid, CoinReceived) => (Crafter, Confirmed),
            (Confirmed, Started) => (Crafter, Forge),
            (Forge, Posted) => (Crafter, Delivered),
            (Delivered, Received) => (Buyer, Complete),
            _ => return Err(Refused::OutOfOrder { phase: self, event }),
        };

        if actor != needs {
            return Err(Refused::NotYours { needs });
        }
        Ok(next)
    }
}

/// A crafter's terms, checked before an order may even be offered.
#[derive(Clone, Copy, PartialEq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Terms {
    pub open: bool,
    pub guild_only: bool,
    pub least_regard: Regard,
    /// Fraction off for guildmates.
    pub courtesy: f64,
}

impl Default for Terms {
    fn default() -> Self {
        Terms {
            open: true,
            guild_only: false,
            least_regard: Regard::DEFAULT_FLOOR,
            courtesy: 0.0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Barred {
    /// Different server. There is no cross-shard trade, so this is absolute.
    WrongServer,
    Shut,
    GuildOnly,
    /// Standing is below the crafter's floor.
    Regard {
        needs: Regard,
    },
}

/// May this buyer commission this crafter at all?
pub fn may_commission(
    terms: &Terms,
    same_server: bool,
    same_guild: bool,
    buyer: Regard,
) -> Result<(), Barred> {
    if !same_server {
        return Err(Barred::WrongServer);
    }
    if !terms.open {
        return Err(Barred::Shut);
    }
    if terms.guild_only && !same_guild {
        return Err(Barred::GuildOnly);
    }
    if !buyer.clears(terms.least_regard) {
        return Err(Barred::Regard {
            needs: terms.least_regard,
        });
    }
    Ok(())
}

/// Courtesy this buyer actually gets from this crafter.
pub fn courtesy_for(terms: &Terms, same_guild: bool) -> f64 {
    if same_guild {
        terms.courtesy.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Party::*;
    use Phase::*;

    fn walk() -> Phase {
        let steps = [
            (Crafter, Event::Accept),
            (Buyer, Event::CoinSent),
            (Crafter, Event::CoinReceived),
            (Crafter, Event::Started),
            (Crafter, Event::Posted),
            (Buyer, Event::Received),
        ];
        let mut p = Offered;
        for (who, e) in steps {
            p = p.apply(who, e).expect("happy path broke");
        }
        p
    }

    #[test]
    fn the_happy_path_completes() {
        assert_eq!(walk(), Complete);
    }

    #[test]
    fn a_closed_order_accepts_nothing_further() {
        assert_eq!(walk().apply(Buyer, Event::Received), Err(Refused::Closed));
        let dead = Offered.apply(Crafter, Event::Decline).unwrap();
        assert_eq!(dead, Cancelled(Crafter));
        assert_eq!(dead.apply(Crafter, Event::Accept), Err(Refused::Closed));
    }

    #[test]
    fn the_buyer_cannot_accept_his_own_order() {
        assert_eq!(
            Offered.apply(Buyer, Event::Accept),
            Err(Refused::NotYours { needs: Crafter })
        );
    }

    #[test]
    fn the_crafter_cannot_declare_that_he_was_paid() {
        // Only the buyer says the coin left. Otherwise a crafter can march an order to
        // Delivered on his own and the state stops being evidence of anything.
        assert_eq!(
            Accepted.apply(Crafter, Event::CoinSent),
            Err(Refused::NotYours { needs: Buyer })
        );
    }

    #[test]
    fn the_crafter_cannot_close_the_order_by_declaring_receipt() {
        assert_eq!(
            Delivered.apply(Crafter, Event::Received),
            Err(Refused::NotYours { needs: Buyer })
        );
    }

    #[test]
    fn steps_cannot_be_skipped() {
        assert!(matches!(
            Offered.apply(Crafter, Event::Posted),
            Err(Refused::OutOfOrder { .. })
        ));
        assert!(matches!(
            Accepted.apply(Crafter, Event::Started),
            Err(Refused::OutOfOrder { .. })
        ));
    }

    #[test]
    fn either_side_may_walk_away_before_money_moves() {
        assert_eq!(
            Offered.apply(Buyer, Event::Cancel(Buyer)),
            Ok(Cancelled(Buyer))
        );
        assert_eq!(
            Accepted.apply(Crafter, Event::Cancel(Crafter)),
            Ok(Cancelled(Crafter))
        );
    }

    #[test]
    fn nobody_walks_away_once_coin_has_moved() {
        for phase in [Paid, Confirmed, Forge, Delivered] {
            for who in [Buyer, Crafter] {
                assert_eq!(
                    phase.apply(who, Event::Cancel(who)),
                    Err(Refused::CoinInPlay),
                    "{phase:?} let {who:?} walk"
                );
            }
        }
    }

    #[test]
    fn you_cannot_cancel_on_someone_elses_behalf() {
        assert_eq!(
            Offered.apply(Buyer, Event::Cancel(Crafter)),
            Err(Refused::NotYours { needs: Crafter })
        );
    }

    #[test]
    fn every_live_phase_is_waiting_on_exactly_one_party() {
        for p in [Offered, Accepted, Paid, Confirmed, Forge, Delivered] {
            assert!(p.waiting_on().is_some(), "{p:?} is waiting on nobody");
        }
        assert!(Complete.waiting_on().is_none());
        assert!(Cancelled(Buyer).waiting_on().is_none());
    }

    #[test]
    fn the_party_being_waited_on_is_the_one_who_can_move() {
        // Any live phase must be advanceable by whoever it says it is waiting on.
        let events = [
            Event::Accept,
            Event::CoinSent,
            Event::CoinReceived,
            Event::Started,
            Event::Posted,
            Event::Received,
        ];
        for p in [Offered, Accepted, Paid, Confirmed, Forge, Delivered] {
            let who = p.waiting_on().unwrap();
            let moved = events.iter().any(|e| p.apply(who, *e).is_ok());
            assert!(
                moved,
                "{p:?} says it waits on {who:?} but they can do nothing"
            );
        }
    }

    #[test]
    fn a_shut_workshop_takes_no_orders() {
        let shut = Terms {
            open: false,
            ..Default::default()
        };
        assert_eq!(
            may_commission(&shut, true, true, Regard::Ally),
            Err(Barred::Shut)
        );
    }

    #[test]
    fn server_beats_everything_else() {
        // No cross-shard trade, so a different server is refused even for a guildmate Ally
        // of an open workshop.
        assert_eq!(
            may_commission(&Terms::default(), false, true, Regard::Ally),
            Err(Barred::WrongServer)
        );
    }

    #[test]
    fn guild_only_shuts_out_strangers_but_not_guildmates() {
        let t = Terms {
            guild_only: true,
            ..Default::default()
        };
        assert_eq!(
            may_commission(&t, true, false, Regard::Ally),
            Err(Barred::GuildOnly)
        );
        assert!(may_commission(&t, true, true, Regard::Indifferently).is_ok());
    }

    #[test]
    fn the_regard_floor_is_inclusive() {
        let t = Terms {
            least_regard: Regard::Kindly,
            ..Default::default()
        };
        assert!(may_commission(&t, true, false, Regard::Kindly).is_ok());
        assert_eq!(
            may_commission(&t, true, false, Regard::Amiably),
            Err(Barred::Regard {
                needs: Regard::Kindly
            })
        );
    }

    #[test]
    fn courtesy_only_applies_to_guildmates() {
        let t = Terms {
            courtesy: 0.15,
            ..Default::default()
        };
        assert_eq!(courtesy_for(&t, true), 0.15);
        assert_eq!(courtesy_for(&t, false), 0.0);
    }

    #[test]
    fn a_nonsense_courtesy_cannot_pay_the_buyer() {
        let t = Terms {
            courtesy: 4.0,
            ..Default::default()
        };
        assert_eq!(courtesy_for(&t, true), 1.0);
    }
}
