# Grimoire

A companion app for **Monsters & Memories**: watch and chat with the stream, find a group, and, as
it grows, browse the game's community wiki, read what the developers say, and ask questions about
the game.

**Website: [ragnarok.systems/grimoire](https://ragnarok.systems/grimoire)**

---

## Install

1. Download Grimoire from [ragnarok.systems/grimoire](https://ragnarok.systems/grimoire).
2. Put the downloaded `.exe` wherever you want it to live, and run it. There is no installer. Pin it to
   your taskbar or Start menu if you like: that file keeps working through every update.
3. **Windows may warn you** with "Windows protected your PC", because the app is not yet signed with a
   code signing certificate. Click **More info**, then **Run anyway**.

Grimoire runs on 64-bit Windows.

## What's in it

The menu on the left:

| Section | What it is |
|---|---|
| **Compendium** | Zones, NPCs, items, quests and spells from the Monsters & Memories community wiki, offline: search a list, open a record, and follow its links (an item to the NPC that drops it, a zone to its neighbours). Every record credits the wiki and links to its page. |
| **Herald** | Everything the developers say in the game's Discord, kept and searchable, with the question each answer was replying to. It fills once our Discord recorder is running. |
| **Oracle** | Ask a question in plain words and get a short answer written from the wiki, with a link to every source. It runs on your PC, on the processor, and costs nothing to use. A small model writes the answers, so check the sources. |
| **The Tavern** | **Groups**: write the lines you mean to say in game and copy each one ready to paste, as a group or a raid post. |
| **Streams** | Watch the Broken Stoic stream live, the videos, and chat. |

### Hotkeys

Hotkeys work while the game has focus:

| Keys | Opens |
|---|---|
| Ctrl+Alt+G | the whole app, shown or hidden |
| Ctrl+Alt+W | the stream, picture in picture |
| Ctrl+Alt+C | chat, over the game |
| Ctrl+Alt+L | looking for group (then R for a raid) |
| Ctrl+Alt+R | looking for raid |

Change any of them in **Settings → HOTKEYS**.

## Updates

Grimoire updates itself. When a new version is out it downloads, checks the download's signature, and
installs, with nothing for you to click. It waits until no stream is playing, so it never cuts one
off. If a new version ever fails to start, Grimoire goes back to the one that worked.

To get new features early, open **Settings → UPDATES** and switch the channel to **beta**. Beta builds
are newer and less tested.

## Your data

Grimoire keeps its own files in these folders. They still carry the name of the app's first edition,
for EverQuest Legends, on purpose: every installed copy finds its updates and settings there, so the
names cannot change without stranding them.

- `%APPDATA%\eql-grimoire`: your settings
- `%LOCALAPPDATA%\eql-grimoire`: the installed app versions, the wiki data (`data`), Oracle's engine
  and model (`oracle`, about 780 MB once downloaded) and Herald's posts (`herald`)
- `%LOCALAPPDATA%\EQLGrimoire`: cached images and the stream player's data

Grimoire goes online to check for updates, to play the stream and videos, and when you sign in to
Twitch or to a Ragnarok account. The first time you use Oracle it asks before downloading its answer
engine (llama.cpp, from GitHub, 19 MB) and model (LFM2.5 1.2B, from Hugging Face, 731 MB); neither is
part of the app download. After that, questions are answered offline.

**To uninstall**, delete the Grimoire `.exe` you downloaded and those three folders.

## Help and bugs

Found something wrong? [Open an issue](https://github.com/Ragnarok-Systems/eql-grimoire/issues) with:

- what you expected, and what happened
- your Grimoire version (it's in the title bar)
- the steps that led there

## Contributing

Grimoire is open source. To build it from source or send a change, read
[CONTRIBUTING.md](CONTRIBUTING.md).

## Licence

GNU Affero General Public Licence v3 (`AGPL-3.0-only`), held by James McMenamin. Full text in
[LICENSE](LICENSE). Contributions are covered by [CLA.md](CLA.md). Third-party material, including the
bundled fonts and the game data the app will load, is listed in [docs/LICENSING.md](docs/LICENSING.md).

Monsters & Memories is a trademark of Niche Worlds Cult, LLC. Game content and art are © Niche Worlds
Cult, LLC. Grimoire is a fan-made companion and is not affiliated with or endorsed by Niche Worlds
Cult.
