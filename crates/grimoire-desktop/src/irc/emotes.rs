//! Cutting one chat line into the runs a renderer draws: plain text and emote, in that order.
//!
//! WHY THIS IS ITS OWN FILE AND NOT FOUR LINES INSIDE THE READER.
//! The `emotes` tag hands over positions, and the positions are ZERO BASED CODE POINT OFFSETS,
//! INCLUSIVE AT BOTH ENDS. Rust slices bytes. Those two facts are the whole of this file. A body
//! with one multi byte character before an emote makes a byte offset slicer either paint the emote
//! over the wrong run of text or panic outright on a character boundary, and neither failure shows
//! up in a pure ASCII test, which is what most of chat is not. Measured on the 2026-09-04 capture
//! of seven busy channels: of 469 emote ranges, 17 would have been cut into the wrong text and 4
//! would have panicked. Those 21 hide inside 448 that a byte slicer gets right by luck.
//!
//! THE SECOND FACT, WHICH THE DOCUMENTATION DOES NOT SAY AND THE CAPTURE DOES.
//! When the body is a CTCP ACTION, that is a `/me`, the body arrives wrapped as
//! `\u{1}ACTION <text>\u{1}` and the emote offsets are counted from the start of `<text>`, NOT from
//! the start of the body. This is not a guess. Across the capture, resolving every range against
//! the raw body makes 28 emote ids resolve to two different names in different messages (id 80958
//! is "sumLove" in one line and "! sum1g" in another); resolving them against the unwrapped text
//! makes all 469 ranges agree, every id to exactly one name, zero conflicts. So the wrapper is
//! stripped for the arithmetic, and it is put back as ordinary text spans so the runs still add up
//! to the body byte for byte. A screen that would rather draw `/me` in the Twitch way can ask
//! [`action_payload`] for the inner text and hand THAT to [`spans`], which is equally correct.
//!
//! THE OUTPUT COVERS THE BODY EXACTLY ONCE. No gap, no overlap, nothing dropped. That is the one
//! property everything else here is in service of, and it is the property the tests hammer: if the
//! spans do not concatenate back to the input then some line of chat is going to render with a word
//! missing or a word twice, and nobody will be able to tell you which line.
//!
//! MALFORMED INPUT IS REFUSED, NOT PATCHED. Ranges that overlap, or that run past the end of the
//! body, cannot be cut into a sane sequence of runs. Guessing at a repair produces a body that is
//! subtly not what was said, which is worse than a body drawn with no emotes at all. So the whole
//! line's spans are refused, the refusal is typed, and it is COUNTED in a process wide total, so a
//! malformed line is never silently dropped even by the caller that does not look at the error.
//! The lenient [`spans`] then draws the body as one plain run: the words still arrive, the pictures
//! do not. The 2026-09-04 capture contains no malformed line at all, which is exactly why the
//! counter exists rather than an assumption that they cannot happen.

use std::sync::atomic::{AtomicU64, Ordering};

/* ---------------------------------------------------------------- the shapes -- */

/// The CTCP ACTION opener, as it arrives on the wire. Eight bytes, all ASCII apart from the leading
/// `\u{1}`, which is itself one byte, so the whole prefix is a character boundary at both ends.
pub const ACTION_PREFIX: &str = "\u{1}ACTION ";

/// The CTCP delimiter that closes an ACTION body.
pub const CTCP_DELIM: char = '\u{1}';

/// One run of a message body. Borrowed: a span points into the body and into the tag it came from,
/// so cutting a line allocates one `Vec` and copies no text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Span<'a> {
    /// Text to draw as text. Never empty: see [`push_text`].
    Text(&'a str),
    /// An emote. `id` is what Twitch calls it in the CDN url, `name` is the text it replaces.
    ///
    /// `id` IS A STRING AND NOT A NUMBER, and that is load bearing. The capture carries 226 ranges
    /// with plain numeric ids and 78 with ids of the form `emotesv2_<32 hex digits>`. Anything
    /// that parses an id as an integer throws away every follower emote on the service.
    Emote { id: &'a str, name: &'a str },
}

impl<'a> Span<'a> {
    /// The text this span occupies in the body. For an emote that is the name it was typed as,
    /// which is what has to be drawn while the image is still downloading, and what has to be
    /// copied when the reader selects the line.
    pub fn text(&self) -> &'a str {
        match *self {
            Span::Text(t) => t,
            Span::Emote { name, .. } => name,
        }
    }

    pub fn is_emote(&self) -> bool {
        matches!(*self, Span::Emote { .. })
    }
}

/// Why a line's spans were refused. Carries numbers rather than the offending text so that the type
/// stays free of lifetimes: the caller still holds the tag it passed in and can log that beside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A `/` separated section had no `:` between the id and its ranges.
    NoColon,
    /// The id before the `:` was empty.
    EmptyId,
    /// A range was not `<number>-<number>`.
    BadRange,
    /// `end` came before `start`. Twitch has never sent one; a proxy or a rewrite might.
    Reversed { start: usize, end: usize },
    /// The range runs past the last code point of the body. `code_points` is the body's length in
    /// code points, so a range ending at exactly `code_points` is already one too far: the offsets
    /// are INCLUSIVE, and that off by one is the whole reason this variant carries both numbers.
    OutOfRange { end: usize, code_points: usize },
    /// Two ranges cover the same code point. Sorting cannot rescue this; one of them is wrong and
    /// there is no way to tell which.
    Overlap { first_end: usize, next_start: usize },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Refusal::NoColon => write!(f, "an emotes section had no ':' between id and ranges"),
            Refusal::EmptyId => write!(f, "an emotes section had an empty id"),
            Refusal::BadRange => write!(f, "an emote range was not two decimal numbers and a '-'"),
            Refusal::Reversed { start, end } => {
                write!(f, "emote range {start}-{end} ends before it starts")
            }
            Refusal::OutOfRange { end, code_points } => write!(
                f,
                "emote range ends at code point {end} but the body has {code_points}, and the \
                 offsets are inclusive"
            ),
            Refusal::Overlap {
                first_end,
                next_start,
            } => write!(
                f,
                "emote ranges overlap: one ends at {first_end} and the next starts at {next_start}"
            ),
        }
    }
}

/* -------------------------------------------------------------- the counting -- */

/// Every refusal, for the life of the process.
///
/// A GLOBAL IN AN OTHERWISE PURE FILE NEEDS A REASON, and the reason is that the lenient entry
/// point throws the error away by design so that the UI can keep drawing. Without this, a Twitch
/// change that made every line malformed would look exactly like a channel that had stopped using
/// emotes: no error, no log, no difference on screen except missing pictures. Monotonic and
/// `Relaxed`, because nothing branches on it: it is read by a diagnostics line, not by logic.
static REFUSED: AtomicU64 = AtomicU64::new(0);

/// How many lines have had their spans refused since the process started.
pub fn refused_total() -> u64 {
    REFUSED.load(Ordering::Relaxed)
}

/// The one place a `Refusal` is allowed to leave this file, so the count cannot drift from the
/// truth by a path that forgot to increment it.
fn refuse(r: Refusal) -> Refusal {
    REFUSED.fetch_add(1, Ordering::Relaxed);
    r
}

/* --------------------------------------------------------------- the cutting -- */

/// The text of a `/me`, if the body is one. `None` for an ordinary message.
///
/// This is the text the `emotes` offsets are counted from. It is exported because a screen that
/// draws actions the Twitch way, italic and in the sender's colour with no wrapper visible, needs
/// the same substring this file computes, and two implementations of that rule would eventually
/// disagree by one character and nobody would know which was right.
pub fn action_payload(body: &str) -> Option<&str> {
    let (prefix, inner, _) = split_action(body);
    if prefix.is_empty() {
        None
    } else {
        Some(inner)
    }
}

/// Split a body into the CTCP wrapper's opener, the text the offsets address, and the closer.
/// `("", body, "")` when there is no wrapper.
///
/// Both ends of an ACTION body are checked. A body that opens with the prefix and does NOT close
/// with the delimiter is left alone: the closer is what makes it a CTCP frame rather than a message
/// that happens to begin with a control character, and every one of the 42 actions in the capture
/// carries it.
fn split_action(body: &str) -> (&str, &str, &str) {
    let head = ACTION_PREFIX.len();
    if body.len() > head && body.starts_with(ACTION_PREFIX) && body.ends_with(CTCP_DELIM) {
        let tail = body.len() - CTCP_DELIM.len_utf8();
        /* Both cuts are character boundaries by construction: `head` is the end of an all ASCII
         * prefix that `starts_with` has just confirmed, and `tail` is the start of a one byte
         * delimiter that `ends_with` has just confirmed. */
        (&body[..head], &body[head..tail], &body[tail..])
    } else {
        ("", body, "")
    }
}

/// Push a text run, dropping it if it is empty.
///
/// NO SPAN IS EVER EMPTY, and that is a promise the renderer gets to lean on. Two emotes with
/// nothing between them, an emote at offset zero, and an emote that ends the body all produce a
/// zero length gap, and an empty `Text` span for each would be a run that draws nothing, takes
/// layout space in some layouts, and makes every count of "how many runs is this line" wrong.
fn push_text<'a>(out: &mut Vec<Span<'a>>, t: &'a str) {
    if !t.is_empty() {
        out.push(Span::Text(t));
    }
}

/// Cut `body` into runs using the raw value of the `emotes` tag. The typed answer: `Err` means the
/// tag and the body disagree and no honest cut exists.
///
/// `body` is the trailing parameter of the PRIVMSG or USERNOTICE exactly as it arrived, CTCP
/// wrapper and all. Passing [`action_payload`]'s result instead is also correct, and yields the
/// same emote spans without the wrapper's two text runs.
pub fn spans_checked<'a>(body: &'a str, emotes: &'a str) -> Result<Vec<Span<'a>>, Refusal> {
    cut(body, emotes).map_err(refuse)
}

/// Cut `body` into runs, falling back to one plain run over the whole body when the tag is
/// malformed. What a screen calls: the words always arrive, and the refusal is in the count.
pub fn spans<'a>(body: &'a str, emotes: &'a str) -> Vec<Span<'a>> {
    match spans_checked(body, emotes) {
        Ok(v) => v,
        Err(_) => {
            let mut out = Vec::new();
            push_text(&mut out, body);
            out
        }
    }
}

/// The whole of the work. Separate from [`spans_checked`] so that every error path goes through
/// exactly one `refuse` and the counter cannot double count or miss.
fn cut<'a>(body: &'a str, emotes: &'a str) -> Result<Vec<Span<'a>>, Refusal> {
    /* An absent or empty tag is not an error and not a special case worth branching later: it is a
     * body with no emotes in it, which is one plain run, or none at all if the body is empty. */
    if emotes.is_empty() {
        let mut out = Vec::new();
        push_text(&mut out, body);
        return Ok(out);
    }

    let (prefix, inner, suffix) = split_action(body);

    let mut ranges = parse_ranges(emotes)?;

    /* THE TAG IS NOT SORTED AND NOTHING SAYS IT WILL BE. 10 of the 283 tagged lines in the capture
     * arrive out of order, including one whose sections read 71-75, 77-83, 0-5, 29-38. Cutting in
     * tag order walks the cursor to the end of the body and then asks for a slice that starts
     * behind it, which is a reversed range and a panic, or worse, a body reassembled in the wrong
     * order that still looks like words. */
    ranges.sort_unstable_by_key(|&(start, _, _)| start);

    /* CODE POINT INDEX TO BYTE OFFSET, with a terminator entry so `end + 1` always has one: the
     * offsets are inclusive, so the byte after the emote's last code point is what a slice needs,
     * and for an emote that ends the body that is the body's length. Twitch refuses a message over
     * 500 characters, so this table is at most 501 entries and building it per line is cheaper than
     * the string comparisons the caller has already done to get here. */
    let mut at: Vec<usize> = Vec::with_capacity(inner.len() + 1);
    at.extend(inner.char_indices().map(|(byte, _)| byte));
    at.push(inner.len());
    let code_points = at.len() - 1;

    /* VALIDATE EVERY RANGE BEFORE CUTTING ANY OF THEM. A line is refused whole or cut whole: half a
     * line of spans plus an error is a shape the caller would have to think about, and thinking
     * about it is how a mangled body reaches the screen. */
    let mut previous_end: Option<usize> = None;
    for &(start, end, _) in &ranges {
        if end >= code_points {
            return Err(Refusal::OutOfRange { end, code_points });
        }
        if let Some(first_end) = previous_end {
            if start <= first_end {
                return Err(Refusal::Overlap {
                    first_end,
                    next_start: start,
                });
            }
        }
        previous_end = Some(end);
    }

    let mut out = Vec::with_capacity(ranges.len() * 2 + 2);
    push_text(&mut out, prefix);
    let mut cursor = 0usize; // a byte offset into `inner`, never a code point index
    for &(start, end, id) in &ranges {
        let from = at[start];
        let to = at[end + 1];
        push_text(&mut out, &inner[cursor..from]);
        out.push(Span::Emote {
            id,
            name: &inner[from..to],
        });
        cursor = to;
    }
    push_text(&mut out, &inner[cursor..]);
    push_text(&mut out, suffix);
    Ok(out)
}

/// `id:start-end,start-end/id2:start-end` into a flat list. Order is preserved here and fixed by
/// the caller; validation against the body happens there, where the body's length is known.
fn parse_ranges(emotes: &str) -> Result<Vec<(usize, usize, &str)>, Refusal> {
    let mut out: Vec<(usize, usize, &str)> = Vec::new();
    for section in emotes.split('/') {
        let (id, list) = section.split_once(':').ok_or(Refusal::NoColon)?;
        if id.is_empty() {
            return Err(Refusal::EmptyId);
        }
        for range in list.split(',') {
            let (a, b) = range.split_once('-').ok_or(Refusal::BadRange)?;
            /* `parse` is the refusal, not a panic and not a silent zero. A leading '-' lands here
             * as an empty first half, an overflowing number lands here as a parse error, and both
             * are a tag this file will not pretend to understand. */
            let start: usize = a.parse().map_err(|_| Refusal::BadRange)?;
            let end: usize = b.parse().map_err(|_| Refusal::BadRange)?;
            if end < start {
                return Err(Refusal::Reversed { start, end });
            }
            out.push((start, end, id));
        }
    }
    Ok(out)
}

/* ----------------------------------------------------------------- the tests -- */

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Real lines from irc.chat.twitch.tv, captured 2026-09-04 from seven busy channels with an
    /// anonymous login, as `(emotes tag value, body)`. Bodies are byte for byte what arrived after
    /// the `:` of the PRIVMSG or USERNOTICE, CTCP wrapper and invisible characters included.
    ///
    /// NOT ONE OF THESE WAS TYPED BY HAND. A hand written fixture is a fixture that agrees with
    /// whatever the parser happens to do, and the two things this file exists to get right, code
    /// point offsets and the ACTION wrapper, are exactly the two things an author writing a fixture
    /// from the documentation would get wrong in the fixture as well as in the code.
    const CAPTURED: &[(&str, &str)] = &[
        ("80958:0-6,80-86/90969:30-39/9874:74-78,88-92", "\u{1}ACTION sumLove ✧ RESUB HYPE! (ﾉ◕ヮ◕)ﾉ sumBuhblam Mokiz0r stayed on the 1G Squad!! sum1g sumLove sum1g\u{1}"),
        ("1:48-49", "\u{1}ACTION [Trivia] aintnoway_deadass has started a trivia :) Question: A classic 1976 song by Blue Oyster Cult features the advice \"Don't Fear The\" what? | Do !hint for a hint 🤔\u{1}"),
        ("90969:33-42/9874:85-89,99-103/80958:91-97/3689:0-9", "\u{1}ACTION sumCreeper ✧ RESUB HYPE! (ﾉ◕ヮ◕)ﾉ sumBuhblam BaldurKilgannon stayed on the 1G Squad!! sum1g sumLove sum1g\u{1}"),
        ("80996:0-4/90969:28-37/9874:73-77,87-91/80958:79-85", "\u{1}ACTION sumUp ✧ RESUB HYPE! (ﾉ◕ヮ◕)ﾉ sumBuhblam Milllguy stayed on the 1G Squad!! sum1g sumLove sum1g\u{1}"),
        ("52:0-4", "SMOrc hem hm harrr! 🔪"),
        ("425618:21-23,25-27", "his 95 can’t keep up LUL LUL"),
        ("1:48-49", "\u{1}ACTION [Trivia] aintnoway_deadass has started a trivia :) Question: What does the abbreviation \"CAT\" in \"CAT scan\" stand for? | Do !hint for a hint 🤔\u{1}"),
        ("1:48-49", "\u{1}ACTION [Trivia] aintnoway_deadass has started a trivia :) Question: On the classic TV series \"The Brady Bunch,\"what is the name of the Brady family dog? | Do !hint for a hint 🤔\u{1}"),
        ("1:48-49", "\u{1}ACTION [Trivia] aintnoway_deadass has started a trivia :) Question: In the classic version of Monopoly,the two \"utility companies\" are Water Works and the what? | Do !hint for a hint 🤔\u{1}"),
        ("9874:71-75,85-89/80958:77-83/208772:0-5/90969:29-38", "\u{1}ACTION sumLUL ✧ RESUB HYPE! (ﾉ◕ヮ◕)ﾉ sumBuhblam kieer stayed on the 1G Squad!! sum1g sumLove sum1g\u{1}"),
        ("1035663:34-37", "take the day off x you deserve it xqcL ͏"),
        ("1035663:38-41", "take the weekend off x you deserve it xqcL  ͏"),
        ("425618:40-42", "but her feelings aren’t being validated LUL"),
        ("425618:57-59", "Satans off the chains. it’s the only logical explanation LUL"),
        ("58765:34-44", "wtf Dante’s explaining things bro NotLikeThis"),
        ("305954156:76-83", "\u{1}ACTION [Trivia] aintnoway_deadass, you got the answer right! It was \"Led Zeppelin\" PogChamp\u{1}"),
        ("1:48-49", "\u{1}ACTION [Trivia] aintnoway_deadass has started a trivia :) Question: Which British band won the first edition of the Brit Awards in 1977? | Do !hint if you need a \"hint\" forsenCD\u{1}"),
        ("1:48-49", "\u{1}ACTION [Trivia] aintnoway_deadass has started a trivia :) Question: Which Sonic the Hedgehog game was originally supposed to be packaged with Sonic 3, but was cut in half due to time constraints? | Do !hint if you need a \"hint\" forsenCD\u{1}"),
        ("emotesv2_d82f3713d3ca435196838b30a95786a8:0-11/emotesv2_96983d72842246aab21a79d4b212cbe4:13-22,24-33,35-44,46-55,57-66,68-77", "jynxziBounce jynxziPACK jynxziPACK jynxziPACK jynxziPACK jynxziPACK jynxziPACK"),
        ("1:18-19", "@s4boteur_ u good :)"),
        ("emotesv2_b39e577c1ae34476b5980d4c744540fd:17-35/emotesv2_e7c6120fa1a6465fb4a3c01b87fd2bb3:37-54", "@metalheadguy501 smugalanaFrostdance smugalanaFiredance"),
        ("360:38-45", "US is not doing the same shit as Iran FailFish"),
        ("306833763:0-8", "jynxziWTF"),
        ("emotesv2_25cf5d5522e94c6bb2ad183e0f1b51ba:17-29/emotesv2_4b501d0de7984ac8af7b710c21b78acc:31-47", "@metalheadguy501 smugalanaWave smugalanaTailfire"),
        ("41:0-7", "Kreygasm dr peppa"),
        ("emotesv2_c4611ebaf90348edbeda9b80cfa5d4fd:0-10,12-22,24-34,36-46,48-58", "jynxziRAHHH jynxziRAHHH jynxziRAHHH jynxziRAHHH jynxziRAHHH"),
        ("emotesv2_76bf827630634dcbbf60a045bfe741b7:0-11", "annytfWokege"),
        ("305288711:40-48", "Stream goes live at NOON EST / 5 pm GMT lirikOSVN (Thursdays are off)"),
        ("425618:0-2,4-6,8-10", "LUL LUL LUL"),
        ("emotesv2_fcb03e2255774abaa26af34b2b89f821:12-18", "@kisuklessa fsmLove"),
        ("425618:3-5", "no LUL"),
        ("emotesv2_13b6dd7f3a3146ef8dc10f66d8b42a96:0-12", "TwitchConHYPE"),
        ("425618:0-2", "LUL"),
        ("emotesv2_3e0edbfc0ed04277b18fcd108718bc51:0-11,13-24,26-37", "wesal5Freaky wesal5Freaky wesal5Freaky"),
        ("425618:26-28", "I thought that was a dude LUL"),
        ("25:0-4", "Kappa"),
        ("92:0-6", "PMSTwin"),
        ("425618:71-73", "parasite making zombies stronger now what game does that remind you of LUL"),
        ("555555558:16-17", "dont yell at me :("),
        ("425618:0-2,4-6", "LUL LUL"),
        ("emotesv2_c414a4d445764f3da3a801ba18ad7306:0-12", "databasePlorp"),
        ("1035663:38-41", "take the weekend off x you deserve it xqcL"),
        ("743904:0-9,11-20,22-31", "PokPikachu PokPikachu PokPikachu"),
        ("58765:0-10", "NotLikeThis"),
        ("425618:20-22", "that thing can vote LUL"),
        ("28087:0-6", "WutFace"),
        ("9874:0-4", "sum1g"),
        ("emotesv2_442c55a4fa5549319602a4d25d3d6148:0-10", "tiuuieTired"),
        ("emotesv2_239d75a88d9748c9b00f74d4051b21ee:0-10", "velcuzAight"),
        ("33:26-33", "gangsta rap made me do it DansGame"),
        ("120232:0-6", "TriHard TeaTime"),
        /* Out of tag order across two ids, and every emote adjacent to another with one space
         * between: the shape that catches both a missing sort and an empty gap span. */
        ("emotesv2_dcd06b30a5c24f6eb871e8f5edbd44f7:0-8,10-18,44-52,54-62/196892:20-30,32-42", "DinoDance DinoDance TwitchUnity TwitchUnity DinoDance DinoDance"),
        ("emotesv2_f3d6812f247a40c7b2d1b92e9b392646:0-17,39-56/emotesv2_d13005ca395049879e8c184f1a68ac1f:19-37,58-76", "smugalanaHappifire smugalanaHappifrost smugalanaHappifire smugalanaHappifrost"),
        /* The one line from irc_anon.txt rather than irc_busy.txt, and a USERNOTICE rather than a
         * PRIVMSG: the same tag on a different command, from a different session, on the channel
         * this app actually watches. */
        ("160392:84-91", "180sec ad break starting. Thank you for sticking around and supporting the channel! ThankEgg"),
        ("", "that’s really how it is"),
        ("", "WhatDoesHeEvenDo  ͏"),
        ("", "My new Webcomic, Beyond the Vale is out next week and it’s sick. Sign up at https://btvcomic.com/asmongold for free stuff! #BeyondTheVale"),
        ("", "chat make sure he doesn’t  miss the shards"),
        ("", "chủ"),
        ("", "SON ͏"),
        ("", "SNEAKO SOUNDS SO DUMB WHEN HE THINKS HE IS RIGHT"),
        ("", "Play yi lil bro"),
        ("", "do the JP voices match at least?"),
        ("", "I do"),
        ("", "MUGA TAGLIAFICO"),
        ("", "LO"),
    ];

    /// One captured line used by name in several tests below, so that an edit to the corpus cannot
    /// silently move what those tests are about. Byte for byte from irc_busy.txt.
    const CANT_KEEP_UP: (&str, &str) = ("425618:21-23,25-27", "his 95 can’t keep up LUL LUL");

    /// The RESUB action: a CTCP wrapper, four multi byte characters before the second emote, and a
    /// tag whose sections are out of order.
    const RESUB: (&str, &str) = (
        "80958:0-6,80-86/90969:30-39/9874:74-78,88-92",
        "\u{1}ACTION sumLove ✧ RESUB HYPE! (ﾉ◕ヮ◕)ﾉ sumBuhblam Mokiz0r stayed on the 1G Squad!! sum1g sumLove sum1g\u{1}",
    );

    fn rebuilt(spans: &[Span<'_>]) -> String {
        spans.iter().map(|s| s.text()).collect()
    }

    /// THE SPANS OF EVERY REAL BODY REBUILD THAT BODY EXACTLY.
    ///
    /// The defect: any arithmetic slip at all. A byte offset used as a code point index, an
    /// exclusive end treated as inclusive, a gap run that starts one character late, a tail run
    /// that is forgotten when the last emote does not end the body. Each of those ships a chat
    /// window that drops or repeats a piece of somebody's sentence, on some lines and not others,
    /// which is the kind of bug that gets reported as "chat looks weird sometimes" and never
    /// reproduced. This is the assertion that makes every other test in the file honest.
    ///
    /// COULD IT PASS TRIVIALLY? Yes: a function that always returned one `Text` span over the whole
    /// body would satisfy the concatenation on its own. So the emote spans are counted too, and the
    /// count is compared against what the capture actually contains.
    #[test]
    fn the_spans_of_every_captured_body_rebuild_that_body_exactly() {
        let mut emote_spans = 0usize;
        for &(emotes, body) in CAPTURED {
            let cut = spans_checked(body, emotes)
                .unwrap_or_else(|e| panic!("captured line refused ({e}): tag {emotes:?}"));
            assert_eq!(
                rebuilt(&cut),
                body,
                "spans do not rebuild the body\n  tag {emotes:?}\n  body {body:?}\n  spans {cut:?}"
            );
            emote_spans += cut.iter().filter(|s| s.is_emote()).count();
        }
        assert!(
            emote_spans >= 90,
            "the corpus produced {emote_spans} emote spans, so the concatenation above was proving \
             almost nothing: the capture has 98 emote ranges in these lines and if that number has \
             collapsed then the cutter is returning one plain run and passing"
        );
    }

    /// AN EMOTE ID NAMES THE SAME TEXT IN EVERY MESSAGE IT APPEARS IN.
    ///
    /// The defect this catches is the CTCP ACTION offset, and it is the defect most likely to ship:
    /// a `/me` body arrives wrapped as `\u{1}ACTION <text>\u{1}` and the offsets are counted from
    /// the start of `<text>`, which no Twitch document says. Resolving them against the raw body
    /// instead produces spans that still rebuild the body perfectly and still look like plausible
    /// runs, so the property test above cannot see it. What it produces is emote id 80958 naming
    /// "sumLove" on one line and "! sum1g" on another, and a chat window that draws the wrong
    /// picture in the middle of a word. Measured over the whole capture: 28 such conflicts against
    /// the raw body, zero against the unwrapped text.
    ///
    /// COULD IT PASS TRIVIALLY? Only if no id appeared twice, so the number of ids that appear in
    /// more than one captured message is asserted as well.
    #[test]
    fn an_emote_id_names_the_same_text_in_every_message_it_appears_in() {
        let mut names: BTreeMap<&str, &str> = BTreeMap::new();
        let mut messages_per_id: BTreeMap<&str, usize> = BTreeMap::new();
        for &(emotes, body) in CAPTURED {
            let cut = spans(body, emotes);
            let mut here: BTreeMap<&str, ()> = BTreeMap::new();
            for s in &cut {
                if let Span::Emote { id, name } = *s {
                    if let Some(seen) = names.get(id) {
                        assert_eq!(
                            *seen, name,
                            "emote id {id} is {seen:?} in one captured line and {name:?} in this \
                             one, so the offsets are being counted from the wrong place: tag \
                             {emotes:?} body {body:?}"
                        );
                    }
                    names.insert(id, name);
                    here.insert(id, ());
                }
            }
            for id in here.keys() {
                *messages_per_id.entry(id).or_insert(0) += 1;
            }
        }
        let shared = messages_per_id.values().filter(|&&n| n > 1).count();
        assert!(
            shared >= 6,
            "only {shared} emote ids appear in more than one captured line, so this test compared \
             almost nothing against anything"
        );
    }

    /// A CODE POINT INDEX IS NOT A BYTE INDEX, AND THIS REAL BODY PROVES IT.
    ///
    /// The defect: slicing the body by the tag's numbers. "his 95 can’t keep up LUL LUL" carries a
    /// three byte right single quote at code point 10, so the first LUL sits at code points 21..23
    /// and at bytes 23..26. A byte slicer paints the emote over "p L" and leaves the real "LUL" in
    /// the text run beside it, on every message anybody has ever typed an apostrophe into.
    #[test]
    fn a_code_point_index_is_not_a_byte_index_and_this_captured_body_proves_it() {
        let (emotes, body) = CANT_KEEP_UP;
        let cut = spans(body, emotes);
        assert_eq!(
            cut,
            vec![
                Span::Text("his 95 can’t keep up "),
                Span::Emote {
                    id: "425618",
                    name: "LUL"
                },
                Span::Text(" "),
                Span::Emote {
                    id: "425618",
                    name: "LUL"
                },
            ]
        );
        /* The assertion that stops the one above being a coincidence: the naive slice really does
         * differ here, so the test would fail against a byte offset implementation rather than
         * happening to agree with it. */
        assert_eq!(
            &body[21..24],
            "p L",
            "this fixture was chosen because byte offsets and code point offsets disagree on it; \
             if they now agree the fixture has been edited and proves nothing"
        );
    }

    /// AN EMOTE WHOSE BYTE OFFSET LANDS INSIDE A CHARACTER IS STILL CUT AT THE CHARACTER.
    ///
    /// The defect: the same one as above, in its louder form. In the RESUB action the emote
    /// "sumBuhblam" begins at code point 30, which is byte 42, and byte 30 is in the middle of the
    /// three byte "◕" of the kaomoji. A byte slicer does not draw the wrong text here, it panics on
    /// a character boundary and takes the reader thread with it. Four ranges in the capture do this.
    #[test]
    fn an_emote_whose_byte_offset_lands_inside_a_character_is_still_cut_at_the_character() {
        let (emotes, body) = RESUB;
        let inner = action_payload(body).expect("the RESUB fixture is a CTCP ACTION");
        assert!(
            !inner.is_char_boundary(30),
            "this fixture was chosen because byte 30 of the action text is inside a character; if \
             that is no longer true the fixture has been edited and the test proves nothing"
        );
        let cut = spans(body, emotes);
        assert_eq!(
            cut,
            vec![
                Span::Text("\u{1}ACTION "),
                Span::Emote {
                    id: "80958",
                    name: "sumLove"
                },
                Span::Text(" ✧ RESUB HYPE! (ﾉ◕ヮ◕)ﾉ "),
                Span::Emote {
                    id: "90969",
                    name: "sumBuhblam"
                },
                Span::Text(" Mokiz0r stayed on the 1G Squad!! "),
                Span::Emote {
                    id: "9874",
                    name: "sum1g"
                },
                Span::Text(" "),
                Span::Emote {
                    id: "80958",
                    name: "sumLove"
                },
                Span::Text(" "),
                Span::Emote {
                    id: "9874",
                    name: "sum1g"
                },
                Span::Text("\u{1}"),
            ]
        );
    }

    /// THE RANGES OF A LINE ARE SORTED BEFORE THEY ARE CUT.
    ///
    /// The defect: trusting the tag's order. 10 of the 283 tagged lines in the capture are out of
    /// order, and this is one of them: the first id's ranges are 0-8, 10-18, 44-52, 54-62 and the
    /// second id's are 20-30, 32-42, so a cutter that walks the tag in order has its cursor at code
    /// point 63 when it is asked for the run starting at 20. That is a reversed slice, which panics,
    /// or, in an implementation that clamps instead, a body reassembled with two emotes in the
    /// wrong half of the sentence.
    #[test]
    fn the_ranges_of_a_line_are_sorted_into_body_order_before_they_are_cut() {
        let emotes =
            "emotesv2_dcd06b30a5c24f6eb871e8f5edbd44f7:0-8,10-18,44-52,54-62/196892:20-30,32-42";
        let body = "DinoDance DinoDance TwitchUnity TwitchUnity DinoDance DinoDance";
        let cut = spans(body, emotes);
        let names: Vec<&str> = cut
            .iter()
            .filter(|s| s.is_emote())
            .map(|s| s.text())
            .collect();
        assert_eq!(
            names,
            vec![
                "DinoDance",
                "DinoDance",
                "TwitchUnity",
                "TwitchUnity",
                "DinoDance",
                "DinoDance"
            ],
            "the emotes came out in tag order rather than body order"
        );
        assert_eq!(rebuilt(&cut), body);
        /* Without the sort the tag's own order would have been DinoDance, DinoDance, DinoDance,
         * DinoDance, TwitchUnity, TwitchUnity, which is a different vector from the one above, so
         * the assertion is not satisfiable by accident. */
    }

    /// AN EMPTY EMOTES TAG IS EXACTLY ONE PLAIN RUN.
    ///
    /// The defect: a cutter that returns an empty vector for the common case, so a channel with no
    /// emotes in a line draws a blank message. 5,680 of the 5,963 PRIVMSG lines in the capture have
    /// an empty tag, so this is not the edge case, it is the case.
    #[test]
    fn an_empty_emotes_tag_is_exactly_one_plain_run_over_the_whole_body() {
        let body = "My new Webcomic, Beyond the Vale is out next week and it’s sick. Sign up at \
                    https://btvcomic.com/asmongold for free stuff! #BeyondTheVale";
        let cut = spans(body, "");
        assert_eq!(cut, vec![Span::Text(body)]);
    }

    /// AN EMPTY BODY IS NO RUNS AT ALL, NOT ONE EMPTY RUN.
    ///
    /// The defect: an empty `Text` span reaching the renderer. The capture has no zero length body,
    /// so this case is constructed rather than captured, and it is constructed by taking the empty
    /// tag of a real line and the shortest body the protocol permits. An empty run is a thing the
    /// renderer has to remember to skip, in every renderer, forever.
    #[test]
    fn an_empty_body_produces_no_spans_rather_than_one_empty_span() {
        assert_eq!(spans("", ""), Vec::<Span<'_>>::new());
        assert_eq!(rebuilt(&spans("", "")), "");
    }

    /// NO SPAN IS EVER EMPTY, OVER THE WHOLE CAPTURE.
    ///
    /// The defect: zero length runs emitted at the seams. An emote at code point zero leaves an
    /// empty run before it, an emote that ends the body leaves an empty run after it, and two
    /// adjacent emotes leave one between. 162 captured lines start with an emote and 247 end with
    /// one, so a cutter without this rule ships hundreds of empty runs an hour.
    #[test]
    fn no_span_in_any_captured_line_is_empty() {
        for &(emotes, body) in CAPTURED {
            for s in spans(body, emotes) {
                assert!(
                    !s.text().is_empty(),
                    "empty span in tag {emotes:?} body {body:?}"
                );
            }
        }
    }

    /// AN EMOTE THAT ENDS THE BODY IS THE LAST SPAN AND KEEPS ITS LAST CHARACTER.
    ///
    /// The defect: treating the inclusive end as exclusive. "wtf Dante’s explaining things bro
    /// NotLikeThis" ends at code point 44 and the range is 34-44, so an off by one drops the final
    /// "s" into a trailing text run and draws the emote as "NotLikeThi". The multi byte apostrophe
    /// earlier in the body means the byte length and the code point length differ too, so this also
    /// catches a terminator entry computed from `body.len()` instead of the character table.
    #[test]
    fn an_emote_that_ends_the_body_keeps_its_last_character_and_leaves_no_tail() {
        let body = "wtf Dante’s explaining things bro NotLikeThis";
        let cut = spans(body, "58765:34-44");
        assert_eq!(
            cut,
            vec![
                Span::Text("wtf Dante’s explaining things bro "),
                Span::Emote {
                    id: "58765",
                    name: "NotLikeThis"
                },
            ]
        );
        assert_eq!(body.chars().count(), 45);
        assert_ne!(body.len(), body.chars().count());
    }

    /// AN EMOTE AT CODE POINT ZERO LEAVES NOTHING BEFORE IT.
    ///
    /// The defect: an empty leading text run, or, in a cutter that starts its cursor at one, the
    /// first character of the emote eaten. "LUL" alone is a whole captured message and the range is
    /// 0-2, which is the entire body.
    #[test]
    fn an_emote_at_code_point_zero_is_the_first_span_and_may_be_the_only_one() {
        assert_eq!(
            spans("LUL", "425618:0-2"),
            vec![Span::Emote {
                id: "425618",
                name: "LUL"
            }]
        );
    }

    /// AN EMOTESV2 ID SURVIVES AS THE STRING IT WAS.
    ///
    /// The defect: parsing the id as a number. 78 of the 469 emote ranges in the capture carry ids
    /// of the form `emotesv2_<32 hex digits>`, which is every follower and channel emote on the
    /// modern service. An integer id drops all of them, and it drops them by failing to parse,
    /// which in a hurry becomes an `unwrap_or(0)` and a chat full of the wrong picture.
    #[test]
    fn an_emotesv2_id_survives_as_the_string_it_was_rather_than_a_number() {
        let id = "emotesv2_c4611ebaf90348edbeda9b80cfa5d4fd";
        let cut = spans(
            "jynxziRAHHH jynxziRAHHH jynxziRAHHH jynxziRAHHH jynxziRAHHH",
            "emotesv2_c4611ebaf90348edbeda9b80cfa5d4fd:0-10,12-22,24-34,36-46,48-58",
        );
        let ids: Vec<&str> = cut
            .iter()
            .filter_map(|s| match *s {
                Span::Emote { id, .. } => Some(id),
                Span::Text(_) => None,
            })
            .collect();
        assert_eq!(ids, vec![id; 5]);
        assert!(
            id.parse::<u64>().is_err(),
            "the fixture id has become numeric, so this test no longer proves the id is kept as text"
        );
    }

    /// THE ACTION PAYLOAD IS THE TEXT THE OFFSETS ADDRESS, AND THE WRAPPER IS STILL DRAWN.
    ///
    /// The defect: two answers to "what is the body of a /me". A screen that strips the wrapper
    /// itself, by hand, and then hands the stripped text to a cutter that strips it again, loses
    /// eight characters of the message. Exporting the one rule is what stops that, and this test is
    /// what stops the two entry points drifting apart.
    #[test]
    fn the_action_payload_and_the_whole_body_give_the_same_emotes() {
        let (emotes, body) = RESUB;
        let inner = action_payload(body).expect("the RESUB fixture is a CTCP ACTION");
        assert!(!inner.starts_with(ACTION_PREFIX) && !inner.ends_with(CTCP_DELIM));

        let whole = spans(body, emotes);
        let payload = spans(inner, emotes);
        let emotes_of = |v: &Vec<Span<'_>>| -> Vec<(String, String)> {
            v.iter()
                .filter_map(|s| match *s {
                    Span::Emote { id, name } => Some((id.to_owned(), name.to_owned())),
                    Span::Text(_) => None,
                })
                .collect()
        };
        assert_eq!(emotes_of(&whole), emotes_of(&payload));
        assert_eq!(rebuilt(&whole), body);
        assert_eq!(rebuilt(&payload), inner);
        /* And the difference between the two is exactly the wrapper, nothing else. */
        assert_eq!(whole.len(), payload.len() + 2);
    }

    /// A BODY THAT OPENS LIKE AN ACTION AND DOES NOT CLOSE LIKE ONE IS NOT AN ACTION.
    ///
    /// The defect: stripping eight characters off a message that merely begins with a control
    /// character, which shifts every emote in it by eight and paints pictures over the wrong words.
    /// Built by taking a real action body and deleting its trailing delimiter, which is the only
    /// edit made, because the capture has no such line: all 42 of its actions are well formed.
    #[test]
    fn a_body_that_opens_like_an_action_but_does_not_close_like_one_is_left_alone() {
        let truncated = "\u{1}ACTION sumLove and then the connection dropped";
        assert_eq!(action_payload(truncated), None);
        let cut = spans(truncated, "80958:0-6");
        /* Offsets now count from the start of the body, so 0-6 is the control character and
         * "ACTION", not "sumLove". That is what the tag literally says about this body, and this
         * file does not guess otherwise. */
        assert_eq!(rebuilt(&cut), truncated);
        assert_eq!(
            cut.first(),
            Some(&Span::Emote {
                id: "80958",
                name: "\u{1}ACTION"
            })
        );
    }

    /// OVERLAPPING RANGES ARE REFUSED RATHER THAN MANGLED.
    ///
    /// Built from the captured line "his 95 can’t keep up LUL LUL" with tag `425618:21-23,25-27` by
    /// changing the second range's start from 25 to 23, which is the last code point of the first
    /// range. Nothing else was touched. The capture contains no overlapping tag, which is why this
    /// case has to be built and why it has to exist: the defect is a cutter that walks its cursor
    /// backwards, slices `23..26` when the cursor is already at 26, and panics inside the reader
    /// thread on a line of somebody else's chat.
    #[test]
    fn overlapping_ranges_are_refused_rather_than_cut_into_a_mangled_body() {
        let (_, body) = CANT_KEEP_UP;
        let err = spans_checked(body, "425618:21-23,23-27").expect_err("overlap must be refused");
        assert_eq!(
            err,
            Refusal::Overlap {
                first_end: 23,
                next_start: 23
            }
        );
    }

    /// A RANGE ONE PAST THE END OF THE BODY IS REFUSED.
    ///
    /// Built from the same captured line by changing the final range's end from 27 to 28. The body
    /// is 28 code points, so 27 is its last and 28 is one too far. This is the exact off by one
    /// that an exclusive reading of the tag produces, and a cutter that checks `end > code_points`
    /// instead of `end >= code_points` passes every other test in this file and then indexes the
    /// terminator entry of the character table for a code point that does not exist.
    #[test]
    fn a_range_that_ends_one_code_point_past_the_body_is_refused() {
        let (_, body) = CANT_KEEP_UP;
        assert_eq!(body.chars().count(), 28);
        let err = spans_checked(body, "425618:21-23,25-28").expect_err("out of range must refuse");
        assert_eq!(
            err,
            Refusal::OutOfRange {
                end: 28,
                code_points: 28
            }
        );
        /* And the last valid index is accepted, so the bound is not merely tight, it is correct. */
        assert!(spans_checked(body, "425618:21-23,25-27").is_ok());
    }

    /// A REVERSED, UNPARSEABLE OR HEADLESS TAG IS REFUSED BY SHAPE, NOT BY PANIC.
    ///
    /// Each of these is the captured tag `425618:21-23` with one documented edit: the ends swapped,
    /// the hyphen replaced by an underscore, the colon removed, and the id deleted. The defect is
    /// house style rather than protocol: an `unwrap` on `parse`, or a subtraction that underflows,
    /// turns a rewritten tag from a proxy or a future Twitch change into a crash in a background
    /// thread that the UI never hears about.
    #[test]
    fn a_reversed_or_unparseable_or_headless_tag_is_refused_by_shape() {
        let (_, body) = CANT_KEEP_UP;
        assert_eq!(
            spans_checked(body, "425618:23-21"),
            Err(Refusal::Reversed { start: 23, end: 21 })
        );
        assert_eq!(spans_checked(body, "425618:21_23"), Err(Refusal::BadRange));
        assert_eq!(spans_checked(body, "425618:-1-3"), Err(Refusal::BadRange));
        assert_eq!(
            spans_checked(body, "425618:99999999999999999999999999-3"),
            Err(Refusal::BadRange)
        );
        assert_eq!(spans_checked(body, "42561821-23"), Err(Refusal::NoColon));
        assert_eq!(spans_checked(body, "425618:21-23/"), Err(Refusal::NoColon));
        assert_eq!(spans_checked(body, ":21-23"), Err(Refusal::EmptyId));
    }

    /// A REFUSED LINE STILL SHOWS EVERY WORD THAT WAS SAID.
    ///
    /// The defect: a malformed tag swallowing the message. The tag decides where pictures go, not
    /// whether the sentence exists, so a line this file will not cut is a line drawn as plain text.
    /// A cutter that returns an empty vector on refusal hides a chatter's words behind a bug in
    /// Twitch's tag, and the reader has no way to know a message was even sent.
    #[test]
    fn a_refused_line_is_still_drawn_as_its_whole_body_in_plain_text() {
        let (_, body) = CANT_KEEP_UP;
        let cut = spans(body, "425618:21-23,23-27");
        assert_eq!(cut, vec![Span::Text(body)]);
        assert_eq!(rebuilt(&cut), body);
    }

    /// REFUSALS ARE COUNTED, SO A MALFORMED LINE IS NEVER SILENTLY DROPPED.
    ///
    /// The defect: the lenient entry point throws the error away by design, so without a count a
    /// change at Twitch that made every tag malformed would be indistinguishable from a chat that
    /// had stopped using emotes. Nothing on screen, nothing in the log, and the first report is
    /// "emotes stopped working" with no way to tell whether it is the tag, the parser or the CDN.
    ///
    /// The assertion is a strict increase rather than an exact total, because the test harness runs
    /// tests in parallel threads against one process wide counter and an exact total would be a
    /// flake, which is a worse test than no test.
    #[test]
    fn a_refused_line_increments_the_process_wide_refusal_count() {
        let (_, body) = CANT_KEEP_UP;
        let before = refused_total();
        let _ = spans(body, "425618:21-23,23-27");
        assert!(
            refused_total() > before,
            "the refusal was not counted, so the lenient path drops malformed lines in silence"
        );
    }

    /// NO EMOTE NAME IN THE CAPTURE CONTAINS WHITESPACE.
    ///
    /// The defect this catches and the rebuild property cannot: a uniform shift. Add one to every
    /// start and every end and the spans still tile the body perfectly, still concatenate back to
    /// it, and still look like a sensible cut. What they no longer do is line up with words. Twitch
    /// emote codes cannot contain whitespace, so a span that has swallowed a space is a cut that has
    /// slid, and 469 of 469 ranges in the capture agree with that rule.
    ///
    /// COULD IT PASS TRIVIALLY? If no emote spans were produced at all, so the number checked is
    /// asserted too.
    ///
    /// The counter is deliberately not asserted here. A well formed line cannot touch it, because
    /// `spans_checked` is `cut(..).map_err(refuse)` and the only call to `refuse` is on that one
    /// line, but the assertion that would say so out loud is an exact count over a process wide
    /// static, and the harness runs these tests in parallel with the ones that refuse on purpose.
    #[test]
    fn no_emote_name_in_the_capture_contains_whitespace() {
        let mut checked = 0usize;
        for &(emotes, body) in CAPTURED {
            let cut = spans_checked(body, emotes)
                .unwrap_or_else(|e| panic!("captured line refused ({e}): tag {emotes:?}"));
            for s in &cut {
                if let Span::Emote { id, name } = *s {
                    assert!(
                        !name.chars().any(char::is_whitespace),
                        "emote {id} was cut as {name:?}, which contains whitespace, so the offsets \
                         have slid off the words: tag {emotes:?} body {body:?}"
                    );
                    checked += 1;
                }
            }
        }
        assert!(
            checked >= 90,
            "only {checked} emote names were checked, so this test proved almost nothing"
        );
    }

    /// ONE ID TWICE IN ONE TAG IS TWO EMOTES, NOT ONE.
    ///
    /// The defect: keying the parsed ranges by id, which is the natural shape if the tag is read as
    /// a map. "LUL LUL LUL" is one captured message with `425618:0-2,4-6,8-10`, and a map keyed by
    /// id keeps whichever range it saw last and draws one emote where three were typed.
    #[test]
    fn one_id_with_three_ranges_produces_three_emote_spans() {
        let cut = spans("LUL LUL LUL", "425618:0-2,4-6,8-10");
        assert_eq!(cut.iter().filter(|s| s.is_emote()).count(), 3);
        assert_eq!(cut.len(), 5, "three emotes and the two spaces between them");
        assert_eq!(rebuilt(&cut), "LUL LUL LUL");
    }

    /// AN INVISIBLE CHARACTER AFTER AN EMOTE IS STILL PART OF THE BODY.
    ///
    /// The defect: trimming. "take the day off x you deserve it xqcL ͏" ends with a space and a
    /// combining grapheme joiner, which the chatter added to defeat Twitch's duplicate message
    /// filter. A cutter that trims the tail run away, or that treats a run of invisible characters
    /// as nothing, breaks the rebuild property on a message shape that is extremely common in the
    /// capture and would break it only on those messages.
    #[test]
    fn an_invisible_character_after_an_emote_is_kept_in_the_trailing_run() {
        let body = "take the day off x you deserve it xqcL ͏";
        let cut = spans(body, "1035663:34-37");
        assert_eq!(rebuilt(&cut), body);
        assert_eq!(
            cut.last(),
            Some(&Span::Text(" \u{34f}")),
            "the trailing run must carry the space and the combining grapheme joiner"
        );
    }
}
