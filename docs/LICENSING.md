# Licensing

## 1. The licence and the holder

EQL Grimoire is original work: the Rust workspace under `crates/`, the web bundle under `web/`, and
the documents under `docs/`. It is licensed under the GNU Affero General Public Licence, version 3
(`AGPL-3.0-only`). The full text is at [`../LICENSE`](../LICENSE).

**What the AGPL asks of anyone who runs a modified copy over a network:** section 13 means a hosted,
modified Grimoire has to offer its users the source of that modified version. The app and the web
bundle therefore carry a visible source link, and [`../CONTRIBUTING.md`](../CONTRIBUTING.md) says so
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

**Game data.** Item, quest, zone and wiki data the desktop app reads is third-party content. It
is loaded at runtime from a data directory and never compiled into the binary; the loader is
`crates/grimoire-desktop/src/data/mod.rs`. Its source is eqlwiki.com, and it is attributed to
eqlwiki.

**The terms that data ships under are NOT settled, and this file does not settle them.** Earlier
notes in this repository disagreed: one described the data as CC BY-SA 4.0, another recorded that
eqlwiki publishes no robots.txt and declares no licence at all. Both cannot be true, so what is
recorded here is the open question. Settling it is the data pass's work, and the answer belongs in
this file on the day it exists.
