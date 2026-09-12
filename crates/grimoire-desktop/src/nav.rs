//! The regrouped nav: PLAY CRAFT FIND CHARACTER SKY GROUP STOIC. Decision D5.
//!
//! WHY THE ROWS ARE BOUND TO AN ENUM AND NOT TO A STRING.
//! The web app's rail was a list of labels and the body switched on the label text, which is the
//! kind of coupling that survives every rename except the one that matters. Here every row carries
//! a `ScreenId`, the body switches on that, and a test below proves each variant appears in the
//! table exactly once: a screen that is built but unreachable, or a row that leads nowhere, fails
//! the build rather than shipping as a dead drawer.
//!
//! WHY THE LEADING SQUARE IS COMPUTED HERE AND NOT IN EACH SCREEN.
//! `square` is the one place that answers "does this row have what it needs", so the answers can
//! be read side by side and argued with. A screen deciding its own square would be twenty nine
//! opinions about what Settled means. The rules are deliberately conservative: nothing the rail can
//! see runs in the background except the watcher and the snapshot loader, so Working is returned
//! only while that loader runs, and a row only goes Settled when the thing it draws from is
//! actually present.
//!
//! WHY THE SQUARE IS A PURE FUNCTION OF A `Facts` STRUCT.
//! `Cx` lends a live `Ingest`, and a test cannot plant a log file inside one without touching the
//! disk and the reader thread. So `facts` boils the context down to a handful of booleans first
//! (`facts`) and `square` decides from those alone. The tests plant facts directly, which is how
//! every branch below is covered rather than only the ones an empty machine happens to hit.
//!
//! WHY NO ROW CARRIES A BADGE.
//! It used to answer a number for five rows, and every one of those numbers was the length of a
//! file that ships with the binary: the same digits on every launch forever. The owner asked for
//! them out twice. The count rule, and the pill `chrome` drew it as, are both deleted rather than
//! left uncalled; the block where they stood says what would have to come back and, more to the
//! point, what a badge would have to MEAN before one does.

use crate::chrome::State;
use crate::ingest::Source;
use crate::screens::Cx;
use std::path::Path;

/// Every screen the body can show. One variant per nav row, and the test below holds that to be
/// exactly true in both directions.
///
/// DECLARATION ORDER IS NOT RAIL ORDER. It was, until Standing moved from STOIC to CRAFT; `ordinal`
/// is a stable slot per variant (the App indexes a per screen array by it, main.rs), and renumbering
/// it to chase a rail reshuffle would move every screen's slot for a cosmetic reason. `NAV` is the
/// order the rail draws and `find` is the only mapping between the two.
///
/// REMOVING A VARIANT IS THE ONE THING THAT DOES RENUMBER IT, and Sources leaving did, then Drops.
/// The
/// ordinals have to stay dense, because `ALL` is a permutation of them and the App's tab array is
/// `[usize; ALL.len()]`, so a hole would be an index nothing answers for. That costs nothing here
/// and it is worth saying why: the only thing indexed by an ordinal is which of a two faced row's
/// tabs you were last on, that array lives on the App and is never written to disk, so a slot
/// moving is invisible across a launch. It would NOT be free if an ordinal were ever persisted.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ScreenId {
    /* FIVE ROWS LEFT THIS LIST WHEN THE SECTIONS ARRIVED, AND EVERY ONE OF THEM WAS A DOOR ONTO A
     * VIEW THAT NOW HAS ITS OWN. `Fights` was the parser's fights view, `Islands` and `Keys` were
     * the sky screen's island and ladder views, `Raid` and `Motes` were two of the group finder's
     * three modes. Each had been a destination only because tier 3 did not exist to hold it, and
     * once it did they were two names for one place: pressing `Islands` and pressing PLANE OF SKY /
     * By island did the identical thing and the rail lit differently for each.
     *
     * NOTHING WAS LOST WITH THEM. The views are all still reachable, under the destination they
     * belong to, which is the whole of what this cost. */
    Commission,
    WorkOrders,
    Workshop,
    Parser,
    KillTracker,
    Loot,
    Items,
    Zones,
    Quests,
    Spells,
    Inventory,
    Gear,
    Exalt,
    Trio,
    Aa,
    Levelling,
    Sky,
    Lfg,
    SpawnTimers,
    Watch,
    Videos,
    Standing,
    /* CHAT HAS A SCREEN AND SO SITS WITH THE ROWS THAT DO. It was declared inside the block below
     * for rows this build has no screen for, and `square`'s own Chat arm says in as many words
     * that it is not in that group any more. A reader of this enum was told the opposite of what
     * the router does with it. */
    Chat,

    /* THE ROWS THE OWNER'S RAIL NAMES THAT THIS BUILD HAS NO SCREEN FOR.
     *
     * They are declared rather than left out, and that is the whole decision. A rail drawn to a
     * design, with the unbuilt half silently missing, is a rail that lies about the shape of the
     * product: the reader cannot tell what is coming from what was forgotten. Every one of these
     * lands on the same honest page, `main::unbuilt`, which names what is missing rather than
     * apologising. `UNBUILT` below is where each one says its own reason. */
    Gina,
    CharacterSheet,
    Loadouts,
    Theorycraft,
    Compendium,
    Guild,
    Collections,
    Archive,
    Bestiary,
    LootTables,
    Crafting,
    RaidTargets,
    Schedule,
    RaidHistory,
    Lockouts,
    FarmPlan,

    /// The achievements dump, a section of PLANE OF SKY. It was reachable only as a tab of a
    /// row that no longer exists.
    Achievements,

    /* THE TWO HALVES OF WHAT THE BAZAAR SELLS, both unwritten.
     *
     * A `TradeHall` STOOD HERE FOR ONE BUILD AND SHOULD NOT HAVE. It was a second destination
     * holding the commission work, split from Bazaar on a goods-versus-services argument that
     * was mine and not the owner's. The Bazaar sells both: things people make and things mobs
     * drop. Splitting them put one market behind two doors and made a reader ask which one a
     * crafted sword was behind. The commission work is sections of Bazaar now. */
    TradeGoods,
    DroppedItems,

    /* FOUR OF LOG PARSER'S FIVE SECTIONS, AND ALL FOUR ARE WRITTEN.
     *
     * This block said they were "declared and unwritten" and that each says on its own page
     * what it waits on, which was true when the ids landed and stopped being true when the
     * screens did. `Dashboards`, `Live`, `Reports` and `Logs` are routed by `draw_screen` and
     * none of them reaches `main::unbuilt` any more; the fifth section is Fights, drawn by the
     * Parser screen itself and so needing no id.
     *
     * THEY ARE IDS RATHER THAN BARE NAMES for the reason every section that is a screen of its
     * own is: an id is what `draw_screen` hops to and what `main::on_tab` routes a tab row by. */
    ParserLive,
    ParserReports,
    ParserDashboards,
    ParserLogs,
}

impl ScreenId {
    /// Every variant, in declaration order.
    ///
    /// Kept honest by `ordinal`: that match is exhaustive, so adding a variant without adding it
    /// here fails to compile, and the test that checks `ALL` against the ordinals fails if the two
    /// ever disagree in length.
    pub const ALL: [ScreenId; 46] = [
        ScreenId::Commission,
        ScreenId::WorkOrders,
        ScreenId::Workshop,
        ScreenId::Parser,
        ScreenId::KillTracker,
        ScreenId::Loot,
        ScreenId::Items,
        ScreenId::Zones,
        ScreenId::Quests,
        ScreenId::Spells,
        ScreenId::Inventory,
        ScreenId::Gear,
        ScreenId::Exalt,
        ScreenId::Trio,
        ScreenId::Aa,
        ScreenId::Levelling,
        ScreenId::Sky,
        ScreenId::Lfg,
        ScreenId::SpawnTimers,
        ScreenId::Watch,
        ScreenId::Videos,
        ScreenId::Standing,
        ScreenId::Gina,
        ScreenId::CharacterSheet,
        ScreenId::Loadouts,
        ScreenId::Theorycraft,
        ScreenId::Compendium,
        ScreenId::Guild,
        ScreenId::Chat,
        ScreenId::Collections,
        ScreenId::Archive,
        ScreenId::Bestiary,
        ScreenId::LootTables,
        ScreenId::Crafting,
        ScreenId::RaidTargets,
        ScreenId::Schedule,
        ScreenId::RaidHistory,
        ScreenId::Lockouts,
        ScreenId::FarmPlan,
        ScreenId::Achievements,
        ScreenId::TradeGoods,
        ScreenId::DroppedItems,
        ScreenId::ParserLive,
        ScreenId::ParserReports,
        ScreenId::ParserDashboards,
        ScreenId::ParserLogs,
    ];

    /// A stable position per variant. Exhaustive on purpose: see `ALL`.
    pub fn ordinal(self) -> usize {
        match self {
            ScreenId::Commission => 0,
            ScreenId::WorkOrders => 1,
            ScreenId::Workshop => 2,
            ScreenId::Parser => 3,
            ScreenId::KillTracker => 4,
            ScreenId::Loot => 5,
            ScreenId::Items => 6,
            ScreenId::Zones => 7,
            ScreenId::Quests => 8,
            ScreenId::Spells => 9,
            ScreenId::Inventory => 10,
            ScreenId::Gear => 11,
            ScreenId::Exalt => 12,
            ScreenId::Trio => 13,
            ScreenId::Aa => 14,
            ScreenId::Levelling => 15,
            ScreenId::Sky => 16,
            ScreenId::Lfg => 17,
            ScreenId::SpawnTimers => 18,
            ScreenId::Watch => 19,
            ScreenId::Videos => 20,
            ScreenId::Standing => 21,
            ScreenId::Gina => 22,
            ScreenId::CharacterSheet => 23,
            ScreenId::Loadouts => 24,
            ScreenId::Theorycraft => 25,
            ScreenId::Compendium => 26,
            ScreenId::Guild => 27,
            ScreenId::Chat => 28,
            ScreenId::Collections => 29,
            ScreenId::Archive => 30,
            ScreenId::Bestiary => 31,
            ScreenId::LootTables => 32,
            ScreenId::Crafting => 33,
            ScreenId::RaidTargets => 34,
            ScreenId::Schedule => 35,
            ScreenId::RaidHistory => 36,
            ScreenId::Lockouts => 37,
            ScreenId::FarmPlan => 38,
            ScreenId::Achievements => 39,
            ScreenId::TradeGoods => 40,
            ScreenId::DroppedItems => 41,
            ScreenId::ParserLive => 42,
            ScreenId::ParserReports => 43,
            ScreenId::ParserDashboards => 44,
            ScreenId::ParserLogs => 45,
        }
    }
}

/// THE RAIL: A LAUNCHER AND SIX REALMS, held to exactly that by
/// [`the_rail_is_the_launcher_and_six_realms`].
///
/// # THIS DOC USED TO DRAW A RAIL THAT NO LONGER EXISTS
///
/// It printed the D5 table as a code block: seven sections called PLAY, CRAFT, FIND, CHARACTER,
/// SKY, GROUP and STOIC, with rows named Fights, Islands, Keys, Raid and Motes. Not one of those
/// seven headings is in [`NAV`] and several of those rows have been folded into sections of other
/// screens. A reader trusting it would have gone looking for a FIND heading, which is also where
/// the snapshot footer used to send him.
///
/// NO SECOND COPY OF THE TABLE IS WRITTEN HERE, and that is the lesson rather than the tidy-up: a
/// map of a structure, kept next to the structure, is a second definition with no guard on it. The
/// headings are in `NAV` twenty lines down and the test above holds their number and their names.
///
/// WHY STANDING IS UNDER CRAFT AND NOT STOIC.
/// It sat under STOIC because the word reads like a streaming thing. It is not one. Standing is the
/// regard a trading partner is held in on the EverQuest faction ladder: `grimoire_core::Regard`,
/// the rungs Dubiously through Ally, with Indifferently where an unproven hand starts so that a
/// reputation system does not become a closed shop. It exists to gate commissions and nothing else.
/// `order::Terms` carries `least_regard`, and the engine's MayCommission op weighs a hand's standing
/// against a crafter's terms alongside same_server and same_guild, so a crafter who will not take
/// work from a Dubious buyer is answered by this rung. The web app had it under IN THREAD beside
/// Offer, Materials and Delivered, which is the commission thread. CRAFT is this build's name for
/// that work, so Standing belongs with Work orders and Workshop.
///
/// AND SOURCES WAS UNDER CRAFT UNTIL THIS BUILD AND HAS LEFT IT ALTOGETHER.
/// It is not a place in the product at all, it is the app account of its own plumbing: which
/// files on this machine the ingest read, when, and what came out. The owner put it plainly, that
/// there is no reason for sources in craft, and the name it was holding the slot with belongs to a
/// different screen: the web app Sources row under Tradesman is "where a component comes from",
/// vendor, drop and forage rows for a component (app.html, from line 2338), and it says nothing
/// about files. So the ledger folded into the Settings screen, under the LOG FOLDER field that
/// answers every question it raises, and this table lost a row rather than gaining a better one.
/// `screens::sources` went with it; `settings::SettingsScreen::sources` is where it lives now.
///
/// AND DROPS HAS LEFT FIND, WHICH IS A DIFFERENT REASON AGAIN AND WORTH KEEPING APART FROM IT.
/// Sources left because it was not a place in the product. Drops left because it was the SECOND
/// door onto one. The owner put it plainly, that the user does not need to see drops, and the code
/// agreed with him: `screens::drops` drew its search box, its tables, its rows and its detail pane
/// out of `screens::items`, and the Items detail pane already prints the same quest-items dropper
/// rows under the item they belong to (`Snapshot::drop_sources`, mob and zone, every row). A rail
/// row whose whole screen is another screen's furniture over data that screen already shows is a
/// row that splits one answer across two places.
///
/// WHAT LEAVING COST, MEASURED RATHER THAN WAVED AT, because a row is not free to remove. The drop
/// table keys 1637 item names and only 682 of them are gear-data items, so the Items screen is not
/// where the other 955 live. 1615 of the 1637 are named on a Zone: `screens::zones` indexes the
/// same table by zone and prints "from quest-items.json drops" with the item and its mob under the
/// zone page. That leaves 14 names in the file that no screen prints any more, because the only
/// zone their rows name is one no Zone record answers to. They are in the snapshot and reachable
/// through `Snapshot::drop_sources`; nothing draws them. That is the price, it is written here
/// rather than discovered later, and it is a ticket.
///
/// THE `d:` FIND PREFIX WENT WITH THE SCREEN and could not have stayed. A prefixed hit is routed by
/// `main::ask_for` to the screen that owns its kind, and with no Drops screen there is no owner:
/// pointing a drop hit at Items would have dead ended 955 of 1637 names on "no item under this
/// name", which is a worse answer than the one the finder gives now. So `data::HitKind::Drop` is
/// gone too, and with it `Drop::hit`, `detail`, `matches` and `zones`, which nothing but that hit
/// reached. The RECORDS stay: `Snapshot::drops`, its index, `drop_sources` and `droppers` are what
/// Items and Zones draw from.
///
/// WHY OFFER, MATERIALS AND DELIVERED DID NOT COME WITH IT.
/// The case for bringing the other three IN THREAD rows back is that the four were the stages of
/// one commission. That is the reason they stay out: A STAGE IS NOT A PLACE, and that is the whole
/// test. Offer, Materials and Delivered are `order::Phase` seen from inside ONE order, and the web
/// screens say so in their own markup, every one of them pinned to `# order-4417 · private thread`
/// and naming Grimtooth. Each has to be handed an order before it means anything, so from a rail it
/// is not somewhere you can be, it is a stage you can be at. When an order store lands they belong
/// INSIDE an order opened from Work orders, as its phases, not as rail rows that each need an order
/// picked somewhere else first. Standing passes the test they fail: a rung is a property of a HAND
/// and not of an order, so it reads with no order open.
///
/// AND STANDING IS NOT EXEMPT FROM THE UNBUILT PAGE, WHICH THE OLD WORDING HERE CLAIMED FOR IT.
/// This note used to disqualify the other three with "every one of them would land on Not built in
/// this release", and then exempt Standing by saying it "does not have that problem". Neither half
/// survives. The first does not discriminate and the second is false: eight rows in this table land
/// on that page today and Standing is one of them, listed with the rest in [`UNBUILT`]. Being
/// unbuilt cannot be what keeps a row out, because if it were, seven built-in-name-only rows would
/// have to go with it. What keeps a row out is having no place of its own to stand, which is a
/// question about the SHAPE of the thing and not about what this release got round to building.
/// The one heading that is a BUTTON rather than a fold.
///
/// It is drawn lit gold at all times, where every other shut heading rests in shadow, because it
/// is the primary act of the whole app and not a drawer to open. That is the whole of what this
/// constant decides; whether a heading FOLDS is a separate question with a separate answer, which
/// is simply whether it has rows.
///
/// A STRING AND NOT A FLAG ON THE ROW, because adding a third element to every tuple in the table
/// below would be nine edits to record one fact about one of them.
/// `the_launcher_is_a_heading_that_really_exists` is what stops this drifting off a rename.
pub const LAUNCHER: &str = "ENTER NORRATH";

/* ONE TABLE ANSWERS WHERE EVERY SCREEN IS.
 *
 * A heading here is a SECTION of the rail and its rows are the screens under it. `crumb` builds
 * `section/screen` out of this and nothing else, so a row moving between sections moves its crumb
 * with it and there is no second list to forget.
 *
 * THE HEADINGS ARE THE OWNER'S AND SO IS THEIR ORDER: the launcher, then Chronicle, My Legend,
 * Compendium, The Tavern, War Council and Broken Stoic, then GENERAL.
 *
 * GENERAL IS THE DRAWER AND IT IS LAST ON PURPOSE. The owner's standing rule for it is that a row
 * the rail does not name is kept rather than hidden, so everything this app can do stays one
 * click away whether or not the design has found a home for it yet. Its own order is the order
 * those rows arrived in, which is the only thing left carrying the grouping they used to have.
 *
 * A ROW MAY NAME A SCREEN THAT DOES NOT EXIST, and many do. That is deliberate and it is argued
 * at the foot of `ScreenId`: a rail drawn to a design with the unbuilt half silently missing
 * cannot be read, because the reader cannot tell what is coming from what was forgotten. Every
 * one lands on `main::unbuilt` saying what it would be and what it waits on (`UNBUILT`).
 *
 * NO COUNT IS WRITTEN HERE, AND THAT IS THE SECOND TIME THIS PARAGRAPH HAS BEEN WRONG. It said
 * seventeen, then the number moved twice and nobody came back. `the_unbuilt_route_and_the_
 * unbuilt_list_hold_the_same_rows` is what actually holds the two lists together; a number in
 * prose is a copy of that with no guard on it.
 *
 * AND IT DOES NOT PROMISE A HOLLOW RING. The rail paints no mark at all for `State::Idle`;
 * `square` returns it and `rail_plan` draws nothing. The ring is in `chrome`'s vocabulary and
 * is used by pages, not by this list. */
pub const NAV: &[(&str, &[(&str, ScreenId)])] = &[
    ("ENTER NORRATH", &[]),
    (
        "CHRONICLE",
        &[("GINA", ScreenId::Gina), ("Log Parser", ScreenId::Parser)],
    ),
    (
        /* MY LEGEND IS YOUR CHARACTER: what it is, what it carries, what it has done, where it
         * has been, and what you are planning for it.
         *
         * THE PLANNING ROWS ARE BACK HERE AFTER A DETOUR. They spent one build under a heading I
         * invented called THE FORGE, on an argument that a plan is a different KIND of thing from
         * a record. That may even be true, but the owner did not ask for the heading and the name
         * read as crafting, which is the Bazaar's job. A loadout belongs to a character, so it
         * lives with the character. */
        "MY LEGEND",
        &[
            ("Character Sheet", ScreenId::CharacterSheet),
            ("Inventory", ScreenId::Inventory),
            ("Gear", ScreenId::Gear),
            ("Exaltations", ScreenId::Exalt),
            ("Loadouts", ScreenId::Loadouts),
            ("Theorycraft", ScreenId::Theorycraft),
            ("Trio", ScreenId::Trio),
            ("AA plans", ScreenId::Aa),
            ("Farm plans", ScreenId::FarmPlan),
            ("Collections", ScreenId::Collections),
            ("Archive", ScreenId::Archive),
            ("Hunt Journal", ScreenId::KillTracker),
            ("Loot Journal", ScreenId::Loot),
            ("Plane of Sky", ScreenId::Sky),
            ("Raid History", ScreenId::RaidHistory),
            ("Lockouts", ScreenId::Lockouts),
        ],
    ),
    (
        /* THE COMPENDIUM IS WHAT THE GAME CONTAINS, the same for every player. */
        "COMPENDIUM",
        &[
            ("Search All", ScreenId::Compendium),
            ("Continents & Zones", ScreenId::Zones),
            ("Bestiary", ScreenId::Bestiary),
            ("Raid Targets", ScreenId::RaidTargets),
            ("Items", ScreenId::Items),
            ("Loot Tables", ScreenId::LootTables),
            ("Quests", ScreenId::Quests),
            ("Spells & Abilities", ScreenId::Spells),
            ("Crafting", ScreenId::Crafting),
            ("Levelling", ScreenId::Levelling),
            ("Spawn Timers", ScreenId::SpawnTimers),
        ],
    ),
    (
        /* THE BAZAAR IS THE MARKET, and it is one heading because everything in it is for sale in
         * the same place: things people make and things mobs drop.
         *
         * IT SITS AFTER THE COMPENDIUM ON PURPOSE. You look a thing up and then you go and buy it,
         * so the reading order matches the order you do it in.
         *
         * COMMISSION LEADS AND IT IS THE ONE ROW THAT RUNS. It prices an order through the
         * in-process engine. The three beside it are the ledger around it and the two after are the
         * stock, and every one of those five waits on a store or a price this build has no source
         * for. Each says so on its own page. */
        "THE BAZAAR",
        &[
            ("Commission", ScreenId::Commission),
            ("Work orders", ScreenId::WorkOrders),
            ("Workshop", ScreenId::Workshop),
            ("Standing", ScreenId::Standing),
            ("Trade goods", ScreenId::TradeGoods),
            ("Dropped items", ScreenId::DroppedItems),
        ],
    ),
    (
        /* THE TAVERN IS OTHER PEOPLE. */
        "THE TAVERN",
        &[
            ("Groups", ScreenId::Lfg),
            ("Guild", ScreenId::Guild),
            ("Schedule", ScreenId::Schedule),
        ],
    ),
    (
        "BROKEN STOIC",
        &[
            ("Watch Live", ScreenId::Watch),
            ("Videos", ScreenId::Videos),
            ("Chat", ScreenId::Chat),
        ],
    ),
];
/* ------------------------------------------------------- tier 3: the sections -- */

/// One section of a destination's workspace: its name, what draws it, and its own tabs.
///
/// THE `Option` ANSWERS TWO DIFFERENT QUESTIONS WITH ONE SHAPE.
///
/// `None` is a VIEW of the destination's own screen. Plane of Sky's five are one screen in five
/// states; there is nothing to route to, so `main::on_section` puts the screen into the state.
///
/// `Some(id)` is a SCREEN OF ITS OWN standing inside another destination. The Tradeskill Hall is
/// four of these. They are ordinary `ScreenId`s and everything that already knows what to do with
/// one still does: `square` reports on them, `UNBUILT` gives the unwritten ones words, and
/// `main::draw_screen` draws them.
///
/// AND THE LAST FIELD IS TIER 4, the views of one section, drawn as tabs on the context bar.
///
/// EMPTY FOR EVERY SECTION THIS BUILD CAN DRAW, which is not a gap in the design but the state of
/// the code: a tier 4 tab is a view of a SELECTED RECORD, and no screen here selects one yet. The
/// tiers that do carry tabs are the ones whose whole subtree is unwritten, where the tab is part
/// of saying what the destination will be. `main::unbuilt` names the tier 3 and tier 4 you are
/// standing in, so a tab here is a live control that changes the page rather than a dead label.
pub type Section = (&'static str, Option<ScreenId>, &'static [&'static str]);

pub const SECTIONS: &[(ScreenId, &[Section])] = &[
    (
        /* THE LOG PARSER IS FIVE SECTIONS AND FOUR OF THEM ARE WRITTEN.
         *
         * FIGHTS CARRIES NO TABS YET AND THAT IS A REFUSAL, NOT AN OVERSIGHT. The design gives it
         * ten (Summary, Damage, Healing, Tanking, Abilities, Buffs & Debuffs, Pets, Threat,
         * Deaths, Timeline) and every one of them describes ONE SELECTED FIGHT. Nothing in this
         * build selects a fight, so listing them now would put ten controls on a WORKING screen
         * that select, highlight and change nothing, which is the state a reader cannot tell
         * from broken. They land with fight selection and not before.
         *
         * A TAB ROW IS DRAWN BY THE SHELL AND ROUTED BY `main::on_tab`, and only two sections here
         * have one: Dashboards, whose eight roles are real views, and Reports, whose three are.
         * Live and Logs carry no tab row, because neither page has a second view to switch to.
         *
         * THIS COMMENT USED TO CLAIM ALL FOUR CARRIED WORKING TABS, and it was written on the day
         * those screens landed, before anything read the number. Thirteen buttons were drawn, lit
         * when pressed, and moved nothing. A doc that describes the intention rather than the code
         * is how that survived being written and read in the same hour. */
        ScreenId::Parser,
        &[
            /* DASHBOARDS LEADS, AND IT LEADS BECAUSE IT IS THE LANDING PAGE.
             *
             * Every other section here answers a question you already have: Live is what is
             * happening right now, Reports is a night you want to hand to somebody, Logs is the
             * file itself. Dashboards is the one that answers `how did I do`, which is the
             * question a reader opening a parser has before they have any other, and it is the
             * only section that reads whether or not a log is being tailed this minute.
             *
             * IT ALSO CHANGES WHAT THE FIRST CLICK SHOWS. `default_section` opens on the first
             * section it can draw, so the order in this list is the order a reader meets the
             * parser in. */
            /* DASHBOARDS CARRIES NO TAB ROW, AND IT USED TO CARRY EIGHT ROLES.
             *
             * `DPS`, `Healer`, `Tank`, `Pet`, `Solo`, `Group`, `Raid Leader` and `Custom` were
             * eight tabs over ONE set of panels in eight different orders. Two of them refused
             * outright: Pet said `No pet in this log.` and Raid Leader said in as many words that
             * nothing in a log line makes a fight a raid.
             *
             * A ROLE IS A CLAIM ABOUT THE PERSON READING AND THIS APP CANNOT MAKE ONE. Nothing in
             * a log line says what anybody was trying to do, so `Healer` was a tab that reordered
             * a damage meter and then apologised on its hover for having no healing breakdown.
             * The owner`s ruling was that roles go entirely, and the page is a grid of tiles the
             * reader arranges himself: see `screens::dashboards::Tile`.
             *
             * THE CONTROLS THAT REPLACED THEM ARE NOT TABS AND DO NOT BELONG IN THIS TABLE. A
             * lock and a widget picker act on the page`s LAYOUT rather than switching between
             * views of it, so they are drawn by the context header directly and this list is
             * empty, which is the honest shape for a section with one view. */
            ("Dashboards", Some(ScreenId::ParserDashboards), &[]),
            /* LIVE CARRIES NO TAB ROW, AND IT USED TO CARRY FOUR DEAD ONES.
             *
             * `Personal Meter`, `Group Meter`, `Encounter Status` and `Live Widgets` were drawn
             * as real buttons: pressing one filled it and turned it FLARE, and the page below did
             * not move by a pixel. `Encounter Status` in particular named the one thing this app
             * refuses to draw, the health bar, whose denominator the log never states.
             *
             * THEY ARE NOT WIRED UP, THEY ARE GONE, because the page has no views to switch
             * between: it draws its three panels and its effects list every time. A tab row over
             * a page with one view is a control for a choice that does not exist. */
            ("Live", Some(ScreenId::ParserLive), &[]),
            ("Fights", None, &[]),
            /* REPORTS TAKES THE PAGE'S OWN THREE, IN THE PAGE'S OWN ORDER.
             *
             * It listed Personal, Group, Raid, Encounter and Session while the screen drew its
             * own row of Session, Personal and Encounters underneath: two rows with overlapping
             * names in different orders, the top one inert. And two of the five could never be
             * honoured, because `reports::group_and_raid_are_not_offered` is a guard keeping
             * exactly those off this page: the log carries no roster, so a Raid report is a
             * heading with the group's numbers under it.
             *
             * `the_parser_tab_rows_are_the_pages_own` is what holds this list to `reports::TABS`
             * now, so the two cannot drift apart again. */
            (
                "Reports",
                Some(ScreenId::ParserReports),
                &["Session", "Personal", "Encounters"],
            ),
            /* LOGS CARRIES NONE EITHER, and two of its four were worse than dead: `Raw Log` and
             * `Search & Filters` sat directly above the section of that page explaining that the
             * ingest publishes no lines, so there is nothing to show and nothing to filter. The
             * app drew a control for a feature and then explained underneath that it has neither. */
            ("Logs", Some(ScreenId::ParserLogs), &[]),
            /* ANALYSIS AND OVERLAYS, WHICH THIS DESTINATION HAS ALWAYS HAD AND COULD NOT REACH.
             *
             * # THEY WERE VIEWS OF A SCREEN NOTHING COULD SELECT
             *
             * `screens::analysis` is a 658 line page that reads ONE fight as deeply as the log
             * supports, and `screens::parser`'s Overlays view is where a combat overlay is made
             * and named. Both were reachable only at `ParserScreen` view indices 3 and 4, and
             * `ParserScreen::show`'s own doc admitted that nothing in the crate ever constructs
             * `View::Analysis` or `View::Overlays`. In the main window the only way in was the
             * in-page view row, which that screen draws ONLY when `Cx::railed` is false, which
             * in the main window it never is.
             *
             * SO THE MAIN WINDOW COULD NOT OPEN EITHER PAGE, AT ALL, and `Settings::fight_notes`
             * (written and read only by Analysis) was a setting no main-window reader could
             * ever produce. That is this tree's signature defect at its largest scale so far:
             * two whole pages, compiled, tested and unreachable.
             *
             * `None` FOR THE SCREEN, LIKE FIGHTS ABOVE, because both are views of the
             * destination's own body screen rather than screens of their own. `main::on_section`
             * is what turns the section into the view, and it looks the index up BY NAME so
             * inserting a section here cannot silently open the wrong page. */
            ("Analysis", None, &[]),
            ("Overlays", None, &[]),
        ],
    ),
    (
        /* THE SKY SCREEN'S OWN VIEWS, IN ITS OWN WORDS, plus the achievements dump that used to
         * hang off a Keys row nobody found. Renaming them would put one name in the rail and
         * another on the page for the same view. */
        ScreenId::Sky,
        &[
            ("By giver", None, &[]),
            ("By island", None, &[]),
            ("Island ladder", None, &[]),
            ("Achievements", Some(ScreenId::Achievements), &[]),
            ("Get rid of", None, &[]),
        ],
    ),
    (
        ScreenId::Lfg,
        &[
            ("Groups", None, &[]),
            ("Raids", None, &[]),
            ("Motes", None, &[]),
        ],
    ),
    (
        ScreenId::Gear,
        &[("Gear score", None, &[]), ("Valet", None, &[])],
    ),
    (
        /* THE GUILD, WHICH IS THE FIRST DESTINATION MAPPED ALL THE WAY DOWN BEFORE ANY OF IT IS
         * WRITTEN, and the first to carry tier 4 at all.
         *
         * EVERY ROW AND EVERY TAB HERE LANDS ON `main::unbuilt`, which names the path you are
         * standing in and then says what the guild would be and what it waits on. That is what
         * makes these live controls rather than decoration: pressing Roster changes the page, and
         * pressing Keys & Flags changes it again, so the shape of the thing is walkable before a
         * line of it exists.
         *
         * IT IS DECLARED RATHER THAN HELD BACK FOR THE SAME REASON THE UNBUILT ROWS ARE. A rail
         * drawn to a design with the unwritten half missing cannot be read: the reader cannot tell
         * what is coming from what was forgotten. The alternative here was one Guild row with
         * nothing under it, which says the guild is a page rather than seven of them. */
        ScreenId::Guild,
        &[
            (
                "Home",
                None,
                &["Overview", "Announcements", "Recent Activity"],
            ),
            (
                "Roster",
                None,
                &[
                    "Members",
                    "Characters & Loadouts",
                    "Keys & Flags",
                    "Raid Readiness",
                ],
            ),
            (
                "Calendar",
                None,
                &["Guild Events", "Raid Schedule", "Availability"],
            ),
            (
                "Events",
                None,
                &["Upcoming", "Sign-ups", "Waitlist", "Past"],
            ),
            (
                "Raids",
                None,
                &["Roster", "Groups & Assignments", "Capabilities"],
            ),
            ("Attendance", None, &["Raids", "Events", "History"]),
            (
                "Logs",
                None,
                &["Shared Encounters", "Parses", "Reports", "Uploads"],
            ),
        ],
    ),
    /* THE LOG PARSER HAS NO LIST AND THAT IS THE POINT OF THIS NOTE.
     *
     * It had one: `Kills | Loot | Fights`. But Kills and Loot are HUNT JOURNAL and LOOT JOURNAL,
     * two destinations of their own in the rail, so listing them here made the same two views
     * reachable twice under two different names, and the rail lit differently depending on which
     * door you came through. What was left after removing them was `Fights` alone, and a section
     * list of one is a caption that costs a row.
     *
     * SO LOG PARSER IS THE FIGHTS DESTINATION. `main::on_view` puts it on that view. The sitemap
     * asks for exactly this in as many words: fights belong to Log Parser, and there is to be no
     * standalone Fights row. There no longer is one. */
];
/// THE RAIL`S OWN WORD FOR A DESTINATION, or `None` for a screen the rail has no row for.
///
/// # THIS IS WHAT THE CONTEXT HEADER`S BREADCRUMB IS BUILT FROM
///
/// `chrome::context_bar` cut a crumb once, on the grounds that the rail already says where you
/// are three times over. The owner asked for it back in as many words, and he asked for it on the
/// row that no longer carries eight role tabs, which is the difference: the crumb is now the only
/// thing on the left of that header.
///
/// OFF `NAV` AND NEVER TYPED AGAIN. A crumb that spelled the destination itself would be a fourth
/// place the words `Log Parser` live, and the one place a reader is most likely to notice them
/// disagreeing with the rail six inches to the left.
///
/// `None` AND NOT A GUESS. `ParserLive` and its three siblings are section screens with no row of
/// their own; a crumb for one of those is built from the row it hangs under plus the section name,
/// which is what the caller has and this function does not.
pub fn name_of(id: ScreenId) -> Option<&'static str> {
    let (head, row) = find(id)?;
    NAV.get(head)
        .and_then(|(_, rows)| rows.get(row))
        .map(|(name, _)| *name)
}

/// The sections of a destination, or nothing when it has one view.
pub fn sections_of(id: ScreenId) -> &'static [Section] {
    SECTIONS
        .iter()
        .find(|(s, _)| *s == id)
        .map(|(_, rows)| *rows)
        .unwrap_or(&[])
}

/// TIER 4: the tabs of one section, or nothing when it has none.
///
/// TAKES BOTH TIERS, because tier 4 belongs to a SECTION and not to a destination. `Guild` alone
/// cannot answer it: Roster's tabs and Calendar's tabs are different lists and neither is the
/// guild's. An out of range section answers with nothing rather than panicking, because the
/// selected section is App state that outlives an edit to this table.
pub fn tabs_of(id: ScreenId, section: usize) -> &'static [&'static str] {
    sections_of(id)
        .get(section)
        .map(|(_, _, tabs)| *tabs)
        .unwrap_or(&[])
}

/// WHICH SECTION A DESTINATION OPENS ON: the first one it can actually draw.
///
/// NOT ZERO, AND THE LOG PARSER IS WHY. When this rule was written the parser`s sections were
/// Live, Fights, Reports, Dashboards and Logs and only Fights could be drawn, so opening on index
/// 0 put a destination whose whole point is combat analysis onto a page saying nothing was built,
/// every time, on the first click. A reader would conclude the parser does not exist.
///
/// THE RULE OUTLIVED THE CONDITION THAT PROMPTED IT, WHICH IS THE POINT. Four of those five
/// sections have screens now and Dashboards leads the list, so for the parser this function
/// returns 0 and the fallback never fires. It still fires for every destination whose first
/// section is a screen that has not been written, and there are plenty.
///
/// THE RULE IS GENERAL AND NOT A SPECIAL CASE FOR ONE ROW. A section that is a view of the
/// destination`s own screen can always be drawn. A section that is a screen of its own can be
/// drawn when that screen exists, which is exactly what `unbuilt_why` answers. Fall back to zero
/// when nothing qualifies, because a destination whose every section is unwritten is unwritten
/// itself and its first section is as good a place to say so as any.
pub fn default_section(id: ScreenId) -> usize {
    sections_of(id)
        .iter()
        .position(|(_, inner, _)| match inner {
            None => true,
            Some(screen) => unbuilt_why(*screen).is_none(),
        })
        .unwrap_or(0)
}

/// The destination a screen stands inside, and where in its list, when it is a section rather than
/// a row of the rail. `None` for every ordinary destination.
pub fn parent_of(id: ScreenId) -> Option<(ScreenId, usize)> {
    SECTIONS.iter().find_map(|(owner, rows)| {
        rows.iter()
            .position(|(_, sec, _)| *sec == Some(id))
            .map(|at| (*owner, at))
    })
}

pub fn default_open() -> Vec<usize> {
    ["CHRONICLE", "BROKEN STOIC"]
        .iter()
        .filter_map(|want| NAV.iter().position(|(head, _)| head == want))
        .collect()
}

/// Where a screen sits in the table: (section index, row index).
pub fn find(id: ScreenId) -> Option<(usize, usize)> {
    NAV.iter()
        .enumerate()
        .find_map(|(si, (_, rows))| rows.iter().position(|(_, r)| *r == id).map(|ri| (si, ri)))
}

/// The label a screen is listed under, whether it is a row of the rail or a section nested in one.
pub fn label(id: ScreenId) -> &'static str {
    if let Some((si, ri)) = find(id) {
        return NAV[si].1[ri].0;
    }
    parent_of(id)
        .map(|(owner, at)| sections_of(owner)[at].0)
        .unwrap_or("")
}

/// The rail heading a screen lives under. A section reports its destination's heading, because
/// that is where a reader would go looking for it.
pub fn section_of(id: ScreenId) -> &'static str {
    if let Some((si, _)) = find(id) {
        return NAV[si].0;
    }
    parent_of(id)
        .map(|(owner, _)| section_of(owner))
        .unwrap_or("")
}

/* ------------------------------------------------------------- the file names -- */

/// What the EQ client names its log files: `eqlog_<character>_<server>.txt`, in any case, with at
/// least one character between the prefix and the extension. This is the one rule that decides
/// whether a file in the Logs folder is a log at all.
pub fn looks_like_eq_log(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    lower.starts_with("eqlog_") && lower.ends_with(".txt") && lower.len() > "eqlog_.txt".len()
}

/// What `/outputfile inventory` writes: `<Char>_<server>-Inventory.txt`, matched on the suffix in
/// any case.
pub fn looks_like_inventory_dump(path: &Path) -> bool {
    ends_with_ci(path, "-inventory.txt")
}

/// What `/outputfile achievements` writes: `<Char>_<server>-Achievements.txt`, matched on the
/// suffix in any case.
pub fn looks_like_achievements_dump(path: &Path) -> bool {
    ends_with_ci(path, "-achievements.txt")
}

fn ends_with_ci(path: &Path, suffix: &str) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.to_ascii_lowercase().ends_with(suffix))
        .unwrap_or(false)
}

/* ------------------------------------------------------------------ the facts -- */

/// The ingest's own words for a log it lists but does not read (ingest.rs, `sources()`): only the
/// most recently written log is tailed, and every other one carries this note in `problem`. It is
/// information, not a fault, so the rail must not go red on it. Matched by prefix; the test
/// `not_tailed_note_is_not_a_fault` guards the coupling.
pub const NOT_TAILED_PREFIX: &str = "not tailed:";

/// What the ingest has found, boiled down to the facts the rail decides on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sources {
    /// An `eqlog_*.txt` was found.
    pub log: bool,
    /// A found log reported a problem that is not the `not tailed` note.
    pub log_fault: bool,
    /// A `*-Inventory.txt` was found.
    pub inventory: bool,
    /// A found dump reported a problem.
    pub inventory_fault: bool,
    /// A `*-Achievements.txt` was found.
    ///
    /// # NO SQUARE READS THIS, AND THAT IS RECORDED RATHER THAN QUIETLY TRUE
    ///
    /// It feeds one arm of [`square`], for `ScreenId::Achievements`. That screen is not a row of
    /// [`NAV`]: it is a SECTION of PLANE OF SKY, and `rail_plan` is the only production caller of
    /// `square` and it walks `NAV`. So the arm is correct and undrawn, and it is kept for the
    /// reasons written on it thirty lines below.
    ///
    /// THE FIELD STAYS BECAUSE THE DETECTION IS REAL AND IS READ. `source_files` counts the dump
    /// for the Settings screen, and `the_achievements_dump_and_spawn_timers_and_videos` measures
    /// that a dump is told apart from a log, a snapshot and a respawn interval. What was wrong was
    /// a rail square drawn from it, not the fact itself.
    ///
    /// AND THE SWEEP NAMES IT INSTEAD OF PASSING BY ACCIDENT. See
    /// `every_fact_changes_some_row_or_it_is_computed_for_nobody`: that guard was green on this
    /// field because every `src.*` case also flipped `log_dir` and was compared against a baseline
    /// with no log folder, so the log rows answered every comparison. Ten entries, none testing.
    pub achievements: bool,
    /// A found achievements dump reported a problem. Drawn by no rail row: see
    pub achievements_fault: bool,
}

/// Where the crafting corpus (`web/corpus.grim`, read by the Commission screen through the
/// in-process engine) stands. The App reads it off the screen each frame; a `Cx` alone cannot
/// see it, so `facts` leaves it at `Reading` and the App overwrites.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Corpus {
    /// The reader thread has not answered yet. The App starts it at launch, so this is the state
    /// for the first few frames and then never again.
    #[default]
    Reading,
    Ready,
    /// No corpus.grim at any candidate path, or one that did not open. The screen prints why.
    Failed,
}

/// Classify what the ingest lists.
///
/// FILES AND PLACEHOLDERS ARE NOT THE SAME THING. The ingest lists a row for a folder it looked in
/// and found nothing (the Logs folder with "No eqlog_*.txt files", the game folder with "No
/// *-Inventory.txt found"), and those rows carry that note in `problem`. A found file that could
/// not be read is a fault and earns Wrong; a folder with nothing in it yet is a gap and earns Idle.
/// The two are told apart by the path: a placeholder is a directory, a source is a file the name
/// rules above recognise. The SOURCES section of the Settings screen prints the words
/// either way; the square only has one bit and it errs toward the hollow ring rather than a red
/// square on a fresh install before the game has written a single line.
pub fn summarise(sources: &[Source]) -> Sources {
    let mut s = Sources::default();
    for src in sources {
        let fault = src
            .problem
            .as_deref()
            .is_some_and(|p| !p.starts_with(NOT_TAILED_PREFIX));
        /* Classified by file name rather than by the ingest's own kind enum, because the file name
         * rules are the client's names and are tested here, and because the ingest's kind is
         * also on its placeholder rows, which are not files. */
        if looks_like_eq_log(&src.path) {
            s.log = true;
            s.log_fault |= fault;
        } else if looks_like_inventory_dump(&src.path) {
            s.inventory = true;
            s.inventory_fault |= fault;
        } else if looks_like_achievements_dump(&src.path) {
            s.achievements = true;
            s.achievements_fault |= fault;
        }
    }
    s
}

/// Everything `square` decides from. Built from a `Cx` by `facts`, planted directly by tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    /// Settings names a Logs folder. Without one, every log reader is a You.
    pub log_dir: bool,
    pub src: Sources,
    /// THE ENGINE FOLDED AT LEAST ONE FIGHT OUT OF THE TAIL. `Ingest::fights` is the bootstrap
    /// fold, written once when the app starts, and it is what the Log Parser destination draws.
    ///
    /// A LOG BEING TAILED IS NOT THE SAME FACT, which is why this is its own bit rather than
    /// `src.log`. A person can be standing in a bank for an hour with a log that is being read
    /// line by line and no combat in any of it: the two journals settle on that log because a kill
    /// list with nothing in it is still a kill list of that file, and the fights page has nothing
    /// at all. Settling Log Parser off `src.log` would put a filled square beside a page that
    /// says, correctly, that nothing in the tail attacked anything.
    pub fights: bool,
    /// THE INGEST'S BOOTSTRAP FOLD IS STILL ON THE WORKER THREAD (`Ingest::scanning`).
    ///
    /// A 40MB tail takes long enough to read that a rail which went hollow for it would say
    /// "there is nothing here" about a folder it has not finished opening. The Fights page paints
    /// `NoFights::Reading` for this same instant, in words; this is the square that agrees with it.
    pub scanning: bool,
    /// The snapshot loaded.
    pub data: bool,
    /// A snapshot load was attempted and failed.
    pub data_failed: bool,
    /// The snapshot loader thread is running. The App owns that thread and sets this after
    /// `facts`, which cannot see it through `Cx` and leaves it false.
    pub data_loading: bool,
    /// The crafting corpus, owned by the Commission screen; the App sets it after `facts`.
    pub corpus: Corpus,
    /// `settings.extra["lfg_board"]` has entries.
    pub lfg_entries: bool,
    /// The watcher knows Twitch's state, live or not.
    pub twitch_known: bool,
    /// The watcher's last Twitch poll failed.
    pub twitch_error: bool,
    /// The watcher knows YouTube's state, live or not.
    pub youtube_known: bool,
    /// The watcher's last YouTube poll failed.
    pub youtube_error: bool,
}

/// Read the facts off the shared context.
pub fn facts(cx: &Cx) -> Facts {
    let has_entries = match cx.settings.extra.get("lfg_board") {
        Some(serde_json::Value::Array(a)) => !a.is_empty(),
        Some(serde_json::Value::Object(o)) => !o.is_empty(),
        _ => false,
    };
    Facts {
        log_dir: cx.settings.log_dir.is_some(),
        src: summarise(&cx.ingest.sources()),
        fights: !cx.ingest.fights().is_empty(),
        scanning: cx.ingest.scanning(),
        data: cx.data.is_some(),
        data_failed: cx.data_err.is_some(),
        data_loading: false,
        corpus: Corpus::Reading,
        lfg_entries: has_entries,
        twitch_known: cx.live.twitch.live.is_some(),
        twitch_error: cx.live.twitch.error.is_some(),
        youtube_known: cx.live.youtube.live.is_some(),
        youtube_error: cx.live.youtube.error.is_some(),
    }
}

/* ----------------------------------------------------------------- the square -- */

/// The leading square for a row, answered honestly, from facts alone. Pure, so every branch is
/// testable without a disk or a thread; the App builds the facts once per frame with `facts` and
/// asks this for every row.
///
/// The vocabulary, from the Gnomish console: Settled when the row has the data it draws from,
/// Idle when it has nothing (a hollow ring, nothing is happening), You when a person has to act
/// before the row can do anything (the trailing gold bar), Wrong when something the row depends on
/// failed outright. Working is returned for exactly three things, and each one is a thread this
/// process actually started and can see: the snapshot loader, which the App reports through
/// `Facts::data_loading`; the corpus reader the Commission screen starts at launch, reported
/// through `Facts::corpus`; and the ingest's bootstrap fold of the log tail, reported through
/// `Facts::scanning`, which is what Log Parser waits on.
///
/// IT SAID TWO, AND THE THIRD IS NOT A RELAXATION OF THE RULE. The rule was never "two things",
/// it was "never guess": the watcher's `Status` carries no in-flight bit, so a channel row that
/// drew Working would be inventing one, and it still does not. `Ingest::scanning` is a real
/// answer about a real worker, so the row that draws that worker's output may say so.
pub fn square(id: ScreenId, f: &Facts) -> State {
    /* The one question most rows reduce to: is there a log folder, and what is in it. A missing
     * log folder is a You everywhere it matters, because the fix is a person pointing Settings at
     * the EverQuest Legends Logs folder and nothing the app can do alone. */
    let from_log = |present: bool, fault: bool| -> State {
        if !f.log_dir {
            State::You
        } else if fault {
            State::Wrong
        } else if present {
            State::Settled
        } else {
            State::Idle
        }
    };
    /* The snapshot. Settled when loaded, Working while the loader thread is still parsing it (the
     * App owns that thread and reports it through `data_loading`; a row that went red or hollow
     * for the few hundred milliseconds of a parse would lie twice a launch), Wrong when a load was
     * attempted and failed (a file that is there and does not parse is a fault the rail should
     * show in red, not a gap it should show as nothing), Idle when nothing was even found: the
     * FIND screens themselves say where to put the files, per D6. */
    let from_data = || -> State {
        if f.data {
            State::Settled
        } else if f.data_loading {
            State::Working
        } else if f.data_failed {
            State::Wrong
        } else {
            State::Idle
        }
    };
    /* A channel the watcher polls. A known state, live or not, is a fact and earns Settled. A poll
     * that failed with nothing ever known is Wrong, because the row would otherwise sit idle while
     * the network is refusing us. Never checked yet is Idle. */
    let from_channel = |known: bool, error: bool| -> State {
        if known {
            State::Settled
        } else if error {
            State::Wrong
        } else {
            State::Idle
        }
    };

    match id {
        /* PLAY. Every one of these reads the EQ log tail. */
        /* THE TWO JOURNALS READ THE EQ LOG TAIL and settle on it.
         *
         * LOG PARSER IS NOT WITH THEM ANY MORE, and its arm below says why: it is the fights
         * destination now, and a tailing log does not make a fight computable. */
        ScreenId::KillTracker | ScreenId::Loot => from_log(f.src.log, f.src.log_fault),

        /* LOG PARSER SHOWS FIGHTS, AND IT SHOWS REAL ONES NOW.
         *
         * THIS ARM WAS THE CONSTANT `State::Idle`, and the comment on it said fights need a combat
         * engine "which nothing in this build implements", citing the words the Fights page used
         * to print, `screens::parser::FIGHTS_EMPTY`. Both halves stopped being true. The engine is
         * `grimoire_parse::combat` and `grimoire_parse::fights` in this same workspace, this crate
         * mirrors its fights into `crate::fights::FightRow`, `Ingest::fights` holds them, and the
         * Fights page draws a table of them. `FIGHTS_EMPTY` itself was deleted with the sentence
         * it held: the page now says which of four things is missing, through
         * `screens::parser::why_no_fights`. A pinned Idle beside a page listing four fights read
         * off a log that is being tailed right now is the rail contradicting the screen under it,
         * which is exactly the fault the pin was written to avoid, pointed the other way.
         *
         * IT IS NOT `from_log(f.src.log, ..)` EITHER, and that is the point of `Facts::fights`. A
         * log being read is not a fight: an hour in a bank tails a log and folds no combat. The
         * two journals beside this row settle on the FILE because a kill list of a quiet file is
         * still that file's kill list; this row draws FIGHTS, so it answers on fights.
         *
         * THE FOUR ANSWERS LINE UP WITH THE FOUR THE PAGE PRINTS (`NoFights`): no Logs folder is a
         * You (only a person can fix it), a file that would not open is a Wrong, a bootstrap still
         * folding is a Working, and a log being read with no combat in it yet is the honest Idle.
         * `from_log` already decides the first, the second and the last from the same two bits
         * every other log row uses, so this arm adds only the one state it has that they do not. */
        ScreenId::Parser => match from_log(f.fights, f.src.log_fault) {
            State::Idle if f.scanning => State::Working,
            other => other,
        },

        /* CRAFT. Commission is wired to the in-process engine (`screens::commission` calls
         * `grimoire_wasm::call`), but the engine prices nothing without `corpus.grim`, which is a
         * file that can be absent. The screen paints that as a WRONG problem bar, so the rail
         * says the same: the two cannot disagree. Work orders and Workshop have no data source in
         * this build. */
        ScreenId::Commission => match f.corpus {
            Corpus::Ready => State::Settled,
            Corpus::Reading => State::Working,
            Corpus::Failed => State::Wrong,
        },
        /* Standing would settle on a rung read off a trading partner, and nothing in this build
         * holds one: `grimoire_core::Regard` is a type the engine takes, not a record this app
         * keeps. It moved here from STOIC because it gates commissions (see NAV); the square it
         * draws did not change with the move, because the reason for it never was the section. */
        ScreenId::WorkOrders | ScreenId::Workshop | ScreenId::Standing => State::Idle,

        /* THE ROWS THIS RAIL HAS NOTHING TO SAY A SQUARE ABOUT. Idle is the quiet answer: "there
         * is nothing here yet" as opposed to "there is nothing here and that is your fault"
         * (You) or "something broke" (Wrong). Neither of those is true of a row nobody has built.
         *
         * NOT ALL OF THEM ARE UNBUILT ANY MORE, and the caption used to say they were: the four
         * parser sections in this group have screens and are routed, and they answer Idle for a
         * different reason, which is that `rail_plan` only ever asks `square` about `NAV` rows
         * and none of the four is one. Same answer, two reasons, and the old wording claimed the
         * first for all of them.
         *
         * NO COUNT, for the reason the note above `NAV` gives: it said seventeen over twenty-two
         * variants. They share one arm because they share one answer; the moment one
         * of them gains a screen it earns an arm of its own with a real answer in it. */
        ScreenId::Gina
        | ScreenId::CharacterSheet
        | ScreenId::Loadouts
        | ScreenId::Theorycraft
        | ScreenId::Compendium
        | ScreenId::Guild
        | ScreenId::TradeGoods
        | ScreenId::DroppedItems
        | ScreenId::Collections
        | ScreenId::Archive
        | ScreenId::Bestiary
        | ScreenId::LootTables
        | ScreenId::Crafting
        | ScreenId::RaidTargets
        | ScreenId::Schedule
        | ScreenId::RaidHistory
        | ScreenId::Lockouts
        | ScreenId::FarmPlan
        | ScreenId::ParserLive
        | ScreenId::ParserReports
        | ScreenId::ParserDashboards
        | ScreenId::ParserLogs => State::Idle,

        /* FIND. All four come straight from the snapshot. Spells was the odd one out until
         * 2026-09-03: the snapshot carried no spell records at all (gear-data's `effects` map is
         * the item-proc slice, not a spell list) and this arm was a flat Idle with a page under it
         * saying so. spells.json is in the snapshot now, so the row settles with the rest of them
         * and its words came out of `UNBUILT`. Drops was the fifth and its row has left the rail
         * (see NAV); the drop records it read are still in the snapshot, drawn by Items and Zones. */
        ScreenId::Items | ScreenId::Zones | ScreenId::Quests | ScreenId::Spells => from_data(),

        /* CHARACTER. Inventory and Exaltations read the /outputfile inventory dump. Gear score needs
         * the dump AND the item stats from gear-data, so it settles only when both are present.
         * Trio, AA and Levelling are planners with no source wired in this build. */
        ScreenId::Inventory | ScreenId::Exalt => from_log(f.src.inventory, f.src.inventory_fault),
        ScreenId::Gear => {
            let dump = from_log(f.src.inventory, f.src.inventory_fault);
            if dump == State::Settled && !f.data {
                from_data()
            } else {
                dump
            }
        }
        ScreenId::Trio | ScreenId::Aa | ScreenId::Levelling => State::Idle,

        /* SKY. The checklist and the island list are catalogued in sky.json, so they settle with
         * the snapshot; what you HOLD comes from the log and the dump, which the screen reconciles
         * on its own. Keys has two faces: its first tab is the island ladder from sky.json, its
         * second the achievements dump (unlocks::keys reads the /outputfile achievements file,
         * which the ingest lists and reads like the inventory dump). The row settles when EITHER
         * face has its data, goes Working while the snapshot parses, red when either source
         * failed, gold when nothing is loaded and the fix is pointing Settings at the Logs
         * folder. */
        ScreenId::Sky => from_data(),
        /* THE ACHIEVEMENTS DUMP READS THE FILE THE GAME WRITES ON /outputfile, which the ingest
         * lists and reads like the inventory dump. This was the Keys row`s arm, where it was mixed
         * with the sky ladder`s answer because one row drew both; it is its own screen now and
         * answers for itself alone. */

        /* GROUP. The LFG board lives in settings.extra["lfg_board"], owned by the LFG lane; the
         * rail settles when it has entries and stays idle when it is empty or absent. Spawn timers used
         * to read the kill events here; it has no screen, so it reads nothing (see its arm). */
        /* ACHIEVEMENTS: A CORRECT SQUARE THAT NOTHING CURRENTLY DRAWS.
         *
         * `rail_plan` is the only production caller of [`square`] and it walks [`NAV`]. This
         * screen is not a row of `NAV`: it is a SECTION of PLANE OF SKY. So this arm, and the two
         * `Sources` fields behind it, are computed every frame for a surface that never asks.
         *
         * IT IS KEPT RATHER THAN DELETED, and that is a judgement rather than an oversight. The
         * arm is not WRONG, it is UNDRAWN: `the_achievements_dump_and_spawn_timers_and_videos`
         * pins four states of it against real source lists and each one is the right answer. What
         * is missing is a rail that asks section rows for squares, which is a feature and not a
         * repair. Deleting a correct, measured answer to satisfy a reachability rule would cost
         * more than the frame it takes to compute.
         *
         * WHAT IS NOT ALLOWED IS FOR THIS TO BE QUIET. It is named in `NO_SQUARE` inside
         * `every_fact_changes_some_row_or_it_is_computed_for_nobody`, which asserts that these
         * two facts move NO rail square: the day the rail starts asking, that guard goes red and
         * this comment comes out. It was found because that sweep stopped being vacuous. */
        ScreenId::Achievements => from_log(f.src.achievements, f.src.achievements_fault),

        ScreenId::Lfg => {
            if f.lfg_entries {
                State::Settled
            } else {
                State::Idle
            }
        }
        /* Spawn timers has no screen ([`UNBUILT`]), so it has nothing to settle on and says so.
         *
         * IT USED TO ANSWER A QUESTION ABOUT THE LOG INSTEAD, and got a true answer to the wrong
         * question. The arm was `You` without a log folder, `Settled` once the ingest held any
         * kill, `Idle` otherwise, which reads as "this row has the data it draws from" on a row
         * that opens onto "Not built in this release." Worse, the page it opens ends with "The
         * rail draws this row as a hollow ring for the same reason", so on any machine that had
         * ever logged a kill the screen sent the reader to look at a hollow ring that was a filled
         * square. The kills are real and the row was right about them; they are just not this
         * row's, and MY LEGEND / Hunt Journal is where they show.
         *
         * A timer needs a respawn interval to count against and the snapshot carries none, so no
         * amount of log makes this row have anything. Idle unconditionally, like every other row
         * with no screen behind it.
         *
         * IT TOOK `Facts::kills` WITH IT. That field was this arm's only reader, so once the arm
         * stopped asking, `facts` was walking the ingest's kill list once a frame to fill a bool
         * nothing consumed. Neither reachability floor could say so: the bin target's dead code
         * lint is blind to a field on a struct behind `PartialEq`, and `reach.rs`'s field floor is
         * a TEXT floor, so the `state.kills` and `session.kills` that ingest.rs really does read
         * vouched for a `Facts::kills` nobody read. That is the name collision blind spot
         * `reach.rs` documents, caught here by hand. */
        ScreenId::SpawnTimers => State::Idle,

        /* STOIC. Watch follows the Twitch channel. */
        /* CHAT FOLLOWS NOTHING YET AND SAYS SO. Idle is the honest answer while no reader is
         * connected: not You, because there is nothing a person can do about it, and not Wrong,
         * because nothing failed. The day the reader lands, this arm reads the connection.
         *
         * IT IS NOT IN THE HOLLOW GROUP ANY MORE, and that group is the point: those rows have no
         * screen behind them at all. This one has a screen that draws real words about what is and
         * is not possible on each platform. Leaving it there told the reader the page did not
         * exist while the page was on their monitor. */
        ScreenId::Chat => State::Idle,

        ScreenId::Watch => from_channel(f.twitch_known, f.twitch_error),
        /* Videos follows the YouTube channel the same way Watch follows Twitch. This arm used to
         * carry a third answer, a flat Idle for "no handle configured", because the handle was a
         * settings field that started empty. It is a constant now, so that arm could not be
         * reached and went with the field. */
        ScreenId::Videos => from_channel(f.youtube_known, f.youtube_error),
    }
}

/* ----------------------------------------------------------------- the unbuilt -- */

/// The rows this release does not draw a screen for, and the words each one shows instead.
///
/// ONE LIST, BECAUSE THE RAIL AND THE BODY WERE TWO AND HAD ALREADY COME APART. The App's
/// `unbuilt` painter held these reasons inline while `square` above decided the row's colour here,
/// and the painter's own comment claimed the two "cannot disagree" with nothing holding them
/// together. They disagreed. Standing's words said it was "read from a thread of offers and
/// deliveries" and that this build "has no thread, so there is nothing to stand in"; `square` said
/// in the same breath that a rung is read off a TRADING PARTNER and that `grimoire_core::Regard`
/// is a type the engine takes rather than a record this app keeps. A rung read off a hand does not
/// need a thread, so one of the two was invented, and it was the one on screen. The reasons live
/// here now, next to the square that has to agree with them, and a test holds the list to the
/// App's routing rather than trusting a comment to.
///
/// WHAT BELONGS IN IT. A row whose SCREEN this release does not draw at all. Not a row that is
/// merely empty today and would fill on its own once a file appears: those have screens, and the
/// screens name the file and where it goes. Every entry here names what is MISSING and where it
/// would come from, because "not built" on its own tells a reader nothing they could act on.
///
/// WHY THESE ARE ROWS AT ALL, given the rule against drawing a control for a feature that does not
/// exist. A rail row is a PLACE in the product, and a place that is not built yet is still where
/// the thing will be; opening it lands on a page that names the absence and its source, which is
/// what the house rule asks an empty state to do. The alert bell `persona.rs` cut is the other case
/// and the distinction is destination: the bell drew "absent" forever with nowhere to go and
/// nothing to read, so it taught the reader nothing and was ornament. These teach.
///
/// STANDING IS THE ONE ROW WHOSE WORDS TOUCH A DRAWN ELEMENT, so it carries a third line. The
/// persona footer now presses a regard seal that prints YOUR standing, while this screen is about
/// the standing of a TRADING PARTNER. Those are reconcilable and they were not reconciled on
/// screen, which is the exact shape of the defect the paragraph above records. The third line
/// reconciles them in the only place a reader can see it.
pub const UNBUILT: &[(ScreenId, &[&str])] = &[
    /* THE OWNER'S RAIL, THE PART OF IT THAT IS NOT WRITTEN YET. Each says what the row would BE
     * and what it is waiting on, because "not built" on its own tells a reader nothing they
     * could not see. */
    (
        ScreenId::Gina,
        &[
            "GINA is the audio and overlay trigger tool EverQuest players run beside the game: it watches the log for lines you describe and answers with a sound, a timer or a banner.",
            "This app already tails that same log (see CHRONICLE / Log Parser), so the reading half exists. What does not exist is a trigger store, a matcher, or any way to make a noise.",
        ],
    ),
    (
        ScreenId::CharacterSheet,
        &[
            "One page for who your character is: level, class, resists, and what is worn.",
            "The pieces are in the app and scattered across rows that each answer part of it. Gathering them is a page, not a parser, and it is not written.",
        ],
    ),
    (
        ScreenId::Loadouts,
        &["Named sets of gear you can swap between. Nothing in this build stores a set."],
    ),
    (
        ScreenId::Theorycraft,
        &[
            "Trying a change before you make it: swap an item, move an augment, and see what it does.",
            "The engine that would answer is in-process already (`grimoire_wasm::call`, the same one CRAFT prices with). What is missing is the question: nothing here builds a character to ask it about.",
        ],
    ),
    (
        ScreenId::Compendium,
        &[
            "One place to look a thing up, whatever kind of thing it is.",
            "The records are loaded and several rows already read them one kind at a time. A compendium is the search across all of them, and the finder in the context header is as far as this build takes that.",
        ],
    ),
    (
        ScreenId::Guild,
        &["Your guild, its roster and its events. This build knows of no guild."],
    ),
    /* CRAFT. Commission is wired; the ledger around it is not. */
    (
        ScreenId::WorkOrders,
        &[
            "Nothing in this build reads or writes work orders.",
            "THE BAZAAR / Commission prices one order at a time through the in-process engine; keeping a list of them needs a store this build does not have.",
        ],
    ),
    (
        ScreenId::Workshop,
        &["Nothing in this build reads or writes a workshop."],
    ),
    (
        ScreenId::Standing,
        &[
            "Standing is the rung a trading partner is held at on the faction ladder, Dubiously through Ally, and it gates commissions: order::Terms carries least_regard and the engine weighs it when it answers MayCommission.",
            "This build keeps no trading partners. grimoire_core::Regard is a type the engine takes as an argument, not a record this app stores, so there is no hand here to hold at a rung.",
            "Your own standing is a different question and the rail already answers it. grimoire_core::Standing::UNPROVEN is a defined value, 3.0 on no ratings, so the seal in the persona footer prints it with a dashed ring and a hover saying nobody has rated your work. That is you, not a hand you trade with, and this screen still has no hand.",
            /* THE WIKI'S FACTIONS ARE NOT THE FILLING FOR THIS ROW, and this is where the reader
             * is told so. factions.json was wired into the app on 2026-09-03 and the obvious next
             * thought is that this screen has been waiting for it: seven rungs, faction names, the
             * same words. It has not, for a reason that is measured rather than argued, and the
             * measurement is `data::factions::NO_STANDINGS` with the test named beside it. Saying
             * it here rather than only in a comment is the point: the person looking at this page
             * is exactly the person about to have the idea. */
            crate::data::factions::NO_STANDINGS,
            "So the wiki's faction pages went where the data supports: COMPENDIUM / Continents & Zones says which factions a zone raises and lowers, and COMPENDIUM / Quests says which ones a hand-in moves.",
        ],
    ),
    /* CHARACTER. */
    (
        ScreenId::Trio,
        &[
            "No trio planner is wired.",
            "The trio and level you play are set on MY LEGEND / Gear, where they drive the gear score and the valet, and are kept in settings.",
        ],
    ),
    (
        ScreenId::Aa,
        &["The snapshot has no AA records, and nothing in this build reads them from the game."],
    ),
    (
        ScreenId::Levelling,
        &["The snapshot has no levelling guide, and nothing in this build reads one."],
    ),
    /* MY LEGEND. */
    (
        ScreenId::Collections,
        &[
            "The sets you are working through and the piece each one still wants.",
            "Nothing in this build records a collection and the snapshot carries none to record against: it holds items, zones, quests, spells and who drops what, and no notion of a set of them that belongs together.",
        ],
    ),
    (
        ScreenId::Archive,
        &[
            "Characters you have put down and the state they were in when you did.",
            "This build keeps one character's present and no history of it. There is nothing yet to archive, and archiving is a store rather than a page.",
        ],
    ),
    /* COMPENDIUM. Two of these have their data already and want only the page. */
    (
        ScreenId::Bestiary,
        &[
            "Every creature the game has, what it drops and where it lives.",
            "The records are loaded. Every zone in the roster carries its mobs, each with a level, a named flag and the items it drops (data::zones::ZoneMob), and several rows already read them one zone at a time.",
            "A bestiary is that list turned inside out, creature first and zone second. Nothing turns it, so this is a page that is missing rather than data that is.",
        ],
    ),
    (
        ScreenId::LootTables,
        &[
            "What one creature drops, rather than what one item drops from.",
            "Both directions of that relation are in the snapshot already: data::drops indexes it item first, which COMPENDIUM / Items reads to answer who drops this, and data::zones::ZoneMob::drops indexes the same thing mob first.",
            "Nothing reads the mob-first side. That is the whole of what is missing.",
        ],
    ),
    (
        ScreenId::Crafting,
        &[
            "Recipes, what they take, and what a crafted thing is worth making.",
            "The engine that would answer the last of those is in-process already: grimoire_wasm::call is what THE BAZAAR / Commission prices an order with. What is missing is the recipe list to price against, and the snapshot carries none.",
        ],
    ),
    /* THE RAID ROWS, now spread across MY LEGEND, COMPENDIUM and THE TAVERN. */
    (
        ScreenId::RaidTargets,
        &[
            "The bosses worth taking a raid to, what they drop, and whether you are ready for one.",
            "The roster marks a mob as named (data::zones::ZoneMob::named) and that is the only standing this build knows a creature by. Nothing in the snapshot says which of them is a raid target, so the list cannot be derived from what is here and none is written by hand.",
        ],
    ),
    (
        ScreenId::Schedule,
        &[
            "When the guild is raiding and who said they would be there.",
            "This build knows of no guild (see THE TAVERN / Guild) and keeps no calendar. Both are stores it does not have.",
        ],
    ),
    (
        ScreenId::RaidHistory,
        &[
            "Every raid you have been on, what it killed and what dropped.",
            "The log holds the kills and the loot and the ingest reads both; MY LEGEND / Hunt Journal and Loot Journal count them tonight.",
            "What is missing is the raid itself. Nothing here gathers a night's log into an event with a roster on it, and without that there is no row to keep.",
        ],
    ),
    /* REPORTS, DASHBOARDS AND LOGS HAVE LEFT THIS LIST. They are written, they are routed, and a
     * row on the unbuilt page is a promise that there is nothing there to see; keeping their words
     * here after the screens landed would make the app apologise for work it has done. The guard
     * `main::the_unbuilt_route_and_the_unbuilt_list_hold_the_same_rows` is what forces the two to
     * move together, and it went red on the routing edit before this one was made.
     *
     * WHAT THE HEADER USED TO SAY, and why the substance is worth keeping: all three were blocked
     * on `Fight` being a scalar aggregate that keeps a first stamp, a last stamp and totals, with
     * no per event retention of any kind. That is what the `series`, `events`, `by_name`,
     * `by_target` and `by_school` fields on `Participant` were added for. Fights is the section
     * that has not been written, and it is not in this list either: it has no screen at all. */
    /* THE BAZAAR'S STOCK. Two rooms, one blocker, and it is the same blocker the Bazaar row
     * itself used to carry before it became a door onto six screens rather than one page. */
    (
        ScreenId::TradeGoods,
        &[
            "What crafters have made and are selling, and what they are asking for it.",
            "A price is the one kind of record this app cannot read out of a file on your machine: it is a live market between players, and there is no source for one here. The engine can price a COMMISSION, which is a recipe and its materials, and that is a different question with a different answer (see Commission, which runs).",
        ],
    ),
    (
        ScreenId::DroppedItems,
        &[
            "What came off a mob and is being sold on.",
            "Blocked on the same missing price as the trade goods beside it. What the app DOES know is where a thing drops from, which is COMPENDIUM / Loot Tables, and how often you have seen it drop, which is MY LEGEND / Loot Journal. Neither of those is what somebody will take for it.",
        ],
    ),
    /* MY LEGEND, AND THE FORGE. */
    (
        ScreenId::Lockouts,
        &[
            "What your character is locked out of, and for how long.",
            "A lockout is a restriction on YOU, which is why it is here and not with the world's spawn timers. Nothing in this build reads one: the game writes no lockout line to the log, so the record would have to be kept by hand and there is no store for it.",
        ],
    ),
    (
        ScreenId::FarmPlan,
        &[
            "What you are farming, from what, and how far along you are.",
            "Two of the three parts are here already: the drops corpus says which mob in which zone drops a thing, and the ingest counts what you have actually seen (MY LEGEND / Loot Journal). What is missing is the plan, which is a target you set and a store to keep it in.",
        ],
    ),
    /* GROUP. */
    (
        ScreenId::SpawnTimers,
        &[
            "The kill events a timer would start from are in the ingest now (MY LEGEND / Hunt Journal shows them).",
            "The snapshot carries no respawn intervals to time them against, so nothing is timed.",
        ],
    ),
];

/// The words the unbuilt page shows for a row, or None when this release draws that row's screen.
///
/// The App paints whatever this returns and says "routed here by mistake" when it returns None, so
/// a row can only reach that page with a reason attached.
pub fn unbuilt_why(id: ScreenId) -> Option<&'static [&'static str]> {
    UNBUILT
        .iter()
        .find(|(row, _)| *row == id)
        .map(|(_, why)| *why)
}

/* ------------------------------------------------ THE COUNT BADGE, GONE, AND THE COUNTS WITH IT --
 *
 * `COUNTED`, `count_of` and `some_if_counted` stood here and answered a number for five rows:
 * Items 6891, Zones 122, Drops 1637, Quests 924, Spells 2001. `chrome::nav_row` painted each one
 * as a gilt pill. All of it is deleted, and the reasoning is kept because the machinery was good
 * and the answer it gave was not.
 *
 * THEY WERE NOT COUNTS, THEY WERE THE CORPUS. Every one of those five is the length of a JSON file
 * that ships beside the binary, so the number is the same on every launch of this build, on every
 * machine, forever. A badge is a claim about what stands behind a row RIGHT NOW, and a constant
 * cannot make that claim: it never moves, so it never says look here, and it spends the rail's
 * only spare column competing with the label for the reader's eye. The owner asked for them out
 * twice, naming those exact five numbers, and he is describing this.
 *
 * THE ORIGINAL AGREES, WHICH IS WHY THIS IS NOT A DEPARTURE FROM THE PORT. `.cnt` in
 * `web/app.html` is on In flight and Work orders: things IN FLIGHT, a queue with a length that
 * changes while you watch it. It is a pending count. Putting the same pill on a shipped catalogue
 * borrowed the form and dropped the meaning.
 *
 * WHY THE WHOLE MECHANISM WENT AND NOT JUST THE LIST. Emptying `COUNTED` would have left
 * `count_of`, `some_if_counted` and the four functions in `chrome` that draw a pill with no
 * production caller at all, which is the uncalled-code defect this tree has paid for repeatedly:
 * code that compiles, is tested, and is reached by nothing. Keeping it warm for a day that has not
 * come is how that defect gets argued for every time.
 *
 * WHAT WOULD HAVE TO COME BACK, so this is a decision and not an amputation. The rail needs, in
 * order: a `count: Option<u32>` parameter on `chrome::nav_row`; the badge constants and
 * `badge_text` / `badge_w` / `badge_span` beside it (a `Some(0)` folds to nothing, an absent
 * badge reserves no width, and the label clip subtracts the span); the pill paint between the
 * label and the trailing bar's lane; a `count` field on `main::Row`; and a rule here answering
 * the number. THE RULE IS THE HARD PART AND IT IS THE ONLY PART THAT MATTERS: it must answer a
 * quantity that CHANGES, of things that are waiting. Work orders is the row that will have one
 * first, when a store lands behind it (see `UNBUILT`), and it will be the length of that store's
 * open orders. Not the length of a file. All of it is in this file's history. */

/// How many of the ingest's rows are actual source FILES.
///
/// NOT `sources().len()`. The ingest lists a row for a folder it looked in and found nothing in:
/// the Logs folder with "No eqlog_*.txt files", the game folder with "No *-Inventory.txt found".
/// On a fresh install those placeholders are the only rows there are, so `sources().len()` would
/// say 2 sources on a machine that has no sources, which is a number nobody computed. The three
/// file name rules are what tell a file from a folder, and `summarise`
/// above already leans on them for exactly this distinction; counting through the same rules is
/// what keeps every surface that says how many there are resting on one classification.
///
/// PUBLIC BECAUSE THE SETTINGS SCREEN HAD THE BUG THIS FUNCTION EXISTS TO PREVENT. Its LOG FOLDER
/// line printed `ingest.sources().len()` in the SETTLED green, so a fresh install with a valid
/// Logs folder and no game files read "3 sources found" beside a filled square while the rail's
/// badge, counted here, correctly said nothing. Two surfaces, one question, two answers, and the
/// wrong one was the reassuring one.
///
/// IT HAS ONE CALLER NOW AND IT IS THAT LINE. The rail badge went with the Sources row, and then
/// every other badge went too (see the block above), so the surface that had the bug is the only
/// surface left asking the question, and it asks it here. `pub` and not `pub(crate)` for nothing
/// but the module boundary between `nav` and `settings`.
pub fn source_files(sources: &[Source]) -> usize {
    sources
        .iter()
        .filter(|s| {
            looks_like_eq_log(&s.path)
                || looks_like_inventory_dump(&s.path)
                || looks_like_achievements_dump(&s.path)
        })
        .count()
}

/* --------------------------------------------------------------- THE SECTION DOT, GONE --
 *
 * `section_wants_you` stood here and answered "does anything under this section need a person",
 * and `chrome::SectionHead` carried its answer to the rail's headers. It is deleted, along with
 * the dot it lit, and the argument is worth keeping because the dot was a good signal.
 *
 * IT HAD EXACTLY ONE CAUSE AND THAT CAUSE HAS LEFT THE RAIL. The note here used to read: the only
 * thing in the whole rail waiting on a person is a Logs folder nobody has pointed at; `square`
 * answers that for every log fed row; lighting a dot on every section holding one lights five at
 * once (PLAY, CRAFT, CHARACTER, SKY and GROUP) for one cause with one fix, which is five dots
 * saying the same sentence and not one of them saying where to go; so the dot follows Sources, the
 * row that names the missing folder, and moves with it.
 *
 * It moved somewhere no section can follow. Sources is a section of the Settings screen now, so
 * there is no row to read the dot off, and the five-dots answer is the one this file rejected in
 * writing. A `section_wants_you` that can only ever return false is worse than none: it is a
 * signal that has quietly stopped being a signal, on a rail that has no way to say so.
 *
 * WHAT STILL SAYS IT, so this is not a warning deleted in silence. The rows themselves: every log
 * fed row is `State::You` without a folder and `chrome::nav_row` draws that as its trailing gold
 * bar, in both rail widths. The persona footer: no log read means no character, and the name line
 * says "no character yet" with a hover that names the Logs folder AND the Settings screen
 * (`persona::NO_NAME_WHY`). And the Settings screen itself, where the fix is: LOG FOLDER prints
 * the gold "no log folder set" beside the field that sets it.
 *
 * WHAT IS ACTUALLY LOST is the one thing the dot was for: a SHUT section cannot show its rows'
 * trailing bars, so a rail folded down to headings no longer carries this. That is a real loss and
 * it is written here rather than in a commit message. The honest way back is not a section dot: it
 * is the persona footer's gear, which is the door to the screen that fixes it and already carries
 * `main::settings_state`. That is a decision about what the gear means and it is the owner's to
 * make, so it is a ticket and not a change smuggled in under this one. */

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::SourceKind;
    use std::path::PathBuf;

    /* ------------------------------------------------------------- the unbuilt -- */

    /// Everything present that this build can have: a log folder with all three dumps in it, the
    /// snapshot loaded, the corpus read, entries on the board, both channels answering. A row that is still Idle against THIS is idle because it has no screen, not
    /// because the machine it was asked on was bare.
    fn everything() -> Facts {
        Facts {
            log_dir: true,
            src: Sources {
                log: true,
                inventory: true,
                achievements: true,
                log_fault: false,
                inventory_fault: false,
                achievements_fault: false,
            },
            /* A LOG WITH COMBAT IN IT, ALREADY FOLDED. "Everything present" has to include the
             * fights, or the one row that draws them would still be Idle here and the list below
             * would read that as "no screen" when it means "no data". */
            fights: true,
            scanning: false,
            data: true,
            data_failed: false,
            data_loading: false,
            corpus: Corpus::Ready,
            lfg_entries: true,
            twitch_known: true,
            twitch_error: false,
            youtube_known: true,
            youtube_error: false,
        }
    }

    /// Every fact `facts` computes must change some row's square, or it is work done for nobody.
    ///
    /// THE FLOOR THE OTHER TWO CANNOT REACH, and it exists because a field slipped through both.
    /// `Facts::kills` was read by exactly one arm of `square`, Spawn timers; when that arm was
    /// corrected to Idle the field kept being filled once a frame, by walking the ingest's kill
    /// list, and fed nothing. Rustc's dead code lint is blind to it because `Facts` derives
    /// `PartialEq`, and `reach.rs`'s field floor is a text scan that saw the `state.kills` and
    /// `session.kills` ingest.rs genuinely reads and passed a different struct's field on the
    /// strength of the name. That is the collision `reach.rs` names as its own blind spot.
    ///
    /// This asks the question behaviourally instead: flip one field, and if no screen in the whole
    /// rail answers differently, nothing downstream is reading it. `Facts` is `Copy` and `square`
    /// is pure, so the sweep is cheap and needs no disk.
    ///
    /// WHAT IT CANNOT SEE. A field read by `square` only in a branch this pair of Facts does not
    /// reach, and a field consumed somewhere other than `square`. Both would be false alarms
    /// rather than silent passes, which is the safe direction for a floor to fail in.
    #[test]
    fn every_fact_changes_some_row_or_it_is_computed_for_nobody() {
        let all: Vec<ScreenId> = NAV
            .iter()
            .flat_map(|(_, rs)| rs.iter().map(|(_, id)| *id))
            .collect();
        let differs = |a: &Facts, b: &Facts| all.iter().any(|id| square(*id, a) != square(*id, b));

        let base = Facts::default();
        let full = everything();
        /* Each entry flips ONE field away from the value `base` holds and names it. */
        let flips: Vec<(&str, Facts)> = vec![
            (
                "log_dir",
                Facts {
                    log_dir: true,
                    ..base
                },
            ),
            (
                "src.log",
                Facts {
                    log_dir: true,
                    src: Sources {
                        log: true,
                        ..base.src
                    },
                    ..base
                },
            ),
            (
                "src.log_fault",
                Facts {
                    log_dir: true,
                    src: Sources {
                        log: true,
                        log_fault: true,
                        ..base.src
                    },
                    ..base
                },
            ),
            (
                "src.inventory",
                Facts {
                    log_dir: true,
                    src: Sources {
                        inventory: true,
                        ..base.src
                    },
                    ..base
                },
            ),
            (
                "src.inventory_fault",
                Facts {
                    log_dir: true,
                    src: Sources {
                        inventory: true,
                        inventory_fault: true,
                        ..base.src
                    },
                    ..base
                },
            ),
            /* THE TWO LOG PARSER READS. Both need a folder for the same reason every `src.*`
             * entry does: without one the row is a You whatever else is true, so a flip measured
             * against a bare `base` would be answered by the folder and not by the field. */
            (
                "fights",
                Facts {
                    log_dir: true,
                    fights: true,
                    ..base
                },
            ),
            (
                "scanning",
                Facts {
                    log_dir: true,
                    scanning: true,
                    ..base
                },
            ),
            ("data", Facts { data: true, ..base }),
            (
                "data_failed",
                Facts {
                    data_failed: true,
                    ..base
                },
            ),
            (
                "data_loading",
                Facts {
                    data_loading: true,
                    ..base
                },
            ),
            (
                "corpus",
                Facts {
                    corpus: Corpus::Ready,
                    ..base
                },
            ),
            (
                "lfg_entries",
                Facts {
                    lfg_entries: true,
                    ..base
                },
            ),
            (
                "twitch_known",
                Facts {
                    twitch_known: true,
                    ..base
                },
            ),
            (
                "twitch_error",
                Facts {
                    twitch_error: true,
                    ..base
                },
            ),
            (
                "youtube",
                Facts {
                    youtube_known: true,
                    youtube_error: false,
                    ..base
                },
            ),
        ];
        /* EACH FLIP IS COMPARED AGAINST A BASELINE THAT DIFFERS ONLY IN THE FIELD IT NAMES.
         *
         * # THIS SWEEP WAS VACUOUS FOR EVERY `src.*` FIELD AND SAID SO IN ITS OWN SHAPE
         *
         * A source fact means nothing without a log folder, so every `src.*` entry sets
         * `log_dir: true` as well. Compared against a `base` whose `log_dir` is FALSE, each of
         * those comparisons was answered by the log-folder rows changing, and the field the entry
         * names was never the reason. Ten entries, all passing, none of them testing anything.
         *
         * WHAT IT LET THROUGH: `Sources::achievements` and `Sources::achievements_fault`, computed
         * once a frame off the ingest's source list, feeding one arm of `square` for a screen that
         * is not a row of `NAV` at all. `rail_plan` is the only production caller of `square` and
         * it walks `NAV`, so `ScreenId::Achievements` was never passed in and those two fields
         * were production-dead. The guard written to catch precisely that was passing.
         *
         * SO A `src.*` ENTRY IS NOW MEASURED AGAINST A FOLDER THAT EXISTS AND NOTHING ELSE. */
        let with_dir = Facts {
            log_dir: true,
            ..base
        };
        /* THE ONE FACT NO RAIL ROW READS, NAMED. `Sources::achievements` is real detection and
         * is counted by `source_files` for the Settings screen; what it does NOT do is change a
         * square, because the only screen that would draw one is a SECTION of PLANE OF SKY and
         * `rail_plan` asks `square` about `NAV` rows only. Listing it here is the difference
         * between a known exception and a guard that was passing for the wrong reason. */
        const NO_SQUARE: &[&str] = &["src.achievements", "src.achievements_fault"];
        /* THE FACTS THAT MEAN NOTHING WITHOUT A LOGS FOLDER, so a flip of one of them is measured
         * against a folder that exists. The `src.*` family was the original reason (see above);
         * `fights` and `scanning` join it because Log Parser answers You before it answers
         * anything else, exactly as the journals do. */
        let needs_dir =
            |name: &str| name.starts_with("src.") || name == "fights" || name == "scanning";
        for (name, flipped) in flips {
            let from = if needs_dir(name) { &with_dir } else { &base };
            let moved = differs(from, &flipped);
            if NO_SQUARE.contains(&name) {
                assert!(
                    !moved,
                    "{name} now changes a rail square, so it is no longer an exception and this \
                     list should lose it"
                );
                continue;
            }
            assert!(
                moved,
                "no row's square answers differently when {name} changes, so nothing reads it"
            );
        }
        /* And the two ends of the range are not the same rail, which would make the sweep vacuous
         * by making every comparison above trivially true for the wrong reason. */
        assert!(
            differs(&base, &full),
            "the fullest install reads like a bare one"
        );
    }

    #[test]
    fn every_unbuilt_row_is_a_rail_row_and_draws_the_hollow_ring() {
        let mut rows: Vec<ScreenId> = NAV
            .iter()
            .flat_map(|(_, rs)| rs.iter().map(|(_, id)| *id))
            .collect();
        /* A SECTION IS A PLACE IN THE RAIL TOO. The Tradeskill Hall's rooms are nested under it
         * rather than listed beside it, and a reader reaches their words the same way. */
        rows.extend(
            SECTIONS
                .iter()
                .flat_map(|(_, parts)| parts.iter().filter_map(|(_, inner, _)| *inner)),
        );
        let f = everything();
        for (id, why) in UNBUILT {
            assert!(
                rows.contains(id),
                "{} is on the unbuilt list and is neither a row nor a section, so nothing can \
                 reach its words",
                label(*id)
            );
            assert_state(
                square(*id, &f),
                State::Idle,
                "a row with no screen must read as nothing happening on the fullest install there is",
            );
            assert!(
                !why.is_empty(),
                "{} says 'not built' and stops, which tells a reader nothing they can act on",
                label(*id)
            );
            for line in *why {
                assert!(
                    !line.trim().is_empty(),
                    "{} has a blank reason line",
                    label(*id)
                );
            }
        }
    }

    /// The defect this is here for: the App's copy of Standing's words blamed a missing THREAD
    /// while `square` blamed a missing TRADING PARTNER, and both were on screen at once.
    #[test]
    fn standing_names_the_record_it_is_missing_and_does_not_invent_a_thread() {
        let why = unbuilt_why(ScreenId::Standing).expect("Standing is unbuilt in this release");
        let all = why.join(" ");
        assert!(
            all.contains("trading partner"),
            "a rung is held by a hand, and the words have to say whose: {all}"
        );
        assert!(
            all.contains("Regard"),
            "the reader needs the name of the thing that would fill this: {all}"
        );
        assert!(
            !all.to_lowercase().contains("thread"),
            "a rung reads with no order open, so a missing thread is not why it is absent: {all}"
        );
    }

    /// The house forbids both dashes in code, comments and UI strings alike. This file now holds
    /// every "not built in this release" reason in the app, which `main.rs` used to hold under its
    /// own guard, so the guard comes with them.
    #[test]
    fn no_dashes_anywhere_in_this_file() {
        for (id, why) in UNBUILT {
            for line in *why {
                assert!(
                    !line.contains('\u{2014}') && !line.contains('\u{2013}'),
                    "dash in {}'s reason: {line}",
                    label(*id)
                );
            }
        }
        for (head, rows) in NAV {
            assert!(
                !head.contains('\u{2014}') && !head.contains('\u{2013}'),
                "{head}"
            );
            for (row, _) in *rows {
                assert!(
                    !row.contains('\u{2014}') && !row.contains('\u{2013}'),
                    "{row}"
                );
            }
        }
        let src = include_str!("nav.rs");
        for (i, line) in src.lines().enumerate() {
            assert!(!line.contains('\u{2014}'), "em dash on line {}", i + 1);
            assert!(!line.contains('\u{2013}'), "en dash on line {}", i + 1);
        }
    }

    #[test]
    fn unbuilt_why_is_none_for_a_row_that_has_a_screen() {
        assert!(
            unbuilt_why(ScreenId::Parser).is_none(),
            "Parser is built, and a reason for it would let the App paint the unbuilt page over a real screen"
        );
        assert!(unbuilt_why(ScreenId::Commission).is_none());
        assert!(unbuilt_why(ScreenId::Spells).is_none());
    }

    /// THE REALMS AND THEIR ORDER, WHICH ARE THE OWNER'S.
    ///
    /// ENTER NORRATH holds nothing: it is the launcher's heading and a button rather than a fold.
    ///
    /// AND THERE IS NO GENERAL ANY MORE, WHICH IS THE POINT OF ASSERTING THE LIST EXACTLY.
    /// The drawer held what the design had not placed, and the standing rule was that such a row
    /// is kept rather than hidden. It emptied: five of its rows were doors onto views that gained
    /// sections of their own, and the other five found realms. A heading kept past its last row is
    /// a dead chevron, so it went with them, and the day a screen has nowhere to go
    /// `every_screen_appears_exactly_once` fails rather than the screen being quietly dropped.
    ///
    /// EXACTLY ONE SECTION MAY BE EMPTY. That is asserted rather than left to read, because an
    /// empty section draws no chevron and takes no click, and a section that lost its rows to an
    /// editing accident would look exactly like the launcher and say nothing about it.
    #[test]
    fn the_rail_is_the_launcher_and_six_realms() {
        let heads: Vec<&str> = NAV.iter().map(|(h, _)| *h).collect();
        assert_eq!(
            heads,
            [
                "ENTER NORRATH",
                "CHRONICLE",
                "MY LEGEND",
                "COMPENDIUM",
                "THE BAZAAR",
                "THE TAVERN",
                "BROKEN STOIC"
            ]
        );
        let empty: Vec<&str> = NAV
            .iter()
            .filter(|(_, rows)| rows.is_empty())
            .map(|(h, _)| *h)
            .collect();
        assert_eq!(
            empty,
            [LAUNCHER],
            "the launcher is the only heading with nothing under it; anything else here has lost \
             its rows and would draw as a dead heading"
        );
    }

    /// THE LAUNCHER NAMES A HEADING THAT IS REALLY IN THE TABLE.
    ///
    /// `LAUNCHER` is a string compared against the table, so a rename of the heading would leave
    /// it matching nothing, PLAY would quietly stop being gold, and every other test here would
    /// still pass. This is the one that would not.
    ///
    /// AND IT NAMES EXACTLY ONE, because "lit at all times" is what makes it the primary act, and
    /// two primary acts is none.
    #[test]
    fn the_launcher_is_a_heading_that_really_exists() {
        let hits: Vec<&str> = NAV
            .iter()
            .map(|(h, _)| *h)
            .filter(|h| *h == LAUNCHER)
            .collect();
        assert_eq!(
            hits,
            [LAUNCHER],
            "LAUNCHER is {LAUNCHER:?} and the table has {} heading(s) by that name",
            hits.len()
        );
        assert!(
            NAV.iter()
                .find(|(h, _)| *h == LAUNCHER)
                .is_some_and(|(_, rows)| rows.is_empty()),
            "the launcher grew rows; it is a button, and a button does not fold"
        );
    }

    /// EVERY SCREEN IS REACHABLE FROM EXACTLY ONE PLACE, and there are two kinds of place now.
    ///
    /// A screen is either a ROW of the rail or a SECTION nested inside one. Counting only rows
    /// would call the Tradeskill Hall's four rooms unreachable; counting them as both would let a
    /// screen sit in two places at once, which is the duplication this table exists to end.
    #[test]
    fn every_screen_appears_exactly_once() {
        let mut seen = vec![0usize; ScreenId::ALL.len()];
        let mut rows = 0;
        for (_, items) in NAV {
            for (_, id) in *items {
                seen[id.ordinal()] += 1;
                rows += 1;
            }
        }
        for (_, parts) in SECTIONS {
            for (_, inner, _) in *parts {
                if let Some(id) = inner {
                    seen[id.ordinal()] += 1;
                    rows += 1;
                }
            }
        }
        for (i, n) in seen.iter().enumerate() {
            assert_eq!(
                *n,
                1,
                "{:?} appears {n} times in NAV, expected once",
                ScreenId::ALL[i]
            );
        }
        assert_eq!(
            rows,
            ScreenId::ALL.len(),
            "NAV has {rows} rows for {} screens",
            ScreenId::ALL.len()
        );
    }

    #[test]
    fn all_is_a_permutation_of_the_ordinals() {
        /* ALL and `ordinal` are two lists of the same variants; if a variant is added to one and
         * not the other, this is what notices. */
        let mut ords: Vec<usize> = ScreenId::ALL.iter().map(|id| id.ordinal()).collect();
        ords.sort_unstable();
        let want: Vec<usize> = (0..ScreenId::ALL.len()).collect();
        assert_eq!(ords, want);
    }

    #[test]
    fn row_labels_are_the_d5_table() {
        let want: &[&[&str]] = &[
            /* ENTER NORRATH: the launcher, and it has no rows on purpose. */
            &[],
            &["GINA", "Log Parser"],
            &[
                "Character Sheet",
                "Inventory",
                "Gear",
                "Exaltations",
                "Loadouts",
                "Theorycraft",
                "Trio",
                "AA plans",
                "Farm plans",
                "Collections",
                "Archive",
                "Hunt Journal",
                "Loot Journal",
                "Plane of Sky",
                "Raid History",
                "Lockouts",
            ],
            &[
                "Search All",
                "Continents & Zones",
                "Bestiary",
                "Raid Targets",
                "Items",
                "Loot Tables",
                "Quests",
                "Spells & Abilities",
                "Crafting",
                "Levelling",
                "Spawn Timers",
            ],
            &[
                "Commission",
                "Work orders",
                "Workshop",
                "Standing",
                "Trade goods",
                "Dropped items",
            ],
            &["Groups", "Guild", "Schedule"],
            &["Watch Live", "Videos", "Chat"],
        ];
        for (si, (_, rows)) in NAV.iter().enumerate() {
            let got: Vec<&str> = rows.iter().map(|(l, _)| *l).collect();
            assert_eq!(got, want[si], "section {}", NAV[si].0);
        }
    }

    /// STANDING SITS WITH THE COMMISSION WORK, AND IT IS NOT A STREAMING ROW.
    ///
    /// It is a partner's rung on the faction ladder and it gates commissions, so it belongs beside
    /// the three rows that make a thing to order. It used to live in STOIC and this is the test
    /// that fails if it drifts back.
    ///
    /// THE SECTION IT NAMED IS GONE AND THE RULE SURVIVED THE FOLD. CRAFT, FIND and CHARACTER were
    /// folded into GENERAL, so "Standing is under CRAFT" is a sentence about a heading that no
    /// longer exists. What it was really asserting was an ARRANGEMENT: that the four rows of the
    /// commission work stand together and that Standing is one of them. In one long list that is
    /// adjacency, and adjacency is now the only thing carrying the old grouping, so it is worth
    /// more here than it was when a heading was doing the work.
    ///
    /// IT READS THE RUN OUT OF THE TABLE rather than asserting an index, so inserting a row above
    /// the four moves the whole test with it and only breaking them APART turns it red.
    /// STANDING STANDS WITH THE COMMISSION WORK, UNDER THE BAZAAR.
    ///
    /// THE FOUR WERE ROWS IN A GENERAL DRAWER while a Bazaar row, the same idea written down, sat
    /// unbuilt one heading away: one system in two places because neither had a home. The Bazaar is
    /// the heading now and they are its first four rows. Their ORDER is what says they belong to
    /// each other, so it is asserted rather than left to a reader, and Commission leads because it
    /// is the one of them that runs.
    ///
    /// AND STANDING IS STILL NOT A STREAMING ROW. That half is kept because it is the mistake the
    /// word invites: your own standing is the seal in the persona footer, a different thing from
    /// the rung a trading partner is held at.
    #[test]
    fn standing_stands_with_the_commission_work_under_the_bazaar() {
        assert_eq!(section_of(ScreenId::Standing), "THE BAZAAR");

        let (_, rows) = NAV
            .iter()
            .find(|(h, _)| *h == "THE BAZAAR")
            .expect("THE BAZAAR is a heading");
        let ids: Vec<ScreenId> = rows.iter().map(|(_, id)| *id).collect();
        let want = [
            ScreenId::Commission,
            ScreenId::WorkOrders,
            ScreenId::Workshop,
            ScreenId::Standing,
        ];
        assert_eq!(
            &ids[..want.len().min(ids.len())],
            &want,
            "the four rows of the commission work no longer lead the Bazaar together, and their \
             order is the only thing left saying they belong to each other"
        );
        assert!(
            ids.len() > want.len(),
            "the Bazaar sells stock as well as services; a list of exactly the commission work \
             means the stock rows were dropped"
        );

        let stoic = NAV
            .iter()
            .find(|(h, _)| *h == "BROKEN STOIC")
            .expect("BROKEN STOIC is a heading")
            .1;
        assert!(
            !stoic.iter().any(|(_, id)| *id == ScreenId::Standing),
            "Standing is not a streaming row"
        );
    }

    #[test]
    fn default_open_is_the_log_and_the_stream_oldest_first() {
        let open = default_open();
        assert_eq!(open.len(), 2);
        assert_eq!(NAV[open[0]].0, "CHRONICLE");
        assert_eq!(NAV[open[1]].0, "BROKEN STOIC");
        for i in open {
            assert!(
                !NAV[i].1.is_empty(),
                "the rail opens on {}, which has no rows to show",
                NAV[i].0
            );
        }
    }

    /// FIND, LABEL AND SECTION_OF AGREE ABOUT WHERE A SCREEN IS.
    ///
    /// THIS USED TO CHECK A CRUMB TOO, and the crumb is gone: the rail says where you are three
    /// times over now, so a mono path string on the context header was the app repeating itself.
    /// What it was really pinning survives here, which is that the four lookups cannot disagree.
    ///
    /// A SECTION THAT IS ITS OWN SCREEN IS THE CASE THAT BREAKS THEM. It is labelled by its
    /// section name, filed under the heading its DESTINATION sits in, and `find` says nothing at
    /// all about it because that answers for rows of the rail and this is not one.
    #[test]
    fn find_label_and_section_of_agree() {
        assert_eq!(find(ScreenId::KillTracker), Some((2, 11)));
        assert_eq!(label(ScreenId::KillTracker), "Hunt Journal");
        assert_eq!(section_of(ScreenId::KillTracker), "MY LEGEND");
        assert_eq!(parent_of(ScreenId::KillTracker), None);

        /* THE ACHIEVEMENTS DUMP IS THE CASE THAT BREAKS THE FOUR LOOKUPS: it is a section of
         * PLANE OF SKY rather than a row of the rail, so it is labelled by its section name,
         * filed under the heading its DESTINATION sits in, and `find` says nothing about it. */
        assert_eq!(label(ScreenId::Achievements), "Achievements");
        assert_eq!(section_of(ScreenId::Achievements), "MY LEGEND");
        assert_eq!(find(ScreenId::Achievements), None);
        assert_eq!(parent_of(ScreenId::Achievements), Some((ScreenId::Sky, 3)));

        /* And Commission is an ordinary row again, under the heading the market finally got. */
        assert_eq!(label(ScreenId::Commission), "Commission");
        assert_eq!(section_of(ScreenId::Commission), "THE BAZAAR");
        assert_eq!(parent_of(ScreenId::Commission), None);

        for id in ScreenId::ALL {
            assert!(
                find(id).is_some() || parent_of(id).is_some(),
                "{id:?} is neither a row of the rail nor a section of one"
            );
            assert!(!label(id).is_empty(), "{id:?} has no label");
            assert!(
                !section_of(id).is_empty(),
                "{id:?} is under no heading, so the rail cannot draw it"
            );
        }
    }

    /* chrome::State does not derive Debug and chrome.rs is not this lane's to edit, so the
     * assertions compare names. */
    fn name(s: State) -> &'static str {
        match s {
            State::Idle => "Idle",
            State::Working => "Working",
            State::You => "You",
            State::Wrong => "Wrong",
            State::Settled => "Settled",
        }
    }

    fn assert_state(got: State, want: State, why: &str) {
        assert_eq!(name(got), name(want), "{why}");
    }

    /* A Cx with nothing in it: no snapshot, no log folder, a watcher that has never answered. */
    fn empty_status() -> crate::watcher::Status {
        crate::watcher::Status {
            twitch: crate::watcher::Channel {
                handle: "Broken_Stoic".into(),
                live: None,
                title: None,
                viewers: None,
                game: None,
                video_id: None,
                checked_at: None,
                source: "none",
                error: None,
            },
            youtube: crate::watcher::Channel::unchecked(crate::settings::YOUTUBE_HANDLE),
        }
    }

    #[test]
    fn square_with_nothing_loaded_and_no_log_folder() {
        let mut settings = crate::settings::Settings::default();
        let mut ingest = crate::ingest::Ingest::new(&settings);
        let live = empty_status();
        let cx = Cx {
            data: None,
            railed: false,
            data_err: None,
            live: &live,
            settings: &mut settings,
            ingest: &mut ingest,
            chat: crate::chat::ChatHandle::idle(),
            chat_wanted: false,
            auth: Default::default(),
            auth_begin: false,
            auth_cancel: false,
            yt: Default::default(),
            yt_wanted: false,
            player: Default::default(),
            stage: None,
            demand: None,
            ask: crate::screens::Ask::None,
        };
        /* everything that reads the log needs a person to point at the Logs folder.
         *
         * LOG PARSER IS BACK IN THIS LIST, and it was taken out of it for a reason that expired.
         * The note here read that a Logs folder "does not make a fight computable", which was true
         * of a build with no combat engine in it: the row was pinned Idle and asserted with the
         * honest idles below. The engine landed, `Ingest::fights` holds real fights, and a folder
         * is once again the FIRST thing this row is missing. It cannot fold a fight out of a
         * folder nobody has named, and only a person can name one. */
        for id in [
            ScreenId::Parser,
            ScreenId::KillTracker,
            ScreenId::Loot,
            ScreenId::Inventory,
            ScreenId::Gear,
            ScreenId::Exalt,
            ScreenId::Achievements,
        ] {
            assert_state(square(id, &facts(&cx)), State::You, &format!("{id:?}"));
        }
        /* Spawn timers was in that list and does not belong in it: it has no screen, so there is
         * no page for a named Logs folder to fill and no reason to ask a person for one. */
        assert_state(
            square(ScreenId::SpawnTimers, &facts(&cx)),
            State::Idle,
            "a row with no screen asks nobody for anything",
        );
        /* AND NEITHER DOES GROUP, which is worth pinning here now that the section dot is gone:
         * these three read the LFG board and answer Settled or Idle, never You, so a missing Logs
         * folder is a question asked of PLAY, CHARACTER and SKY and of nobody else. This assertion
         * was carried by the deleted `one_dot_and_it_is_the_section_holding_sources`; it is about
         * the square, not about the dot, so it outlives it. */
        /* ONE ROW NOW WHERE THERE WERE THREE: Raids and Motes were the group finder`s other two
         * modes wearing rows of their own, and they are sections of this one. The rule is
         * unchanged and is about the screen, which is why it still reads. */
        assert_state(
            square(ScreenId::Lfg, &facts(&cx)),
            State::Idle,
            "Groups does not ask a person for a Logs folder",
        );
        /* nothing was attempted for the snapshot, so nothing is wrong yet. Spells joined this
         * list when it got a screen: it is idle here for the same reason the other four are, no
         * snapshot yet, and not for the reason it used to be, no such thing as a spell record. */
        for id in [
            ScreenId::Items,
            ScreenId::Zones,
            ScreenId::Quests,
            ScreenId::Spells,
            ScreenId::Sky,
        ] {
            assert_state(square(id, &facts(&cx)), State::Idle, &format!("{id:?}"));
        }
        /* the honest idles */
        for id in [
            ScreenId::WorkOrders,
            ScreenId::Workshop,
            ScreenId::Trio,
            ScreenId::Aa,
            ScreenId::Levelling,
            ScreenId::Lfg,
            ScreenId::Watch,
            ScreenId::Videos,
            ScreenId::Standing,
        ] {
            assert_state(square(id, &facts(&cx)), State::Idle, &format!("{id:?}"));
        }
        /* Commission follows the corpus read the App starts at launch: a Cx alone sees it as
         * still reading; the App overwrites with what the screen reports. */
        assert_state(
            square(ScreenId::Commission, &facts(&cx)),
            State::Working,
            "corpus.grim is being read",
        );
        assert_state(
            square(
                ScreenId::Commission,
                &Facts {
                    corpus: Corpus::Ready,
                    ..Default::default()
                },
            ),
            State::Settled,
            "the engine has its corpus",
        );
        assert_state(
            square(
                ScreenId::Commission,
                &Facts {
                    corpus: Corpus::Failed,
                    ..Default::default()
                },
            ),
            State::Wrong,
            "no corpus.grim: the screen paints a problem bar and the rail agrees",
        );
        /* Working is never answered from a Cx alone for anything else: the loader flag is the
         * App's to set */
        for id in ScreenId::ALL {
            if id != ScreenId::Commission {
                assert_ne!(
                    name(square(id, &facts(&cx))),
                    name(State::Working),
                    "{id:?}"
                );
            }
        }
    }

    #[test]
    fn square_follows_the_snapshot_and_the_watcher() {
        let mut settings = crate::settings::Settings::default();
        let mut ingest = crate::ingest::Ingest::new(&settings);
        let mut live = empty_status();
        {
            let cx = Cx {
                data: None,
                railed: false,
                data_err: Some("gear-data.json: not valid JSON"),
                live: &live,
                settings: &mut settings,
                ingest: &mut ingest,
                chat: crate::chat::ChatHandle::idle(),
                chat_wanted: false,
                auth: Default::default(),
                auth_begin: false,
                auth_cancel: false,
                yt: Default::default(),
                yt_wanted: false,
                player: Default::default(),
                stage: None,
                demand: None,
                ask: crate::screens::Ask::None,
            };
            assert_state(
                square(ScreenId::Items, &facts(&cx)),
                State::Wrong,
                "a load that failed is a fault, not a gap",
            );
            assert_state(
                square(ScreenId::Sky, &facts(&cx)),
                State::Wrong,
                "sky.json is part of the snapshot",
            );
        }
        live.twitch.live = Some(false);
        {
            let cx = Cx {
                data: None,
                railed: false,
                data_err: None,
                live: &live,
                settings: &mut settings,
                ingest: &mut ingest,
                chat: crate::chat::ChatHandle::idle(),
                chat_wanted: false,
                auth: Default::default(),
                auth_begin: false,
                auth_cancel: false,
                yt: Default::default(),
                yt_wanted: false,
                player: Default::default(),
                stage: None,
                demand: None,
                ask: crate::screens::Ask::None,
            };
            assert_state(
                square(ScreenId::Watch, &facts(&cx)),
                State::Settled,
                "a known offline is a fact",
            );
        }
        live.twitch.live = None;
        live.twitch.error = Some("gql: timed out".into());
        {
            let cx = Cx {
                data: None,
                railed: false,
                data_err: None,
                live: &live,
                settings: &mut settings,
                ingest: &mut ingest,
                chat: crate::chat::ChatHandle::idle(),
                chat_wanted: false,
                auth: Default::default(),
                auth_begin: false,
                auth_cancel: false,
                yt: Default::default(),
                yt_wanted: false,
                player: Default::default(),
                stage: None,
                demand: None,
                ask: crate::screens::Ask::None,
            };
            assert_state(
                square(ScreenId::Watch, &facts(&cx)),
                State::Wrong,
                "never known and the last poll failed",
            );
        }
    }

    #[test]
    fn square_follows_the_log_folder_and_the_lfg_board() {
        let mut settings = crate::settings::Settings {
            log_dir: Some(std::env::temp_dir()),
            ..Default::default()
        };
        settings
            .extra
            .insert("lfg_board".into(), serde_json::json!([{"who": "Stoic"}]));
        let mut ingest = crate::ingest::Ingest::new(&settings);
        let live = empty_status();
        let cx = Cx {
            data: None,
            railed: false,
            data_err: None,
            live: &live,
            settings: &mut settings,
            ingest: &mut ingest,
            chat: crate::chat::ChatHandle::idle(),
            chat_wanted: false,
            auth: Default::default(),
            auth_begin: false,
            auth_cancel: false,
            yt: Default::default(),
            yt_wanted: false,
            player: Default::default(),
            stage: None,
            demand: None,
            ask: crate::screens::Ask::None,
        };
        /* a folder is set, so nobody is being asked for anything; whether a log was found is the
         * ingest's answer, and this test cannot plant one, so the only certainty is "not You" */
        assert_ne!(
            name(square(ScreenId::KillTracker, &facts(&cx))),
            name(State::You)
        );
        assert_state(
            square(ScreenId::Lfg, &facts(&cx)),
            State::Settled,
            "the board has an entry",
        );
        /* Motes was a second row reading the same board and is a SECTION of Groups now, so there
         * is one square to assert rather than two saying the same thing. */
        let f = facts(&cx);
        assert!(f.log_dir);
        assert!(f.lfg_entries);
        assert!(!f.data && !f.data_failed);
    }

    /* ---- the pure square, with planted facts ---- */

    fn src(kind: SourceKind, path: &str, problem: Option<&str>) -> Source {
        Source {
            kind,
            path: PathBuf::from(path),
            last_read: None,
            records: 0,
            problem: problem.map(str::to_owned),
        }
    }

    #[test]
    fn placeholders_are_gaps_not_faults() {
        /* What the ingest lists on a fresh install with a Logs folder set and the game not yet
         * logging: the folder itself, and the game folder, each with a note in `problem`. */
        let listed = vec![
            src(SourceKind::Log, "C:/eq/Logs", Some("No eqlog_*.txt files in C:/eq/Logs. Is logging on? (/log)")),
            src(SourceKind::Inventory, "C:/eq", Some("No *-Inventory.txt found beside the Logs folder. In game: /outputfile inventory")),
        ];
        let s = summarise(&listed);
        assert_eq!(
            s,
            Sources::default(),
            "a folder with nothing in it is neither a source nor a fault"
        );
        let f = Facts {
            log_dir: true,
            src: s,
            ..Default::default()
        };
        assert_state(
            square(ScreenId::KillTracker, &f),
            State::Idle,
            "nothing to read yet",
        );
        assert_state(square(ScreenId::Inventory, &f), State::Idle, "no dump yet");
    }

    #[test]
    fn not_tailed_note_is_not_a_fault() {
        /* Two characters, two logs. The ingest tails the newest and annotates the other. */
        let listed = vec![
            src(SourceKind::Log, "C:/eq/Logs/eqlog_Stoic_legends.txt", None),
            src(SourceKind::Log, "C:/eq/Logs/eqlog_Alt_legends.txt", Some("not tailed: only the most recently written log is read, and that is eqlog_Stoic_legends.txt")),
        ];
        let s = summarise(&listed);
        assert!(s.log);
        assert!(
            !s.log_fault,
            "the note on the other log is information, not a failure"
        );
        let f = Facts {
            log_dir: true,
            src: s,
            ..Default::default()
        };
        assert_state(
            square(ScreenId::KillTracker, &f),
            State::Settled,
            "the newest log is being read",
        );
        assert_state(
            square(ScreenId::KillTracker, &f),
            State::Settled,
            "same log",
        );
        /* and the coupling this rests on: the ingest's note begins with the prefix */
        assert!(listed[1]
            .problem
            .as_deref()
            .unwrap()
            .starts_with(NOT_TAILED_PREFIX));
    }

    #[test]
    fn a_found_file_that_will_not_read_is_wrong() {
        let listed = vec![src(
            SourceKind::Log,
            "C:/eq/Logs/eqlog_Stoic_legends.txt",
            Some("C:/eq/Logs/eqlog_Stoic_legends.txt: permission denied"),
        )];
        let s = summarise(&listed);
        assert!(s.log && s.log_fault);
        let f = Facts {
            log_dir: true,
            src: s,
            ..Default::default()
        };
        for id in [ScreenId::KillTracker, ScreenId::Loot] {
            assert_state(square(id, &f), State::Wrong, &format!("{id:?}"));
        }
        /* a dump fault reddens the dump rows, not the log rows */
        let listed = vec![
            src(SourceKind::Log, "C:/eq/Logs/eqlog_Stoic_legends.txt", None),
            src(
                SourceKind::Inventory,
                "C:/eq/Stoic_legends-Inventory.txt",
                Some("C:/eq/Stoic_legends-Inventory.txt: unexpected end of file"),
            ),
        ];
        let f = Facts {
            log_dir: true,
            src: summarise(&listed),
            ..Default::default()
        };
        assert_state(
            square(ScreenId::KillTracker, &f),
            State::Settled,
            "the log is fine",
        );
        assert_state(
            square(ScreenId::Inventory, &f),
            State::Wrong,
            "the dump is not",
        );
        assert_state(
            square(ScreenId::Exalt, &f),
            State::Wrong,
            "reads the same dump",
        );
    }

    #[test]
    fn gear_needs_the_dump_and_the_snapshot() {
        let listed = vec![src(
            SourceKind::Inventory,
            "C:/eq/Stoic_legends-Inventory.txt",
            None,
        )];
        let dump_only = Facts {
            log_dir: true,
            src: summarise(&listed),
            ..Default::default()
        };
        assert_state(
            square(ScreenId::Inventory, &dump_only),
            State::Settled,
            "the dump alone settles Inventory",
        );
        assert_state(
            square(ScreenId::Gear, &dump_only),
            State::Idle,
            "gear score also needs gear-data item stats",
        );
        let both = Facts {
            data: true,
            ..dump_only
        };
        assert_state(
            square(ScreenId::Gear, &both),
            State::Settled,
            "dump and snapshot",
        );
        let broken = Facts {
            data_failed: true,
            ..dump_only
        };
        assert_state(
            square(ScreenId::Gear, &broken),
            State::Wrong,
            "the snapshot failed to load",
        );
        let no_dump = Facts {
            log_dir: true,
            data: true,
            ..Default::default()
        };
        assert_state(
            square(ScreenId::Gear, &no_dump),
            State::Idle,
            "snapshot alone is not a character",
        );
    }

    /// THE ACHIEVEMENTS DUMP ANSWERS FOR ITSELF ALONE NOW.
    ///
    /// This was the `Keys` row, which drew two things at once: the island ladder from the snapshot
    /// on its first tab and the achievements dump on its second, so its square was the OR of two
    /// unrelated sources and could settle while half of it was missing. The ladder is a section of
    /// PLANE OF SKY and answers with the rest of that screen; this reads the dump and nothing else.
    #[test]
    fn the_achievements_dump_and_spawn_timers_and_videos() {
        let listed = vec![src(
            SourceKind::Achievements,
            "C:/eq/Stoic_legends-Achievements.txt",
            None,
        )];
        let f = Facts {
            log_dir: true,
            src: summarise(&listed),
            ..Default::default()
        };
        assert!(f.src.achievements);
        assert_state(
            square(ScreenId::Achievements, &f),
            State::Settled,
            "the dump is there and reads",
        );
        /* AND THE SNAPSHOT ALONE DOES NOT SETTLE IT, which it did while this square was the OR of
         * the ladder and the dump: a loaded sky.json turned the row green on a machine that had
         * never written an achievements file. The ladder is PLANE OF SKY's answer and is asserted
         * right beside this so the two cannot quietly become one again. */
        assert_state(
            square(
                ScreenId::Achievements,
                &Facts {
                    data: true,
                    ..Default::default()
                },
            ),
            State::You,
            "a snapshot is not an achievements dump",
        );
        assert_state(
            square(
                ScreenId::Sky,
                &Facts {
                    data: true,
                    ..Default::default()
                },
            ),
            State::Settled,
            "the ladder draws from the snapshot",
        );
        assert_state(
            square(ScreenId::Achievements, &Facts::default()),
            State::You,
            "nothing loaded: point Settings at the Logs folder",
        );
        let broken = vec![src(
            SourceKind::Achievements,
            "C:/eq/Stoic_legends-Achievements.txt",
            Some("C:/eq/Stoic_legends-Achievements.txt: permission denied"),
        )];
        let f2 = Facts {
            log_dir: true,
            src: summarise(&broken),
            ..Default::default()
        };
        assert!(f2.src.achievements_fault);
        assert_state(
            square(ScreenId::Achievements, &f2),
            State::Wrong,
            "a dump that will not read is a fault",
        );
        /* SPAWN TIMERS DOES NOT FOLLOW THE KILLS, and this pair used to assert that it did.
         * The row has no screen: it opens on "Not built in this release", whose closing line tells
         * the reader the rail draws a hollow ring for the same reason. A Settled square there made
         * that sentence false on any machine that had ever logged a kill, which is every machine
         * the app is any use on. The kills are real and they are not this row's; MY LEGEND /
         * Hunt Journal shows them. A timer also needs a respawn interval to count against, and the
         * snapshot carries none, so no amount of log gives this row anything. */
        assert_state(
            square(ScreenId::SpawnTimers, &f),
            State::Idle,
            "an achievements dump is not a respawn interval",
        );
        assert_state(
            square(ScreenId::SpawnTimers, &everything()),
            State::Idle,
            "and neither is the fullest install this build can describe",
        );
        assert_state(
            square(ScreenId::SpawnTimers, &Facts::default()),
            State::Idle,
            "a row with no screen never asks a person for a Logs folder either",
        );
        /* Videos follows the YouTube channel exactly as Watch follows Twitch. The "no handle
         * configured" answer that used to lead this block is gone: the handle is a constant. */
        let f = Facts::default();
        assert_state(
            square(ScreenId::Videos, &f),
            State::Idle,
            "nothing known yet, no failure either",
        );
        let f = Facts {
            youtube_known: true,
            ..Default::default()
        };
        assert_state(square(ScreenId::Videos, &f), State::Settled, "known");
        let f = Facts {
            youtube_error: true,
            ..Default::default()
        };
        assert_state(
            square(ScreenId::Videos, &f),
            State::Wrong,
            "never known, last poll failed",
        );
        let f = Facts {
            youtube_known: true,
            youtube_error: true,
            ..Default::default()
        };
        assert_state(
            square(ScreenId::Videos, &f),
            State::Settled,
            "a known state outranks a failed refresh",
        );
    }

    /// WORKING IS ONLY EVER SAID OF A THREAD THIS PROCESS ACTUALLY STARTED.
    ///
    /// THIS TEST WAS NAMED `working_is_answered_only_while_the_snapshot_loads` AND THE NAME WAS
    /// ALREADY WRONG WHEN IT WAS WRITTEN: the Commission row answers Working off the corpus reader
    /// and never off the snapshot, so the name described one of the two things the body checked.
    /// The ingest's bootstrap fold is the third, and it arrived with Log Parser: `Facts::scanning`
    /// is `Ingest::scanning`, a real worker with a real in-flight bit. The rule the body has always
    /// enforced is the one in the name now: no row may claim Working out of a fact that is not
    /// about a running thread. The watcher, whose `Status` carries no such bit, still never does.
    #[test]
    fn working_is_only_said_of_a_thread_that_is_running() {
        /* Every boolean fact at both ends, nothing and everything, with all three workers idle: no
         * row may claim Working, because nothing the rail can see is running. */
        let all = Facts {
            log_dir: true,
            src: Sources {
                log: true,
                log_fault: true,
                inventory: true,
                inventory_fault: true,
                achievements: true,
                achievements_fault: true,
            },
            fights: true,
            scanning: false,
            data: true,
            data_failed: true,
            data_loading: false,
            corpus: Corpus::Ready,
            lfg_entries: true,
            twitch_known: true,
            twitch_error: true,
            youtube_known: true,
            youtube_error: true,
        };
        for f in [
            Facts {
                corpus: Corpus::Ready,
                ..Default::default()
            },
            all,
        ] {
            for id in ScreenId::ALL {
                assert_ne!(name(square(id, &f)), name(State::Working), "{id:?}");
            }
        }
        /* The loader running and nothing loaded yet: exactly the rows that draw from the snapshot
         * go Working, nothing else does, and a snapshot that has loaded outranks a flag left on.
         * Keys is in the first list: its island ladder is sky.json. */
        let loading = Facts {
            data_loading: true,
            log_dir: true,
            corpus: Corpus::Ready,
            ..Default::default()
        };
        for id in [
            ScreenId::Items,
            ScreenId::Zones,
            ScreenId::Quests,
            ScreenId::Sky,
        ] {
            assert_state(square(id, &loading), State::Working, &format!("{id:?}"));
        }
        for id in [
            ScreenId::Parser,
            ScreenId::Commission,
            ScreenId::Inventory,
            ScreenId::Watch,
            ScreenId::Lfg,
        ] {
            assert_ne!(
                name(square(id, &loading)),
                name(State::Working),
                "{id:?} does not draw from the snapshot"
            );
        }
        let loaded = Facts {
            data: true,
            ..loading
        };
        assert_state(
            square(ScreenId::Items, &loaded),
            State::Settled,
            "loaded wins over a stale flag",
        );

        /* THE THIRD WORKER: the ingest's bootstrap fold, which is Log Parser's and nobody else's.
         *
         * A folder is named and the tail has not been folded yet, which is the first second of
         * every launch. The Fights page paints `NoFights::Reading` for that instant; the rail has
         * to say the same thing or the two disagree on screen. */
        let folding = Facts {
            log_dir: true,
            scanning: true,
            corpus: Corpus::Ready,
            ..Default::default()
        };
        assert_state(
            square(ScreenId::Parser, &folding),
            State::Working,
            "the bootstrap is still folding the tail and the rail called it nothing",
        );
        for id in ScreenId::ALL {
            if id == ScreenId::Parser {
                continue;
            }
            assert_ne!(
                name(square(id, &folding)),
                name(State::Working),
                "{id:?} does not wait on the ingest's fold and must not borrow its Working"
            );
        }

        /* Gear with the dump present waits on the snapshot the same way */
        let listed = vec![src(
            SourceKind::Inventory,
            "C:/eq/Stoic_legends-Inventory.txt",
            None,
        )];
        let gear = Facts {
            log_dir: true,
            data_loading: true,
            src: summarise(&listed),
            ..Default::default()
        };
        assert_state(
            square(ScreenId::Gear, &gear),
            State::Working,
            "gear needs the snapshot too",
        );
    }

    /// DEFECT: THE RAIL SAYING LOG PARSER IS IDLE WHILE IT IS READING A LIVE LOG.
    ///
    /// # WHAT WAS BELIEVED, AND WHEN IT STOPPED BEING TRUE
    ///
    /// `square`'s arm for this row was the constant `State::Idle`, and the comment above it argued
    /// the case honestly for the build it was written in: Log Parser draws FIGHTS, fights need a
    /// combat engine, the engine's grammar was a document (`docs/COMBAT-PARSER.md`) that nothing
    /// implemented, and the page itself printed a sentence saying so, which the comment named
    /// (`screens::parser::FIGHTS_EMPTY`). A hollow ring beside a page that says "not built" is the
    /// rail agreeing with the screen, which is the rule this whole function exists to keep.
    ///
    /// EVERY PREMISE OF THAT ARGUMENT IS NOW FALSE. `grimoire_parse::combat` and
    /// `grimoire_parse::fights` are in this workspace, `crate::fights::FightRow` mirrors their
    /// output, `Ingest::fights` holds it, and the Fights page draws a table of it. `FIGHTS_EMPTY`
    /// was deleted along with the sentence it carried: the page now names which of four things is
    /// missing, through `screens::parser::why_no_fights`. So the constant outlived its reason and
    /// became the defect it was written to prevent, pointed the other way: the rail claiming there
    /// is nothing here while the page under it lists four fights read off a log being tailed now.
    ///
    /// # WHY IT IS NOT SIMPLY `from_log(f.src.log, ..)`
    ///
    /// Because a log being READ is not a fight being FOUND, and the four states have to line up
    /// with the four the page prints. An hour standing in a bank tails a log and folds no combat:
    /// the two journals beside this row settle on that file because a kill list of a quiet file is
    /// still that file's kill list, and this row would be claiming a fight that is not there.
    ///
    /// WHAT MUTATION MAKES THIS RED: pinning this arm to any constant, answering it off
    /// `f.src.log` instead of `f.fights`, or dropping the Working the bootstrap fold earns.
    #[test]
    fn the_log_parser_row_follows_the_fights_it_draws_and_not_a_constant() {
        let named = Facts {
            log_dir: true,
            corpus: Corpus::Ready,
            ..Default::default()
        };
        /* NO FOLDER: only a person can name one, so this is a You exactly as it is for the two
         * journals. `square_with_nothing_loaded_and_no_log_folder` asserts it from a real `Cx`. */
        assert_state(
            square(ScreenId::Parser, &Facts::default()),
            State::You,
            "nobody has named a Logs folder and the rail asked nobody for one",
        );
        /* A FOLD THAT FINISHED WITH FIGHTS IN IT SETTLES, which is the state this row spends the
         * rest of a session in and the whole of what the pinned Idle was getting wrong. */
        assert_state(
            square(
                ScreenId::Parser,
                &Facts {
                    fights: true,
                    ..named
                },
            ),
            State::Settled,
            "fights are on the page and the rail says the row is idle",
        );
        /* A TAILED LOG WITH NO COMBAT IN IT IS IDLE AND NOT SETTLED. This is the assertion that
         * keeps the row off `src.log`, and it is the one a lazy fix would break. */
        assert_state(
            square(
                ScreenId::Parser,
                &Facts {
                    src: Sources {
                        log: true,
                        ..Sources::default()
                    },
                    ..named
                },
            ),
            State::Idle,
            "a tailed log with no combat in it settled a row that draws fights",
        );
        /* AND A LOG FILE THAT WOULD NOT OPEN IS A FAULT, the same one the journals report, and it
         * outranks fights already in hand for the same reason it does there: the list is stale. */
        assert_state(
            square(
                ScreenId::Parser,
                &Facts {
                    fights: true,
                    src: Sources {
                        log: true,
                        log_fault: true,
                        ..Sources::default()
                    },
                    ..named
                },
            ),
            State::Wrong,
            "a log that would not open is a fault this row must show",
        );
    }

    #[test]
    fn eq_log_name_rule_matches_what_the_client_writes() {
        let p = |s: &str| PathBuf::from(s);
        assert!(looks_like_eq_log(&p("C:/logs/eqlog_Stoic_legends.txt")));
        assert!(
            looks_like_eq_log(&p("EQLOG_Stoic_legends.TXT")),
            "the name rule is case insensitive"
        );
        assert!(
            !looks_like_eq_log(&p("eqlog_.txt")),
            "a log name carries at least one char between the prefix and the extension"
        );
        assert!(!looks_like_eq_log(&p("dbg.txt")));
        assert!(!looks_like_eq_log(&p("eqlog_Stoic_legends.txt.bak")));
        assert!(!looks_like_eq_log(&p("Stoic_legends-Inventory.txt")));
        assert!(
            !looks_like_eq_log(&p("C:/eq/Logs")),
            "a folder is not a log"
        );
    }

    #[test]
    fn dump_name_rules_match_outputfile_names() {
        let p = |s: &str| PathBuf::from(s);
        assert!(looks_like_inventory_dump(&p("Stoic_legends-Inventory.txt")));
        assert!(looks_like_inventory_dump(&p("stoic_legends-INVENTORY.TXT")));
        assert!(!looks_like_inventory_dump(&p(
            "Stoic_legends-Achievements.txt"
        )));
        assert!(
            !looks_like_inventory_dump(&p("C:/eq")),
            "a folder is not a dump"
        );
        assert!(looks_like_achievements_dump(&p(
            "Stoic_legends-Achievements.txt"
        )));
        assert!(!looks_like_achievements_dump(&p(
            "Stoic_legends-Inventory.txt"
        )));
        assert!(!looks_like_achievements_dump(&p("eqlog_Stoic_legends.txt")));
    }

    /* ---- what the ingest found, which is a count of FILES and not of rows ---- */

    #[test]
    fn a_folder_the_ingest_looked_in_is_not_a_source_it_found() {
        /* The fresh install rows: two placeholders, no files. A 2 here would count the looking,
         * not the finding, and the Settings screen's LOG FOLDER line prints this number. */
        let placeholders = vec![
            src(SourceKind::Log, "C:/eq/Logs", Some("No eqlog_*.txt files in C:/eq/Logs. Is logging on? (/log)")),
            src(SourceKind::Inventory, "C:/eq", Some("No *-Inventory.txt found beside the Logs folder. In game: /outputfile inventory")),
        ];
        assert_eq!(source_files(&placeholders), 0);
        /* Real files count, including the one the ingest lists but does not tail: it was found, and
         * the SOURCES ledger prints a row for it. */
        let found = vec![
            src(SourceKind::Log, "C:/eq/Logs/eqlog_Stoic_legends.txt", None),
            src(SourceKind::Log, "C:/eq/Logs/eqlog_Alt_legends.txt", Some("not tailed: only the most recently written log is read, and that is eqlog_Stoic_legends.txt")),
            src(SourceKind::Inventory, "C:/eq/Stoic_legends-Inventory.txt", None),
            src(SourceKind::Achievements, "C:/eq/Stoic_legends-Achievements.txt", None),
            src(SourceKind::Log, "C:/eq/Logs", Some("No eqlog_*.txt files")),
        ];
        assert_eq!(source_files(&found), 4, "four files and one folder");
        /* A file that will not read is still a source that was found: the square goes red, the
         * count does not shrink. */
        let faulty = vec![src(
            SourceKind::Log,
            "C:/eq/Logs/eqlog_Stoic_legends.txt",
            Some("permission denied"),
        )];
        assert_eq!(source_files(&faulty), 1);
    }
    /// A SECTION LIST IS ONLY EVER FOR A DESTINATION THAT HAS ONE VIEW TOO MANY.
    ///
    /// Three rules, and each is a different way the table goes wrong. A list of one is a rail that
    /// costs 132 points to say the destination's own name back. A list for a screen that is not in
    /// the rail is a section of nothing. A duplicate entry is two answers to one question, and
    /// `sections_of` would silently return the first.
    #[test]
    fn every_section_list_belongs_to_a_real_destination_and_has_something_to_choose() {
        let mut seen: Vec<ScreenId> = Vec::new();
        for (id, rows) in SECTIONS {
            assert!(
                find(*id).is_some(),
                "{id:?} has sections but is not a row in the rail"
            );
            assert!(
                rows.len() > 1,
                "{id:?} lists {} section(s); a rail of one row is a caption",
                rows.len()
            );
            assert!(
                !seen.contains(id),
                "{id:?} appears twice in SECTIONS, so one of the two lists can never be reached"
            );
            seen.push(*id);
            for (name, _, _) in *rows {
                assert!(!name.trim().is_empty(), "{id:?} has an unnamed section");
            }
        }
        assert!(
            !SECTIONS.is_empty(),
            "no sections at all, so this proved nothing"
        );
    }
    /// A DESTINATION OPENS ON A SECTION IT CAN DRAW.
    ///
    /// Log Parser was the case that produced this rule: its sections are Live, Fights, Reports,
    /// Dashboards and Logs, and for a long time only Fights had a screen, so a flat zero opened the
    /// combat destination on a page saying nothing was built, on the first click, every time.
    ///
    /// THE SPECIFIC ASSERTION ABOUT LOG PARSER IS GONE AND THAT IS THE POINT OF THE RULE. It read
    /// `default_section(Parser) == 1`, "not Live, which has no screen". Live HAS a screen now
    /// (`screens::live`, built on `Ingest::current_fight`), so pinning the answer to Fights would
    /// keep sending a person past a built page to reach a built page. The property was never about
    /// which index Log Parser lands on; it is that a destination lands somewhere it can DRAW, and
    /// the sweep below says exactly that for every row without having to be retaught each time one
    /// of them gets built.
    #[test]
    fn a_destination_opens_on_a_section_it_can_draw() {
        let mut checked = 0usize;
        for (owner, rows) in SECTIONS {
            let on = default_section(*owner);
            assert!(
                on < rows.len(),
                "{owner:?} opens on section {on} of {}",
                rows.len()
            );
            let drawable = |i: usize| match rows[i].1 {
                None => true,
                Some(inner) => unbuilt_why(inner).is_none(),
            };
            /* EVERY DESTINATION WITH A DRAWABLE SECTION OPENS ON ONE, and on the FIRST one, so the
             * order the design gives its sections is still what decides. */
            if (0..rows.len()).any(drawable) {
                assert!(
                    drawable(on),
                    "{owner:?} opens on {} which cannot be drawn",
                    rows[on].0
                );
                assert!(
                    !(0..on).any(drawable),
                    "{owner:?} skipped a drawable section to open on {}",
                    rows[on].0
                );
                checked += 1;
            }
        }
        assert!(
            checked > 0,
            "no destination has a drawable section, so this proved nothing"
        );
    }

    /// DEFECT: AN UNBUILT PAGE SENDING THE READER TO A ROOM THAT DOES NOT EXIST.
    ///
    /// # THREE OF THEM SHIPPED
    ///
    /// Work orders printed "THE TAVERN / Tradeskill Hall / Commission prices one order at a time",
    /// Crafting printed "grimoire_wasm::call is what THE TAVERN / Tradeskill Hall prices an order
    /// with", and Trio printed "The trio and level you play are set on GENERAL / Gear". A reader
    /// following any of the three opens THE TAVERN and finds Groups, Guild and Schedule; there is
    /// no Tradeskill Hall row anywhere in [`NAV`], and no GENERAL heading either. Commission is
    /// under THE BAZAAR and Gear is under MY LEGEND, and this file's OWN tests already assert both.
    ///
    /// A WRONG DIRECTION IS WORSE THAN NO DIRECTION. These pages exist to say what a thing will be
    /// and where the working part of it lives today; a reader who goes and cannot find it concludes
    /// the feature was removed rather than that the sentence was stale.
    ///
    /// # WHAT IS ASSERTED
    ///
    /// Any `HEADING / Row` shape inside an [`UNBUILT`] sentence names a heading that is in `NAV`
    /// and a row that is under it. Read out of the table rather than compared against a list of
    /// literals, so moving a row between headings updates the check rather than defeating it.
    ///
    /// SHOUTED HEADINGS ONLY, which is what makes this cheap and exact: every section name in this
    /// file is upper case (`THE BAZAAR`, `MY LEGEND`, `CHRONICLE`), and no ordinary prose in these
    /// sentences is. So the scan looks for a run of capitals followed by ` / ` and a row name.
    ///
    /// WHAT MUTATION MAKES THIS RED: naming a heading or a row that is not in the table.
    #[test]
    fn an_unbuilt_page_never_points_at_a_room_that_does_not_exist() {
        /* THE SHOUTED HEADING IMMEDIATELY BEFORE A SLASH, AND THE WORDS AFTER IT. */
        let claims = |s: &str| -> Vec<(String, String)> {
            let mut out = Vec::new();
            for (at, _) in s.match_indices(" / ") {
                let head: String = s[..at]
                    .chars()
                    .rev()
                    .take_while(|c| c.is_ascii_uppercase() || *c == ' ')
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                let head = head.trim().to_owned();
                /* Two letters or more, so a stray capital in prose is not a heading. */
                if head.len() < 3 || !head.contains(' ') {
                    continue;
                }
                let rest = &s[at + 3..];
                let row: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == ' ' || *c == '&')
                    .collect();
                out.push((head, row.trim().to_owned()));
            }
            out
        };

        let mut checked = 0;
        for (id, why) in UNBUILT {
            for line in *why {
                for (head, rest) in claims(line) {
                    let Some(section) = NAV.iter().find(|(name, _)| *name == head) else {
                        panic!(
                            "{id:?} sends the reader to a heading called {head:?}, which is not \
                             in NAV: {line}"
                        );
                    };
                    checked += 1;
                    /* THE ROW IS THE FIRST WORDS AFTER THE SLASH, and a sentence continues past
                     * it, so the row name is a PREFIX of what was captured. */
                    assert!(
                        section.1.iter().any(|(row, _)| rest.starts_with(row)),
                        "{id:?} sends the reader to {head} / {rest:?}, and {head} has no such \
                         row. It has: {:?}",
                        section.1.iter().map(|(r, _)| *r).collect::<Vec<_>>()
                    );
                }
            }
        }

        assert!(
            checked >= 3,
            "only {checked} directions were checked, so this is passing by not looking"
        );
    }
}
