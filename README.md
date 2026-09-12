# Grimoire

A companion app for the games you play. **This is the EverQuest Legends edition.**

Grimoire reads the log file the game writes on your own PC and turns it into a DPS parser, overlays
you can keep on top of the game, dashboards of your nights, and a record of your character. It also
carries a searchable offline compendium of the game, and the Broken Stoic stream and videos.

**Website and download: [ragnarok.systems/grimoire](https://ragnarok.systems/grimoire)**

---

## Install

1. Download Grimoire from [ragnarok.systems/grimoire](https://ragnarok.systems/grimoire).
2. Put the downloaded `.exe` wherever you want it to live, and run it. There is no installer. Pin it to
   your taskbar or Start menu if you like: that file keeps working through every update.
3. **Windows may warn you** with "Windows protected your PC", because the app is not yet signed with a
   code signing certificate. Click **More info**, then **Run anyway**.

Grimoire runs on 64-bit Windows.

## Set up the game

In EverQuest Legends, type:

```
/log on
```

The game only writes a log while logging is on, and Grimoire can only show what the log records.

Grimoire looks for the logs in the default install folder:

```
C:\Users\Public\Daybreak Game Company\Installed Games\EverQuest Legends\Logs
```

If you installed the game somewhere else, open **Settings** and point **LOG FOLDER** at your game's
`Logs` folder.

For the inventory and gear pages, type `/outputfile inventory` in game whenever you want Grimoire to see
what you are carrying.

## What's in it

The menu on the left is grouped the way the app is:

| Section | What it is |
|---|---|
| **Chronicle** | The **Log Parser**: live DPS, every fight, dashboards of a night, and reports. **GINA** triggers are coming. |
| **My Legend** | Your character: inventory, gear, exaltations, loadouts, plans, hunt and loot journals, Plane of Sky, lockouts |
| **Compendium** | The game itself, offline and searchable: zones, bestiary, items, loot tables, quests, spells, crafting |
| **The Bazaar** | Crafting orders, priced from the game's own combine odds |
| **The Tavern** | Groups, guild and schedule |
| **Broken Stoic** | Watch the stream live, the videos, and chat |

Some pages are still being built. Those say so on the page, and say what they are waiting on.

### Overlays

The DPS **Pill**, **Meter** and **Coach** are small windows that stay on top of the game. Open them from
**Log Parser → Overlays**, drag them where you want them, and they open there next time.

The dot on an overlay tells you where the fight is:

- **Green**: you are fighting.
- **Gold**: the fight just ended, and the encounter is held open for a few seconds in case another mob
  joins.
- **Red**: the encounter is over. The numbers from it stay up until the next fight starts.

## Updates

Grimoire updates itself. When a new version is out it downloads, checks the download's signature, and
installs, with nothing for you to click. If a new version ever fails to start, Grimoire goes back to the
one that worked.

To get new features early, open **Settings → UPDATES** and switch the channel to **beta**. Beta builds
are newer and less tested.

## Your data

Your logs are read where the game writes them, on your PC. Grimoire keeps its own files in:

- `%APPDATA%\eql-grimoire`: your settings and the fights it has saved
- `%LOCALAPPDATA%\eql-grimoire`: the installed app versions
- `%LOCALAPPDATA%\EQLGrimoire`: cached images and the stream player's data

Grimoire goes online to check for updates, to play the Broken Stoic stream and videos, and when you sign
in to Twitch.

**To uninstall**, delete the Grimoire `.exe` you downloaded and those three folders.

## Help and bugs

Found something wrong? [Open an issue](https://github.com/Ragnarok-Systems/eql-grimoire/issues) with:

- what you expected, and what happened
- your Grimoire version (it's in the title bar)
- the log lines where it went wrong, **with other players' names and chat removed**

Wrong DPS numbers count as a serious bug. Please report them.

## Contributing

Grimoire is open source. To build it from source or send a change, read
[CONTRIBUTING.md](CONTRIBUTING.md).

## Licence

GNU Affero General Public Licence v3 (`AGPL-3.0-only`), held by James McMenamin. Full text in
[LICENSE](LICENSE). Contributions are covered by [CLA.md](CLA.md). Third-party material, including the
bundled fonts and the game data the app loads, is listed in [docs/LICENSING.md](docs/LICENSING.md).

EverQuest Legends is a trademark of its owner. Grimoire is a fan-made companion and is not affiliated
with or endorsed by the game's publisher.
