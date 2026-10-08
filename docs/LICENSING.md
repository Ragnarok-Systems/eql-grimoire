# Licensing

## 1. The licence and the holder

Grimoire is original work: the Rust workspace under `crates/` and the documents under `docs/`. It is licensed under the GNU Affero General Public Licence, version 3
(`AGPL-3.0-only`). The full text is at [`../LICENSE`](../LICENSE).

**What the AGPL asks of anyone who runs a modified copy over a network:** section 13 means a hosted,
modified Grimoire has to offer its users the source of that modified version. The app therefore
carries a visible source link, and [`../CONTRIBUTING.md`](../CONTRIBUTING.md) says so
to anyone building on it.

**Contributions** are covered by [`../CLA.md`](../CLA.md): contributors keep their copyright and
grant the holder the right to publish their work under this licence and, if he chooses, under other
terms as well.

**The holder of record is James McMenamin.** The `Copyright (c)` line in `LICENSE` and the
`authors` entry in `Cargo.toml`'s `[workspace.package]` both name him, and no assignment of the
work to any entity exists anywhere in the repository.

**If this is revisited:** switching the holder to Ragnarok Systems, the Delaware corporation, would
mean editing the `Copyright (c) ` line in `LICENSE` and the `authors` entry in `Cargo.toml`'s
`[workspace.package]` to name the corporation, and recording the assignment that makes it true.

## 2. Third-party material in the tree

The AGPL covers the project's own work. Two kinds of material in the tree are not the
project's own, and each keeps its own terms.

**Fonts.** `crates/grimoire-desktop/assets/` bundles three typefaces. What each file's own name
table states:

| File | Copyright notice in the file | Licence URL in the file |
|---|---|---|
| `Cinzel-Variable.ttf` | Copyright 2020 The Cinzel Project Authors (https://github.com/NDISCOVER/Cinzel) | `https://scripts.sil.org/OFL` |
| `IBMPlexSans-Regular.ttf` | Copyright 2019 IBM Corp. All rights reserved. | `http://scripts.sil.org/OFL` |
| `IBMPlexMono-Regular.ttf` | Copyright 2017 IBM Corp. All rights reserved. | `http://scripts.sil.org/OFL` |

**Game data.** The zone, NPC, item, quest and spell data the app loads comes from the
Monsters & Memories community wiki, https://monstersandmemories.miraheze.org, by its contributors,
licensed under **CC BY-SA 4.0** (https://creativecommons.org/licenses/by-sa/4.0/). It is built into a
data bundle outside this repository and delivered at runtime, never compiled into the binary. Every
record keeps the URL of the page it came from, and the app shows that credit wherever the data
appears. The bundle is an adaptation and is shared under the same licence.

Not used, deliberately: monstersandmemories.wiki (a separate fan site whose terms forbid copying it
into another app; it offers a partner API for lookups only), and the official site and other fan
databases whose robots.txt asks AI crawlers to stay away.

**Monsters & Memories** is a trademark of Niche Worlds Cult, LLC, and game content and art are
© Niche Worlds Cult, LLC. Grimoire is not affiliated with or endorsed by them.

**Oracle's model** (LFM2.5 1.2B Instruct Q4_K_M, LFM Open License v1.0, from LiquidAI on Hugging
Face) and its engine (llama.cpp build b11476, Windows CPU, MIT, from ggml-org on GitHub) are
downloaded by the player's app from their own publishers on first use, after the player agrees on
Oracle's consent card, and each is checked against a pinned sha256 (`src/oracle/fetch.rs`).
Neither is redistributed by this project.
