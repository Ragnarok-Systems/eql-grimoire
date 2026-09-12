//! IRCv3 message tags: the blob between the leading '@' and the first space of a Twitch chat line.
//!
//! WHAT THIS FILE OWNS, AND WHAT IT REFUSES TO OWN. It takes one already-cut tag blob and answers
//! `get("display-name")`. It does not split lines, does not know what a PRIVMSG is, does not touch
//! a socket, and has no dependency past `std`. That boundary is the point: the reader thread is
//! where the timeouts and the reconnects live, and none of that belongs anywhere near the one piece
//! of this feature that has to be exactly right on every single line.
//!
//! WHY THE ESCAPES ARE THE WHOLE JOB. A tag value cannot contain a raw space or a raw semicolon,
//! because those two bytes are the wire's own framing. IRCv3 gives five escapes so a value can
//! carry them anyway: `\:` is a semicolon, `\s` a space, `\\` a backslash, `\r` a carriage return,
//! `\n` a line feed. In the 2026-09-04 capture of seven busy channels, 1,762 backslashes appear
//! inside tag blobs and every one of them is `\s`. They are not decoration. This is a real value,
//! verbatim, from irc_busy.txt line 128:
//!
//! ```text
//! system-msg=Mokiz0r\ssubscribed\swith\sPrime.\sThey've\ssubscribed\sfor\s39\smonths!
//! ```
//!
//! A parser that skips the unescape step puts `Mokiz0r\ssubscribed\swith\sPrime.` on screen, in
//! the one message type a viewer is most likely to read carefully, and it does it 55 times per
//! 6,000 lines. That is the defect this module exists to prevent.
//!
//! WHY EMPTY IS NOT MISSING. 24,540 of the 104,442 tags in that capture have an empty value, and
//! the very last tag on every PRIVMSG Twitch sends is `user-type=` with nothing after it. `emotes=`
//! means "this message contains no emotes"; a missing `emotes` tag would mean "the server did not
//! tell us", which is a different claim and, on Twitch, means the line came through a path that
//! stripped tags. So `get` answers `Some("")` for the first and `None` for the second, and no
//! caller is ever handed a value that has quietly collapsed the two.
//!
//! WHY NOTHING HERE INDEXES BY POSITION. Order is not a contract, and the capture proves it rather
//! than the specification merely allowing it: `room-id` appears at nine different offsets across
//! the capture (offset 1 on a CLEARCHAT, 3 on a ROOMSTATE, 11 on a PRIVMSG, up to 20 on a reply
//! with a full thread-parent block), and 19 distinct key sequences occur across four commands. The
//! order is not even alphabetical: 30 USERNOTICE lines send `msg-param-sub-plan-name` BEFORE
//! `msg-param-sub-plan`, which rules out a binary search as well as a fixed offset.
//!
//! WHY A VEC AND A LINEAR SCAN RATHER THAN A HASHMAP. The measured maximum is 26 tags on one line
//! and the longest blob in 2.5 MB is 971 bytes. Hashing 26 short keys and heap-allocating a table
//! per message costs more than 26 pointer-length string compares, and the Vec keeps the wire order,
//! which a HashMap would throw away and which is the only thing that makes an anomaly like a
//! duplicate key legible when it turns up in a log.
//!
//! WHY THE KEYS BORROW AND THE VALUES ARE Cow. A key is a slice of the blob, always: the IRCv3
//! grammar has no escapes in a key, so there is never anything to expand and never a reason to
//! allocate. A value borrows too, until an escape is actually found. Measured by running this
//! parser over the whole capture: 245 of the 104,442 values carry a backslash, 1,762 backslashes
//! between them, so 99.8 percent of values are handed back as a slice and a parser that returns
//! `String` allocates a hundred thousand times to make a hundred thousand copies of bytes it is
//! already holding.

use std::borrow::Cow;
use std::fmt;

/* ------------------------------------------------------------------ refusals -- */

/// Why a whole tag blob could not be read.
///
/// Every variant here is a FRAMING fault, which is why the answer is a refusal rather than a
/// best effort: the caller handed over something that is not a tag blob, and the honest response
/// is to say so and count it. Faults inside a single tag (an empty key, a duplicate, an escape
/// nobody recognises) are the opposite case and are handled the opposite way: the rest of the
/// line still parses and the anomaly lands in [`TagDiagnostics`], because losing a whole chat
/// message over one weird tag is a worse outcome than rendering it with one tag missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagRefusal {
    /// Nothing between the '@' and the space. IRCv3 has no such message and Twitch has never sent
    /// one; an empty blob means the caller found an '@' where there were no tags.
    Empty,
    /// The blob still has its leading '@'. This is the slice-off-by-one, `&line[..sp]` where
    /// `&line[1..sp]` was meant, and it is worth its own variant because the failure it causes is
    /// silent: every key becomes `@badge-info`, `get("badge-info")` answers `None`, and chat
    /// renders with no names, no badges and no colours while nothing anywhere reports an error.
    LeadingAt,
    /// A space inside the blob, at this byte offset. The tag section ends at the first space by
    /// definition, so a space inside it means the caller passed more of the line than the tags.
    /// Parsing on regardless is how `user-type` ends up holding the entire message body.
    EmbeddedSpace { at: usize },
    /// A NUL, CR or LF inside the blob, at this byte offset. IRC line framing cannot deliver any
    /// of the three, so their presence means the framing above this module is broken and the
    /// contents cannot be trusted to be one line's worth of tags.
    ControlByte { at: usize, byte: u8 },
    /// Separators but not one usable key, for example ";;;". There is nothing to look up, and
    /// answering `None` to every query would be indistinguishable from a healthy line that simply
    /// lacks the tag being asked for.
    NoUsableTags,
}

impl fmt::Display for TagRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TagRefusal::Empty => write!(f, "the tag blob was empty"),
            TagRefusal::LeadingAt => write!(
                f,
                "the tag blob still carries its leading '@': the caller kept the sigil"
            ),
            TagRefusal::EmbeddedSpace { at } => write!(
                f,
                "the tag blob has a space at byte {at}: the caller did not cut the line at the \
                 first space"
            ),
            TagRefusal::ControlByte { at, byte } => write!(
                f,
                "the tag blob has byte 0x{byte:02x} at byte {at}, which IRC line framing cannot \
                 deliver"
            ),
            TagRefusal::NoUsableTags => {
                write!(f, "the tag blob held separators but not one non-empty key")
            }
        }
    }
}

impl std::error::Error for TagRefusal {}

/// A running count of refused blobs, for the reader thread to report.
///
/// A reader that answers a refusal with `continue` turns a framing regression into "chat looks
/// quiet", which is the hardest class of bug to find because nothing is on fire. This is the
/// smallest thing that stops that: the thread keeps one of these behind its mutex, the UI can ask
/// for it, and a log line every so often names the kind. Kinds are counted separately on purpose,
/// because "3,000 leading-at" and "3,000 control-byte" call for entirely different fixes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TagRefusalTally {
    pub empty: u64,
    pub leading_at: u64,
    pub embedded_space: u64,
    pub control_byte: u64,
    pub no_usable_tags: u64,
}

impl TagRefusalTally {
    /// Add one refusal. Saturating because a tally that wraps to zero after a long session would
    /// report a healthy reader at exactly the moment the reader is at its least healthy.
    pub fn record(&mut self, refusal: &TagRefusal) {
        let slot = match refusal {
            TagRefusal::Empty => &mut self.empty,
            TagRefusal::LeadingAt => &mut self.leading_at,
            TagRefusal::EmbeddedSpace { .. } => &mut self.embedded_space,
            TagRefusal::ControlByte { .. } => &mut self.control_byte,
            TagRefusal::NoUsableTags => &mut self.no_usable_tags,
        };
        *slot = slot.saturating_add(1);
    }

    /// How many blobs have been refused in total.
    pub fn total(&self) -> u64 {
        self.empty
            .saturating_add(self.leading_at)
            .saturating_add(self.embedded_space)
            .saturating_add(self.control_byte)
            .saturating_add(self.no_usable_tags)
    }

    /// One line naming every kind, for a log or a diagnostics pane. Allocates, so call it when
    /// something is being reported rather than once per message.
    pub fn describe(&self) -> String {
        format!(
            "{} refused: {} empty, {} leading-at, {} embedded-space, {} control-byte, {} no-usable-tags",
            self.total(),
            self.empty,
            self.leading_at,
            self.embedded_space,
            self.control_byte,
            self.no_usable_tags,
        )
    }
}

/* --------------------------------------------------------------- diagnostics -- */

/// What was odd about one blob that still parsed.
///
/// None of these throw the line away. They exist so that a change in what Twitch sends is visible
/// as a number rather than as a slow drift in what appears on screen: in the whole 2.5 MB capture
/// every one of these counters is zero except `escaped_values`, so any of them going non-zero in
/// production is news.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TagDiagnostics {
    /// Values that needed a `String` because an escape was expanded. Not a fault: this is the
    /// measurement that says whether the borrowed fast path is still carrying the traffic.
    pub escaped_values: u32,
    /// Parts with no key at all, from a doubled or leading separator. The part is dropped, since
    /// there is no name to look it up by, and counted here so the drop is not silent.
    pub empty_keys: u32,
    /// Keys sent with no '=' at all. IRCv3 says an absent value is an empty value, so these are
    /// readable as `Some("")`, but Twitch has never sent one in 104,442 tags, so one appearing
    /// means the server or a proxy in front of it changed.
    pub bare_keys: u32,
    /// A key that had already appeared on this line. Zero across 6,063 tagged lines in the
    /// capture. See [`TagMap::get`] for which one answers and why neither is discarded.
    pub duplicate_keys: u32,
    /// A backslash followed by something that is not one of the five documented escapes. Per
    /// IRCv3 the backslash is dropped and the character kept.
    pub unknown_escapes: u32,
    /// A value ending in a lone backslash with nothing to escape. Per IRCv3 it produces no output
    /// character.
    pub dangling_backslashes: u32,
}

impl TagDiagnostics {
    /// True when nothing anomalous happened. `escaped_values` is deliberately not consulted: an
    /// escape is the normal, correct case, and folding it in here would make every subscription
    /// notice on Twitch report itself as a problem.
    pub fn is_clean(&self) -> bool {
        self.empty_keys == 0
            && self.bare_keys == 0
            && self.duplicate_keys == 0
            && self.unknown_escapes == 0
            && self.dangling_backslashes == 0
    }

    /// One line for a log. Allocates; call it on the anomaly path, not per message.
    pub fn summary(&self) -> String {
        format!(
            "{} escaped values, {} empty keys, {} valueless keys, {} duplicate keys, {} unknown \
             escapes, {} dangling backslashes",
            self.escaped_values,
            self.empty_keys,
            self.bare_keys,
            self.duplicate_keys,
            self.unknown_escapes,
            self.dangling_backslashes,
        )
    }
}

/* ----------------------------------------------------------------- the value -- */

/// Expand the IRCv3 escapes in one tag value.
///
/// Borrowed when there is no backslash, which is the overwhelming majority of real traffic. The
/// five documented sequences are `\:` `\s` `\\` `\r` `\n`; anything else after a backslash keeps
/// the character and loses the backslash, and a backslash at the very end produces nothing, both
/// of which are what the specification asks for rather than choices made here.
///
/// SCANNING BYTES IS SAFE HERE AND IT IS WORTH SAYING WHY, because doing it wrong splits UTF-8.
/// 0x5C, the backslash, is an ASCII byte, and UTF-8 never uses an ASCII byte as a continuation
/// byte, so every backslash found by a byte scan is a real backslash on a character boundary and
/// every slice cut at one is a valid `&str`. What is NOT safe is advancing by one byte past an
/// unrecognised escape: irc_busy.txt line 4900 carries `reply-parent-msg-body=!games\s` followed
/// by U+034F, a two byte character, so the branch that steps over a kept character steps by that
/// character's own width.
pub fn unescape_tag_value(raw: &str) -> Cow<'_, str> {
    let mut discard = TagDiagnostics::default();
    unescape_counted(raw, &mut discard)
}

/// [`unescape_tag_value`] with the anomaly counters wired up. Private because the counters belong
/// to a [`TagMap`], and a caller holding a loose `TagDiagnostics` would be counting nothing that
/// anyone reads.
fn unescape_counted<'a>(raw: &'a str, diag: &mut TagDiagnostics) -> Cow<'a, str> {
    let Some(first) = raw.find('\\') else {
        return Cow::Borrowed(raw);
    };

    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len());
    out.push_str(&raw[..first]);
    let mut i = first;

    while i < bytes.len() {
        if bytes[i] != b'\\' {
            /* Copy the whole run up to the next backslash in one push rather than one char at a
             * time. The runs are long: a `system-msg` is a sentence with a backslash every word. */
            let start = i;
            while i < bytes.len() && bytes[i] != b'\\' {
                i += 1;
            }
            out.push_str(&raw[start..i]);
            continue;
        }

        match bytes.get(i + 1) {
            None => {
                /* A lone trailing backslash. IRCv3: no output character. */
                diag.dangling_backslashes = diag.dangling_backslashes.saturating_add(1);
                i += 1;
            }
            Some(b':') => {
                out.push(';');
                i += 2;
            }
            Some(b's') => {
                out.push(' ');
                i += 2;
            }
            Some(b'\\') => {
                out.push('\\');
                i += 2;
            }
            Some(b'r') => {
                out.push('\r');
                i += 2;
            }
            Some(b'n') => {
                out.push('\n');
                i += 2;
            }
            Some(_) => {
                /* Unrecognised: drop the backslash, keep the character WHOLE. `i + 1` is past an
                 * ASCII backslash and so is a char boundary; taking the next `char` and advancing
                 * by `len_utf8` is what keeps a multi byte character from being cut in half. */
                diag.unknown_escapes = diag.unknown_escapes.saturating_add(1);
                i += 1;
                if let Some(c) = raw[i..].chars().next() {
                    out.push(c);
                    i += c.len_utf8();
                }
            }
        }
    }

    Cow::Owned(out)
}

/* ------------------------------------------------------------------- the map -- */

/// The tags of one line, looked up by name.
///
/// Built from the blob between the '@' and the first space. Keys borrow from that blob and so does
/// the map's lifetime, which is why nothing here outlives the read buffer it came from: a caller
/// that wants to keep a value past the next line copies it out deliberately, and the borrow
/// checker makes that a decision rather than an accident.
#[derive(Debug, Clone)]
pub struct TagMap<'a> {
    /// Wire order, preserved. See the module note on why this is a Vec and not a map.
    entries: Vec<(&'a str, Cow<'a, str>)>,
    diagnostics: TagDiagnostics,
}

/// AN UNTAGGED LINE IS NOT A FAULT, AND THIS IS WHAT IT PARSES TO.
///
/// `parse` refuses an empty blob with [`TagRefusal::Empty`] because an empty blob BETWEEN AN `@`
/// AND A SPACE is a framing fault: the sender wrote the tag marker and then no tags. But a line
/// with no `@` at all carries no blob to refuse, and both captures are full of them: every `PING`,
/// every server `NOTICE`, every numeric of the welcome burst. The reader asks those lines for tags
/// the same way it asks a PRIVMSG, and the honest answer is a map that holds nothing, not an error
/// that would make an untagged PING look like a corrupted one in the refusal tally.
///
/// `diagnostics` is `TagDiagnostics::default`, which is `is_clean`. Nothing was parsed, so nothing
/// was odd; counting an absent blob as a fault would inflate every diagnostic this file reports.
impl Default for TagMap<'_> {
    fn default() -> Self {
        TagMap {
            entries: Vec::new(),
            diagnostics: TagDiagnostics::default(),
        }
    }
}

impl<'a> TagMap<'a> {
    /// Parse one tag blob.
    ///
    /// `blob` is everything between the leading '@' and the first space, with neither included.
    /// Framing faults refuse; per tag oddities parse and are counted. See [`TagRefusal`] for the
    /// line between the two and why it falls there.
    pub fn parse(blob: &'a str) -> Result<TagMap<'a>, TagRefusal> {
        if blob.is_empty() {
            return Err(TagRefusal::Empty);
        }
        if blob.as_bytes().first() == Some(&b'@') {
            return Err(TagRefusal::LeadingAt);
        }
        /* One scan for the four bytes IRC framing guarantees cannot appear inside a tag section.
         * Other control bytes are NOT rejected: 0x01 and friends do occur in chat text, a value
         * is free text on Twitch, and throwing a message away over a byte that merely looks odd
         * would be the parser inventing a rule the wire does not have. */
        for (at, byte) in blob.bytes().enumerate() {
            match byte {
                b' ' => return Err(TagRefusal::EmbeddedSpace { at }),
                0x00 | b'\r' | b'\n' => return Err(TagRefusal::ControlByte { at, byte }),
                _ => {}
            }
        }

        /* Exact capacity. There is no raw ';' inside a value (that is what `\:` is for), so the
         * separator count is the tag count and this Vec never reallocates. */
        let expected = blob.bytes().filter(|b| *b == b';').count() + 1;
        let mut entries: Vec<(&'a str, Cow<'a, str>)> = Vec::with_capacity(expected);
        let mut diagnostics = TagDiagnostics::default();

        for part in blob.split(';') {
            if part.is_empty() {
                diagnostics.empty_keys = diagnostics.empty_keys.saturating_add(1);
                continue;
            }
            /* The FIRST '=' splits, not every '='. irc_busy.txt line 3297 carries
             * `reply-parent-msg-body=2+2=5.\srespect\smy\sopinion,\sfascist.`, and a split that
             * takes the second field of a full split truncates that message at "2+2". */
            let (key, raw) = match part.find('=') {
                Some(eq) => (&part[..eq], &part[eq + 1..]),
                None => {
                    diagnostics.bare_keys = diagnostics.bare_keys.saturating_add(1);
                    (part, "")
                }
            };
            if key.is_empty() {
                diagnostics.empty_keys = diagnostics.empty_keys.saturating_add(1);
                continue;
            }

            let value = unescape_counted(raw, &mut diagnostics);
            if matches!(value, Cow::Owned(_)) {
                diagnostics.escaped_values = diagnostics.escaped_values.saturating_add(1);
            }
            /* O(n squared) over at most 26 entries, so at worst a few hundred short compares on a
             * line that already cost a parse. It buys the duplicate count, and the count is the
             * only way a duplicate is ever seen: without it a shadowed tag is invisible. */
            if entries.iter().any(|(k, _)| *k == key) {
                diagnostics.duplicate_keys = diagnostics.duplicate_keys.saturating_add(1);
            }
            entries.push((key, value));
        }

        if entries.is_empty() {
            return Err(TagRefusal::NoUsableTags);
        }
        Ok(TagMap {
            entries,
            diagnostics,
        })
    }

    /// The value of one tag, unescaped, or `None` when the line did not carry that tag.
    ///
    /// `Some("")` and `None` are different answers and callers must keep them different: `Some("")`
    /// is the server saying "no emotes", `None` is the server not saying anything.
    ///
    /// EXACT SPELLING, INCLUDING CASE. IRCv3 keys are case sensitive and Twitch relies on it:
    /// `msg-param-copoReward` (irc_busy.txt line 4751) is the one key of the sixty in the capture
    /// with a capital letter in it. Folding case here would be a guess that happens to work for
    /// fifty nine keys.
    ///
    /// THE FIRST MATCH ANSWERS when a key somehow appears twice. Neither ordering is a security
    /// control and it would be dishonest to describe it as one, since the escaping is what stops a
    /// chatter smuggling a ';' into a value in the first place; first wins is chosen because it is
    /// the one an append cannot change, and the duplicate is counted so the question is asked out
    /// loud rather than settled quietly.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.as_ref())
    }

    /// The same lookup, keeping the `Cow` so a caller can tell a borrowed value from one that had
    /// to be expanded, and can take ownership without a second copy.
    pub fn get_cow(&self, key: &str) -> Option<&Cow<'a, str>> {
        self.entries.iter().find(|(k, _)| *k == key).map(|(_, v)| v)
    }

    /// Whether the line carried this tag at all, empty value or not.
    pub fn contains(&self, key: &str) -> bool {
        self.entries.iter().any(|(k, _)| *k == key)
    }

    /// How many tags were read. Includes a duplicated key twice, because nothing is discarded.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Never true for a successfully parsed blob: a blob with no usable tag refuses instead. Here
    /// because `len` without `is_empty` is a lint, and because the invariant is worth stating.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every tag in the order the wire sent it.
    pub fn iter(&self) -> impl Iterator<Item = (&'a str, &str)> + '_ {
        self.entries.iter().map(|(k, v)| (*k, v.as_ref()))
    }

    /// What was odd about this line. See [`TagDiagnostics`].
    pub fn diagnostics(&self) -> &TagDiagnostics {
        &self.diagnostics
    }
}

/* ---------------------------------------------------------------------- tests -- */

#[cfg(test)]
mod tests {
    use super::*;

    /* THE FIXTURES ARE CAPTURED, NOT WRITTEN. Every blob below was cut out of a real anonymous
     * session against irc.chat.twitch.tv on 2026-09-04 and is quoted verbatim, file and line
     * named. Raw strings are used so that a `\s` on the wire is a `\s` in the source with no
     * layer of Rust escaping in between for a reviewer to have to undo in their head.
     *
     * Where a test needs a byte sequence the capture does not contain, it says so in its own doc
     * comment, starts from a captured value, and performs ONE substitution in view of the reader.
     * A fixture assembled out of imagination is how a parser passes its tests and fails on the
     * wire, which is the failure these captures exist to prevent. */

    /// irc_anon.txt line 18. A plain subscriber message. Note `emotes=`, `flags=` and the trailing
    /// `user-type=`, all empty, and `room-id` at offset 11.
    const ANON_PRIVMSG: &str = r"badge-info=subscriber/15;badges=subscriber/12,campaign-29737511-6496c7fd-09fc-47fb-9442-b99933723e28-mw/1;client-nonce=b2bb7aa898314e2b9a3b3aee169d5a3f;color=#FF0000;display-name=hd_dean;emotes=;first-msg=0;flags=;id=0a6cfab6-8f57-4266-b4d6-f41e8db4adc8;mod=0;returning-chatter=0;room-id=29737511;subscriber=1;tmi-sent-ts=1788555846726;turbo=0;user-id=1164259347;user-type=";

    /// The rest of that same line 18, after the space that ends the tag section. Kept so the
    /// embedded-space test can reassemble the real line rather than approximate it.
    const ANON_PRIVMSG_REMAINDER: &str =
        r":hd_dean!hd_dean@hd_dean.tmi.twitch.tv PRIVMSG #broken_stoic :first";

    /// irc_anon.txt line 12. Six tags, a different command, `room-id` at offset 3.
    const ANON_ROOMSTATE: &str =
        r"emote-only=0;followers-only=-1;r9k=0;room-id=29737511;slow=0;subs-only=0";

    /// irc_anon.txt line 19. A broadcaster announcement. `color=` and `system-msg=` are both
    /// empty here while `emotes=160392:84-91` is not, which is the pairing that makes an
    /// empty-value assertion mean something.
    const ANON_ANNOUNCEMENT: &str = r"badge-info=subscriber/43;badges=broadcaster/1,subscriber/3042,partner/1;color=;display-name=Broken_Stoic;emotes=160392:84-91;flags=;id=4c6584ee-15f9-4544-a9f8-549ce15e1d7c;login=broken_stoic;mod=0;msg-id=announcement;msg-param-color=PURPLE;room-id=29737511;subscriber=1;system-msg=;tmi-sent-ts=1788555867322;user-id=29737511;user-type=;vip=0";

    /// irc_busy.txt line 128. A resub in #summit1g. Two escaped values, and the key order is not
    /// alphabetical: `msg-param-sub-plan-name` at offset 15 precedes `msg-param-sub-plan` at 16.
    const RESUB_SUMMIT: &str = r"badge-info=subscriber/39;badges=subscriber/36,share-the-love/1;color=#FF4500;display-name=Mokiz0r;emotes=302793217:0-6;flags=;id=5a759653-4e06-4dca-935d-32e6d4b4d612;login=mokiz0r;mod=0;msg-id=resub;msg-param-cumulative-months=39;msg-param-months=0;msg-param-multimonth-duration=1;msg-param-multimonth-tenure=0;msg-param-should-share-streak=0;msg-param-sub-plan-name=Channel\sSubscription\s(summit1g);msg-param-sub-plan=Prime;msg-param-was-gifted=false;room-id=26490481;subscriber=1;system-msg=Mokiz0r\ssubscribed\swith\sPrime.\sThey've\ssubscribed\sfor\s39\smonths!;tmi-sent-ts=1788555458254;user-id=54232964;user-type=;vip=0";

    /// irc_busy.txt line 4751. A viewer milestone, and the only capitalised key in the capture:
    /// `msg-param-copoReward`.
    const MILESTONE_MKA1: &str = r"badge-info=;badges=premium/1;color=#FFFFFF;display-name=mka1_;emotes=;flags=;id=754e1976-64c2-4983-9315-7083c63d3b60;login=mka1_;mod=0;msg-id=viewermilestone;msg-param-category=watch-streak;msg-param-copoReward=450;msg-param-id=cd39f8fc-9721-42e3-be27-96803faf5183;msg-param-value=5;room-id=411377640;subscriber=0;system-msg=mka1_\swatched\s5\sconsecutive\sstreams\sand\ssparked\sa\swatch\sstreak!;tmi-sent-ts=1788555990336;user-id=130743830;user-type=;vip=0";

    /// irc_busy.txt line 3297. A reply whose parent body contains an '=' sign.
    const REPLY_EQUALS: &str = r"badge-info=;badges=lost-ark-anniversary/1;client-nonce=d92125b999494595ac26168eb44692e6;color=#690707;display-name=chadegist;emotes=;first-msg=0;flags=;id=20b054a6-1815-4e40-a184-86733dca8e15;mod=0;reply-parent-display-name=joeblow44444444;reply-parent-msg-body=2+2=5.\srespect\smy\sopinion,\sfascist.;reply-parent-msg-id=1d14d21f-14cb-44db-97bb-976b70393d06;reply-parent-user-id=178085754;reply-parent-user-login=joeblow44444444;reply-thread-parent-display-name=joeblow44444444;reply-thread-parent-msg-id=1d14d21f-14cb-44db-97bb-976b70393d06;reply-thread-parent-user-id=178085754;reply-thread-parent-user-login=joeblow44444444;returning-chatter=0;room-id=552120296;subscriber=0;tmi-sent-ts=1788555857339;turbo=0;user-id=41519858;user-type=";

    /// irc_busy.txt line 4900. The parent body is `!games\s` followed immediately by U+034F, a two
    /// byte combining grapheme joiner. The character is written as an escape rather than pasted in
    /// because it renders as nothing at all: an invisible byte sequence in a source file cannot be
    /// reviewed, and `\u{34f}` is the same two bytes with a name attached. This is also the one
    /// fixture where `user-type` is not empty.
    const REPLY_COMBINING: &str = concat!(
        r"badge-info=subscriber/94;badges=moderator/1,subscriber/3084,bot-badge/1;color=#1976D2;display-name=Fossabot;emotes=;first-msg=0;flags=;id=ffe0ad91-265f-4adb-96b4-dfe5d3d710c4;mod=1;reply-parent-display-name=mobin_mli;reply-parent-msg-body=!games\s",
        "\u{34f}",
        r";reply-parent-msg-id=f699975f-c8bd-41cb-85a8-8ead1b9e0f41;reply-parent-user-id=605398543;reply-parent-user-login=mobin_mli;reply-thread-parent-display-name=mobin_mli;reply-thread-parent-msg-id=f699975f-c8bd-41cb-85a8-8ead1b9e0f41;reply-thread-parent-user-id=605398543;reply-thread-parent-user-login=mobin_mli;returning-chatter=0;room-id=23161357;subscriber=1;tmi-sent-ts=1788556001631;turbo=0;user-id=237719657;user-type=mod",
    );

    /// irc_busy.txt line 295. A timeout. Four tags, `room-id` at offset 1, and `ban-duration` is
    /// the tag that separates a 30 second timeout from a permanent ban.
    const CLEARCHAT_BAN: &str =
        r"ban-duration=30;room-id=411377640;target-user-id=1339433483;tmi-sent-ts=1788555477625";

    /// Provenance, blob, and the tag count counted by hand out of the capture file.
    const CAPTURED: [(&str, &str, usize); 8] = [
        ("irc_anon.txt:12 ROOMSTATE", ANON_ROOMSTATE, 6),
        ("irc_anon.txt:18 PRIVMSG", ANON_PRIVMSG, 17),
        (
            "irc_anon.txt:19 USERNOTICE announcement",
            ANON_ANNOUNCEMENT,
            18,
        ),
        ("irc_busy.txt:128 USERNOTICE resub", RESUB_SUMMIT, 25),
        ("irc_busy.txt:295 CLEARCHAT", CLEARCHAT_BAN, 4),
        ("irc_busy.txt:3297 PRIVMSG reply", REPLY_EQUALS, 26),
        ("irc_busy.txt:4751 USERNOTICE milestone", MILESTONE_MKA1, 21),
        ("irc_busy.txt:4900 PRIVMSG reply", REPLY_COMBINING, 25),
    ];

    /// EVERY CAPTURED BLOB PARSES, AND YIELDS THE NUMBER OF TAGS IT ACTUALLY CARRIES.
    ///
    /// The defect: a parser that stops at the first oddity, or that merges the line into one
    /// entry, or that quietly drops the last tag. The counts are hand counted from the capture
    /// rather than derived from the blob inside the test, so a parser cannot satisfy them by
    /// agreeing with itself. The key assertions are what stops the count being reached with
    /// garbage: a parser that emitted 26 entries all keyed "" would hit the number and fail here.
    #[test]
    fn every_captured_blob_parses_and_yields_the_tag_count_it_actually_carries() {
        for (whence, blob, want) in CAPTURED {
            let tags = TagMap::parse(blob).unwrap_or_else(|e| panic!("{whence}: {e}"));
            assert_eq!(tags.len(), want, "{whence}: wrong tag count");
            assert!(!tags.is_empty(), "{whence}");
            for (k, _) in tags.iter() {
                assert!(!k.is_empty(), "{whence}: a nameless key got through");
                assert!(
                    !k.contains('=') && !k.contains(';') && !k.contains(' '),
                    "{whence}: key {k:?} still has a separator in it"
                );
            }
            let d = tags.diagnostics();
            assert!(
                d.is_clean(),
                "{whence}: real Twitch traffic has no anomalies, got {}",
                d.summary()
            );
        }
    }

    /// AN EMPTY VALUE IS PRESENT AND A TAG THE SERVER NEVER SENT IS ABSENT.
    ///
    /// The defect: collapsing `Some("")` into `None`, which is the single easiest mistake to make
    /// here because both read as "nothing". They are different facts. `emotes=` says the message
    /// has no emotes; a missing `emotes` says the line arrived through something that stripped
    /// tags, and a renderer that cannot tell them apart will happily draw a message it has no
    /// metadata for as though it had checked.
    ///
    /// `bits` is the chosen absent key on purpose: it is one of the sixty keys that really does
    /// occur in the capture, just not on this line, so `None` here means absent rather than
    /// unknown-to-Twitch. The non-empty pairings on the second half stop the whole test being
    /// satisfied by a parser that answers `Some("")` to everything.
    #[test]
    fn an_empty_value_is_present_and_a_tag_the_server_never_sent_is_absent() {
        let tags = TagMap::parse(ANON_PRIVMSG).expect("captured PRIVMSG blob");
        assert_eq!(tags.get("emotes"), Some(""));
        assert_eq!(tags.get("flags"), Some(""));
        assert!(tags.contains("emotes"));

        assert_eq!(tags.get("bits"), None, "this line carries no bits tag");
        assert!(!tags.contains("bits"));
        assert_eq!(tags.get("system-msg"), None, "a PRIVMSG has no system-msg");

        let ann = TagMap::parse(ANON_ANNOUNCEMENT).expect("captured USERNOTICE blob");
        assert_eq!(ann.get("emotes"), Some("160392:84-91"));
        assert_eq!(ann.get("color"), Some(""), "this broadcaster set no colour");
        assert_eq!(
            ann.get("system-msg"),
            Some(""),
            "an announcement carries the tag with nothing in it, which is not the same as a \
             PRIVMSG not carrying it at all"
        );
    }

    /// THE LAST TAG ON THE LINE SURVIVES EVEN THOUGH ITS VALUE IS EMPTY.
    ///
    /// The defect: `user-type=` is the final tag on every PRIVMSG Twitch sends, and it is empty on
    /// almost all of them. A parser that trims a trailing separator, filters out parts with no
    /// value, or reads tags with `rsplit_once` loses the last tag on every single line, and the
    /// loss is invisible until the day it is a moderator's line, where the same tag reads `mod`.
    /// The two halves of this test are the same key on two captured lines and neither half alone
    /// would catch it.
    #[test]
    fn the_last_tag_on_the_line_survives_even_though_its_value_is_empty() {
        let plain = TagMap::parse(ANON_PRIVMSG).expect("captured PRIVMSG blob");
        assert_eq!(plain.get("user-type"), Some(""));
        assert_eq!(
            plain.iter().last().map(|(k, _)| k),
            Some("user-type"),
            "user-type really is the last tag on the wire, so this is the trailing case"
        );

        let moderator = TagMap::parse(REPLY_COMBINING).expect("captured reply blob");
        assert_eq!(
            moderator.get("user-type"),
            Some("mod"),
            "the same trailing key with a value; losing it loses moderator status"
        );
    }

    /// ESCAPES IN A REAL RESUB NOTICE BECOME SPACES AND LEAVE NO BACKSLASH BEHIND.
    ///
    /// The defect: printing `Mokiz0r\ssubscribed\swith\sPrime.` in the chat pane. This is not
    /// hypothetical, it is what 55 of the 62 USERNOTICE lines in the capture look like on the
    /// wire. The negative assertions are the ones that matter: an equality alone could be met by a
    /// parser that got lucky on this string, while asserting the output contains no backslash at
    /// all fails any parser that expanded some sequences and not others.
    #[test]
    fn escapes_in_a_real_resub_notice_become_spaces_and_leave_no_backslash_behind() {
        let tags = TagMap::parse(RESUB_SUMMIT).expect("captured resub blob");
        let sys = tags.get("system-msg").expect("a resub carries system-msg");
        assert_eq!(
            sys,
            "Mokiz0r subscribed with Prime. They've subscribed for 39 months!"
        );
        assert!(!sys.contains('\\'), "a backslash survived: {sys:?}");
        assert!(!sys.contains("\\s"), "an unexpanded escape: {sys:?}");

        let plan = tags
            .get("msg-param-sub-plan-name")
            .expect("a resub carries a plan name");
        assert_eq!(plan, "Channel Subscription (summit1g)");
        assert!(!plan.contains('\\'));

        let milestone = TagMap::parse(MILESTONE_MKA1).expect("captured milestone blob");
        assert_eq!(
            milestone.get("system-msg"),
            Some("mka1_ watched 5 consecutive streams and sparked a watch streak!"),
            "a different notice type, from a different channel, with the same escaping"
        );
    }

    /// AN UNESCAPED VALUE IS BORROWED OUT OF THE BLOB AND AN ESCAPED ONE IS OWNED.
    ///
    /// The defect: allocating a `String` for every value. Only 245 of the capture's 104,442 values
    /// contain a backslash, so 99.8 percent have nothing to expand and a `String`-returning parser
    /// makes a hundred thousand copies of bytes it is already holding, on the thread that has to
    /// keep up with seven busy channels.
    ///
    /// A `matches!(.., Cow::Borrowed(_))` on its own is close to trivial: a parser could return
    /// `Cow::Borrowed` of some other string entirely and pass. The address range check is what
    /// stops that, by proving the borrowed value points inside the blob it came from. `blob` is
    /// bound once so that both the parse and the address arithmetic are talking about one value.
    #[test]
    fn an_unescaped_value_is_borrowed_out_of_the_blob_and_an_escaped_one_is_owned() {
        let blob: &str = RESUB_SUMMIT;
        let tags = TagMap::parse(blob).expect("captured resub blob");

        let id = tags.get_cow("id").expect("every line has an id");
        assert!(
            matches!(id, Cow::Borrowed(_)),
            "a value with no backslash must not allocate"
        );
        let lo = blob.as_ptr() as usize;
        let at = id.as_ptr() as usize;
        assert!(
            at >= lo && at < lo + blob.len(),
            "the borrowed value must point INTO the blob, not at some other string"
        );

        let sys = tags
            .get_cow("system-msg")
            .expect("a resub carries system-msg");
        assert!(
            matches!(sys, Cow::Owned(_)),
            "a value whose escapes were expanded cannot be a slice of the blob"
        );

        assert_eq!(
            tags.diagnostics().escaped_values,
            2,
            "exactly two of this line's 25 values carry a backslash: system-msg and \
             msg-param-sub-plan-name"
        );
        let plain = TagMap::parse(ANON_PRIVMSG).expect("captured PRIVMSG blob");
        assert_eq!(
            plain.diagnostics().escaped_values,
            0,
            "an ordinary chat line allocates nothing at all"
        );
    }

    /// ROOM ID SITS AT FOUR DIFFERENT OFFSETS IN THE CAPTURE SO LOOKUP IS BY NAME.
    ///
    /// The defect: reading tags by position, which is tempting because the first hundred PRIVMSG
    /// lines of a capture all look identical. They are not. `room-id` occupies nine distinct
    /// offsets across the capture and this test pins four of them from four different commands.
    ///
    /// Asserting only that `get("room-id")` works on all four would be nearly trivial, since it
    /// would also pass if all four happened to sit at the same offset. The offset assertions are
    /// what make it a statement about position, and they are hand counted from the capture.
    #[test]
    fn room_id_sits_at_four_different_offsets_in_the_capture_so_lookup_is_by_name() {
        let cases = [
            (CLEARCHAT_BAN, 1_usize, "411377640"),
            (ANON_ROOMSTATE, 3, "29737511"),
            (ANON_PRIVMSG, 11, "29737511"),
            (RESUB_SUMMIT, 18, "26490481"),
        ];
        let mut offsets = Vec::new();
        for (blob, offset, want) in cases {
            let tags = TagMap::parse(blob).expect("captured blob");
            assert_eq!(tags.get("room-id"), Some(want));
            let seen = tags
                .iter()
                .position(|(k, _)| k == "room-id")
                .expect("room-id is on every one of these lines");
            assert_eq!(seen, offset, "the capture puts room-id here");
            offsets.push(seen);
        }
        offsets.sort_unstable();
        offsets.dedup();
        assert_eq!(
            offsets.len(),
            4,
            "the point of this test is that the four offsets differ"
        );
    }

    /// THE CAPTURE SENDS SUB PLAN NAME BEFORE SUB PLAN SO KEYS ARE NOT IN SORTED ORDER.
    ///
    /// The defect: assuming the server sorts its keys and reaching for a binary search or a merge
    /// walk. Twitch very nearly does sort them, which is the trap: 6,033 of the 6,063 tagged lines
    /// in the capture are in ascending key order, and the 30 that are not are all this pair.
    /// `msg-param-sub-plan` sorts before `msg-param-sub-plan-name` but arrives after it, so a
    /// binary search finds one of the two and misses the other, on subscription notices only,
    /// which is exactly the traffic nobody tests against.
    #[test]
    fn the_capture_sends_sub_plan_name_before_sub_plan_so_keys_are_not_in_sorted_order() {
        let tags = TagMap::parse(RESUB_SUMMIT).expect("captured resub blob");
        let name_at = tags
            .iter()
            .position(|(k, _)| k == "msg-param-sub-plan-name")
            .expect("present");
        let plan_at = tags
            .iter()
            .position(|(k, _)| k == "msg-param-sub-plan")
            .expect("present");
        assert!(
            name_at < plan_at,
            "the wire order is name then plan: {name_at} then {plan_at}"
        );
        assert!(
            "msg-param-sub-plan" < "msg-param-sub-plan-name",
            "and that wire order is the reverse of sorted order, which is the whole point"
        );
        assert_eq!(
            tags.get("msg-param-sub-plan-name"),
            Some("Channel Subscription (summit1g)")
        );
        assert_eq!(tags.get("msg-param-sub-plan"), Some("Prime"));
    }

    /// AN EQUALS SIGN INSIDE A VALUE BELONGS TO THE VALUE.
    ///
    /// The defect: splitting a tag on every '=' and taking field one, which truncates this real
    /// message from irc_busy.txt line 3297 at "2+2". Chat is free text and users type '='; the
    /// capture also carries a `gifs` tag holding a full URL with four query parameters in it.
    ///
    /// The equality alone could be met by a parser that dropped everything after the first '=' if
    /// the expected string were written to match, so the second assertion states independently
    /// that the surviving value still contains an '=' sign.
    #[test]
    fn an_equals_sign_inside_a_value_belongs_to_the_value() {
        let tags = TagMap::parse(REPLY_EQUALS).expect("captured reply blob");
        let body = tags
            .get("reply-parent-msg-body")
            .expect("a reply carries the parent body");
        assert_eq!(body, "2+2=5. respect my opinion, fascist.");
        assert!(body.contains('='), "the value keeps its own equals sign");
        assert_eq!(
            tags.get("reply-parent-user-login"),
            Some("joeblow44444444"),
            "and the tags after it are unaffected"
        );
    }

    /// A MULTIBYTE CHARACTER IMMEDIATELY AFTER AN ESCAPE SURVIVES WHOLE.
    ///
    /// The defect: an unescaper that walks bytes and advances by one after copying a character
    /// splits a UTF-8 sequence, which in Rust is not a wrong string but a panic on the slice, on
    /// the reader thread, taking chat down. irc_busy.txt line 4900 is the real line that reaches
    /// it: `!games\s` and then U+034F, two bytes, directly after the escape.
    ///
    /// Checking only the string equality would pass on a parser that returned the value untouched
    /// if the expected string were copied out of a buggy run, so the char count and byte count are
    /// asserted separately: eight characters in nine bytes is a statement that a multibyte
    /// character is in there and is intact.
    #[test]
    fn a_multibyte_character_immediately_after_an_escape_survives_whole() {
        let tags = TagMap::parse(REPLY_COMBINING).expect("captured reply blob");
        let body = tags
            .get("reply-parent-msg-body")
            .expect("a reply carries the parent body");
        assert_eq!(body, "!games \u{34f}");
        assert_eq!(body.chars().count(), 8, "!games, a space, and the joiner");
        assert_eq!(
            body.len(),
            9,
            "nine bytes, so the joiner is still two of them"
        );
        assert!(
            body.chars().any(|c| c == '\u{34f}'),
            "the combining character itself, not a replacement"
        );
    }

    /// A KEY WITH A CAPITAL LETTER IS FOUND ONLY BY ITS EXACT SPELLING.
    ///
    /// The defect: lowercasing keys "to be safe". Fifty nine of the sixty keys in the capture are
    /// already lower case, so the habit survives every test until `msg-param-copoReward` arrives
    /// on a viewer milestone and the caller who spelled it the way Twitch spells it gets `None`.
    /// Case folding would also mean allocating a key, which is the other reason not to.
    ///
    /// The `None` half alone is trivially satisfiable by a parser that returns `None` for
    /// everything, which is why it is paired with the exact-spelling hit.
    #[test]
    fn a_key_with_a_capital_letter_is_found_only_by_its_exact_spelling() {
        let tags = TagMap::parse(MILESTONE_MKA1).expect("captured milestone blob");
        assert_eq!(tags.get("msg-param-copoReward"), Some("450"));
        assert!(tags.contains("msg-param-copoReward"));
        assert_eq!(
            tags.get("msg-param-coporeward"),
            None,
            "the folded spelling is a different key and must not answer"
        );
        assert!(!tags.contains("MSG-PARAM-COPOREWARD"));
    }

    /// EVERY DOCUMENTED ESCAPE EXPANDS AND A LONE TRAILING BACKSLASH IS DROPPED.
    ///
    /// The defect: implementing the one escape the capture happens to contain. Only `\s` appears
    /// in 2.5 MB of real tag blobs, so a parser handling `\s` alone passes every test written from
    /// the capture as it stands and then prints `\:` or a doubled backslash the first time a
    /// chatter types a semicolon or a backslash into a message that someone replies to.
    ///
    /// `\r` and `\n` CANNOT appear in a capture, ever, because a raw CR or LF is what ends an IRC
    /// line: the escape exists precisely so a value can carry one. Each case below therefore
    /// starts from the real value at irc_busy.txt line 128 and makes one substitution, shown in
    /// the source, so a reviewer can see exactly what is captured and what is derived.
    #[test]
    fn every_documented_escape_expands_and_a_lone_trailing_backslash_is_dropped() {
        /* Verbatim from irc_busy.txt line 128. */
        let real = r"Mokiz0r\ssubscribed\swith\sPrime.";
        assert_eq!(unescape_tag_value(real), "Mokiz0r subscribed with Prime.");

        /* Same value, the space before "subscribed" replaced by each of the other four escapes. */
        assert_eq!(
            unescape_tag_value(r"Mokiz0r\:subscribed"),
            "Mokiz0r;subscribed",
            "backslash-colon is a semicolon, the byte a raw value may not contain"
        );
        assert_eq!(
            unescape_tag_value(r"Mokiz0r\\subscribed"),
            "Mokiz0r\\subscribed",
            "backslash-backslash is one backslash, not two and not none"
        );
        assert_eq!(
            unescape_tag_value(r"Mokiz0r\rsubscribed"),
            "Mokiz0r\rsubscribed"
        );
        assert_eq!(
            unescape_tag_value(r"Mokiz0r\nsubscribed"),
            "Mokiz0r\nsubscribed"
        );

        /* A value ending in a lone backslash. IRCv3: no output character. */
        assert_eq!(
            unescape_tag_value(r"Mokiz0r\ssubscribed\"),
            "Mokiz0r subscribed"
        );
        assert_eq!(unescape_tag_value(r"\"), "");

        /* AND THE ONE ASSERTION THE OUTPUT CANNOT MAKE. Dropping the `\\` arm entirely produces
         * the SAME characters, every time, because the fallback for an unrecognised escape is
         * also "lose the backslash, keep what follows" and what follows is a backslash. The two
         * implementations are output-equivalent and differ only here, in whether the sequence was
         * understood or merely survived. It matters because `unknown_escapes` is the counter that
         * says Twitch has started sending something new: a parser with no `\\` arm raises it on
         * ordinary chat text, since chatters really do type backslashes (three times in the 6,000
         * message capture), and a counter that cries wolf gets muted and then catches nothing. */
        let mut diag = TagDiagnostics::default();
        let _ = unescape_counted(r"Mokiz0r\\subscribed", &mut diag);
        assert_eq!(
            diag.unknown_escapes, 0,
            "backslash-backslash is one of the five documented escapes, not a surprise"
        );
        assert_eq!(diag.dangling_backslashes, 0);
        let mut diag = TagDiagnostics::default();
        let _ = unescape_counted(r"a\sb\:c\\d\re\nf", &mut diag);
        assert_eq!(
            diag.unknown_escapes, 0,
            "none of the five is a surprise, and a parser missing any one of them says otherwise"
        );

        /* A value with no backslash is handed straight back, not copied. */
        let clean = "0a6cfab6-8f57-4266-b4d6-f41e8db4adc8";
        assert!(
            matches!(unescape_tag_value(clean), Cow::Borrowed(_)),
            "the common case must not allocate"
        );
        assert!(
            matches!(unescape_tag_value(real), Cow::Owned(_)),
            "and the escaped case must, since the bytes differ from the blob's"
        );
    }

    /// AN UNKNOWN ESCAPE DROPS THE BACKSLASH AND KEEPS THE CHARACTER.
    ///
    /// The defect: leaving `\b` as `\b`, or worse, dropping both bytes. IRCv3 is explicit that an
    /// unrecognised sequence keeps its character and loses its backslash, and Twitch's own text is
    /// where it comes up: irc_busy.txt line 5519 is a chatter sending the literal text
    /// `\LLL cops`, and line 4341 is `fatty retard\fck these people`. Correctly escaped, those
    /// arrive in a `reply-parent-msg-body` as `\\LLL cops`; a server or proxy that forgets to
    /// double the backslash sends `\LLL cops` and the parser must still produce something legible
    /// rather than eating the L.
    ///
    /// Both halves are needed. The doubled form alone would pass on a parser with no unknown-escape
    /// branch at all, and the single form alone would pass on one that drops every backslash it
    /// sees, including the ones that mean a real backslash.
    #[test]
    fn an_unknown_escape_drops_the_backslash_and_keeps_the_character() {
        /* The text is verbatim from irc_busy.txt line 5519; the escaping around it is the two
         * forms a server can send it as. */
        assert_eq!(
            unescape_tag_value(r"\\LLL cops"),
            r"\LLL cops",
            "properly escaped, the backslash the chatter typed comes back"
        );
        assert_eq!(
            unescape_tag_value(r"\LLL cops"),
            "LLL cops",
            "unescaped by a broken sender, the backslash is dropped and the L survives"
        );
        /* irc_busy.txt line 4341, same two forms. */
        assert_eq!(unescape_tag_value(r"retard\\fck"), r"retard\fck");
        assert_eq!(unescape_tag_value(r"retard\fck"), "retardfck");

        let mut diag = TagDiagnostics::default();
        let _ = unescape_counted(r"\LLL cops", &mut diag);
        assert_eq!(diag.unknown_escapes, 1, "and it is counted, not swallowed");
        assert_eq!(diag.dangling_backslashes, 0);

        /* AND THE ONE THAT DOES NOT MERELY GET THE STRING WRONG. This is line 4900's value with
         * its `\s` cut down to a bare backslash, which puts a stray backslash of the kind line
         * 5519 proves chatters send directly in front of the two byte U+034F that line 4900
         * proves Twitch delivers. An unescaper that steps ONE BYTE past a kept character lands
         * inside that character, and the next slice is not a panic in some other language, it is
         * a panic in this one, on the reader thread, taking chat down. The equality below is the
         * lesser half of this assertion: the test's real work is completing at all. */
        assert_eq!(
            unescape_tag_value(concat!(r"!games\", "\u{34f}")),
            "!games\u{34f}",
            "the backslash goes, the two byte character survives in one piece"
        );
        assert_eq!(
            unescape_tag_value("\\\u{34f}").len(),
            2,
            "two bytes, not one"
        );
    }

    /// AN ESCAPED SEMICOLON REACHES THE READER AS A SEMICOLON NOT AS BACKSLASH COLON.
    ///
    /// The defect: rendering `opinion\: fascist` in the chat pane. This sequence appears zero
    /// times in the capture's 104,442 tags, which is exactly why it is dangerous: it is the one
    /// escape a parser can omit and still look perfect against a day's real traffic. It reaches
    /// production the moment a chatter types a semicolon into a message that someone replies to,
    /// or a channel names a subscription tier with one, because both of those tag values are free
    /// text.
    ///
    /// The fixture is the real line 3297 with ONE substitution, performed here in view: the comma
    /// the chatter actually typed becomes the semicolon they might have. Everything else, tags,
    /// order, lengths, is the capture.
    ///
    /// The length assertion is the anti-trivial half. Unescaping happens after the split, so the
    /// tag count is unchanged whether or not the escape is understood; asserting it separately
    /// proves the substitution did not restructure the line, which means the value assertion is
    /// testing the unescaper and nothing else.
    #[test]
    fn an_escaped_semicolon_reaches_the_reader_as_a_semicolon_not_as_backslash_colon() {
        let derived = REPLY_EQUALS.replace(r"opinion,\sfascist", r"opinion\:\sfascist");
        assert_ne!(derived, REPLY_EQUALS, "the substitution actually happened");

        let real = TagMap::parse(REPLY_EQUALS).expect("captured reply blob");
        let tags = TagMap::parse(&derived).expect("the derived blob is still a valid blob");
        assert_eq!(
            tags.len(),
            real.len(),
            "an escaped semicolon does not create a tag"
        );

        let body = tags.get("reply-parent-msg-body").expect("present");
        assert_eq!(body, "2+2=5. respect my opinion; fascist.");
        assert!(body.contains(';'), "a real semicolon reached the value");
        assert!(!body.contains('\\'), "and no backslash did: {body:?}");
        assert_eq!(
            tags.get("reply-parent-msg-id"),
            Some("1d14d21f-14cb-44db-97bb-976b70393d06"),
            "the tag after the semicolon is still its own tag"
        );
    }

    /// A BLOB THAT STILL CARRIES ITS AT SIGN IS REFUSED RATHER THAN PARSED INTO JUNK.
    ///
    /// The defect: `&line[..sp]` where `&line[1..sp]` was meant. It is one character and it fails
    /// silently: the first key becomes `@badge-info`, every other tag parses perfectly, and
    /// `get("badge-info")` answers `None` forever. Chat renders with no badges and nothing logs
    /// anything, because from the parser's point of view the line was fine.
    ///
    /// The second half is what makes the first mean something: the same bytes without the sigil
    /// parse to all 17 tags, so the refusal is provably about the '@' and not about the content.
    #[test]
    fn a_blob_that_still_carries_its_at_sign_is_refused_rather_than_parsed_into_junk() {
        let with_sigil = format!("@{ANON_PRIVMSG}");
        assert_eq!(
            TagMap::parse(&with_sigil).unwrap_err(),
            TagRefusal::LeadingAt
        );

        let ok = TagMap::parse(ANON_PRIVMSG).expect("the same bytes, sigil removed");
        assert_eq!(ok.len(), 17);
        assert_eq!(ok.get("badge-info"), Some("subscriber/15"));
    }

    /// A WHOLE LINE HANDED OVER AS A BLOB IS REFUSED AT THE FIRST SPACE.
    ///
    /// The defect: forgetting to cut at the first space. Nothing about the result looks broken:
    /// every tag up to the last one parses correctly, and `user-type`, the final tag on every
    /// Twitch PRIVMSG, quietly ends up holding the prefix, the command, the channel and the entire
    /// message body. A UI that draws `user-type` would print the raw line back at the viewer.
    ///
    /// The reassembled line is the real irc_anon.txt line 18, blob and remainder both captured.
    /// The offset assertion pins the refusal to the first space rather than to any space, and the
    /// last assertion shows the prefix alone is perfectly good, so the refusal is about the extra
    /// bytes and not about the tags.
    #[test]
    fn a_whole_line_handed_over_as_a_blob_is_refused_at_the_first_space() {
        let whole = format!("{ANON_PRIVMSG} {ANON_PRIVMSG_REMAINDER}");
        assert_eq!(
            TagMap::parse(&whole).unwrap_err(),
            TagRefusal::EmbeddedSpace {
                at: ANON_PRIVMSG.len()
            }
        );
        assert_eq!(ANON_PRIVMSG.len(), 373, "where line 18's tag section ends");
        assert!(TagMap::parse(&whole[..373]).is_ok(), "the prefix is fine");
    }

    /// A DOUBLED SEPARATOR AND A VALUELESS KEY ARE BOTH COUNTED AND THE TAGS AROUND THEM ARRIVE.
    ///
    /// The defect: aborting the whole line on the first malformed part. On a CLEARCHAT that means
    /// losing `ban-duration`, and losing `ban-duration` does not mean "unknown", it means the
    /// message reads as a PERMANENT BAN when the moderator issued a 30 second timeout. The
    /// separators are put in front of it deliberately so that an aborting parser reaches exactly
    /// that outcome.
    ///
    /// The second half covers the valueless key, which IRCv3 allows and Twitch has never sent in
    /// 104,442 tags: it must read as `Some("")`, not `None`, and it must be counted, because on a
    /// CLEARCHAT `Some("")` and a zero-second timeout are things a caller has to be able to tell
    /// apart.
    ///
    /// Neither half is satisfied by silence: the diagnostics assertions fail on a parser that
    /// skips the bad part without recording it, which is the shape this whole module exists to
    /// avoid.
    #[test]
    fn a_doubled_separator_and_a_valueless_key_are_both_counted_and_the_tags_around_them_arrive() {
        let doubled = format!(";;{CLEARCHAT_BAN}");
        let tags = TagMap::parse(&doubled).expect("the real tags are still in there");
        assert_eq!(tags.len(), 4, "all four captured tags survived");
        assert_eq!(
            tags.get("ban-duration"),
            Some("30"),
            "a 30 second timeout, not a permanent ban"
        );
        assert_eq!(tags.diagnostics().empty_keys, 2);
        assert!(!tags.diagnostics().is_clean());
        assert_eq!(
            tags.get(""),
            None,
            "a nameless part is dropped, never stored under an empty key"
        );

        let valueless = CLEARCHAT_BAN.replace("ban-duration=30", "ban-duration");
        let tags = TagMap::parse(&valueless).expect("a bare key is legal IRCv3");
        assert_eq!(tags.len(), 4);
        assert!(tags.contains("ban-duration"));
        assert_eq!(
            tags.get("ban-duration"),
            Some(""),
            "present with no value, which is not the same as absent"
        );
        assert_eq!(tags.diagnostics().bare_keys, 1);
        assert_eq!(
            tags.diagnostics().empty_keys,
            0,
            "a bare key is not a nameless one"
        );
    }

    /// A DUPLICATE KEY IS COUNTED, THE FIRST ONE ANSWERS, AND NEITHER IS DISCARDED.
    ///
    /// The defect: a shadowed tag nobody can see. There are no duplicate keys in 6,063 tagged
    /// lines of capture, so this is derived: the real line 18 with its real `mod=0` followed by a
    /// second `mod=1`. That is the shape an upstream framing change would take, and the failure it
    /// causes is a viewer being drawn with a moderator's sword because the last value won.
    ///
    /// First wins is not offered here as a security property, and it would be dishonest to write
    /// it as one: values are escaped, so a chatter cannot inject a ';' to append a tag in the
    /// first place. It is chosen because it is the ordering an append cannot change, and the count
    /// is what actually matters, since it is the only way anyone finds out.
    ///
    /// The `len` and `iter` assertions stop first-wins being implemented by throwing the second
    /// entry away. Nothing is dropped silently, including a duplicate.
    #[test]
    fn a_duplicate_key_is_counted_the_first_one_answers_and_neither_is_discarded() {
        let shadowed = format!("{ANON_PRIVMSG};mod=1");
        let tags = TagMap::parse(&shadowed).expect("a duplicate key is not a framing fault");
        assert_eq!(
            tags.get("mod"),
            Some("0"),
            "the captured value still answers"
        );
        assert_eq!(tags.diagnostics().duplicate_keys, 1);
        assert!(!tags.diagnostics().is_clean());
        assert_eq!(
            tags.len(),
            18,
            "17 captured tags plus the shadow, none dropped"
        );
        let mods: Vec<&str> = tags
            .iter()
            .filter(|(k, _)| *k == "mod")
            .map(|(_, v)| v)
            .collect();
        assert_eq!(mods, vec!["0", "1"], "both are visible to anyone who looks");
    }

    /// REFUSALS ARE TALLIED AND THE TALLY SAYS WHICH KIND IT SAW.
    ///
    /// The defect: a reader that answers a refusal with `continue`. The symptom is "chat is
    /// quiet", the hardest thing to notice and the hardest to attribute, and the fix depends
    /// entirely on which refusal it was, which is why the kinds are counted apart rather than
    /// summed. `describe` is asserted to name the kind because a tally nobody can read is the same
    /// as no tally.
    ///
    /// The zero-check on the untouched kinds is the anti-trivial half: without it a tally that
    /// incremented every counter on every refusal would pass.
    #[test]
    fn refusals_are_tallied_and_the_tally_says_which_kind_it_saw() {
        let with_sigil = format!("@{ANON_PRIVMSG}");
        let whole = format!("{ANON_PRIVMSG} {ANON_PRIVMSG_REMAINDER}");

        let mut tally = TagRefusalTally::default();
        for blob in [with_sigil.as_str(), whole.as_str(), "", ";;;"] {
            if let Err(e) = TagMap::parse(blob) {
                tally.record(&e);
            }
        }
        assert_eq!(tally.total(), 4);
        assert_eq!(tally.leading_at, 1);
        assert_eq!(tally.embedded_space, 1);
        assert_eq!(tally.empty, 1);
        assert_eq!(tally.no_usable_tags, 1);
        assert_eq!(tally.control_byte, 0, "nothing here carried a control byte");

        let text = tally.describe();
        assert!(text.contains("1 leading-at"), "{text}");
        assert!(text.contains("4 refused"), "{text}");

        /* And a healthy line adds nothing, so the tally is a signal rather than a clock. */
        let mut quiet = TagRefusalTally::default();
        if let Err(e) = TagMap::parse(ANON_PRIVMSG) {
            quiet.record(&e);
        }
        assert_eq!(quiet.total(), 0);
    }

    /// AN EMPTY BLOB, A BLOB OF ONLY SEPARATORS, AND A BLOB WITH A LINE BREAK ARE ALL REFUSED.
    ///
    /// The defect: returning an empty map. An empty map answers `None` to everything, which is
    /// exactly what a healthy line missing one optional tag answers, so the caller cannot tell "I
    /// have no tags" from "this line has no bits". Every one of these means the layer above got
    /// the framing wrong, and each has its own variant so the log says which.
    ///
    /// The CR and LF cases are the ones that cannot be reached from a correct reader at all: an
    /// IRC line is terminated by them, so finding one inside a tag section means the line splitter
    /// above is broken, and continuing to parse would be building on it.
    #[test]
    fn an_empty_blob_and_a_blob_of_only_separators_and_one_with_a_line_break_are_refused() {
        assert_eq!(TagMap::parse("").unwrap_err(), TagRefusal::Empty);
        assert_eq!(TagMap::parse(";").unwrap_err(), TagRefusal::NoUsableTags);
        assert_eq!(TagMap::parse(";;;").unwrap_err(), TagRefusal::NoUsableTags);
        assert_eq!(
            TagMap::parse("=nokey").unwrap_err(),
            TagRefusal::NoUsableTags,
            "a value with no key is not a tag"
        );

        /* The real line 18 with the line terminator left on, which is what happens when a reader
         * splits on LF and keeps the CR. */
        let unterminated = format!("{ANON_PRIVMSG}\r");
        assert_eq!(
            TagMap::parse(&unterminated).unwrap_err(),
            TagRefusal::ControlByte {
                at: ANON_PRIVMSG.len(),
                byte: b'\r'
            }
        );
        assert!(
            TagMap::parse(ANON_PRIVMSG).is_ok(),
            "and without it the same bytes parse, so the refusal is about the CR"
        );
    }
}
