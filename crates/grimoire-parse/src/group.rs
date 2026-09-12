//! Who is in the reader's group, read off the lines the game prints about it.
//!
//! THE GRAMMAR WAS MEASURED OVER ALL FOUR OF THE OWNER'S LOGS (440 MB, stamps stripped), not
//! guessed. `<Name>` is always one word of letters in every form below:
//!
//! ```text
//!   147  <Name> has joined the group.
//!   140  You have joined the group.
//!   133  You have been removed from the group.
//!   131  <Name> has left the group.
//!   183  You invite <Name> to join your group.              155 capitalised, 28 typed in lower case
//!    83  You notify <Name> that you agree to join the group.
//!    92  You are now the leader of your group.
//!    62  <Name> is now the leader of your group.
//!    95  You are not in a group. Talking to yourself again?
//!     7  You are not in a group!  Keep it all.
//!    34  <Name> is currently considering joining another group.
//!    13  <Name> rejects your offer to join the group.
//!     2  Player <name> was not found.                       both straight after an invite
//!  5263  <Name> tells the group, '...'
//!  4694  You tell your party, '...'
//!  8672  You gain party experience! (1.268%)                and `(with a bonus)!`
//!   736  <Name> tells the raid, '...'
//!   177  You tell your raid, '...'
//!    39  You receive no experience for defeating this creature as you are in a raid.
//!    12  You receive no loot for defeating this creature as you are in a raid.
//!    95  Welcome to EverQuest Legends!
//!     8  To invite another group into yours, please invite the leader of the other group.
//!                                                        each 0 to 2 seconds after `You invite`
//! ```
//!
//! THE READER'S OWN PETS ARE READ TOO, off the answers a pet gives its owner. `<Name>` is one word
//! here as well, and no pet is ever put in the group: see [`Party::pets_during`] for why a filter
//! that knows the group still needs them.
//!
//! ```text
//!   692  <Name> says, 'Sorry, Master... calming down.'
//!   480  <Name> told you, 'Attacking <mob> Master.'
//!   339  <Name> told you, 'I am unable to wake <mob>, Master.'
//!    45  <Name> says, 'I beg forgiveness, Master.  That is not a legal target.'
//!    27  <Name> says, 'Following you, Master.'
//!    10  <Name> says, 'Guarding with my life, oh splendid one.'
//!     4  <Name> says, 'Now regrouping, master.'
//! ```
//!
//! READ AND DELIBERATELY DROPPED, each for a measured reason:
//!
//! * `<Name> has been removed from Nagafen's Lair - Group.` and `has been added to` (95 lines over
//!   seventeen zone names, some ending `- Solo.`). That is an expedition's list, not the group, and
//!   nothing here matches on a sentence ending: every form above is matched whole.
//! * `<Name> is now group Main Assist` and `is no longer group Main Tank` (37 lines). A role handoff,
//!   printed in pairs on one second, and 10 of the 37 name the OWNER in the third person
//!   (`Reviir is now group Main Assist`). This crate does not know the owner's name, so reading
//!   these as member evidence would put the reader in his own group.
//! * `You remove <Name> from the party.` (19 lines). 16 are followed within a second by
//!   `<Name> has left the group.`, which is the line acted on. Acting on the kick itself would drop a
//!   member on a kick that did not take, and dropping a member who is still there hides him.
//! * `<Name> invites you to join a group.` (105). Accepting prints `You notify`, so the invite alone
//!   changes nothing.
//! * ZERO lines of any form say the reader joined or left a raid. Raid membership is only ever
//!   evidence, never an event, and [`Party`] treats it that way.

use crate::fights::seconds;
use crate::line::split_stamp;

/// One thing a line says about the reader's group or raid.
///
/// Names are borrowed exactly as the line spelled them. An invite carries the name as the reader
/// TYPED it (`You invite flagg`), which is why every comparison in [`Party`] ignores case and every
/// name [`Party`] reports comes from a line the game wrote.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Change<'a> {
    /// `You have joined the group.`
    YouJoined,
    /// `You have been removed from the group.` Leaving, being kicked and a disband all print it.
    YouLeft,
    /// `You are now the leader of your group.`
    YouLead,
    /// `You notify Hert that you agree to join the group.` Hert is the one who invited.
    YouAccepted(&'a str),
    /// `You invite Zarmin to join your group.`
    YouInvited(&'a str),
    /// An invite that will not bring anybody in: `rejects your offer`, `is currently considering
    /// joining another group`, or `Player me was not found.`
    Declined(&'a str),
    /// `Zarmin has joined the group.`
    Joined(&'a str),
    /// `Zarmin has left the group.`
    Left(&'a str),
    /// `Eldoth is now the leader of your group.`
    Leads(&'a str),
    /// `Flagg tells the group, '...'`. Only a member can speak there.
    Spoke(&'a str),
    /// `You tell your party, '...'` or `You gain party experience!`. Proof the reader is grouped,
    /// with nobody named. Not grouped, the game answers the first with `You are not in a group.
    /// Talking to yourself again?` instead.
    YouGrouped,
    /// `You are not in a group. Talking to yourself again?` or `You are not in a group!  Keep it
    /// all.` Proof the reader is alone at that second.
    NotGrouped,
    /// Raid chat, or the raid's refusal of experience or loot.
    Raid,
    /// `Welcome to EverQuest Legends!`, printed once per login.
    Login,
    /// `To invite another group into yours, please invite the leader of the other group.` The game
    /// turning down the reader's own invite because the invitee is in a group already. It names
    /// nobody, so [`Party`] can only answer it against an invite it cannot belong to anything but.
    Refused,
}

/// Read one line body, stamp already stripped, into what it says about the group.
///
/// `None` for everything else, which is nearly every line. Every form is matched whole or by a
/// fixed prefix plus a one-word name, so chat cannot forge one: `Bada tells you, 'Hert has joined
/// the group.'` has no one-word name in front of `has joined the group.`
pub fn read(body: &str) -> Option<Change<'_>> {
    match body {
        "You have joined the group." => return Some(Change::YouJoined),
        "You have been removed from the group." => return Some(Change::YouLeft),
        "You are now the leader of your group." => return Some(Change::YouLead),
        "You are not in a group. Talking to yourself again?"
        | "You are not in a group!  Keep it all." => return Some(Change::NotGrouped),
        "You receive no experience for defeating this creature as you are in a raid."
        | "You receive no loot for defeating this creature as you are in a raid." => {
            return Some(Change::Raid)
        }
        "Welcome to EverQuest Legends!" => return Some(Change::Login),
        "To invite another group into yours, please invite the leader of the other group." => {
            return Some(Change::Refused)
        }
        _ => {}
    }
    if body.starts_with("You gain party experience") || body.starts_with("You tell your party, '") {
        return Some(Change::YouGrouped);
    }
    if body.starts_with("You tell your raid, '") {
        return Some(Change::Raid);
    }
    if let Some(rest) = body.strip_prefix("You invite ") {
        return one_name(rest.strip_suffix(" to join your group.")?).map(Change::YouInvited);
    }
    if let Some(rest) = body.strip_prefix("You notify ") {
        return one_name(rest.strip_suffix(" that you agree to join the group.")?)
            .map(Change::YouAccepted);
    }
    if let Some(name) = body
        .strip_prefix("Player ")
        .and_then(|rest| rest.strip_suffix(" was not found."))
        .and_then(one_name)
    {
        return Some(Change::Declined(name));
    }
    let (name, rest) = body.split_once(' ')?;
    let name = one_name(name)?;
    Some(match rest {
        "has joined the group." => Change::Joined(name),
        "has left the group." => Change::Left(name),
        "is now the leader of your group." => Change::Leads(name),
        "rejects your offer to join the group."
        | "is currently considering joining another group." => Change::Declined(name),
        _ if rest.starts_with("tells the group, '") => Change::Spoke(name),
        _ if rest.starts_with("tells the raid, '") => Change::Raid,
        _ => return None,
    })
}

/// A player name: one word, letters only. Pets (``Lumpy`s warder``) and NPCs with a space in their
/// name fail here, which is what keeps them out of a group they cannot be in.
fn one_name(s: &str) -> Option<&str> {
    (!s.is_empty() && s.bytes().all(|b| b.is_ascii_alphabetic())).then_some(s)
}

/// THE NAME OF THE READER'S OWN PET, when a line body is one of the seven answers a pet gives its
/// owner (the table in the module doc), and `None` for everything else.
///
/// # WHY THESE LINES ARE THE OWNER'S AND NOBODY ELSE'S, MEASURED
///
/// The two `told you` forms are tells, addressed to the reader. The `says` forms are not
/// addressed, so they were checked against everything that marks a player: across the four logs
/// fifty one-word names give one of the seven answers, and not one of the fifty was ever
/// `Targeted (Player)` or spoke on a chat channel. Eight of them answered with a `says` form only.
/// Two of those eight were checked line by line and both are the reader's: `Jebobab` was
/// `Targeted (NPC)` twenty seconds after `You begin casting Bone Walk.`, and `Gabtik` is the pet
/// the reader fought beside, known solo, on Jul 18.
///
/// NOT A GROUP LINE, AND KEPT OUT OF [`Change`] ON PURPOSE. A pet's answer says nothing about who is
/// in the group, and a pet speaks in the middle of every pull, so as a `Change` it would sit
/// between `You have joined the group.` and `You are now the leader of your group.` on one second
/// and break the formation rule, which reads the line straight after the join.
///
/// WHERE IT CAN BE WRONG, SAID OUT LOUD: a player who types `/say Following you, Master.` beside
/// the reader passes for his pet until the next login. That shows a stranger, which is the side a
/// roster errs on when it does not know; it never hides anybody.
fn pet_of(body: &str) -> Option<&str> {
    let (name, rest) = body.split_once(' ')?;
    let name = one_name(name)?;
    let answered = match rest {
        "says, 'Sorry, Master... calming down.'"
        | "says, 'Following you, Master.'"
        | "says, 'I beg forgiveness, Master.  That is not a legal target.'"
        | "says, 'Guarding with my life, oh splendid one.'"
        | "says, 'Now regrouping, master.'" => true,
        _ => rest
            .strip_prefix("told you, '")
            .and_then(|q| q.strip_suffix("Master.'"))
            .is_some_and(|q| {
                (q.starts_with("Attacking ") && q.ends_with(' '))
                    || (q.starts_with("I am unable to wake ") && q.ends_with(", "))
            }),
    };
    answered.then_some(name)
}

/// How far behind a nameless refusal the invite it answers can be. Measured: all eight refusals
/// in the four logs came 0 to 2 seconds after a `You invite`, and the widest was Berkshire's,
/// invited at Jul 30 23:29:22 and refused at 23:29:24.
const REFUSAL_SECONDS: i64 = 2;

/// HOW FAR BEHIND THE READER'S OWN CHARM CAST ITS LANDING LINE CAN BE.
///
/// `X has been charmed.` is printed for ANYBODY'S charm in range, not only the reader's: of the 270 in
/// the owner's Sep 11 log only 45 followed a charm spell he cast. Those 45 landed 0 to 2 seconds after
/// `You begin casting Charm..` (one at 0, twenty at 1, twenty four at 2), and nothing of his landed
/// between 3 and 15. So the landing is his only inside that window, measured the way
/// [`REFUSAL_SECONDS`] was.
const CHARM_SECONDS: i64 = 2;

/// IS THIS ONE OF THE READER'S CHARM SPELLS? The three he cast in the Sep 11 log: `Charm`, `Charm IV`
/// and `Charm VII`, so the family is `Charm` with or without a rank in roman numerals. Not
/// `Charm Animals` or any other class's charm: nothing in a log this app has measured casts one, and
/// a spell guessed into this list is somebody else's damage in the reader's row.
fn charm_spell(spell: &str) -> bool {
    spell == "Charm"
        || spell.strip_prefix("Charm ").is_some_and(|rank| {
            !rank.is_empty() && rank.bytes().all(|b| matches!(b, b'I' | b'V' | b'X' | b'L'))
        })
}

/// Whether the reader is in a group, as far as the log has said.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Membership {
    /// Nothing seen yet that says either way, or something happened that the log does not
    /// describe (a login, a clock stepping back). NOT "nobody": it must never be rendered as solo.
    Unknown,
    /// The last word the log had was that the reader is alone.
    Solo,
    /// The reader is grouped. `members` is everyone the log has named in the group, never the
    /// reader, spelled as the game spelled them. `complete` is whether the log watched the group
    /// form, so every member since has had a line; see [`Party`] for why even that is a claim.
    Grouped {
        members: Vec<String>,
        complete: bool,
    },
}

/// The party state at the newest line pushed.
///
/// PRIVATE, AND SO IS [`Membership`]. Nothing outside this module has a use for the state as of
/// one line: every caller asks [`Party::during`] about a span, after the text, because a later line
/// can revoke. A public accessor had only tests calling it, so the tests moved in beside it.
#[derive(Clone, PartialEq, Eq, Debug)]
struct State {
    membership: Membership,
    /// Raid evidence is current. See [`Party`] for what ends it.
    raid: bool,
    /// Invites the reader sent that nothing has answered yet, as he typed the names.
    invites: Vec<String>,
}

impl State {
    /// EVERYONE IN THE READER'S GROUP, WHEN THE LOG PROVES IT, and `None` otherwise.
    ///
    /// `Some(&[])` is solo: provably nobody. `None` is "not known" and is the answer for a partial
    /// group, for any state while raid evidence is current, and for any state with an unanswered
    /// invite outstanding, because an invite can be accepted without a line (see [`Party`]).
    fn known(&self) -> Option<&[String]> {
        if self.raid || !self.invites.is_empty() {
            return None;
        }
        match &self.membership {
            Membership::Solo => Some(&[]),
            Membership::Grouped {
                members,
                complete: true,
            } => Some(members),
            _ => None,
        }
    }
}

/// What was known from one second on, until the next span starts.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Span {
    from: i64,
    known: Option<Vec<String>>,
}

/// THE READER'S GROUP, FOLDED OVER STAMPED LINES, AND WHAT WAS KNOWN ABOUT IT AT EVERY SECOND.
///
/// Push every line of the log in order with [`Party::push`], then ask [`Party::during`] about any
/// span of it. `Clone` is the resume point: a live fold keeps the party as of the line before its
/// window and clones it per poll, so nothing is read twice and nothing starts from `Unknown` that
/// earlier text had already settled. Pushing a line twice is NOT harmless (a member leaving twice
/// looks like somebody who was never there leaving), so a caller must never replay overlap.
///
/// # THE RULES, EACH WITH WHAT IT RESTS ON
///
/// * JOINING SOMEBODY ELSE'S GROUP IS PARTIAL. 79 of the 140 joins follow a `You notify` line, 58
///   of them on the same second, and nothing afterwards lists the rest of the group. The inviter
///   is recorded as a member; the membership is not known.
/// * FORMING A GROUP IS COMPLETE. 58 joins are followed on their own second by `You are now the
///   leader of your group.` and then `<Name> has joined the group.`, and a group that starts with
///   the reader alone in it has had every member announced since. Leadership passed to the reader
///   later, in a group he joined, proves nothing about who was already there and completes nothing.
/// * AN UNANSWERED INVITE IS NOT KNOWN, and this is where the owner-approved rule above turned out
///   to be wrong on its own. Four invitees (Jul 15 03:10, Aug 01 14:38, Aug 06 14:19 and Aug 06
///   14:55) spoke in the group or left it 94 to 1,897 seconds after the invite with NO join line
///   at all; at Aug 06 14:19 the reader zoned mid-invite and not even `You have joined the group.`
///   was printed. Answered joins took up to 2,194 seconds (median 5, 90th percentile 88) and 20 of
///   151 invites got no answer of any kind within half an hour, so no timeout is safe. An invite
///   ends at a join, a `Declined` answer, the invitee showing up in the group, removal, or a login.
/// * A MEMBER NOBODY ANNOUNCED REVOKES THE CLAIM. Other members invite too: 13 joins the reader
///   never invited arrived while he led and 36 while someone else did, and their invites print
///   nothing here. When a line proves a member the log never announced (they speak in the group,
///   lead it, or leave it), every span since the group formed stops being known, retroactively,
///   because the silent join could have been at any second of it. The same goes for group evidence
///   while solo, back to the last line that proved solo. [`Party::during`] can therefore answer
///   `None` about a span it once answered `Some`: ask it after the text, not while folding.
///   WHERE THIS CAN STILL BE WRONG: a member invited by somebody else, never announced, who never
///   speaks in the group, never leads and leaves without a line, is in the group and not in the
///   list. The log offers nothing that would catch him.
/// * REMOVAL PROVES SOLO. 133 lines. The first group-relevant thing after one was solo experience
///   85 times, `You are not in a group` 16, a new join 11, and group chat once: the Aug 06 14:19
///   silent join, which the invite rule covers. Removal also ends every invite and any raid.
/// * `You are not in a group` PROVES SOLO AT THAT SECOND (102 lines), and moves the point any later
///   contradiction revokes back to. It ends neither invites (a silent join came 1,897 seconds after
///   its invite) nor raid evidence. That second half is UNMEASURED: the line arrived inside raid
///   evidence zero times in the four logs, so it is a choice, taken on the side that only keeps
///   membership unknown longer.
/// * PARTY EXPERIENCE AND PARTY CHAT PROVE GROUPED. Solo experience proves nothing: it arrived 68
///   times inside a group the log watched form, while the member was merely out of range.
/// * RAID EVIDENCE IS CURRENT UNTIL `You have been removed from the group.` Eight removals happened
///   while raid evidence was current; the next raid line after any of them came 2,960 seconds later
///   at the soonest, and four were followed by none at all before the next login. Nothing else ends
///   it. Experience does not: 151 experience lines sit between two raid lines, one of them two
///   seconds after `Loxo tells the raid`. A login does not, on the safe side of an unmeasured
///   question: keeping it current longer only keeps membership unknown longer.
/// * A LOGIN FORGETS THE GROUP. Thirteen logins came while a group was held. After six the reader
///   was still grouped, after three he was earning solo experience, and not one printed a removal.
/// * A NAMELESS REFUSAL ANSWERS THE ONE INVITE IT CAN BELONG TO. `To invite another group into
///   yours` names nobody. All eight came 0 to 2 seconds after a `You invite`, and every time exactly
///   one name had been invited in that window, so it answers that invite. With two names invited
///   inside the window it answers neither: striking the wrong one would call an invite answered
///   that can still bring somebody in silently.
/// * A `You notify` THAT A LATER PROOF OUTLIVES NEVER BECAME A JOIN. It waits for the join it
///   announces, and a removal or `You are not in a group` on a LATER second proves that join did
///   not come. Measured once, Jul 30: `You notify Mayja` at 21:14:55 with no join, `You are not in
///   a group` at 21:53:34, and the reader FORMING a group with Mayja at 22:11:57, which the stale
///   accept had turned into a partial group. On the notify's own second it is kept, by the rule
///   below on order inside a second.
/// * NAMELESS PARTY EVIDENCE BREAKS A COMPLETE GROUP WITH NOBODY LISTED. `You gain party experience`
///   and `You tell your party` prove another member. A complete group that lists one explains it;
///   an empty one with no invite outstanding cannot, so it revokes exactly as a named stranger
///   does. Seventeen empty complete stretches in the four logs, all zero seconds long, and no party
///   evidence inside any of them: latent, and it is the answer solo would give wrongly.
/// * ORDER INSIDE ONE SECOND IS NOT EVIDENCE. The game printed `Eldoth is now the leader of your
///   group.` BEFORE `You have been removed from the group.` for one change at Aug 06 14:19:13, and
///   up to 32 lines share a second. A line on the same second as the proof it seems to contradict
///   is taken as printed before it.
/// * A CLOCK THAT STEPS BACK IS NOT KNOWN. Everything is keyed on the log's own seconds, so any
///   span overlapping a backward step is `None` and the state restarts at `Unknown`.
///
/// # WHAT IT KNOWS, MEASURED
///
/// Folded over all four logs beside `fights::Fights`, exactly as a caller would: 3,466 fights, of
/// which 1,490 are known solo, 254 known grouped (168 with one member, 83 with two, 3 with three)
/// and 1,722 not known. Of the not known, 1,022 ended with the state `Unknown`, nearly all after a
/// login that no later line settled; 386 in a partial group; 131 with an invite outstanding; 69
/// inside raid evidence. Asked 30 seconds after each fight and again after the whole file, the
/// answers never differed: no fight in these logs was ever revoked after the fact. A known fight
/// hides every one-word name outside the group, 2,386 hidings across 967 known fights, and 141 of
/// the hidings in known solo fights are of somebody who was the reader's group mate at another
/// time in the same file. Three of
/// those were checked line by line, and in all three the removal came first: Clea's group ended 27
/// seconds before the fight began, Thalish's 78, and the Jul 15 bystanders were only invited after.
#[derive(Clone, Debug)]
pub struct Party {
    state: State,
    spans: Vec<Span>,
    /// The span, and its second, holding the proof the current known run rests on: where a
    /// contradiction revokes back to.
    proof: Option<(usize, i64)>,
    /// Second ranges a backward clock step made ambiguous.
    fog: Vec<(i64, i64)>,
    last: Option<i64>,
    /// The inviter from `You notify`, and the second it was sent, waiting for the join it announces.
    accepted: Option<(String, i64)>,
    /// The second of a join the reader did not accept an invite for, waiting to see whether the
    /// very next group line makes him its leader on that same second.
    forming: Option<i64>,
    /// The newest second each outstanding invite was typed, so a nameless refusal can be matched to
    /// the invite in front of it. One entry per name; entries for answered invites are pruned.
    sent: Vec<(String, i64)>,
    /// The second of every `Welcome to EverQuest Legends!`, in the order pushed. Session `n` runs
    /// from `logins[n - 1]` to `logins[n]`, open at either end that has no login.
    logins: Vec<i64>,
    /// The reader's pets, each with the session it answered him in. One entry per name per session.
    pets: Vec<(String, usize)>,
    /// The second of the reader's newest charm cast that nothing has landed or interrupted yet.
    charm_cast: Option<i64>,
    /// THE MOB THE READER HAS CHARMED NOW, spelled as its landing line spelled it. See
    /// [`Party::charmed`].
    charmed: Option<String>,
}

impl Default for Party {
    fn default() -> Self {
        Self::new()
    }
}

impl Party {
    /// A party that has seen nothing, and so knows nothing.
    pub fn new() -> Self {
        Party {
            state: State {
                membership: Membership::Unknown,
                raid: false,
                invites: Vec::new(),
            },
            spans: Vec::new(),
            proof: None,
            fog: Vec::new(),
            last: None,
            accepted: None,
            forming: None,
            sent: Vec::new(),
            logins: Vec::new(),
            pets: Vec::new(),
            charm_cast: None,
            charmed: None,
        }
    }

    /// Offer one raw log line, stamp and all. Every stamped line should be offered, not only
    /// group lines: the clock is watched on all of them so a backward step is never missed.
    pub fn push(&mut self, raw: &str) {
        let Some((at, body)) = split_stamp(raw.trim_end_matches(['\r', '\n'])) else {
            return;
        };
        let now = seconds(at);
        if let (Some(now), Some(last)) = (now, self.last) {
            if now < last {
                self.fog.push((now, last));
                self.forget();
                self.record(now, false);
            }
        }
        if now.is_some() {
            self.last = now;
        }
        /* THE READER'S CHARM, WHICH BEGINS AND ENDS ON LINES OF ITS OWN and says nothing about the
         * group, so like a pet's answer it never reaches `apply`. See `Party::charmed`. */
        if self.charm(body, now) {
            return;
        }
        /* A PET'S ANSWER NAMES THE READER'S PET AND NOTHING ABOUT THE GROUP, so it goes nowhere
         * near `apply`: no span, no stamp needed, and it must not consume `forming` (see `pet_of`). */
        if let Some(name) = pet_of(body) {
            let session = self.logins.len();
            if !self
                .pets
                .iter()
                .any(|(n, s)| *s == session && n.eq_ignore_ascii_case(name))
            {
                self.pets.push((name.to_owned(), session));
            }
            return;
        }
        let Some(change) = read(body) else {
            return;
        };
        match now {
            Some(now) => self.apply(change, now),
            /* A GROUP LINE WITH NO PLACE IN TIME. Skipping it could skip a removal and keep
             * claiming a group that is gone, so what it did is unknowable and the state says so,
             * from the last second this party could read. */
            None => {
                self.forget();
                if let Some(last) = self.last {
                    self.record(last, false);
                }
            }
        }
    }

    /// EVERYONE PROVABLY IN THE READER'S GROUP AT ANY SECOND FROM `start` TO `end`, both stamps as
    /// the log printed them.
    ///
    /// `Some(names)` only when membership was known for every second of the span; the union keeps
    /// a member who left halfway, because he was in the group for part of it. `Some(vec![])` is
    /// solo throughout. `None` when any second of it was not known, when either stamp is
    /// unreadable, and for any span that starts on or before the first group line this party saw.
    pub fn during(&self, start: &str, end: &str) -> Option<Vec<String>> {
        let (from, to) = (seconds(start)?, seconds(end)?);
        if to < from || self.fog.iter().any(|&(a, b)| from <= b && to >= a) {
            return None;
        }
        /* NOTHING IS RECORDED BEFORE THE FIRST GROUP LINE, SO NOTHING BELOW COVERS IT. The loop
         * counts a span as covered the moment any recorded span overlaps it, and without this a
         * fight that opened while nothing was known and ran past a removal came out solo. Measured
         * on real bytes: the desktop's capture has one group line, a removal at 23:21:54, and at a
         * quiet window of 168 its first two fights are one run from 23:16:50 that spent five
         * minutes grouped with Fylasem and Poguhy and was reported as `Some([])`.
         *
         * `>=`, NOT `>`: the first line's own second is partly before it, by the same inclusive
         * rule the loop states, and before it nothing was known. */
        if !self.spans.first().is_some_and(|first| first.from < from) {
            return None;
        }
        let mut names: Vec<String> = Vec::new();
        let mut covered = false;
        for (i, span) in self.spans.iter().enumerate() {
            /* INCLUSIVE AT BOTH ENDS. A change stamped on a second happened somewhere inside it, so
             * the state before it held for part of that second too. */
            let until = self.spans.get(i + 1).map_or(i64::MAX, |next| next.from);
            if span.from > to || until < from {
                continue;
            }
            covered = true;
            for name in span.known.as_ref()? {
                if !has(&names, name) {
                    names.push(name.clone());
                }
            }
        }
        covered.then_some(names)
    }

    /// THE READER'S OWN PETS DURING A SPAN: every name that answered him as `Master` anywhere in a
    /// session the span touches, sessions being the stretches between logins. Empty when none did,
    /// and empty is "none proven", never "he had no pet".
    ///
    /// # WHY A GROUP FILTER NEEDS THIS AT ALL
    ///
    /// A summoned pet's name is one word (`Gabtik`, `Vibartik`, `Jabaner`), so by name it is a
    /// player, and a filter that keeps only the reader and his group took the reader's own pet off
    /// his meter the moment the group was known. Measured on the four logs before this existed: 63
    /// known fights lost 122,294 damage of the reader's pets that way, while 81 fights whose group was
    /// not known kept the same pets, so a pet appeared or vanished on nothing but whether the group
    /// was known. A charmed one-word mob (`Bzzazzt`, charmed Aug 14 03:36:07) was dropped from 25
    /// known solo fights the same way.
    ///
    /// # THE WHOLE SESSION, BACKWARDS AS WELL AS FORWARDS
    ///
    /// A pet exists before its first answer: across the four logs 186,071 damage of these pets came
    /// before the first answer in its session, against 1,390,808 after. So a proof counts for the
    /// session it is in, from the login before it to the login after, and like a revocation it is
    /// asked after the text. A login ends it, because a pet does not survive one.
    ///
    /// # WHERE IT CAN BE WRONG, SAID OUT LOUD
    ///
    /// * A charmed mob is the reader's for the whole session, including before the charm, when it
    ///   was hitting him: `Bazzzazzt says, 'You will not evade me, Reviir!'` three times, then later
    ///   answers `Master`. Its damage then is on the reader's side of the ledger.
    /// * Another pet that happens to carry the same generated name in the same session is kept.
    /// * A pet that never answered in its session is not proven and is not here.
    /// * A GROUP MEMBER'S PET IS NEVER HERE. Nothing the reader's log prints ties a pet to its owner
    ///   unless the owner is the reader, so a filter that knows the group leaves members' pets off.
    pub fn pets_during(&self, start: &str, end: &str) -> Vec<String> {
        let (Some(from), Some(to)) = (seconds(start), seconds(end)) else {
            return Vec::new();
        };
        let mut out: Vec<String> = Vec::new();
        for (name, session) in &self.pets {
            let opened = session
                .checked_sub(1)
                .and_then(|i| self.logins.get(i))
                .copied()
                .unwrap_or(i64::MIN);
            let closed = self.logins.get(*session).copied().unwrap_or(i64::MAX);
            if from <= closed && to >= opened && !has(&out, name) {
                out.push(name.clone());
            }
        }
        out
    }

    /// THE MOB THE READER HAS CHARMED AS OF THE NEWEST LINE PUSHED, or `None`.
    ///
    /// # AS OF ONE LINE, AND THAT IS THE POINT
    ///
    /// Unlike [`Party::during`] this is asked WHILE folding, line by line, because a charm is
    /// minutes long and its mob's name is shared with every other mob of its kind: `a tormented dead`
    /// that is the reader's at 03:30:40 is hitting him at 03:31:40. A whole fight or session asked
    /// after the text cannot tell those apart, and the fold beside this can, one line at a time.
    ///
    /// # WHAT BEGINS ONE AND WHAT ENDS IT, MEASURED ON THE OWNER'S SEP 11 LOG
    ///
    /// * BEGINS: `X has been charmed.` within [`CHARM_SECONDS`] of the reader casting a
    ///   [`charm_spell`]. 45 charms, and 27 of them were confirmed by the pet telling him
    ///   `Attacking ... Master.` afterwards.
    /// * ENDS: `Your Charm spell has worn off of X.` (31 of the 45), a new charm of his (11), a zone
    ///   (3), or a login (none in that log, and a pet does not survive one). `Your Charm spell is
    ///   interrupted.` ends the cast before it can land.
    /// * NOT ENDED BY A DEATH. In 6 charms something of the pet's name was slain and the pet went on
    ///   dealing 5,659 damage afterwards: what died was a hostile mob that shared its name.
    ///
    /// # WHERE IT CAN BE WRONG, SAID OUT LOUD
    ///
    /// Another player's charm landing within two seconds of the reader's own cast, on some other
    /// mob, is taken for his. And the name alone cannot tell the pet from a hostile mob of its kind,
    /// which is why the fold also asks who each line's other party is: see
    /// `grimoire_parse::fights::Fights::push_with_pet`.
    pub fn charmed(&self) -> Option<&str> {
        self.charmed.as_deref()
    }

    /// READ ONE LINE BODY FOR THE READER'S CHARM, and say whether it was a charm line. See
    /// [`Party::charmed`] for each rule and what it rests on.
    fn charm(&mut self, body: &str, now: Option<i64>) -> bool {
        if let Some(spell) = body
            .strip_prefix("You begin casting ")
            .and_then(|rest| rest.strip_suffix('.'))
        {
            if charm_spell(spell) {
                self.charm_cast = now;
            }
            return true;
        }
        if let Some(name) = body.strip_suffix(" has been charmed.") {
            if let (Some(now), Some(cast)) = (now, self.charm_cast) {
                if (0..=CHARM_SECONDS).contains(&(now - cast)) {
                    self.charmed = Some(name.to_owned());
                    self.charm_cast = None;
                }
            }
            return true;
        }
        if let Some(rest) = body.strip_prefix("Your ") {
            if let Some(spell) = rest.strip_suffix(" spell is interrupted.") {
                if charm_spell(spell) {
                    self.charm_cast = None;
                }
                return true;
            }
            if let Some((spell, name)) = rest
                .strip_suffix('.')
                .and_then(|r| r.split_once(" spell has worn off of "))
            {
                if charm_spell(spell)
                    && self
                        .charmed
                        .as_deref()
                        .is_some_and(|c| c.eq_ignore_ascii_case(name))
                {
                    self.charmed = None;
                }
                return true;
            }
        }
        /* THE SAME TWO SHAPES `combat` READS AS A ZONE, and the same one it refuses. */
        if let Some(rest) = body.strip_prefix("You have entered ") {
            if !rest.starts_with("an area where") {
                self.charmed = None;
                self.charm_cast = None;
            }
            return true;
        }
        false
    }

    fn apply(&mut self, change: Change<'_>, now: i64) {
        let forming = self.forming.take();
        let same_second = self.proof.is_some_and(|(_, at)| at == now);
        let mut proved = false;
        match change {
            Change::YouJoined => {
                let inviter = self.accepted.take().map(|(name, _)| name);
                self.forming = inviter.is_none().then_some(now);
                self.state.membership = Membership::Grouped {
                    members: inviter.into_iter().collect(),
                    complete: false,
                };
                self.proof = None;
            }
            Change::YouLead => {
                if forming == Some(now) {
                    if let Membership::Grouped { complete, .. } = &mut self.state.membership {
                        *complete = true;
                        proved = true;
                    }
                }
            }
            Change::YouLeft => {
                if self.state.membership == Membership::Solo
                    && self.state.invites.is_empty()
                    && !same_second
                {
                    self.revoke();
                }
                self.state.membership = Membership::Solo;
                self.state.raid = false;
                self.state.invites.clear();
                self.stale_accept(now);
                proved = true;
            }
            Change::NotGrouped => {
                if matches!(
                    self.state.membership,
                    Membership::Grouped { complete: true, .. }
                ) && !same_second
                {
                    self.revoke();
                }
                self.state.membership = Membership::Solo;
                self.stale_accept(now);
                proved = true;
            }
            Change::Login => {
                self.forget();
                self.logins.push(now);
            }
            Change::Raid => self.state.raid = true,
            Change::YouAccepted(name) => self.accepted = Some((name.to_owned(), now)),
            Change::YouInvited(name) => {
                let member = matches!(&self.state.membership,
                    Membership::Grouped { members, .. } if has(members, name));
                if !member && !has(&self.state.invites, name) {
                    self.state.invites.push(name.to_owned());
                }
                let invites = &self.state.invites;
                self.sent
                    .retain(|(n, _)| !n.eq_ignore_ascii_case(name) && has(invites, n));
                if has(invites, name) {
                    self.sent.push((name.to_owned(), now));
                }
            }
            Change::Declined(name) => {
                self.answer(name);
            }
            Change::Refused => {
                let invites = &self.state.invites;
                let mut window = self
                    .sent
                    .iter()
                    .filter(|(n, at)| now - at <= REFUSAL_SECONDS && has(invites, n));
                if let (Some((only, _)), None) = (window.next(), window.next()) {
                    let only = only.clone();
                    self.answer(&only);
                }
            }
            Change::Joined(name) => self.seen(Some(name), true, same_second),
            Change::Spoke(name) | Change::Leads(name) => self.seen(Some(name), false, same_second),
            Change::YouGrouped => self.seen(None, false, same_second),
            Change::Left(name) => self.left(name, same_second),
        }
        self.record(now, proved);
    }

    /// A line that proves the reader is grouped, naming a member or nobody. `announced` is a
    /// `has joined` line, the one way a member is SUPPOSED to arrive.
    fn seen(&mut self, name: Option<&str>, announced: bool, same_second: bool) {
        let invited = name.is_some_and(|n| has(&self.state.invites, n));
        let surprise = match &self.state.membership {
            Membership::Unknown => false,
            Membership::Solo => {
                if same_second {
                    return;
                }
                /* ANY OUTSTANDING INVITE EXPLAINS IT: the invitee accepted without a line, and
                 * whoever he brought is in too. */
                self.state.invites.is_empty()
            }
            Membership::Grouped { members, complete } => {
                *complete
                    && !announced
                    && !invited
                    && !same_second
                    && match name {
                        Some(n) => !has(members, n),
                        /* NAMELESS: somebody else is in the group. A listed member explains it,
                         * and so does an invite out; an empty list with neither cannot. */
                        None => members.is_empty() && self.state.invites.is_empty(),
                    }
            }
        };
        if surprise {
            self.revoke();
        }
        if let Some(n) = name {
            self.answer(n);
        }
        match &mut self.state.membership {
            Membership::Grouped { members, .. } => {
                if let Some(n) = name.filter(|n| !has(members, n)) {
                    members.push(n.to_owned());
                }
            }
            other => {
                *other = Membership::Grouped {
                    members: name.map(str::to_owned).into_iter().collect(),
                    complete: false,
                }
            }
        }
    }

    fn left(&mut self, name: &str, same_second: bool) {
        let invited = self.answer(name);
        match &mut self.state.membership {
            Membership::Unknown => {}
            Membership::Solo => {
                if same_second {
                    return;
                }
                if !invited && self.state.invites.is_empty() {
                    self.revoke();
                }
                /* Grouped at some point, and one member has gone: who remains is not in the log. */
                self.state.membership = Membership::Unknown;
            }
            Membership::Grouped { members, complete } => {
                if let Some(i) = members.iter().position(|m| m.eq_ignore_ascii_case(name)) {
                    members.remove(i);
                } else if *complete && !invited && !same_second {
                    self.revoke();
                }
            }
        }
    }

    /// Strike an invite off, reporting whether it was outstanding.
    fn answer(&mut self, name: &str) -> bool {
        let before = self.state.invites.len();
        self.state.invites.retain(|i| !i.eq_ignore_ascii_case(name));
        self.state.invites.len() != before
    }

    /// THE KNOWN RUN WAS A LIE. Every span from its proof on stops being known, and a group the
    /// log was trusted to have watched form is no longer trusted to be complete.
    fn revoke(&mut self) {
        if let Some((from, _)) = self.proof.take() {
            for span in &mut self.spans[from..] {
                span.known = None;
            }
        }
        if let Membership::Grouped { complete, .. } = &mut self.state.membership {
            *complete = false;
        }
    }

    /// A removal or a solo proof on a LATER second than the `You notify` in hand: that accept never
    /// became a join, and it must not name an inviter in a group the reader forms afterwards.
    fn stale_accept(&mut self, now: i64) {
        if self.accepted.as_ref().is_some_and(|(_, at)| *at < now) {
            self.accepted = None;
        }
    }

    fn forget(&mut self) {
        self.state.membership = Membership::Unknown;
        self.state.invites.clear();
        self.sent.clear();
        self.accepted = None;
        self.forming = None;
        self.proof = None;
        /* A LOGIN, OR A CLOCK THAT STEPPED BACK: a charm does not survive the first, and after the
         * second nothing says which lines came before the charm and which after. */
        self.charmed = None;
        self.charm_cast = None;
    }

    fn record(&mut self, now: i64, proved: bool) {
        let known = self.state.known().map(<[String]>::to_vec);
        if proved || self.spans.last().map(|s| &s.known) != Some(&known) {
            self.spans.push(Span { from: now, known });
        }
        if proved {
            self.proof = Some((self.spans.len() - 1, now));
        }
    }
}

fn has(names: &[String], name: &str) -> bool {
    names.iter().any(|n| n.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests;
