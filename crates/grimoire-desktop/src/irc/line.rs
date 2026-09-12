//! THE LINE SHAPE. One raw IRC line cut into its five parts, and nothing beyond that.
//!
//! This module knows the grammar of a line and none of its meaning. It cannot tell a chat message
//! from a timeout, it does not know what a channel is, and it never looks inside a tag. That
//! separation is deliberate: the shape is RFC old and has not moved in thirty years, while the
//! meaning is Twitch's and gains a tag whenever Twitch ships a feature. A parser that mixes the
//! two has to be re-proved against a live socket every time a new tag appears, and there is no
//! live socket in a test run.
//!
//! THE SHAPE, in the order the bytes arrive:
//!
//! ```text
//! [ '@' tags SPACE ] [ ':' prefix SPACE ] command [ SPACE param ]* [ SPACE ':' trailing ]
//! ```
//!
//! Every bracketed part is optional and the capture contains real lines with each one missing:
//! `PING :tmi.twitch.tv` has neither tags nor prefix nor middle params, and a `ROOMSTATE` has
//! tags, a prefix and a param but no trailing at all.
//!
//! WHAT THE SERVER ACTUALLY SENDS, counted rather than assumed. Two anonymous captures taken from
//! irc.chat.twitch.tv on 2026-09-04: `irc_anon.txt` (one channel, 23 lines) and `irc_busy.txt`
//! (seven busy channels, 6094 lines). The census of `irc_busy.txt`, by command:
//!
//! ```text
//! PRIVMSG 5963   USERNOTICE 62   CLEARCHAT 31   ROOMSTATE 7   JOIN 7
//! 353 7   366 7   PING 2   CAP 1   001 002 003 004 372 375 376 one each
//! ```
//!
//! Sixteen commands, and that is the whole vocabulary: no PART, no NOTICE, no MODE, no RECONNECT,
//! no HOSTTARGET, no CLEARMSG, no USERSTATE and no GLOBALUSERSTATE, because this login asks for no
//! membership capability on the busy capture and never speaks. All 6117 lines across both files
//! split and reassemble byte for byte through the code below and none is refused. That total is
//! what this module is measured against, not RFC 1459.
//!
//! THE TRAILING PARAMETER IS THE PART EVERYBODY GETS WRONG, and each wrong way was run against the
//! capture to see how loudly it fails:
//!
//!   * The first ':' anywhere in the line. Wrong on 6113 of 6117 lines, because 857 tag blobs
//!     carry a colon of their own (`emotes=1:18-19`, `flags=0-2:P.3`) and every tagged line puts
//!     the prefix's own colon in front of the body.
//!   * The first " :" over the WHOLE line. Wrong on 6069 of 6117, which is the same trap one step
//!     later: in `@tags :prefix PRIVMSG #chan :hi` the first " :" is the separator in front of the
//!     PREFIX, so the "message" comes out as `drdewd!drdewd@drdewd.tmi.twitch.tv PRIVMSG ...`.
//!   * The LAST " :" of what is left after tags and prefix. This is the dangerous one, because it
//!     is wrong on only 23 of 6117 lines and every one of those 23 is a human typing an emoticon:
//!     `@s4boteur_ u good :)` loses everything before the smiley. A parser built this way passes a
//!     casual eyeball on a quiet channel and eats one message in every 266 on a busy one. It has a
//!     test below with the real line in it.
//!
//! The rule that is actually right: step over the tags, then step over the prefix, then take the
//! FIRST " :" of what remains. From that point every byte to end of line is body, colons and all,
//! and no further scanning happens. 23 real bodies in the capture contain " :" and 109 contain a
//! colon somewhere.
//!
//! BORROW, DO NOT ALLOCATE. Everything returned is a `&str` slice into the caller's line. A busy
//! channel delivers well over a thousand lines a minute and the reader already owns those bytes; a
//! `String` per part would be five copies per line of something that is about to be dropped.
//! `params` is likewise the raw middle span with an iterator over it rather than a `Vec`.
//!
//! REFUSE RATHER THAN GUESS. A line that does not have this shape returns `None` from [`parse`],
//! or a named [`Refusal`] from [`read`] so the reader can count WHICH way it was malformed. This
//! follows `grimoire_parse::combat`, where a line that matches nothing becomes
//! `Reading::Unrecognised` rather than being dropped or forced into the nearest event: a silent
//! drop makes coverage unmeasurable, and a guess makes it wrong while looking right.

/* ---------------------------------------------------------------- the refusals -- */

/// Why a line was refused. Every variant is a shape this splitter can prove is not a line, and the
/// reader counts them by variant rather than as one lump: "eight lines torn mid-tags" is a framing
/// bug in the reader, while "eight lines whose command was not a word" is the far end sending
/// something this module has never seen. Those need different people to look at them, so they must
/// not arrive as the same number.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refusal {
    /// Nothing but line endings. A keepalive newline, or a read that ended on a boundary.
    Empty,
    /// A line opened with '@' and no space ever followed, so the tags never ended. A whole line of
    /// tags with no command after it is a torn read, not a message.
    TagsUnterminated,
    /// `@ ` with nothing between the sigil and the space. There is no tag here to hand on, and
    /// reporting an empty blob as "no tags" would tell the next stage the server sent an untagged
    /// line, which is a different and false thing.
    EmptyTags,
    /// A prefix opened with ':' and no space ever followed.
    PrefixUnterminated,
    /// `: ` with nothing between the sigil and the space.
    EmptyPrefix,
    /// Tags and a prefix and then nothing, or a section that began with a space. There is no
    /// command word to dispatch on.
    NoCommand,
    /// The command is neither a run of letters nor exactly three digits, which is the whole of
    /// what IRC allows a command to be. In practice this fires when a token has gone missing and
    /// the next one has slid into the command's place, so the "command" is a channel name or a
    /// word out of somebody's sentence.
    CommandNotAWord,
    /// Two spaces where the shape allows one, which would make an empty middle parameter. See the
    /// note in [`read`]: zero of the 6117 captured lines do this, so it means our framing of the
    /// stream is wrong rather than that a chatter did something clever.
    BlankParam,
}

/* ------------------------------------------------------------------ the params -- */

/// The middle parameters: everything between the command and the trailing marker, unsplit.
///
/// IT IS THE RAW SPAN AND NOT A `Vec`, for the reason in the module note: this is the hot path of
/// a reader that sees thousands of lines a minute, and 6095 of the 6117 captured lines carry
/// exactly ONE middle parameter. Allocating a vector to hold one `&str` is the kind of cost that
/// never shows up in a profile as a line of code, only as a number.
///
/// The field is private because [`read`] is the only thing allowed to build one, and it is the
/// only place that has proved the span holds no empty parameter. [`ParamIter`] leans on that
/// invariant to keep its own promise never to yield an empty string.
#[derive(Clone, Copy, Debug)]
pub struct Params<'a> {
    raw: &'a str,
}

impl<'a> Params<'a> {
    /// The whole middle span exactly as it arrived, space separators included. Empty when the line
    /// carried no middle parameters at all.
    pub fn raw(self) -> &'a str {
        self.raw
    }

    /// The parameters in the order they arrived.
    pub fn iter(self) -> ParamIter<'a> {
        /* THE EMPTY CASE IS NOT AN OVERSIGHT, IT IS THE WHOLE REASON THIS IS NOT `split(' ')`.
         * `"".split(' ')` yields one item, the empty string, so a line with no middle parameters
         * would report exactly as many parameters as a line with one blank one. `PING
         * :tmi.twitch.tv` is that line, it arrives twice in each capture, and it is the one the
         * reader must answer to stay connected. */
        ParamIter {
            rest: if self.raw.is_empty() {
                None
            } else {
                Some(self.raw)
            },
        }
    }

    /// Parameter `i`, counting from zero at the first one AFTER the command. The command is not a
    /// parameter; a caller that wants it reads [`Line::command`].
    pub fn get(self, i: usize) -> Option<&'a str> {
        self.iter().nth(i)
    }

    /// How many there are. Walks the span, so a caller in a loop should keep the answer.
    pub fn len(self) -> usize {
        self.iter().count()
    }

    /// Whether the line carried any middle parameter at all.
    pub fn is_empty(self) -> bool {
        self.raw.is_empty()
    }
}

impl<'a> IntoIterator for Params<'a> {
    type Item = &'a str;
    type IntoIter = ParamIter<'a>;
    fn into_iter(self) -> ParamIter<'a> {
        self.iter()
    }
}

/// Walks [`Params`] one space separated token at a time. `Copy`, so a caller can restart it.
#[derive(Clone, Copy, Debug)]
pub struct ParamIter<'a> {
    /// What is left to walk. `None` once the span is exhausted, which is also the state an empty
    /// span starts in, so an empty span yields nothing rather than one empty string.
    rest: Option<&'a str>,
}

impl<'a> Iterator for ParamIter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        let rest = self.rest?;
        match rest.split_once(' ') {
            Some((head, tail)) => {
                self.rest = Some(tail);
                Some(head)
            }
            None => {
                self.rest = None;
                Some(rest)
            }
        }
    }
}

/* -------------------------------------------------------------------- the line -- */

/// One line, cut. Every field borrows the input; nothing here owns a byte.
///
/// IT DELIBERATELY DOES NOT DERIVE `PartialEq`. Two of this crate's reachability floors are blind
/// to the fields of a struct that derives one of `PartialEq`, `Eq`, `Hash`, `PartialOrd` or `Ord`,
/// because the derive expands to an impl that reads every field, so rustc's dead code pass and
/// `reach.rs` both fall silent (the note at the top of `reach.rs` was written after an adversarial
/// pass found fourteen fields hiding exactly there). Equality of a `Line` is equality of five
/// borrowed slices, which nothing in production wants; buying it would cost the only check that
/// says the parts this splitter produces are read by somebody. Tests compare fields, and a refusal
/// is matched with `matches!` rather than `assert_eq!`.
#[derive(Clone, Copy, Debug)]
pub struct Line<'a> {
    /// The tags blob with its '@' removed and NOTHING else done to it: still semicolon separated,
    /// still carrying the `\s` escapes the server writes for a space inside a value (215 lines in
    /// the capture have one). Unescaping here would mean the tag parser has to know whether its
    /// input had already been through this module, and the day those two disagree is the day a
    /// display name with a backslash in it goes missing. `None` means the line carried no tags,
    /// which 48 of the 6117 captured lines do.
    pub tags: Option<&'a str>,
    /// The prefix with its ':' removed: `tmi.twitch.tv`, or `nick!user@host`. Not split here,
    /// because splitting it is a claim about what the parts MEAN. `None` on the two `PING` lines
    /// and on nothing else in either capture.
    pub prefix: Option<&'a str>,
    /// The command word, exactly as it arrived and in the case it arrived in. NOT uppercased: that
    /// would be an allocation on every line to normalise something the server has sent in upper
    /// case 6117 times out of 6117. A caller compares with `eq_ignore_ascii_case` and pays
    /// nothing.
    pub command: &'a str,
    /// The middle parameters. See [`Params`]: the raw span, walked on demand.
    pub params: Params<'a>,
    /// The trailing parameter, the part after " :", with the marker removed.
    ///
    /// `None` AND `Some("")` ARE DIFFERENT AND THE DIFFERENCE IS LOAD BEARING. `None` is "this
    /// line had no trailing parameter", which 47 captured lines are: every `JOIN`, every
    /// `ROOMSTATE`, and the 27 `USERNOTICE` lines that are a resub or a watch streak the viewer
    /// typed no message with. `Some("")` is "a trailing parameter arrived and it was empty". Zero
    /// captured lines are that, so a caller reaching for `unwrap_or("")` would be flattening a
    /// distinction the server has so far always sent one side of, which is exactly the kind of
    /// thing that stays invisible until the other side turns up.
    pub trailing: Option<&'a str>,
}

/// Cut one raw line into its parts, or say why it is not a line.
///
/// The order of the three steps is the entire correctness argument, so it is written out here:
/// tags first, then prefix, then trailing, each searching only what the previous step left. Doing
/// the trailing search first, over the whole line, is wrong on 6069 of the 6117 captured lines
/// (module note), and it is wrong in the worst way, by returning something that looks like a
/// message.
pub fn read(raw: &str) -> Result<Line<'_>, Refusal> {
    /* The reader hands lines off a CRLF framed stream and may or may not have taken the framing
     * bytes off already. Trimming them here rather than trusting the caller is what stops a
     * trailing parameter of "first\r": that string is not equal to "first", it draws a stray glyph
     * or a break in a chat row, and it compares unequal to every literal a test could write. */
    let line = raw.trim_end_matches(['\r', '\n']);
    if line.is_empty() {
        return Err(Refusal::Empty);
    }

    let mut rest = line;

    let tags = match rest.strip_prefix('@') {
        None => None,
        Some(after) => {
            let (blob, tail) = after.split_once(' ').ok_or(Refusal::TagsUnterminated)?;
            if blob.is_empty() {
                return Err(Refusal::EmptyTags);
            }
            rest = tail;
            Some(blob)
        }
    };

    /* This tests `rest` and not `line`, and that is the whole of why the tags cannot swallow the
     * prefix or the prefix the body. Every tagged line in the capture is followed immediately by a
     * ':' prefix, so a version of this that looked at `line` would find the '@' gone, find the ':'
     * where it expected the start, and quietly cut the wrong section. */
    let prefix = match rest.strip_prefix(':') {
        None => None,
        Some(after) => {
            let (name, tail) = after.split_once(' ').ok_or(Refusal::PrefixUnterminated)?;
            if name.is_empty() {
                return Err(Refusal::EmptyPrefix);
            }
            rest = tail;
            Some(name)
        }
    };

    /* FIRST occurrence, in what is left after the two sigil sections. `rfind` here would be right
     * on 6094 lines and wrong on the 23 where somebody typed " :)". Slicing by these indices
     * cannot panic and cannot split a codepoint: `find` returns a byte offset that is already a
     * char boundary, the byte it points at is the ASCII space, and the byte after it is the ASCII
     * colon, so `at` and `at + 2` are both boundaries even in a body full of emoji, which the
     * capture has. */
    let (middle, trailing) = match rest.find(" :") {
        Some(at) => (&rest[..at], Some(&rest[at + 2..])),
        None => (rest, None),
    };

    if middle.is_empty() {
        return Err(Refusal::NoCommand);
    }

    let (command, params) = match middle.split_once(' ') {
        Some((command, params)) => (command, params),
        None => (middle, ""),
    };
    if command.is_empty() {
        return Err(Refusal::NoCommand);
    }
    if !is_command_word(command) {
        return Err(Refusal::CommandNotAWord);
    }
    /* An empty token in the middle span means two spaces in a row, which is the one place this
     * module chooses refusal over tolerance. It could be absorbed: skipping empty tokens would
     * parse such a line perfectly well. The reason not to is that zero of 6117 captured lines have
     * one, so a doubled space is not something this server does, and the two ways it could reach
     * us are a bug in our own framing and a change at the far end. Both want a counter to go up,
     * and neither wants a message delivered as though the shape were normal. */
    if !params.is_empty() && params.split(' ').any(str::is_empty) {
        return Err(Refusal::BlankParam);
    }

    Ok(Line {
        tags,
        prefix,
        command,
        params: Params { raw: params },
        trailing,
    })
}

/// The same split for a caller that does not count. `None` wherever [`read`] refuses.
pub fn parse(raw: &str) -> Option<Line<'_>> {
    read(raw).ok()
}

/// An IRC command is a run of letters or exactly three digits, and nothing else. Both halves are
/// needed for this stream: the words are `PRIVMSG` and its fourteen siblings, the digits are the
/// `001` through `376` of the connect handshake.
///
/// WHY CHECK AT ALL, given the split does not need it. Because the first thing to arrive in the
/// command's place when a line is damaged is a token that is obviously not a command: a `#channel`
/// when a token has been dropped, or half a word when a read was torn. Letting those through hands
/// the next stage a "command" it will simply not match, and an unmatched command is
/// indistinguishable from a message type this build has not implemented yet. One of those needs a
/// bug report and the other needs a feature, so the refusal keeps them in separate counters.
fn is_command_word(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() == 3 && b.iter().all(u8::is_ascii_digit) {
        return true;
    }
    !b.is_empty() && b.iter().all(u8::is_ascii_alphabetic)
}

/* ------------------------------------------------------------------- the tests -- */

#[cfg(test)]
mod tests {
    use super::*;

    /* EVERY FIXTURE BELOW IS A REAL LINE FROM A REAL SOCKET, byte for byte including its CRLF,
     * copied out of the two captures taken from irc.chat.twitch.tv on 2026-09-04 with an anonymous
     * justinfan login. Nothing here was typed by hand to make a test pass, because a hand written
     * fixture only proves the parser agrees with whoever wrote the fixture. `concat!` is how the
     * CRLF gets onto the end of a raw string literal, and the literal is raw so that the escapes
     * inside a tag blob stay the two bytes the server sent. */

    /// `irc_anon.txt`: no tags, a prefix, two middle params, a trailing with spaces in it.
    const CAP_ACK: &str = concat!(
        r#":tmi.twitch.tv CAP * ACK :twitch.tv/tags twitch.tv/commands twitch.tv/membership"#,
        "\r\n"
    );
    const WELCOME_001: &str = concat!(
        r#":tmi.twitch.tv 001 justinfan73921 :Welcome, GLHF!"#,
        "\r\n"
    );
    const HOST_002: &str = concat!(
        r#":tmi.twitch.tv 002 justinfan73921 :Your host is tmi.twitch.tv"#,
        "\r\n"
    );
    const CREATED_003: &str = concat!(
        r#":tmi.twitch.tv 003 justinfan73921 :This server is rather new"#,
        "\r\n"
    );
    /// A one byte trailing. Worth keeping: it is the shortest body the server sends and it walks
    /// straight into any splitter that assumes a body has words in it.
    const MYINFO_004: &str = concat!(r#":tmi.twitch.tv 004 justinfan73921 :-"#, "\r\n");
    const MOTD_START_375: &str = concat!(r#":tmi.twitch.tv 375 justinfan73921 :-"#, "\r\n");
    const MOTD_372: &str = concat!(
        r#":tmi.twitch.tv 372 justinfan73921 :You are in a maze of twisty passages, all alike."#,
        "\r\n"
    );
    const MOTD_END_376: &str = concat!(r#":tmi.twitch.tv 376 justinfan73921 :>"#, "\r\n");
    /// A prefix in `nick!user@host` form, one middle param, no trailing at all.
    const JOIN_STOIC: &str = concat!(
        r#":justinfan73921!justinfan73921@justinfan73921.tmi.twitch.tv JOIN #broken_stoic"#,
        "\r\n"
    );
    /// Three middle params, one of which is a bare `=`. The only shape in either capture with more
    /// than two, and the reason [`Params`] is an iterator rather than a pair.
    const NAMES_353: &str = concat!(
        r#":justinfan73921.tmi.twitch.tv 353 justinfan73921 = #broken_stoic :justinfan73921"#,
        "\r\n"
    );
    const NAMES_END_366: &str = concat!(
        r#":justinfan73921.tmi.twitch.tv 366 justinfan73921 #broken_stoic :End of /NAMES list"#,
        "\r\n"
    );
    /// Tags, a prefix, one param, and NO trailing.
    const ROOMSTATE_STOIC: &str = concat!(
        r#"@emote-only=0;followers-only=-1;r9k=0;room-id=29737511;slow=0;subs-only=0 :tmi.twitch.tv ROOMSTATE #broken_stoic"#,
        "\r\n"
    );
    /// No tags, no prefix, no middle params. The line that catches `"".split(' ')`.
    const PING: &str = concat!(r#"PING :tmi.twitch.tv"#, "\r\n");
    /// The first message of the anon capture.
    const PRIVMSG_FIRST: &str = concat!(
        r#"@badge-info=subscriber/15;badges=subscriber/12,campaign-29737511-6496c7fd-09fc-47fb-9442-b99933723e28-mw/1;client-nonce=b2bb7aa898314e2b9a3b3aee169d5a3f;color=#FF0000;display-name=hd_dean;emotes=;first-msg=0;flags=;id=0a6cfab6-8f57-4266-b4d6-f41e8db4adc8;mod=0;returning-chatter=0;room-id=29737511;subscriber=1;tmi-sent-ts=1788555846726;turbo=0;user-id=1164259347;user-type= :hd_dean!hd_dean@hd_dean.tmi.twitch.tv PRIVMSG #broken_stoic :first"#,
        "\r\n"
    );

    /* The rest are from `irc_busy.txt`. */

    /// A body ending in an emoticon, so the body contains a " :" that is not a separator. This is
    /// the line that kills an `rfind(" :")` splitter, and its tag blob carries `emotes=1:18-19`,
    /// so it kills a first-colon splitter too.
    const PRIVMSG_EMOTICON: &str = concat!(
        r#"@badge-info=;badges=turbo/1;client-nonce=8F9D0203-6F80-4DCB-9EAF-32C6E4A46281;color=#B22222;display-name=Drdewd;emotes=1:18-19;first-msg=0;flags=;id=2b5f5c8e-5c7f-4ad9-b0b3-00841821eb6a;mod=0;returning-chatter=0;room-id=23161357;subscriber=0;tmi-sent-ts=1788555538054;turbo=1;user-id=61028566;user-type= :drdewd!drdewd@drdewd.tmi.twitch.tv PRIVMSG #lirik :@s4boteur_ u good :)"#,
        "\r\n"
    );
    /// The same trap with a two word body, where the whole message is lost rather than half of it.
    const PRIVMSG_THANKS: &str = concat!(
        r#"@badge-info=subscriber/14;badges=subscriber/12,bits/1000;client-nonce=542cbc7b0aa34708adf079b9686f92a0;color=#FF7F50;display-name=Emphasyze;emotes=1:7-8;first-msg=0;flags=;id=0ef14b98-a29a-49f2-8cdd-ccca0209d08b;mod=0;returning-chatter=0;room-id=23161357;subscriber=1;tmi-sent-ts=1788555597377;turbo=0;user-id=30609166;user-type= :emphasyze!emphasyze@emphasyze.tmi.twitch.tv PRIVMSG #lirik :Thanks :)"#,
        "\r\n"
    );
    /// A colon inside a tag value (`flags=0-2:P.3`) and a plain three letter body.
    const PRIVMSG_WTF: &str = concat!(
        r#"@badge-info=;badges=;color=;display-name=ilekai;emotes=;first-msg=0;flags=0-2:P.3;id=221ce712-4ec0-4592-b379-b2515c6e2db2;mod=0;returning-chatter=0;room-id=552120296;subscriber=0;tmi-sent-ts=1788556102714;turbo=0;user-id=1193342676;user-type= :ilekai!ilekai@ilekai.tmi.twitch.tv PRIVMSG #zackrawrr :wtf"#,
        "\r\n"
    );
    /// Tags carrying escaped spaces in a `system-msg` sentence, and NO trailing: nobody typed a
    /// message with this watch streak.
    const USERNOTICE_SILENT: &str = concat!(
        r#"@badge-info=;badges=;color=#1E90FF;display-name=ethanc_051;emotes=;flags=;id=bcbfaf1f-b35a-4760-8fa3-0c4cf0a9c1e5;login=ethanc_051;mod=0;msg-id=viewermilestone;msg-param-category=watch-streak;msg-param-copoReward=450;msg-param-id=67495015-a8c7-45cb-8f78-a8518a3f36ab;msg-param-value=5;room-id=411377640;subscriber=0;system-msg=ethanc_051\swatched\s5\sconsecutive\sstreams\sand\ssparked\sa\swatch\sstreak!;tmi-sent-ts=1788555836864;user-id=1521708486;user-type=;vip=0 :tmi.twitch.tv USERNOTICE #jynxzi"#,
        "\r\n"
    );
    /// A trailing with a URL in it, so the body carries colons that are not preceded by a space.
    const USERNOTICE_ANNOUNCE: &str = concat!(
        r#"@badge-info=subscriber/94;badges=moderator/1,subscriber/3084,partner/1;color=#1976D2;display-name=Fossabot;emotes=;flags=;id=1bc82b12-6a68-4373-be11-f7d8b47f599d;login=fossabot;mod=1;msg-id=announcement;msg-param-color=BLUE;room-id=23161357;subscriber=1;system-msg=;tmi-sent-ts=1788555732564;user-id=237719657;user-type=mod;vip=0 :tmi.twitch.tv USERNOTICE #lirik :Follow Lirik on X: https://x.com/lirik"#,
        "\r\n"
    );
    const CLEARCHAT: &str = concat!(
        r#"@ban-duration=30;room-id=411377640;target-user-id=1339433483;tmi-sent-ts=1788555477625 :tmi.twitch.tv CLEARCHAT #jynxzi :sixfivee65"#,
        "\r\n"
    );

    /// Every fixture, in capture order. Twenty real lines covering all sixteen commands that
    /// arrived across the two files.
    const CAPTURED: &[&str] = &[
        CAP_ACK,
        WELCOME_001,
        HOST_002,
        CREATED_003,
        MYINFO_004,
        MOTD_START_375,
        MOTD_372,
        MOTD_END_376,
        JOIN_STOIC,
        NAMES_353,
        NAMES_END_366,
        ROOMSTATE_STOIC,
        PING,
        PRIVMSG_FIRST,
        PRIVMSG_EMOTICON,
        PRIVMSG_THANKS,
        PRIVMSG_WTF,
        USERNOTICE_SILENT,
        USERNOTICE_ANNOUNCE,
        CLEARCHAT,
    ];

    /// The sixteen commands the server sent across both captures, counted over the files. If a
    /// seventeenth ever turns up, this list is where the surprise gets recorded.
    const COMMANDS_SEEN: [&str; 16] = [
        "PRIVMSG",
        "USERNOTICE",
        "CLEARCHAT",
        "ROOMSTATE",
        "JOIN",
        "PING",
        "CAP",
        "001",
        "002",
        "003",
        "004",
        "353",
        "366",
        "372",
        "375",
        "376",
    ];

    /// Put a cut line back together from its parts. Byte for byte, or the split lost something.
    fn rebuilt(line: &Line<'_>) -> String {
        let mut s = String::new();
        if let Some(tags) = line.tags {
            s.push('@');
            s.push_str(tags);
            s.push(' ');
        }
        if let Some(prefix) = line.prefix {
            s.push(':');
            s.push_str(prefix);
            s.push(' ');
        }
        s.push_str(line.command);
        if !line.params.is_empty() {
            s.push(' ');
            s.push_str(line.params.raw());
        }
        if let Some(trailing) = line.trailing {
            s.push_str(" :");
            s.push_str(trailing);
        }
        s
    }

    fn cut(raw: &str) -> Line<'_> {
        match read(raw) {
            Ok(line) => line,
            Err(why) => panic!("a real captured line was refused as {why:?}: {raw:?}"),
        }
    }

    /// THE DEFECT: a split that loses or duplicates bytes. Every part of this parser hands back a
    /// slice, and a wrong offset anywhere (an `at + 1` for the two byte " :" marker, a prefix that
    /// keeps its own colon, a middle span that keeps its separator) produces something that still
    /// looks like a message on screen and quietly is not what the server said. Reassembly is the
    /// only assertion that catches all of those at once, because it is the only one that accounts
    /// for every byte.
    ///
    /// The loop alone could pass on an empty corpus, or on twenty copies of one shape, so the
    /// coverage assertions underneath it are part of the test rather than decoration.
    #[test]
    fn every_captured_line_reassembles_from_its_parts_byte_for_byte() {
        let mut with_tags = 0;
        let mut without_tags = 0;
        let mut with_trailing = 0;
        let mut without_trailing = 0;
        let mut body_holds_the_marker = 0;
        let mut commands: Vec<&str> = Vec::new();

        for raw in CAPTURED {
            let line = cut(raw);
            assert_eq!(
                rebuilt(&line),
                raw.trim_end_matches(['\r', '\n']),
                "the parts of this line do not add back up to the line: {raw:?}"
            );

            /* A middle parameter can never begin with a colon, because the first " :" ends the
             * middle span by definition. The next stage leans on that to know that parameter zero
             * is a channel name and not the start of a body. */
            for param in line.params {
                assert!(
                    !param.starts_with(':'),
                    "a middle param began with the trailing marker: {param:?}"
                );
                assert!(!param.is_empty(), "an empty middle param survived");
            }
            assert!(
                COMMANDS_SEEN.contains(&line.command),
                "a fixture carries a command the census does not list: {:?}",
                line.command
            );

            commands.push(line.command);
            if line.tags.is_some() {
                with_tags += 1;
            } else {
                without_tags += 1;
            }
            match line.trailing {
                Some(body) => {
                    with_trailing += 1;
                    if body.contains(" :") {
                        body_holds_the_marker += 1;
                    }
                }
                None => without_trailing += 1,
            }
        }

        for command in COMMANDS_SEEN {
            assert!(
                commands.contains(&command),
                "the corpus no longer covers {command}, which the server really sends"
            );
        }
        assert!(
            with_tags >= 5 && without_tags >= 5,
            "the corpus stopped covering both tagged and untagged lines"
        );
        assert!(
            with_trailing >= 5,
            "the corpus stopped covering lines that carry a body"
        );
        assert!(
            without_trailing >= 3,
            "the corpus stopped covering lines with NO body, which is what makes None mean anything"
        );
        assert!(
            body_holds_the_marker >= 2,
            "the corpus stopped covering bodies that contain \" :\", which is the whole hazard"
        );
    }

    /// THE DEFECT: taking the LAST " :" instead of the first. It is the split that survives
    /// review, because it is correct on 6094 of the 6117 captured lines: every line where nobody
    /// typed an emoticon. On the other 23 it does not drop the message, which would at least be
    /// visible; it delivers a TRUNCATED one, so `@s4boteur_ u good :)` arrives in the chat pane as
    /// `)` and nothing anywhere says a byte went missing.
    #[test]
    fn a_colon_in_the_message_body_does_not_end_the_message() {
        assert_eq!(cut(PRIVMSG_EMOTICON).trailing, Some("@s4boteur_ u good :)"));
        assert_eq!(cut(PRIVMSG_THANKS).trailing, Some("Thanks :)"));

        /* Without this, the two assertions above could be satisfied by a parser that never looks
         * for a second marker, on fixtures that turned out not to have one. They do have one, and
         * the test says so rather than assuming it. */
        for raw in [PRIVMSG_EMOTICON, PRIVMSG_THANKS] {
            let body = cut(raw).trailing.unwrap_or_default();
            assert!(
                body.contains(" :"),
                "this fixture no longer exercises the hazard it was chosen for: {body:?}"
            );
        }
    }

    /// THE DEFECT: searching the WHOLE line for the trailing marker instead of what is left after
    /// the tags and the prefix. Measured wrong on 6069 of 6117 captured lines, and wrong in the
    /// shape that hurts most: the "message" comes out as the sender's own prefix followed by the
    /// real message, so every chat row reads `drdewd!drdewd@drdewd.tmi.twitch.tv PRIVMSG #lirik
    /// :@s4boteur_ u good :)` and the parser looks like it is working.
    #[test]
    fn the_tags_and_the_prefix_cannot_swallow_the_body() {
        let line = cut(PRIVMSG_EMOTICON);

        assert_eq!(line.prefix, Some("drdewd!drdewd@drdewd.tmi.twitch.tv"));
        assert_eq!(line.command, "PRIVMSG");
        assert_eq!(line.params.get(0), Some("#lirik"));
        assert_eq!(line.trailing, Some("@s4boteur_ u good :)"));
        assert!(
            line.tags
                .unwrap_or_default()
                .starts_with("badge-info=;badges=turbo/1;"),
            "the tags blob lost its start"
        );

        /* The fixture only exercises the hazard because its tag blob really does carry a colon of
         * its own and a prefix really does follow it. Both are asserted so that swapping in a
         * tamer line cannot quietly disarm the test. */
        assert!(line.tags.unwrap_or_default().contains("emotes=1:18-19"));
        assert!(line.prefix.unwrap_or_default().contains('!'));
    }

    /// THE DEFECT: `trailing.unwrap_or("")`, or a `String` field that starts out empty. 47 of the
    /// 6117 captured lines carry no trailing parameter at all and zero carry an empty one, so
    /// today those two states can be told apart only by keeping the `Option`. A `USERNOTICE` with
    /// no trailing is a resub or a watch streak the viewer wrote no message with, and a chat pane
    /// that renders it as an empty message from that viewer is printing something the viewer never
    /// said.
    #[test]
    fn a_line_with_no_trailing_parameter_reports_none_and_not_an_empty_body() {
        for raw in [ROOMSTATE_STOIC, JOIN_STOIC, USERNOTICE_SILENT] {
            let line = cut(raw);
            assert!(
                line.trailing.is_none(),
                "a line with no body reported one: {:?}",
                line.trailing
            );
            /* Refusing to guess must not cost the rest of the line: these still have their command
             * and their channel, which is what makes them worth delivering. */
            assert!(!line.command.is_empty());
            assert_eq!(line.params.len(), 1);
        }
        assert_eq!(cut(ROOMSTATE_STOIC).params.get(0), Some("#broken_stoic"));
        assert_eq!(cut(USERNOTICE_SILENT).params.get(0), Some("#jynxzi"));

        /* And the other side of the distinction, so this cannot be passed by a parser that answers
         * None for everything. */
        assert_eq!(cut(PRIVMSG_WTF).trailing, Some("wtf"));
    }

    /// THE DEFECT: an empty trailing read as no trailing. `PRIVMSG #chan :` is legal IRC and the
    /// two states have to stay apart in both directions, or the `Option` above is only half a
    /// distinction.
    ///
    /// HONESTY ABOUT THIS FIXTURE: it is a real captured line with its body bytes removed, not a
    /// line the server sent. Zero of the 6117 captured lines carry an empty trailing, so this
    /// asserts the splitter's rule and is NOT evidence about what the server does.
    #[test]
    fn an_empty_trailing_is_still_a_trailing() {
        let real = PRIVMSG_WTF.trim_end_matches(['\r', '\n']);
        let bodyless = real.trim_end_matches("wtf");
        assert!(
            bodyless.ends_with(" :"),
            "the fixture was edited and no longer ends at the marker: {bodyless:?}"
        );

        let line = cut(bodyless);
        assert_eq!(line.trailing, Some(""));
        assert_eq!(line.command, "PRIVMSG");
        assert_eq!(line.params.get(0), Some("#zackrawrr"));
    }

    /// THE DEFECT: the framing bytes riding into the message. The reader splits a CRLF stream and
    /// whether it hands the CR on is its business, not this module's; if a CR survives into the
    /// trailing then every body ends in a control character, every comparison against a literal
    /// fails, and the chat pane draws a stray glyph. The whole corpus is stored WITH its CRLF so
    /// that no test here can be passed by a parser that only works on pre-trimmed input.
    #[test]
    fn the_framing_bytes_are_not_part_of_the_message() {
        assert!(
            PRIVMSG_FIRST.ends_with("\r\n"),
            "the fixture lost the line ending it was chosen to carry"
        );
        assert_eq!(cut(PRIVMSG_FIRST).trailing, Some("first"));

        for raw in CAPTURED {
            let line = cut(raw);
            for part in [line.tags, line.prefix, line.trailing, Some(line.command)]
                .into_iter()
                .flatten()
            {
                assert!(
                    !part.contains('\r') && !part.contains('\n'),
                    "a framing byte survived into a part: {part:?}"
                );
            }
        }
    }

    /// THE DEFECT: `"".split(' ')` yields one item, the empty string. A `Params` built on
    /// `split(' ')` alone reports that a `PING` carries one middle parameter whose value is "", so
    /// a caller that reads parameter zero to find a channel gets `Some("")` instead of `None` and
    /// goes looking for a channel called nothing. `PING` arrives twice in each capture and is the
    /// line the reader must answer to stay connected, so this is not a corner.
    #[test]
    fn a_line_with_no_middle_parameters_reports_none_rather_than_one_empty_one() {
        let line = cut(PING);
        assert_eq!(line.command, "PING");
        assert!(line.tags.is_none());
        assert!(line.prefix.is_none());
        assert_eq!(line.trailing, Some("tmi.twitch.tv"));

        assert!(line.params.is_empty());
        assert_eq!(line.params.len(), 0);
        assert_eq!(line.params.get(0), None);
        assert_eq!(line.params.iter().next(), None);
        assert_eq!(line.params.raw(), "");
        assert_eq!(line.params.into_iter().count(), 0);
    }

    /// THE DEFECT: counting the command as parameter zero, or keeping only the first parameter.
    /// 6095 of the 6117 captured lines carry exactly one middle parameter, so a splitter that
    /// keeps only the first is right about 99 percent of the traffic and wrong about the handshake
    /// it has to get through before any of that traffic arrives.
    #[test]
    fn the_middle_parameters_keep_their_order_and_their_count() {
        let line = cut(NAMES_353);
        assert_eq!(line.command, "353");
        assert_eq!(
            line.params.iter().collect::<Vec<_>>(),
            vec!["justinfan73921", "=", "#broken_stoic"]
        );
        assert_eq!(line.params.len(), 3);
        assert_eq!(line.trailing, Some("justinfan73921"));

        let line = cut(CAP_ACK);
        assert_eq!(line.params.iter().collect::<Vec<_>>(), vec!["*", "ACK"]);
        assert_eq!(
            line.trailing,
            Some("twitch.tv/tags twitch.tv/commands twitch.tv/membership")
        );

        let line = cut(NAMES_END_366);
        assert_eq!(
            line.params.iter().collect::<Vec<_>>(),
            vec!["justinfan73921", "#broken_stoic"]
        );
        assert_eq!(line.trailing, Some("End of /NAMES list"));
    }

    /// THE DEFECT: this module quietly doing the tag parser's job. The server escapes a space
    /// inside a tag value and 215 captured lines carry one; if the blob were unescaped here as
    /// well as there, a `system-msg` would come out with its backslashes eaten twice and a display
    /// name containing one would be corrupted. The blob is handed on exactly as it arrived, sigil
    /// off and nothing else.
    #[test]
    fn the_tags_blob_is_handed_on_raw_with_its_escapes_intact() {
        let line = cut(USERNOTICE_SILENT);
        let tags = line.tags.unwrap_or_default();
        assert!(
            tags.contains(r"system-msg=ethanc_051\swatched\s5\sconsecutive\sstreams"),
            "the escapes in the tag blob were altered: {tags:?}"
        );
        assert!(
            !tags.starts_with('@'),
            "the sigil is the frame, not the data"
        );
        assert!(
            !tags.ends_with(' '),
            "the separator after the blob belongs to neither part"
        );
        assert_eq!(line.command, "USERNOTICE");
    }

    /// THE DEFECT: a body that contains a colon which is NOT a marker. A URL is the everyday case,
    /// and a splitter that looks for a bare ':' rather than " :" cuts `Follow Lirik on X:
    /// https://x.com/lirik` at the `X:`, or at the `https:` if it takes the last one. Measured: a
    /// first-colon splitter is wrong on 6113 of 6117 captured lines.
    #[test]
    fn a_colon_that_is_not_preceded_by_a_space_is_body_text() {
        let line = cut(USERNOTICE_ANNOUNCE);
        assert_eq!(
            line.trailing,
            Some("Follow Lirik on X: https://x.com/lirik")
        );
        assert!(
            line.trailing.unwrap_or_default().matches(':').count() >= 2,
            "this fixture no longer carries the colons it was chosen for"
        );
    }

    /// THE DEFECT: half parsing a line the socket tore in two. A reader that pulls a chunk and
    /// splits it on CRLF hands over a partial line the moment a message straddles a chunk
    /// boundary, and on a busy channel that is constant. Every fragment below is a real captured
    /// line cut short at a byte, which is exactly what arrives. If a fragment parsed, the reader
    /// would emit a message with a truncated body or a nonsense command and never count the
    /// damage.
    #[test]
    fn a_line_torn_by_the_socket_is_refused_rather_than_half_parsed() {
        let whole = PRIVMSG_EMOTICON.trim_end_matches(['\r', '\n']);

        /* Torn inside the tag blob, so no space ever arrives. */
        assert!(matches!(read(&whole[..40]), Err(Refusal::TagsUnterminated)));
        /* Torn just after a prefix started. The connect handshake is all prefix lines, so this is
         * the first thing a torn read can produce. */
        assert!(matches!(
            read(":tmi.twitch.tv"),
            Err(Refusal::PrefixUnterminated)
        ));
        /* Tags and prefix arrived and the command had not. */
        let upto_command = match whole.find("PRIVMSG") {
            Some(at) => &whole[..at],
            None => panic!("the fixture no longer contains its own command"),
        };
        assert!(matches!(read(upto_command), Err(Refusal::NoCommand)));
        /* Nothing at all, and a bare line ending, which a keepalive can produce. */
        assert!(matches!(read(""), Err(Refusal::Empty)));
        assert!(matches!(read("\r\n"), Err(Refusal::Empty)));

        /* The guard that stops this being passed by a parser which refuses everything. */
        assert!(read(whole).is_ok());
        assert!(parse(PRIVMSG_EMOTICON).is_some());
        assert!(parse(&whole[..40]).is_none());
    }

    /// THE DEFECT: a token going missing and the next one sliding into the command's place. What
    /// arrives then is a line whose "command" is a channel name, and a dispatcher matching on
    /// known commands will simply not match it, which is indistinguishable from a message type
    /// this build has not implemented yet. One of those needs a bug report and the other needs a
    /// feature, so they must not land in the same counter.
    #[test]
    fn a_channel_name_in_the_command_slot_is_refused_rather_than_dispatched() {
        let whole = CLEARCHAT.trim_end_matches(['\r', '\n']);
        let mangled = whole.replace("CLEARCHAT #jynxzi", "#jynxzi");
        assert!(
            mangled.contains(":tmi.twitch.tv #jynxzi :sixfivee65"),
            "the mangling did not produce the shape it was meant to: {mangled:?}"
        );
        assert!(matches!(read(&mangled), Err(Refusal::CommandNotAWord)));

        /* Three digits IS a command, and the handshake is nothing but. A rule that rejected
         * "#jynxzi" by rejecting everything non-alphabetic would take the whole connect sequence
         * with it, so both halves are asserted here rather than assumed. */
        assert_eq!(cut(WELCOME_001).command, "001");
        assert_eq!(cut(MOTD_END_376).command, "376");
        assert_eq!(cut(CLEARCHAT).command, "CLEARCHAT");
    }

    /// THE DEFECT: a doubled space absorbed as though it were one. Zero of the 6117 captured lines
    /// have one, so a line that does means either our own framing has gone wrong or the far end
    /// has changed, and both want a counter rather than a message. Absorbing it would hide the
    /// first case completely: a reader that started inserting a space would keep working.
    ///
    /// HONESTY ABOUT THIS FIXTURE: a real line with one byte inserted. It asserts this module's
    /// rule, and is not evidence about what the server does.
    #[test]
    fn a_doubled_space_is_refused_rather_than_absorbed() {
        let whole = NAMES_353.trim_end_matches(['\r', '\n']);

        let doubled = whole.replace("353 justinfan73921", "353  justinfan73921");
        assert_ne!(doubled, whole, "the fixture did not actually gain a space");
        assert!(matches!(read(&doubled), Err(Refusal::BlankParam)));

        /* A doubled space in front of the trailing marker leaves the extra one at the END of the
         * middle span, which is the variant a check on the command alone would miss. */
        let before_marker = whole.replace("#broken_stoic :", "#broken_stoic  :");
        assert_ne!(before_marker, whole);
        assert!(matches!(read(&before_marker), Err(Refusal::BlankParam)));

        /* And spaces inside a BODY are untouched by any of that. */
        assert_eq!(
            cut(MOTD_372).trailing,
            Some("You are in a maze of twisty passages, all alike.")
        );
    }

    /// THE DEFECT: someone changing a `&'a str` to a `String` because it was convenient at one
    /// call site. This is the hot path of a reader that sees over a thousand lines a minute on a
    /// busy channel and already owns every byte it is handing over; five owned copies per line is
    /// a cost that never shows up as a slow function, only as a number. A type signature states
    /// the intent, and pointer identity is the only thing that PROVES no copy happened.
    #[test]
    fn every_part_points_into_the_callers_own_bytes() {
        let mut parts_checked = 0;
        let mut params_checked = 0;

        for raw in CAPTURED {
            let trimmed = raw.trim_end_matches(['\r', '\n']);
            let start = trimmed.as_ptr() as usize;
            let end = start + trimmed.len();
            let line = cut(raw);

            let mut on_this_line = 0;
            for part in [line.tags, line.prefix, line.trailing, Some(line.command)]
                .into_iter()
                .flatten()
                .chain(line.params.iter())
            {
                let at = part.as_ptr() as usize;
                assert!(
                    at >= start && at + part.len() <= end,
                    "a part was copied instead of borrowed: {part:?}"
                );
                on_this_line += 1;
            }
            parts_checked += on_this_line;
            params_checked += line.params.len();

            /* Two is the floor rather than five because `PING :tmi.twitch.tv` genuinely has only a
             * command and a trailing. Without a floor of some kind the loop would pass on a `Line`
             * whose every field was None or empty. */
            assert!(
                on_this_line >= 2,
                "this line yielded too few parts to prove anything: {raw:?}"
            );
        }

        /* The per line floor above is satisfied by command plus trailing alone, so these two say
         * the corpus really did put every KIND of part through the check, params included. */
        assert!(
            parts_checked >= 60,
            "the corpus stopped covering enough parts to mean anything: {parts_checked}"
        );
        assert!(
            params_checked >= 20,
            "no middle parameters were checked, which is the part most likely to be rebuilt as an \
             owned Vec: {params_checked}"
        );
    }
}
