//! THE READER'S GROUP: what the log proves about who was in it, and what it does not.
//!
//! Every body below is a form measured in the owner's four logs (see the module doc above this
//! file), and the sequences are the real ones they came from wherever a real one exists: the Aug 06
//! Dathgunz invite that was never announced, the Aug 06 14:19 join lost to a zone, Loxo's raid.
//! Stamps are synthetic seconds after noon so a test can say exactly which span it asks about.
//!
//! A UNIT TEST MODULE AND NOT `tests/party.rs`, WHICH IT WAS. Several of these assert the state as
//! of one line, and the only way an integration test could see that was a public `Party::state`
//! whose only callers were these tests. Nothing in the app has a use for it, so it went private
//! and the tests moved in beside it.
use super::{pet_of, read, Change, Membership, Party};

fn stamp(at: u32) -> String {
    format!(
        "Thu Aug 06 {:02}:{:02}:{:02} 2026",
        12 + at / 3600,
        at / 60 % 60,
        at % 60
    )
}

fn line(at: u32, body: &str) -> String {
    format!("[{}] {body}", stamp(at))
}

fn fold(lines: &[(u32, &str)]) -> Party {
    let mut p = Party::new();
    for (at, body) in lines {
        p.push(&line(*at, body));
    }
    p
}

fn over(p: &Party, from: u32, to: u32) -> Option<Vec<String>> {
    p.during(&stamp(from), &stamp(to))
}

fn names(list: &[&str]) -> Option<Vec<String>> {
    Some(list.iter().map(|s| (*s).to_owned()).collect())
}

/// The reader forms his own group, the 58-times-measured shape: his invite, his join, his
/// leadership and the invitee's join, the last three on one second.
const FORM: &[(u32, &str)] = &[
    (0, "You invite Zarmin to join your group."),
    (4, "You have joined the group."),
    (4, "You are now the leader of your group."),
    (4, "Zarmin has joined the group."),
];

/// DEFECT: A SPAN THAT OPENS BEFORE THE FIRST GROUP LINE THE PARTY SAW, ANSWERED FROM THAT LINE ON.
///
/// A party records nothing until its first group line, so no span covers the stretch in front of
/// it, and `during` counted a query as covered as soon as ANY recorded span overlapped it. A fight
/// that opened while nothing was known and ran past a removal came out solo. That is real bytes,
/// not a construction: the desktop's capture fixture has exactly one group line, `You have been
/// removed from the group.` at 23:21:54, and at a quiet window of 168 its first two fights are one
/// run from 23:16:50, five minutes of it grouped with Fylasem and Poguhy, reported as
/// `Some([])` (`grimoire-desktop` `fights::tests::the_captures_fights_are_not_known_before_its_removal_line_and_solo_after_it`).
///
/// THE FIRST LINE'S OWN SECOND IS NOT KNOWN EITHER, by the rule `during` already states for every
/// other change: a change stamped on a second happened somewhere inside it, so what held before it
/// held for part of that second too, and before the first line that is nothing.
///
/// WHAT MUTATION MAKES THIS RED: deleting the first-span check (the first assertion); `>` in place
/// of `>=` (the second).
#[test]
fn a_span_that_opens_before_the_first_group_line_is_not_known() {
    let p = fold(&[(100, "You have been removed from the group.")]);
    assert_eq!(
        over(&p, 50, 150),
        None,
        "a span that opened before the first group line the party saw was answered from that line on"
    );
    assert_eq!(
        over(&p, 100, 150),
        None,
        "the removal's own second is partly before the removal, and before it nothing was known"
    );
    assert_eq!(
        over(&p, 101, 150),
        names(&[]),
        "after the removal the reader is provably alone"
    );
}

/// DEFECT: a measured form the reader does not recognise, or recognises as the wrong change.
#[test]
fn the_reader_knows_every_measured_form() {
    let table: &[(&str, Option<Change>)] = &[
        ("You have joined the group.", Some(Change::YouJoined)),
        ("You have been removed from the group.", Some(Change::YouLeft)),
        ("You are now the leader of your group.", Some(Change::YouLead)),
        ("Eldoth is now the leader of your group.", Some(Change::Leads("Eldoth"))),
        ("Zarmin has joined the group.", Some(Change::Joined("Zarmin"))),
        ("Asano has left the group.", Some(Change::Left("Asano"))),
        (
            "You notify Hert that you agree to join the group.",
            Some(Change::YouAccepted("Hert")),
        ),
        ("You invite Zarmin to join your group.", Some(Change::YouInvited("Zarmin"))),
        ("You invite flagg to join your group.", Some(Change::YouInvited("flagg"))),
        (
            "Avidar is currently considering joining another group.",
            Some(Change::Declined("Avidar")),
        ),
        (
            "Avidar rejects your offer to join the group.",
            Some(Change::Declined("Avidar")),
        ),
        ("Player me was not found.", Some(Change::Declined("me"))),
        (
            "You are not in a group. Talking to yourself again?",
            Some(Change::NotGrouped),
        ),
        ("You are not in a group!  Keep it all.", Some(Change::NotGrouped)),
        (
            "Flagg tells the group, 'lol this level 25 shadowknight hitting me for 1-2 damage on most hits'",
            Some(Change::Spoke("Flagg")),
        ),
        ("You tell your party, 'join me in the real real'", Some(Change::YouGrouped)),
        ("You gain party experience! (1.268%)", Some(Change::YouGrouped)),
        ("You gain party experience (with a bonus)! (0.112%)", Some(Change::YouGrouped)),
        ("Loxo tells the raid, 'WHO CAN MAIN ASSIST TOP DOWN>?????'", Some(Change::Raid)),
        ("You tell your raid, 'rebuff?'", Some(Change::Raid)),
        (
            "You receive no experience for defeating this creature as you are in a raid.",
            Some(Change::Raid),
        ),
        (
            "You receive no loot for defeating this creature as you are in a raid.",
            Some(Change::Raid),
        ),
        ("Welcome to EverQuest Legends!", Some(Change::Login)),
        (
            "To invite another group into yours, please invite the leader of the other group.",
            Some(Change::Refused),
        ),
        /* A PET'S ANSWER IS NOT A GROUP LINE: `pet_of` reads it, `read` must not. */
        ("Gabtik says, 'Sorry, Master... calming down.'", None),
        /* READ AND DROPPED on purpose; the module doc says why for each. */
        ("Trueheart invites you to join a group.", None),
        ("To join the group, click on the 'FOLLOW' option, or 'DECLINE' to cancel.", None),
        ("You remove Chuchu from the party.", None),
        ("Ghorr is now group Main Assist", None),
        ("Reviir is no longer group Main Assist", None),
        ("You gain experience! (1.898%)", None),
    ];
    for (body, want) in table {
        assert_eq!(read(body), *want, "read {body:?} as the wrong change");
    }
}

/// DEFECT: an expedition list, a pet, or a chat line read as a change to the group.
///
/// `Hert has been removed from Nagafen's Lair - Group.` is 14 of the 95 expedition lines, and
/// reading it as a departure would drop a member who never left.
#[test]
fn expedition_lists_pets_and_chat_are_not_the_group() {
    for body in [
        "Hert has been removed from Nagafen's Lair - Group.",
        "Hert has been added to Nagafen's Lair - Group.",
        "Hert has been removed from The Ruins of Old Paineel - Solo.",
        "Bada tells you, 'Hert has joined the group.'",
        "Krelian tells General:1, 'You have been removed from the group.'",
        "You say, 'You have joined the group.'",
        "Lumpy`s warder has joined the group.",
        "Torklar Battlemaster has left the group.",
    ] {
        assert_eq!(
            read(body),
            None,
            "{body:?} was read as a change to the group"
        );
    }

    let mut lines = FORM.to_vec();
    lines.push((50, "Zarmin has been removed from Nagafen's Lair - Group."));
    lines.push((51, "Bada tells you, 'Zarmin has left the group.'"));
    assert_eq!(
        over(&fold(&lines), 100, 200),
        names(&["Zarmin"]),
        "an expedition list or a tell changed a group the log watched form"
    );
}

/// DEFECT: joining somebody else's group treated as knowing who is in it.
///
/// `You notify Hert` then `You have joined the group.` names only Hert. The rest of his group is
/// never listed, so membership is partial and must never filter anybody out.
#[test]
fn joining_an_existing_group_is_partial_and_names_the_inviter() {
    let p = fold(&[
        (0, "Hert invites you to join a group."),
        (60, "You notify Hert that you agree to join the group."),
        (60, "You have joined the group."),
    ]);
    assert_eq!(
        p.state.membership,
        Membership::Grouped {
            members: vec!["Hert".to_owned()],
            complete: false
        },
        "the inviter is the one member the join proves, and nobody else is known"
    );
    assert_eq!(
        over(&p, 100, 150),
        None,
        "a partial group was reported as known"
    );
}

/// DEFECT: leadership that arrives LATER treated as the reader having formed the group.
///
/// Formation is the leadership line on the join's own second. A join with no `You notify` in
/// front of it (a log cut mid-invite) followed a second later by leadership is a group the reader
/// took over, whose members before him the log never listed.
#[test]
fn leadership_a_second_after_the_join_does_not_complete_the_group() {
    let p = fold(&[
        (60, "You have joined the group."),
        (61, "You are now the leader of your group."),
    ]);
    assert_eq!(
        over(&p, 100, 150),
        None,
        "leadership one second after a join was taken as forming the group"
    );
}

/// DEFECT: forming your own group not treated as complete, so the filter never switches on.
#[test]
fn forming_your_own_group_is_complete() {
    let p = fold(FORM);
    assert_eq!(
        p.state.membership,
        Membership::Grouped {
            members: vec!["Zarmin".to_owned()],
            complete: true
        },
        "the reader formed this group on one second and every member since was announced, yet it \
         is not complete"
    );
    assert_eq!(
        over(&p, 100, 200),
        names(&["Zarmin"]),
        "a group the log watched form was not known"
    );
}

/// DEFECT: an invite nobody has answered treated as known.
///
/// The real sequence, Aug 06 14:55: Flagg's group formed, `You invite dathgunz`, and Dathgunz was
/// never announced. The next line naming him is `Dathgunz has left the group.` 1,897 seconds later.
/// Everything between the invite and that line had a member the list did not have.
#[test]
fn an_unanswered_invite_is_not_known_until_the_invitee_shows_up() {
    let p = fold(&[
        (0, "You invite flagg to join your group."),
        (4, "You have joined the group."),
        (4, "You are now the leader of your group."),
        (4, "Flagg has joined the group."),
        (30, "You invite dathgunz to join your group."),
        (1927, "Dathgunz has left the group."),
    ]);
    assert_eq!(
        over(&p, 10, 20),
        names(&["Flagg"]),
        "the invite typed as `flagg` was not answered by `Flagg has joined`, or the silent join \
         revoked the span BEFORE the invite that explains it"
    );
    assert_eq!(
        over(&p, 40, 50),
        None,
        "a span with Dathgunz's invite outstanding was reported as known"
    );
    assert_eq!(
        over(&p, 2000, 2010),
        names(&["Flagg"]),
        "Dathgunz leaving did not answer his invite"
    );
}

/// DEFECT: a join the game never printed, during a zone, turning a proven solo span into a lie
/// or leaving the reader solo while he is grouped.
///
/// The real sequence, Aug 06 14:19: removed, `You invite flagg`, a zone, and then Flagg speaking in
/// the group with no join line of any kind. The invite explains it, so the solo span before the
/// invite stands.
#[test]
fn a_join_lost_in_a_zone_is_explained_by_the_invite() {
    let p = fold(&[
        (0, "You have been removed from the group."),
        (5, "You invite flagg to join your group."),
        (8, "LOADING, PLEASE WAIT..."),
        (14, "You have entered West Commonlands."),
        (
            99,
            "Flagg tells the group, 'lol this level 25 shadowknight hitting me for 1-2 damage on most hits'",
        ),
    ]);
    assert_eq!(
        over(&p, 1, 4),
        names(&[]),
        "the solo span before the invite was revoked"
    );
    assert_eq!(
        over(&p, 10, 50),
        None,
        "the span with the invite outstanding was known"
    );
    assert_eq!(
        p.state.membership,
        Membership::Grouped {
            members: vec!["Flagg".to_owned()],
            complete: false
        },
        "Flagg speaking in the group did not make the reader grouped"
    );
}

/// DEFECT: a refused invite held open for ever, so a group stays unknown after its answer.
#[test]
fn a_refused_invite_is_answered() {
    for answer in [
        "Avidar is currently considering joining another group.",
        "Avidar rejects your offer to join the group.",
    ] {
        let mut lines = FORM.to_vec();
        lines.push((30, "You invite Avidar to join your group."));
        lines.push((30, "You invite Avidar to join your group."));
        lines.push((31, answer));
        assert_eq!(
            over(&fold(&lines), 40, 50),
            names(&["Zarmin"]),
            "{answer:?} did not answer the invite"
        );
    }
}

/// DEFECT: removal not proving solo.
#[test]
fn removal_proves_solo_and_the_fight_across_it_keeps_the_member() {
    let mut lines = FORM.to_vec();
    lines.push((300, "You have been removed from the group."));
    let p = fold(&lines);
    assert_eq!(
        p.state.membership,
        Membership::Solo,
        "`You have been removed from the group.` did not make the reader solo"
    );
    assert_eq!(
        over(&p, 301, 400),
        names(&[]),
        "after removal the reader is provably alone"
    );
    assert_eq!(
        over(&p, 100, 350),
        names(&["Zarmin"]),
        "a fight that ran across the removal lost the member who was there for part of it"
    );
}

/// DEFECT: a member who left mid-fight dropped from that fight, or kept in the next one.
#[test]
fn a_member_who_left_mid_fight_is_in_that_fight_and_not_the_next() {
    let mut lines = FORM.to_vec();
    lines.push((10, "You invite Hert to join your group."));
    lines.push((12, "Hert has joined the group."));
    lines.push((150, "Hert has left the group."));
    let p = fold(&lines);
    assert_eq!(
        over(&p, 100, 200),
        names(&["Zarmin", "Hert"]),
        "Hert was in the group for half this fight and is missing from it"
    );
    assert_eq!(
        over(&p, 160, 200),
        names(&["Zarmin"]),
        "Hert was kept after he left"
    );
}

/// DEFECT: `You are not in a group` not proving solo from a cold start.
#[test]
fn not_in_a_group_proves_solo_in_both_of_its_forms() {
    for body in [
        "You are not in a group. Talking to yourself again?",
        "You are not in a group!  Keep it all.",
    ] {
        let p = fold(&[(50, body)]);
        assert_eq!(
            p.state.membership,
            Membership::Solo,
            "{body:?} did not prove solo"
        );
        assert_eq!(
            over(&p, 60, 70),
            names(&[]),
            "{body:?} proved solo and the span after it was not known"
        );
    }
}

/// DEFECT: raid evidence ignored, so a raid's other groups are filtered out of every meter.
///
/// Loxo's raid, Sep 05: party experience two seconds after raid chat, so experience does not end a
/// raid. What does is removal from the group, measured on all eight removals inside raid evidence.
/// `You are not in a group` arrived inside raid evidence ZERO times in the four logs, so whether it
/// would end a raid is unmeasured, and it does not: the safe side only keeps membership unknown.
#[test]
fn raid_evidence_is_not_known_until_the_reader_leaves_the_group() {
    let mut lines = FORM.to_vec();
    lines.push((
        100,
        "Loxo tells the raid, 'WHO CAN MAIN ASSIST TOP DOWN>?????'",
    ));
    lines.push((102, "You gain party experience (with a bonus)! (0.112%)"));
    lines.push((3000, "You have entered Nagafen's Lair."));
    let mut p = fold(&lines);
    /* EACH SPAN IS ASKED BEFORE THE NEXT LINE IS PUSHED, AND THAT ORDER IS FORCED. `You are not in a
     * group` inside a complete group revokes back to its forming, and a removal while solo revokes
     * back to the last solo proof, so asking any of these afterwards passes whether or not the rule
     * it names holds. Mutation-tested: both orders let a planted defect through. */
    assert_eq!(
        over(&p, 160, 170),
        None,
        "a fight inside raid evidence was known"
    );
    p.push(&line(
        7200,
        "You are not in a group. Talking to yourself again?",
    ));
    assert_eq!(
        over(&p, 7300, 7400),
        None,
        "`You are not in a group` ended raid evidence, which no line in the four logs supports"
    );
    p.push(&line(9000, "You have been removed from the group."));
    assert_eq!(
        over(&p, 9001, 9100),
        names(&[]),
        "removal from the group did not end raid evidence"
    );

    let p = fold(&[
        (0, "You are not in a group. Talking to yourself again?"),
        (
            5,
            "You receive no loot for defeating this creature as you are in a raid.",
        ),
    ]);
    assert_eq!(
        over(&p, 10, 20),
        None,
        "the raid's loot refusal was not raid evidence"
    );
}

/// DEFECT: a log read from the middle of a session treated as known.
#[test]
fn starting_mid_session_is_unknown() {
    assert_eq!(
        over(&Party::new(), 0, 10),
        None,
        "a party that has seen nothing claimed to know the group"
    );
    let p = fold(&[
        (0, "Hert tells the group, 'inc'"),
        (5, "You gain party experience! (1.268%)"),
        (6, "You tell your party, 'omw'"),
    ]);
    assert_eq!(
        p.state.membership,
        Membership::Grouped {
            members: vec!["Hert".to_owned()],
            complete: false
        },
        "group chat with no join in front of it must give a partial group"
    );
    assert_eq!(
        over(&p, 0, 10),
        None,
        "mid-session group chat was taken as the whole group"
    );
}

/// DEFECT: a group carried across a login, with no removal line to end it.
#[test]
fn a_login_forgets_the_group() {
    let mut lines = FORM.to_vec();
    lines.push((500, "Welcome to EverQuest Legends!"));
    let p = fold(&lines);
    assert_eq!(
        p.state.membership,
        Membership::Unknown,
        "the group survived a login, which prints no removal"
    );
    assert_eq!(
        over(&p, 600, 700),
        None,
        "the group was believed across a login"
    );
    assert_eq!(
        over(&p, 100, 200),
        names(&["Zarmin"]),
        "the login rewrote the span before it"
    );
}

/// DEFECT: a member nobody announced speaking in a "complete" group, and the claim standing.
///
/// Other members invite too (13 unannounced joins while the reader led), so the silent join could
/// be anywhere since the group formed, and every span since stops being known. The solo span
/// before the group formed is untouched.
#[test]
fn an_unannounced_member_revokes_the_group_back_to_its_forming() {
    let mut lines = vec![(0, "You are not in a group. Talking to yourself again?")];
    lines.extend(FORM.iter().map(|(at, body)| (at + 10, *body)));
    let mut p = fold(&lines);
    assert_eq!(
        over(&p, 100, 200),
        names(&["Zarmin"]),
        "the group was not known before the unannounced member spoke, so this proves nothing"
    );

    p.push(&line(500, "Dathgunz tells the group, 'pull'"));
    assert_eq!(
        over(&p, 100, 200),
        None,
        "a member the log never announced spoke, and the span that left him out still claims to \
         be the whole group"
    );
    assert_eq!(
        over(&p, 1, 5),
        names(&[]),
        "the revocation reached back before the group formed"
    );
    assert_eq!(
        p.state.membership,
        Membership::Grouped {
            members: vec!["Zarmin".to_owned(), "Dathgunz".to_owned()],
            complete: false
        },
        "the unannounced member was not added, or the group is still trusted to be complete"
    );
}

/// DEFECT: group evidence while solo revoking past the proof that bounds it, or not at all.
#[test]
fn party_chat_while_solo_revokes_back_to_the_last_proof_and_no_further() {
    let mut p = fold(&[
        (0, "You are not in a group. Talking to yourself again?"),
        (100, "You are not in a group!  Keep it all."),
    ]);
    p.push(&line(200, "You tell your party, 'hi'"));
    assert_eq!(
        over(&p, 110, 120),
        None,
        "party chat proved a join the log never printed, and the solo span it breaks stood"
    );
    assert_eq!(
        over(&p, 10, 20),
        names(&[]),
        "the revocation went past `You are not in a group` at second 100, which proved solo there"
    );
}

/// DEFECT: order inside one second read as a contradiction.
///
/// Aug 06 14:19:13 printed the old group's leader change before the removal it belongs to. The
/// reverse order on one second is the same event and must leave the reader solo.
#[test]
fn a_line_on_the_same_second_as_a_removal_does_not_contradict_it() {
    let mut lines = FORM.to_vec();
    lines.push((100, "You have been removed from the group."));
    lines.push((100, "Zarmin tells the group, 'bye'"));
    let p = fold(&lines);
    assert_eq!(
        p.state.membership,
        Membership::Solo,
        "a group line on the removal's own second was read as coming after it"
    );
    assert_eq!(
        over(&p, 101, 150),
        names(&[]),
        "the span after the removal was not known solo"
    );
}

/// DEFECT: a clock that steps back answered from the wrong side of the step.
///
/// Solo until a group forms at 3300, then a stamp back at 1800. Seconds 3400 to 3500 exist on both
/// sides of the step, once grouped with Zarmin and once solo, and are not known.
#[test]
fn a_clock_that_steps_back_is_not_known_where_it_overlaps() {
    let mut lines = vec![(0, "You are not in a group. Talking to yourself again?")];
    lines.extend(FORM.iter().map(|(at, body)| (at + 3300, *body)));
    lines.push((3600, "Zarmin tells the group, 'inc'"));
    lines.push((1800, "You are not in a group. Talking to yourself again?"));
    let p = fold(&lines);
    assert_eq!(
        over(&p, 3400, 3500),
        None,
        "a span that exists on both sides of a backward clock step was answered from one of them"
    );
    assert_eq!(
        over(&p, 100, 200),
        names(&[]),
        "the step swallowed a span it does not overlap"
    );
}

/// DEFECT: a group line whose stamp cannot be read skipped, so a removal is lost.
#[test]
fn a_group_line_with_an_unreadable_stamp_makes_the_state_unknown() {
    let mut p = fold(FORM);
    p.push("[Thu Foo 06 12:05:00 2026] You have been removed from the group.");
    assert_eq!(
        p.state.membership,
        Membership::Unknown,
        "a removal nobody could place in time left the old group standing"
    );
    assert_eq!(
        over(&p, 400, 500),
        None,
        "a removal nobody could place was ignored"
    );
}

/// DEFECT: a member somebody else invited treated as a contradiction.
///
/// 36 joins the reader never invited arrived while someone else led and 13 while he did. A join the
/// game ANNOUNCED is the normal way in, whoever sent the invite, and keeps the group known.
#[test]
fn a_member_somebody_else_invited_is_announced_and_stays_known() {
    let mut lines = FORM.to_vec();
    lines.push((50, "Hert has joined the group."));
    let p = fold(&lines);
    assert_eq!(
        over(&p, 100, 200),
        names(&["Zarmin", "Hert"]),
        "an announced join through somebody else's invite broke a known group"
    );
    assert_eq!(
        over(&p, 10, 20),
        names(&["Zarmin"]),
        "an announced join revoked the span before it"
    );
}

/// DEFECT: inviting somebody already in the group opening an invite nothing will ever answer.
#[test]
fn inviting_a_member_opens_no_invite() {
    let mut lines = FORM.to_vec();
    lines.push((50, "You invite zarmin to join your group."));
    assert_eq!(
        over(&fold(&lines), 60, 70),
        names(&["Zarmin"]),
        "an invite to a member already in the group held it unknown"
    );
}

/// DEFECT: somebody the log never listed leaving a "complete" group, and the list standing.
///
/// Dathgunz's real `has left the group.` with his invite line taken away: nothing could have listed
/// him, so every span that left him out was never the whole group.
#[test]
fn an_unlisted_member_leaving_revokes_the_group() {
    let mut lines = FORM.to_vec();
    lines.push((500, "Dathgunz has left the group."));
    let p = fold(&lines);
    assert_eq!(
        over(&p, 100, 200),
        None,
        "somebody never listed left a complete group, and the list without him still stood"
    );
    assert_eq!(
        over(&p, 600, 700),
        None,
        "the group was still trusted to be complete after it was proven not to be"
    );
}

/// DEFECT: a member leaving while the reader is believed solo, and the belief standing.
#[test]
fn a_member_leaving_while_solo_revokes_back_to_the_proof() {
    let p = fold(&[
        (0, "You are not in a group. Talking to yourself again?"),
        (100, "Hert has left the group."),
    ]);
    assert_eq!(
        over(&p, 10, 20),
        None,
        "a departure from the reader's group broke a span called solo, and it stood"
    );
    assert_eq!(
        p.state.membership,
        Membership::Unknown,
        "who remains after Hert left is not in the log"
    );
}

/// DEFECT: a removal while believed solo, and the solo span before it standing.
///
/// `You have been removed from the group.` proves the reader was in one, so the solo span since the
/// last proof did not hold.
#[test]
fn a_removal_while_solo_revokes_back_to_the_proof() {
    let p = fold(&[
        (0, "You are not in a group. Talking to yourself again?"),
        (100, "You are not in a group!  Keep it all."),
        (200, "You have been removed from the group."),
    ]);
    assert_eq!(
        over(&p, 110, 120),
        None,
        "a removal proved a group inside a span called solo, and the span stood"
    );
    assert_eq!(
        over(&p, 10, 20),
        names(&[]),
        "the revocation went past the proof at second 100"
    );
    assert_eq!(
        over(&p, 201, 300),
        names(&[]),
        "the removal did not prove solo"
    );
}

/// DEFECT: `You are not in a group` inside a group the log watched form, and the list standing.
///
/// Measured once. The group ended at a second the log did not print, so the list it carried since
/// forming is not trusted.
#[test]
fn not_in_a_group_inside_a_complete_group_revokes_it() {
    let mut lines = FORM.to_vec();
    lines.push((500, "You are not in a group. Talking to yourself again?"));
    let p = fold(&lines);
    assert_eq!(
        over(&p, 100, 200),
        None,
        "the group ended without a line, and the list it carried still stood"
    );
    assert_eq!(
        over(&p, 501, 600),
        names(&[]),
        "`You are not in a group` did not prove solo after the group it ended"
    );
}

/// DEFECT: a departure on the removal's own second read as coming after it.
///
/// Jul 18 01:32:42 printed `Barman has left the group.` and then the removal: one disband. The
/// other order on one second is the same disband and must leave the reader solo.
#[test]
fn a_departure_on_the_same_second_as_a_removal_does_not_contradict_it() {
    let mut lines = FORM.to_vec();
    lines.push((100, "You have been removed from the group."));
    lines.push((100, "Zarmin has left the group."));
    let p = fold(&lines);
    assert_eq!(
        p.state.membership,
        Membership::Solo,
        "a departure on the removal's own second was read as coming after it"
    );
    assert_eq!(
        over(&p, 101, 150),
        names(&[]),
        "the span after the removal was not known solo"
    );
}

fn pets(p: &Party, from: u32, to: u32) -> Vec<String> {
    p.pets_during(&stamp(from), &stamp(to))
}

/// DEFECT: A PET'S ANSWER NOT RECOGNISED, OR SOMETHING THAT IS NOT ONE READ AS THE READER'S PET.
///
/// The seven measured answers are all the proof of a pet this log has, and a pet the reader does
/// not recognise is a pet a known group's filter takes off his meter. The forgeries are the lines
/// that look closest: chat quoting a pet, a multi-word mob (a charmed one keeps its own name and
/// shares it with every mob of its kind), an NPC whose NAME starts with Master, and a greeting.
///
/// WHAT MUTATION MAKES THIS RED: dropping a `says` form from `pet_of`; dropping the `told you`
/// branch; a `told you` branch that takes any tell ending in `Master.` (Hert's thanks).
#[test]
fn the_reader_knows_every_measured_pet_answer_and_nothing_that_only_looks_like_one() {
    for (body, name) in [
        ("Gabtik says, 'Sorry, Master... calming down.'", "Gabtik"),
        ("Jebobab says, 'Following you, Master.'", "Jebobab"),
        (
            "Vibartik says, 'I beg forgiveness, Master.  That is not a legal target.'",
            "Vibartik",
        ),
        (
            "Bazzzazzt says, 'Guarding with my life, oh splendid one.'",
            "Bazzzazzt",
        ),
        ("Bzzazzt says, 'Now regrouping, master.'", "Bzzazzt"),
        (
            "Bzzazzt told you, 'Attacking Protector of Sky Master.'",
            "Bzzazzt",
        ),
        (
            "Bzzazzt told you, 'I am unable to wake an azarack, Master.'",
            "Bzzazzt",
        ),
    ] {
        assert_eq!(
            pet_of(body),
            Some(name),
            "{body:?} was not read as the reader's pet"
        );
    }
    for body in [
        "Bada tells you, 'Gabtik says, 'Sorry, Master... calming down.''",
        "A bok ghoul knight says, 'Sorry, Master... calming down.'",
        "Master Yael staggers.",
        "Hert says, 'Hail, Master Xalg'",
        "Hert told you, 'thanks Master.'",
        "Bazzzazzt says, 'You will not evade me, Reviir!'",
    ] {
        assert_eq!(pet_of(body), None, "{body:?} was read as the reader's pet");
    }
}

/// DEFECT: THE READER'S PET PROVEN FOR PART OF A SESSION, OR CARRIED INTO THE NEXT ONE.
///
/// A pet exists before it first answers (186,071 damage of the owner's pets came before the first
/// answer in their session), so a proof covers its whole session, backwards too. A login ends it.
///
/// WHAT MUTATION MAKES THIS RED: a session opening at its own closing login rather than the one
/// before it (the first assertion); ignoring the login that closes the session (the second); a
/// login not recorded at all (the second).
#[test]
fn a_pet_is_the_readers_for_its_whole_session_and_not_the_next() {
    let p = fold(&[
        (0, "Welcome to EverQuest Legends!"),
        (100, "Gabtik hits a large rat for 5 points of damage."),
        (200, "Gabtik says, 'Sorry, Master... calming down.'"),
        (500, "Welcome to EverQuest Legends!"),
    ]);
    assert_eq!(
        pets(&p, 50, 150),
        vec!["Gabtik".to_owned()],
        "a fight before the pet's first answer in the same session lost the reader's pet"
    );
    assert_eq!(
        pets(&p, 600, 700),
        Vec::<String>::new(),
        "a pet was carried across a login, which it does not survive"
    );
    assert_eq!(
        pets(&Party::new(), 0, 10),
        Vec::<String>::new(),
        "a party that saw no pet answer named one"
    );
}

/// DEFECT: A PET'S ANSWER BETWEEN THE JOIN AND THE LEADERSHIP BREAKING THE FORMATION.
///
/// Formation is the leadership line on the join's second, read as the NEXT group line after the
/// join. A pet answers in the middle of every pull and up to 32 lines share a second, so a pet's
/// answer treated as a group line would sit between the two and turn the reader's own group into a
/// partial one.
///
/// WHAT MUTATION MAKES THIS RED: the pet branch in `push` clearing `forming`, or `pet_of` running
/// after `read` inside `apply`.
#[test]
fn a_pet_answering_between_the_join_and_the_leadership_does_not_break_the_formation() {
    let p = fold(&[
        (0, "You invite Zarmin to join your group."),
        (4, "You have joined the group."),
        (4, "Gabtik told you, 'Attacking a large rat Master.'"),
        (4, "You are now the leader of your group."),
        (4, "Zarmin has joined the group."),
    ]);
    assert_eq!(
        over(&p, 100, 200),
        names(&["Zarmin"]),
        "a pet's answer on the formation's second made the reader's own group partial"
    );
}

/// DEFECT: NAMELESS PARTY EVIDENCE IGNORED BY A COMPLETE GROUP WITH NOBODY LEFT IN IT.
///
/// The reader forms a group with Zarmin, Zarmin leaves, and then party experience arrives. Party
/// experience proves somebody else is grouped with him; the complete group lists nobody and no
/// invite is out, so it cannot be the whole group, and the answer was solo.
///
/// WHAT MUTATION MAKES THIS RED: the `None` arm in `seen` answering `false`, which is what the
/// nameless case did before.
#[test]
fn party_experience_in_a_complete_group_with_nobody_listed_revokes_it() {
    let mut lines = FORM.to_vec();
    lines.push((5, "Zarmin has left the group."));
    let mut p = fold(&lines);
    assert_eq!(
        over(&p, 6, 10),
        names(&[]),
        "before the party experience the empty complete group was known, so this proves nothing"
    );
    p.push(&line(20, "You gain party experience! (1.000%)"));
    assert_eq!(
        over(&p, 6, 30),
        None,
        "party experience proved another member and the empty complete group still answered solo"
    );
}

/// DEFECT: THE GAME'S REFUSAL OF AN INVITE NOT READ, SO THE INVITE STAYS OUT UNTIL REMOVAL.
///
/// The real sequence, Jul 30: `You invite Berkshire` at 23:29:22, the refusal at 23:29:24, `You are
/// not in a group` a little later. Nobody can join off a refused invite, so after the solo proof
/// the reader is known solo. Two names invited inside the window is the case the refusal cannot be
/// matched in, and a refusal three seconds after the invite is wider than any the logs show.
///
/// WHAT MUTATION MAKES THIS RED: the `Refused` arm doing nothing (first assertion); answering the
/// first invite in the window whatever else is in it (second); `REFUSAL_SECONDS` wide enough to
/// reach three seconds (third).
#[test]
fn a_refused_invite_is_answered_when_one_invite_could_be_the_one_refused() {
    let refused =
        "To invite another group into yours, please invite the leader of the other group.";
    let solo = "You are not in a group. Talking to yourself again?";
    let p = fold(&[
        (0, solo),
        (10, "You invite Berkshire to join your group."),
        (12, refused),
        (20, solo),
    ]);
    assert_eq!(
        over(&p, 21, 30),
        names(&[]),
        "the refusal did not answer Berkshire's invite, so a refused invite held solo unknown"
    );

    /* BERKSHIRE'S OWN REJECTION LATER IS WHAT MAKES THIS ABLE TO FAIL. Without it either answer
     * leaves one invite out and the span is not known whichever the refusal struck; with it, a
     * refusal that struck Pikey leaves nothing out and the reader reads as solo while Pikey's
     * invite can still bring him in silently. */
    let p = fold(&[
        (0, solo),
        (10, "You invite Pikey to join your group."),
        (11, "You invite Berkshire to join your group."),
        (12, refused),
        (15, "Berkshire rejects your offer to join the group."),
        (20, solo),
    ]);
    assert_eq!(
        over(&p, 21, 30),
        None,
        "a refusal that could belong to either of two invites answered Pikey's, the older one"
    );

    let p = fold(&[
        (0, solo),
        (10, "You invite Berkshire to join your group."),
        (13, refused),
        (20, solo),
    ]);
    assert_eq!(
        over(&p, 21, 30),
        None,
        "a refusal three seconds after the invite, wider than any measured, answered it"
    );
}

/// DEFECT: A `You notify` THAT NEVER BECAME A JOIN NAMING THE INVITER IN THE READER'S OWN GROUP.
///
/// The real sequence, Jul 30: `You notify Mayja` at 21:14:55 and no join, `You are not in a group`
/// at 21:53:34, then the reader forming his own group at 22:11:57. The stale accept made that
/// formation a partial group led by an inviter nobody accepted, so every fight in it was not known.
/// A removal proves the same. On the notify's own second the accept stands, because order inside a
/// second is not evidence.
///
/// WHAT MUTATION MAKES THIS RED: `stale_accept` not called on `NotGrouped` or on `YouLeft` (the
/// loop); `*at <= now` in `stale_accept` (the last assertion).
#[test]
fn an_accept_a_later_proof_outlives_does_not_name_the_inviter_in_a_formed_group() {
    for proof in [
        "You are not in a group. Talking to yourself again?",
        "You have been removed from the group.",
    ] {
        let mut lines = vec![
            (0, "You notify Hert that you agree to join the group."),
            (100, proof),
        ];
        lines.extend(FORM.iter().map(|(at, body)| (at + 200, *body)));
        let p = fold(&lines);
        assert_eq!(
            p.state.membership,
            Membership::Grouped {
                members: vec!["Zarmin".to_owned()],
                complete: true
            },
            "after {proof:?} the reader formed his own group, and a stale accept named Hert in it"
        );
        assert_eq!(over(&p, 300, 400), names(&["Zarmin"]));
    }

    let p = fold(&[
        (0, "You notify Hert that you agree to join the group."),
        (0, "You have been removed from the group."),
        (0, "You have joined the group."),
        (0, "You are now the leader of your group."),
    ]);
    assert_eq!(
        over(&p, 10, 20),
        None,
        "a removal on the notify's own second threw the accept away and made the join a formation"
    );
}

/// DEFECT: JOINING HERT'S GROUP AND TAKING ITS LEAD ON ONE SECOND CALLED FORMING IT.
///
/// `You notify Hert`, `You have joined the group.` and `You are now the leader of your group.`, all
/// on one second. The reader joined a group that already existed; leadership on the join's second
/// completes a group only when nobody's invite was accepted.
///
/// WHAT MUTATION MAKES THIS RED: `self.forming = Some(now)` in place of
/// `inviter.is_none().then_some(now)`.
#[test]
fn accepting_an_invite_and_leading_on_the_same_second_is_still_partial() {
    let p = fold(&[
        (60, "You notify Hert that you agree to join the group."),
        (60, "You have joined the group."),
        (60, "You are now the leader of your group."),
    ]);
    assert_eq!(
        over(&p, 100, 150),
        None,
        "a group the reader joined through Hert's invite was taken as formed by him"
    );
    assert_eq!(
        p.state.membership,
        Membership::Grouped {
            members: vec!["Hert".to_owned()],
            complete: false
        }
    );
}

/// DEFECT: A LOGIN ENDING RAID EVIDENCE, OR `You are not in a group` ENDING AN INVITE.
///
/// Both are documented choices on the side that keeps membership unknown longer, and both are
/// UNMEASURED the other way round. Each span is asked straight after the line that would end the
/// evidence, before anything else is pushed, or a later line settles it and the rule is not tested.
///
/// WHAT MUTATION MAKES THIS RED: `Change::Login` clearing `raid` (first); `NotGrouped` clearing
/// `invites` (second).
#[test]
fn a_login_keeps_raid_evidence_and_a_solo_proof_keeps_an_invite() {
    let solo = "You are not in a group. Talking to yourself again?";
    let p = fold(&[
        (0, solo),
        (10, "Loxo tells the raid, 'inc'"),
        (20, "Welcome to EverQuest Legends!"),
        (30, solo),
    ]);
    assert_eq!(
        over(&p, 31, 40),
        None,
        "a login ended raid evidence, which no line in the four logs supports"
    );

    let p = fold(&[
        (0, solo),
        (10, "You invite Hert to join your group."),
        (20, solo),
    ]);
    assert_eq!(
        over(&p, 21, 30),
        None,
        "`You are not in a group` ended an invite, and a silent join came 1,897 seconds after one"
    );
}
/// THE READER'S CHARM: HIS OWN CAST, ITS LANDING, AND EVERY LINE THAT ENDS IT, AND NONE THAT DOES NOT.
///
/// Bodies from the owner's Sep 11 log. A charm is asked as of the newest line, so each assertion
/// is taken after exactly the lines in front of it.
///
/// WHAT MUTATION MAKES THIS RED: a landing taken without the reader's cast, or outside
/// `CHARM_SECONDS`; a non-charm spell's cast or worn-off counted; a worn-off of another name
/// ending it; the levitation line read as a zone; a zone, a login or an interrupt not ending it.
#[test]
fn a_charm_is_the_readers_from_his_own_cast_until_a_line_ends_it() {
    let mut p = Party::new();
    let push = |p: &mut Party, at: u32, body: &str| p.push(&line(at, body));

    push(&mut p, 0, "You begin casting Togor's Insects IX.");
    push(&mut p, 1, "a thunder spirit has been charmed.");
    assert_eq!(
        p.charmed(),
        None,
        "somebody else's charm was taken for the reader's"
    );

    push(&mut p, 10, "You begin casting Charm VII.");
    push(&mut p, 13, "a ghoul has been charmed.");
    assert_eq!(
        p.charmed(),
        None,
        "a charm three seconds after the cast was taken for his"
    );

    push(&mut p, 20, "You begin casting Charm VII.");
    push(&mut p, 22, "a tormented dead has been charmed.");
    assert_eq!(
        p.charmed(),
        Some("a tormented dead"),
        "the reader's own charm did not land"
    );

    for body in [
        "Your Mesmerization spell has worn off of a tormented dead.",
        "Your Charm spell has worn off of a ghoul.",
        "A tormented dead has been slain by Nith!",
        "You have entered an area where levitation effects do not function.",
    ] {
        push(&mut p, 30, body);
        assert_eq!(
            p.charmed(),
            Some("a tormented dead"),
            "{body:?} ended the reader's charm"
        );
    }
    push(
        &mut p,
        40,
        "Your Charm spell has worn off of A tormented dead.",
    );
    assert_eq!(
        p.charmed(),
        None,
        "the charm outlived its own worn-off line"
    );

    for (end, what) in [
        ("You have entered The Estate of Unrest.", "a zone"),
        ("Welcome to EverQuest Legends!", "a login"),
    ] {
        push(&mut p, 100, "You begin casting Charm IV.");
        push(&mut p, 101, "a greater dark bone has been charmed.");
        assert_eq!(p.charmed(), Some("a greater dark bone"));
        push(&mut p, 150, end);
        assert_eq!(p.charmed(), None, "the charm outlived {what}");
    }

    push(&mut p, 200, "You begin casting Charm.");
    push(&mut p, 201, "Your Charm spell is interrupted.");
    push(&mut p, 202, "a dusty werebat has been charmed.");
    assert_eq!(
        p.charmed(),
        None,
        "a charm landed off a cast that was interrupted"
    );
}
